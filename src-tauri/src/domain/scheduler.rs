//! Scheduler engine running inside the Background Host (§4.5).
//! GUI-independent: talks to the outside world through HostCore's backend.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::history::{schedule_run_record, truncate};
use crate::domain::model::{
    Action, MissedPolicy, Notification, ScheduleEntry, ScheduleFile, ScheduleStatus, Trigger,
};
use crate::domain::ops_schedule::{match_stream, render_template};
use crate::domain::repo::{Domain, SCHEDULE_OUTPUT_MAX_BYTES};
use crate::domain::time::{format_instant, now_iso, parse_stored_instant};

pub enum SchedulerMsg {
    Reload,
    RunNow {
        id: String,
        wait: bool,
        respond: Sender<CoreResult<Value>>,
    },
    Stop {
        id: String,
        respond: Sender<CoreResult<Value>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct SchedulerHandle {
    tx: Arc<Mutex<Sender<SchedulerMsg>>>,
}

impl SchedulerHandle {
    pub fn send(&self, msg: SchedulerMsg) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(msg);
        }
    }

    pub fn reload(&self) {
        self.send(SchedulerMsg::Reload);
    }

    pub fn shutdown(&self) {
        self.send(SchedulerMsg::Shutdown);
    }
}

pub struct Scheduler {
    core: Arc<crate::domain::host::HostCore>,
    rx: Receiver<SchedulerMsg>,
    file: ScheduleFile,
    revision: u64,
}

pub fn start(core: Arc<crate::domain::host::HostCore>) -> SchedulerHandle {
    let (tx, rx) = channel::<SchedulerMsg>();
    let handle = SchedulerHandle {
        tx: Arc::new(Mutex::new(tx)),
    };
    let thread_core = core.clone();
    thread::Builder::new()
        .name("kxtodo-scheduler".to_string())
        .spawn(move || {
            let mut scheduler = Scheduler::new(thread_core, rx);
            scheduler.run();
        })
        .expect("spawn scheduler thread");
    handle
}

impl Scheduler {
    fn new(core: Arc<crate::domain::host::HostCore>, rx: Receiver<SchedulerMsg>) -> Self {
        let (file, revision) = core
            .repo
            .load_schedule()
            .map(|file| (file.clone(), file.meta.revision))
            .unwrap_or_default();
        let mut scheduler = Self {
            core,
            rx,
            file,
            revision,
        };
        scheduler.recompute_all_next();
        scheduler
    }

    fn run(&mut self) {
        self.handle_missed_on_start();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(SchedulerMsg::Reload) => self.reload(),
                Ok(SchedulerMsg::RunNow { id, wait, respond }) => {
                    let result = self.run_now(&id, wait);
                    let _ = respond.send(result);
                }
                Ok(SchedulerMsg::Stop { id, respond }) => {
                    let result = self.stop_task(&id);
                    let _ = respond.send(result);
                }
                Ok(SchedulerMsg::Shutdown) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
    }

    fn reload(&mut self) {
        if let Ok(file) = self.core.repo.load_schedule() {
            if file.meta.revision != self.revision {
                self.revision = file.meta.revision;
                self.file = file;
                self.recompute_all_next();
            }
        }
    }

    fn reload_forced(&mut self) {
        if let Ok(file) = self.core.repo.load_schedule() {
            self.revision = file.meta.revision;
            self.file = file;
        }
    }

    /// Recompute nextRunAt for every enabled entry (after reload/migration).
    fn recompute_all_next(&mut self) {
        let mut changed = false;
        let now = Utc::now();
        for entry in self.file.tasks.iter_mut() {
            if !entry.spec.enabled {
                if entry.state.next_run_at.is_some() {
                    entry.state.next_run_at = None;
                    changed = true;
                }
                continue;
            }
            match crate::domain::plan::compute_next_run(entry, now) {
                Ok(next) => {
                    let next = next.map(format_instant);
                    if next != entry.state.next_run_at {
                        entry.state.next_run_at = next;
                        changed = true;
                    }
                }
                Err(_) => {
                    entry.spec.enabled = false;
                    changed = true;
                }
            }
        }
        if changed {
            self.persist("schedule.recompute", |file| {
                for entry in file.tasks.iter_mut() {
                    if let Some(fresh) = self.file.tasks.iter().find(|item| item.id == entry.id) {
                        entry.state.next_run_at = fresh.state.next_run_at.clone();
                        entry.spec.enabled = fresh.spec.enabled;
                    }
                }
            });
            self.reload_forced();
        }
    }

    fn persist<F>(&self, command: &str, apply: F)
    where
        F: FnOnce(&mut ScheduleFile),
    {
        let _ = self.core.repo.write_schedule(None, None, command, |file| {
            apply(file);
            Ok(json!({ "command": command }))
        });
    }

    /// On host start: apply missedPolicy to overdue enabled schedules (§4.5).
    fn handle_missed_on_start(&mut self) {
        let now = Utc::now();
        let overdue: Vec<String> = self
            .file
            .tasks
            .iter()
            .filter(|entry| entry.spec.enabled)
            .filter(|entry| {
                entry
                    .state
                    .next_run_at
                    .as_deref()
                    .and_then(|value| parse_stored_instant(value).ok())
                    .map(|at| at < now)
                    .unwrap_or(false)
            })
            .map(|entry| entry.id.clone())
            .collect();
        for id in overdue {
            let Some(entry) = self.file.tasks.iter().find(|entry| entry.id == id).cloned() else {
                continue;
            };
            match entry.spec.trigger.effective_missed_policy() {
                MissedPolicy::Skip => {
                    let _ = self.update_entry_state(&id, "schedule.missed-skip", |state| {
                        state.missed_count += 1;
                        state.last_missed_at = Some(now_iso());
                        Ok(())
                    });
                    self.recompute_next_for(&id);
                }
                MissedPolicy::RunOnce => {
                    let _ = self.update_entry_state(&id, "schedule.missed-run", |state| {
                        state.missed_count += 1;
                        state.last_missed_at = Some(now_iso());
                        Ok(())
                    });
                    let _ = self.execute_entry(&id, RunReason::Missed);
                }
            }
        }
    }

    fn tick(&mut self) {
        let now = Utc::now();
        let due: Vec<String> = self
            .file
            .tasks
            .iter()
            .filter(|entry| entry.spec.enabled && !entry.state.running)
            .filter(|entry| {
                entry
                    .state
                    .next_run_at
                    .as_deref()
                    .and_then(|value| parse_stored_instant(value).ok())
                    .map(|at| at <= now)
                    .unwrap_or(false)
            })
            .map(|entry| entry.id.clone())
            .collect();
        for id in due {
            let _ = self.execute_entry(&id, RunReason::Scheduled);
        }
    }

    fn recompute_next_for(&mut self, id: &str) {
        let now = Utc::now();
        let mut next = None;
        if let Some(entry) = self.file.tasks.iter().find(|entry| entry.id == id) {
            next = crate::domain::plan::compute_next_run(entry, now)
                .ok()
                .flatten()
                .map(format_instant);
        }
        let next_value = next;
        let _ = self.update_entry_state(id, "schedule.replan", move |state| {
            state.next_run_at = next_value.clone();
            Ok(())
        });
        self.reload_forced();
    }

    fn update_entry_state<F>(
        &mut self,
        id: &str,
        command: &str,
        apply: F,
    ) -> CoreResult<()>
    where
        F: FnOnce(&mut crate::domain::model::ScheduleState) -> CoreResult<()>,
    {
        let (_file, outcome) = self.core.repo.write_schedule(None, None, command, |file| {
            let entry = file
                .tasks
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}")))?;
            apply(&mut entry.state)?;
            entry.updated_at = now_iso();
            Ok(json!({ "id": id }))
        })?;
        self.revision = outcome.revision;
        self.reload_forced();
        self.core.emit_domain_event(Domain::Schedule, outcome.revision, vec![id.to_string()]);
        Ok(())
    }

    fn update_entry_full<F>(&mut self, id: &str, command: &str, apply: F) -> CoreResult<()>
    where
        F: FnOnce(&mut ScheduleEntry) -> CoreResult<()>,
    {
        let (_file, outcome) = self.core.repo.write_schedule(None, None, command, |file| {
            let entry = file
                .tasks
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}")))?;
            apply(entry)?;
            entry.updated_at = now_iso();
            Ok(json!({ "id": id }))
        })?;
        self.revision = outcome.revision;
        self.reload_forced();
        self.core.emit_domain_event(Domain::Schedule, outcome.revision, vec![id.to_string()]);
        Ok(())
    }

    fn stop_task(&mut self, id: &str) -> CoreResult<Value> {
        let stopped = self.core.processes.stop(id);
        if stopped {
            self.update_entry_full(id, "schedule.stop", |entry| {
                entry.state.running = false;
                entry.state.last_status = ScheduleStatus::Stopped;
                Ok(())
            })?;
            self.reload_forced();
            return Ok(json!({ "id": id, "stopped": true }));
        }
        Ok(json!({ "id": id, "stopped": false, "note": "任务未在运行" }))
    }

    fn run_now(&mut self, id: &str, wait: bool) -> CoreResult<Value> {
        if self.file.tasks.iter().all(|entry| entry.id != id) {
            return Err(CoreError::not_found(
                "SCHEDULE_NOT_FOUND",
                format!("未找到定时任务 {id}"),
            ));
        }
        if !wait {
            let _ = self.execute_entry(id, RunReason::Manual);
            return Ok(json!({ "id": id, "queued": true }));
        }
        self.execute_entry(id, RunReason::Manual)
    }

    /// Execute one entry's main action (or probe+main for condition triggers).
    fn execute_entry(&mut self, id: &str, reason: RunReason) -> CoreResult<Value> {
        let Some(entry) = self.file.tasks.iter().find(|entry| entry.id == id).cloned() else {
            return Err(CoreError::not_found(
                "SCHEDULE_NOT_FOUND",
                format!("未找到定时任务 {id}"),
            ));
        };
        if entry.state.running {
            return Ok(json!({ "id": id, "skipped": "already-running" }));
        }
        self.update_entry_full(id, "schedule.mark-running", |entry| {
            entry.state.running = true;
            entry.state.last_status = ScheduleStatus::Running;
            Ok(())
        })?;
        self.reload_forced();

        let started = now_iso();
        let result = self.execute_action_flow(&entry, &reason);
        let finished = now_iso();

        // condition probe 未命中不是主任务失败：不重算状态，仅安排下一次探测（§3.5.2）。
        if let Err(error) = &result {
            if error.code == "PROBE_NO_MATCH" {
                self.update_entry_full(id, "schedule.probe-miss", |entry| {
                    entry.state.running = false;
                    Ok(())
                })?;
                self.recompute_next_for(id);
                return Ok(json!({ "id": id, "probe": "no-match" }));
            }
        }

        let output = match &result {
            Ok(output) => output.clone(),
            Err(error) => crate::domain::exec::ExecOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: error.message.clone(),
                timed_out: false,
                cancelled: false,
            },
        };
        let (stdout_summary, _) = truncate(&output.stdout, SCHEDULE_OUTPUT_MAX_BYTES);
        let (stderr_summary, _) = truncate(&output.stderr, SCHEDULE_OUTPUT_MAX_BYTES);

        // stopWhen / maxRuns / once disable semantics (scheduled runs only).
        let manual = matches!(reason, RunReason::Manual);
        let mut disable = false;
        let mut stopped_by_rule = false;
        let mut stop_reason: Option<String> = None;
        if !manual {
            match &entry.spec.trigger {
                Trigger::Once { .. } => {
                    disable = true;
                    stop_reason = Some("once 已执行".to_string());
                }
                Trigger::Interval {
                    max_runs,
                    stop_when,
                    ..
                } => {
                    if let Some(matcher) = stop_when {
                        if match_stream(matcher, &output.stdout, &output.stderr) {
                            disable = true;
                            stopped_by_rule = true;
                            stop_reason = Some("stopWhen 匹配，任务已停止".to_string());
                        }
                    }
                    if !disable {
                        if let Some(max) = max_runs {
                            if entry.state.run_count + 1 >= *max {
                                disable = true;
                                stopped_by_rule = true;
                                stop_reason = Some("已达到 maxRuns".to_string());
                            }
                        }
                    }
                }
                Trigger::Condition { .. } => {
                    if result.is_ok() {
                        disable = true;
                        stop_reason = Some("condition 命中并已执行".to_string());
                    }
                }
                Trigger::Calendar { .. } => {}
            }
        }
        let status = if result.is_err() && !output.cancelled {
            ScheduleStatus::Failed
        } else if output.cancelled {
            ScheduleStatus::Stopped
        } else if stopped_by_rule {
            ScheduleStatus::Stopped
        } else if output.exit_code == Some(0) || matches!(entry.spec.action, Action::Notification { .. }) {
            ScheduleStatus::Success
        } else {
            ScheduleStatus::Failed
        };
        if status == ScheduleStatus::Stopped && stop_reason.is_none() {
            stop_reason = Some("被用户终止".to_string());
        }

        let run_count_delta = if manual { 0 } else { 1 };
        let status_clone = status;
        let next_run_count = entry.state.run_count + run_count_delta;
        self.update_entry_full(id, "schedule.record-run", |entry| {
            entry.state.running = false;
            entry.state.run_count = next_run_count;
            entry.state.last_run_at = Some(finished.clone());
            entry.state.last_status = status_clone;
            entry.state.last_exit_code = output.exit_code;
            entry.state.last_stdout = Some(stdout_summary.clone());
            entry.state.last_stderr = Some(stderr_summary.clone());
            if disable {
                entry.spec.enabled = false;
                entry.state.next_run_at = None;
            }
            Ok(())
        })?;
        if !disable {
            self.recompute_next_for(id);
        } else {
            self.reload_forced();
        }

        // History.
        let kind = match reason {
            RunReason::Scheduled => "scheduled",
            RunReason::Missed => "missed",
            RunReason::Manual => "manual",
        };
        let scheduled_at = entry.state.next_run_at.clone();
        let record = schedule_run_record(
            id,
            scheduled_at.as_deref(),
            &started,
            &finished,
            kind,
            status.as_str(),
            output.exit_code,
            &output.stdout,
            &output.stderr,
            stop_reason.as_deref(),
            entry.state.missed_count,
        );
        let _ = crate::domain::history::append_bounded_jsonl(
            &self.core.repo.layout.schedule_history(),
            &record,
            crate::domain::repo::SCHEDULE_HISTORY_MAX_BYTES,
            Some(crate::domain::repo::SCHEDULE_HISTORY_PER_TASK),
        );

        // Action notifications.
        if result.is_ok() && !output.cancelled {
            self.dispatch_action_notifications(&entry, &output);
        }

        if let Err(error) = &result {
            return Err(error.clone());
        }
        Ok(json!({
            "id": id,
            "status": status.as_str(),
            "exitCode": output.exit_code,
            "stdout": stdout_summary,
            "stderr": stderr_summary,
            "disabled": disable,
        }))
    }

    fn execute_action_flow(
        &mut self,
        entry: &ScheduleEntry,
        _reason: &RunReason,
    ) -> CoreResult<crate::domain::exec::ExecOutput> {
        // Condition trigger: probe first; only a match runs the main action.
        if let Trigger::Condition { probe, when, .. } = &entry.spec.trigger {
            let probe_spec = crate::domain::exec::build_probe_spec(probe, &self.file.runtimes)?;
            let probe_output = self
                .core
                .processes
                .run(&format!("{}:probe", entry.id), probe_spec)?;
            let probe_failed = probe_output.timed_out || probe_output.exit_code != Some(0);
            let matched = !probe_failed && match_stream(when, &probe_output.stdout, &probe_output.stderr);
            let (probe_stdout, _) = truncate(&probe_output.stdout, SCHEDULE_OUTPUT_MAX_BYTES);
            let (probe_stderr, _) = truncate(&probe_output.stderr, SCHEDULE_OUTPUT_MAX_BYTES);
            self.update_entry_full(&entry.id, "schedule.record-probe", |entry| {
                entry.state.last_probe = Some(crate::domain::model::ProbeState {
                    at: now_iso(),
                    status: if matched {
                        ScheduleStatus::Success
                    } else {
                        ScheduleStatus::Failed
                    },
                    exit_code: probe_output.exit_code,
                    stdout: Some(probe_stdout.clone()),
                    stderr: Some(probe_stderr.clone()),
                });
                Ok(())
            })?;
            if !matched {
                // Probe failure/no-match: main action does not run (§3.5.2);
                // an independent probe history record is kept.
                let record = schedule_run_record(
                    &entry.id,
                    None,
                    &now_iso(),
                    &now_iso(),
                    "probe",
                    "failed",
                    probe_output.exit_code,
                    &probe_output.stdout,
                    &probe_output.stderr,
                    Some("probe 未命中"),
                    0,
                );
                let _ = crate::domain::history::append_bounded_jsonl(
                    &self.core.repo.layout.schedule_history(),
                    &record,
                    crate::domain::repo::SCHEDULE_HISTORY_MAX_BYTES,
                    Some(crate::domain::repo::SCHEDULE_HISTORY_PER_TASK),
                );
                return Err(CoreError::execution(
                    "PROBE_NO_MATCH",
                    "condition probe 未命中，本次不执行主动作",
                ));
            }
        }
        self.execute_main_action(entry)
    }

    fn execute_main_action(
        &mut self,
        entry: &ScheduleEntry,
    ) -> CoreResult<crate::domain::exec::ExecOutput> {
        match &entry.spec.action {
            Action::Notification { notification } => {
                self.core.show_notification_rendered(notification, &entry.spec.name, &[])?;
                Ok(crate::domain::exec::ExecOutput {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out: false,
                    cancelled: false,
                })
            }
            _ => {
                let spec = crate::domain::exec::build_action_spec(&entry.spec.action, &self.file.runtimes)?
                    .expect("non-notification action has a spec");
                self.core.processes.run(&entry.id, spec)
            }
        }
    }

    fn dispatch_action_notifications(
        &mut self,
        entry: &ScheduleEntry,
        output: &crate::domain::exec::ExecOutput,
    ) {
        let Some(notifications) = entry.spec.action.notifications() else {
            return;
        };
        let exit_code = output
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_default();
        let vars: [(&str, &str); 4] = [
            ("taskName", &entry.spec.name),
            ("stdout", &output.stdout),
            ("stderr", &output.stderr),
            ("exitCode", &exit_code),
        ];
        if let Some(on_complete) = &notifications.on_complete {
            let _ = self
                .core
                .show_notification_rendered(on_complete, &entry.spec.name, &vars);
        }
        if let Some(on_output) = &notifications.on_output {
            if match_stream(&on_output.when, &output.stdout, &output.stderr) {
                let _ = self
                    .core
                    .show_notification_rendered(&on_output.notification, &entry.spec.name, &vars);
            }
        }
    }
}

enum RunReason {
    Scheduled,
    Missed,
    Manual,
}

/// Render a notification template with the allowed variables (§3.5.2).
pub fn render_notification(notification: &Notification, task_name: &str, extra: &[(&str, &str)]) -> Notification {
    let mut vars: Vec<(&str, &str)> = vec![("taskName", task_name)];
    vars.extend_from_slice(extra);
    Notification {
        title: notification
            .title
            .as_ref()
            .map(|title| render_template(title, &vars)),
        message: render_template(&notification.message, &vars),
        duration: notification.duration.clone(),
        tone: notification.tone,
        position: notification.position,
        extra: notification.extra.clone(),
    }
}


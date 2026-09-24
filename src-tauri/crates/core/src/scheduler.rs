//! Asynchronous scheduler engine owned by the Background Host (§4.5).
//! The control thread only plans and routes messages; each accepted schedule
//! runs in an independent worker guarded by a per-id run slot.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{json, Value};

use crate::error::{CoreError, CoreResult};
use crate::history::{schedule_run_record, truncate};
use crate::model::{
    Action, MissedPolicy, Notification, ProbeState, ScheduleEntry, ScheduleStatus, Trigger,
};
use crate::ops_schedule::{match_stream, render_template};
use crate::repo::{Domain, SCHEDULE_OUTPUT_MAX_BYTES};
use crate::time::{format_instant, now_iso, parse_stored_instant};

pub enum SchedulerMsg {
    Reload,
    RunNow {
        id: String,
        wait: bool,
        respond: Sender<CoreResult<Value>>,
    },
    Shutdown,
}

#[derive(Default)]
struct ActiveRuns {
    slots: Mutex<HashMap<String, Arc<AtomicBool>>>,
    changed: Condvar,
    accepting: AtomicBool,
}

impl ActiveRuns {
    fn start_accepting(&self) {
        self.accepting.store(true, Ordering::SeqCst);
    }

    fn reserve(&self, id: &str) -> Option<Arc<AtomicBool>> {
        let mut slots = self.slots.lock().ok()?;
        if !self.accepting.load(Ordering::SeqCst) || slots.contains_key(id) {
            return None;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        slots.insert(id.to_string(), cancelled.clone());
        Some(cancelled)
    }

    fn finish(&self, id: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.remove(id);
            self.changed.notify_all();
        }
    }

    fn cancel(&self, id: &str) -> bool {
        let slot = self
            .slots
            .lock()
            .ok()
            .and_then(|slots| slots.get(id).cloned());
        if let Some(slot) = slot {
            slot.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn cancel_all(&self) -> Vec<String> {
        self.accepting.store(false, Ordering::SeqCst);
        let slots: Vec<(String, Arc<AtomicBool>)> = self
            .slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .map(|(id, flag)| (id.clone(), flag.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (_, flag) in &slots {
            flag.store(true, Ordering::SeqCst);
        }
        slots.into_iter().map(|(id, _)| id).collect()
    }

    fn contains(&self, id: &str) -> bool {
        self.slots
            .lock()
            .map(|slots| slots.contains_key(id))
            .unwrap_or(false)
    }

    fn ids(&self) -> Vec<String> {
        self.slots
            .lock()
            .map(|slots| slots.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn wait_for(&self, id: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut slots = match self.slots.lock() {
            Ok(slots) => slots,
            Err(_) => return false,
        };
        while slots.contains_key(id) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match self.changed.wait_timeout(slots, remaining) {
                Ok((next, result)) => {
                    slots = next;
                    if result.timed_out() && slots.contains_key(id) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }

    fn wait_all(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut slots = match self.slots.lock() {
            Ok(slots) => slots,
            Err(_) => return false,
        };
        while !slots.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match self.changed.wait_timeout(slots, remaining) {
                Ok((next, result)) => {
                    slots = next;
                    if result.timed_out() && !slots.is_empty() {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }
}

#[derive(Clone)]
pub struct SchedulerHandle {
    tx: Sender<SchedulerMsg>,
    core: Arc<crate::host::HostCore>,
    active: Arc<ActiveRuns>,
    control: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// 还有等着触发的任务提醒吗（看门狗据此决定能不能自动退出）。
    /// 用标志位而不是让看门狗自己 `plan()`：看门狗每 2 秒跑一次，
    /// 为它每 2 秒解析一遍 data.json 不值当。
    reminder_work: Arc<AtomicBool>,
}

impl SchedulerHandle {
    pub fn send(&self, msg: SchedulerMsg) {
        let _ = self.tx.send(msg);
    }

    pub fn reload(&self) {
        self.send(SchedulerMsg::Reload);
    }

    pub fn run_now(&self, id: &str, wait: bool) -> CoreResult<Value> {
        let (tx, rx) = channel();
        self.send(SchedulerMsg::RunNow {
            id: id.to_string(),
            wait,
            respond: tx,
        });
        rx.recv_timeout(if wait {
            Duration::from_secs(3600)
        } else {
            Duration::from_secs(5)
        })
        .map_err(|error| {
            CoreError::execution("RUN_TIMEOUT", format!("等待调度器响应超时：{error}"))
        })?
    }

    /// Cancellation bypasses the control queue so a long task cannot block it.
    pub fn stop(&self, id: &str) -> CoreResult<Value> {
        let active = self.active.cancel(id);
        let main = self.core.processes.stop(id);
        let probe = self.core.processes.stop(&format!("{id}:probe"));
        if active && !self.active.wait_for(id, Duration::from_secs(15)) {
            return Err(CoreError::execution(
                "STOP_TIMEOUT",
                format!("等待任务 {id} 回收超时"),
            ));
        }
        Ok(json!({
            "id": id,
            "stopped": active || main || probe,
            "active": self.active.contains(id),
        }))
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.active.ids()
    }

    /// 是否还有未触发的任务提醒（`reminders::Engine::has_pending` 的快照）。
    pub fn has_reminder_work(&self) -> bool {
        self.reminder_work.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        let ids = self.active.cancel_all();
        let _ = self.core.processes.stop_all();
        let _ = self.tx.send(SchedulerMsg::Shutdown);
        let _ = self.active.wait_all(Duration::from_secs(30));
        if let Ok(mut control) = self.control.lock() {
            if let Some(handle) = control.take() {
                let _ = handle.join();
            }
        }
        if !ids.is_empty() {
            let _ = reconcile_stale_running(&self.core, "host-shutdown/interrupted");
        }
    }
}

pub struct Scheduler {
    core: Arc<crate::host::HostCore>,
    rx: Receiver<SchedulerMsg>,
    active: Arc<ActiveRuns>,
    reminder_work: Arc<AtomicBool>,
}

pub fn start(core: Arc<crate::host::HostCore>) -> SchedulerHandle {
    let (tx, rx) = channel::<SchedulerMsg>();
    let active = Arc::new(ActiveRuns::default());
    active.start_accepting();
    let control = Arc::new(Mutex::new(None));
    let reminder_work = Arc::new(AtomicBool::new(false));
    let handle = SchedulerHandle {
        tx: tx.clone(),
        core: core.clone(),
        active: active.clone(),
        control: control.clone(),
        reminder_work: reminder_work.clone(),
    };
    let thread_handle = thread::Builder::new()
        .name("kxtodo-scheduler".to_string())
        .spawn(move || {
            let mut scheduler = Scheduler {
                core,
                rx,
                active,
                reminder_work,
            };
            scheduler.run();
        })
        .expect("spawn scheduler thread");
    if let Ok(mut slot) = control.lock() {
        *slot = Some(thread_handle);
    }
    handle
}

impl Scheduler {
    fn run(&mut self) {
        if reconcile_stale_running(&self.core, "host-crash/interrupted").is_ok() {
            let _ = expire_schedules(&self.core);
            self.handle_missed_on_start();
            self.recompute_all_next();
        }
        // 任务提醒跟着调度线程跑：它不是定时任务（不建 ScheduleEntry、不进 tasks.json），
        // 但「每 500ms 醒一次看看到点了没」这套节拍是现成的，没理由再开一条线程。
        let mut reminders = crate::reminders::Engine::default();
        let mut previous_wall = Utc::now();
        let mut previous_instant = Instant::now();
        // 首轮一律当作时钟跳变：刚启动时看见的「已到点」全是停机期间错过的，不补发。
        let mut first = true;
        let mut last_error: Option<String> = None;
        loop {
            let now = Utc::now();
            let reset = first
                || crate::reminders::clock_discontinuous(
                    previous_wall,
                    now,
                    previous_instant.elapsed(),
                );
            first = false;
            // 轮询期间不许看门狗判定「没活可干」（它 2 秒问一次）
            self.reminder_work.store(true, Ordering::SeqCst);
            match reminders.poll(&self.core, now, reset) {
                Ok(()) => last_error = None,
                Err(error) => {
                    // 同一条错误只报一次：台账损坏这类问题会每 500ms 复现一次，
                    // 逐轮上报等于把前端的事件通道刷爆。
                    if last_error.as_deref() != Some(error.message.as_str()) {
                        self.emit_reminder_error(&error);
                        last_error = Some(error.message);
                    }
                }
            }
            self.reminder_work
                .store(reminders.has_pending(), Ordering::SeqCst);
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(SchedulerMsg::Reload) => {}
                Ok(SchedulerMsg::RunNow { id, wait, respond }) => {
                    self.spawn_worker(id, RunReason::Manual, wait, respond);
                }
                Ok(SchedulerMsg::Shutdown) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
            previous_wall = Utc::now();
            previous_instant = Instant::now();
        }
        self.reminder_work.store(false, Ordering::SeqCst);
    }

    /// 提醒引擎的故障报给前端（台账损坏、通知发不出去）。走事件而不是日志：
    /// 这类问题只有用户能修（比如去系统设置里开通知权限），写进日志没人看得见。
    fn emit_reminder_error(&self, error: &CoreError) {
        let Ok(backend) = self.core.backend.read() else {
            return;
        };
        if let Some(backend) = backend.as_ref() {
            backend.emit(
                "kxtodo://reminder-error",
                json!({ "code": error.code, "message": error.message }),
            );
        }
    }

    fn spawn_worker(
        &self,
        id: String,
        reason: RunReason,
        wait: bool,
        respond: Sender<CoreResult<Value>>,
    ) {
        let exists = self
            .core
            .repo
            .load_schedule()
            .map(|file| file.tasks.iter().any(|entry| entry.id == id))
            .unwrap_or(false);
        if !exists {
            let _ = respond.send(Err(CoreError::not_found(
                "SCHEDULE_NOT_FOUND",
                format!("未找到定时任务 {id}"),
            )));
            return;
        }
        let Some(cancelled) = self.active.reserve(&id) else {
            let _ = respond.send(Ok(json!({ "id": id, "skipped": "already-running" })));
            return;
        };
        let core = self.core.clone();
        let active = self.active.clone();
        let worker_id = id.clone();
        let immediate_respond = respond.clone();
        let worker = thread::Builder::new()
            .name(format!("kxtodo-run-{id}"))
            .spawn(move || {
                let result = execute_entry(&core, &worker_id, reason, cancelled);
                if wait {
                    let _ = respond.send(result);
                }
                active.finish(&worker_id);
            });
        if let Err(error) = worker {
            self.active.finish(&id);
            let _ = immediate_respond.send(Err(CoreError::internal(format!(
                "无法创建执行 worker：{error}"
            ))));
            return;
        }
        if !wait {
            let _ = immediate_respond.send(Ok(json!({ "id": id, "queued": true })));
        }
    }

    fn tick(&self) {
        // Expiry is independent of nextRunAt (including absent or missed timers).
        if expire_schedules(&self.core).is_err() { return; }
        let Ok(file) = self.core.repo.load_schedule() else {
            return;
        };
        let now = Utc::now();
        let due: Vec<String> = file
            .tasks
            .iter()
            .filter(|entry| entry.spec.enabled && !entry.state.running)
            .filter(|entry| !self.active.contains(&entry.id))
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
            let (tx, _rx) = channel();
            self.spawn_worker(id, RunReason::Scheduled, false, tx);
        }
    }

    /// Inspect persisted nextRunAt before any startup replanning.
    fn handle_missed_on_start(&self) {
        let Ok(file) = self.core.repo.load_schedule() else {
            return;
        };
        let now = Utc::now();
        let overdue: Vec<ScheduleEntry> = file
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
            .cloned()
            .collect();
        for entry in overdue {
            match entry.spec.trigger.effective_missed_policy() {
                MissedPolicy::Skip => {
                    let id = entry.id.clone();
                    let result = self.core.repo.write_schedule_internal(
                        "scheduler.missed-skip",
                        move |file| {
                            let entry = file
                                .tasks
                                .iter_mut()
                                .find(|entry| entry.id == id)
                                .ok_or_else(|| {
                                    CoreError::not_found(
                                        "SCHEDULE_NOT_FOUND",
                                        format!("未找到定时任务 {id}"),
                                    )
                                })?;
                            entry.state.missed_count += 1;
                            entry.state.last_missed_at = Some(now_iso());
                            if matches!(entry.spec.trigger, Trigger::Once { .. }) {
                                entry.spec.enabled = false;
                                entry.state.next_run_at = None;
                                entry.state.last_status = ScheduleStatus::Stopped;
                            } else {
                                apply_next_plan(entry);
                            }
                            Ok(json!({ "id": id }))
                        },
                    );
                    if let Ok((_file, outcome)) = result {
                        self.core.emit_domain_event(
                            Domain::Schedule,
                            outcome.revision,
                            vec![entry.id],
                        );
                    }
                }
                MissedPolicy::RunOnce => {
                    let (tx, _rx) = channel();
                    self.spawn_worker(entry.id, RunReason::Missed, false, tx);
                }
            }
        }
    }

    fn recompute_all_next(&self) {
        let Ok(snapshot) = self.core.repo.load_schedule() else {
            return;
        };
        let active = self.active.ids();
        let mut updates: Vec<(String, Option<String>, Option<Value>)> = Vec::new();
        for entry in &snapshot.tasks {
            if active.iter().any(|id| id == &entry.id) {
                continue;
            }
            let (next, diagnostic) = planned_next(entry);
            let current_diag = entry.state.extra.get("schedulerDiagnostic").cloned();
            if next != entry.state.next_run_at || diagnostic != current_diag {
                updates.push((entry.id.clone(), next, diagnostic));
            }
        }
        if updates.is_empty() {
            return;
        }
        let result = self
            .core
            .repo
            .write_schedule_internal("scheduler.recompute", move |file| {
                for (id, next, diagnostic) in &updates {
                    if let Some(entry) = file.tasks.iter_mut().find(|entry| &entry.id == id) {
                        entry.state.next_run_at = next.clone();
                        if let Some(diagnostic) = diagnostic {
                            entry
                                .state
                                .extra
                                .insert("schedulerDiagnostic".to_string(), diagnostic.clone());
                        } else {
                            entry.state.extra.remove("schedulerDiagnostic");
                        }
                    }
                }
                Ok(json!({ "count": updates.len() }))
            });
        if let Ok((_file, outcome)) = result {
            self.core
                .emit_domain_event(Domain::Schedule, outcome.revision, Vec::new());
        }
    }
}

fn expire_schedules(core: &Arc<crate::host::HostCore>) -> CoreResult<()> {
    let now = Utc::now();
    let snapshot = core.repo.load_schedule()?;
    let ids: Vec<String> = snapshot.tasks.iter()
        .filter(|entry| entry.spec.enabled && crate::plan::is_expired(&entry.spec, now).unwrap_or(false))
        .map(|entry| entry.id.clone()).collect();
    if ids.is_empty() { return Ok(()); }
    let (_file, outcome) = core.repo.write_schedule_internal("scheduler.expire", |file| {
        for entry in &mut file.tasks {
            if ids.contains(&entry.id) && crate::plan::is_expired(&entry.spec, now)? {
                entry.spec.enabled = false;
                entry.state.next_run_at = None;
                entry.updated_at = format_instant(now);
            }
        }
        Ok(json!({ "ids": ids }))
    })?;
    core.emit_domain_event(Domain::Schedule, outcome.revision, ids);
    Ok(())
}

fn planned_next(entry: &ScheduleEntry) -> (Option<String>, Option<Value>) {
    if !entry.spec.enabled {
        return (None, None);
    }
    match crate::plan::compute_next_run(entry, Utc::now()) {
        Ok(next) => (next.map(format_instant), None),
        Err(error) => (
            None,
            Some(json!({
                "code": error.code,
                "message": error.message,
                "at": now_iso(),
                "retryable": false,
            })),
        ),
    }
}

fn apply_next_plan(entry: &mut ScheduleEntry) {
    let (next, diagnostic) = planned_next(entry);
    entry.state.next_run_at = next;
    if let Some(diagnostic) = diagnostic {
        entry
            .state
            .extra
            .insert("schedulerDiagnostic".to_string(), diagnostic);
    } else {
        entry.state.extra.remove("schedulerDiagnostic");
    }
}

fn reconcile_stale_running(
    core: &Arc<crate::host::HostCore>,
    reason: &str,
) -> CoreResult<()> {
    let file = core.repo.load_schedule()?;
    let stale: Vec<String> = file
        .tasks
        .iter()
        .filter(|entry| entry.state.running)
        .map(|entry| entry.id.clone())
        .collect();
    if stale.is_empty() {
        return Ok(());
    }
    let reason_owned = reason.to_string();
    let stale_for_write = stale.clone();
    let (_file, outcome) =
        core.repo
            .write_schedule_internal("scheduler.reconcile", move |file| {
                for id in &stale_for_write {
                    if let Some(entry) = file.tasks.iter_mut().find(|entry| &entry.id == id) {
                        entry.state.running = false;
                        entry.state.last_status = ScheduleStatus::Stopped;
                        entry.state.last_stderr = Some(reason_owned.clone());
                        entry.updated_at = now_iso();
                    }
                }
                Ok(json!({ "ids": stale_for_write }))
            })?;
    core.emit_domain_event(Domain::Schedule, outcome.revision, stale.clone());
    let now = now_iso();
    for id in stale {
        let record = schedule_run_record(
            &id,
            None,
            &now,
            &now,
            "recovery",
            "stopped",
            None,
            "",
            reason,
            false,
            false,
            Some(reason),
            0,
        );
        let _ = crate::history::append_bounded_jsonl(
            &core.repo.layout.schedule_history(),
            &record,
            crate::repo::SCHEDULE_HISTORY_MAX_BYTES,
            Some(crate::repo::SCHEDULE_HISTORY_PER_TASK),
        );
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RunReason {
    Scheduled,
    Missed,
    Manual,
}

struct FlowResult {
    result: CoreResult<crate::exec::ExecOutput>,
    probe: Option<ProbeState>,
    probe_no_match: bool,
    main_action_started: bool,
}

fn execute_entry(
    core: &Arc<crate::host::HostCore>,
    id: &str,
    reason: RunReason,
    cancelled: Arc<AtomicBool>,
) -> CoreResult<Value> {
    let manual = matches!(reason, RunReason::Manual);
    let snapshot = core.repo.load_schedule()?;
    let before = crate::ops_schedule::require_entry(&snapshot, id)?.clone();
    crate::ops_schedule::ensure_platform_supported(&before.spec)?;
    if !manual {
        if !before.spec.enabled || crate::plan::is_expired(&before.spec, Utc::now())? {
            expire_schedules(core)?;
            return Ok(json!({ "id": id, "skipped": "disabled-or-expired" }));
        }
        // Reserve the worker slot, but do not publish Running while a gate is probing.
        let mut allowed = crate::plan::gate_window_allows(&before.spec, Utc::now())?;
        if allowed {
            if let Some(gate) = &before.spec.gate {
                if let (Some(probe), Some(when)) = (&gate.probe, &gate.when) {
                    let result = crate::exec::build_probe_spec(probe, &snapshot.runtimes)
                        .and_then(|spec| core.processes.run_with_cancel(
                            &format!("{id}:probe"), spec, cancelled.clone()));
                    allowed = result.map(|output| !output.timed_out && !output.cancelled
                        && output.exit_code == Some(0)
                        && match_stream(when, &output.stdout, &output.stderr)).unwrap_or(false);
                }
            }
        }
        if cancelled.load(Ordering::SeqCst) { allowed = false; }
        // Re-check after a slow probe: edits, disables, window end and midnight win.
        let latest = core.repo.load_schedule()?;
        let current = crate::ops_schedule::require_entry(&latest, id)?;
        if serde_json::to_value(&current.spec)? != serde_json::to_value(&before.spec)? {
            return Ok(json!({ "id": id, "skipped": "changed-during-gate" }));
        }
        if crate::plan::is_expired(&current.spec, Utc::now())? {
            expire_schedules(core)?;
            return Ok(json!({ "id": id, "skipped": "expired" }));
        }
        allowed = allowed && crate::plan::gate_window_allows(&current.spec, Utc::now())?;
        if !allowed {
            let (_file, outcome) = core.repo.write_schedule_internal("scheduler.gate-skip", |file| {
                if let Some(entry) = file.tasks.iter_mut().find(|entry| entry.id == id) {
                    if serde_json::to_value(&entry.spec)? == serde_json::to_value(&before.spec)? {
                        crate::plan::advance_skipped_gate(entry, Utc::now())?;
                    }
                }
                Ok(json!({ "id": id, "skipped": "gate" }))
            })?;
            core.emit_domain_event(Domain::Schedule, outcome.revision, vec![id.to_string()]);
            return Ok(json!({ "id": id, "skipped": "gate" }));
        }
    }
    let expected_spec = serde_json::to_value(&before.spec)?;
    let id_owned = id.to_string();
    let (file, start_outcome) =
        core.repo
            .write_schedule_internal("scheduler.mark-running", move |file| {
                let entry = file
                    .tasks
                    .iter_mut()
                    .find(|entry| entry.id == id_owned)
                    .ok_or_else(|| {
                        CoreError::not_found(
                            "SCHEDULE_NOT_FOUND",
                            format!("未找到定时任务 {id_owned}"),
                        )
                    })?;
                if !manual && (serde_json::to_value(&entry.spec)? != expected_spec
                    || !entry.spec.enabled || crate::plan::is_expired(&entry.spec, Utc::now())?) {
                    return Err(CoreError::execution("SCHEDULE_CHANGED", "执行前定时任务已更改或过期"));
                }
                entry.state.running = true;
                entry.state.last_status = ScheduleStatus::Running;
                if matches!(reason, RunReason::Missed) {
                    entry.state.missed_count += 1;
                    entry.state.last_missed_at = Some(now_iso());
                }
                Ok(json!({ "id": id_owned }))
            })?;
    core.emit_domain_event(
        Domain::Schedule,
        start_outcome.revision,
        vec![id.to_string()],
    );
    let entry = file
        .tasks
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| {
            CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
        })?;
    let runtimes = file.runtimes.clone();
    let started = now_iso();

    let flow = if cancelled.load(Ordering::SeqCst) {
        FlowResult {
            result: Ok(cancelled_output()),
            probe: None,
            probe_no_match: false,
            main_action_started: false,
        }
    } else {
        execute_action_flow(core, &entry, &runtimes, &cancelled)
    };
    let finished = now_iso();

    if flow.probe_no_match {
        let probe_error = flow.result.as_ref().err().cloned();
        let probe_cancelled = flow
            .result
            .as_ref()
            .ok()
            .map(|output| output.cancelled)
            .unwrap_or(false);
        let probe_status = if probe_cancelled {
            ScheduleStatus::Stopped
        } else if probe_error.as_ref().map(|e| e.code.as_str()) == Some("PROBE_NO_MATCH") {
            before.state.last_status
        } else {
            ScheduleStatus::Failed
        };
        let probe = flow.probe.clone();
        let id_owned = id.to_string();
        let (_file, outcome) =
            core.repo
                .write_schedule_internal("scheduler.probe-result", move |file| {
                    let entry = file
                        .tasks
                        .iter_mut()
                        .find(|entry| entry.id == id_owned)
                        .ok_or_else(|| {
                            CoreError::not_found(
                                "SCHEDULE_NOT_FOUND",
                                format!("未找到定时任务 {id_owned}"),
                            )
                        })?;
                    entry.state.running = false;
                    entry.state.last_status = probe_status;
                    if !manual {
                        entry.state.last_probe = probe;
                        apply_next_plan(entry);
                    }
                    Ok(json!({ "id": id_owned }))
                })?;
        core.emit_domain_event(Domain::Schedule, outcome.revision, vec![id.to_string()]);
        if let Some(error) = probe_error {
            if error.code != "PROBE_NO_MATCH" {
                return Err(error);
            }
        }
        return Ok(json!({
            "id": id,
            "probe": if probe_cancelled { "cancelled" } else { "no-match" },
        }));
    }

    let output = match &flow.result {
        Ok(output) => output.clone(),
        Err(error) => crate::exec::ExecOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: error.message.clone(),
            timed_out: false,
            cancelled: cancelled.load(Ordering::SeqCst),
            stdout_truncated: false,
            stderr_truncated: false,
        },
    };
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
                if flow.main_action_started {
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
            }
            Trigger::Condition { cooldown, .. } => {
                if cooldown.is_none() && flow.main_action_started && flow.result.is_ok() {
                    disable = true;
                    stop_reason = Some("condition 命中并已执行".to_string());
                }
            }
            Trigger::Calendar { .. } => {}
        }
    }
    let status = if output.cancelled || cancelled.load(Ordering::SeqCst) {
        ScheduleStatus::Stopped
    } else if flow.result.is_err() {
        ScheduleStatus::Failed
    } else if stopped_by_rule {
        ScheduleStatus::Stopped
    } else if output.exit_code == Some(0)
        || matches!(entry.spec.action, Action::Notification { .. })
    {
        ScheduleStatus::Success
    } else {
        ScheduleStatus::Failed
    };
    if status == ScheduleStatus::Stopped && stop_reason.is_none() {
        stop_reason = Some("被用户终止".to_string());
    }
    let (stdout_summary, stdout_trimmed) = truncate(&output.stdout, SCHEDULE_OUTPUT_MAX_BYTES);
    let (stderr_summary, stderr_trimmed) = truncate(&output.stderr, SCHEDULE_OUTPUT_MAX_BYTES);
    let run_count = entry.state.run_count
        + if manual || !flow.main_action_started {
            0
        } else {
            1
        };
    let probe = flow.probe.clone();
    let id_owned = id.to_string();
    let finished_for_write = finished.clone();
    let stdout_for_write = stdout_summary.clone();
    let stderr_for_write = stderr_summary.clone();
    let (_file, outcome) =
        core.repo
            .write_schedule_internal("scheduler.record-run", move |file| {
                let entry = file
                    .tasks
                    .iter_mut()
                    .find(|entry| entry.id == id_owned)
                    .ok_or_else(|| {
                        CoreError::not_found(
                            "SCHEDULE_NOT_FOUND",
                            format!("未找到定时任务 {id_owned}"),
                        )
                    })?;
                entry.state.running = false;
                entry.state.run_count = run_count;
                // Manual trials live in history/output only, never in planning anchors.
                if !manual { entry.state.last_run_at = Some(finished_for_write.clone()); }
                entry.state.last_status = status;
                entry.state.last_exit_code = output.exit_code;
                entry.state.last_stdout = Some(stdout_for_write.clone());
                entry.state.last_stderr = Some(stderr_for_write.clone());
                if !manual {
                    if let Some(probe) = probe { entry.state.last_probe = Some(probe); }
                    // A rule from an old spec must not disable a task edited during execution.
                    if disable && serde_json::to_value(&entry.spec)? == serde_json::to_value(&before.spec)? {
                        entry.spec.enabled = false;
                        entry.state.next_run_at = None;
                        entry.updated_at = now_iso();
                    } else {
                        apply_next_plan(entry);
                    }
                }
                Ok(json!({ "id": id_owned }))
            })?;
    core.emit_domain_event(Domain::Schedule, outcome.revision, vec![id.to_string()]);

    let kind = match reason {
        RunReason::Scheduled => "scheduled",
        RunReason::Missed => "missed",
        RunReason::Manual => "manual",
    };
    let record = schedule_run_record(
        id,
        entry.state.next_run_at.as_deref(),
        &started,
        &finished,
        kind,
        status.as_str(),
        output.exit_code,
        &output.stdout,
        &output.stderr,
        output.stdout_truncated || stdout_trimmed,
        output.stderr_truncated || stderr_trimmed,
        stop_reason.as_deref(),
        entry.state.missed_count,
    );
    let _ = crate::history::append_bounded_jsonl(
        &core.repo.layout.schedule_history(),
        &record,
        crate::repo::SCHEDULE_HISTORY_MAX_BYTES,
        Some(crate::repo::SCHEDULE_HISTORY_PER_TASK),
    );

    if flow.result.is_ok() && !output.cancelled && !cancelled.load(Ordering::SeqCst) {
        dispatch_action_notifications(core, &entry, &output);
    }
    if let Err(error) = flow.result {
        return Err(error);
    }
    if status == ScheduleStatus::Failed {
        return Err(CoreError::execution(
            "SCHEDULE_ACTION_FAILED",
            format!(
                "定时任务执行失败{}",
                output
                    .exit_code
                    .map(|code| format!("（退出码 {code}）"))
                    .unwrap_or_default()
            ),
        )
        .with_details(json!({
            "id": id,
            "exitCode": output.exit_code,
            "stdout": stdout_summary,
            "stderr": stderr_summary,
            "timedOut": output.timed_out,
            "stdoutTruncated": output.stdout_truncated || stdout_trimmed,
            "stderrTruncated": output.stderr_truncated || stderr_trimmed,
        })));
    }
    Ok(json!({
        "id": id,
        "status": status.as_str(),
        "exitCode": output.exit_code,
        "stdout": stdout_summary,
        "stderr": stderr_summary,
        "stdoutTruncated": output.stdout_truncated || stdout_trimmed,
        "stderrTruncated": output.stderr_truncated || stderr_trimmed,
        "disabled": disable,
    }))
}

fn cancelled_output() -> crate::exec::ExecOutput {
    crate::exec::ExecOutput {
        exit_code: None,
        stdout: String::new(),
        stderr: "任务已取消".to_string(),
        timed_out: false,
        cancelled: true,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn probe_error_flow(
    core: &Arc<crate::host::HostCore>,
    entry: &ScheduleEntry,
    error: CoreError,
) -> FlowResult {
    let now = now_iso();
    let probe = ProbeState {
        at: now.clone(),
        status: ScheduleStatus::Failed,
        exit_code: None,
        stdout: Some(String::new()),
        stderr: Some(error.message.clone()),
    };
    let record = schedule_run_record(
        &entry.id,
        None,
        &now,
        &now,
        "probe",
        "failed",
        None,
        "",
        &error.message,
        false,
        false,
        Some("probe 启动失败"),
        entry.state.missed_count,
    );
    let _ = crate::history::append_bounded_jsonl(
        &core.repo.layout.schedule_history(),
        &record,
        crate::repo::SCHEDULE_HISTORY_MAX_BYTES,
        Some(crate::repo::SCHEDULE_HISTORY_PER_TASK),
    );
    FlowResult {
        result: Err(error),
        probe: Some(probe),
        probe_no_match: true,
        main_action_started: false,
    }
}

fn execute_action_flow(
    core: &Arc<crate::host::HostCore>,
    entry: &ScheduleEntry,
    runtimes: &crate::model::Runtimes,
    cancelled: &Arc<AtomicBool>,
) -> FlowResult {
    // 移动端跑不了脚本 / 外部程序 / 条件探针。同步会把桌面建的这类任务原样推过来，
    // 与其让 spawn 抛一个看不懂的 OS 错误，不如明确说清是平台不支持。
    if let Err(error) = crate::ops_schedule::ensure_platform_supported(&entry.spec) {
        return FlowResult {
            result: Err(error),
            probe: None,
            probe_no_match: false,
            main_action_started: false,
        };
    }
    let mut probe_state = None;
    if let Trigger::Condition { probe, when, .. } = &entry.spec.trigger {
        if cancelled.load(Ordering::SeqCst) {
            return FlowResult {
                result: Ok(cancelled_output()),
                probe: None,
                probe_no_match: false,
                main_action_started: false,
            };
        }
        let probe_spec = match crate::exec::build_probe_spec(probe, runtimes) {
            Ok(spec) => spec,
            Err(error) => return probe_error_flow(core, entry, error),
        };
        let probe_output = match core.processes.run_with_cancel(
            &format!("{}:probe", entry.id),
            probe_spec,
            cancelled.clone(),
        ) {
            Ok(output) => output,
            Err(error) => return probe_error_flow(core, entry, error),
        };
        let probe_failed =
            probe_output.timed_out || probe_output.cancelled || probe_output.exit_code != Some(0);
        let matched =
            !probe_failed && match_stream(when, &probe_output.stdout, &probe_output.stderr);
        let (probe_stdout, stdout_trimmed) =
            truncate(&probe_output.stdout, SCHEDULE_OUTPUT_MAX_BYTES);
        let (probe_stderr, stderr_trimmed) =
            truncate(&probe_output.stderr, SCHEDULE_OUTPUT_MAX_BYTES);
        probe_state = Some(ProbeState {
            at: now_iso(),
            status: if matched {
                ScheduleStatus::Success
            } else {
                ScheduleStatus::Failed
            },
            exit_code: probe_output.exit_code,
            stdout: Some(probe_stdout),
            stderr: Some(probe_stderr),
        });
        if !matched {
            // Ordinary nonmatches are expected polling, not failed execution history.
            if probe_failed {
            let now = now_iso();
            let record = schedule_run_record(
                &entry.id,
                None,
                &now,
                &now,
                "probe",
                "failed",
                probe_output.exit_code,
                &probe_output.stdout,
                &probe_output.stderr,
                probe_output.stdout_truncated || stdout_trimmed,
                probe_output.stderr_truncated || stderr_trimmed,
                Some(if probe_output.cancelled {
                    "probe 被取消"
                } else {
                    "probe 执行失败"
                }),
                entry.state.missed_count,
            );
            let _ = crate::history::append_bounded_jsonl(
                &core.repo.layout.schedule_history(),
                &record,
                crate::repo::SCHEDULE_HISTORY_MAX_BYTES,
                Some(crate::repo::SCHEDULE_HISTORY_PER_TASK),
            );
            }
            if probe_output.cancelled || cancelled.load(Ordering::SeqCst) {
                return FlowResult {
                    result: Ok(cancelled_output()),
                    probe: probe_state,
                    probe_no_match: true,
                    main_action_started: false,
                };
            }
            return FlowResult {
                result: Err(CoreError::execution(
                    if probe_failed { "PROBE_FAILED" } else { "PROBE_NO_MATCH" },
                    if probe_failed { "condition probe 执行失败" } else { "condition probe 未命中，本次不执行主动作" },
                )),
                probe: probe_state,
                probe_no_match: true,
                main_action_started: false,
            };
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        return FlowResult {
            result: Ok(cancelled_output()),
            probe: probe_state,
            probe_no_match: false,
            main_action_started: false,
        };
    }
    let result = match &entry.spec.action {
        Action::Notification { notification } => core
            .show_notification_rendered(notification, &entry.spec.name, &[])
            .map(|_| crate::exec::ExecOutput {
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
                cancelled: false,
                stdout_truncated: false,
                stderr_truncated: false,
            }),
        _ => {
            crate::exec::build_action_spec(&entry.spec.action, runtimes).and_then(|spec| {
                core.processes.run_with_cancel(
                    &entry.id,
                    spec.expect("non-notification action has an execution spec"),
                    cancelled.clone(),
                )
            })
        }
    };
    let main_action_started = result.is_ok();
    FlowResult {
        result,
        probe: probe_state,
        probe_no_match: false,
        main_action_started,
    }
}

fn dispatch_action_notifications(
    core: &Arc<crate::host::HostCore>,
    entry: &ScheduleEntry,
    output: &crate::exec::ExecOutput,
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
        let _ = core.show_notification_rendered(on_complete, &entry.spec.name, &vars);
    }
    if let Some(on_output) = &notifications.on_output {
        if match_stream(&on_output.when, &output.stdout, &output.stderr) {
            let _ =
                core.show_notification_rendered(&on_output.notification, &entry.spec.name, &vars);
        }
    }
}

/// Render a notification template with the allowed variables (§3.5.2).
pub fn render_notification(
    notification: &Notification,
    task_name: &str,
    extra: &[(&str, &str)],
) -> Notification {
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

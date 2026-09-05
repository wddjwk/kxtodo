//! v8 → v9 data migration (requirements §4.2.2). Idempotent; runs under the repo lock.

use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::error::CoreResult;
use crate::history::truncate;
use crate::ids::gen_id;
use crate::model::{DATA_SCHEMA_VERSION, SCHEDULE_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION};
use crate::repo::{
    atomic_write, read_json_value, Layout, DATA_FILE, SCHEDULE_FILE, SCHEDULE_OUTPUT_MAX_BYTES,
    SETTINGS_FILE,
};
use crate::time::{
    format_duration, local_timezone, migrate_legacy_local_time, now_iso, parse_stored_instant,
};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub migrated: bool,
    pub backup_dir: Option<String>,
    pub timezone: Option<String>,
    pub warnings: Vec<String>,
    pub schedule_tasks: Vec<ScheduleMigrationEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleMigrationEntry {
    pub id: String,
    pub status: String,
    pub warnings: Vec<String>,
}

/// Run pending migrations. Safe to call repeatedly; no-op when everything is current.
pub fn migrate_if_needed(layout: &Layout) -> CoreResult<MigrationReport> {
    let mut report = MigrationReport::default();

    let data_path = layout.data_file();
    let settings_path = layout.settings_file();
    let schedule_path = layout.schedule_file();

    let data_raw = if data_path.exists() {
        Some(read_json_value(&data_path)?)
    } else {
        None
    };
    let settings_raw = if settings_path.exists() {
        Some(read_json_value(&settings_path)?)
    } else {
        None
    };
    let schedule_raw = if schedule_path.exists() {
        Some(read_json_value(&schedule_path)?)
    } else {
        None
    };

    let needs_data = data_raw
        .as_ref()
        .map(|value| {
            value
                .get("schemaVersion")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                < DATA_SCHEMA_VERSION as u64
        })
        .unwrap_or(false);
    let needs_settings = settings_raw
        .as_ref()
        .map(|value| {
            value
                .get("_meta")
                .and_then(|m| m.get("schemaVersion"))
                .is_none()
        })
        .unwrap_or(false);
    let needs_schedule = schedule_raw
        .as_ref()
        .map(|value| {
            value
                .get("_meta")
                .and_then(|m| m.get("schemaVersion"))
                .is_none()
        })
        .unwrap_or(false);

    if !needs_data && !needs_settings && !needs_schedule {
        return Ok(report);
    }

    // Backup before any migration (§4.2.2).
    let stamp = now_iso().replace([':', '.'], "-");
    let backup_dir = layout.backup_dir().join(format!("{stamp}-migrate-v9"));
    std::fs::create_dir_all(&backup_dir)?;
    for (path, name) in [
        (&data_path, DATA_FILE),
        (&settings_path, SETTINGS_FILE),
        (&schedule_path, SCHEDULE_FILE),
    ] {
        if path.exists() {
            std::fs::copy(path, backup_dir.join(name))?;
        }
    }
    report.migrated = true;
    report.backup_dir = Some(backup_dir.display().to_string());

    if let Some(mut value) = data_raw {
        if needs_data {
            let warnings = migrate_data(&mut value);
            report.warnings.extend(warnings);
            write_json(&data_path, &value)?;
        }
    }
    if let Some(mut value) = settings_raw {
        if needs_settings {
            let warnings = migrate_settings(&mut value);
            report.warnings.extend(warnings);
            write_json(&settings_path, &value)?;
        }
    }
    if let Some(value) = schedule_raw {
        if needs_schedule {
            let tz = local_timezone();
            report.timezone = tz.map(|zone| zone.to_string());
            if tz.is_none() {
                report.warnings.push(
                    "无法确定本机 IANA 时区；受影响的 once/calendar 定时任务已被禁用".to_string(),
                );
            }
            let migrated = migrate_schedule(&value, tz, &mut report);
            write_json(&schedule_path, &migrated)?;
        }
    }

    // Persist the report alongside the backup for later inspection.
    let report_json = serde_json::to_string_pretty(&report)?;
    atomic_write(&backup_dir.join("migration-report.json"), &report_json)?;

    Ok(report)
}

fn write_json(path: &Path, value: &Value) -> CoreResult<()> {
    let raw = serde_json::to_string_pretty(value)?;
    atomic_write(path, &raw)
}

// ---------------------------------------------------------------------------
// data.json v4 → v5（legacy）+ v5 → v6（order 字段）
// ---------------------------------------------------------------------------

fn migrate_data(value: &mut Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let from = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let Some(root) = value.as_object_mut() else {
        warnings.push("data.json 顶层不是对象，跳过迁移".to_string());
        return warnings;
    };

    if from < 5 {
        // v4 → v5：补 _meta / updatedAt（旧结构没有 _meta，可以整体插入）。
        root.insert("schemaVersion".to_string(), json!(5));
        let mut meta = Map::new();
        meta.insert("revision".to_string(), json!(0));
        meta.insert("idempotency".to_string(), json!([]));
        root.insert("_meta".to_string(), Value::Object(meta));
        let now = now_iso();
        if let Some(nodes) = root.get_mut("nodes").and_then(Value::as_array_mut) {
            for node in nodes.iter_mut().filter_map(Value::as_object_mut) {
                let has_updated = node.get("updatedAt").and_then(Value::as_str).is_some();
                if !has_updated {
                    let fallback = node
                        .get("createdAt")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| now.clone());
                    node.insert("updatedAt".to_string(), json!(fallback));
                }
            }
        }
    }

    if from < 6 {
        // v5 → v6：按数组位置补 order（同级分组内 0,1,2...）。
        if let Some(nodes) = root.get_mut("nodes").and_then(Value::as_array_mut) {
            let mut counters: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
            for node in nodes.iter_mut().filter_map(Value::as_object_mut) {
                let parent = node
                    .get("parentId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let next = counters.entry(parent).or_insert(0.0);
                node.insert("order".to_string(), json!(*next));
                *next += 1.0;
            }
        }
        if let Some(tasks) = root.get_mut("tasks").and_then(Value::as_array_mut) {
            let mut counters: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
            for task in tasks.iter_mut().filter_map(Value::as_object_mut) {
                let node_id = task
                    .get("nodeId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let next = counters.entry(node_id).or_insert(0.0);
                task.insert("order".to_string(), json!(*next));
                *next += 1.0;
            }
        }
    }

    root.insert(
        "schemaVersion".to_string(),
        json!(DATA_SCHEMA_VERSION),
    );
    warnings
}

// ---------------------------------------------------------------------------
// settings.json → _meta.schemaVersion 1 (+ legacy key normalization)
// ---------------------------------------------------------------------------

fn migrate_settings(value: &mut Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(root) = value.as_object_mut() else {
        warnings.push("settings.json 顶层不是对象，跳过迁移".to_string());
        return warnings;
    };

    // Legacy key normalization ported from the TS normalizer rules.
    if let Some(profile) = root.get_mut("profile").and_then(Value::as_object_mut) {
        if profile.get("displayName").is_none() {
            if let Some(name) = profile.remove("name") {
                profile.insert("displayName".to_string(), name);
            }
        }
    }
    if let Some(behavior) = root.remove("behavior") {
        if let Some(mode) = behavior.get("linkOpenMode").cloned() {
            root.entry("appearance")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .map(|appearance| appearance.entry("linkOpenMode".to_string()).or_insert(mode));
        }
    }
    if let Some(display) = root.remove("display") {
        let display = display.as_object().cloned().unwrap_or_default();
        let appearance = root
            .entry("appearance")
            .or_insert_with(|| json!({}))
            .as_object_mut();
        if let (Some(appearance), Some(scale)) = (appearance, display.get("uiScale")) {
            appearance
                .entry("uiScale".to_string())
                .or_insert_with(|| scale.clone());
        }
        let lifecycle = root
            .entry("lifecycle")
            .or_insert_with(|| json!({}))
            .as_object_mut();
        if let Some(lifecycle) = lifecycle {
            if let Some(value) = display.get("closeToTray") {
                lifecycle
                    .entry("closeToTray".to_string())
                    .or_insert_with(|| value.clone());
            }
            if let Some(value) = display.get("launchAtStartup") {
                lifecycle
                    .entry("launchAtStartup".to_string())
                    .or_insert_with(|| value.clone());
            }
        }
        if let Some(value) = display.get("notificationDurationMs") {
            if let Some(notifications) = root
                .entry("notifications")
                .or_insert_with(|| json!({}))
                .as_object_mut()
            {
                notifications
                    .entry("durationMs".to_string())
                    .or_insert_with(|| value.clone());
            }
        }
    }
    if let Some(global_shortcut) = root.remove("globalShortcut") {
        if global_shortcut.is_string() {
            if let Some(shortcuts) = root
                .entry("shortcuts")
                .or_insert_with(|| json!({}))
                .as_object_mut()
            {
                shortcuts
                    .entry("toggleWindow".to_string())
                    .or_insert(global_shortcut);
            }
        }
    }
    if let Some(shortcuts) = root.get_mut("shortcuts") {
        if shortcuts.is_array() {
            let entries = shortcuts.as_array().cloned().unwrap_or_default();
            let mut mapped = Map::new();
            for entry in entries.iter().filter_map(Value::as_object) {
                let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
                let combo = entry.get("combo").and_then(Value::as_str);
                let Some(combo) = combo else { continue };
                let key = match id {
                    "newTask" => Some("newTask"),
                    "focusSearch" => Some("focusSearch"),
                    "toggleWindow" => Some("toggleWindow"),
                    "openSettings" | "toggleSettings" => Some("openSettings"),
                    _ => None,
                };
                if let Some(key) = key {
                    mapped
                        .entry(key.to_string())
                        .or_insert_with(|| json!(combo));
                }
            }
            *shortcuts = Value::Object(mapped);
        }
    }
    if let Some(cloud_sync) = root.remove("cloudSync") {
        if cloud_sync.is_object() && root.get("cloud").is_none() {
            root.insert("cloud".to_string(), cloud_sync);
        }
    }
    // Legacy uiScale values snap back to the default (ported from TS normalizeUiScale).
    if let Some(appearance) = root.get_mut("appearance").and_then(Value::as_object_mut) {
        if let Some(scale) = appearance.get("uiScale").and_then(Value::as_f64) {
            if [0.62, 0.72, 0.86, 0.92]
                .iter()
                .any(|legacy| (scale - legacy).abs() < 0.001)
            {
                appearance.insert("uiScale".to_string(), json!(0.75));
            }
        }
    }

    let mut meta = Map::new();
    meta.insert("schemaVersion".to_string(), json!(SETTINGS_SCHEMA_VERSION));
    meta.insert("revision".to_string(), json!(0));
    meta.insert("idempotency".to_string(), json!([]));
    root.insert("_meta".to_string(), Value::Object(meta));
    warnings
}

// ---------------------------------------------------------------------------
// tasks.json → _meta.schemaVersion 2 (the big mapping, §4.2.2)
// ---------------------------------------------------------------------------

fn migrate_schedule(raw: &Value, tz: Option<chrono_tz::Tz>, report: &mut MigrationReport) -> Value {
    let mut out = Map::new();
    out.insert(
        "_meta".to_string(),
        json!({
            "schemaVersion": SCHEDULE_SCHEMA_VERSION,
            "revision": 0,
            "idempotency": []
        }),
    );
    out.insert(
        "runtimes".to_string(),
        raw.get("runtimes").cloned().unwrap_or_else(|| json!({})),
    );

    let now = now_iso();
    let mut tasks_out = Vec::new();
    if let Some(tasks) = raw.get("tasks").and_then(Value::as_array) {
        for task in tasks {
            let (entry, task_report) = migrate_schedule_task(task, tz, &now);
            report.schedule_tasks.push(task_report);
            tasks_out.push(entry);
        }
    }
    out.insert("tasks".to_string(), Value::Array(tasks_out));
    Value::Object(out)
}

fn migrate_schedule_task(
    raw: &Value,
    tz: Option<chrono_tz::Tz>,
    now: &str,
) -> (Value, ScheduleMigrationEntry) {
    let mut warnings: Vec<String> = Vec::new();
    let id = raw
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| gen_id("schedule"));

    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            warnings.push("任务名称无效，已使用“未命名定时任务”".to_string());
            "未命名定时任务".to_string()
        });
    let mut enabled = raw.get("enabled").and_then(Value::as_bool).unwrap_or(false);

    let created_at = normalize_instant_field(raw.get("createdAt"), now, &mut warnings, "createdAt");
    let updated_at = normalize_instant_field(
        raw.get("updatedAt"),
        &created_at,
        &mut warnings,
        "updatedAt",
    );

    // ----- state -----
    let run_count = raw.get("runCount").and_then(Value::as_u64).unwrap_or(0);
    let last_run_at = raw
        .get("lastRunAt")
        .and_then(Value::as_str)
        .and_then(|value| parse_stored_instant(value).ok())
        .map(|at| crate::time::format_instant(at));
    let last_status = match raw.get("lastStatus").and_then(Value::as_str) {
        Some("success") => "success",
        Some("failed") => "failed",
        Some("stopped") => "stopped",
        Some("running") => {
            warnings.push("lastStatus=running：旧进程已不存在，迁移为 stopped".to_string());
            "stopped"
        }
        _ => "idle",
    };
    let last_exit_code = raw
        .get("lastExitCode")
        .cloned()
        .filter(|v| v.is_i64() || v.is_u64());
    let last_stdout = raw
        .get("lastStdout")
        .and_then(Value::as_str)
        .map(|value| truncate(value, SCHEDULE_OUTPUT_MAX_BYTES).0);
    let last_stderr = raw
        .get("lastStderr")
        .and_then(Value::as_str)
        .map(|value| truncate(value, SCHEDULE_OUTPUT_MAX_BYTES).0);
    if raw.get("nextRunAt").is_some() {
        warnings.push("旧 nextRunAt 不迁移，由调度器按 spec 重新计算".to_string());
    }

    // ----- trigger -----
    let trigger_raw = raw.get("trigger").cloned().unwrap_or_else(|| json!({}));
    let trigger_type = trigger_raw
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("once")
        .to_string();
    let mut trigger_ok = true;
    let trigger = match trigger_type.as_str() {
        "once" => {
            let run_at = trigger_raw
                .get("runAt")
                .and_then(Value::as_str)
                .unwrap_or("");
            match tz {
                Some(zone) => {
                    let (at, mut w) = migrate_legacy_local_time(run_at, zone);
                    warnings.append(&mut w);
                    match at {
                        Some(at) => json!({ "type": "once", "at": at }),
                        None => {
                            trigger_ok = false;
                            warnings.push(format!("once 触发器的 runAt `{run_at}` 无法迁移"));
                            placeholder_trigger(now)
                        }
                    }
                }
                None => {
                    trigger_ok = false;
                    warnings.push("无法确定 IANA 时区，once 任务已禁用".to_string());
                    placeholder_trigger(now)
                }
            }
        }
        "interval" => {
            let every_seconds = trigger_raw
                .get("everySeconds")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if every_seconds == 0 {
                trigger_ok = false;
                warnings.push("interval 的 everySeconds 无效".to_string());
                placeholder_trigger(now)
            } else {
                let mut obj = json!({
                    "type": "interval",
                    "every": format_duration(every_seconds * 1000),
                });
                let repeat = trigger_raw
                    .get("repeatCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if repeat > 0 {
                    obj["maxRuns"] = json!(repeat);
                }
                if let Some(stop) = migrate_match(trigger_raw.get("stopCondition"), "stdout") {
                    obj["stopWhen"] = stop;
                }
                obj
            }
        }
        "calendar" => {
            let cron = trigger_raw
                .get("cron")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            match (cron, tz) {
                (Some(cron), Some(zone)) => {
                    if let Err(error) = crate::plan::validate_cron(&cron) {
                        trigger_ok = false;
                        warnings.push(format!("cron 无效：{error}"));
                        placeholder_trigger(now)
                    } else {
                        json!({
                            "type": "calendar",
                            "cron": cron,
                            "timezone": zone.to_string(),
                        })
                    }
                }
                (None, _) => {
                    trigger_ok = false;
                    warnings.push("calendar 触发器缺少 cron".to_string());
                    placeholder_trigger(now)
                }
                (_, None) => {
                    trigger_ok = false;
                    warnings.push("无法确定 IANA 时区，calendar 任务已禁用".to_string());
                    placeholder_trigger(now)
                }
            }
        }
        "condition" => {
            let every_seconds = trigger_raw
                .get("everySeconds")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let probe = migrate_probe(trigger_raw.get("probeAction"), &mut warnings);
            let when = migrate_match(trigger_raw.get("probeCondition"), "stdout");
            match (every_seconds > 0, probe, when) {
                (true, Some(probe), Some(when)) => json!({
                    "type": "condition",
                    "every": format_duration(every_seconds * 1000),
                    "probe": probe,
                    "when": when,
                }),
                (false, _, _) => {
                    trigger_ok = false;
                    warnings.push("condition 的 everySeconds 无效".to_string());
                    placeholder_trigger(now)
                }
                (_, None, _) => {
                    trigger_ok = false;
                    warnings.push(
                        "condition 的 probeAction 无法迁移（仅支持 script/executable）".to_string(),
                    );
                    placeholder_trigger(now)
                }
                (_, _, None) => {
                    trigger_ok = false;
                    warnings.push("condition 的 probeCondition 无效，任务已禁用".to_string());
                    placeholder_trigger(now)
                }
            }
        }
        other => {
            trigger_ok = false;
            warnings.push(format!("未知触发器类型 `{other}`"));
            placeholder_trigger(now)
        }
    };

    // ----- action -----
    let action_raw = raw.get("action").cloned().unwrap_or_else(|| json!({}));
    let (action, action_ok) = migrate_action(&action_raw, &mut warnings);
    if !action_ok {
        enabled = false;
    }
    if !trigger_ok {
        enabled = false;
    }

    let mut spec = json!({
        "name": name,
        "enabled": enabled,
        "trigger": trigger,
        "action": action,
    });

    // Post-migration run-state rules (§4.2.2 末尾).
    let trigger_kind = spec["trigger"]["type"]
        .as_str()
        .unwrap_or("once")
        .to_string();
    let mut final_status = last_status.to_string();
    if enabled && (trigger_kind == "once" || trigger_kind == "condition") && run_count > 0 {
        spec["enabled"] = json!(false);
        warnings.push("once/condition 已有运行记录，迁移后保持 disabled".to_string());
    }
    if trigger_kind == "interval" {
        if let (Some(max), true) = (
            spec["trigger"]["maxRuns"].as_u64(),
            spec["trigger"]["maxRuns"].is_u64(),
        ) {
            if run_count >= max {
                spec["enabled"] = json!(false);
                final_status = "stopped".to_string();
                warnings.push("interval 已达到 maxRuns，迁移为 disabled/stopped".to_string());
            }
        }
    }

    // Strict structural validation against the real model; fall back to a safe
    // placeholder spec (disabled) when conversion is not lossless.
    if serde_json::from_value::<crate::model::ScheduleSpec>(spec.clone()).is_err() {
        warnings.push("迁移结果未通过 ScheduleSpec 校验，已迁移为禁用占位任务".to_string());
        spec = json!({
            "name": name,
            "enabled": false,
            "trigger": placeholder_trigger(now),
            "action": {
                "type": "notification",
                "notification": { "message": "该定时任务在 v9 迁移中未通过校验，请检查后重新配置" }
            }
        });
    }

    // Compute nextRunAt for enabled schedules.
    let mut state = json!({
        "runCount": run_count,
        "lastStatus": final_status,
    });
    if let Some(value) = last_run_at {
        state["lastRunAt"] = json!(value);
    }
    if let Some(value) = last_exit_code {
        state["lastExitCode"] = value;
    }
    if let Some(value) = last_stdout {
        state["lastStdout"] = json!(value);
    }
    if let Some(value) = last_stderr {
        state["lastStderr"] = json!(value);
    }
    if spec["enabled"].as_bool().unwrap_or(false) {
        let entry_stub = crate::model::ScheduleEntry {
            id: id.clone(),
            spec: serde_json::from_value(spec.clone()).unwrap_or_else(|_| {
                crate::model::ScheduleSpec {
                    name: name.clone(),
                    enabled: false,
                    trigger: crate::model::Trigger::Once {
                        at: now.to_string(),
                        missed_policy: None,
                    },
                    action: crate::model::Action::Notification {
                        notification: crate::model::Notification {
                            title: None,
                            message: String::new(),
                            duration: None,
                            tone: None,
                            position: None,
                            extra: Map::new(),
                        },
                    },
                }
            }),
            state: serde_json::from_value(state.clone()).unwrap_or_default(),
            ui: Default::default(),
            created_at: created_at.clone(),
            updated_at: updated_at.clone(),
            extra: Map::new(),
        };
        match crate::plan::compute_next_run_iso(&entry_stub, chrono::Utc::now()) {
            Ok(Some(next)) => {
                state["nextRunAt"] = json!(next);
            }
            Ok(None) => {}
            Err(error) => {
                spec["enabled"] = json!(false);
                warnings.push(format!("nextRunAt 计算失败，任务已禁用：{error}"));
            }
        }
    }

    let spec_enabled = spec["enabled"].as_bool().unwrap_or(false);
    let mut entry = json!({
        "id": id,
        "spec": spec,
        "state": state,
        "ui": {},
        "createdAt": created_at,
        "updatedAt": updated_at,
    });
    if let Some(expanded) = raw.get("expanded").and_then(Value::as_bool) {
        entry["ui"]["expanded"] = json!(expanded);
    }
    if let Some(editing) = raw.get("editing").and_then(Value::as_bool) {
        entry["ui"]["editing"] = json!(editing);
    }

    let status = if warnings.is_empty() {
        "migrated"
    } else if spec_enabled {
        "migrated-with-warnings"
    } else {
        "disabled"
    };
    let entry_id = entry["id"].as_str().unwrap_or_default().to_string();
    (
        entry,
        ScheduleMigrationEntry {
            id: entry_id,
            status: status.to_string(),
            warnings,
        },
    )
}

fn placeholder_trigger(now: &str) -> Value {
    // Far-future once trigger so a disabled placeholder never fires.
    let at = chrono::DateTime::parse_from_rfc3339(now)
        .map(|value| value + chrono::Duration::days(3650))
        .map(|value| {
            value
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .unwrap_or_else(|_| now.to_string());
    json!({ "type": "once", "at": at })
}

fn normalize_instant_field(
    value: Option<&Value>,
    fallback: &str,
    warnings: &mut Vec<String>,
    field: &str,
) -> String {
    value
        .and_then(Value::as_str)
        .and_then(|raw| parse_stored_instant(raw).ok())
        .map(|at| crate::time::format_instant(at))
        .unwrap_or_else(|| {
            if value.and_then(Value::as_str).is_some() {
                warnings.push(format!("{field} 无效，已按当前时间处理"));
            }
            fallback.to_string()
        })
}

/// v8 SchedulerCondition {enabled, mode, pattern} → v9 Match (only when enabled & pattern set).
fn migrate_match(raw: Option<&Value>, stream: &str) -> Option<Value> {
    let raw = raw?;
    let enabled = raw.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let pattern = raw
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if !enabled {
        return None;
    }
    let mode = match raw.get("mode").and_then(Value::as_str) {
        Some("regex") => "regex",
        _ => "contains",
    };
    Some(json!({
        "stream": stream,
        "mode": mode,
        "pattern": pattern,
    }))
}

/// v8 action → v9 probe (script/executable only, notifications stripped).
fn migrate_probe(raw: Option<&Value>, warnings: &mut Vec<String>) -> Option<Value> {
    let raw = raw?;
    let action_type = raw.get("type").and_then(Value::as_str).unwrap_or("script");
    if action_type == "notification" {
        return None;
    }
    let (value, ok) = migrate_action(raw, warnings);
    if !ok {
        return None;
    }
    // Strip notifications from the probe.
    if let Some(obj) = value.as_object() {
        let mut obj = obj.clone();
        obj.remove("notifications");
        Some(Value::Object(obj))
    } else {
        Some(value)
    }
}

/// v8 action → v9 action. Returns (action, ok); ok=false → caller disables the task.
fn migrate_action(raw: &Value, warnings: &mut Vec<String>) -> (Value, bool) {
    let action_type = raw.get("type").and_then(Value::as_str).unwrap_or("script");
    let script_mode = raw
        .get("scriptMode")
        .and_then(Value::as_str)
        .unwrap_or("inline");
    let language = raw
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("python");
    let interpreter = raw
        .get("interpreter")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let file_path = raw
        .get("filePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let code = raw
        .get("code")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    let executable_path = raw
        .get("executablePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    // Legacy arguments splitting (Domain Core implementation).
    let legacy_args = raw.get("arguments").and_then(Value::as_str).unwrap_or("");
    let args = match crate::exec::split_legacy_arguments(legacy_args) {
        Ok(args) => args,
        Err(error) => {
            warnings.push(format!("arguments 无法无损解析：{error}"));
            return (placeholder_action(), false);
        }
    };

    let working_directory = raw
        .get("workingDirectory")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut wd_invalid = false;
    if let Some(dir) = &working_directory {
        if !std::path::Path::new(dir).is_dir() {
            wd_invalid = true;
            warnings.push(format!("workingDirectory `{dir}` 不存在，任务已禁用"));
        }
    }

    let notifications = migrate_action_notifications(raw, warnings);

    let mut action = match action_type {
        "executable" => {
            let Some(program) = executable_path else {
                warnings.push("executable 缺少 executablePath".to_string());
                return (placeholder_action(), false);
            };
            let mut obj = json!({
                "type": "executable",
                "program": program,
            });
            if !args.is_empty() {
                obj["args"] = json!(args);
            }
            obj
        }
        "notification" => {
            let notification = migrate_notification(raw.get("notification"), "定时任务已触发");
            json!({
                "type": "notification",
                "notification": notification,
            })
        }
        _ => {
            // script
            if language == "custom" {
                // custom language → executable per mapping table.
                let Some(program) = interpreter.clone() else {
                    warnings.push("language=custom 但 interpreter 为空".to_string());
                    return (placeholder_action(), false);
                };
                let mut full_args = Vec::new();
                if script_mode == "path" {
                    let Some(path) = file_path else {
                        warnings.push("custom path 模式缺少 filePath".to_string());
                        return (placeholder_action(), false);
                    };
                    full_args.push(path);
                } else {
                    let Some(code) = code else {
                        warnings.push("custom inline 模式缺少 code".to_string());
                        return (placeholder_action(), false);
                    };
                    full_args.push("-c".to_string());
                    full_args.push(code);
                }
                full_args.extend(args);
                let mut obj = json!({
                    "type": "executable",
                    "program": program,
                });
                if !full_args.is_empty() {
                    obj["args"] = json!(full_args);
                }
                obj
            } else {
                let valid_language = matches!(
                    language,
                    "python" | "javascript" | "powershell" | "bash" | "makefile"
                );
                if !valid_language {
                    warnings.push(format!("未知 script language `{language}`"));
                    return (placeholder_action(), false);
                }
                let source = if script_mode == "path" {
                    match file_path {
                        Some(path) => json!({ "type": "file", "path": path }),
                        None => {
                            warnings.push("script path 模式缺少 filePath".to_string());
                            return (placeholder_action(), false);
                        }
                    }
                } else {
                    match code {
                        Some(code) => json!({ "type": "inline", "code": code }),
                        None => {
                            warnings.push("script inline 模式缺少 code".to_string());
                            return (placeholder_action(), false);
                        }
                    }
                };
                let mut obj = json!({
                    "type": "script",
                    "language": language,
                    "source": source,
                });
                if !args.is_empty() {
                    obj["args"] = json!(args);
                }
                if let Some(interpreter) = interpreter {
                    obj["interpreter"] = json!(interpreter);
                }
                obj
            }
        }
    };

    if let Some(dir) = working_directory {
        action["workingDirectory"] = json!(dir);
    }
    if let Some(notifications) = notifications {
        action["notifications"] = notifications;
    }
    (action, !wd_invalid)
}

fn placeholder_action() -> Value {
    json!({
        "type": "notification",
        "notification": { "message": "该定时任务在 v9 迁移中未通过校验，请检查后重新配置" }
    })
}

fn migrate_action_notifications(raw: &Value, warnings: &mut Vec<String>) -> Option<Value> {
    let mut notifications = Map::new();
    let notify_on_complete = raw
        .get("notifyOnComplete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if notify_on_complete {
        notifications.insert(
            "onComplete".to_string(),
            migrate_notification(
                raw.get("completionNotification"),
                "任务 {taskName} 执行完成",
            ),
        );
    }
    if let Some(stdout_notification) = raw.get("stdoutNotification") {
        let enabled = stdout_notification
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if enabled {
            if let Some(when) = migrate_match(stdout_notification.get("condition"), "stdout") {
                let notification = migrate_notification(
                    stdout_notification.get("notification"),
                    "stdout 匹配成功：\n{stdout}",
                );
                notifications.insert(
                    "onOutput".to_string(),
                    json!({ "when": when, "notification": notification }),
                );
            } else {
                warnings.push("stdoutNotification 条件无效，已丢弃".to_string());
            }
        }
    }
    if notifications.is_empty() {
        None
    } else {
        Some(Value::Object(notifications))
    }
}

/// v8 AppNotification {title,message,durationMs,tone,position} → v9 Notification {title,message,duration,tone,position}.
fn migrate_notification(raw: Option<&Value>, default_message: &str) -> Value {
    let empty = json!({});
    let raw = raw.unwrap_or(&empty);
    let title = raw
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let message = raw
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_message);
    let duration = raw
        .get("durationMs")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .map(format_duration);
    let tone = raw
        .get("tone")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "info" | "success" | "warning" | "error"));
    let position = raw.get("position").and_then(Value::as_str).filter(|value| {
        matches!(
            *value,
            "bottom-right" | "top-right" | "bottom-left" | "top-left"
        )
    });

    let mut out = Map::new();
    if let Some(title) = title {
        out.insert("title".to_string(), json!(title));
    }
    out.insert("message".to_string(), json!(message));
    if let Some(duration) = duration {
        out.insert("duration".to_string(), json!(duration));
    }
    if let Some(tone) = tone {
        out.insert("tone".to_string(), json!(tone));
    }
    if let Some(position) = position {
        out.insert("position".to_string(), json!(position));
    }
    Value::Object(out)
}

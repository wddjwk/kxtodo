//! schedule domain: ScheduleSpec validation, patch semantics, CRUD (§3.5).

use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{
    Action, Probe, ScheduleEntry, ScheduleFile, ScheduleSpec, ScheduleState, ScheduleStatus,
    ScheduleUi, Source, Trigger,
};
use crate::time::{now_iso, parse_duration_ms, parse_stored_instant};

pub const ALLOWED_TEMPLATE_VARS: [&str; 4] = ["taskName", "stdout", "stderr", "exitCode"];
pub const RUNTIME_KEYS: [&str; 5] = ["python", "node", "pwsh", "bash", "make"];

// ---------------------------------------------------------------------------
// Path normalization (CLI cwd based, §3.5.2)
// ---------------------------------------------------------------------------

pub fn normalize_path(raw: &str, cwd: &Path) -> String {
    let path = PathBuf::from(raw);
    let absolute = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().to_string()
}

pub fn normalize_spec_paths(spec: &mut Value, cwd: &Path) {
    let Some(action) = spec.get_mut("action").and_then(Value::as_object_mut) else {
        return;
    };
    normalize_action_paths(action, cwd);
    if let Some(trigger) = spec.get_mut("trigger").and_then(Value::as_object_mut) {
        if let Some(probe) = trigger.get_mut("probe").and_then(Value::as_object_mut) {
            normalize_action_paths(probe, cwd);
        }
    }
}

fn normalize_action_paths(action: &mut Map<String, Value>, cwd: &Path) {
    if let Some(source) = action.get_mut("source").and_then(Value::as_object_mut) {
        if source.get("type").and_then(Value::as_str) == Some("file") {
            if let Some(path) = source.get("path").and_then(Value::as_str) {
                let normalized = normalize_path(path, cwd);
                source.insert("path".to_string(), json!(normalized));
            }
        }
    }
    for key in ["program", "interpreter", "workingDirectory"] {
        if let Some(raw) = action.get(key).and_then(Value::as_str) {
            if raw.trim().is_empty() {
                continue;
            }
            let normalized =
                if key != "workingDirectory" && !raw.contains('/') && !raw.contains('\\') {
                    let found = crate::exec::find_executable(&[raw], &[]);
                    if found.is_empty() {
                        normalize_path(raw, cwd)
                    } else {
                        normalize_path(&found, cwd)
                    }
                } else {
                    normalize_path(raw, cwd)
                };
            action.insert(key.to_string(), json!(normalized));
        }
    }
}

// ---------------------------------------------------------------------------
// Spec validation
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct SpecValidation {
    pub spec: Option<ScheduleSpec>,
    pub warnings: Vec<String>,
}

/// serde 的内部标签枚举不拒绝分支外字段，这里按 discriminator 白名单手动检查
///（§3.5.2：未属于当前分支的字段一律非法）。
fn check_branch_fields(raw: &Value) -> CoreResult<()> {
    let spec = raw
        .as_object()
        .ok_or_else(|| CoreError::validation("INVALID_SPEC", "ScheduleSpec 必须是对象"))?;
    for key in spec.keys() {
        if !matches!(key.as_str(), "name" | "enabled" | "trigger" | "action") {
            return Err(branch_error("spec", key));
        }
    }
    if let Some(trigger) = spec.get("trigger").and_then(Value::as_object) {
        let kind = trigger.get("type").and_then(Value::as_str).unwrap_or("");
        let allowed: &[&str] = match kind {
            "once" => &["type", "at", "missedPolicy"],
            "interval" => &["type", "every", "maxRuns", "stopWhen", "missedPolicy"],
            "calendar" => &["type", "cron", "timezone", "missedPolicy"],
            "condition" => &["type", "every", "probe", "when", "missedPolicy"],
            other => {
                return Err(CoreError::validation(
                    "INVALID_SPEC",
                    format!("未知 trigger.type `{other}`"),
                ))
            }
        };
        for key in trigger.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(branch_error(&format!("trigger({kind})"), key));
            }
        }
        if let Some(probe) = trigger.get("probe").and_then(Value::as_object) {
            check_action_like_fields(probe, false)?;
        }
        if let Some(matcher) = trigger.get("stopWhen").and_then(Value::as_object) {
            check_match_fields(matcher)?;
        }
        if let Some(matcher) = trigger.get("when").and_then(Value::as_object) {
            check_match_fields(matcher)?;
        }
    }
    if let Some(action) = spec.get("action").and_then(Value::as_object) {
        check_action_like_fields(action, true)?;
    }
    Ok(())
}

fn check_action_like_fields(
    action: &Map<String, Value>,
    allow_notifications: bool,
) -> CoreResult<()> {
    let kind = action.get("type").and_then(Value::as_str).unwrap_or("");
    let allowed: &[&str] = match (kind, allow_notifications) {
        ("script", true) => &[
            "type",
            "language",
            "source",
            "args",
            "interpreter",
            "workingDirectory",
            "timeout",
            "notifications",
        ],
        ("script", false) => &[
            "type",
            "language",
            "source",
            "args",
            "interpreter",
            "workingDirectory",
            "timeout",
        ],
        ("executable", true) => &[
            "type",
            "program",
            "args",
            "workingDirectory",
            "timeout",
            "notifications",
        ],
        ("executable", false) => &["type", "program", "args", "workingDirectory", "timeout"],
        ("notification", true) => &["type", "notification"],
        ("notification", false) => &["type"],
        (other, _) => {
            return Err(CoreError::validation(
                "INVALID_SPEC",
                format!("未知 action.type `{other}`"),
            ))
        }
    };
    for key in action.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(branch_error(&format!("action({kind})"), key));
        }
    }
    if let Some(source) = action.get("source").and_then(Value::as_object) {
        let source_kind = source.get("type").and_then(Value::as_str).unwrap_or("");
        let source_allowed: &[&str] = match source_kind {
            "file" => &["type", "path"],
            "inline" => &["type", "code"],
            other => {
                return Err(CoreError::validation(
                    "INVALID_SPEC",
                    format!("未知 source.type `{other}`"),
                ))
            }
        };
        for key in source.keys() {
            if !source_allowed.contains(&key.as_str()) {
                return Err(branch_error(&format!("source({source_kind})"), key));
            }
        }
    }
    if let Some(notification) = action.get("notification").and_then(Value::as_object) {
        check_notification_fields(notification)?;
    }
    if let Some(notifications) = action.get("notifications").and_then(Value::as_object) {
        for key in notifications.keys() {
            if !matches!(key.as_str(), "onComplete" | "onOutput") {
                return Err(branch_error("notifications", key));
            }
        }
        if let Some(on_complete) = notifications.get("onComplete").and_then(Value::as_object) {
            check_notification_fields(on_complete)?;
        }
        if let Some(on_output) = notifications.get("onOutput").and_then(Value::as_object) {
            for key in on_output.keys() {
                if !matches!(key.as_str(), "when" | "notification") {
                    return Err(branch_error("onOutput", key));
                }
            }
            if let Some(matcher) = on_output.get("when").and_then(Value::as_object) {
                check_match_fields(matcher)?;
            }
            if let Some(notification) = on_output.get("notification").and_then(Value::as_object) {
                check_notification_fields(notification)?;
            }
        }
    }
    Ok(())
}

fn check_notification_fields(notification: &Map<String, Value>) -> CoreResult<()> {
    for key in notification.keys() {
        if !matches!(
            key.as_str(),
            "title" | "message" | "duration" | "tone" | "position"
        ) {
            return Err(branch_error("notification", key));
        }
    }
    Ok(())
}

fn check_match_fields(matcher: &Map<String, Value>) -> CoreResult<()> {
    for key in matcher.keys() {
        if !matches!(key.as_str(), "stream" | "mode" | "pattern") {
            return Err(branch_error("match", key));
        }
    }
    Ok(())
}

fn branch_error(scope: &str, key: &str) -> CoreError {
    CoreError::validation(
        "INVALID_SPEC",
        format!("{scope} 不接受字段 `{key}`（与 discriminator 分支无关的字段一律非法）"),
    )
}

pub fn validate_spec_value(
    raw: &Value,
    runtimes: &crate::model::Runtimes,
) -> CoreResult<SpecValidation> {
    check_branch_fields(raw)?;
    let spec: ScheduleSpec = serde_json::from_value(raw.clone()).map_err(|error| {
        CoreError::validation("INVALID_SPEC", format!("ScheduleSpec 校验失败：{error}"))
            .with_hint("运行 kxtodo-cli schema schedule.spec 查看权威结构")
    })?;
    let mut validation = SpecValidation {
        spec: Some(spec.clone()),
        warnings: Vec::new(),
    };
    validate_spec_semantics(&spec, runtimes, &mut validation.warnings)?;
    Ok(validation)
}

fn validate_spec_semantics(
    spec: &ScheduleSpec,
    runtimes: &crate::model::Runtimes,
    warnings: &mut Vec<String>,
) -> CoreResult<()> {
    if spec.name.trim().is_empty() {
        return Err(CoreError::validation(
            "NAME_REQUIRED",
            "定时任务 name 不能为空",
        ));
    }
    match &spec.trigger {
        Trigger::Once { at, .. } => {
            parse_stored_instant(at).map_err(|_| {
                CoreError::validation("INVALID_TIME", format!("once.at 无效：{at}"))
            })?;
        }
        Trigger::Interval {
            every,
            max_runs,
            stop_when,
            ..
        } => {
            parse_duration_ms(every)?;
            if let Some(max) = max_runs {
                if *max == 0 {
                    return Err(CoreError::validation(
                        "INVALID_MAX_RUNS",
                        "maxRuns 必须为正整数；不限次数请省略该字段",
                    ));
                }
            }
            if let Some(matcher) = stop_when {
                validate_match(matcher)?;
            }
        }
        Trigger::Calendar { cron, timezone, .. } => {
            crate::plan::validate_cron(cron)?;
            crate::plan::validate_timezone(timezone)?;
        }
        Trigger::Condition {
            every, probe, when, ..
        } => {
            parse_duration_ms(every)?;
            validate_match(when)?;
            if when.pattern.trim().is_empty() {
                return Err(CoreError::validation(
                    "INVALID_MATCH",
                    "condition.when.pattern 不能为空",
                ));
            }
            let timeout = match probe {
                Probe::Script { timeout, .. } | Probe::Executable { timeout, .. } => timeout,
            };
            match timeout {
                Some(raw) => {
                    parse_duration_ms(raw)?;
                }
                None => warnings.push(
                    "condition.probe 未设置 timeout，建议显式设置有限超时（如 30s）".to_string(),
                ),
            }
            validate_executable_like_probe(probe, runtimes, warnings)?;
        }
    }
    match &spec.action {
        Action::Script {
            language,
            source,
            interpreter,
            working_directory,
            timeout,
            notifications,
            ..
        } => {
            match source {
                Source::File { path } => {
                    if path.trim().is_empty() {
                        return Err(CoreError::validation(
                            "SCRIPT_PATH_REQUIRED",
                            "脚本文件路径为空",
                        ));
                    }
                    if !Path::new(path).is_file() {
                        warnings.push(format!("脚本文件当前不存在：{path}"));
                    }
                }
                Source::Inline { code } => {
                    if code.trim().is_empty() {
                        return Err(CoreError::validation(
                            "SCRIPT_CODE_REQUIRED",
                            "inline 脚本内容为空",
                        ));
                    }
                }
            }
            if let Some(raw) = timeout {
                parse_duration_ms(raw)?;
            }
            if let Some(dir) = working_directory {
                if !Path::new(dir).is_dir() {
                    warnings.push(format!("workingDirectory 当前不存在：{dir}"));
                }
            }
            let interpreter_set = interpreter
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some();
            if !interpreter_set {
                let detected = crate::exec::runtime_path(runtimes, language.runtime_key());
                if detected.is_empty() {
                    warnings.push(format!(
                        "未检测到 {} 运行时，执行前请先 kxtodo-cli schedule runtime set {}",
                        language.as_str(),
                        language.runtime_key()
                    ));
                }
            }
            if let Some(notifications) = notifications {
                validate_action_notifications(notifications)?;
            }
        }
        Action::Executable {
            program,
            working_directory,
            timeout,
            notifications,
            ..
        } => {
            if program.trim().is_empty() {
                return Err(CoreError::validation(
                    "PROGRAM_REQUIRED",
                    "可执行程序路径为空",
                ));
            }
            if let Some(raw) = timeout {
                parse_duration_ms(raw)?;
            }
            if let Some(dir) = working_directory {
                if !Path::new(dir).is_dir() {
                    warnings.push(format!("workingDirectory 当前不存在：{dir}"));
                }
            }
            if let Some(notifications) = notifications {
                validate_action_notifications(notifications)?;
            }
        }
        Action::Notification { notification } => {
            validate_notification(notification)?;
        }
    }
    Ok(())
}

fn validate_executable_like_probe(
    probe: &Probe,
    runtimes: &crate::model::Runtimes,
    warnings: &mut Vec<String>,
) -> CoreResult<()> {
    match probe {
        Probe::Script {
            language,
            source,
            interpreter,
            working_directory,
            timeout,
            ..
        } => {
            match source {
                Source::File { path } => {
                    if path.trim().is_empty() {
                        return Err(CoreError::validation(
                            "SCRIPT_PATH_REQUIRED",
                            "probe 脚本路径为空",
                        ));
                    }
                    if !Path::new(path).is_file() {
                        warnings.push(format!("probe 脚本文件当前不存在：{path}"));
                    }
                }
                Source::Inline { code } => {
                    if code.trim().is_empty() {
                        return Err(CoreError::validation(
                            "SCRIPT_CODE_REQUIRED",
                            "probe inline 内容为空",
                        ));
                    }
                }
            }
            if let Some(raw) = timeout {
                parse_duration_ms(raw)?;
            }
            if let Some(dir) = working_directory {
                if !Path::new(dir).is_dir() {
                    warnings.push(format!("probe workingDirectory 当前不存在：{dir}"));
                }
            }
            let has_interpreter = interpreter
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some();
            if !has_interpreter {
                let detected = crate::exec::runtime_path(runtimes, language.runtime_key());
                if detected.is_empty() {
                    warnings.push(format!("未检测到 {} 运行时（probe）", language.as_str()));
                }
            }
        }
        Probe::Executable {
            program,
            working_directory,
            timeout,
            ..
        } => {
            if program.trim().is_empty() {
                return Err(CoreError::validation(
                    "PROGRAM_REQUIRED",
                    "probe 可执行程序路径为空",
                ));
            }
            if let Some(raw) = timeout {
                parse_duration_ms(raw)?;
            }
            if let Some(dir) = working_directory {
                if !Path::new(dir).is_dir() {
                    warnings.push(format!("probe workingDirectory 当前不存在：{dir}"));
                }
            }
        }
    }
    Ok(())
}

fn validate_match(matcher: &crate::model::Match) -> CoreResult<()> {
    if matcher.pattern.is_empty() {
        return Err(CoreError::validation(
            "INVALID_MATCH",
            "Match.pattern 不能为空",
        ));
    }
    if matcher.mode == crate::model::MatchMode::Regex {
        Regex::new(&matcher.pattern).map_err(|error| {
            CoreError::validation("INVALID_REGEX", format!("正则无效：{error}"))
        })?;
    }
    Ok(())
}

pub fn match_stream(matcher: &crate::model::Match, stdout: &str, stderr: &str) -> bool {
    let haystack = match matcher.stream {
        crate::model::MatchStream::Stdout => stdout,
        crate::model::MatchStream::Stderr => stderr,
    };
    match matcher.mode {
        crate::model::MatchMode::Contains => haystack.contains(&matcher.pattern),
        crate::model::MatchMode::Regex => Regex::new(&matcher.pattern)
            .map(|regex| regex.is_match(haystack))
            .unwrap_or(false),
    }
}

fn validate_notification(notification: &crate::model::Notification) -> CoreResult<()> {
    if notification.message.trim().is_empty() {
        return Err(CoreError::validation(
            "MESSAGE_REQUIRED",
            "通知 message 不能为空",
        ));
    }
    if let Some(raw) = &notification.duration {
        let duration_ms = parse_duration_ms(raw)?;
        if !(1_200..=60_000).contains(&duration_ms) {
            return Err(CoreError::validation(
                "DURATION_OUT_OF_RANGE",
                "通知时长必须在 1200ms 到 60000ms 之间",
            ));
        }
    }
    validate_template_vars(notification.message.as_str())?;
    if let Some(title) = &notification.title {
        validate_template_vars(title)?;
    }
    Ok(())
}

fn validate_action_notifications(
    notifications: &crate::model::ActionNotifications,
) -> CoreResult<()> {
    if let Some(on_complete) = &notifications.on_complete {
        validate_notification(on_complete)?;
    }
    if let Some(on_output) = &notifications.on_output {
        validate_match(&on_output.when)?;
        validate_notification(&on_output.notification)?;
    }
    Ok(())
}

pub fn validate_template_vars(template: &str) -> CoreResult<()> {
    let regex = Regex::new(r"\{([a-zA-Z][a-zA-Z0-9]*)\}").expect("static regex");
    for capture in regex.captures_iter(template) {
        let name = &capture[1];
        if !ALLOWED_TEMPLATE_VARS.contains(&name) {
            return Err(CoreError::validation(
                "UNKNOWN_TEMPLATE_VAR",
                format!(
                    "未知模板变量 {{{name}}}，允许 {}",
                    ALLOWED_TEMPLATE_VARS.join("/")
                ),
            ));
        }
    }
    Ok(())
}

pub fn render_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

// ---------------------------------------------------------------------------
// Patch semantics (§3.2, §3.5.4)
// ---------------------------------------------------------------------------

const FORBIDDEN_PATCH_KEYS: [&str; 13] = [
    "id",
    "createdAt",
    "updatedAt",
    "runCount",
    "lastStatus",
    "lastRunAt",
    "nextRunAt",
    "lastExitCode",
    "lastStdout",
    "lastStderr",
    "state",
    "ui",
    "nextWakeAt",
];

/// Apply a SchedulePatch onto the current spec JSON. Patch semantics:
/// - absent key = unchanged; explicit null = clear optional key;
/// - objects merge recursively, arrays replace wholesale;
/// - changing a `type` discriminator replaces the whole object.
pub fn apply_patch(current: &Value, patch: &Value) -> CoreResult<Value> {
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| CoreError::validation("INVALID_PATCH", "patch 必须是 JSON 对象"))?;
    for key in patch_obj.keys() {
        if FORBIDDEN_PATCH_KEYS.contains(&key.as_str()) {
            return Err(CoreError::validation(
                "PATCH_FORBIDDEN_FIELD",
                format!("patch 不允许包含运行时字段 `{key}`"),
            ));
        }
        if !matches!(key.as_str(), "name" | "enabled" | "trigger" | "action") {
            return Err(CoreError::validation(
                "PATCH_UNKNOWN_FIELD",
                format!("patch 包含未知字段 `{key}`"),
            )
            .with_hint("SchedulePatch 只允许 name/enabled/trigger/action"));
        }
    }
    let mut merged = current.clone();
    merge_value(&mut merged, patch, "")?;
    Ok(merged)
}

fn merge_value(target: &mut Value, patch: &Value, path: &str) -> CoreResult<()> {
    let (Some(target_obj), Some(patch_obj)) = (target.as_object_mut(), patch.as_object()) else {
        *target = patch.clone();
        return Ok(());
    };
    // Discriminator change replaces the whole object (§3.5.4).
    let patch_type = patch_obj.get("type").and_then(Value::as_str);
    let target_type = target_obj.get("type").and_then(Value::as_str);
    if let (Some(new_type), Some(old_type)) = (patch_type, target_type) {
        if new_type != old_type {
            *target = patch.clone();
            return Ok(());
        }
    }
    for (key, value) in patch_obj {
        if value.is_null() {
            if target_obj.remove(key).is_none() {
                // Clearing an absent optional key is a no-op; required-field
                // removal fails at final validation.
            }
            continue;
        }
        match (target_obj.get_mut(key), value) {
            (Some(existing), patch_value) if existing.is_object() && patch_value.is_object() => {
                let next_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                merge_value(existing, patch_value, &next_path)?;
            }
            (Some(existing), patch_value) => {
                *existing = patch_value.clone();
            }
            (None, patch_value) => {
                target_obj.insert(key.clone(), patch_value.clone());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRUD on ScheduleFile
// ---------------------------------------------------------------------------

pub fn find_entry<'a>(file: &'a ScheduleFile, id: &str) -> Option<&'a ScheduleEntry> {
    file.tasks.iter().find(|entry| entry.id == id)
}

pub fn require_entry<'a>(file: &'a ScheduleFile, id: &str) -> CoreResult<&'a ScheduleEntry> {
    find_entry(file, id).ok_or_else(|| {
        CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
            .with_hint("先运行 kxtodo-cli schedule list 或 schedule find --query ...")
    })
}

pub fn schedule_view(entry: &ScheduleEntry) -> Value {
    json!({
        "id": entry.id,
        "spec": entry.spec,
        "state": entry.state,
        "ui": entry.ui,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
    })
}

pub struct AddOutcome {
    pub entry: ScheduleEntry,
    pub warnings: Vec<String>,
}

pub fn add_schedule(
    file: &mut ScheduleFile,
    spec_raw: &Value,
    cwd: &Path,
) -> CoreResult<AddOutcome> {
    let mut spec_json = spec_raw.clone();
    normalize_spec_paths(&mut spec_json, cwd);
    let validation = validate_spec_value(&spec_json, &file.runtimes)?;
    let spec = validation.spec.expect("validated");
    let now = now_iso();
    let mut entry = ScheduleEntry {
        id: gen_id("schedule"),
        spec,
        state: ScheduleState::default(),
        ui: ScheduleUi::default(),
        created_at: now.clone(),
        updated_at: now,
        extra: Map::new(),
    };
    if entry.spec.enabled {
        entry.state.next_run_at =
            crate::plan::compute_next_run_iso(&entry, chrono::Utc::now())?;
    }
    file.tasks.push(entry.clone());
    Ok(AddOutcome {
        entry,
        warnings: validation.warnings,
    })
}

pub fn modify_schedule(
    file: &mut ScheduleFile,
    id: &str,
    patch: &Value,
    cwd: &Path,
) -> CoreResult<AddOutcome> {
    let index = file
        .tasks
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| {
            CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
        })?;
    let current = serde_json::to_value(&file.tasks[index].spec)?;
    let mut merged = apply_patch(&current, patch)?;
    normalize_spec_paths(&mut merged, cwd);
    let validation = validate_spec_value(&merged, &file.runtimes)?;
    let spec = validation.spec.expect("validated");

    let entry = &mut file.tasks[index];
    let trigger_changed =
        serde_json::to_value(&entry.spec)?["trigger"] != serde_json::to_value(&spec)?["trigger"];
    entry.spec = spec;
    entry.updated_at = now_iso();
    if trigger_changed || entry.spec.enabled {
        entry.state.next_run_at = if entry.spec.enabled {
            crate::plan::compute_next_run_iso(entry, chrono::Utc::now())?
        } else {
            None
        };
    }
    Ok(AddOutcome {
        entry: entry.clone(),
        warnings: validation.warnings,
    })
}

pub fn set_enabled(file: &mut ScheduleFile, id: &str, enabled: bool) -> CoreResult<AddOutcome> {
    let index = file
        .tasks
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| {
            CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
        })?;
    if enabled {
        let spec_json = serde_json::to_value(&file.tasks[index].spec)?;
        let validation = validate_spec_value(&spec_json, &file.runtimes)?;
        let entry = &mut file.tasks[index];
        entry.spec.enabled = true;
        entry.updated_at = now_iso();
        entry.state.next_run_at =
            crate::plan::compute_next_run_iso(entry, chrono::Utc::now())?;
        return Ok(AddOutcome {
            entry: entry.clone(),
            warnings: validation.warnings,
        });
    }
    let entry = &mut file.tasks[index];
    entry.spec.enabled = false;
    entry.state.next_run_at = None;
    entry.updated_at = now_iso();
    Ok(AddOutcome {
        entry: entry.clone(),
        warnings: Vec::new(),
    })
}

pub fn remove_schedule(file: &mut ScheduleFile, id: &str) -> CoreResult<ScheduleEntry> {
    let index = file
        .tasks
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| {
            CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
        })?;
    Ok(file.tasks.remove(index))
}

// ---------------------------------------------------------------------------
// list / find
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ScheduleFilter {
    pub enabled: Option<bool>,
    pub status: Option<String>,
    pub trigger_type: Option<String>,
    pub query: Option<String>,
    pub sort: Option<String>,
    pub descending: bool,
    pub created_from: Option<String>,
    pub created_to: Option<String>,
    pub updated_from: Option<String>,
    pub updated_to: Option<String>,
    pub last_run_from: Option<String>,
    pub last_run_to: Option<String>,
    pub next_run_from: Option<String>,
    pub next_run_to: Option<String>,
}

fn schedule_range(
    from: &Option<String>,
    to: &Option<String>,
) -> CoreResult<(Option<i64>, Option<i64>)> {
    let from = from
        .as_deref()
        .map(|raw| crate::ops_task::parse_range_bound(raw, false))
        .transpose()?;
    let to = to
        .as_deref()
        .map(|raw| crate::ops_task::parse_range_bound(raw, true))
        .transpose()?;
    if matches!((from, to), (Some(start), Some(end)) if start > end) {
        return Err(CoreError::validation(
            "INVALID_TIME_RANGE",
            "时间范围 from 不得晚于 to",
        ));
    }
    Ok((from, to))
}

fn schedule_in_range(value: Option<&str>, range: (Option<i64>, Option<i64>)) -> bool {
    if range.0.is_none() && range.1.is_none() {
        return true;
    }
    let Some(ms) = value
        .and_then(|raw| parse_stored_instant(raw).ok())
        .map(|instant| instant.timestamp_millis())
    else {
        return false;
    };
    range.0.map(|from| ms >= from).unwrap_or(true) && range.1.map(|to| ms <= to).unwrap_or(true)
}

pub fn filter_schedules<'a>(
    file: &'a ScheduleFile,
    filter: &ScheduleFilter,
) -> CoreResult<Vec<&'a ScheduleEntry>> {
    if let Some(status) = filter.status.as_deref() {
        if !matches!(
            status,
            "idle" | "running" | "success" | "failed" | "stopped" | "all"
        ) {
            return Err(CoreError::validation(
                "INVALID_STATUS",
                format!("无效 --status `{status}`"),
            ));
        }
    }
    if let Some(kind) = filter.trigger_type.as_deref() {
        if !matches!(kind, "once" | "interval" | "calendar" | "condition") {
            return Err(CoreError::validation(
                "INVALID_TRIGGER_TYPE",
                format!("无效 --trigger-type `{kind}`"),
            ));
        }
    }
    let sort = filter.sort.as_deref().unwrap_or("createdAt");
    if !matches!(
        sort,
        "name" | "createdAt" | "updatedAt" | "lastRunAt" | "nextRunAt"
    ) {
        return Err(CoreError::validation(
            "INVALID_SORT",
            format!("无效 --sort `{sort}`"),
        ));
    }
    let created_range = schedule_range(&filter.created_from, &filter.created_to)?;
    let updated_range = schedule_range(&filter.updated_from, &filter.updated_to)?;
    let last_run_range = schedule_range(&filter.last_run_from, &filter.last_run_to)?;
    let next_run_range = schedule_range(&filter.next_run_from, &filter.next_run_to)?;
    let mut entries: Vec<&ScheduleEntry> = file
        .tasks
        .iter()
        .filter(|entry| {
            if let Some(enabled) = filter.enabled {
                if entry.spec.enabled != enabled {
                    return false;
                }
            }
            if let Some(status) = &filter.status {
                if status != "all" && entry.state.last_status.as_str() != status {
                    return false;
                }
            }
            if let Some(kind) = &filter.trigger_type {
                if entry.spec.trigger.kind_str() != kind {
                    return false;
                }
            }
            if !schedule_in_range(Some(&entry.created_at), created_range)
                || !schedule_in_range(Some(&entry.updated_at), updated_range)
                || !schedule_in_range(entry.state.last_run_at.as_deref(), last_run_range)
                || !schedule_in_range(entry.state.next_run_at.as_deref(), next_run_range)
            {
                return false;
            }
            if let Some(query) = &filter.query {
                let needle = query.trim().to_lowercase();
                if needle.is_empty() {
                    return true;
                }
                let mut haystacks: Vec<String> =
                    vec![entry.spec.name.to_lowercase(), entry.id.to_lowercase()];
                match &entry.spec.action {
                    Action::Script { source, .. } => match source {
                        Source::File { path } => haystacks.push(path.to_lowercase()),
                        Source::Inline { .. } => {}
                    },
                    Action::Executable { program, .. } => haystacks.push(program.to_lowercase()),
                    Action::Notification { notification } => {
                        haystacks.push(notification.message.to_lowercase());
                        if let Some(title) = &notification.title {
                            haystacks.push(title.to_lowercase());
                        }
                    }
                }
                if !haystacks.iter().any(|text| text.contains(&needle)) {
                    return false;
                }
            }
            true
        })
        .collect();
    entries.sort_by(|a, b| {
        let ordering = match sort {
            "name" => a.spec.name.cmp(&b.spec.name),
            "updatedAt" => a.updated_at.cmp(&b.updated_at),
            "nextRunAt" => a
                .state
                .next_run_at
                .as_deref()
                .unwrap_or("")
                .cmp(b.state.next_run_at.as_deref().unwrap_or("")),
            "lastRunAt" => a
                .state
                .last_run_at
                .as_deref()
                .unwrap_or("")
                .cmp(b.state.last_run_at.as_deref().unwrap_or("")),
            _ => a.created_at.cmp(&b.created_at),
        };
        if filter.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(entries)
}

// ---------------------------------------------------------------------------
// runtimes
// ---------------------------------------------------------------------------

pub fn runtime_view(file: &ScheduleFile) -> Value {
    let detected = crate::exec::detect_runtimes();
    let entry = |key: &str, configured: &str, detected: &str| {
        json!({
            "name": key,
            "path": if configured.trim().is_empty() { detected } else { configured },
            "source": if configured.trim().is_empty() { "detected" } else { "configured" },
            "available": !(if configured.trim().is_empty() { detected } else { configured }).is_empty(),
        })
    };
    json!({
        "runtimes": [
            entry("python", &file.runtimes.python, &detected.python),
            entry("node", &file.runtimes.node, &detected.node),
            entry("pwsh", &file.runtimes.pwsh, &detected.pwsh),
            entry("bash", &file.runtimes.bash, &detected.bash),
            entry("make", &file.runtimes.make, &detected.make),
        ]
    })
}

pub fn set_runtime(file: &mut ScheduleFile, name: &str, path: &str) -> CoreResult<Value> {
    if !RUNTIME_KEYS.contains(&name) {
        return Err(CoreError::validation(
            "UNKNOWN_RUNTIME",
            format!("未知运行时 `{name}`，支持 {}", RUNTIME_KEYS.join("/")),
        ));
    }
    if !path.trim().is_empty() && !Path::new(path).is_file() {
        return Err(CoreError::validation(
            "RUNTIME_NOT_FOUND",
            format!("运行时路径不存在：{path}"),
        ));
    }
    let value = path.trim().to_string();
    match name {
        "python" => file.runtimes.python = value.clone(),
        "node" => file.runtimes.node = value.clone(),
        "pwsh" => file.runtimes.pwsh = value.clone(),
        "bash" => file.runtimes.bash = value.clone(),
        "make" => file.runtimes.make = value.clone(),
        _ => unreachable!(),
    }
    Ok(json!({ "name": name, "path": value, "source": "configured" }))
}

/// Re-detect runtimes; fills only empty slots (explicit `runtime set` wins).
pub fn detect_runtimes(file: &mut ScheduleFile) -> Value {
    let detected = crate::exec::detect_runtimes();
    let mut updated = Vec::new();
    let fill =
        |key: &str, configured: &mut String, detected_value: &str, updated: &mut Vec<String>| {
            if configured.trim().is_empty() && !detected_value.is_empty() {
                *configured = detected_value.to_string();
                updated.push(key.to_string());
            }
        };
    fill(
        "python",
        &mut file.runtimes.python,
        &detected.python,
        &mut updated,
    );
    fill(
        "node",
        &mut file.runtimes.node,
        &detected.node,
        &mut updated,
    );
    fill(
        "pwsh",
        &mut file.runtimes.pwsh,
        &detected.pwsh,
        &mut updated,
    );
    fill(
        "bash",
        &mut file.runtimes.bash,
        &detected.bash,
        &mut updated,
    );
    fill(
        "make",
        &mut file.runtimes.make,
        &detected.make,
        &mut updated,
    );
    json!({ "updated": updated })
}

pub fn last_status_of(entry: &ScheduleEntry) -> ScheduleStatus {
    entry.state.last_status
}

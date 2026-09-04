//! schema command: machine-readable structures generated from the real models (§3.7).

use serde_json::{json, Map, Value};

use crate::error::{CoreError, CoreResult};
use crate::model::{Match, Notification, ScheduleSpec};

/// JSON Schema for ScheduleSpec, straight from the authoritative Rust model.
pub fn spec_schema() -> Value {
    let schema = schemars::schema_for!(ScheduleSpec);
    serde_json::to_value(schema).unwrap_or(Value::Null)
}

pub fn notification_schema() -> Value {
    let schema = schemars::schema_for!(Notification);
    serde_json::to_value(schema).unwrap_or(Value::Null)
}

pub fn match_schema() -> Value {
    let schema = schemars::schema_for!(Match);
    serde_json::to_value(schema).unwrap_or(Value::Null)
}

/// SchedulePatch schema is auto-derived from the spec schema (§3.5.4):
/// every `required` constraint is dropped recursively; unknown keys stay illegal.
pub fn patch_schema() -> Value {
    let mut schema = spec_schema();
    strip_required(&mut schema);
    if let Some(obj) = schema.as_object_mut() {
        obj.insert(
            "description".to_string(),
            json!("SchedulePatch：由 ScheduleSpec 派生；所有字段可选，null 清除可选字段，type 变化时整个对象替换"),
        );
    }
    schema
}

fn strip_required(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("required");
            for (_, child) in map.iter_mut() {
                strip_required(child);
            }
        }
        Value::Array(items) => {
            for child in items.iter_mut() {
                strip_required(child);
            }
        }
        _ => {}
    }
}

/// Curated, validated examples referenced by `--example <name>`.
pub fn spec_example(name: &str) -> Option<Value> {
    let tool = if cfg!(windows) { "./tool.exe" } else { "./tool" };
    let example = match name {
        "once-notification" => json!({
            "name": "提交周报提醒",
            "enabled": false,
            "trigger": { "type": "once", "at": "2026-07-31T17:30:00+08:00" },
            "action": {
                "type": "notification",
                "notification": { "title": "KXToDo", "message": "记得提交周报" }
            }
        }),
        "interval-script" => json!({
            "name": "等待并下载 XXX",
            "enabled": true,
            "trigger": {
                "type": "interval",
                "every": "1h",
                "stopWhen": { "stream": "stdout", "mode": "contains", "pattern": "DOWNLOAD_DONE" }
            },
            "action": {
                "type": "script",
                "language": "python",
                "source": { "type": "file", "path": "./download_when_ready.py" },
                "args": [],
                "workingDirectory": "./downloads",
                "timeout": "10m"
            }
        }),
        "calendar-notification" => json!({
            "name": "每日站会提醒",
            "enabled": true,
            "trigger": {
                "type": "calendar",
                "cron": "0 9 * * *",
                "timezone": "Asia/Shanghai",
                "missedPolicy": "skip"
            },
            "action": {
                "type": "notification",
                "notification": { "message": "站会开始了", "tone": "warning" }
            }
        }),
        "condition-script" => json!({
            "name": "就绪后立即构建",
            "enabled": true,
            "trigger": {
                "type": "condition",
                "every": "1m",
                "probe": {
                    "type": "script",
                    "language": "python",
                    "source": { "type": "inline", "code": "print('READY')" },
                    "args": [],
                    "timeout": "30s"
                },
                "when": { "stream": "stdout", "mode": "contains", "pattern": "READY" }
            },
            "action": {
                "type": "script",
                "language": "bash",
                "source": { "type": "file", "path": "./build.sh" },
                "timeout": "10m",
                "notifications": {
                    "onComplete": { "message": "{taskName} 完成：{stdout}", "tone": "success" }
                }
            }
        }),
        "executable" => json!({
            "name": "每小时健康检查",
            "enabled": false,
            "trigger": { "type": "interval", "every": "1h" },
            "action": {
                "type": "executable",
                "program": tool,
                "args": ["--mode", "check"],
                "workingDirectory": "./work",
                "timeout": "5m"
            }
        }),
        _ => return None,
    };
    Some(example)
}

pub fn example_names() -> Vec<&'static str> {
    vec![
        "once-notification",
        "interval-script",
        "calendar-notification",
        "condition-script",
        "executable",
    ]
}

pub fn jq_schema() -> Value {
    serde_json::from_str(crate::jq::JQ_SUBSET_DOC).unwrap_or(Value::Null)
}

/// Command schema from the live clap tree (single source = CLI definitions).
pub fn command_schema(root: &clap::Command, path: &str) -> CoreResult<Value> {
    let mut current = root;
    let mut usage = vec!["kxtodo".to_string()];
    for segment in path.split('.') {
        let next = current
            .get_subcommands()
            .find(|sub| sub.get_name() == segment)
            .ok_or_else(|| {
                CoreError::not_found(
                    "SCHEMA_NOT_FOUND",
                    format!("未知命令 `{path}`；运行 kxtodo-cli --help 查看命令树"),
                )
            })?;
        usage.push(segment.to_string());
        current = next;
    }
    let mut params = Map::new();
    for arg in current.get_arguments() {
        let id = arg.get_id().as_str().to_string();
        if matches!(id.as_str(), "help" | "version") {
            continue;
        }
        let mut entry = Map::new();
        entry.insert(
            "type".to_string(),
            json!(arg
                .get_value_names()
                .and_then(|names| names.first())
                .map(|name| name.as_str())
                .unwrap_or("bool")),
        );
        if arg.is_required_set() {
            entry.insert("required".to_string(), json!(true));
        }
        let possible: Vec<String> = arg
            .get_possible_values()
            .iter()
            .map(|value| value.get_name().to_string())
            .collect();
        if !possible.is_empty() {
            entry.insert("enum".to_string(), json!(possible));
        }
        let defaults: Vec<String> = arg
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().to_string())
            .collect();
        if !defaults.is_empty() {
            entry.insert("default".to_string(), json!(defaults));
        }
        if let Some(help) = arg.get_help() {
            entry.insert("description".to_string(), json!(help.to_string()));
        }
        if let Some(num_args) = arg.get_num_args() {
            if num_args.max_values() > 1 {
                entry.insert("repeatable".to_string(), json!(true));
            }
        }
        let flag = arg
            .get_long()
            .map(|long| format!("--{long}"))
            .unwrap_or_else(|| id.clone());
        params.insert(flag, Value::Object(entry));
    }
    Ok(json!({
        "command": path,
        "usage": usage.join(" "),
        "risk": risk_for(path),
        "description": current
            .get_about()
            .map(|text| text.to_string())
            .unwrap_or_default(),
        "params": params,
    }))
}

/// Risk levels per command (§3.2). Kept next to help text via long_about too.
pub fn risk_for(command: &str) -> &'static str {
    match command {
        "task.remove" | "schedule.remove" | "schedule.enable" | "schedule.run" | "config.reset" => {
            "high-risk-write"
        }
        "task.add"
        | "task.modify"
        | "schedule.add"
        | "schedule.modify"
        | "schedule.disable"
        | "schedule.stop"
        | "schedule.runtime.detect"
        | "schedule.runtime.set"
        | "config.set"
        | "config.unset"
        | "notify" => "write",
        _ => "read",
    }
}

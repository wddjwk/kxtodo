//! Output rendering: json / pretty / table / ndjson (§3.2 输出协议).

use serde_json::Value;

use crate::core::ExecOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Json,
    Pretty,
    Table,
    Ndjson,
}

impl Format {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "json" => Some(Format::Json),
            "pretty" => Some(Format::Pretty),
            "table" => Some(Format::Table),
            "ndjson" => Some(Format::Ndjson),
            _ => None,
        }
    }
}

/// Render the final CLI output: (exit code, stdout, stderr).
/// Errors always render as the JSON envelope to stderr.
pub fn render(
    outcome: &ExecOutcome,
    format: Format,
    jq: Option<&crate::jq::CompiledJq>,
) -> (i32, String, String) {
    if outcome.code != 0 {
        return (
            outcome.code,
            String::new(),
            serde_json::to_string_pretty(&outcome.envelope).unwrap_or_default(),
        );
    }
    let envelope = outcome.envelope.clone();
    if let Some(program) = jq {
        match crate::jq::evaluate(program, &envelope) {
            Ok(filtered) => {
                return (
                    0,
                    serde_json::to_string_pretty(&filtered).unwrap_or_default(),
                    String::new(),
                );
            }
            Err(error) => {
                let failure = serde_json::json!({
                    "ok": false,
                    "command": envelope.get("command").cloned().unwrap_or(Value::Null),
                    "error": error.to_json(),
                    "meta": envelope.get("meta").cloned().unwrap_or(Value::Null),
                });
                return (
                    error.exit_code(),
                    String::new(),
                    serde_json::to_string_pretty(&failure).unwrap_or_default(),
                );
            }
        }
    }
    let code = 0;
    match format {
        Format::Json => (
            code,
            serde_json::to_string_pretty(&envelope).unwrap_or_default(),
            String::new(),
        ),
        Format::Ndjson => {
            let mut lines = Vec::new();
            if let Some(items) = envelope
                .get("data")
                .and_then(|data| data.get("items"))
                .and_then(Value::as_array)
            {
                for item in items {
                    lines.push(serde_json::to_string(item).unwrap_or_default());
                }
            } else if let Some(data) = envelope.get("data") {
                lines.push(serde_json::to_string(data).unwrap_or_default());
            }
            (code, lines.join("\n"), String::new())
        }
        Format::Table => (code, render_table(&envelope), String::new()),
        Format::Pretty => (code, render_pretty(&envelope), String::new()),
    }
}

fn command_of(envelope: &Value) -> &str {
    envelope
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn items_of(envelope: &Value) -> Option<&Vec<Value>> {
    envelope
        .get("data")
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
}

fn truncate(text: &str, max: usize) -> String {
    let text = text.replace('\n', " ").replace('\r', "");
    let mut chars = text.chars();
    let taken: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

fn render_table(envelope: &Value) -> String {
    let command = command_of(envelope);
    let mut out: Vec<String> = Vec::new();
    if let Some(items) = items_of(envelope) {
        match command {
            "task.list" | "task.find" => {
                let is_node = items
                    .first()
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    .map(|kind| kind != "item")
                    .unwrap_or(false);
                if is_node {
                    out.push(format!("{:<22} {:<10} {:<30} PATH", "ID", "TYPE", "NAME"));
                    for item in items {
                        out.push(format!(
                            "{:<22} {:<10} {:<30} {}",
                            item["id"].as_str().unwrap_or(""),
                            item["type"].as_str().unwrap_or(""),
                            truncate(item["name"].as_str().unwrap_or(""), 30),
                            item["path"].as_str().unwrap_or(""),
                        ));
                    }
                } else {
                    out.push(format!(
                        "{:<22} {:<4} {:<4} {:<10} {:<40} ENTRY",
                        "ID", "DONE", "IMP", "DUE", "MARKDOWN"
                    ));
                    for item in items {
                        out.push(format!(
                            "{:<22} {:<4} {:<4} {:<10} {:<40} {}",
                            item["id"].as_str().unwrap_or(""),
                            if item["completed"].as_bool().unwrap_or(false) {
                                "✓"
                            } else {
                                ""
                            },
                            if item["important"].as_bool().unwrap_or(false) {
                                "★"
                            } else {
                                ""
                            },
                            item["dueDate"].as_str().unwrap_or(""),
                            truncate(item["markdown"].as_str().unwrap_or(""), 40),
                            item["entry"]["name"].as_str().unwrap_or(""),
                        ));
                    }
                }
            }
            "schedule.list" | "schedule.find" => {
                out.push(format!(
                    "{:<22} {:<8} {:<10} {:<10} {:<24} NAME",
                    "ID", "ENABLED", "TRIGGER", "STATUS", "NEXT RUN"
                ));
                for item in items {
                    out.push(format!(
                        "{:<22} {:<8} {:<10} {:<10} {:<24} {}",
                        item["id"].as_str().unwrap_or(""),
                        item["spec"]["enabled"].as_bool().unwrap_or(false),
                        item["spec"]["trigger"]["type"].as_str().unwrap_or(""),
                        item["state"]["lastStatus"].as_str().unwrap_or("idle"),
                        item["state"]["nextRunAt"].as_str().unwrap_or(""),
                        truncate(item["spec"]["name"].as_str().unwrap_or(""), 30),
                    ));
                }
            }
            "config.list" => {
                out.push(format!("{:<32} {:<36} SOURCE", "PATH", "VALUE"));
                for item in items {
                    let value = serde_json::to_string(&item["value"]).unwrap_or_default();
                    out.push(format!(
                        "{:<32} {:<36} {}",
                        item["path"].as_str().unwrap_or(""),
                        truncate(&value, 36),
                        item["source"].as_str().unwrap_or(""),
                    ));
                }
            }
            _ => {
                for item in items {
                    out.push(serde_json::to_string(item).unwrap_or_default());
                }
            }
        }
        if let Some(count) = envelope.get("meta").and_then(|m| m.get("count")) {
            out.push(format!("-- 共 {} 条", count));
        }
        return out.join("\n");
    }
    render_kv(envelope.get("data").cloned().unwrap_or(Value::Null))
}

fn render_kv(value: Value) -> String {
    match value {
        Value::Object(map) => {
            let mut lines = Vec::new();
            for (key, value) in map {
                let rendered = match &value {
                    Value::String(text) => text.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                lines.push(format!("{key}: {}", truncate(&rendered, 80)));
            }
            lines.join("\n")
        }
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    }
}

fn render_pretty(envelope: &Value) -> String {
    let command = command_of(envelope);
    let data = envelope.get("data").cloned().unwrap_or(Value::Null);
    let mut out: Vec<String> = Vec::new();
    if let Some(items) = items_of(envelope) {
        match command {
            "task.list" | "task.find" => {
                for item in items {
                    if item["type"].as_str() == Some("item") {
                        let mut line = String::new();
                        line.push_str(if item["completed"].as_bool().unwrap_or(false) {
                            "[x] "
                        } else {
                            "[ ] "
                        });
                        line.push_str(&truncate(item["markdown"].as_str().unwrap_or(""), 60));
                        line.push_str(&format!("  ({})", item["id"].as_str().unwrap_or("")));
                        if item["important"].as_bool().unwrap_or(false) {
                            line.push_str(" ★");
                        }
                        if let Some(due) = item["dueDate"].as_str() {
                            line.push_str(&format!(" 📅{due}"));
                        }
                        if let Some(entry) = item["entry"]["path"].as_str() {
                            line.push_str(&format!("  ＠{entry}"));
                        }
                        out.push(line);
                    } else {
                        out.push(format!(
                            "[{}] {}  ({})  {}",
                            item["type"].as_str().unwrap_or(""),
                            item["name"].as_str().unwrap_or(""),
                            item["id"].as_str().unwrap_or(""),
                            item["path"].as_str().unwrap_or(""),
                        ));
                    }
                }
                if out.is_empty() {
                    out.push("（无匹配项）".to_string());
                }
            }
            "schedule.list" | "schedule.find" => {
                for item in items {
                    out.push(format!(
                        "{} {} [{}] {} 下次: {}",
                        if item["spec"]["enabled"].as_bool().unwrap_or(false) {
                            "▶"
                        } else {
                            "⏸"
                        },
                        item["spec"]["name"].as_str().unwrap_or(""),
                        item["spec"]["trigger"]["type"].as_str().unwrap_or(""),
                        item["id"].as_str().unwrap_or(""),
                        item["state"]["nextRunAt"].as_str().unwrap_or("-"),
                    ));
                }
                if out.is_empty() {
                    out.push("（无定时任务）".to_string());
                }
            }
            "config.list" => {
                for item in items {
                    let value = serde_json::to_string(&item["value"]).unwrap_or_default();
                    out.push(format!(
                        "{} = {}{}",
                        item["path"].as_str().unwrap_or(""),
                        truncate(&value, 60),
                        if item["source"].as_str() == Some("default") {
                            "  (默认)"
                        } else {
                            ""
                        },
                    ));
                }
            }
            _ => return render_kv(data),
        }
        if let Some(count) = envelope.get("meta").and_then(|m| m.get("count")) {
            out.push(format!("-- 共 {} 条", count));
        }
        return out.join("\n");
    }
    match command {
        "task.get" => {
            if data["type"].as_str() == Some("item") {
                out.push(format!("任务 {}", data["id"].as_str().unwrap_or("")));
                out.push(format!(
                    "  状态: {}{}{}",
                    if data["completed"].as_bool().unwrap_or(false) {
                        "已完成"
                    } else {
                        "未完成"
                    },
                    if data["important"].as_bool().unwrap_or(false) {
                        "，重要"
                    } else {
                        ""
                    },
                    if data["myDay"].as_bool().unwrap_or(false) {
                        "，我的一天"
                    } else {
                        ""
                    },
                ));
                if let Some(entry) = data["entry"]["path"].as_str() {
                    out.push(format!("  位置: {entry}"));
                }
                if let Some(planned) = data["plannedDate"].as_str() {
                    out.push(format!("  计划: {planned}"));
                }
                if let Some(due) = data["dueDate"].as_str() {
                    out.push(format!("  截止: {due}"));
                }
                out.push("  ---".to_string());
                for line in data["markdown"].as_str().unwrap_or("").lines() {
                    out.push(format!("  {line}"));
                }
                return out.join("\n");
            }
            render_kv(data)
        }
        _ => render_kv(data),
    }
}

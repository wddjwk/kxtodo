#![recursion_limit = "512"]
//! version / schema / skills / doctor / jq / help / formats (§3.1, §3.2, §3.7).

mod common;

use common::TestEnv;
use serde_json::Value;
use todo_note_lib::domain::jq;

#[test]
fn version_reports_schema_versions() {
    let env = TestEnv::fresh();
    let data = env.ok(&["version"]);
    assert_eq!(data["schemaVersions"]["data"], 5);
    assert_eq!(data["schemaVersions"]["settings"], 1);
    assert_eq!(data["schemaVersions"]["schedule"], 2);
}

#[test]
fn help_exits_zero_at_every_level() {
    let env = TestEnv::fresh();
    for args in [
        vec!["--help"],
        vec!["task", "--help"],
        vec!["task", "add", "--help"],
        vec!["schedule", "--help"],
        vec!["config", "set", "--help"],
    ] {
        let result = env.run(&args);
        assert_eq!(result.code, 0, "{args:?} 帮助应退出 0：{}", result.stderr);
        assert!(result.stderr.contains("Risk") || result.stderr.contains("用法") || result.stderr.contains("Usage") || result.stderr.contains("KXToDo"));
    }
}

#[test]
fn action_help_shows_risk_level() {
    let env = TestEnv::fresh();
    let result = env.run(&["task", "remove", "--help"]);
    assert!(result.stderr.contains("high-risk-write"));
    let result = env.run(&["task", "add", "--help"]);
    assert!(result.stderr.contains("Risk: write"));
    let result = env.run(&["task", "get", "--help"]);
    assert!(result.stderr.contains("Risk: read"));
}

#[test]
fn unknown_command_is_exit_2() {
    let env = TestEnv::fresh();
    let result = env.run(&["frobnicate"]);
    assert_eq!(result.code, 2);
}

#[test]
fn schema_spec_and_patch_come_from_model() {
    let env = TestEnv::fresh();
    let spec = env.ok(&["schema", "schedule.spec"]);
    assert!(spec.get("properties").is_some() || spec.get("$ref").is_some());
    let text = spec.to_string();
    assert!(text.contains("missedPolicy"));
    assert!(text.contains("stopWhen"));

    let patch = env.ok(&["schema", "schedule.patch"]);
    // patch schema 递归移除了 required
    fn has_required(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.contains_key("required") || map.values().any(has_required)
            }
            Value::Array(items) => items.iter().any(has_required),
            _ => false,
        }
    }
    assert!(!has_required(&patch), "patch schema 不得包含 required");

    let notification = env.ok(&["schema", "notification"]);
    assert!(notification.to_string().contains("duration"));
    let matcher = env.ok(&["schema", "match"]);
    assert!(matcher.to_string().contains("pattern"));
}

#[test]
fn schema_command_introspection() {
    let env = TestEnv::fresh();
    let add = env.ok(&["schema", "task.add"]);
    assert_eq!(add["command"], "task.add");
    assert_eq!(add["risk"], "write");
    assert!(add["params"]["--type"].is_object());
    let remove = env.ok(&["schema", "task.remove"]);
    assert_eq!(remove["risk"], "high-risk-write");
    env.err(&["schema", "task.nope"], 3);
}

#[test]
fn schema_jq_lists_supported_subset() {
    let env = TestEnv::fresh();
    let doc = env.ok(&["schema", "jq"]);
    assert!(doc["syntax"].is_array());
    assert!(doc["examples"].is_array());
}

#[test]
fn jq_subset_behaviour() {
    let input = serde_json::json!({
        "data": {
            "items": [
                { "id": "a", "completed": false, "markdown": "任务A" },
                { "id": "b", "completed": true, "markdown": "任务B" },
            ],
            "count": 2,
        }
    });
    assert_eq!(jq::apply(".data.count", &input).unwrap(), 2);
    assert_eq!(
        jq::apply(".data.items | length", &input).unwrap(),
        serde_json::json!(2)
    );
    assert_eq!(
        jq::apply(".data.items[0].id", &input).unwrap(),
        serde_json::json!("a")
    );
    assert_eq!(
        jq::apply(".data.items[-1].id", &input).unwrap(),
        serde_json::json!("b")
    );
    assert_eq!(
        jq::apply(".data.items[] | .id", &input).unwrap(),
        serde_json::json!(["a", "b"])
    );
    assert_eq!(
        jq::apply(".data.items | map(.id)", &input).unwrap(),
        serde_json::json!(["a", "b"])
    );
    assert_eq!(
        jq::apply(".data.items[] | select(.completed == false) | .id", &input).unwrap(),
        serde_json::json!("a")
    );
    assert_eq!(
        jq::apply(".data.items | keys", &input).unwrap_err().code,
        "JQ_TYPE"
    );
    assert!(jq::apply(".data | length", &input).is_ok());
    assert!(jq::apply("..", &input).is_err(), "递归下降不在子集内");
    assert!(jq::apply(".data.items | sort_by(.id)", &input).is_err());
}

#[test]
fn skills_commands_work_embedded() {
    let env = TestEnv::fresh();
    let list = env.ok(&["skills", "list"]);
    assert_eq!(list["skills"][0]["name"], "kxtodo");
    assert_eq!(list["skills"][0]["available"], true);
    assert_eq!(list["skills"][0]["source"], "embedded");

    let read = env.ok(&["skills", "read", "kxtodo"]);
    assert!(read["content"].as_str().unwrap().contains("KXToDo Agent SKILL"));
    assert_eq!(read["version"], 1);
    assert_eq!(read["source"], "embedded");

    env.err(&["skills", "read", "unknown"], 3);

    let path = env.ok(&["skills", "path"]);
    assert_eq!(path["source"], "embedded");

    let validate = env.ok(&["skills", "validate"]);
    assert!(
        validate["valid"].as_bool().unwrap(),
        "SKILL 校验应通过：{validate}"
    );
}

#[test]
fn doctor_on_fresh_dir() {
    let env = TestEnv::fresh();
    let report = env.ok(&["doctor"]);
    assert!(report["checks"].as_array().unwrap().len() >= 8);
    assert!(report["healthy"].as_bool().unwrap(), "新目录应健康：{report}");
}

#[test]
fn doctor_reports_corrupted_data() {
    let env = TestEnv::fresh();
    env.ok(&["task", "tree"]);
    std::fs::write(env.path().join("data.json"), "{ bad").unwrap();
    let report = env.ok(&["doctor"]);
    assert_eq!(report["healthy"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "data" && check["ok"] == false));
}

#[test]
fn output_formats_table_pretty_ndjson() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&["task", "add", "--type", "item", "--entry-id", &entry_id, "--markdown", "任务甲"]);
    env.ok(&["task", "add", "--type", "item", "--entry-id", &entry_id, "--markdown", "任务乙"]);

    let table = env.run(&[
        "task", "list", "--type", "item", "--entry-id", &entry_id, "--format", "table",
    ]);
    assert_eq!(table.code, 0);
    assert!(table.stdout.contains("MARKDOWN"));
    assert!(table.stdout.contains("任务甲"));

    let pretty = env.run(&[
        "task", "list", "--type", "item", "--entry-id", &entry_id, "--format", "pretty",
    ]);
    assert_eq!(pretty.code, 0);
    assert!(pretty.stdout.contains("[ ] 任务甲"));

    let ndjson = env.run(&[
        "task", "list", "--type", "item", "--entry-id", &entry_id, "--format", "ndjson",
    ]);
    assert_eq!(ndjson.code, 0);
    let lines: Vec<&str> = ndjson.stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let parsed: Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["type"], "item");
    }

    // 错误信封不受格式影响
    let error = env.run(&["task", "get", "--type", "item", "--id", "nope", "--format", "table"]);
    assert_eq!(error.code, 3);
    let parsed: Value = serde_json::from_str(&error.stderr).unwrap();
    assert_eq!(parsed["ok"], false);
}

#[test]
fn duration_parser_is_shared() {
    use todo_note_lib::domain::time::{format_duration, parse_duration_ms};
    assert_eq!(parse_duration_ms("500ms").unwrap(), 500);
    assert_eq!(parse_duration_ms("5s").unwrap(), 5000);
    assert_eq!(parse_duration_ms("10m").unwrap(), 600_000);
    assert_eq!(parse_duration_ms("1h").unwrap(), 3_600_000);
    assert_eq!(parse_duration_ms("2d").unwrap(), 172_800_000);
    assert!(parse_duration_ms("0s").is_err());
    assert!(parse_duration_ms("5x").is_err());
    assert!(parse_duration_ms("").is_err());
    assert_eq!(format_duration(86_400_000), "1d");
    assert_eq!(format_duration(3_600_000), "1h");
    assert_eq!(format_duration(90_000), "90s");
    assert_eq!(format_duration(5200), "5200ms");
}

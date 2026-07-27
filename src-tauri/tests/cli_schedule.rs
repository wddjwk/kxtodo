#![recursion_limit = "512"]
//! schedule domain tests (§3.5).

mod common;

use common::TestEnv;
use serde_json::json;

fn notification_spec() -> serde_json::Value {
    json!({
        "name": "提交周报提醒",
        "enabled": false,
        "trigger": { "type": "once", "at": "2030-07-31T17:30:00+08:00" },
        "action": {
            "type": "notification",
            "notification": { "title": "KXToDo", "message": "记得提交周报" }
        }
    })
}

fn add_spec(env: &TestEnv, spec: &serde_json::Value, extra: &[&str]) -> serde_json::Value {
    let spec_file = env.path().join("spec.json");
    std::fs::write(&spec_file, serde_json::to_string(spec).unwrap()).unwrap();
    let spec_arg = format!("@{}", spec_file.display());
    let mut args = vec!["schedule", "add", "--spec", spec_arg.as_str()];
    args.extend_from_slice(extra);
    env.ok(&args)
}

#[test]
fn add_and_get_schedule() {
    let env = TestEnv::fresh();
    let created = add_spec(&env, &notification_spec(), &[]);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("schedule-"));
    assert_eq!(created["spec"]["name"], "提交周报提醒");
    assert_eq!(created["spec"]["enabled"], false);
    assert_eq!(created["state"]["runCount"], 0);

    let got = env.ok(&["schedule", "get", "--id", &id]);
    assert_eq!(got["spec"]["trigger"]["type"], "once");
    assert_eq!(got["spec"]["action"]["type"], "notification");
}

#[test]
fn add_enabled_code_requires_yes() {
    let env = TestEnv::fresh();
    let spec = json!({
        "name": "代码任务",
        "enabled": true,
        "trigger": { "type": "interval", "every": "1h" },
        "action": {
            "type": "script",
            "language": "python",
            "source": { "type": "inline", "code": "print('x')" }
        }
    });
    let spec_file = env.path().join("spec.json");
    std::fs::write(&spec_file, serde_json::to_string(&spec).unwrap()).unwrap();
    let spec_arg = format!("@{}", spec_file.display());
    let error = env.err(&["schedule", "add", "--spec", &spec_arg], 10);
    assert_eq!(error["code"], "CONFIRMATION_REQUIRED");

    let created = env.ok(&["schedule", "add", "--spec", &spec_arg, "--yes"]);
    assert_eq!(created["spec"]["enabled"], true);
    assert!(created["state"]["nextRunAt"].is_string());
}

#[test]
fn validate_catches_schema_and_semantic_errors() {
    let env = TestEnv::fresh();

    // 未知字段
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"once","at":"2030-01-01T00:00:00Z","cron":"0 9 * * *"},"action":{"type":"notification","notification":{"message":"m"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_SPEC");

    // interval 分支不接受 cron（discriminator 分支外字段）
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"interval","every":"1h","cron":"0 9 * * *"},"action":{"type":"notification","notification":{"message":"m"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_SPEC");

    // 无效 duration
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"interval","every":"5x"},"action":{"type":"notification","notification":{"message":"m"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_DURATION");

    // 无效 cron
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"calendar","cron":"not a cron","timezone":"Asia/Shanghai"},"action":{"type":"notification","notification":{"message":"m"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_CRON");

    // 未知模板变量
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"once","at":"2030-01-01T00:00:00Z"},"action":{"type":"notification","notification":{"message":"{secret} 泄露"}}}"#,
    ], 2);
    assert_eq!(error["code"], "UNKNOWN_TEMPLATE_VAR");

    // file script 不接受 code；inline 不接受 path
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","trigger":{"type":"interval","every":"1h"},"action":{"type":"script","language":"python","source":{"type":"file","path":"./a.py","code":"print(1)"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_SPEC");

    // 合法 spec 通过
    let valid = env.ok(&["schedule", "validate", "--spec", &notification_spec().to_string()]);
    assert_eq!(valid["valid"], true);
    assert!(valid["normalizedSpec"]["trigger"]["at"].is_string());
}

#[test]
fn spec_rejects_runtime_fields() {
    let env = TestEnv::fresh();
    let error = env.err(&[
        "schedule", "validate", "--spec",
        r#"{"name":"x","id":"schedule-hack","runCount":3,"trigger":{"type":"once","at":"2030-01-01T00:00:00Z"},"action":{"type":"notification","notification":{"message":"m"}}}"#,
    ], 2);
    assert_eq!(error["code"], "INVALID_SPEC");
}

#[test]
fn patch_semantics() {
    let env = TestEnv::fresh();
    let spec = json!({
        "name": "间隔任务",
        "enabled": false,
        "trigger": { "type": "interval", "every": "1h", "maxRuns": 5 },
        "action": {
            "type": "script",
            "language": "python",
            "source": { "type": "inline", "code": "print('x')" }
        }
    });
    let created = add_spec(&env, &spec, &[]);
    let id = created["id"].as_str().unwrap().to_string();

    // 局部合并
    let modified = env.ok(&["schedule", "modify", "--id", &id, "--patch", r#"{"trigger":{"every":"30m"}}"#]);
    assert_eq!(modified["spec"]["trigger"]["every"], "30m");
    assert_eq!(modified["spec"]["trigger"]["maxRuns"], 5);

    // null 清除可选字段
    let modified = env.ok(&["schedule", "modify", "--id", &id, "--patch", r#"{"trigger":{"maxRuns":null}}"#]);
    assert!(modified["spec"]["trigger"]["maxRuns"].is_null());

    // 运行时字段被拒绝
    let error = env.err(&["schedule", "modify", "--id", &id, "--patch", r#"{"runCount":9}"#], 2);
    assert_eq!(error["code"], "PATCH_FORBIDDEN_FIELD");

    // 未知字段被拒绝
    env.err(&["schedule", "modify", "--id", &id, "--patch", r#"{"nope":1}"#], 2);

    // discriminator 切换必须完整（once 缺少 at）
    let error = env.err(&["schedule", "modify", "--id", &id, "--patch", r#"{"trigger":{"type":"once"}}"#], 2);
    assert_eq!(error["code"], "INVALID_SPEC");

    // 完整切换成功，旧分支字段不残留
    let modified = env.ok(&[
        "schedule", "modify", "--id", &id,
        "--patch", r#"{"trigger":{"type":"once","at":"2031-01-01T00:00:00Z"}}"#,
    ]);
    assert_eq!(modified["spec"]["trigger"]["type"], "once");
    assert!(modified["spec"]["trigger"]["every"].is_null());

    // patch 校验命令
    let valid = env.ok(&["schedule", "validate", "--id", &id, "--patch", r#"{"name":"新名字"}"#]);
    assert_eq!(valid["valid"], true);
}

#[test]
fn enable_disable_and_revision() {
    let env = TestEnv::fresh();
    let created = add_spec(&env, &notification_spec(), &[]);
    let id = created["id"].as_str().unwrap().to_string();

    let enabled = env.run(&["schedule", "enable", "--id", &id]);
    assert_eq!(enabled.code, 0);
    let envelope = enabled.envelope();
    assert_eq!(envelope["data"]["spec"]["enabled"], true);
    assert!(envelope["data"]["state"]["nextRunAt"].is_string());
    let rev_enable = envelope["meta"]["revision"].as_u64().unwrap();

    let disabled = env.run(&["schedule", "disable", "--id", &id]);
    let envelope = disabled.envelope();
    assert_eq!(envelope["data"]["spec"]["enabled"], false);
    assert!(envelope["data"]["state"]["nextRunAt"].is_null());
    assert!(envelope["meta"]["revision"].as_u64().unwrap() > rev_enable);
}

#[test]
fn remove_requires_yes() {
    let env = TestEnv::fresh();
    let created = add_spec(&env, &notification_spec(), &[]);
    let id = created["id"].as_str().unwrap().to_string();

    env.err(&["schedule", "remove", "--id", &id], 10);
    env.ok(&["schedule", "remove", "--id", &id, "--yes"]);
    env.err(&["schedule", "get", "--id", &id], 3);
}

#[test]
fn list_find_and_filters() {
    let env = TestEnv::fresh();
    add_spec(&env, &notification_spec(), &[]);
    let spec2 = json!({
        "name": "每小时下载",
        "enabled": false,
        "trigger": { "type": "interval", "every": "1h" },
        "action": {
            "type": "script",
            "language": "python",
            "source": { "type": "file", "path": "./download.py" }
        }
    });
    add_spec(&env, &spec2, &[]);

    let all = env.ok(&["schedule", "list"]);
    assert_eq!(all["items"].as_array().unwrap().len(), 2);

    let by_trigger = env.ok(&["schedule", "list", "--trigger-type", "interval"]);
    assert_eq!(by_trigger["items"].as_array().unwrap().len(), 1);

    let found = env.ok(&["schedule", "find", "--query", "下载"]);
    assert_eq!(found["items"].as_array().unwrap().len(), 1);

    let found_path = env.ok(&["schedule", "find", "--query", "download.py"]);
    assert_eq!(found_path["items"].as_array().unwrap().len(), 1);

    let found_notification = env.ok(&["schedule", "find", "--query", "提交周报"]);
    assert_eq!(found_notification["items"].as_array().unwrap().len(), 1);
}

#[test]
fn runtime_list_set_detect() {
    let env = TestEnv::fresh();
    let list = env.ok(&["schedule", "runtime", "list"]);
    assert!(list["runtimes"].as_array().unwrap().len() == 5);

    env.err(&["schedule", "runtime", "set", "ruby", "/x"], 2);
    env.err(&["schedule", "runtime", "set", "python", "/no/such/file"], 2);

    let python = todo_note_lib::domain::exec::find_executable(&["python", "python3", "py"], &[]);
    if !python.is_empty() {
        let set = env.ok(&["schedule", "runtime", "set", "python", &python]);
        assert_eq!(set["runtime"]["path"].as_str().unwrap(), python);
        let list = env.ok(&["schedule", "runtime", "list"]);
        let entry = list["runtimes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "python")
            .unwrap();
        assert_eq!(entry["source"], "configured");
    }

    let detected = env.ok(&["schedule", "runtime", "detect"]);
    assert!(detected["runtimes"].is_array());
}

#[test]
fn logs_empty_for_new_task() {
    let env = TestEnv::fresh();
    let created = add_spec(&env, &notification_spec(), &[]);
    let id = created["id"].as_str().unwrap().to_string();
    let logs = env.ok(&["schedule", "logs", "--id", &id]);
    assert_eq!(logs["runs"].as_array().unwrap().len(), 0);
}

#[test]
fn status_reports_host_and_counts() {
    let env = TestEnv::fresh();
    add_spec(&env, &notification_spec(), &[]);
    let status = env.ok(&["schedule", "status"]);
    assert_eq!(status["tasks"]["total"], 1);
    assert!(status["host"].is_object());
    assert!(status["runtimes"].is_array());
}

#[test]
fn run_without_host_returns_execution_error() {
    let env = TestEnv::fresh();
    let created = add_spec(&env, &notification_spec(), &[]);
    let id = created["id"].as_str().unwrap().to_string();
    // Local routing：无 Host → HOST_REQUIRED（退出码 20 类）
    let error = env.err(&["schedule", "run", "--id", &id, "--yes"], 20);
    assert_eq!(error["code"], "HOST_REQUIRED");
}

#[test]
fn spec_example_from_schema_validates() {
    let env = TestEnv::fresh();
    for name in ["once-notification", "interval-script", "calendar-notification", "condition-script", "executable"] {
        let output = env.ok(&["schema", "schedule.spec", "--example", name]);
        let example = output["example"].to_string();
        let valid = env.ok(&["schedule", "validate", "--spec", &example]);
        assert_eq!(valid["valid"], true, "示例 {name} 应通过校验");
    }
}

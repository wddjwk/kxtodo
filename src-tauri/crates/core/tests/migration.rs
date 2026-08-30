#![recursion_limit = "512"]
//! v8 → v9 migration tests (§4.2.2).

mod common;

use common::{v8_data, v8_settings, v8_tasks, TestEnv};
use serde_json::json;

#[test]
fn data_migration_adds_meta_and_updated_at() {
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), v8_tasks());
    // 触发迁移
    env.ok(&["task", "tree"]);

    let data = env.read_file("data.json");
    assert_eq!(data["schemaVersion"], 5);
    assert_eq!(data["_meta"]["revision"], 0);
    assert!(data["_meta"]["idempotency"].is_array());

    // updatedAt 以 createdAt 回填
    let category = data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "category-aaa")
        .unwrap();
    assert_eq!(category["updatedAt"], "2025-01-02T00:00:00.000Z");
    // 未知字段保留
    assert_eq!(category["customField"], "keep-me");
    // 动态 backgrounds key 保留
    assert_eq!(data["backgrounds"]["entry-bbb"]["image"], "img:bg-1.png");
    // GUI 临时字段保留
    let item = &data["tasks"][0];
    assert_eq!(item["expanded"], false);
    assert_eq!(item["tags"][0]["text"], "需求");
}

#[test]
fn settings_migration_normalizes_legacy_keys() {
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), v8_tasks());
    env.ok(&["config", "get", "appearance.uiScale"]);

    let settings = env.read_file("settings.json");
    assert_eq!(settings["_meta"]["schemaVersion"], 1);
    // profile.name → displayName
    assert_eq!(settings["profile"]["displayName"], "旧版用户");
    // display.* 迁移
    assert_eq!(settings["lifecycle"]["closeToTray"], false);
    assert_eq!(settings["notifications"]["durationMs"], 4200);
    // globalShortcut → shortcuts.toggleWindow
    assert_eq!(settings["shortcuts"]["toggleWindow"], "Ctrl+Shift+K");
    // 旧 uiScale 0.72 吸附回默认
    assert_eq!(settings["appearance"]["uiScale"], 0.75);
    // 动态 uiColors key 保留（含 entry id）
    assert_eq!(settings["appearance"]["uiColors"]["entry-bbb"], "#dfe8df");
}

#[test]
fn schedule_migration_maps_every_trigger_type() {
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), v8_tasks());
    env.ok(&["schedule", "list"]);

    let tasks = env.read_file("tasks.json");
    assert_eq!(tasks["_meta"]["schemaVersion"], 2);
    assert_eq!(tasks["runtimes"]["python"], "C:\\Python\\python.exe");
    let entries = tasks["tasks"].as_array().unwrap();
    assert_eq!(entries.len(), 3);

    // 1) interval script path 任务
    let first = entries.iter().find(|e| e["id"] == "schedule-0001").unwrap();
    assert_eq!(first["spec"]["name"], "定时任务 1");
    assert_eq!(first["spec"]["trigger"]["type"], "interval");
    assert_eq!(first["spec"]["trigger"]["every"], "1h"); // 3600s → 1h
    assert_eq!(first["spec"]["trigger"]["maxRuns"], 10);
    assert_eq!(
        first["spec"]["trigger"]["stopWhen"]["pattern"],
        "DOWNLOAD_DONE"
    );
    assert!(
        first["spec"]["trigger"]["cron"].is_null(),
        "interval 不得残留 cron"
    );
    let action = &first["spec"]["action"];
    assert_eq!(action["type"], "script");
    assert_eq!(action["language"], "python");
    assert_eq!(
        action["source"],
        json!({ "type": "file", "path": "D:\\scripts\\download.py" })
    );
    assert!(action.get("code").is_none(), "path 分支残留 code 必须丢弃");
    assert_eq!(action["args"], json!(["--fast", "quoted arg"]));
    assert_eq!(
        action["notifications"]["onComplete"]["message"],
        "完成：{stdout}"
    );
    assert_eq!(action["notifications"]["onComplete"]["duration"], "5200ms");
    assert_eq!(
        action["notifications"]["onOutput"]["when"]["pattern"],
        "READY"
    );
    // state
    assert_eq!(first["state"]["runCount"], 5);
    assert_eq!(first["state"]["lastStatus"], "stopped");
    assert_eq!(first["state"]["lastExitCode"], 0);
    assert_eq!(first["state"]["lastStdout"], "hello");
    // ui 保留
    assert_eq!(first["ui"]["expanded"], true);
    assert_eq!(first["ui"]["editing"], false);
    // 旧 nextRunAt 不直接迁移
    assert!(
        first["state"]["nextRunAt"].is_null(),
        "disabled 任务不重算 nextRunAt"
    );

    // 2) once + custom language → executable；running → stopped
    let second = entries.iter().find(|e| e["id"] == "schedule-0002").unwrap();
    assert_eq!(second["spec"]["trigger"]["type"], "once");
    let at = second["spec"]["trigger"]["at"].as_str().unwrap();
    assert!(at.contains("T"), "runAt 应规范化：{at}");
    let action = &second["spec"]["action"];
    assert_eq!(action["type"], "executable");
    assert_eq!(action["program"], "D:\\tools\\rscript.exe");
    assert_eq!(action["args"], json!(["-c", "print(1)", "-v"]));
    assert_eq!(
        second["state"]["lastStatus"], "stopped",
        "running 应迁移为 stopped"
    );
    // once 未运行过且 enabled → 重算 nextRunAt
    assert!(second["spec"]["enabled"].is_boolean());

    // 3) condition：probe 通知被剥除
    let third = entries.iter().find(|e| e["id"] == "schedule-0003").unwrap();
    assert_eq!(third["spec"]["trigger"]["type"], "condition");
    assert_eq!(third["spec"]["trigger"]["every"], "1m");
    assert_eq!(third["spec"]["trigger"]["when"]["pattern"], "READY");
    let probe = &third["spec"]["trigger"]["probe"];
    assert_eq!(probe["type"], "script");
    assert!(
        probe.get("notifications").is_none(),
        "probe 不得携带 notifications"
    );
    let action = &third["spec"]["action"];
    assert_eq!(action["type"], "notification");
    assert_eq!(action["notification"]["message"], "资源就绪");
    assert_eq!(action["notification"]["duration"], "5200ms");
    assert_eq!(action["notification"]["position"], "top-right");

    // 迁移备份存在且包含原文件
    let backups = std::fs::read_dir(env.path().join("backups"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .count();
    assert!(backups >= 1, "迁移必须保留备份");
}

#[test]
fn migration_is_idempotent() {
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), v8_tasks());
    env.ok(&["schedule", "list"]);
    let first = env.read_file("tasks.json");
    env.ok(&["schedule", "list"]);
    let second = env.read_file("tasks.json");
    assert_eq!(first, second, "重复迁移必须无变化");
}

#[test]
fn migrated_schedules_queryable_via_cli() {
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), v8_tasks());
    let list = env.ok(&["schedule", "list", "--all"]);
    assert_eq!(list["items"].as_array().unwrap().len(), 3);
    let got = env.ok(&["schedule", "get", "--id", "schedule-0001"]);
    assert_eq!(got["spec"]["trigger"]["every"], "1h");
    // spec 可重新校验
    let validate = env.ok(&[
        "schedule",
        "validate",
        "--id",
        "schedule-0001",
        "--patch",
        r#"{"name":"新名"}"#,
    ]);
    assert_eq!(validate["valid"], true);
}

#[test]
fn legacy_local_time_without_offset_uses_host_timezone() {
    let mut tasks = v8_tasks();
    tasks["tasks"]
        .as_array_mut()
        .unwrap()
        .retain(|task| task["id"] == "schedule-0002");
    let env = TestEnv::with_v8_data(v8_data(), v8_settings(), tasks);
    env.ok(&["schedule", "list"]);
    let file = env.read_file("tasks.json");
    let at = file["tasks"][0]["spec"]["trigger"]["at"].as_str().unwrap();
    // "2026-07-31T17:30"（本地墙钟）→ 带时区的 instant
    let parsed = chrono::DateTime::parse_from_rfc3339(at).expect("应为带时区 ISO");
    let local_tz: chrono_tz::Tz = kxtodo_core::time::local_timezone()
        .map(|tz| tz.to_string())
        .unwrap_or_else(|| "UTC".to_string())
        .parse()
        .unwrap_or(chrono_tz::UTC);
    let rendered = parsed.with_timezone(&local_tz).format("%H:%M").to_string();
    assert_eq!(rendered, "17:30", "本地墙钟时间应保持 17:30，实际 {at}");
}

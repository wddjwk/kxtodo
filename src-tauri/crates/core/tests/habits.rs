//! habit domain end-to-end（v0.9.3）：习惯打卡的增删改查、打卡语义、确认门与同步实体。

mod common;

use chrono::Datelike;
use common::TestEnv;
use serde_json::Value;

fn add(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["habit", "add"];
    full.extend_from_slice(args);
    env.ok(&full)
}

fn today() -> String {
    chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()
}

#[test]
fn add_defaults_and_list_filter() {
    let env = TestEnv::fresh();
    let brush = add(&env, &["--name", "刷牙", "--time", "08:00"]);
    assert!(brush["id"].as_str().unwrap().starts_with("habit-"));
    assert_eq!(brush["freq"]["kind"], "daily");
    assert_eq!(brush["time"], "08:00");
    assert_eq!(brush["status"], "active");
    assert_eq!(brush["checkins"].as_array().map(Vec::len).unwrap_or(0), 0);

    let report = add(&env, &["--name", "周报", "--freq", "weekly", "--weekdays", "0", "--time", "17:00"]);
    assert_eq!(report["freq"]["kind"], "weekly");
    assert_eq!(report["freq"]["weekdays"], serde_json::json!([0]));

    let listed = env.ok(&["habit", "list"]);
    assert_eq!(listed["count"], 2);
    let paused = env.ok(&["habit", "list", "--status", "paused"]);
    assert_eq!(paused["count"], 0);

    let got = env.ok(&["habit", "get", "--id", brush["id"].as_str().unwrap()]);
    assert_eq!(got["name"], "刷牙");
}

#[test]
fn validation_rejects_bad_shapes() {
    let env = TestEnv::fresh();
    let error = env.err(&["habit", "add", "--name", "x", "--freq", "weekly"], 2);
    assert_eq!(error["code"], "HABIT_FREQ_INVALID");
    let error = env.err(&["habit", "add", "--name", "x", "--weekdays", "1"], 2);
    assert_eq!(error["code"], "CLI_ARGUMENT_ERROR");
    let error = env.err(&["habit", "add", "--name", "x", "--color", "red"], 2);
    assert_eq!(error["code"], "HABIT_COLOR_INVALID");
    let error = env.err(&["habit", "add", "--name", "x", "--icon", "NotAnIcon"], 2);
    assert_eq!(error["code"], "HABIT_ICON_INVALID");
    let error = env.err(&["habit", "add", "--name", "x", "--time", "25:00"], 2);
    assert_eq!(error["code"], "HABIT_TIME_INVALID");
    let error = env.err(&["habit", "add", "--name", "   "], 2);
    assert_eq!(error["code"], "HABIT_NAME_INVALID");
    add(&env, &["--name", "刷牙"]);
    let error = env.err(&["habit", "add", "--name", "刷牙"], 2);
    assert_eq!(error["code"], "HABIT_EXISTS");
}

#[test]
fn check_uncheck_are_idempotent_and_respect_schedule() {
    let env = TestEnv::fresh();
    let brush = add(&env, &["--name", "刷牙"]);
    let id = brush["id"].as_str().unwrap();
    let day = today();

    let checked = env.ok(&["habit", "check", "--id", id]);
    assert_eq!(checked["checkins"], serde_json::json!([day.clone()]));
    let revision_after_check = env.read_file("data.json")["_meta"]["revision"].as_u64().unwrap();
    // 再打一次：幂等读路径，不写盘（revision 不动）
    env.ok(&["habit", "check", "--id", id]);
    assert_eq!(
        env.read_file("data.json")["_meta"]["revision"].as_u64().unwrap(),
        revision_after_check
    );
    let unchecked = env.ok(&["habit", "uncheck", "--id", id]);
    assert_eq!(unchecked["checkins"].as_array().map(Vec::len).unwrap_or(0), 0);

    // weekly：非计划日拒打
    let report = add(&env, &["--name", "周报", "--freq", "weekly", "--weekdays", "0"]);
    let rid = report["id"].as_str().unwrap();
    let weekday = chrono::Local::now().date_naive().weekday().num_days_from_sunday();
    if weekday != 0 {
        let error = env.err(&["habit", "check", "--id", rid], 2);
        assert_eq!(error["code"], "HABIT_NOT_SCHEDULED");
    }
    // 找一个周日打上去
    let mut sunday = chrono::Local::now().date_naive();
    while sunday.weekday().num_days_from_sunday() != 0 {
        sunday += chrono::Duration::days(1);
    }
    let sunday = sunday.format("%Y-%m-%d").to_string();
    let ok = env.ok(&["habit", "check", "--id", rid, "--date", &sunday]);
    assert_eq!(ok["checkins"], serde_json::json!([sunday]));

    // 暂停后不能打
    env.ok(&["habit", "modify", "--id", id, "--status", "paused"]);
    let error = env.err(&["habit", "check", "--id", id], 2);
    assert_eq!(error["code"], "HABIT_NOT_ACTIVE");
}

#[test]
fn modify_replaces_reminders_and_status() {
    let env = TestEnv::fresh();
    let habit = add(&env, &["--name", "锻炼", "--time", "19:00"]);
    let id = habit["id"].as_str().unwrap();
    let modified = env.ok(&["habit", "modify", "--id", id, "--reminder", "due-15", "--reminder", "due-0"]);
    assert_eq!(modified["reminders"].as_array().unwrap().len(), 2);
    let cleared = env.ok(&["habit", "modify", "--id", id, "--reminder"]);
    assert_eq!(cleared["reminders"].as_array().map(Vec::len).unwrap_or(0), 0);
    let paused = env.ok(&["habit", "modify", "--id", id, "--status", "paused"]);
    assert_eq!(paused["status"], "paused");
    // 没给的字段不动
    assert_eq!(paused["time"], "19:00");
    assert_eq!(paused["name"], "锻炼");
}

#[test]
fn remove_needs_confirmation_and_leaves_tombstone() {
    let env = TestEnv::fresh();
    let habit = add(&env, &["--name", "刷牙"]);
    let id = habit["id"].as_str().unwrap();
    env.ok(&["habit", "check", "--id", id]);
    env.err(&["habit", "remove", "--id", id], 10);
    env.ok(&["habit", "remove", "--id", id, "--yes"]);
    env.err(&["habit", "get", "--id", id], 3);
    let tombstones = env.read_file("data.json")["_meta"]["tombstones"].as_array().unwrap().clone();
    assert!(
        tombstones.iter().any(|tomb| tomb["id"] == id && tomb["type"] == "habit"),
        "删除必须留 kind=habit 墓碑供同步传播：{tombstones:?}"
    );
}

#[test]
fn habit_syncs_as_lww_entity_under_data_scope() {
    use kxtodo_core::model::DataFile;
    use kxtodo_core::sync::merge::{apply_data_record, data_entity_stamp, extract_entities, Scopes};

    let source: DataFile = serde_json::from_value(serde_json::json!({
        "schemaVersion": 6,
        "habits": [{
            "id": "habit-a",
            "name": "刷牙",
            "freq": { "kind": "daily", "weekdays": [] },
            "status": "active",
            "order": 1,
            "checkins": ["2026-11-18"],
            "createdAt": "2026-11-01T00:00:00.000Z",
            "updatedAt": "2026-11-18T08:00:00.000Z"
        }]
    }))
    .unwrap();
    let diary = Default::default();
    let ledger = Default::default();
    let calendar = Default::default();
    let settings = Default::default();
    let schedule = Default::default();
    let scopes = Scopes { data: true, ..Default::default() };
    let records = extract_entities(&source, &diary, &ledger, &calendar, &settings, &schedule, &scopes, "dev-a");
    let habit_record = records.iter().find(|record| record.kind == "habit").expect("data 范围应提取 habit 实体");
    assert_eq!(habit_record.id, "habit-a");
    assert_eq!(habit_record.data["checkins"], serde_json::json!(["2026-11-18"]));

    // 远端更新（多打了一天）胜出
    let mut target: DataFile = serde_json::from_value(serde_json::json!({
        "schemaVersion": 6,
        "habits": [{
            "id": "habit-a",
            "name": "刷牙",
            "freq": { "kind": "daily", "weekdays": [] },
            "status": "active",
            "order": 1,
            "checkins": [],
            "createdAt": "2026-11-01T00:00:00.000Z",
            "updatedAt": "2026-11-01T00:00:00.000Z"
        }]
    }))
    .unwrap();
    let mut remote = habit_record.clone();
    remote.updated_at = "2026-11-19T08:00:00.000Z".to_string();
    remote.data["checkins"] = serde_json::json!(["2026-11-18", "2026-11-19"]);
    remote.data["updatedAt"] = serde_json::json!("2026-11-19T08:00:00.000Z");
    apply_data_record(&remote, &mut target).unwrap();
    assert_eq!(target.habits[0].checkins, vec!["2026-11-18", "2026-11-19"]);
    assert_eq!(
        data_entity_stamp(&target, "habit-a").as_deref(),
        Some("2026-11-19T08:00:00.000Z")
    );

    // 墓碑传播删除
    let deleted = kxtodo_core::sync::merge::EntityRecord {
        kind: "habit".to_string(),
        id: "habit-a".to_string(),
        updated_at: "2026-11-20T00:00:00.000Z".to_string(),
        updated_by: "dev-b".to_string(),
        deleted: true,
        data: Value::Null,
        seq: 0,
    };
    apply_data_record(&deleted, &mut target).unwrap();
    assert!(target.habits.is_empty());
    assert!(target.meta.tombstones.iter().any(|tomb| tomb.id == "habit-a" && tomb.kind == "habit"));
    // 关掉 data 范围就不提取
    let off = Scopes { data: false, ..Default::default() };
    let none = extract_entities(&source, &diary, &ledger, &calendar, &settings, &schedule, &off, "dev-a");
    assert!(none.iter().all(|record| record.kind != "habit"));
}
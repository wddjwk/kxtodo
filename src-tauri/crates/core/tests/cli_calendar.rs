#![recursion_limit = "512"]
//! calendar domain end-to-end tests：日程的增删改查、全天/定时语义、提醒与关联任务。

mod common;

use common::TestEnv;
use serde_json::Value;

fn add(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["calendar", "add"];
    full.extend_from_slice(args);
    env.ok(&full)
}

#[test]
fn add_timed_event_defaults_end_to_one_hour_and_lists_by_start() {
    let env = TestEnv::fresh();

    let meeting = add(
        &env,
        &[
            "--title", "周会",
            "--start", "2026-10-25 09:00",
        ],
    );
    assert!(meeting["id"].as_str().unwrap().starts_with("cal-"));
    assert_eq!(meeting["startDate"], "2026-10-25");
    assert_eq!(meeting["startTime"], "09:00");
    assert_eq!(meeting["endDate"], "2026-10-25");
    assert_eq!(meeting["endTime"], "10:00", "定时事件缺省一小时");
    assert_eq!(meeting["allDay"], false);

    // --end 只给时刻：沿用开始日期
    let lunch = add(
        &env,
        &[
            "--title", "午饭",
            "--start", "2026-10-25T12:00",
            "--end", "13:30",
        ],
    );
    assert_eq!(lunch["endDate"], "2026-10-25");
    assert_eq!(lunch["endTime"], "13:30");

    // 跨午夜：开始 23:30 不给结束 → 进位到第二天 00:30
    let night = add(&env, &["--title", "夜班", "--start", "2026-10-24 23:30"]);
    assert_eq!(night["endDate"], "2026-10-25");
    assert_eq!(night["endTime"], "00:30");

    let listed = env.ok(&["calendar", "list"]);
    assert_eq!(listed["total"], 3);
    let ids: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    // 开始早的在前：24 日夜班 → 25 日周会 → 25 日午饭
    assert_eq!(ids, vec!["夜班", "周会", "午饭"]);

    // 区间过滤按交叠命中
    let range = env.ok(&["calendar", "list", "--from", "2026-10-25", "--to", "2026-10-25"]);
    assert_eq!(range["returned"], 3, "夜班跨到 25 日，交叠即命中");
    let one = env.ok(&["calendar", "list", "--date", "2026-10-24"]);
    assert_eq!(one["returned"], 1);
    assert_eq!(one["items"][0]["title"], "夜班");
}

#[test]
fn date_only_start_becomes_all_day_and_multi_day_range_is_inclusive() {
    let env = TestEnv::fresh();
    let trip = add(
        &env,
        &[
            "--title", "出差",
            "--start", "2026-11-01",
            "--end", "2026-11-03",
        ],
    );
    assert_eq!(trip["allDay"], true, "只给日期 = 全天事件");
    assert_eq!(trip["startTime"], "");
    assert_eq!(trip["endTime"], "");

    // 首尾两天都命中（endDate 是包含语义）
    for day in ["2026-11-01", "2026-11-02", "2026-11-03"] {
        let hit = env.ok(&["calendar", "list", "--date", day]);
        assert_eq!(hit["returned"], 1, "{day} 应命中出差");
    }
    let miss = env.ok(&["calendar", "list", "--date", "2026-11-04"]);
    assert_eq!(miss["returned"], 0);
}

#[test]
fn end_before_start_is_rejected() {
    let env = TestEnv::fresh();
    let error = env.err(
        &[
            "calendar", "add",
            "--title", "倒流",
            "--start", "2026-10-25 10:00",
            "--end", "2026-10-25 09:00",
        ],
        2,
    );
    assert_eq!(error["code"], "CALENDAR_RANGE_INVALID");
}

#[test]
fn modify_moves_keep_duration_and_toggles_all_day() {
    let env = TestEnv::fresh();
    let created = add(&env, &["--title", "评审", "--start", "2026-10-25 14:00"]);
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["endTime"], "15:00");

    // 只改开始时刻：结束跟着平移（挪动保持时长）
    let moved = env.ok(&["calendar", "modify", "--id", &id, "--start", "16:30"]);
    assert_eq!(moved["startDate"], "2026-10-25");
    assert_eq!(moved["startTime"], "16:30");
    assert_eq!(moved["endTime"], "17:30", "结束随开始平移，时长不变");

    // 起止一起给：显式值优先（改时长）
    let resized = env.ok(&[
        "calendar", "modify", "--id", &id,
        "--start", "2026-10-25 09:00",
        "--end", "12:00",
    ]);
    assert_eq!(resized["startTime"], "09:00");
    assert_eq!(resized["endTime"], "12:00");

    // 显式给倒挂的区间仍然拒绝
    let error = env.err(
        &[
            "calendar", "modify", "--id", &id,
            "--start", "2026-10-25 10:00",
            "--end", "09:00",
        ],
        2,
    );
    assert_eq!(error["code"], "CALENDAR_RANGE_INVALID");

    // 只改日期：整天平移，时刻不动
    let day_moved = env.ok(&["calendar", "modify", "--id", &id, "--start", "2026-10-26"]);
    assert_eq!(day_moved["startDate"], "2026-10-26");
    assert_eq!(day_moved["endDate"], "2026-10-26");
    assert_eq!(day_moved["startTime"], "09:00");
    assert_eq!(day_moved["endTime"], "12:00");

    // 切全天：时刻清空
    let all_day = env.ok(&["calendar", "modify", "--id", &id, "--all-day", "true"]);
    assert_eq!(all_day["allDay"], true);
    assert_eq!(all_day["startTime"], "");
    assert_eq!(all_day["endTime"], "");

    // 标题与详情：空串清除
    let titled = env.ok(&["calendar", "modify", "--id", &id, "--title", "", "--details", "备注"]);
    assert_eq!(titled["title"], "");
    assert_eq!(titled["details"], "备注");
}

#[test]
fn modify_moves_multi_day_all_day_event_by_day_span() {
    let env = TestEnv::fresh();
    let trip = add(
        &env,
        &["--title", "出差", "--start", "2026-11-01", "--end", "2026-11-03"],
    );
    let id = trip["id"].as_str().unwrap().to_string();
    let moved = env.ok(&["calendar", "modify", "--id", &id, "--start", "2026-11-10"]);
    assert_eq!(moved["startDate"], "2026-11-10");
    assert_eq!(moved["endDate"], "2026-11-12", "全天事件按天数差平移");
    assert_eq!(moved["allDay"], true);
}

#[test]
fn task_link_roundtrip_and_filter() {
    let env = TestEnv::fresh();
    let linked = add(
        &env,
        &[
            "--title", "交报告",
            "--start", "2026-10-25 09:00",
            "--task", "task-1",
        ],
    );
    assert_eq!(linked["taskId"], "task-1");
    add(&env, &["--title", "无关", "--start", "2026-10-25 11:00"]);

    let only = env.ok(&["calendar", "list", "--task", "task-1"]);
    assert_eq!(only["returned"], 1);
    assert_eq!(only["items"][0]["title"], "交报告");

    // 解除关联
    let id = linked["id"].as_str().unwrap().to_string();
    let unlinked = env.ok(&["calendar", "modify", "--id", &id, "--task", ""]);
    assert_eq!(unlinked["taskId"], "");
}

#[test]
fn reminders_reject_the_past_and_replace_as_a_whole() {
    let env = TestEnv::fresh();
    // 用足够远的未来日期，测试运行期间不会过期
    let created = add(
        &env,
        &[
            "--title", "发布会",
            "--start", "2099-01-01 10:00",
            "--reminder", "due-15",
            "--reminder", "due-60",
        ],
    );
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["reminders"].as_array().unwrap().len(), 2);

    // 整体替换：给一组就换掉全部
    let replaced = env.ok(&["calendar", "modify", "--id", &id, "--reminder", "due-5"]);
    let reminders = replaced["reminders"].as_array().unwrap();
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0]["minutes"], 5);

    // 清空：--reminder 不带值 = 空数组
    let cleared = env.ok(&["calendar", "modify", "--id", &id, "--reminder"]);
    assert!(cleared["reminders"].as_array().unwrap().is_empty());

    // 过去的提醒拒绝落盘（新规则必须在未来）
    let error = env.err(
        &[
            "calendar", "add",
            "--title", "过期提醒",
            "--start", "2099-01-01 10:00",
            "--reminder", "2020-01-01T00:00:00Z",
        ],
        2,
    );
    assert_eq!(error["code"], "REMINDER_IN_PAST");
}

#[test]
fn remove_requires_confirmation_and_records_tombstone() {
    let env = TestEnv::fresh();
    let created = add(&env, &["--title", "待删", "--start", "2026-10-25"]);
    let id = created["id"].as_str().unwrap().to_string();

    // 未带 --yes：确认门（退出码 10）
    env.err(&["calendar", "remove", "--id", &id], 10);

    env.ok(&["calendar", "remove", "--id", &id, "--yes"]);
    let listed = env.ok(&["calendar", "list"]);
    assert_eq!(listed["total"], 0);

    // 墓碑必须落盘（删除要传播到其它设备）
    let file = env.read_file("calendar.json");
    let tombstones = file["_meta"]["tombstones"].as_array().unwrap();
    assert!(tombstones.iter().any(|tomb| tomb["id"] == Value::String(id.clone())
        && tomb["type"] == Value::String("calendar".to_string())));

    // 删完再删：not found
    env.err(&["calendar", "remove", "--id", &id, "--yes"], 3);
}

#[test]
fn calendar_settings_view_and_accent_round_trip() {
    let env = TestEnv::fresh();
    env.ok(&["config", "set", "calendar.view", "threeday"]);
    env.ok(&["config", "set", "calendar.accent", "#7c5cff"]);
    let view = env.ok(&["config", "get", "calendar.view"]);
    assert_eq!(view["value"], "threeday");
    let accent = env.ok(&["config", "get", "calendar.accent"]);
    assert_eq!(accent["value"], "#7c5cff");

    // 非法视图名拒绝
    let error = env.err(&["config", "set", "calendar.view", "year"], 2);
    assert!(error["message"].as_str().unwrap().contains("month/week/threeday/day"));

    // view 不进同步共享子集（每台设备各看各的），accent 进
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("calendar.view"));
    assert!(kxtodo_core::ops_config::is_shared_settings_path("calendar.accent"));
}

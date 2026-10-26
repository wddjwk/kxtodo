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

// ---------------------------------------------------------------------------
// v0.9.1：分类 / 重复 / 按次删除 / 地点
// ---------------------------------------------------------------------------

fn category(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["calendar", "category-add"];
    full.extend_from_slice(args);
    env.ok(&full)
}

#[test]
fn category_crud_and_event_reference_round_trip() {
    let env = TestEnv::fresh();

    let work = category(&env, &["--name", "工作", "--color", "#1F6FEB"]);
    assert!(work["id"].as_str().unwrap().starts_with("ccat-"));
    assert_eq!(work["color"], "#1f6feb", "颜色归一成小写");
    assert_eq!(work["isDefault"], false);

    // 重名拒绝；非法颜色拒绝；空名拒绝
    assert_eq!(
        env.err(&["calendar", "category-add", "--name", "工作"], 2)["code"],
        "CALENDAR_CATEGORY_EXISTS"
    );
    assert_eq!(
        env.err(&["calendar", "category-add", "--name", "红", "--color", "red"], 2)["code"],
        "CALENDAR_CATEGORY_COLOR_INVALID"
    );
    assert_eq!(
        env.err(&["calendar", "category-add", "--name", "   "], 2)["code"],
        "CALENDAR_CATEGORY_NAME_EMPTY"
    );

    let personal = category(&env, &["--name", "个人", "--color", "#d97706", "--is-default"]);
    assert_eq!(personal["isDefault"], true);

    // 事件按名字或 ID 挂分类都行
    let by_name = add(&env, &["--title", "写周报", "--start", "2026-10-25 09:00", "--category", "工作"]);
    assert_eq!(by_name["categoryId"], work["id"]);
    let by_id = add(
        &env,
        &[
            "--title", "陪家人",
            "--start", "2026-10-25 18:00",
            "--category", personal["id"].as_str().unwrap(),
            "--location", "家里",
        ],
    );
    assert_eq!(by_id["categoryId"], personal["id"]);
    assert_eq!(by_id["location"], "家里");
    // 不存在的分类当场报错，不静默落成空
    assert_eq!(
        env.err(
            &["calendar", "add", "--title", "x", "--start", "2026-10-25", "--category", "没有这个"],
            3
        )["code"],
        "CALENDAR_CATEGORY_NOT_FOUND"
    );

    // categories 带使用计数
    let list = env.ok(&["calendar", "categories"]);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let used = |name: &str| -> u64 {
        items
            .iter()
            .find(|item| item["name"] == name)
            .unwrap()["used"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(used("工作"), 1);
    assert_eq!(used("个人"), 1);

    // list 能按分类过滤
    let filtered = env.ok(&["calendar", "list", "--category", "工作"]);
    assert_eq!(filtered["total"].as_u64().unwrap(), 1);
    assert_eq!(filtered["items"][0]["title"], "写周报");

    // 「设为默认」全局互斥
    env.ok(&[
        "calendar", "category-modify",
        "--id", work["id"].as_str().unwrap(),
        "--is-default",
    ]);
    let after = env.ok(&["calendar", "categories"]);
    let defaults: Vec<&str> = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["isDefault"] == Value::Bool(true))
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(defaults, vec!["工作"]);

    // 改名撞车拒绝；改色生效
    assert_eq!(
        env.err(
            &[
                "calendar", "category-modify",
                "--id", work["id"].as_str().unwrap(),
                "--name", "个人",
            ],
            2
        )["code"],
        "CALENDAR_CATEGORY_EXISTS"
    );
    env.ok(&[
        "calendar", "category-modify",
        "--id", work["id"].as_str().unwrap(),
        "--color", "#2f9e6e",
    ]);
    assert_eq!(
        env.ok(&["calendar", "categories"])["items"][0]["color"],
        "#2f9e6e"
    );

    // 删分类要确认；删完事件退回无分类（事件本身不动）
    env.err(
        &["calendar", "category-remove", "--id", work["id"].as_str().unwrap()],
        10,
    );
    let removed = env.ok(&[
        "calendar", "category-remove",
        "--id", work["id"].as_str().unwrap(),
        "--yes",
    ]);
    assert_eq!(removed["removed"]["detachedEvents"], 1);
    let still_there = env.ok(&["calendar", "get", "--id", by_name["id"].as_str().unwrap()]);
    assert_eq!(still_there["title"], "写周报");
    assert_eq!(still_there["categoryId"], "", "删分类不删日程，退回无分类");
    // 删除写墓碑（同步要能传播）
    let file = env.read_file("calendar.json");
    assert!(file["_meta"]["tombstones"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tomb| tomb["id"] == work["id"] && tomb["type"] == "calendarCategory"));
}

#[test]
fn repeat_presets_expand_in_list_and_can_be_turned_off() {
    let env = TestEnv::fresh();

    let weekly = add(
        &env,
        &[
            "--title", "周会",
            "--start", "2026-10-07 09:00",
            "--end", "09:30",
            "--repeat", "weekly",
        ],
    );
    assert_eq!(weekly["repeat"]["freq"], "week");
    assert_eq!(weekly["repeat"]["interval"], 1);
    // 空集合在 JSON 里被 skip_serializing_if 省掉了：null 与 [] 是同一个意思
    assert!(
        weekly["repeat"]["byWeekday"].is_null()
            || weekly["repeat"]["byWeekday"].as_array().unwrap().is_empty(),
        "「每周」不写死星期，跟开始日期的星期走：{}", weekly["repeat"]
    );
    assert_eq!(weekly["repeatText"], "每周");

    // 闭区间逐次展开：10 月里 7/14/21/28 四次
    let expanded = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31"]);
    let dates: Vec<&str> = expanded["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-10-07", "2026-10-14", "2026-10-21", "2026-10-28"]);
    assert_eq!(expanded["total"].as_u64().unwrap(), 4);
    assert_eq!(expanded["items"][0]["recurring"], true);
    assert_eq!(expanded["items"][1]["startDate"], "2026-10-14", "实例日期已平移");
    assert_eq!(expanded["items"][1]["startTime"], "09:00");
    assert_eq!(expanded["items"][1]["endTime"], "09:30");
    // 系列本身的 id 每条都一样（改的是系列，不是副本）
    assert!(expanded["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["id"] == weekly["id"]));

    // --no-expand 只看系列本身
    let series = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31", "--no-expand"]);
    assert_eq!(series["total"].as_u64().unwrap(), 1);
    assert_eq!(series["items"][0]["occurrence"], "2026-10-07");

    // 只给 --from（开区间）按一年为界展开：既看得到后面的实例，也不会展开到永远
    let open = env.ok(&["calendar", "list", "--from", "2026-10-20"]);
    let open_dates: Vec<&str> = open["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(open_dates[0], "2026-10-21", "从 --from 之后的第一次开始");
    assert!(open_dates.len() > 40 && open_dates.len() < 60, "一年为界：{}", open_dates.len());

    // 工作日预设 = 周一到周五
    let workdays = add(&env, &["--title", "站会", "--start", "2026-10-07 09:30", "--repeat", "weekdays"]);
    assert_eq!(workdays["repeat"]["byWeekday"], serde_json::json!([1, 2, 3, 4, 5]));
    assert_eq!(workdays["repeatText"], "每周的周一、二、三、四、五");

    // 自定义：每 2 周的周一与周四，共 4 次
    let custom = add(
        &env,
        &[
            "--title", "复盘",
            "--start", "2026-10-07",
            "--repeat", "week",
            "--repeat-every", "2",
            "--repeat-days", "1,4",
            "--repeat-times", "4",
        ],
    );
    assert_eq!(custom["repeat"]["interval"], 2);
    assert_eq!(custom["repeat"]["end"], "count");
    assert_eq!(custom["repeat"]["count"], 4);
    assert_eq!(custom["repeatText"], "每 2 周的周一、四（共 4 次）");
    let custom_list = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-12-31", "--task", ""]);
    let custom_dates: Vec<&str> = custom_list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["title"] == "复盘")
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(
        custom_dates,
        vec!["2026-10-08", "2026-10-19", "2026-10-22", "2026-11-02"],
        "次数名额是全局的，第 4 次之后就没有了"
    );

    // 细化键必须和 --repeat 一起给
    assert_eq!(
        env.err(&["calendar", "add", "--title", "x", "--start", "2026-10-07", "--repeat-days", "1"], 2)
            ["code"],
        "CLI_ARGUMENT_ERROR"
    );
    // 认不出的预设
    assert_eq!(
        env.err(&["calendar", "add", "--title", "x", "--start", "2026-10-07", "--repeat", "hourly"], 2)
            ["code"],
        "CALENDAR_REPEAT_INVALID"
    );

    // 取消重复：--repeat none（连带清掉例外日期）
    let cleared = env.ok(&[
        "calendar", "modify",
        "--id", weekly["id"].as_str().unwrap(),
        "--repeat", "none",
    ]);
    assert!(cleared["repeat"].is_null());
    assert_eq!(cleared["repeatText"], "");
    let after = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31"]);
    assert!(after["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["title"] == "周会")
        .count() == 1);
}

#[test]
fn monthly_and_yearly_skip_impossible_days() {
    let env = TestEnv::fresh();
    add(&env, &[
        "--title", "月底",
        "--start", "2026-01-31",
        "--repeat", "month",
        "--repeat-month-days", "31",
    ]);
    let list = env.ok(&["calendar", "list", "--from", "2026-01-01", "--to", "2026-06-30"]);
    let dates: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-01-31", "2026-03-31", "2026-05-31"], "2/4/6 月没有 31 日 → 跳过");

    add(&env, &["--title", "生日", "--start", "2024-02-29", "--repeat", "yearly"]);
    let leap = env.ok(&["calendar", "list", "--from", "2024-01-01", "--to", "2032-12-31"]);
    let birthdays: Vec<&str> = leap["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["title"] == "生日")
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(birthdays, vec!["2024-02-29", "2028-02-29", "2032-02-29"]);
}

#[test]
fn remove_by_occurrence_and_tail_keeps_the_series() {
    let env = TestEnv::fresh();
    let daily = add(&env, &[
        "--title", "吃药",
        "--start", "2026-10-01 08:00",
        "--repeat", "daily",
    ]);
    let id = daily["id"].as_str().unwrap();

    // 只删一次
    env.err(&["calendar", "remove", "--id", id, "--occurrence", "2026-10-05"], 10);
    env.ok(&["calendar", "remove", "--id", id, "--occurrence", "2026-10-05", "--yes"]);
    let after = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-07"]);
    let dates: Vec<&str> = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-06", "2026-10-07"]);
    assert_eq!(
        env.ok(&["calendar", "get", "--id", id])["exceptDates"],
        serde_json::json!(["2026-10-05"])
    );

    // 两个 scope 互斥：clap 先拦下来（core 里那层 CALENDAR_OCCURRENCE_CONFLICT 是给 GUI/agent 的兜底）
    let conflicted = env.run(&[
        "calendar", "remove", "--id", id,
        "--occurrence", "2026-10-06",
        "--from-occurrence", "2026-10-07",
        "--yes",
    ]);
    assert_ne!(conflicted.code, 0, "同时给两个 scope 必须被拒");
    let still = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-07"]);
    assert_eq!(still["total"].as_u64().unwrap(), 6, "被拒的那次不该动数据");

    // 此次及以后：把重复截止日期收到前一天
    env.ok(&["calendar", "remove", "--id", id, "--from-occurrence", "2026-10-07", "--yes"]);
    let truncated = env.ok(&["calendar", "get", "--id", id]);
    assert_eq!(truncated["repeat"]["end"], "until");
    assert_eq!(truncated["repeat"]["until"], "2026-10-06");
    let tail = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31"]);
    let tail_dates: Vec<&str> = tail["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(
        tail_dates,
        vec!["2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-06"]
    );

    // 从第一次开始「此次及以后」= 整个系列删掉（写墓碑）
    env.ok(&["calendar", "remove", "--id", id, "--from-occurrence", "2026-10-01", "--yes"]);
    assert_eq!(
        env.err(&["calendar", "get", "--id", id], 3)["code"],
        "CALENDAR_NOT_FOUND"
    );

    // 不重复的日程不许按次删
    let plain = add(&env, &["--title", "一次性的事", "--start", "2026-11-01"]);
    assert_eq!(
        env.err(
            &[
                "calendar", "remove",
                "--id", plain["id"].as_str().unwrap(),
                "--occurrence", "2026-11-01",
                "--yes",
            ],
            2
        )["code"],
        "CALENDAR_NOT_RECURRING"
    );
}

#[test]
fn modify_replaces_repeat_and_moves_the_series() {
    let env = TestEnv::fresh();
    let weekly = add(&env, &[
        "--title", "周会",
        "--start", "2026-10-07 09:00",
        "--end", "10:00",
        "--repeat", "weekly",
    ]);
    let id = weekly["id"].as_str().unwrap();

    // 改到别的时间：整个系列跟着改，星期集合也跟着换（前端拖动就是这么发的）
    let moved = env.ok(&[
        "calendar", "modify", "--id", id,
        "--start", "2026-10-08 14:00",
        "--end", "15:30",
        "--repeat", "week",
        "--repeat-days", "4",
    ]);
    assert_eq!(moved["startDate"], "2026-10-08");
    assert_eq!(moved["startTime"], "14:00");
    assert_eq!(moved["endTime"], "15:30");
    assert_eq!(moved["repeat"]["byWeekday"], serde_json::json!([4]));
    let list = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31"]);
    let dates: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-10-08", "2026-10-15", "2026-10-22", "2026-10-29"], "改成每周四");

    // 例外日期整体替换
    env.ok(&[
        "calendar", "modify", "--id", id,
        "--except-dates", "2026-10-15,2026-10-22",
    ]);
    let excepted = env.ok(&["calendar", "list", "--from", "2026-10-01", "--to", "2026-10-31"]);
    let kept: Vec<&str> = excepted["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["occurrence"].as_str().unwrap())
        .collect();
    assert_eq!(kept, vec!["2026-10-08", "2026-10-29"]);
}

#[test]
fn hour_zoom_is_a_local_preference_with_bounds() {
    let env = TestEnv::fresh();
    assert_eq!(env.ok(&["config", "get", "calendar.hourZoom"])["value"], 1.0);
    env.ok(&["config", "set", "calendar.hourZoom", "1.75"]);
    assert_eq!(env.ok(&["config", "get", "calendar.hourZoom"])["value"], 1.75);
    for bad in ["0.1", "9", "\"2\""] {
        let value = serde_json::from_str::<Value>(bad).unwrap();
        let args = vec![
            "config".to_string(),
            "set".to_string(),
            "calendar.hourZoom".to_string(),
            value.to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(
            env.err(&refs, 2)["code"],
            "INVALID_CONFIG_VALUE",
            "{bad} 该被拒"
        );
    }
    // 本机偏好：不进同步的共享子集（换设备不该带着别人的缩放）
    let stored: kxtodo_core::model::SettingsFile =
        serde_json::from_value(env.read_file("settings.json")).unwrap();
    let shared = kxtodo_core::sync::merge::settings_payload(&stored);
    let calendar = shared.get("calendar").unwrap();
    assert!(calendar.get("hourZoom").is_none(), "缩放是本机偏好，不该进同步：{calendar}");
    assert!(calendar.get("view").is_none(), "视图同理");
    assert!(calendar.get("accent").is_some(), "外观仍然共享");
}

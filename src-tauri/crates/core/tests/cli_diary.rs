#![recursion_limit = "512"]
//! diary domain end-to-end tests：日记的增删改查与视图偏好。

mod common;

use common::TestEnv;
use kxtodo_core::core::Invocation;
use serde_json::Value;

fn add(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["diary", "add"];
    full.extend_from_slice(args);
    env.ok(&full)
}

#[test]
fn add_defaults_to_local_today_and_lists_newest_first() {
    let env = TestEnv::fresh();

    let today = add(&env, &["--markdown", "今天的第一篇"]);
    assert_eq!(
        today["date"].as_str().unwrap(),
        kxtodo_core::time::today_local(),
        "不给 --date 就归到本地今天（不能用 UTC，否则东八区凌晨会算成昨天）"
    );
    assert!(today["id"].as_str().unwrap().starts_with("diary-"));
    assert_eq!(today["title"], "", "视图里字段一律给出，没写标题就是空串");

    add(&env, &["--date", "2026-09-01", "--title", "开学", "--markdown", "补写的一篇"]);
    add(&env, &["--markdown", "今天的第二篇"]);

    let listed = env.ok(&["diary", "list"]);
    assert_eq!(listed["total"], 3);
    let dates: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["date"].as_str().unwrap())
        .collect();
    // 日期由近及远；同一天内按写作先后
    assert_eq!(dates[0], dates[1], "今天的两篇排在一起");
    assert_eq!(dates[2], "2026-09-01");
    assert!(
        listed["items"][0]["markdown"]
            .as_str()
            .unwrap()
            .contains("第一篇"),
        "同一天里先写的在上面"
    );

    // 日期过滤
    let one_day = env.ok(&["diary", "list", "--date", "2026-09-01"]);
    assert_eq!(one_day["returned"], 1);
    let range = env.ok(&["diary", "list", "--from", "2026-09-01", "--to", "2026-09-01"]);
    assert_eq!(range["returned"], 1);
    let limited = env.ok(&["diary", "list", "--limit", "2"]);
    assert_eq!(limited["returned"], 2);
    assert_eq!(limited["total"], 3, "total 说的是命中的全部，不是这一页");
}

#[test]
fn modify_changes_fields_clears_with_empty_string_and_bumps_updated_at() {
    let env = TestEnv::fresh();
    let created = add(&env, &["--markdown", "原文", "--mood", "🙂", "--tag", "blue:工作"]);
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["updatedAt"], created["createdAt"]);

    let modified = env.ok(&[
        "diary", "modify", "--id", &id,
        "--title", "改过的标题",
        "--date", "2026-08-15",
        "--markdown", "改过的正文",
        "--weather", "☀️",
        "--replace-tags", "green:旅行",
    ]);
    assert_eq!(modified["title"], "改过的标题");
    assert_eq!(modified["date"], "2026-08-15");
    assert_eq!(modified["markdown"], "改过的正文");
    assert_eq!(modified["mood"], "🙂", "没提到的字段不动");
    assert_eq!(modified["weather"], "☀️");
    assert_eq!(modified["tags"][0]["text"], "旅行");
    assert_ne!(
        modified["updatedAt"], created["updatedAt"],
        "有效修改必须刷新 updatedAt（同步的 LWW 键）"
    );

    // 空串 = 清除（心情/天气/标题都是可选字段）
    let cleared = env.ok(&["diary", "modify", "--id", &id, "--mood", ""]);
    assert!(cleared["mood"].is_null() || cleared["mood"] == Value::String(String::new()));

    let got = env.ok(&["diary", "get", "--id", &id]);
    assert_eq!(got["date"], "2026-08-15");

    env.err(&["diary", "modify", "--id", "diary-nope", "--title", "x"], 3);
}

#[test]
fn add_rejects_empty_content_and_bad_dates() {
    let env = TestEnv::fresh();
    let empty = env.err(&["diary", "add", "--markdown", "   "], 2);
    assert_eq!(empty["code"], "DIARY_EMPTY");

    // 只有标题也算有内容
    let title_only = add(&env, &["--title", "只有一句话的标题"]);
    assert!(title_only["id"].as_str().unwrap().starts_with("diary-"));

    let bad_date = env.err(&["diary", "add", "--date", "2026-13-45", "--markdown", "x"], 2);
    assert_eq!(bad_date["code"], "INVALID_DATE");

    // 相对日期写法与 task 域一致
    let relative = add(&env, &["--date", "+1d", "--markdown", "明天的预感"]);
    assert!(relative["date"].as_str().unwrap().starts_with("20"));
    assert_ne!(relative["date"], title_only["date"]);
}

#[test]
fn remove_is_gated_by_yes_and_writes_a_sync_tombstone() {
    let env = TestEnv::fresh();
    let created = add(&env, &["--date", "2026-09-01", "--markdown", "要被删掉的一篇"]);
    let id = created["id"].as_str().unwrap().to_string();

    // 高风险写：没有 --yes 就是退出码 10
    let refused = env.err(&["diary", "remove", "--id", &id], 10);
    assert_eq!(refused["code"], "CONFIRMATION_REQUIRED");

    let dry = env.ok(&["diary", "remove", "--id", &id, "--dry-run"]);
    assert_eq!(dry["plan"]["type"], "diary");
    assert_eq!(env.ok(&["diary", "list"])["total"], 1, "dry-run 不动数据");

    let removed = env.ok(&["diary", "remove", "--id", &id, "--yes"]);
    assert_eq!(removed["removed"]["diaryIds"][0], Value::String(id.clone()));
    assert_eq!(env.ok(&["diary", "list"])["total"], 0);

    // 删除必须显式传播：服务器只见密文，没有墓碑对端会把它推回来。
    // 墓碑记在 diary 域自己的 _meta 里（diary.json），不在 data.json。
    let diary = env.read_file("diary.json");
    let tombstones = diary["_meta"]["tombstones"].as_array().unwrap();
    assert!(
        tombstones
            .iter()
            .any(|tomb| tomb["id"] == Value::String(id.clone()) && tomb["type"] == "diary"),
        "缺少 diary 墓碑：{tombstones:?}"
    );

    env.err(&["diary", "remove", "--id", &id, "--yes"], 3);
}

#[test]
fn gui_expanded_state_does_not_touch_updated_at() {
    let env = TestEnv::fresh();
    let created = add(&env, &["--markdown", "展开我"]);
    let id = created["id"].as_str().unwrap().to_string();

    // gui.* 是内部命名空间（没有 CLI 面），直接过核心执行器
    let repo = kxtodo_core::repo::Repository::open(env.path()).unwrap();
    let ctx = kxtodo_core::core::ExecContext {
        repo: &repo,
        cwd: env.path(),
        host: None,
        custom_data_dir: true,
    };
    let outcome = kxtodo_core::core::execute(
        &Invocation::new(
            "gui.set-diary-ui",
            serde_json::json!({ "id": id, "expanded": true }),
        ),
        &ctx,
    );
    assert_eq!(outcome.code, 0, "{:?}", outcome.envelope);
    assert_eq!(outcome.envelope["data"]["expanded"], true);

    let got = env.ok(&["diary", "get", "--id", &id]);
    assert_eq!(got["expanded"], true);
    assert_eq!(
        got["updatedAt"], created["updatedAt"],
        "展开是本机 UI 状态，不该刷新 updatedAt（否则每次展开都被推到别的设备）"
    );
}

#[test]
fn diary_view_preference_is_a_local_setting() {
    let env = TestEnv::fresh();
    assert_eq!(env.ok(&["config", "get", "diary.view"])["value"], "list");

    assert_eq!(
        env.ok(&["config", "set", "diary.view", "calendar"])["value"],
        "calendar"
    );
    assert_eq!(env.ok(&["config", "get", "diary.view"])["value"], "calendar");

    let bad = env.err(&["config", "set", "diary.view", "nonsense"], 2);
    assert_eq!(bad["code"], "INVALID_CONFIG_VALUE");

    // 本机偏好：不进设置同步的共享子集（否则一台设备切视图会把别的设备也切走）
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("diary.view"));

    env.ok(&["config", "reset", "diary.view", "--yes"]);
    assert_eq!(env.ok(&["config", "get", "diary.view"])["value"], "list");
}

#[test]
fn export_then_import_round_trips_and_same_day_just_adds_another_entry() {
    let env = TestEnv::fresh();
    add(&env, &["--date", "2026-09-08", "--title", "早上的想法", "--markdown", "早上的正文", "--mood", "🙂", "--tag", "blue:工作"]);
    add(&env, &["--date", "2026-09-08", "--markdown", "没标题的第二篇"]);
    add(&env, &["--date", "2026-01-05", "--title", "跨年", "--markdown", "跨年的正文", "--weather", "☀️"]);

    let zip = env.path().join("export.zip");
    let zip_path = zip.to_string_lossy().to_string();
    let exported = env.ok(&["diary", "export", "--out", &zip_path]);
    assert_eq!(exported["entries"], 3);
    assert!(zip.is_file(), "压缩包没落盘");

    // 日期范围导出：只要 9 月那两篇
    let partial = env.path().join("sept.zip");
    let partial_path = partial.to_string_lossy().to_string();
    let ranged = env.ok(&[
        "diary", "export", "--out", &partial_path,
        "--from", "2026-09-01", "--to", "2026-09-30",
    ]);
    assert_eq!(ranged["entries"], 2);

    // 导进一个空数据目录：三篇都在，元数据一个不少
    let target = TestEnv::fresh();
    let target_zip = target.path().join("export.zip");
    std::fs::copy(&zip, &target_zip).unwrap();
    let target_path = target_zip.to_string_lossy().to_string();
    target.err(&["diary", "import", "--zip", &target_path], 10);
    let imported = target.ok(&["diary", "import", "--zip", &target_path, "--yes"]);
    assert_eq!(imported["imported"], 3);
    assert_eq!(imported["skipped"], 0);

    let listed = target.ok(&["diary", "list"]);
    assert_eq!(listed["total"], 3);
    let items = listed["items"].as_array().unwrap();
    let morning = items
        .iter()
        .find(|item| item["title"] == "早上的想法")
        .expect("带标题的那篇要能找回来");
    assert_eq!(morning["date"], "2026-09-08");
    assert_eq!(morning["markdown"], "早上的正文");
    assert_eq!(morning["mood"], "🙂");
    assert_eq!(morning["tags"][0]["color"], "blue");
    assert_eq!(morning["tags"][0]["text"], "工作");
    let new_year = items.iter().find(|item| item["title"] == "跨年").unwrap();
    assert_eq!(new_year["weather"], "☀️");

    // 同一天已有日记不是冲突：再导一遍就是再多三篇，不合并正文也不去重
    let again = target.ok(&["diary", "import", "--zip", &target_path, "--yes"]);
    assert_eq!(again["imported"], 3);
    assert_eq!(target.ok(&["diary", "list"])["total"], 6);
    let same_day = target.ok(&["diary", "list", "--date", "2026-09-08"]);
    assert_eq!(same_day["returned"], 4, "9 月 8 日现在应该有 4 篇");
}

#[test]
fn diary_images_travel_with_the_archive() {
    let env = TestEnv::fresh();
    let image_dir = env.path().join("img").join("data").join("diary");
    std::fs::create_dir_all(&image_dir).unwrap();
    let png: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 7, 7, 7];
    std::fs::write(image_dir.join("md-42.png"), &png).unwrap();
    add(&env, &["--date", "2026-09-08", "--title", "带图", "--markdown", "看图：\n\n![截图](md-42.png)"]);

    let zip = env.path().join("with-images.zip");
    let zip_path = zip.to_string_lossy().to_string();
    env.ok(&["diary", "export", "--out", &zip_path]);

    let target = TestEnv::fresh();
    let target_zip = target.path().join("with-images.zip");
    std::fs::copy(&zip, &target_zip).unwrap();
    let target_path = target_zip.to_string_lossy().to_string();
    let imported = target.ok(&["diary", "import", "--zip", &target_path, "--yes"]);
    assert_eq!(imported["imported"], 1);
    assert_eq!(imported["images"], 1, "插图要随包落盘并计数");

    let restored = target.path().join("img").join("data").join("diary").join("md-42.png");
    assert_eq!(std::fs::read(restored).unwrap(), png, "插图要跟着包落回目标数据目录");

    let listed = target.ok(&["diary", "list"]);
    let markdown = listed["items"][0]["markdown"].as_str().unwrap();
    assert!(markdown.contains("![截图](md-42.png)"), "引用归一回裸文件名：{markdown}");

    // 重导同一个包：图已存在就不覆盖、不重复计数（图片是内容寻址的不可变 blob）
    let again = target.ok(&["diary", "import", "--zip", &target_path, "--yes"]);
    assert_eq!(again["images"], 0);
}

#[test]
fn time_param_lands_in_created_at_and_date_moves_preserve_the_clock() {
    use chrono::{DateTime, Local};
    let env = TestEnv::fresh();
    let clock_of = |iso: &str| {
        DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&Local)
            .format("%H:%M")
            .to_string()
    };
    let date_of = |iso: &str| {
        DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string()
    };

    // add --time：createdAt 的钟点就是给的时刻（秒被舍掉），日期部分 = 归属日期
    let created = add(&env, &[
        "--date", "2026-09-01", "--time", "14:30:05", "--markdown", "下午写的一篇",
    ]);
    let id = created["id"].as_str().unwrap().to_string();
    let created_at = created["createdAt"].as_str().unwrap();
    assert_eq!(clock_of(created_at), "14:30");
    assert_eq!(date_of(created_at), "2026-09-01");
    assert_eq!(created["date"], "2026-09-01");

    // 只改日期：createdAt 的日期部分跟着走、钟点保留（导出 front-matter 的 date/time 靠这对齐）
    let moved = env.ok(&["diary", "modify", "--id", &id, "--date", "2026-09-05"]);
    let moved_at = moved["createdAt"].as_str().unwrap();
    assert_eq!(date_of(moved_at), "2026-09-05");
    assert_eq!(clock_of(moved_at), "14:30", "改日期不许弄丢写作时刻");

    // 只改时刻：日期部分保留
    let retimed = env.ok(&["diary", "modify", "--id", &id, "--time", "08:05"]);
    let retimed_at = retimed["createdAt"].as_str().unwrap();
    assert_eq!(clock_of(retimed_at), "08:05");
    assert_eq!(date_of(retimed_at), "2026-09-05");

    // 日期 + 时刻一起改
    let both = env.ok(&["diary", "modify", "--id", &id, "--date", "2026-10-01", "--time", "21:00"]);
    let both_at = both["createdAt"].as_str().unwrap();
    assert_eq!(date_of(both_at), "2026-10-01");
    assert_eq!(clock_of(both_at), "21:00");

    // updatedAt 每次 modify 都自动刷新（用户口中的「编辑时间」）
    assert_ne!(both["updatedAt"], retimed["updatedAt"]);
    assert_ne!(retimed["updatedAt"], moved["updatedAt"]);

    // 非法时刻拒绝，createdAt 不动
    env.err(&["diary", "modify", "--id", &id, "--time", "24:00"], 2);
    env.err(&["diary", "add", "--time", "noon", "--markdown", "x"], 2);
    let unchanged = env.ok(&["diary", "get", "--id", &id]);
    assert_eq!(unchanged["createdAt"].as_str().unwrap(), both_at);

    // 不给 --time 的新日记：createdAt 仍是「现在」，与 updatedAt 同值
    let plain = add(&env, &["--markdown", "没给时刻的一篇"]);
    assert_eq!(plain["createdAt"], plain["updatedAt"]);
}

#[test]
fn list_returns_everything_by_default_and_double_writes_paging_meta() {
    let env = TestEnv::fresh();
    for day in 1..=4 {
        let date = format!("2026-09-0{day}");
        add(&env, &["--date", &date, "--markdown", "一篇"]);
    }

    // 不传 --limit 就返回全部（月度回顾要的是整月，不是「前 50 篇」）
    let all = env.run(&["diary", "list"]);
    assert_eq!(all.code, 0, "{}", all.stderr);
    let envelope = all.envelope();
    assert_eq!(envelope["data"]["total"], 4);
    assert_eq!(envelope["data"]["returned"], 4);
    // meta 双写同一份分页信息：render 的人类可读输出（table/pretty）只认 meta.count
    assert_eq!(envelope["meta"]["count"], 4);
    assert_eq!(envelope["meta"]["nextCursor"], Value::Null);

    // --limit + --cursor 与 task list 同一套语义
    let page = env.run(&["diary", "list", "--limit", "3"]);
    let envelope = page.envelope();
    assert_eq!(envelope["data"]["returned"], 3);
    assert_eq!(envelope["data"]["total"], 4, "data.total 的语义没动");
    assert_eq!(envelope["meta"]["count"], 4);
    assert_eq!(envelope["meta"]["nextCursor"], "3");
    let rest = env.ok(&["diary", "list", "--limit", "3", "--cursor", "3"]);
    assert_eq!(rest["returned"], 1, "游标接着翻，只剩一篇");

    // --all 忽略分页
    assert_eq!(env.ok(&["diary", "list", "--limit", "1", "--all"])["returned"], 4);

    // data.total 是**过滤后**的篇数（与 ledger list 同口径）：「这个月写了几篇」要能直接读它
    let one_day = env.ok(&["diary", "list", "--date", "2026-09-01"]);
    assert_eq!(one_day["returned"], 1);
    assert_eq!(one_day["total"], 1);
    let range = env.ok(&["diary", "list", "--from", "2026-09-02", "--to", "2026-09-03"]);
    assert_eq!(range["total"], 2, "区间过滤同样收窄 total");
    assert_eq!(range["returned"], 2);

    // 游标非法 → 退出码 2
    assert_eq!(env.err(&["diary", "list", "--cursor", "abc"], 2)["code"], "INVALID_CURSOR");

    // --format pretty 的合计行回来了（此前 meta 里没有 count，只有键值兜底）
    let pretty = env.run(&["diary", "list", "--format", "pretty"]);
    assert_eq!(pretty.code, 0, "{}", pretty.stderr);
    assert!(pretty.stdout.contains("-- 共 4 条"), "{}", pretty.stdout);
}

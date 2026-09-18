#![recursion_limit = "512"]
//! config domain tests (§3.6).

mod common;

use common::TestEnv;
use serde_json::json;

#[test]
fn get_set_roundtrip_and_source() {
    let env = TestEnv::fresh();
    let got = env.ok(&["config", "get", "appearance.uiScale"]);
    assert_eq!(got["value"], 0.75);
    assert_eq!(got["source"], "default");

    let set = env.ok(&["config", "set", "appearance.uiScale", "0.85"]);
    assert_eq!(set["value"], 0.85);
    assert!(set["nativeEffects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|effect| effect["name"] == "webviewZoom"));

    let got = env.ok(&["config", "get", "appearance.uiScale"]);
    assert_eq!(got["value"], 0.85);
    assert_eq!(got["source"], "user");
}

#[test]
fn set_validation() {
    let env = TestEnv::fresh();
    env.err(&["config", "set", "appearance.uiScale", "5.0"], 2);
    env.err(&["config", "set", "appearance.uiScale", "abc"], 2);
    env.err(&["config", "set", "nope.key", "1"], 2);
    env.err(&["config", "set", "notifications.position", "middle"], 2);
    env.err(&["config", "set", "appearance.uiColors", "red"], 2);
    // 缺少值
    env.err(&["config", "set", "appearance.uiScale"], 2);
}

#[test]
fn map_key_operations() {
    let env = TestEnv::fresh();
    // 需要 --map-key
    env.err(&["config", "get", "appearance.uiColors"], 2);

    env.ok(&[
        "config",
        "set",
        "appearance.uiColors",
        "#dfe8df",
        "--map-key",
        "entry-abc.def",
    ]);
    let got = env.ok(&[
        "config",
        "get",
        "appearance.uiColors",
        "--map-key",
        "entry-abc.def",
    ]);
    assert_eq!(got["value"], "#dfe8df");
    // 含点 ID 不会被拆解
    assert_eq!(got["mapKey"], "entry-abc.def");

    let previous = env.ok(&[
        "config",
        "unset",
        "appearance.uiColors",
        "--map-key",
        "entry-abc.def",
    ]);
    assert_eq!(previous["previous"], "#dfe8df");
    env.err(
        &[
            "config",
            "get",
            "appearance.uiColors",
            "--map-key",
            "entry-abc.def",
        ],
        3,
    );

    // 标量不可 unset
    let error = env.err(&["config", "unset", "appearance.uiScale"], 2);
    assert_eq!(error["code"], "UNSET_UNSUPPORTED");
}

#[test]
fn json_value_and_file_inputs() {
    let env = TestEnv::fresh();
    env.ok(&[
        "config",
        "set",
        "appearance.themePresets",
        "--json-value",
        r##"[{"name":"项目蓝","color":"#dbeafe"},{"name":"柔和绿","color":"#dcfce7"}]"##,
    ]);
    let got = env.ok(&["config", "get", "appearance.themePresets"]);
    assert_eq!(got["value"].as_array().unwrap().len(), 2);
    assert_eq!(got["value"][0]["color"], "#dbeafe");

    env.err(
        &[
            "config",
            "set",
            "appearance.themePresets",
            "--json-value",
            r##"[{"name":"坏","color":"not-a-color"}]"##,
        ],
        2,
    );

    let palette = env.path().join("palette.json");
    std::fs::write(&palette, r##"[{"name":"文件色","color":"#f4f1ea"}]"##).unwrap();
    env.ok(&[
        "config",
        "set",
        "appearance.themePresets",
        "--value-file",
        palette.to_str().unwrap(),
    ]);
    let got = env.ok(&["config", "get", "appearance.themePresets"]);
    assert_eq!(got["value"][0]["name"], "文件色");
}

#[test]
fn list_with_prefix() {
    let env = TestEnv::fresh();
    let list = env.ok(&["config", "list", "--prefix", "appearance"]);
    assert!(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["path"].as_str().unwrap().starts_with("appearance")));
    env.err(&["config", "list", "--prefix", "nope"], 2);
}

#[test]
fn reset_requires_yes_and_restores_defaults() {
    let env = TestEnv::fresh();
    env.ok(&["config", "set", "appearance.uiScale", "0.9"]);
    env.ok(&["config", "set", "notifications.position", "top-left"]);

    env.err(&["config", "reset", "appearance"], 10);

    let dry = env.ok(&["config", "reset", "appearance", "--dry-run"]);
    assert!(dry["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|change| change["path"] == "appearance.uiScale"));

    env.ok(&["config", "reset", "appearance", "--yes"]);
    assert_eq!(
        env.ok(&["config", "get", "appearance.uiScale"])["value"],
        0.75
    );
    // 其它分支不受影响
    assert_eq!(
        env.ok(&["config", "get", "notifications.position"])["value"],
        "top-left"
    );

    env.ok(&["config", "reset", "--yes"]);
    assert_eq!(
        env.ok(&["config", "get", "notifications.position"])["value"],
        "bottom-right"
    );
}

#[test]
fn config_path_reports_layout() {
    let env = TestEnv::fresh();
    let paths = env.ok(&["config", "path"]);
    assert!(paths["paths"]["data"]["path"]
        .as_str()
        .unwrap()
        .ends_with("data.json"));
    assert!(paths["paths"]["backups"].is_object());
    assert!(paths["limits"]["scheduleHistoryPerTask"].is_number());
    assert!(paths["skills"].is_object());
    assert!(paths["host"].is_object());
}

#[test]
fn config_validate_reports_issues() {
    let env = TestEnv::fresh();
    let valid = env.ok(&["config", "validate"]);
    assert_eq!(valid["valid"], true);

    // 塞入一个无效头像引用
    env.ok(&["config", "set", "profile.avatar", "missing-avatar.png"]);
    let invalid = env.ok(&["config", "validate"]);
    assert_eq!(invalid["valid"], false);
    assert!(invalid["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["path"] == "profile.avatar"));
}

#[test]
fn set_is_atomic_under_bad_values() {
    let env = TestEnv::fresh();
    env.ok(&["config", "set", "appearance.uiScale", "0.8"]);
    env.err(&["config", "set", "appearance.uiScale", "9.9"], 2);
    assert_eq!(
        env.ok(&["config", "get", "appearance.uiScale"])["value"],
        0.8
    );
    let settings = env.read_file("settings.json");
    assert_eq!(settings["appearance"]["uiScale"], json!(0.8));
}

#[test]
fn ledger_and_diary_font_sizes_round_trip_within_range() {
    let env = TestEnv::fresh();
    // 默认 18，source = default
    for path in ["appearance.ledgerFontSize", "appearance.diaryFontSize"] {
        let got = env.ok(&["config", "get", path]);
        assert_eq!(got["value"], 18);
        assert_eq!(got["source"], "default");
    }

    assert_eq!(
        env.ok(&["config", "set", "appearance.ledgerFontSize", "22"])["value"],
        22
    );
    assert_eq!(
        env.ok(&["config", "set", "appearance.diaryFontSize", "16"])["value"],
        16
    );
    assert_eq!(
        env.ok(&["config", "get", "appearance.ledgerFontSize"])["value"],
        22
    );
    assert_eq!(
        env.ok(&["config", "get", "appearance.diaryFontSize"])["value"],
        16
    );

    // 14-26 之外一律拒绝，已存值不动
    env.err(&["config", "set", "appearance.ledgerFontSize", "13"], 2);
    env.err(&["config", "set", "appearance.diaryFontSize", "27"], 2);
    assert_eq!(
        env.ok(&["config", "get", "appearance.ledgerFontSize"])["value"],
        22
    );

    // 字号是本机偏好：不进设置同步的共享子集
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("appearance.ledgerFontSize"));
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("appearance.diaryFontSize"));

    env.ok(&["config", "reset", "appearance", "--yes"]);
    assert_eq!(
        env.ok(&["config", "get", "appearance.ledgerFontSize"])["value"],
        18
    );
    assert_eq!(
        env.ok(&["config", "get", "appearance.diaryFontSize"])["value"],
        18
    );
}

#[test]
fn nav_items_and_layout_are_validated_and_local() {
    let env = TestEnv::fresh();
    // 默认 = 全部七行，按显示顺序
    let got = env.ok(&["config", "get", "appearance.navItems"]);
    assert_eq!(
        got["value"],
        json!(["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"])
    );
    assert_eq!(got["source"], "default");
    assert_eq!(env.ok(&["config", "get", "appearance.navLayout"])["value"], "list");

    // 顺序就是显示顺序；重复项去掉
    env.ok(&[
        "config", "set", "appearance.navItems",
        "--json-value", r#"["diary","my-day","diary","ledger"]"#,
    ]);
    assert_eq!(
        env.ok(&["config", "get", "appearance.navItems"])["value"],
        json!(["diary", "my-day", "ledger"])
    );
    // 空列表 = 全部隐藏，也是合法配置
    env.ok(&["config", "set", "appearance.navItems", "--json-value", "[]"]);
    assert_eq!(env.ok(&["config", "get", "appearance.navItems"])["value"], json!([]));

    // 未知 id 拒绝（today 不是 my-day），已存值不动
    let bad = env.err(
        &["config", "set", "appearance.navItems", "--json-value", r#"["today"]"#],
        2,
    );
    assert_eq!(bad["code"], "INVALID_CONFIG_VALUE");
    assert_eq!(env.ok(&["config", "get", "appearance.navItems"])["value"], json!([]));
    // 不是字符串数组也拒绝
    env.err(&["config", "set", "appearance.navItems", "diary"], 2);

    // 布局三选一
    env.ok(&["config", "set", "appearance.navLayout", "grid"]);
    assert_eq!(env.ok(&["config", "get", "appearance.navLayout"])["value"], "grid");
    env.ok(&["config", "set", "appearance.navLayout", "icons"]);
    assert_eq!(env.ok(&["config", "get", "appearance.navLayout"])["value"], "icons");
    let bad = env.err(&["config", "set", "appearance.navLayout", "carousel"], 2);
    assert_eq!(bad["code"], "INVALID_CONFIG_VALUE");

    // 导航可见性/布局都是本机偏好，不跨设备同步
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("appearance.navItems"));
    assert!(!kxtodo_core::ops_config::is_shared_settings_path("appearance.navLayout"));

    // 默认清单在 config list 里可见；reset 恢复默认
    let list = env.ok(&["config", "list", "--prefix", "appearance"]);
    let paths: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"appearance.navItems"));
    assert!(paths.contains(&"appearance.navLayout"));

    env.ok(&["config", "reset", "appearance", "--yes"]);
    assert_eq!(env.ok(&["config", "get", "appearance.navLayout"])["value"], "list");
    assert_eq!(
        env.ok(&["config", "get", "appearance.navItems"])["value"],
        json!(["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"])
    );
}

#[test]
fn due_colors_whole_map_write() {
    let env = TestEnv::fresh();
    // GUI 是整份写入（不带 --map-key）：v0.8.1 把 dueColors 误标成 is_map，
    // 走进了 uiColors 那条「--map-key + 单个颜色」的分支，GUI 改配色永远报
    // MAP_KEY_REQUIRED（--map-key 是 CLI 概念，GUI 用户无从下手）。
    env.ok(&[
        "config",
        "set",
        "appearance.dueColors",
        "{\"entry-abc\":[\"#808080\",\"#d93025\",\"#eab308\",\"#3b82f6\"]}",
    ]);
    let got = env.ok(&["config", "get", "appearance.dueColors"]);
    assert_eq!(
        got["value"]["entry-abc"],
        json!(["#808080", "#d93025", "#eab308", "#3b82f6"])
    );
    // 共享子集：写 dueColors 要刷新设置的 LWW 时间戳（多端同步靠它）
    assert!(kxtodo_core::ops_config::is_shared_settings_path("appearance.dueColors"));
    // 校验：四色不齐（含 v0.8.3 之前的三色写法）/ 非法色值都拒绝
    env.err(
        &["config", "set", "appearance.dueColors", "{\"e\":[\"#d93025\"]}"],
        2,
    );
    env.err(
        &[
            "config",
            "set",
            "appearance.dueColors",
            // 三色的老写法：加了「已过期」这一档之后必须写四个，少一个就拒
            "{\"e\":[\"#d93025\",\"#eab308\",\"#3b82f6\"]}",
        ],
        2,
    );
    env.err(
        &[
            "config",
            "set",
            "appearance.dueColors",
            "{\"e\":[\"red\",\"#d93025\",\"#eab308\",\"#3b82f6\"]}",
        ],
        2,
    );
    // 非 map 字段带 --map-key：明确拒绝而不是静默忽略
    env.err(
        &[
            "config",
            "set",
            "appearance.dueColors",
            "#ffffff",
            "--map-key",
            "entry-abc",
        ],
        2,
    );
}


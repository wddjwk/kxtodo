#![recursion_limit = "512"]
//! config domain tests (§3.6).

mod common;

use common::TestEnv;
use serde_json::json;

#[test]
fn list_sort_mode_validates_all_modes_and_resets() {
    let env = TestEnv::fresh();
    let path = "appearance.listSortMode";
    let got = env.ok(&["config", "get", path]);
    assert_eq!(got["value"], "created-desc");
    assert_eq!(got["source"], "default");
    assert!(kxtodo_core::ops_config::is_shared_settings_path(path));

    for mode in kxtodo_core::model::LIST_SORT_MODES {
        assert_eq!(env.ok(&["config", "set", path, mode])["value"], mode);
        assert_eq!(env.ok(&["config", "get", path])["value"], mode);
        assert_eq!(env.read_file("settings.json")["appearance"]["listSortMode"], mode);
    }
    assert!(env.read_file("settings.json")["syncUpdatedAt"].is_string());
    for invalid in ["manual", "createdAt", "CREATED-DESC", "", "null", "true", "1", "[]"] {
        let before = env.read_file("settings.json");
        let error = env.err(&["config", "set", path, invalid], 2);
        assert_eq!(error["code"], "INVALID_CONFIG_VALUE");
        assert_eq!(env.read_file("settings.json"), before, "invalid: {invalid}");
    }
    let listed = env.ok(&["config", "list", "--prefix", path]);
    assert_eq!(listed["items"][0]["path"], path);
    assert_eq!(listed["items"][0]["value"], "importance");
    env.ok(&["config", "reset", path, "--yes"]);
    assert_eq!(env.ok(&["config", "get", path])["value"], "created-desc");
    assert_eq!(env.read_file("settings.json")["appearance"]["listSortMode"], "created-desc");
}

#[test]
fn shared_sort_and_pin_setting_changes_and_resets_bump_lww() {
    use kxtodo_core::model::SettingsFile;
    use kxtodo_core::ops_config::{reset_values, set_value};

    let old_stamp = "2020-01-01T00:00:00.000Z";
    for (path, value) in [
        ("appearance.listSortMode", json!("due-desc")),
        ("features.pinnedIcon", json!(false)),
        ("features.pinnedSection", json!(true)),
    ] {
        let mut settings = SettingsFile::default();
        settings.sync_updated_at = Some(old_stamp.to_string());
        set_value(&mut settings, path, value.clone(), None).unwrap();
        assert_ne!(settings.sync_updated_at.as_deref(), Some(old_stamp), "{path}");
        let written = settings.sync_updated_at.clone();
        set_value(&mut settings, path, value, None).unwrap();
        assert_eq!(settings.sync_updated_at, written, "unchanged values keep their LWW stamp");
        settings.sync_updated_at = Some(old_stamp.to_string());
        let reset = reset_values(&mut settings, Some(path)).unwrap();
        assert_eq!(reset.len(), 1);
        assert_eq!(reset[0]["path"], path);
        assert_ne!(settings.sync_updated_at.as_deref(), Some(old_stamp), "reset {path}");
    }
}

#[test]
fn pinned_display_features_are_independent_booleans() {
    let env = TestEnv::fresh();
    let icon = "features.pinnedIcon";
    let section = "features.pinnedSection";
    for (path, default) in [(icon, true), (section, false)] {
        let got = env.ok(&["config", "get", path]);
        assert_eq!(got["value"], default);
        assert_eq!(got["source"], "default");
        assert!(kxtodo_core::ops_config::is_shared_settings_path(path));
        let listed = env.ok(&["config", "list", "--prefix", path]);
        assert_eq!(listed["items"][0]["path"], path);
        assert_eq!(listed["items"][0]["kind"], "boolean");
    }
    for (icon_value, section_value) in [(true, true), (false, true), (false, false), (true, false)] {
        env.ok(&["config", "set", icon, if icon_value { "true" } else { "false" }]);
        env.ok(&["config", "set", section, if section_value { "true" } else { "false" }]);
        assert_eq!(env.ok(&["config", "get", icon])["value"], icon_value);
        assert_eq!(env.ok(&["config", "get", section])["value"], section_value);
        let saved = env.read_file("settings.json");
        assert_eq!(saved["features"]["pinnedIcon"], icon_value);
        assert_eq!(saved["features"]["pinnedSection"], section_value);
        assert!(saved["syncUpdatedAt"].is_string());
    }
    for path in [icon, section] {
        for invalid in ["1", "null", "\"false\"", "{}"] {
            let before = env.read_file("settings.json");
            let error = env.err(&["config", "set", path, "--json-value", invalid], 2);
            assert_eq!(error["code"], "INVALID_CONFIG_VALUE");
            assert_eq!(env.read_file("settings.json"), before);
        }
    }
    env.ok(&["config", "set", icon, "false"]);
    env.ok(&["config", "set", section, "true"]);
    env.ok(&["config", "reset", icon, "--yes"]);
    assert_eq!(env.ok(&["config", "get", icon])["value"], true);
    assert_eq!(env.ok(&["config", "get", section])["value"], true);
    env.ok(&["config", "reset", "features", "--yes"]);
    assert_eq!(env.ok(&["config", "get", icon])["value"], true);
    assert_eq!(env.ok(&["config", "get", section])["value"], false);
}

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


#[test]
fn transfer_relay_accepts_default_sentinel() {
    let env = TestEnv::fresh();
    // 出厂默认就是「使用默认服务」（v0.8.7）
    let got = env.ok(&["config", "get", "transfer.relay"]);
    assert_eq!(got["value"], "default");
    // 三态都合法：default / 空（复用同步）/ 自部署地址；乱写的地址当场拒
    env.ok(&["config", "set", "transfer.relay", "default"]);
    env.ok(&["config", "set", "transfer.relay", ""]);
    env.ok(&["config", "set", "transfer.relay", "https://relay.example.com"]);
    env.err(&["config", "set", "transfer.relay", "不是地址"], 2);
}

#[test]
fn toolbox_tool_colors_are_validated_against_tool_catalog() {
    let env = TestEnv::fresh();
    let got = env.ok(&["config", "get", "toolbox.toolAccents"]);
    assert_eq!(got["value"], json!({}));

    // 整份对象写；键必须是工具目录里的 id（与 appearance.navItems 同一条纪律）
    let set = env.ok(&[
        "config",
        "set",
        "toolbox.toolAccents",
        "{\"rmb\":\"#123456\",\"transfer\":\"#654321\"}",
    ]);
    assert_eq!(set["value"]["rmb"], "#123456");
    env.ok(&[
        "config",
        "set",
        "toolbox.toolBackgrounds",
        "{\"scratchpad\":\"#abcdef\"}",
    ]);
    let got = env.ok(&["config", "get", "toolbox.toolAccents"]);
    assert_eq!(got["value"]["transfer"], "#654321");

    env.err(&[
        "config",
        "set",
        "toolbox.toolAccents",
        "{\"nope\":\"#123456\"}",
    ], 2);
    env.err(&[
        "config",
        "set",
        "toolbox.toolAccents",
        "{\"rmb\":\"blue\"}",
    ], 2);
    env.err(&["config", "set", "toolbox.toolBackgrounds", "#123456"], 2);
}

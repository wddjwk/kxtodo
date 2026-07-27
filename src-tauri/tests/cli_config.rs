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
        "config", "set", "appearance.uiColors", "#dfe8df", "--map-key", "entry-abc.def",
    ]);
    let got = env.ok(&["config", "get", "appearance.uiColors", "--map-key", "entry-abc.def"]);
    assert_eq!(got["value"], "#dfe8df");
    // 含点 ID 不会被拆解
    assert_eq!(got["mapKey"], "entry-abc.def");

    let previous = env.ok(&["config", "unset", "appearance.uiColors", "--map-key", "entry-abc.def"]);
    assert_eq!(previous["previous"], "#dfe8df");
    env.err(&["config", "get", "appearance.uiColors", "--map-key", "entry-abc.def"], 3);

    // 标量不可 unset
    let error = env.err(&["config", "unset", "appearance.uiScale"], 2);
    assert_eq!(error["code"], "UNSET_UNSUPPORTED");
}

#[test]
fn json_value_and_file_inputs() {
    let env = TestEnv::fresh();
    env.ok(&[
        "config", "set", "appearance.themePresets",
        "--json-value", r##"[{"name":"项目蓝","color":"#dbeafe"},{"name":"柔和绿","color":"#dcfce7"}]"##,
    ]);
    let got = env.ok(&["config", "get", "appearance.themePresets"]);
    assert_eq!(got["value"].as_array().unwrap().len(), 2);
    assert_eq!(got["value"][0]["color"], "#dbeafe");

    env.err(&[
        "config", "set", "appearance.themePresets",
        "--json-value", r##"[{"name":"坏","color":"not-a-color"}]"##,
    ], 2);

    let palette = env.path().join("palette.json");
    std::fs::write(&palette, r##"[{"name":"文件色","color":"#f4f1ea"}]"##).unwrap();
    env.ok(&[
        "config", "set", "appearance.themePresets",
        "--value-file", palette.to_str().unwrap(),
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
    assert_eq!(env.ok(&["config", "get", "appearance.uiScale"])["value"], 0.75);
    // 其它分支不受影响
    assert_eq!(env.ok(&["config", "get", "notifications.position"])["value"], "top-left");

    env.ok(&["config", "reset", "--yes"]);
    assert_eq!(env.ok(&["config", "get", "notifications.position"])["value"], "bottom-right");
}

#[test]
fn config_path_reports_layout() {
    let env = TestEnv::fresh();
    let paths = env.ok(&["config", "path"]);
    assert!(paths["paths"]["data"]["path"].as_str().unwrap().ends_with("data.json"));
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
    assert_eq!(env.ok(&["config", "get", "appearance.uiScale"])["value"], 0.8);
    let settings = env.read_file("settings.json");
    assert_eq!(settings["appearance"]["uiScale"], json!(0.8));
}

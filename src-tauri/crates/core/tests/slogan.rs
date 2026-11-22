//! 标语栏（v0.9.4 单例；v0.9.5 条目级图标/六位置/主题色/透明度/轮播间隔）：
//! gui.set-slogan 整份覆盖的校验与落盘 + data 范围单例 LWW 同步。
//! gui.* 是内部命令面（不经 CLI），直接驱动 core::execute + 临时数据目录（与 gui_background 同写法）。

use serde_json::{json, Value};

use kxtodo_core::core::{execute, ExecContext, Invocation};
use kxtodo_core::model::DataFile;
use kxtodo_core::repo::Repository;
use kxtodo_core::sync::merge::{apply_data_record, extract_entities, Scopes};

struct Env {
    dir: tempfile::TempDir,
    repo: Repository,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Repository::open(dir.path().to_path_buf()).expect("open repo");
        repo.ensure_initialized().expect("initialize");
        Self { dir, repo }
    }

    fn dispatch(&self, params: Value) -> Value {
        let invocation = Invocation::new("gui.set-slogan", params);
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        let outcome = execute(&invocation, &ctx);
        assert_eq!(outcome.code, 0, "envelope: {}", outcome.envelope);
        outcome.envelope
    }

    fn err(&self, params: Value, code: &str) {
        let invocation = Invocation::new("gui.set-slogan", params);
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        let outcome = execute(&invocation, &ctx);
        assert_ne!(outcome.code, 0);
        assert_eq!(outcome.envelope["error"]["code"], code, "envelope: {}", outcome.envelope);
    }

    fn slogan(&self) -> Value {
        serde_json::to_value(self.repo.load_data().expect("load data").slogan).unwrap()
    }
}

fn payload() -> Value {
    json!({
        "items": [
            { "id": "s1", "text": "多喝水", "icon": "Flame", "createdAt": "2026-11-01T00:00:00.000Z" },
            { "id": "s2", "text": "久坐了起来活动", "icon": "none", "createdAt": "2026-11-01T00:00:00.000Z" }
        ],
        "activeIds": ["s2", "s1"],
        "bounds": "app",
        "defaultIcon": "💧",
        "accent": "#1f6feb",
        "textColor": "#333333",
        "opacity": 0.8,
        "intervalSeconds": 5,
        "position": "bottom-center",
        "enabled": true
    })
}

#[test]
fn set_slogan_persists_whole_singleton() {
    let env = Env::new();
    // 没写过 = 默认空单例（enabled true / top-right / 透明度 0.94 / 间隔 8）
    let initial = env.slogan();
    assert_eq!(initial["items"].as_array().unwrap().len(), 0);
    assert_eq!(initial["enabled"], true);
    assert_eq!(initial["position"], "top-right");
    assert_eq!(initial["bounds"], "field");
    assert_eq!(initial["opacity"], 0.94);
    assert_eq!(initial["intervalSeconds"], 8);

    env.dispatch(payload());
    let saved = env.slogan();
    assert_eq!(saved["items"].as_array().unwrap().len(), 2);
    assert_eq!(saved["items"][0]["icon"], "Flame");
    assert_eq!(saved["items"][1]["icon"], "none");
    assert_eq!(saved["activeIds"], json!(["s2", "s1"]));
    assert_eq!(saved["bounds"], "app");
    assert_eq!(saved["defaultIcon"], "💧");
    assert_eq!(saved["accent"], "#1f6feb");
    assert_eq!(saved["textColor"], "#333333");
    assert_eq!(saved["opacity"], 0.8);
    assert_eq!(saved["intervalSeconds"], 5);
    assert_eq!(saved["position"], "bottom-center");
    assert!(saved["updatedAt"].as_str().unwrap().len() > 0);

    // 整份覆盖：新载荷不带 s1 就没了
    env.dispatch(json!({
        "items": [{ "id": "s3", "text": "今日事今日毕" }],
        "activeIds": ["s3"],
        "position": "top-left",
        "enabled": false
    }));
    let next = env.slogan();
    assert_eq!(next["items"].as_array().unwrap().len(), 1);
    assert_eq!(next["items"][0]["icon"], Value::Null); // 空图标 skip_serializing_if 省略
    assert_eq!(next["position"], "top-left");
    assert_eq!(next["enabled"], false);
    // 缺省字段回落默认
    assert_eq!(next["bounds"], "field");
    assert_eq!(next["defaultIcon"], Value::Null); // 空默认图标被 skip_serializing_if 省略
    assert_eq!(next["opacity"], 0.94);
    assert_eq!(next["intervalSeconds"], 8);
    assert_eq!(next["accent"], Value::Null);
}

#[test]
fn set_slogan_validates_shapes() {
    let env = Env::new();
    // 幽灵 activeId 过滤、重复 id 去重、空文本跳过、超长截断
    env.dispatch(json!({
        "items": [
            { "id": "s1", "text": "  多喝水  " },
            { "id": "s1", "text": "重复 id" },
            { "id": "s2", "text": "   " },
            { "id": "s3", "text": "x".repeat(120) },
            { "id": "", "text": "无 id" }
        ],
        "activeIds": ["s1", "ghost", "s1", "s3"]
    }));
    let saved = env.slogan();
    let items = saved["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["text"], "多喝水");
    assert_eq!(items[1]["text"].as_str().unwrap().chars().count(), 80);
    assert_eq!(saved["activeIds"], json!(["s1", "s3"]));

    // 条目图标：ASCII 乱写拒；none / 空 / 白名单名 / emoji 都收
    env.err(
        json!({ "items": [{ "id": "s1", "text": "x", "icon": "NotAnIcon" }] }),
        "SLOGAN_ICON_INVALID",
    );
    env.dispatch(json!({ "items": [
        { "id": "s1", "text": "x", "icon": "Megaphone" },
        { "id": "s2", "text": "y" },
        { "id": "s3", "text": "z", "icon": "🔥" }
    ] }));
    assert_eq!(env.slogan()["items"][0]["icon"], "Megaphone");
    assert_eq!(env.slogan()["items"][2]["icon"], "🔥");
    env.err(json!({ "items": [], "defaultIcon": "NotAnIcon" }), "SLOGAN_ICON_INVALID");
    env.dispatch(json!({ "items": [], "defaultIcon": "💧" }));
    assert_eq!(env.slogan()["defaultIcon"], "💧");
    env.err(json!({ "items": [], "bounds": "window" }), "SLOGAN_BOUNDS_INVALID");
    env.dispatch(json!({ "items": [], "bounds": "app" }));
    assert_eq!(env.slogan()["bounds"], "app");

    // 位置六选一
    for position in kxtodo_core::model::SLOGAN_POSITIONS {
        env.dispatch(json!({ "items": [], "position": position }));
        assert_eq!(env.slogan()["position"], position);
    }
    env.err(json!({ "items": [], "position": "middle" }), "SLOGAN_POSITION_INVALID");

    // 颜色：空 / #rrggbb 收，其它拒
    env.err(json!({ "items": [], "accent": "red" }), "SLOGAN_ACCENT_INVALID");
    env.err(json!({ "items": [], "textColor": "#12345" }), "SLOGAN_TEXT_COLOR_INVALID");
    env.dispatch(json!({ "items": [], "accent": "#ABCDEF", "textColor": "" }));
    assert_eq!(env.slogan()["accent"], "#abcdef");

    // 透明度 0.3–1.0；间隔 2–120 秒
    env.err(json!({ "items": [], "opacity": 0.1 }), "SLOGAN_OPACITY_INVALID");
    env.err(json!({ "items": [], "opacity": 1.5 }), "SLOGAN_OPACITY_INVALID");
    env.err(json!({ "items": [], "intervalSeconds": 1 }), "SLOGAN_INTERVAL_INVALID");
    env.err(json!({ "items": [], "intervalSeconds": 500 }), "SLOGAN_INTERVAL_INVALID");
    env.dispatch(json!({ "items": [], "opacity": 1.0, "intervalSeconds": 120 }));
    assert_eq!(env.slogan()["opacity"], 1.0);
    assert_eq!(env.slogan()["intervalSeconds"], 120);
}

#[test]
fn slogan_syncs_as_lww_singleton_under_data_scope() {
    let env = Env::new();
    let source: DataFile = serde_json::from_value(json!({ "schemaVersion": 6 })).unwrap();
    let diary = Default::default();
    let ledger = Default::default();
    let calendar = Default::default();
    let settings = Default::default();
    let schedule = Default::default();
    let scopes = Scopes { data: true, ..Default::default() };

    // 没写过（updatedAt 空）= 没有实体可推
    let none = extract_entities(&source, &diary, &ledger, &calendar, &settings, &schedule, &scopes, "dev-a");
    assert!(none.iter().all(|record| record.kind != "slogan"));

    env.dispatch(payload());
    let written = env.repo.load_data().unwrap();
    let records = extract_entities(&written, &diary, &ledger, &calendar, &settings, &schedule, &scopes, "dev-a");
    let record = records.iter().find(|record| record.kind == "slogan").expect("data 范围应提取 slogan 单例");
    assert_eq!(record.id, "slogan");
    assert_eq!(record.data["activeIds"], json!(["s2", "s1"]));
    assert_eq!(record.data["bounds"], "app");
    assert_eq!(record.data["defaultIcon"], "💧");
    assert_eq!(record.data["intervalSeconds"], 5);
    assert_eq!(record.data["items"][0]["icon"], "Flame");

    // 远端更新胜出（LWW 整份覆盖 + 新字段）
    let mut target: DataFile = serde_json::from_value(json!({ "schemaVersion": 6 })).unwrap();
    let mut remote = record.clone();
    remote.updated_at = "2027-01-01T00:00:00.000Z".to_string();
    remote.data["items"] = json!([{ "id": "r1", "text": "远端标语", "icon": "none" }]);
    remote.data["activeIds"] = json!(["r1"]);
    remote.data["bounds"] = json!("field");
    remote.data["defaultIcon"] = json!("🔥");
    remote.data["accent"] = json!("#d93025");
    remote.data["textColor"] = json!("#222222");
    remote.data["opacity"] = json!(0.7);
    remote.data["intervalSeconds"] = json!(3);
    remote.data["position"] = json!("bottom-right");
    apply_data_record(&remote, &mut target).unwrap();
    assert_eq!(target.slogan.items.len(), 1);
    assert_eq!(target.slogan.items[0].text, "远端标语");
    assert_eq!(target.slogan.items[0].icon, "none");
    assert_eq!(target.slogan.active_ids, vec!["r1"]);
    assert_eq!(target.slogan.bounds, "field");
    assert_eq!(target.slogan.default_icon, "🔥");
    assert_eq!(target.slogan.accent, "#d93025");
    assert_eq!(target.slogan.interval_seconds, 3);
    assert_eq!(target.slogan.position, "bottom-right");
    assert_eq!(target.slogan.updated_at, "2027-01-01T00:00:00.000Z");

    // 远端坏形状：位置/间隔/透明度回落默认
    let mut bad = record.clone();
    bad.updated_at = "2027-02-01T00:00:00.000Z".to_string();
    bad.data["position"] = json!("middle");
    bad.data["bounds"] = json!("window");
    bad.data["defaultIcon"] = json!("NotAnIcon");
    bad.data["intervalSeconds"] = json!(999);
    bad.data["opacity"] = json!(2.0);
    apply_data_record(&bad, &mut target).unwrap();
    assert_eq!(target.slogan.position, "top-right");
    assert_eq!(target.slogan.bounds, "field");
    assert_eq!(target.slogan.default_icon, "");
    assert_eq!(target.slogan.interval_seconds, 8);
    assert_eq!(target.slogan.opacity, 0.94);

    // 关掉 data 范围就不提取
    let off = Scopes { data: false, ..Default::default() };
    let none = extract_entities(&written, &diary, &ledger, &calendar, &settings, &schedule, &off, "dev-a");
    assert!(none.iter().all(|record| record.kind != "slogan"));
}
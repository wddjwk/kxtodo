//! 标语栏（v0.9.4）：gui.set-slogan 整份覆盖的校验与落盘 + data 范围单例 LWW 同步。
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
            { "id": "s1", "text": "多喝水", "createdAt": "2026-11-01T00:00:00.000Z" },
            { "id": "s2", "text": "久坐了起来活动", "createdAt": "2026-11-01T00:00:00.000Z" }
        ],
        "activeIds": ["s2", "s1"],
        "icon": "",
        "position": "bottom",
        "enabled": true
    })
}

#[test]
fn set_slogan_persists_whole_singleton() {
    let env = Env::new();
    // 没写过 = 默认空单例（enabled true / top）
    let initial = env.slogan();
    assert_eq!(initial["items"].as_array().unwrap().len(), 0);
    assert_eq!(initial["enabled"], true);
    assert_eq!(initial["position"], "top");

    env.dispatch(payload());
    let saved = env.slogan();
    assert_eq!(saved["items"].as_array().unwrap().len(), 2);
    assert_eq!(saved["activeIds"], json!(["s2", "s1"]));
    assert_eq!(saved["position"], "bottom");
    assert!(saved["updatedAt"].as_str().unwrap().len() > 0);
    // 写路径抬 data 域 revision
    assert!(env.repo.load_data().unwrap().meta.revision > 0);

    // 整份覆盖：新载荷不带 s1 就没了
    env.dispatch(json!({
        "items": [{ "id": "s3", "text": "今日事今日毕" }],
        "activeIds": ["s3"],
        "icon": "none",
        "position": "top",
        "enabled": false
    }));
    let next = env.slogan();
    assert_eq!(next["items"].as_array().unwrap().len(), 1);
    assert_eq!(next["icon"], "none");
    assert_eq!(next["enabled"], false);
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
    // 缺省 position/enabled 兜底
    assert_eq!(saved["position"], "top");
    assert_eq!(saved["enabled"], true);

    env.err(json!({ "items": [], "icon": "NotAnIcon" }), "SLOGAN_ICON_INVALID");
    env.err(json!({ "items": [], "position": "left" }), "SLOGAN_POSITION_INVALID");
    // 白名单内的图标与 none/空 都收
    env.dispatch(json!({ "items": [], "icon": "Megaphone" }));
    assert_eq!(env.slogan()["icon"], "Megaphone");
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

    // 远端更新胜出（LWW 整份覆盖）
    let mut target: DataFile = serde_json::from_value(json!({ "schemaVersion": 6 })).unwrap();
    let mut remote = record.clone();
    remote.updated_at = "2027-01-01T00:00:00.000Z".to_string();
    remote.data["items"] = json!([{ "id": "r1", "text": "远端标语" }]);
    remote.data["activeIds"] = json!(["r1"]);
    remote.data["position"] = json!("top");
    apply_data_record(&remote, &mut target).unwrap();
    assert_eq!(target.slogan.items.len(), 1);
    assert_eq!(target.slogan.items[0].text, "远端标语");
    assert_eq!(target.slogan.active_ids, vec!["r1"]);
    assert_eq!(target.slogan.updated_at, "2027-01-01T00:00:00.000Z");

    // 关掉 data 范围就不提取
    let off = Scopes { data: false, ..Default::default() };
    let none = extract_entities(&written, &diary, &ledger, &calendar, &settings, &schedule, &off, "dev-a");
    assert!(none.iter().all(|record| record.kind != "slogan"));
}
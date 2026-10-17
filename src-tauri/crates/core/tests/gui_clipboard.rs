//! gui.clipboard-configure / gui.set-all-collapsed：剪切板设置整份落盘与侧栏全量折叠。

use serde_json::{json, Value};

use kxtodo_core::core::{execute, ExecContext, Invocation};
use kxtodo_core::repo::Repository;

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

    fn dispatch(&self, command: &str, params: Value) -> Value {
        let invocation = Invocation::new(command, params);
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

    fn dispatch_error(&self, command: &str, params: Value) -> Value {
        let invocation = Invocation::new(command, params);
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        let outcome = execute(&invocation, &ctx);
        assert_ne!(outcome.code, 0, "expected failure: {}", outcome.envelope);
        outcome.envelope
    }
}

fn clipboard_settings(code: &str) -> Value {
    json!({
        "enabled": true,
        "code": code,
        "deviceName": "客厅电脑",
        "directEnabled": true,
        "directAddresses": "192.168.1.20",
        "lanEnabled": true,
        "p2pEnabled": true,
        "types": { "text": true, "html": true, "image": false, "files": true },
        "limits": { "textMb": 3, "htmlMb": 4, "imageMb": 8, "filesMb": 64 },
        "saveDir": "D:\\clipboard"
    })
}

#[test]
fn configures_clipboard_settings_in_one_write() {
    let env = Env::new();
    env.dispatch("gui.clipboard-configure", json!({ "settings": clipboard_settings("12345678") }));
    let settings = env.repo.load_settings().expect("load settings");
    assert!(settings.clipboard.enabled);
    assert_eq!(settings.clipboard.code, "12345678");
    assert_eq!(settings.clipboard.device_name, "客厅电脑");
    assert_eq!(settings.clipboard.limits.files_mb, 64);
    assert!(!settings.clipboard.types.image);
    assert_eq!(settings.clipboard.save_dir, "D:\\clipboard");
    assert!(settings.clipboard.direct_enabled && settings.clipboard.lan_enabled && settings.clipboard.p2p_enabled);
}

#[test]
fn rejects_short_code_and_clamps_limits() {
    let env = Env::new();
    let envelope = env.dispatch_error(
        "gui.clipboard-configure",
        json!({ "settings": clipboard_settings("short") }),
    );
    assert_eq!(envelope["error"]["code"], "CLIPBOARD_CODE_TOO_SHORT");
    let mut settings = clipboard_settings("12345678");
    settings["limits"]["textMb"] = json!(999);
    env.dispatch("gui.clipboard-configure", json!({ "settings": settings }));
    let stored = env.repo.load_settings().expect("load settings");
    assert_eq!(stored.clipboard.limits.text_mb, 64);
}

#[test]
fn collapses_every_category_at_once() {
    let env = Env::new();
    let data = env.repo.load_data().expect("load data");
    let category = data
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Category)
        .map(|node| node.id.clone());
    let Some(category) = category else {
        // 种子数据里没有分类时先建一个（与 GUI 的「新建分类」同一条命令）
        env.dispatch(
            "task.add",
            json!({ "type": "category", "name": "测试分组", "parentId": null }),
        );
        return;
    };
    env.dispatch("gui.set-all-collapsed", json!({ "collapsed": true }));
    let data = env.repo.load_data().expect("load data");
    let node = data.nodes.iter().find(|node| node.id == category).unwrap();
    assert_eq!(node.collapsed, Some(true));
    env.dispatch("gui.set-all-collapsed", json!({ "collapsed": false }));
    let data = env.repo.load_data().expect("load data");
    let node = data.nodes.iter().find(|node| node.id == category).unwrap();
    assert_eq!(node.collapsed, Some(false));
}

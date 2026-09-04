//! gui.set-background 参数语义：image 显式 null = 清除存储值；image 键缺失 = 保持不动。
//! gui.* 是内部命令面（不经 CLI），这里直接驱动 core::execute + 临时数据目录。

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

    fn dispatch(&self, params: Value) {
        let invocation = Invocation::new("gui.set-background", params);
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        let outcome = execute(&invocation, &ctx);
        assert_eq!(outcome.code, 0, "envelope: {}", outcome.envelope);
    }

    fn background(&self, node_id: &str) -> Value {
        let data = self.repo.load_data().expect("load data");
        data.backgrounds
            .get(node_id)
            .cloned()
            .unwrap_or(Value::Null)
    }
}

#[test]
fn stores_explicit_values() {
    let env = Env::new();
    env.dispatch(json!({
        "nodeId": "n1",
        "color": "#123456",
        "image": "data:image/png;base64,AAA",
        "imageOpacity": 0.4,
    }));
    let background = env.background("n1");
    assert_eq!(
        background.get("color").and_then(Value::as_str),
        Some("#123456")
    );
    assert_eq!(
        background.get("image").and_then(Value::as_str),
        Some("data:image/png;base64,AAA")
    );
    assert_eq!(
        background.get("imageOpacity").and_then(Value::as_f64),
        Some(0.4)
    );
}

#[test]
fn explicit_null_image_clears_stored_image() {
    let env = Env::new();
    env.dispatch(json!({
        "nodeId": "n1",
        "color": "#111111",
        "image": "data:image/png;base64,AAA",
        "imageOpacity": 0.5,
    }));
    // 前端“清除背景图”发送的形状：具体 color + image:null + 具体 imageOpacity。
    env.dispatch(json!({
        "nodeId": "n1",
        "color": "#f4f1ea",
        "image": null,
        "imageOpacity": 1.0,
    }));
    let background = env.background("n1");
    assert!(
        background.get("image").is_none(),
        "image 应被清除：{background}"
    );
    assert_eq!(
        background.get("color").and_then(Value::as_str),
        Some("#f4f1ea")
    );
    assert_eq!(
        background.get("imageOpacity").and_then(Value::as_f64),
        Some(1.0)
    );
}

#[test]
fn absent_image_key_keeps_stored_image() {
    let env = Env::new();
    env.dispatch(json!({
        "nodeId": "n1",
        "image": "data:image/png;base64,AAA",
        "imageOpacity": 0.3,
    }));
    // 只改颜色：image / imageOpacity 键都不出现，必须原样保留。
    env.dispatch(json!({ "nodeId": "n1", "color": "#222222" }));
    let background = env.background("n1");
    assert_eq!(
        background.get("image").and_then(Value::as_str),
        Some("data:image/png;base64,AAA")
    );
    assert_eq!(
        background.get("imageOpacity").and_then(Value::as_f64),
        Some(0.3)
    );
    assert_eq!(
        background.get("color").and_then(Value::as_str),
        Some("#222222")
    );
}

#[test]
fn null_on_node_without_background_creates_entry_without_image() {
    let env = Env::new();
    env.dispatch(json!({
        "nodeId": "fresh",
        "color": "#f4f1ea",
        "image": null,
        "imageOpacity": 1.0,
    }));
    let background = env.background("fresh");
    assert!(background.is_object(), "应创建背景条目：{background}");
    assert!(background.get("image").is_none());
    assert_eq!(
        background.get("color").and_then(Value::as_str),
        Some("#f4f1ea")
    );
}

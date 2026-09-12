//! task.importMarkdown 的文件夹路径（v0.7.2）：不打包，直接指向一个 markdown + 图片的普通目录。
//! 走正常 core 命令层（core::execute + 临时数据目录），断言一个 md 一张卡片、
//! 只有**被引用**的图片落盘。

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
        let outcome = self.run(command, params);
        assert_eq!(outcome.code, 0, "envelope: {}", outcome.envelope);
        outcome.envelope["data"].clone()
    }

    fn run(&self, command: &str, params: Value) -> kxtodo_core::core::ExecOutcome {
        let invocation = Invocation::new(command, params);
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        execute(&invocation, &ctx)
    }

    fn img_dir(&self, node_id: &str) -> std::path::PathBuf {
        self.dir.path().join("img").join("data").join(node_id)
    }
}

#[test]
fn folder_import_brings_cards_and_only_referenced_images() {
    let env = Env::new();
    let entry = env.dispatch("task.add", json!({ "type": "entry", "name": "卡片目录" }));
    let node_id = entry["id"].as_str().unwrap().to_string();

    // 目录结构：md 散在嵌套子目录里，图片在 images/（一张被引用、一张没人引用；
    // 引用写法覆盖 images/<name>、./<name> 与更深的相对路径）
    let source = tempfile::tempdir().expect("source tempdir");
    let root = source.path();
    std::fs::create_dir_all(root.join("2026").join("09")).expect("mkdir nested");
    std::fs::create_dir_all(root.join("images")).expect("mkdir images");
    std::fs::write(
        root.join("2026").join("09").join("第一张.md"),
        "第一张卡片\n\n![](images/used.png)",
    )
    .expect("write md");
    std::fs::write(
        root.join("2026").join("09").join("第二张.markdown"),
        "第二张卡片\n\n![](../../images/used.png)",
    )
    .expect("write md2");
    std::fs::write(root.join("images").join("used.png"), b"used-bytes").expect("write used");
    std::fs::write(root.join("images").join("orphan.png"), b"orphan-bytes").expect("write orphan");

    let result = env.dispatch(
        "task.importMarkdown",
        json!({ "nodeId": node_id, "folderPath": root.to_string_lossy() }),
    );
    assert_eq!(result["imported"], 2, "一个 md 一张卡片");
    assert_eq!(result["images"], 1, "只有被引用的图落盘");

    // 磁盘核验：被引用的在，没人引用的不在（不落孤儿图）
    let img_dir = env.img_dir(&node_id);
    assert!(img_dir.join("used.png").exists(), "被引用的图落回条目插图目录");
    assert!(!img_dir.join("orphan.png").exists(), "没人引用的图不落盘");

    // 正文里的深层相对引用也归一回裸文件名
    let data = env.repo.load_data().expect("load data");
    let markdowns: Vec<&str> = data.tasks.iter().map(|task| task.markdown.as_str()).collect();
    assert_eq!(markdowns.len(), 2);
    assert!(
        markdowns.iter().all(|markdown| markdown.contains("![](used.png)")),
        "引用归一回裸文件名：{markdowns:?}"
    );
}

#[test]
fn import_requires_exactly_one_source() {
    let env = Env::new();
    let entry = env.dispatch("task.add", json!({ "type": "entry", "name": "卡片目录" }));
    let node_id = entry["id"].as_str().unwrap().to_string();

    // 两个都不给
    let outcome = env.run("task.importMarkdown", json!({ "nodeId": node_id }));
    assert_eq!(outcome.code, 2, "envelope: {}", outcome.envelope);
    assert_eq!(outcome.envelope["error"]["code"], "MISSING_PARAM");

    // 两个都给
    let outcome = env.run(
        "task.importMarkdown",
        json!({ "nodeId": node_id, "zipBase64": "eA==", "folderPath": "." }),
    );
    assert_eq!(outcome.code, 2, "envelope: {}", outcome.envelope);
    assert_eq!(outcome.envelope["error"]["code"], "CARDS_IMPORT_INVALID");

    // folderPath 不是目录
    let outcome = env.run(
        "task.importMarkdown",
        json!({ "nodeId": node_id, "folderPath": env.dir.path().join("data.json").to_string_lossy() }),
    );
    assert_eq!(outcome.code, 2, "envelope: {}", outcome.envelope);
    assert_eq!(outcome.envelope["error"]["code"], "CARDS_IMPORT_INVALID");
}

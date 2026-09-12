//! 保存后清理无引用插图（v0.7.2，image_gc）：宽限窗语义 + 任务侧/日记侧的接线。
//!
//! 生产调用点固定 5 分钟宽限窗（保护「图片先落盘、正文后保存」的在途图片），
//! 测试里把 mtime 拨回一小时前让生产路径可判，helper 级测试另跑 Duration::ZERO。

use std::path::Path;
use std::time::{Duration, SystemTime};

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
        let invocation = Invocation {
            command: command.to_string(),
            params,
            controls: kxtodo_core::core::Controls {
                // diary.remove 之类自带确认门；测试里的操作视为已获用户同意
                yes: true,
                ..Default::default()
            },
        };
        let ctx = ExecContext {
            repo: &self.repo,
            cwd: self.dir.path().to_path_buf(),
            host: None,
            custom_data_dir: false,
        };
        let outcome = execute(&invocation, &ctx);
        assert_eq!(outcome.code, 0, "envelope: {}", outcome.envelope);
        outcome.envelope["data"].clone()
    }
}

/// 把 mtime 拨回一小时前，让生产的 5 分钟宽限窗判定为「不是在途图片」。
/// std 自带 File::set_modified，不引新依赖。
fn backdate(path: &Path) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for backdate");
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .expect("set_modified");
}

#[test]
fn zero_grace_sweep_deletes_only_unreferenced() {
    // helper 级：a.png 被 markdown 引用、b.png 没人引用 → grace = ZERO 时只删 b
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("a.png"), b"a").expect("write a");
    std::fs::write(root.join("b.png"), b"b").expect("write b");
    std::fs::write(root.join("notes.md"), b"not an image").expect("write md");

    let removed = kxtodo_core::image_gc::sweep_unreferenced(
        root,
        ["正文 ![](a.png) 与 ![](images/a.png)"],
        Duration::ZERO,
    );
    assert_eq!(removed, 1);
    assert!(root.join("a.png").exists(), "被引用的图保留");
    assert!(!root.join("b.png").exists(), "没人引用的图删掉");
    assert!(root.join("notes.md").exists(), "非图片文件一律不碰");

    // 目录不存在 = 0，不炸
    let missing = kxtodo_core::image_gc::sweep_unreferenced(
        &root.join("nope"),
        ["![](a.png)"],
        Duration::ZERO,
    );
    assert_eq!(missing, 0);
}

#[test]
fn grace_window_protects_in_flight_images() {
    // 宽限窗内的未引用图片 = 可能在途（编辑器先落图/同步先到图），生产 grace 下不删
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("fresh.png"), b"fresh").expect("write fresh");
    std::fs::write(root.join("stale.png"), b"stale").expect("write stale");
    backdate(&root.join("stale.png"));

    let removed = kxtodo_core::image_gc::sweep_unreferenced(
        root,
        std::iter::empty::<&str>(),
        kxtodo_core::image_gc::GRACE,
    );
    assert_eq!(removed, 1, "只删过窗的");
    assert!(root.join("fresh.png").exists(), "宽限窗内的在途图片不动");
    assert!(!root.join("stale.png").exists(), "过窗的孤儿图删掉");
}

#[test]
fn task_modify_sweeps_entry_images() {
    let env = Env::new();
    let entry = env.dispatch("task.add", json!({ "type": "entry", "name": "带图条目" }));
    let node_id = entry["id"].as_str().unwrap().to_string();
    let item = env.dispatch(
        "task.add",
        json!({ "type": "item", "entryId": node_id, "markdown": "卡片正文 ![](a.png)" }),
    );
    let item_id = item["id"].as_str().unwrap().to_string();

    // 条目插图目录：a.png 被引用；b.png 没人引用（已过窗）；c.png 没人引用但刚落盘（在途）
    let img_dir = env.dir.path().join("img").join("data").join(&node_id);
    std::fs::create_dir_all(&img_dir).expect("mkdir img");
    std::fs::write(img_dir.join("a.png"), b"a").expect("write a");
    std::fs::write(img_dir.join("b.png"), b"b").expect("write b");
    std::fs::write(img_dir.join("c.png"), b"c").expect("write c");
    backdate(&img_dir.join("a.png"));
    backdate(&img_dir.join("b.png"));

    // 任意一次 task.modify（这里只是勾完成）都触发保存后的清理
    env.dispatch(
        "task.modify",
        json!({ "type": "item", "id": item_id, "completed": true }),
    );

    assert!(img_dir.join("a.png").exists(), "被引用的图保留");
    assert!(!img_dir.join("b.png").exists(), "没人引用且过窗的图删掉");
    assert!(img_dir.join("c.png").exists(), "宽限窗内的在途图片不动");
}

#[test]
fn diary_modify_sweeps_diary_images() {
    let env = Env::new();
    let entry = env.dispatch(
        "diary.add",
        json!({ "markdown": "今天写了代码 ![](d.png)" }),
    );
    let entry_id = entry["id"].as_str().unwrap().to_string();

    let img_dir = env
        .dir
        .path()
        .join("img")
        .join("data")
        .join(kxtodo_core::model::DIARY_IMAGE_NODE);
    std::fs::create_dir_all(&img_dir).expect("mkdir img");
    std::fs::write(img_dir.join("d.png"), b"d").expect("write d");
    std::fs::write(img_dir.join("orphan.png"), b"orphan").expect("write orphan");
    backdate(&img_dir.join("orphan.png"));

    env.dispatch(
        "diary.modify",
        json!({ "id": entry_id, "mood": "🙂" }),
    );

    assert!(img_dir.join("d.png").exists(), "被日记引用的图保留");
    assert!(!img_dir.join("orphan.png").exists(), "没人引用的孤儿图删掉");
}

//! 保存后清理无引用插图（v0.7.3 起立即删，image_gc）：不再有宽限窗——
//! 清理只跟在本地保存后面，引用集合对目标目录是权威的（见 image_gc 模块注释）。

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

#[test]
fn sweep_deletes_only_unreferenced_regardless_of_age() {
    // a.png 被 markdown 引用 → 保留；fresh.png 没人引用且**刚落盘**（mtime = 现在）→ 照删
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("a.png"), b"a").expect("write a");
    std::fs::write(root.join("fresh.png"), b"fresh").expect("write fresh");
    std::fs::write(root.join("notes.md"), b"not an image").expect("write md");

    let removed = kxtodo_core::image_gc::sweep_unreferenced(
        root,
        ["正文 ![](a.png) 与 ![](images/a.png)"],
    );
    assert_eq!(removed, 1);
    assert!(root.join("a.png").exists(), "被引用的图保留");
    assert!(
        !root.join("fresh.png").exists(),
        "没人引用的图不看 mtime，刚写的也立即删"
    );
    assert!(root.join("notes.md").exists(), "非图片文件一律不碰");

    // 目录不存在 = 0，不炸
    let missing =
        kxtodo_core::image_gc::sweep_unreferenced(&root.join("nope"), ["![](a.png)"]);
    assert_eq!(missing, 0);
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

    // 条目插图目录：a.png 被引用；b.png / c.png 没人引用（都是刚写的）
    let img_dir = env.dir.path().join("img").join("data").join(&node_id);
    std::fs::create_dir_all(&img_dir).expect("mkdir img");
    std::fs::write(img_dir.join("a.png"), b"a").expect("write a");
    std::fs::write(img_dir.join("b.png"), b"b").expect("write b");
    std::fs::write(img_dir.join("c.png"), b"c").expect("write c");

    // 任意一次 task.modify（这里只是勾完成）都触发保存后的清理
    env.dispatch(
        "task.modify",
        json!({ "type": "item", "id": item_id, "completed": true }),
    );

    assert!(img_dir.join("a.png").exists(), "被引用的图保留");
    assert!(!img_dir.join("b.png").exists(), "没人引用的图立即删掉");
    assert!(!img_dir.join("c.png").exists(), "刚写的孤儿图也立即删掉");
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

    env.dispatch(
        "diary.modify",
        json!({ "id": entry_id, "mood": "🙂" }),
    );

    assert!(img_dir.join("d.png").exists(), "被日记引用的图保留");
    assert!(!img_dir.join("orphan.png").exists(), "没人引用的孤儿图删掉");
}

#[test]
fn ledger_write_sweeps_ledger_images() {
    let env = Env::new();
    // 记一笔带附图：引用 ledger/keep.png（ensure_initialized 已种下账户「现金」）
    let entry = env.dispatch(
        "ledger.add",
        json!({ "amount": "12", "account": "现金", "date": "2026-09-08", "images": ["keep.png"] }),
    );
    let entry_id = entry["id"].as_str().unwrap().to_string();
    assert_eq!(entry["images"], json!(["keep.png"]), "附图裸文件名回显");

    let img_dir = env
        .dir
        .path()
        .join("img")
        .join("data")
        .join(kxtodo_core::model::LEDGER_IMAGE_NODE);
    std::fs::create_dir_all(&img_dir).expect("mkdir ledger img");
    std::fs::write(img_dir.join("keep.png"), b"k").expect("write keep");
    std::fs::write(img_dir.join("orphan.png"), b"o").expect("write orphan");

    // 任意一次 ledger 写（这里改备注）都触发保存后的清理
    env.dispatch("ledger.modify", json!({ "id": entry_id, "note": "换了备注" }));
    assert!(img_dir.join("keep.png").exists(), "被账目引用的图保留");
    assert!(!img_dir.join("orphan.png").exists(), "没人引用的孤儿图立即删掉");

    // 删掉这一笔后 keep.png 也没人引用了，一并清掉
    env.dispatch("ledger.remove", json!({ "id": entry_id }));
    assert!(!img_dir.join("keep.png").exists(), "删掉账目后它的附图也被清掉");
}

#[test]
fn moving_a_task_between_entries_takes_its_images() {
    // v0.8.3 review #5：插图目录与节点绑定，而孤儿判定是「按**当前** node_id 收集引用集」。
    // 任务移走而图留在原地 → 旧目录里它没人引用了 → 紧跟保存的那把扫帚把它当孤儿真删
    // （不可逆的数据丢失）。搬迁在 core 一侧做（task_ops::migrate_item_images）。
    let env = Env::new();
    let from = env.dispatch("task.add", json!({ "type": "entry", "name": "原条目" }));
    let to = env.dispatch("task.add", json!({ "type": "entry", "name": "新条目" }));
    let from_id = from["id"].as_str().unwrap().to_string();
    let to_id = to["id"].as_str().unwrap().to_string();
    let item = env.dispatch(
        "task.add",
        json!({ "type": "item", "entryId": from_id, "markdown": "带图的卡片 ![](pic.png)" }),
    );
    let item_id = item["id"].as_str().unwrap().to_string();

    let root = env.dir.path().join("img").join("data");
    let from_dir = root.join(&from_id);
    let to_dir = root.join(&to_id);
    std::fs::create_dir_all(&from_dir).expect("mkdir from");
    std::fs::write(from_dir.join("pic.png"), b"pic").expect("write pic");
    std::fs::write(from_dir.join("other.png"), b"other").expect("write other");

    env.dispatch(
        "task.modify",
        json!({ "type": "item", "id": item_id, "entryId": to_id }),
    );

    assert!(to_dir.join("pic.png").exists(), "搬走的任务把它引用的图一起带走");
    assert!(!from_dir.join("pic.png").exists(), "旧目录里不再留一份（留着就成孤儿）");
    assert!(!from_dir.join("other.png").exists(), "没人引用的图照旧被扫掉");

    // 「释放空间」的扫描结果里一个孤儿都不该有——搬完再跑 clean 也不会删用户的图
    let usage = env.dispatch("storage.usage", json!({}));
    assert_eq!(
        usage["orphanImages"]["count"], json!(0),
        "移动过的任务插图不算孤儿：{}",
        usage
    );
    env.dispatch("storage.clean", json!({}));
    assert!(to_dir.join("pic.png").exists(), "clean 之后图还在");
}

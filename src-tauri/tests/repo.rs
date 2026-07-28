#![recursion_limit = "512"]
//! Repository mechanics: locks, atomic writes, revisions, concurrency (§4.2.3, §4.3).

mod common;

use common::TestEnv;
use todo_note_lib::domain::repo::{RepoLock, Repository};

#[test]
fn revisions_increment_per_domain_independently() {
    let env = TestEnv::fresh();
    let first = env.run(&["task", "add", "--type", "category", "--name", "c"]);
    let data_rev_1 = first.envelope()["meta"]["revision"].as_u64().unwrap();

    let second = env.run(&["config", "set", "appearance.uiScale", "0.8"]);
    let settings_rev_1 = second.envelope()["meta"]["revision"].as_u64().unwrap();

    let third = env.run(&["task", "add", "--type", "category", "--name", "c2"]);
    let data_rev_2 = third.envelope()["meta"]["revision"].as_u64().unwrap();

    assert_eq!(data_rev_2, data_rev_1 + 1);
    assert_eq!(settings_rev_1, 1);
    // 域互不干扰：settings 文件 revision 独立
    let settings = env.read_file("settings.json");
    assert_eq!(settings["_meta"]["revision"], 1);
    let data = env.read_file("data.json");
    assert_eq!(data["_meta"]["revision"], data_rev_2);
}

#[test]
fn atomic_write_leaves_no_temp_files() {
    let env = TestEnv::fresh();
    for index in 0..5 {
        env.ok(&[
            "task",
            "add",
            "--type",
            "category",
            "--name",
            &format!("c{index}"),
        ]);
    }
    let leftovers: Vec<_> = std::fs::read_dir(env.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with('.') && name.ends_with(".tmp")
        })
        .collect();
    assert!(leftovers.is_empty(), "原子写临时文件必须清理");
}

#[test]
fn concurrent_writers_do_not_lose_updates() {
    let env = TestEnv::fresh();
    env.ok(&["task", "add", "--type", "category", "--name", "seed"]);
    let base = env.read_file("data.json")["_meta"]["revision"]
        .as_u64()
        .unwrap();

    let mut handles = Vec::new();
    for thread_index in 0..4 {
        let path = env.path();
        handles.push(std::thread::spawn(move || {
            let repo = Repository::open(path).unwrap();
            for item in 0..5 {
                repo.write_data(None, None, "test.concurrent", |file| {
                    file.nodes.push(todo_note_lib::domain::model::Node {
                        id: format!("t{thread_index}-n{item}"),
                        kind: todo_note_lib::domain::model::NodeKind::Category,
                        name: "x".to_string(),
                        icon: String::new(),
                        parent_id: None,
                        collapsed: None,
                        created_at: todo_note_lib::domain::time::now_iso(),
                        updated_at: None,
                        extra: Default::default(),
                    });
                    Ok(serde_json::json!({ "id": "x" }))
                })
                .unwrap();
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let data = env.read_file("data.json");
    assert_eq!(
        data["_meta"]["revision"].as_u64().unwrap(),
        base + 20,
        "20 次并发写事务必须全部生效"
    );
    let nodes = data["nodes"].as_array().unwrap();
    for thread_index in 0..4 {
        for item in 0..5 {
            assert!(
                nodes
                    .iter()
                    .any(|node| node["id"] == format!("t{thread_index}-n{item}")),
                "并发写入丢失"
            );
        }
    }
}

#[test]
fn lock_is_exclusive() {
    let env = TestEnv::fresh();
    let repo = Repository::open(env.path()).unwrap();
    let _guard = RepoLock::acquire(&repo.layout).unwrap();
    let second = RepoLock::try_acquire(&repo.layout).unwrap();
    assert!(second.is_none(), "排他锁期间 try_acquire 必须失败");
}

#[test]
fn corrupted_json_is_exit_5_not_overwritten() {
    let env = TestEnv::fresh();
    env.ok(&["task", "add", "--type", "category", "--name", "seed"]);
    let before = std::fs::read_to_string(env.path().join("data.json")).unwrap();
    std::fs::write(env.path().join("data.json"), "{ not json").unwrap();
    let result = env.run(&["task", "list", "--type", "category", "--parent-id", "root"]);
    assert_eq!(result.code, 5, "损坏数据必须返回退出码 5");
    // 不得用默认空数据覆盖
    let after = std::fs::read_to_string(env.path().join("data.json")).unwrap();
    assert_eq!(after, "{ not json");
    drop(before);
}

#[test]
fn audit_log_records_writes() {
    let env = TestEnv::fresh();
    env.ok(&["task", "add", "--type", "category", "--name", "c"]);
    env.ok(&["config", "set", "appearance.uiScale", "0.8"]);
    let audit = std::fs::read_to_string(env.path().join("history").join("audit.ndjson")).unwrap();
    let lines: Vec<&str> = audit.lines().collect();
    assert!(lines.len() >= 2);
    assert!(audit.contains("task.add"));
    assert!(audit.contains("config.set"));
    // 审计不含完整正文
    assert!(!audit.contains("markdown"));
}

#[test]
fn exact_idempotency_replay_precedes_revision_guard() {
    let env = TestEnv::fresh();
    let first = env.run(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "once",
        "--if-revision",
        "0",
        "--idempotency-key",
        "same-request",
    ]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    let replay = env.run(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "once",
        "--if-revision",
        "0",
        "--idempotency-key",
        "same-request",
    ]);
    assert_eq!(replay.code, 0, "{}", replay.stderr);
    assert_eq!(replay.envelope()["meta"]["replayed"], true);
}

#[test]
fn idempotency_is_scoped_by_command() {
    let env = TestEnv::fresh();
    let created = env.ok(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "before",
        "--idempotency-key",
        "shared-key",
    ]);
    let id = created["id"].as_str().unwrap().to_string();
    let modified = env.ok(&[
        "task",
        "modify",
        "--type",
        "category",
        "--id",
        &id,
        "--name",
        "after",
        "--idempotency-key",
        "shared-key",
    ]);
    assert_eq!(modified["name"], "after");
}

#[test]
fn audit_failure_does_not_hide_committed_write() {
    let env = TestEnv::fresh();
    env.ok(&["task", "add", "--type", "category", "--name", "seed"]);
    let audit = env.path().join("history").join("audit.ndjson");
    std::fs::remove_file(&audit).unwrap();
    std::fs::create_dir(&audit).unwrap();
    let result = env.run(&["task", "add", "--type", "category", "--name", "committed"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let envelope = result.envelope();
    assert_eq!(
        envelope["meta"]["warnings"][0]["code"],
        "AUDIT_WRITE_FAILED"
    );
    let data = env.read_file("data.json");
    assert!(data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["name"] == "committed"));
}

#[test]
fn backups_created_before_cascade_delete() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
    ]);
    env.ok(&[
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--yes",
    ]);
    let backups: Vec<_> = std::fs::read_dir(env.path().join("backups"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(!backups.is_empty(), "级联删除前必须备份");
    assert!(backups[0].path().join("data.json").exists());
}

#[test]
fn cascade_delete_idempotency_replay_does_not_create_backup_again() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let args = [
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--yes",
        "--idempotency-key",
        "delete-once",
    ];
    env.ok(&args);
    let count_before = std::fs::read_dir(env.path().join("backups"))
        .unwrap()
        .count();
    let replay = env.run(&args);
    assert_eq!(replay.code, 0, "{}", replay.stderr);
    assert_eq!(replay.envelope()["meta"]["replayed"], true);
    let count_after = std::fs::read_dir(env.path().join("backups"))
        .unwrap()
        .count();
    assert_eq!(count_after, count_before);
}

#[test]
fn cascade_delete_failure_leaves_durable_recovery_record() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let image_path = env.path().join("img").join("data").join(&entry_id);
    std::fs::create_dir_all(image_path.parent().unwrap()).unwrap();
    std::fs::write(&image_path, b"not-a-directory").unwrap();
    let result = env.run(&[
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--yes",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        result.envelope()["meta"]["warnings"][0]["code"],
        "ATTACHMENT_DELETE_PENDING"
    );
    let recovery = env.read_file("runtime/recovery.json");
    assert_eq!(recovery["records"].as_array().unwrap().len(), 1);
    let doctor = env.ok(&["doctor", "--check", "recovery"]);
    assert_eq!(doctor["healthy"], false);
    assert_eq!(doctor["checks"][0]["name"], "recovery");
}

#[test]
fn cascade_remove_deletes_image_dir() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
    ]);
    let img_dir = env.path().join("img").join("data").join(&entry_id);
    std::fs::create_dir_all(&img_dir).unwrap();
    std::fs::write(img_dir.join("md-1.png"), b"png").unwrap();

    env.ok(&[
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--yes",
    ]);
    assert!(!img_dir.exists(), "级联删除必须删除条目图片目录");
    // 背景键也被清理
    let data = env.read_file("data.json");
    assert!(data["backgrounds"].get(&entry_id).is_none());
}

//! 端到端同步收敛测试：spawn 真实 kxtodo-server 进程，用 core 引擎模拟双设备。

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use kxtodo_core::repo::Repository;

const USERNAME: &str = "tester";
const EMAIL: &str = "tester@example.com";
const SECRET: &str = "e2e-secret-123";

struct ServerGuard {
    child: Child,
    port: u16,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn server_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kxtodo-server"))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/healthz");
    for _ in 0..100 {
        if let Ok(response) = ureq::get(&url).timeout(Duration::from_secs(2)).call() {
            if response.status() == 200 {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server 未在 10 秒内就绪");
}

fn spawn_server(port: u16, db: &PathBuf) -> ServerGuard {
    let child = Command::new(server_binary())
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--db")
        .arg(db)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kxtodo-server");
    let guard = ServerGuard { child, port };
    wait_health(port);
    guard
}

fn device_repo(tag: &str) -> (Repository, PathBuf) {
    device_repo_impl(tag, true)
}

/// fresh=true：全新设备（无预置数据），首拉直接落服务端内容。
fn fresh_device_repo(tag: &str) -> (Repository, PathBuf) {
    device_repo_impl(tag, false)
}

fn device_repo_impl(tag: &str, initialize: bool) -> (Repository, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "kxtodo-sync-e2e-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = Repository::open(dir.clone()).unwrap();
    if initialize {
        repo.ensure_initialized().unwrap();
    }
    (repo, dir)
}

fn markdown_of(repo: &Repository) -> Vec<String> {
    let data = repo.load_data().unwrap();
    let mut out: Vec<String> = data
        .tasks
        .iter()
        .map(|item| item.markdown.clone())
        .collect();
    out.sort();
    out
}

fn node_names(repo: &Repository) -> Vec<String> {
    let data = repo.load_data().unwrap();
    let mut out: Vec<String> = data
        .nodes
        .iter()
        .filter(|node| node.kind != kxtodo_core::model::NodeKind::System)
        .map(|node| node.name.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn two_devices_converge_with_edits_and_deletes() {
    let port = free_port();
    let db = std::env::temp_dir().join(format!("kxtodo-sync-e2e-server-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _server = spawn_server(port, &db);
    let base = format!("http://127.0.0.1:{port}");

    let (a, _dir_a) = device_repo("a");

    // 设备 A 注册（数据默认开）
    kxtodo_core::sync::engine::register_device(
        &a,
        &base,
        USERNAME,
        EMAIL,
        SECRET,
        Some(kxtodo_core::sync::merge::Scopes {
            data: true,
            settings: false,
            schedules: false,
        }),
    )
    .expect("register");

    // 设备 A 添加条目与任务
    let data = a.load_data().unwrap();
    let entry_id = data
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();
    let _ = a
        .write_data(None, None, "test.add", |file| {
            file.tasks.push(kxtodo_core::model::Item {
                id: "task-aaaa".to_string(),
                node_id: entry_id.clone(),
                order: 0.0,
                markdown: "任务 A1".to_string(),
                completed: false,
                important: false,
                my_day: false,
                planned_date: None,
                due_date: None,
                completed_at: None,
                tags: Vec::new(),
                emojis: Vec::new(),
                expanded: None,
                created_at: kxtodo_core::time::now_iso(),
                updated_at: Some(kxtodo_core::time::now_iso()),
                extra: Default::default(),
            });
            Ok(serde_json::json!({}))
        })
        .unwrap();
    kxtodo_core::sync::engine::run_sync(&a).expect("A push");

    // 设备 B（全新设备，无预置数据）配对并拉取
    let (b, _dir_b) = fresh_device_repo("b");
    kxtodo_core::sync::engine::pair_device(
        &b,
        &base,
        USERNAME,
        EMAIL,
        SECRET,
        Some(kxtodo_core::sync::merge::Scopes {
            data: true,
            settings: false,
            schedules: false,
        }),
    )
    .expect("B pair");
    assert!(
        markdown_of(&b).contains(&"任务 A1".to_string()),
        "B 必须拉到 A 的任务，实际 {:?}",
        markdown_of(&b)
    );

    // B 修改 A1 + 添加 B1；A 并发添加 A2
    let b_entry = b
        .load_data()
        .unwrap()
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();
    let _ = b
        .write_data(None, None, "test.edit", |file| {
            if let Some(item) = file.tasks.iter_mut().find(|item| item.id == "task-aaaa") {
                item.markdown = "任务 A1（B 改）".to_string();
                item.updated_at = Some(kxtodo_core::time::now_iso());
            }
            file.tasks.push(kxtodo_core::model::Item {
                id: "task-bbbb".to_string(),
                node_id: b_entry.clone(),
                order: 1.0,
                markdown: "任务 B1".to_string(),
                completed: false,
                important: false,
                my_day: false,
                planned_date: None,
                due_date: None,
                completed_at: None,
                tags: Vec::new(),
                emojis: Vec::new(),
                expanded: None,
                created_at: kxtodo_core::time::now_iso(),
                updated_at: Some(kxtodo_core::time::now_iso()),
                extra: Default::default(),
            });
            Ok(serde_json::json!({}))
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let _ = a
        .write_data(None, None, "test.add2", |file| {
            file.tasks.push(kxtodo_core::model::Item {
                id: "task-cccc".to_string(),
                node_id: entry_id.clone(),
                order: 2.0,
                markdown: "任务 A2".to_string(),
                completed: false,
                important: false,
                my_day: false,
                planned_date: None,
                due_date: None,
                completed_at: None,
                tags: Vec::new(),
                emojis: Vec::new(),
                expanded: None,
                created_at: kxtodo_core::time::now_iso(),
                updated_at: Some(kxtodo_core::time::now_iso()),
                extra: Default::default(),
            });
            Ok(serde_json::json!({}))
        })
        .unwrap();

    // 交替同步两轮，双向收敛
    kxtodo_core::sync::engine::run_sync(&a).expect("A sync 1");
    kxtodo_core::sync::engine::run_sync(&b).expect("B sync 1");
    kxtodo_core::sync::engine::run_sync(&a).expect("A sync 2");
    kxtodo_core::sync::engine::run_sync(&b).expect("B sync 2");

    let expected = vec![
        "任务 A1（B 改）".to_string(),
        "任务 A2".to_string(),
        "任务 B1".to_string(),
    ];
    assert_eq!(markdown_of(&a), expected, "A 收敛失败");
    assert_eq!(markdown_of(&b), expected, "B 收敛失败");

    // A 删除 task-bbbb → 传播到 B
    let _ = a
        .write_data(None, None, "test.remove", |file| {
            file.tasks.retain(|item| item.id != "task-bbbb");
            file.meta.record_tombstone("task-bbbb", "task", &kxtodo_core::time::now_iso());
            Ok(serde_json::json!({}))
        })
        .unwrap();
    kxtodo_core::sync::engine::run_sync(&a).expect("A sync 3");
    kxtodo_core::sync::engine::run_sync(&b).expect("B sync 3");
    assert!(
        !markdown_of(&b).contains(&"任务 B1".to_string()),
        "删除必须传播到 B，实际 {:?}",
        markdown_of(&b)
    );
    assert_eq!(markdown_of(&a), vec!["任务 A1（B 改）".to_string(), "任务 A2".to_string()]);

    // 节点同步：A 建分类 → B 收敛
    let _ = a
        .write_data(None, None, "test.node", |file| {
            file.nodes.push(kxtodo_core::model::Node {
                id: "category-e2e".to_string(),
                kind: kxtodo_core::model::NodeKind::Category,
                name: "E2E 分类".to_string(),
                icon: "folder".to_string(),
                parent_id: None,
                order: 99.0,
                collapsed: None,
                created_at: kxtodo_core::time::now_iso(),
                updated_at: Some(kxtodo_core::time::now_iso()),
                extra: Default::default(),
            });
            Ok(serde_json::json!({}))
        })
        .unwrap();
    kxtodo_core::sync::engine::run_sync(&a).expect("A sync 4");
    kxtodo_core::sync::engine::run_sync(&b).expect("B sync 4");
    assert!(node_names(&b).contains(&"E2E 分类".to_string()));

    let _ = std::fs::remove_file(&db);
}

#[test]
fn wrong_secret_is_rejected() {
    let port = free_port();
    let db = std::env::temp_dir().join(format!("kxtodo-sync-e2e-wrong-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _server = spawn_server(port, &db);
    let base = format!("http://127.0.0.1:{port}");

    let (a, _dir_a) = device_repo("wrong");
    kxtodo_core::sync::engine::register_device(&a, &base, "u2", "u2@x.y", "right-secret", None)
        .expect("register");

    let (b, _dir_b) = device_repo("wrong-b");
    let result = kxtodo_core::sync::engine::pair_device(&b, &base, "u2", "u2@x.y", "WRONG", None);
    assert!(result.is_err(), "密钥错误必须登录失败");
    let _ = std::fs::remove_file(&db);
}

#[test]
fn server_ciphertext_is_opaque() {
    // 服务器落盘数据必须是密文：数据库文件中不应出现任何明文 markdown。
    let port = free_port();
    let db = std::env::temp_dir().join(format!("kxtodo-sync-e2e-opaque-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _server = spawn_server(port, &db);
    let base = format!("http://127.0.0.1:{port}");

    let (a, _dir_a) = device_repo("opaque");
    kxtodo_core::sync::engine::register_device(&a, &base, "u3", "u3@x.y", "sec", None).expect("register");
    let marker = "绝密明文标记XYZ";
    let data = a.load_data().unwrap();
    let entry_id = data
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();
    let _ = a
        .write_data(None, None, "test.opaque", |file| {
            file.tasks.push(kxtodo_core::model::Item {
                id: "task-opaque".to_string(),
                node_id: entry_id.clone(),
                order: 0.0,
                markdown: marker.to_string(),
                completed: false,
                important: false,
                my_day: false,
                planned_date: None,
                due_date: None,
                completed_at: None,
                tags: Vec::new(),
                emojis: Vec::new(),
                expanded: None,
                created_at: kxtodo_core::time::now_iso(),
                updated_at: Some(kxtodo_core::time::now_iso()),
                extra: Default::default(),
            });
            Ok(serde_json::json!({}))
        })
        .unwrap();
    kxtodo_core::sync::engine::run_sync(&a).expect("push");

    let raw = std::fs::read(&db).unwrap_or_default();
    let text = String::from_utf8_lossy(&raw);
    assert!(
        !text.contains(marker),
        "服务器数据库不得包含明文任务内容"
    );
    let _ = std::fs::remove_file(&db);
}

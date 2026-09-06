//! 端到端同步收敛测试：用真实的 kxtodo-server（子进程或 in-process 内置主机）+ core 引擎
//! 模拟多台设备。
//!
//! 两条主机路径都要覆盖：
//! - `spawn_server`：独立二进制（`--data-dir` 必须隔离，否则会覆盖开发机真实的
//!   `server/settings.json`，含管理员凭据）
//! - `start_embedded`：v0.6.0 的库目标，GUI/APK 的「本机作为服务器」走的就是它

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use kxtodo_core::repo::Repository;

const USERNAME: &str = "tester";
const SECRET: &str = "e2e-secret-123";

/// 自建服务方式的配对目标。v0.6.0 起 register/login 合并成一个 `pair_device`：
/// 账户不存在就当场注册，存在就登录。
fn server_pair(
    base: &str,
    username: &str,
    secret: &str,
) -> kxtodo_core::sync::endpoint::PairRequest {
    kxtodo_core::sync::endpoint::PairRequest {
        mode: kxtodo_core::model::SyncMode::Server,
        server_url: base.to_string(),
        lan_peer: String::new(),
        username: username.to_string(),
        secret: secret.to_string(),
    }
}

/// 局域网方式的配对目标：认的是主机**名字**，不是 ip:port。
fn lan_pair(
    peer: &str,
    username: &str,
    secret: &str,
) -> kxtodo_core::sync::endpoint::PairRequest {
    kxtodo_core::sync::endpoint::PairRequest {
        mode: kxtodo_core::model::SyncMode::Lan,
        server_url: String::new(),
        lan_peer: peer.to_string(),
        username: username.to_string(),
        secret: secret.to_string(),
    }
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kxtodo-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// 测试用 runtime：内置主机跑在它的 worker 线程上，阻塞的 core 调用（ureq）跑在测试线程上，
/// 两边不互相饿死。`block_on(start(...))` 返回后 serve 任务仍在后台跑。
fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

/// in-process 起一台内置主机（GUI/APK 的路径）。**实际端口一律从返回的 handle 上取**
/// （`handle.port()` / `handle.loopback_base_url()`），别假定等于请求的端口。
///
/// - 回环监听 + 关发现：测试只验 HTTP 协议与编排，不验 UDP 广播（容器里广播常常不通）
/// - `port_fallback`：Windows 上优雅停机会让已接受的连接进入 TIME_WAIT，
///   紧接着在同端口重新 bind 会 WSAEADDRINUSE；生产里靠端口自动上移兜住，测试同理。
async fn start_embedded(root: &Path, name: &str) -> kxtodo_server::ServerHandle {
    let config = kxtodo_server::ServerConfig {
        listen: format!("127.0.0.1:{}", free_port()),
        // 数据目录必须在测试自己的 root 下面：与独立 server 的 --data-dir 隔离同理
        data_dir: root.join("server"),
        db: None,
        name: Some(name.to_string()),
        admin: None,
        admin_provision: kxtodo_server::AdminProvision::GenerateIfMissing,
        discovery: false,
        port_fallback: 8,
        retry_bind: false,
    };
    kxtodo_server::host::start(config)
        .await
        .expect("start embedded host")
}

/// 宿主（GUI/APK）启动内置服务器之后写的描述符；core 只读它来解析
/// 「本机就是主机时连哪儿」，所以测试也得扮演宿主把它写出来。
fn write_host_descriptor(repo: &Repository, handle: &kxtodo_server::ServerHandle) {
    let state = kxtodo_core::sync::state::EmbeddedHostState {
        running: true,
        port: handle.port(),
        configured_port: handle.port(),
        name: handle.name().to_string(),
        loopback: false,
        instance_id: handle.instance_id.clone(),
        admin_url: handle.admin_url(),
        admin_user: handle.admin_user().to_string(),
        admin_password: handle
            .generated_admin_password
            .clone()
            .unwrap_or_default(),
        started_at: Some(kxtodo_core::time::now_iso()),
        pid: std::process::id(),
        last_error: None,
    };
    kxtodo_core::sync::state::save_host_state(&repo.layout, &state).unwrap();
}

/// 客户端侧的地址缓存：模拟「上次连过这台主机」，于是解析走缓存直连而不是广播。
fn seed_lan_cache(repo: &Repository, name: &str, port: u16) {
    let mut state = kxtodo_core::sync::state::load_state(&repo.layout);
    state.lan_endpoint = Some(kxtodo_core::sync::state::LanEndpoint {
        name: name.to_string(),
        host: "127.0.0.1".to_string(),
        port,
        instance_id: String::new(),
    });
    kxtodo_core::sync::state::save_state(&repo.layout, &state).unwrap();
}

/// 本机就是主机时的配对目标：lan_peer 留空，靠描述符解析到自己的 localhost。
fn lan_self_pair(username: &str, secret: &str) -> kxtodo_core::sync::endpoint::PairRequest {
    kxtodo_core::sync::endpoint::PairRequest {
        mode: kxtodo_core::model::SyncMode::Lan,
        server_url: String::new(),
        lan_peer: String::new(),
        username: username.to_string(),
        secret: secret.to_string(),
    }
}

/// 给设备加一条任务（直接走 repo 事务，与现有测试同一个套路）。
fn add_task(repo: &Repository, markdown: &str) {
    let data = repo.load_data().unwrap();
    let entry_id = data
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();
    let _ = repo
        .write_data(None, None, "test.add", |file| {
            file.tasks.push(kxtodo_core::model::Item {
                id: kxtodo_core::ids::gen_id("task"),
                node_id: entry_id,
                order: 0.0,
                markdown: markdown.to_string(),
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
}

struct ServerGuard {
    child: Child,
    data_dir: PathBuf,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
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
    // --data-dir 必须隔离：否则测试会覆盖真实服务器目录里的 settings.json
    //（含管理员凭据）、日志与 pidfile。
    let data_dir = db.with_extension("server-data");
    let _ = std::fs::remove_dir_all(&data_dir);
    let child = Command::new(server_binary())
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--db")
        .arg(db)
        .arg("--name")
        .arg("e2e-server")
        .arg("--admin-user")
        .arg("e2e-admin")
        .arg("--admin-password")
        .arg("e2e-pass")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kxtodo-server");
    let guard = ServerGuard { child, data_dir };
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
    let (_device_a, _report_a, registered) = kxtodo_core::sync::engine::pair_device(
        &a,
        &server_pair(&base, USERNAME, SECRET),
        Some(kxtodo_core::sync::merge::Scopes {
            data: true,
            settings: false,
            schedules: false,
        }),
    )
    .expect("pair");
    assert!(registered, "账户不存在时「开始同步」必须当场注册");

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
    let (_device_b, _report_b, registered_b) = kxtodo_core::sync::engine::pair_device(
        &b,
        &server_pair(&base, USERNAME, SECRET),
        Some(kxtodo_core::sync::merge::Scopes {
            data: true,
            settings: false,
            schedules: false,
        }),
    )
    .expect("B pair");
    assert!(!registered_b, "账户已存在时必须走登录，不能重复注册");
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
fn images_converge_between_devices() {
    let port = free_port();
    let db = std::env::temp_dir().join(format!("kxtodo-sync-e2e-img-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _server = spawn_server(port, &db);
    let base = format!("http://127.0.0.1:{port}");

    let (a, _dir_a) = device_repo("img-a");
    kxtodo_core::sync::engine::pair_device(&a, &server_pair(&base, "imguser", "img-secret"), None)
        .expect("pair");
    let entry_id = a
        .load_data()
        .unwrap()
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();

    // 一张 markdown 插图（带二进制字节）+ 一张列表背景图
    let md_payload: Vec<u8> = (0..64u8).chain([0xff, 0x00, 0xfe]).collect();
    let md_dir = a.layout.entry_img_dir(&entry_id);
    std::fs::create_dir_all(&md_dir).unwrap();
    std::fs::write(md_dir.join("md-1-1.png"), &md_payload).unwrap();
    let bg_dir = a.layout.background_img_dir();
    std::fs::create_dir_all(&bg_dir).unwrap();
    std::fs::write(bg_dir.join("bg-1-1.png"), b"background-bytes").unwrap();

    let report = kxtodo_core::sync::engine::run_sync(&a).expect("A push images");
    assert_eq!(report.images_pushed, 2, "两张图片都应上传：{report:?}");

    // 全新设备 B 配对：图片必须自动落盘，字节完全一致
    let (b, _dir_b) = fresh_device_repo("img-b");
    kxtodo_core::sync::engine::pair_device(&b, &server_pair(&base, "imguser", "img-secret"), None)
        .expect("B pair");
    assert_eq!(
        std::fs::read(b.layout.entry_img_dir(&entry_id).join("md-1-1.png")).unwrap(),
        md_payload,
        "markdown 插图必须按原字节同步到 B"
    );
    assert_eq!(
        std::fs::read(b.layout.background_img_dir().join("bg-1-1.png")).unwrap(),
        b"background-bytes",
        "列表背景图必须同步到 B"
    );

    // 幂等：内容寻址，再同步一轮不应有任何图片流量
    let again = kxtodo_core::sync::engine::run_sync(&b).expect("B resync");
    assert_eq!(again.images_pushed, 0, "重复上传：{again:?}");
    assert_eq!(again.images_pulled, 0, "重复下载：{again:?}");
    let again_a = kxtodo_core::sync::engine::run_sync(&a).expect("A resync");
    assert_eq!(again_a.images_pushed, 0);
    assert_eq!(again_a.images_pulled, 0);

    // 服务器只存密文：数据库里不得出现图片明文字节
    let raw = std::fs::read(&db).unwrap_or_default();
    assert!(
        !raw
            .windows(md_payload.len())
            .any(|window| window == md_payload.as_slice()),
        "服务器数据库不得包含图片明文"
    );
    assert!(
        !String::from_utf8_lossy(&raw).contains("background-bytes"),
        "服务器数据库不得包含背景图明文"
    );

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
    kxtodo_core::sync::engine::pair_device(&a, &server_pair(&base, "u2", "right-secret"), None)
        .expect("pair");

    let (b, _dir_b) = device_repo("wrong-b");
    let result = kxtodo_core::sync::engine::pair_device(&b, &server_pair(&base, "u2", "WRONG"), None);
    // 合并注册/登录之后这条最关键：密码错绝不能被当成「账户不存在」而悄悄注册一个新账户
    let error = result.expect_err("密钥错误必须登录失败");
    assert_eq!(error.code, "AUTH_FAILED", "密码不符必须是 AUTH_FAILED，实际 {error}");
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
    kxtodo_core::sync::engine::pair_device(&a, &server_pair(&base, "u3", "sec"), None)
        .expect("pair");
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

#[test]
fn paused_sync_is_refused_and_resumes() {
    let port = free_port();
    let db = std::env::temp_dir().join(format!("kxtodo-sync-e2e-pause-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _server = spawn_server(port, &db);
    let base = format!("http://127.0.0.1:{port}");

    let (a, _dir_a) = device_repo("pause");
    kxtodo_core::sync::engine::pair_device(
        &a,
        &server_pair(&base, "pauseuser", "pause-secret"),
        None,
    )
    .expect("pair");

    // 暂停：只关开关，服务器地址/用户名/密码全部保留
    let _ = a
        .write_settings(None, None, "test.pause", |file| {
            file.sync.enabled = false;
            Ok(serde_json::json!({}))
        })
        .unwrap();
    let settings = a.load_settings().unwrap();
    assert!(settings.sync.is_paired(), "暂停不等于解除配对");
    let error = kxtodo_core::sync::engine::run_sync(&a).unwrap_err();
    assert_eq!(error.code, "SYNC_PAUSED", "暂停时同步必须被拒绝：{error:?}");
    // 探测是只读动作，暂停时照常可用（面板仍要显示在线状态）
    assert!(kxtodo_core::sync::engine::probe_connection(&a).is_ok());

    // 恢复：立刻又能同步
    let _ = a
        .write_settings(None, None, "test.resume", |file| {
            file.sync.enabled = true;
            Ok(serde_json::json!({}))
        })
        .unwrap();
    kxtodo_core::sync::engine::run_sync(&a).expect("恢复后同步应成功");

    // 解除配对才会清掉密码，此后 is_paired 为 false
    let _ = a
        .write_settings(None, None, "test.unpair", |file| {
            file.sync.enabled = false;
            file.sync.secret = String::new();
            Ok(serde_json::json!({}))
        })
        .unwrap();
    assert!(!a.load_settings().unwrap().sync.is_paired());
    assert_eq!(
        kxtodo_core::sync::engine::run_sync(&a).unwrap_err().code,
        "SYNC_NOT_CONFIGURED"
    );
    let _ = std::fs::remove_file(&db);
}

// ---------------------------------------------------------------------------
// v0.6.0：内置主机（in-process）、主机身份 epoch、局域网按名字解析
// ---------------------------------------------------------------------------

/// GUI/APK 的「本机作为服务器」走的就是库目标：两台设备对着 in-process 主机收敛。
#[test]
fn embedded_host_serves_two_devices() {
    let root = temp_root("embedded-host");
    let runtime = test_runtime();
    let handle = runtime.block_on(start_embedded(&root, "测试主机"));
    let base = handle.loopback_base_url();

    // 内置主机的管理台凭据必须自动生成：用户只是勾了个框，不该被要求先想一个密码
    assert!(
        handle.generated_admin_password.is_some(),
        "首次启动必须自动生成管理台密码"
    );
    assert!(!handle.instance_id.is_empty(), "主机必须报自己的库身份");

    let (a, _dir_a) = device_repo("embedded-a");
    let (_device, _report, registered) = kxtodo_core::sync::engine::pair_device(
        &a,
        &server_pair(&base, "embeduser", "embed-secret"),
        None,
    )
    .expect("A pair");
    assert!(registered, "账户不存在时「开始同步」必须当场注册");
    add_task(&a, "嵌入式任务");
    kxtodo_core::sync::engine::run_sync(&a).expect("A push");

    // /healthz 必须带库身份与展示名：客户端靠名字选定主机、靠身份发现「库被重建了」
    let health: serde_json::Value = ureq::get(&format!("{base}/healthz"))
        .call()
        .expect("healthz")
        .into_json()
        .unwrap();
    assert_eq!(health["instanceId"].as_str().unwrap(), handle.instance_id);
    assert_eq!(health["name"].as_str().unwrap(), "测试主机");

    let (b, _dir_b) = fresh_device_repo("embedded-b");
    let (_d2, _r2, registered_b) = kxtodo_core::sync::engine::pair_device(
        &b,
        &server_pair(&base, "embeduser", "embed-secret"),
        None,
    )
    .expect("B pair");
    assert!(!registered_b, "账户已存在必须走登录，不能重复注册");
    assert!(
        markdown_of(&b).contains(&"嵌入式任务".to_string()),
        "B 必须拉到 A 的任务，实际 {:?}",
        markdown_of(&b)
    );

    runtime.block_on(handle.shutdown());
    let _ = std::fs::remove_dir_all(&root);
}

/// 主机的库被重建（换主机 / 删库重来）时，客户端必须自己发现并全量重新播种。
///
/// 不清水位的后果是**静默**的：新库 current_seq 从 1 开始而本地水位停在几百，
/// 于是什么都拉不到、推的时候一路 OCC 409。走的是局域网「本机作为主机」这条真实路径：
/// 宿主重启内置服务器后重写描述符，core 靠里面的 instanceId 发现库变了。
#[test]
fn recreated_host_store_reseeds_clients() {
    let root = temp_root("epoch");
    let runtime = test_runtime();
    let host_name = "epoch-host";

    let (a, _dir_a) = device_repo("epoch-a");
    // 本机作为主机：与 GUI 勾选走的是同一个不变式函数
    let _ = a
        .write_settings(None, None, "test.host", |file| {
            file.sync.apply_lan_role(Some(true), Some(host_name), None);
            Ok(serde_json::json!({}))
        })
        .unwrap();

    let handle = runtime.block_on(start_embedded(&root, host_name));
    write_host_descriptor(&a, &handle);
    let first_instance = handle.instance_id.clone();

    let (_device, _report, registered) = kxtodo_core::sync::engine::pair_device(
        &a,
        &lan_self_pair("epochuser", "epoch-secret"),
        None,
    )
    .expect("A pair");
    assert!(registered);
    add_task(&a, "epoch 任务");
    kxtodo_core::sync::engine::run_sync(&a).expect("A push");
    let state = kxtodo_core::sync::state::load_state(&a.layout);
    assert_eq!(
        state.server_instance_id, first_instance,
        "同步后应记住主机身份"
    );
    assert!(state.last_pulled_seq > 0, "推完之后应该有水位");
    runtime.block_on(handle.shutdown());

    // 关掉之后才删得掉（Windows 上删不了正打开着的文件）
    let server_dir = root.join("server");
    std::fs::remove_dir_all(&server_dir).expect("删掉主机的库目录");
    assert!(!server_dir.exists());

    let handle2 = runtime.block_on(start_embedded(&root, host_name));
    assert_ne!(
        handle2.instance_id, first_instance,
        "重建后的库必须是新身份"
    );
    write_host_descriptor(&a, &handle2);

    let report = kxtodo_core::sync::engine::run_sync(&a).expect("A 重新对账");
    assert!(report.pushed > 0, "换了主机库必须全量重推，实际 {report:?}");
    assert_eq!(
        kxtodo_core::sync::state::load_state(&a.layout).server_instance_id,
        handle2.instance_id
    );

    // 新库里必须重新出现 A 的数据：来一台全新设备当客户端拉一次就知道。
    // 账户也是 A 重新对账时自动重建的（ensure_login），所以 C 只需要登录。
    let (c, _dir_c) = fresh_device_repo("epoch-c");
    seed_lan_cache(&c, host_name, handle2.port());
    let (_dc, _rc, registered_c) = kxtodo_core::sync::engine::pair_device(
        &c,
        &lan_pair(host_name, "epochuser", "epoch-secret"),
        None,
    )
    .expect("C pair");
    assert!(!registered_c, "账户应已由 A 的重新对账重建，C 只需登录");
    assert!(
        markdown_of(&c).contains(&"epoch 任务".to_string()),
        "重建主机后数据必须被重新播种，实际 {:?}",
        markdown_of(&c)
    );

    runtime.block_on(handle2.shutdown());
    let _ = std::fs::remove_dir_all(&root);
}

/// 局域网方式按**名字**解析主机：有地址缓存就直连（不广播），名字对不上才报找不到。
#[test]
fn lan_mode_resolves_cached_host_by_name() {
    let root = temp_root("lan-cache");
    let runtime = test_runtime();
    let handle = runtime.block_on(start_embedded(&root, "客厅的电脑"));

    let (a, _dir_a) = device_repo("lan-a");
    // 预置地址缓存 = 模拟「上次连过这台主机」。CI 容器里 UDP 广播常常不通，
    // 缓存直连这条路径才是真正要覆盖的。
    seed_lan_cache(&a, "客厅的电脑", handle.port());

    let (_device, _report, registered) = kxtodo_core::sync::engine::pair_device(
        &a,
        &lan_pair("客厅的电脑", "lanuser", "lan-secret"),
        None,
    )
    .expect("局域网配对");
    assert!(registered);
    let settings = a.load_settings().unwrap();
    assert_eq!(
        settings.sync.effective_mode(),
        kxtodo_core::model::SyncMode::Lan
    );
    assert_eq!(settings.sync.lan_peer, "客厅的电脑");
    assert!(
        settings.sync.server_url.is_empty(),
        "局域网配对不该往 serverUrl 里写东西"
    );

    add_task(&a, "局域网任务");
    kxtodo_core::sync::engine::run_sync(&a).expect("局域网同步");

    // 换成一个不存在的名字、并且没有可用缓存 → 明确的错误码，且是 Io 类（走静默重连）
    let _ = a
        .write_settings(None, None, "test.peer", |file| {
            file.sync.lan_peer = "不存在的主机".to_string();
            Ok(serde_json::json!({}))
        })
        .unwrap();
    let mut stale = kxtodo_core::sync::state::load_state(&a.layout);
    stale.lan_endpoint = None;
    kxtodo_core::sync::state::save_state(&a.layout, &stale).unwrap();
    let error = kxtodo_core::sync::engine::run_sync(&a).expect_err("找不到的主机必须报错");
    assert_eq!(error.code, "SYNC_LAN_HOST_NOT_FOUND", "实际 {error}");
    assert_eq!(
        error.kind,
        kxtodo_core::error::ErrorKind::Io,
        "必须是 Io 类，才会被记成掉线并按 reconnectSeconds 静默重连"
    );

    runtime.block_on(handle.shutdown());
    let _ = std::fs::remove_dir_all(&root);
}

/// 本机就是主机时，局域网方式解析到自己的内置服务器（连 localhost，不广播）。
#[test]
fn lan_host_mode_connects_to_own_embedded_server() {
    let root = temp_root("lan-self");
    let runtime = test_runtime();
    let handle = runtime.block_on(start_embedded(&root, "我自己的电脑"));

    let (a, _dir_a) = device_repo("lan-self-a");
    write_host_descriptor(&a, &handle);
    let _ = a
        .write_settings(None, None, "test.host", |file| {
            file.sync.apply_lan_role(Some(true), Some("我自己的电脑"), None);
            Ok(serde_json::json!({}))
        })
        .unwrap();

    let (_device, _report, registered) =
        kxtodo_core::sync::engine::pair_device(&a, &lan_self_pair("selfhost", "self-secret"), None)
            .expect("本机主机配对");
    assert!(registered);
    assert_eq!(
        kxtodo_core::sync::state::load_state(&a.layout).server_instance_id,
        handle.instance_id
    );

    add_task(&a, "自己主机上的任务");
    let report = kxtodo_core::sync::engine::run_sync(&a).expect("本机主机同步");
    assert!(
        report.pushed > 0,
        "任务必须推到自己的内置主机，实际 {report:?}"
    );

    // 描述符说在跑但实际连不上 → 必须是 Io 类错误（记成掉线、静默重连），
    // 不能拿一份 stale 描述符当主机用
    runtime.block_on(handle.shutdown());
    let error = kxtodo_core::sync::engine::run_sync(&a).expect_err("主机停了必须报错");
    assert_eq!(error.kind, kxtodo_core::error::ErrorKind::Io);
    let _ = std::fs::remove_dir_all(&root);
}

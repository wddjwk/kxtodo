//! P2P 端到端：两台设备（两个临时数据目录）靠**账户派生密钥签名的 pkarr 目录**互相发现，
//! 非枢纽一方拨号枢纽、在 iroh 隧道里跑一次完整的普通 HTTP 同步，双向收敛。
//!
//! 完全离线可跑：目录用本文件里的 mock pkarr relay（回环 HTTP），relay 设成 `disabled`
//! （两台 endpoint 在同机，目录里带的回环直连地址足够建连），不碰任何公共服务。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kxtodo_core::model::SyncMode;
use kxtodo_core::repo::Repository;
use kxtodo_core::sync::crypto::derive_keys;
use kxtodo_core::sync::endpoint::PairRequest;
use kxtodo_core::sync::engine::{pair_device, run_sync};
use kxtodo_core::sync::p2p::{self, P2pConfig};

const USERNAME: &str = "p2p-tester";
const SECRET: &str = "p2p-secret-123";

// ---------------------------------------------------------------------------
// mock pkarr relay：PUT /<z32> 存 body，GET /<z32> 回 200+body 或 404
// ---------------------------------------------------------------------------

fn start_mock_pkarr() -> String {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind mock pkarr relay");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let store: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let store = store.clone();
            std::thread::spawn(move || serve_conn(stream, store));
        }
    });
    url
}

fn serve_conn(stream: StdTcpStream, store: Arc<Mutex<HashMap<String, Vec<u8>>>>) {
    let mut writer = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(stream);
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((key, value)) = trimmed.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let mut body = vec![0u8; content_length];
        if content_length > 0 && reader.read_exact(&mut body).is_err() {
            return;
        }
        let key = path.trim_start_matches('/').to_string();
        let (status, reason, payload) = match method.as_str() {
            "PUT" => {
                store.lock().expect("store").insert(key, body);
                (204, "No Content", Vec::new())
            }
            "GET" => match store.lock().expect("store").get(&key) {
                Some(value) => (200, "OK", value.clone()),
                None => (404, "Not Found", Vec::new()),
            },
            _ => (405, "Method Not Allowed", Vec::new()),
        };
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n",
            payload.len()
        );
        if writer.write_all(head.as_bytes()).is_err() {
            return;
        }
        if !payload.is_empty() && writer.write_all(&payload).is_err() {
            return;
        }
        if writer.flush().is_err() {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// 设备与主机
// ---------------------------------------------------------------------------

fn device_repo(tag: &str) -> (Repository, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kxtodo-p2p-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = Repository::open(dir.clone()).unwrap();
    repo.ensure_initialized().unwrap();
    (repo, dir)
}

/// 把设备设置成 P2P 已配对形态（目录指向 mock relay，relay 关掉：同机直连就够）
fn configure_p2p(repo: &Repository, directory_url: &str) {
    let url = directory_url.to_string();
    repo.write_settings(None, None, "test.p2p", |file| {
        file.sync.mode = Some(SyncMode::P2p);
        file.sync.enabled = true;
        file.sync.username = USERNAME.to_string();
        file.sync.secret = SECRET.to_string();
        file.sync.p2p_directory = url.clone();
        file.sync.p2p_relay = "disabled".to_string();
        Ok(serde_json::json!({}))
    })
    .unwrap();
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// 扮演宿主：起内置服务器（回环）+ 写描述符（P2P 的内置库只绑回环）
fn start_loopback_host(
    runtime: &tokio::runtime::Runtime,
    repo: &Repository,
    root: &Path,
    name: &str,
) -> kxtodo_server::ServerHandle {
    let config = kxtodo_server::ServerConfig {
        listen: format!("127.0.0.1:{}", free_port()),
        data_dir: root.join("server"),
        db: None,
        name: Some(name.to_string()),
        admin: None,
        admin_provision: kxtodo_server::AdminProvision::GenerateIfMissing,
        discovery: false,
        port_fallback: 8,
        retry_bind: false,
    };
    let handle = runtime.block_on(kxtodo_server::host::start(config)).expect("start host");
    let state = kxtodo_core::sync::state::EmbeddedHostState {
        running: true,
        port: handle.port(),
        configured_port: handle.port(),
        name: handle.name().to_string(),
        loopback: true,
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
    handle
}

fn start_p2p(repo: &Repository, directory_url: &str, name: &str) {
    let keys = derive_keys(USERNAME, SECRET).unwrap();
    p2p::start(P2pConfig {
        layout: repo.layout.clone(),
        keys,
        relay: Some("disabled".to_string()),
        directory_url: directory_url.to_string(),
        name: name.to_string(),
        serve: true,
    })
    .expect("start p2p runtime");
}

/// 等两台设备都把自己的回环地址发布进目录（首发布可能还没拿到地址，3 秒内会补发）
fn wait_directory_ready(repo: &Repository, want: usize) {
    let runtime = p2p::current_for(&repo.layout).expect("p2p runtime");
    let mut last = String::new();
    for _ in 0..60 {
        let entries = runtime.directory().unwrap_or_default();
        let with_addrs = entries.iter().filter(|entry| !entry.addrs.is_empty()).count();
        last = format!("{entries:?}");
        if entries.len() >= want && with_addrs >= want {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let self_id = runtime.device_id();
    panic!("目录里没有带直连地址的 {want} 台设备；本机 id={}；最后看到：{last}", self_id.to_z32());
}

/// 等本机把目录里的对端解析出名字（名字记录可能比目录条目晚一拍发布）。
fn wait_peer_name(repo: &Repository, want: &str) {
    let runtime = p2p::current_for(&repo.layout).expect("p2p runtime");
    let mut last: Vec<String> = Vec::new();
    for _ in 0..60 {
        let entries = runtime.directory().unwrap_or_default();
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        let names = runtime.resolve_peer_names(&ids);
        last = names.values().cloned().collect();
        if last.iter().any(|name| name == want) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("没解析到对端名字「{want}」；当前解析到：{last:?}");
}

fn add_task(repo: &Repository, markdown: &str) {
    let data = repo.load_data().unwrap();
    let entry_id = data
        .nodes
        .iter()
        .find(|node| node.kind == kxtodo_core::model::NodeKind::Entry)
        .map(|node| node.id.clone())
        .unwrap();
    repo.write_data(None, None, "test.add", |file| {
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

fn markdown_of(repo: &Repository) -> Vec<String> {
    let mut out: Vec<String> = repo
        .load_data()
        .unwrap()
        .tasks
        .iter()
        .map(|item| item.markdown.clone())
        .collect();
    out.sort();
    out
}

fn p2p_pair() -> PairRequest {
    PairRequest {
        mode: SyncMode::P2p,
        server_url: String::new(),
        lan_peer: String::new(),
        username: USERNAME.to_string(),
        secret: SECRET.to_string(),
    }
}

/// 两台设备跨 iroh 隧道双向收敛：A 建的任务到 B，B 建的任务到 A。
/// 枢纽是谁由 EndpointId 大小决定，测试不关心——两条方向都走一遍就覆盖了两种角色。
#[test]
fn p2p_two_devices_converge_over_tunnel() {
    let directory_url = start_mock_pkarr();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let (repo_a, root_a) = device_repo("a");
    let (repo_b, root_b) = device_repo("b");
    configure_p2p(&repo_a, &directory_url);
    configure_p2p(&repo_b, &directory_url);

    let handle_a = start_loopback_host(&runtime, &repo_a, &root_a, "p2p-a");
    let handle_b = start_loopback_host(&runtime, &repo_b, &root_b, "p2p-b");
    start_p2p(&repo_a, &directory_url, "设备甲");
    start_p2p(&repo_b, &directory_url, "设备乙");
    wait_directory_ready(&repo_a, 2);

    // 拨号之前先验名字：此刻 A 的拨号历史是空的，能显示「设备乙」只可能是
    // B 用自己设备私钥发布的那条 pkarr 名字记录（否则列表里就是一串 id）。
    wait_peer_name(&repo_a, "设备乙");

    // 配对：账户不存在就当场注册在「本轮枢纽」的库里，另一台登录同一个库
    let (_device_a, _report_a, _registered_a) = pair_device(&repo_a, &p2p_pair(), None).unwrap();
    let (_device_b, _report_b, _registered_b) = pair_device(&repo_b, &p2p_pair(), None).unwrap();

    add_task(&repo_a, "p2p 从 A 建的任务");
    run_sync(&repo_a).expect("sync a");
    run_sync(&repo_b).expect("sync b");
    assert!(
        markdown_of(&repo_b).contains(&"p2p 从 A 建的任务".to_string()),
        "B 没拿到 A 的任务：{:?}",
        markdown_of(&repo_b)
    );

    add_task(&repo_b, "p2p 从 B 建的任务");
    run_sync(&repo_b).expect("sync b 2");
    run_sync(&repo_a).expect("sync a 2");
    assert!(
        markdown_of(&repo_a).contains(&"p2p 从 B 建的任务".to_string()),
        "A 没拿到 B 的任务：{:?}",
        markdown_of(&repo_a)
    );

    // 收尾：先停 P2P 运行时（撤目录）再停内置服务器
    p2p::stop_for_layout(&repo_a.layout);
    p2p::stop_for_layout(&repo_b.layout);
    drop(handle_a);
    drop(handle_b);
    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

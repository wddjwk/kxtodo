//! 局域网发现的服务器侧（v0.5.0）：在固定 UDP 端口上应答客户端的广播/组播查询。
//!
//! 客户端发广播 + 组播查询，服务端**单播**应答——这样 Android 客户端不需要
//! MulticastLock（接收组播才需要，接收单播回包不需要）。
//!
//! 发现端口固定为 [`DISCOVERY_PORT`]，与 TCP 监听端口无关：服务器改了 TCP 端口
//! 仍然可被发现，回包里带的是真实的 TCP 端口。

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kxtodo_core::sync::discovery::{
    DISCOVERY_MULTICAST_GROUP, DISCOVERY_PORT, DISCOVERY_PROTOCOL, DISCOVERY_QUERY,
};
use serde_json::json;

use crate::api::SharedState;

/// 启动发现应答线程。返回 Err 表示端口不可用（例如已有另一台服务器占着），
/// 此时服务器照常提供 HTTP 服务，只是不能被自动发现。
pub fn spawn(
    state: SharedState,
    tcp_port: u16,
    running: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>, String> {
    let socket = bind_socket().map_err(|error| format!("绑定 UDP {DISCOVERY_PORT} 失败：{error}"))?;
    let name = state.settings.name.clone();
    let version = crate::api::APP_VERSION.to_string();
    Ok(std::thread::Builder::new()
        .name("kxtodo-discovery".to_string())
        .spawn(move || {
            responder_loop(socket, &state, &name, &version, tcp_port, running);
        })
        .map_err(|error| format!("启动发现线程失败：{error}"))?)
}

fn bind_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)))?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    let group: Ipv4Addr = DISCOVERY_MULTICAST_GROUP
        .parse()
        .unwrap_or(Ipv4Addr::new(239, 255, 77, 52));
    // 组播加入失败不致命：广播查询照样能收到
    let _ = socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED);
    Ok(socket)
}

fn responder_loop(
    socket: UdpSocket,
    state: &SharedState,
    name: &str,
    version: &str,
    tcp_port: u16,
    running: Arc<AtomicBool>,
) {
    let reply = json!({
        "protocol": DISCOVERY_PROTOCOL,
        "name": name,
        "port": tcp_port,
        "version": version,
    })
    .to_string();
    let mut buffer = [0u8; 512];
    while running.load(Ordering::SeqCst) {
        let (len, from) = match socket.recv_from(&mut buffer) {
            Ok(value) => value,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(_) => continue,
        };
        if !buffer[..len].starts_with(DISCOVERY_QUERY) {
            continue;
        }
        // 只认局域网来源：公网/回环来的查询一律不应答，避免被当成放大器
        if !from.ip().is_ipv4() || from.ip().is_loopback() {
            continue;
        }
        if socket.send_to(reply.as_bytes(), from).is_ok() {
            state.log(
                "info",
                &format!("发现应答：{name} → {}（TCP 端口 {tcp_port}）", from.ip()),
            );
        }
    }
    let group: Ipv4Addr = DISCOVERY_MULTICAST_GROUP
        .parse()
        .unwrap_or(Ipv4Addr::new(239, 255, 77, 52));
    let _ = socket.leave_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED);
}

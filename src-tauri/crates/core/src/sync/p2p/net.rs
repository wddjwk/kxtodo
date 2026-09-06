//! iroh 端点的进程级运行时 + TCP-over-tunnel 承载。
//!
//! - **一个进程一个 endpoint**（`SLOT`），自带一个两线程的 tokio 运行时：
//!   不借用 Tauri 的运行时，`block_on` 就永远不会撞「在异步上下文里 block_on」的雷
//!   （同步引擎跑在 spawn_blocking / CLI 主线程里）。
//! - **宿主（GUI/APK）**以 `serve = true` 启动：发布目录 + 接受拨入，把每条双向流
//!   泵到本机内置服务器的回环端口——被叫方因此天然就是「会话期主机」。
//! - **CLI 一次性进程**以 `serve = false` 启动：只拨不收（进程马上退出，
//!   往目录里发一条短命条目只会让别人拨一个死人）。
//! - 拨号侧开一个本地临时端口，每个本地 TCP 连接开一条新的双向流：
//!   HTTP 客户端（ureq）一行不改，照旧对着 `http://127.0.0.1:<port>` 发请求。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId, RelayMode, RelayUrl, SecretKey};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder as RuntimeBuilder, Handle, Runtime};
use tokio::task::AbortHandle;

use crate::error::{CoreError, CoreResult};
use crate::repo::Layout;
use crate::sync::crypto::SyncKeys;
use crate::sync::p2p::{directory, identity};
use crate::sync::state::load_host_state;

/// 本应用的 ALPN：两边必须一致，服务端至少注册一个
pub const ALPN: &[u8] = b"kxtodo-p2p/1";
/// 拨号超时：实测冷连接经 relay 约 7.5s、热连接 1.6s，留足余量
const DIAL_TIMEOUT: Duration = Duration::from_secs(25);
/// 拨号失败后这个对端冷却 5 分钟：枢纽下线时第一轮探一遍，之后不再每轮白等 6.5s
const COOLDOWN: Duration = Duration::from_secs(300);
/// 目录解析结果的进程内缓存。5 秒：再长会让「对端刚上线」在本地迟很久才可见
/// （同步轮 30s 一次、设置面板只读缓存不联网，5s 的代价只是每 5 秒一次轻量 GET）
const DIRECTORY_CACHE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct P2pConfig {
    pub layout: Layout,
    pub keys: SyncKeys,
    /// None/空 = n0 免费公共 relay；`disabled` = 不用 relay；其它 = 自部署 relay 地址
    pub relay: Option<String>,
    /// pkarr 目录地址；空 = n0 免费公共服务
    pub directory_url: String,
    /// 宿主进程：发布目录 + 接受拨入。CLI 一次性进程为 false（只拨不收）
    pub serve: bool,
}

pub struct P2pRuntime {
    endpoint: Endpoint,
    /// `shutdown_background` 要按值消费 Runtime，而本结构被多个 Arc 持有，
    /// 所以运行时放在 Mutex 里、停机时 take 出来；日常只用 handle。
    runtime: Mutex<Option<Runtime>>,
    handle: Handle,
    dir_client: directory::Client,
    account_secret: SecretKey,
    layout: Layout,
    serve: bool,
    relay: String,
    directory_url: String,
    cooldowns: Mutex<BTreeMap<String, Instant>>,
    dir_cache: Mutex<Option<(Instant, Vec<directory::DirectoryEntry>)>>,
}

static SLOT: OnceLock<Mutex<BTreeMap<String, Arc<P2pRuntime>>>> = OnceLock::new();

fn slot() -> &'static Mutex<BTreeMap<String, Arc<P2pRuntime>>> {
    SLOT.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// 本进程内跑着的 P2P 运行时（没有 = 宿主没启用 P2P，或这是不支持的环境）。
/// 一个进程通常只托管一个数据目录；多个时返回第一个（仅用于无布局上下文的展示路径）。
pub fn current() -> Option<Arc<P2pRuntime>> {
    slot()
        .lock()
        .ok()
        .and_then(|guard| guard.values().next().cloned())
}

/// 指定数据目录的 P2P 运行时（同步/解析端点走这条，多目录并存时不会拿错）。
pub fn current_for(layout: &Layout) -> Option<Arc<P2pRuntime>> {
    let key = layout.runtime_dir().to_string_lossy().to_string();
    slot().lock().ok().and_then(|guard| guard.get(&key).cloned())
}

/// 启动（或复用）P2P 运行时。配置变了就换掉旧的。
pub fn start(config: P2pConfig) -> CoreResult<Arc<P2pRuntime>> {
    let key = config.layout.runtime_dir().to_string_lossy().to_string();
    if let Some(existing) = current_for(&config.layout) {
        if existing.matches(&config) {
            return Ok(existing);
        }
        stop_for(&key);
    }
    let runtime = RuntimeBuilder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| CoreError::internal(format!("P2P 运行时启动失败：{error}")))?;
    let device_secret = identity::device_secret(&config.layout)?;
    let account_secret = directory::account_secret(&config.keys);
    let endpoint = runtime.block_on(bind_endpoint(&device_secret, config.relay.as_deref()))?;
    let dir_client = directory::build_client(&endpoint, &config.directory_url)?;
    let handle = runtime.handle().clone();
    let runtime_handle = Arc::new(P2pRuntime {
        endpoint,
        runtime: Mutex::new(Some(runtime)),
        handle,
        dir_client,
        account_secret,
        layout: config.layout.clone(),
        serve: config.serve,
        relay: config.relay.clone().unwrap_or_default(),
        directory_url: config.directory_url.clone(),
        cooldowns: Mutex::new(BTreeMap::new()),
        dir_cache: Mutex::new(None),
    });
    if config.serve {
        spawn_accept_loop(runtime_handle.clone());
        spawn_publish_loop(runtime_handle.clone());
    }
    if let Ok(mut guard) = slot().lock() {
        guard.insert(key, runtime_handle.clone());
    }
    Ok(runtime_handle)
}

/// 停掉指定数据目录的运行时：尽力从目录撤掉自己，再关端点与运行时。
pub fn stop_for(key: &str) {
    let existing = slot().lock().ok().and_then(|mut guard| guard.remove(key));
    if let Some(runtime) = existing {
        runtime.shutdown();
    }
}

/// 停掉本进程所有 P2P 运行时（宿主退出路径）。
pub fn stop() {
    let keys: Vec<String> = slot()
        .lock()
        .map(|guard| guard.keys().cloned().collect())
        .unwrap_or_default();
    for key in keys {
        stop_for(&key);
    }
}

/// 按数据目录停（宿主只知道 data_dir）。
pub fn stop_for_layout(layout: &Layout) {
    stop_for(&layout.runtime_dir().to_string_lossy());
}

impl P2pRuntime {
    fn matches(&self, config: &P2pConfig) -> bool {
        self.serve == config.serve
            && self.layout.runtime_dir() == config.layout.runtime_dir()
            && self.account_secret.public() == directory::account_secret(&config.keys).public()
            && self.directory_url == config.directory_url
            && self.relay == config.relay.clone().unwrap_or_default()
    }

    pub fn device_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub fn serve(&self) -> bool {
        self.serve
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// 目录（带进程内缓存）。失败按「只有我自己」处理——见 `directory::fetch`。
    pub fn directory(&self) -> CoreResult<Vec<directory::DirectoryEntry>> {
        if let Some(entries) = self.directory_cached() {
            return Ok(entries);
        }
        let client = self.dir_client.clone();
        let account_id = self.account_secret.public();
        let entries = self
            .handle
            .block_on(directory::fetch(&client, account_id));
        // 只缓存非空结果：空结果缓存住会让「对端刚上线」最多迟一个 TTL 才被发现
        if !entries.is_empty() {
            if let Ok(mut guard) = self.dir_cache.lock() {
                *guard = Some((Instant::now(), entries.clone()));
            }
        }
        Ok(entries)
    }

    /// 只读进程内缓存的目录（`sync status` 是纯本地读，不能碰网络）；没缓存或已过期返回 None。
    pub fn directory_cached(&self) -> Option<Vec<directory::DirectoryEntry>> {
        let guard = self.dir_cache.lock().ok()?;
        let (at, entries) = guard.as_ref()?;
        if at.elapsed() >= DIRECTORY_CACHE {
            return None;
        }
        Some(entries.clone())
    }

    /// 让目录缓存立刻失效（刚发布过/刚拨号失败过）
    pub fn invalidate_directory(&self) {
        if let Ok(mut guard) = self.dir_cache.lock() {
            *guard = None;
        }
    }

    pub fn in_cooldown(&self, peer: EndpointId) -> bool {
        self.cooldowns
            .lock()
            .ok()
            .and_then(|guard| guard.get(&peer.to_z32()).copied())
            .map(|at| at.elapsed() < COOLDOWN)
            .unwrap_or(false)
    }

    pub fn note_cooldown(&self, peer: EndpointId) {
        if let Ok(mut guard) = self.cooldowns.lock() {
            guard.insert(peer.to_z32(), Instant::now());
        }
    }

    /// 进目录的直连地址：**只放回环与私网**（RFC1918/ULA）。
    ///
    /// 目录是签名的但**明文**的公共记录，公网地址不该写进去；私网地址对同局域网的
    /// 设备是宝贵的直连捷径（免 relay 绕路），对别人则毫无意义也无害。
    pub fn direct_addrs(&self) -> Vec<std::net::SocketAddr> {
        self.endpoint
            .addr()
            .addrs
            .iter()
            .filter_map(|addr| match addr {
                iroh::TransportAddr::Ip(socket) => Some(*socket),
                _ => None,
            })
            .filter(|socket| is_local_ip(socket.ip()))
            .collect()
    }

    /// 拨号并开一条本地隧道：返回的 base_url 可以直接喂给 `SyncClient::new`。
    /// 隧道活着期间本地端口一直转发；`Tunnel` 掉引用即关闭。
    /// `addrs` 是目录里学到的直连地址（同机/同局域网免 relay）；为空则只按 id 拨（走 relay）。
    pub fn dial(&self, peer: EndpointId, addrs: &[std::net::SocketAddr]) -> CoreResult<Tunnel> {
        let endpoint = self.endpoint.clone();
        let direct: Vec<std::net::SocketAddr> = addrs.to_vec();
        self.handle.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|error| CoreError::io(format!("P2P 本地隧道监听失败：{error}")))?;
            let port = listener
                .local_addr()
                .map_err(|error| CoreError::io(format!("P2P 本地隧道取端口失败：{error}")))?
                .port();
            let target = if direct.is_empty() {
                iroh::EndpointAddr::new(peer)
            } else {
                iroh::EndpointAddr {
                    id: peer,
                    addrs: direct
                        .into_iter()
                        .map(iroh::TransportAddr::Ip)
                        .collect(),
                }
            };
            let connection = tokio::time::timeout(DIAL_TIMEOUT, endpoint.connect(target, ALPN))
                .await
                .map_err(|_| {
                    CoreError::io(format!(
                        "P2P 拨号超时（{}s）：对方可能不在线，或 relay 通路不通",
                        DIAL_TIMEOUT.as_secs()
                    ))
                })?
                .map_err(|error| CoreError::io(format!("P2P 拨号失败：{error:?}")))?;
            let shared = connection.clone();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((tcp, _)) = listener.accept().await else { break };
                    let connection = shared.clone();
                    tokio::spawn(async move {
                        let Ok((send, recv)) = connection.open_bi().await else { return };
                        pump(tcp, send, recv).await;
                    });
                }
            });
            Ok(Tunnel {
                base_url: format!("http://127.0.0.1:{port}"),
                connection,
                listener_task: task.abort_handle(),
            })
        })
    }

    fn shutdown(&self) {
        if self.serve {
            let client = self.dir_client.clone();
            let secret = self.account_secret.clone();
            let self_id = self.device_id();
            let _ = self.handle.block_on(async move {
                tokio::time::timeout(Duration::from_secs(3), directory::unpublish(&client, &secret, self_id))
                    .await
            });
        }
        let endpoint = self.endpoint.clone();
        let _ = self.handle.block_on(endpoint.close());
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(owned) = guard.take() {
                owned.shutdown_background();
            }
        }
    }
}

/// 被叫方：每条双向流接到本机内置服务器的回环端口。
fn spawn_accept_loop(runtime: Arc<P2pRuntime>) {
    let endpoint = runtime.endpoint.clone();
    let layout = runtime.layout.clone();
    runtime.handle.spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let layout = layout.clone();
            tokio::spawn(async move {
                let Ok(accepting) = incoming.accept() else { return };
                let Ok(connection) = accepting.await else { return };
                loop {
                    match connection.accept_bi().await {
                        Ok((send, recv)) => {
                            let layout = layout.clone();
                            tokio::spawn(async move {
                                serve_one_stream(&layout, send, recv).await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });
}

async fn serve_one_stream(layout: &Layout, send: SendStream, recv: RecvStream) {
    // 端口永远以描述符为准：配置端口被占用时内置服务器会自动上移
    let host = load_host_state(layout);
    if !host.running || host.port == 0 {
        return;
    }
    let Ok(tcp) = TcpStream::connect(("127.0.0.1", host.port)).await else {
        return;
    };
    pump(tcp, send, recv).await;
}

/// 常驻主机每 [`directory::PUBLISH_INTERVAL_SECS`] 续一次目录条目（条目带时间戳，
/// 过期即视为离线，所以退出时没来得及撤也有兜底）。
fn spawn_publish_loop(runtime: Arc<P2pRuntime>) {
    let client = runtime.dir_client.clone();
    let secret = runtime.account_secret.clone();
    let self_id = runtime.device_id();
    let handle = runtime.handle.clone();
    let account_id = secret.public();
    handle.spawn(async move {
        let mut published_addrs: Option<Vec<std::net::SocketAddr>> = None;
        let mut verify_pending = false;
        let mut tick: u32 = 0;
        loop {
            let addrs = runtime.direct_addrs();
            if verify_pending {
                // pkarr 是整包 last-write-wins：并发发布时后写的一方会把先写的盖掉，
                // 而被盖掉的一方此刻不会重发。所以每次成功发布后的下一个 tick 回读一次，
                // 自己不在就读-改-写补回（毫秒级治愈，不用等心跳）。
                // 注意这里就在运行时里，直接 await，不能 block_on。
                let entries = directory::fetch(&client, account_id).await;
                if entries.iter().any(|entry| entry.id == self_id) {
                    verify_pending = false;
                } else if directory::publish(&client, &secret, self_id, &addrs)
                    .await
                    .is_ok()
                {
                    runtime.invalidate_directory();
                }
            } else {
                // 首发布时 net report 往往还没给出直连地址；地址一到就补发一条带地址的条目。
                // 心跳（约两分钟）无条件续发：治愈漏网的覆盖，也远早于 15 分钟过期。
                let stale = published_addrs.as_ref() != Some(&addrs) || tick % 40 == 0;
                if stale {
                    match directory::publish(&client, &secret, self_id, &addrs).await {
                        Ok(()) => {
                            published_addrs = Some(addrs);
                            runtime.invalidate_directory();
                            verify_pending = true;
                        }
                        Err(error) => {
                            crate::sync::engine::debug_log(format!(
                                "p2p publish: {}",
                                error.message
                            ))
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            tick += 1;
        }
    });
}

/// 本地 TCP ↔ iroh 双向流 的双向泵。任一侧结束就收尾另一侧。
async fn pump(tcp: TcpStream, mut send: SendStream, mut recv: RecvStream) {
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let downstream = tokio::io::copy(&mut recv, &mut tcp_write);
    let upstream = tokio::io::copy(&mut tcp_read, &mut send);
    let (down, up) = tokio::join!(downstream, upstream);
    if down.is_ok() {
        let _ = tcp_write.shutdown().await;
    }
    if up.is_ok() {
        let _ = send.finish();
    }
}

/// 回环 / 私网 / 链路本地：只有这些地址才进公共目录（见 `direct_addrs`）。
fn is_local_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unique_local() || v6.segments()[0] & 0xffc0 == 0xfe80
        }
    }
}

/// 校验自部署 relay 地址（`sync configure --p2p-relay`）：配置时就报错，别等同步轮才炸。
pub fn parse_relay_url(raw: &str) -> CoreResult<RelayUrl> {
    raw.parse().map_err(|error| {
        CoreError::validation(
            "SYNC_P2P_RELAY_INVALID",
            format!("P2P relay 地址不合法（{raw}）：{error:?}"),
        )
    })
}

async fn bind_endpoint(secret: &SecretKey, relay: Option<&str>) -> CoreResult<Endpoint> {
    let mut builder = Endpoint::builder(presets::N0)
        .secret_key(secret.clone())
        .alpns(vec![ALPN.to_vec()]);
    match relay.map(str::trim).filter(|value| !value.is_empty()) {
        None => {}
        Some("disabled") => builder = builder.relay_mode(RelayMode::Disabled),
        Some(url) => builder = builder.relay_mode(RelayMode::custom([parse_relay_url(url)?])),
    }
    builder
        .bind()
        .await
        .map_err(|error| CoreError::io(format!("P2P 端点启动失败：{error:?}")))
}

/// 一次拨号会话的本地隧道。掉引用即关监听与 QUIC 连接（同步一轮结束自动释放）。
pub struct Tunnel {
    pub base_url: String,
    connection: Connection,
    listener_task: AbortHandle,
}

impl Tunnel {
    pub fn peer_id(&self) -> EndpointId {
        self.connection.remote_id()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.listener_task.abort();
        self.connection.close(0u32.into(), b"session done");
    }
}

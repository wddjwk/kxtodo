//! 剪切板同步（v0.8.9）：三台设备凭同一句同步口令互发剪切板。
//!
//! 设计口径：
//! - **不保存剪切板内容**：只在内存里过一手；落盘的只有设备密钥与用户设置。
//!   收到文件时例外——必须写到保存目录，否则系统剪切板无法引用它。
//! - 三种来源同时可用、按优先级择优：**直接 IP > 局域网发现 > iroh P2P**；
//!   同一台设备只发一次（设备身份 = 持久化的 iroh EndpointId，三种方式共用）。
//! - 局域网走 UDP 广播发现 + 自建 TCP；所有载荷用同步口令派生的
//!   ChaCha20-Poly1305 密钥加密，抓包看不到明文。P2P 复用传输助手那套
//!   iroh + pkarr 目录，但 ALPN / 房间前缀都是独立的。
//! - 接收端按自己的「类型开关 + 大小上限」裁剪，超限或未启用的类型直接丢弃；
//!   整条都不可用时只回 ok（不把对方的内容写进本机）。
//! - 消息 id 去重 5 分钟：同一台设备即使同时从 IP 与局域网各发一遍，也只落一次。
//! - 移动端受系统限制：Android 10+ 只有前台应用能读剪切板，因此移动端没有
//!   Rust 轮询线程，由 Kotlin 桥在前台读取后调用 `clipboard_local_changed`。

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey, TransportAddr};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use tokio::sync::Notify;

use crate::error::{CoreError, CoreResult};
use crate::host::HostBackend;
use crate::model::{ClipboardLimits, ClipboardSettings, SettingsFile};
use crate::repo::Layout;
use crate::sync::crypto::hmac_sha256;
use crate::sync::p2p::{directory, net};

/// 独立的 ALPN（不能与同步 `kxtodo-p2p/1`、传输 `kxtodo-transfer/1` 相同）。
pub const ALPN: &[u8] = b"kxtodo-clipboard/1";
/// 同步口令下限，与传输助手同一口径。
pub const CODE_MIN_CHARS: usize = 8;
/// 局域网发现端口（固定，所有设备一致）。
pub const DISCOVERY_PORT: u16 = 52178;
/// 局域网 TCP 默认端口；被占用时自动换临时端口并在 UI 里显示实际端口。
pub const LAN_PORT: u16 = 52179;
const APP: &str = "kxtodo-clipboard";
const PROTOCOL_VERSION: u32 = 1;
const REPUBLISH_SECS: u64 = 20;
const DEVICE_FRESH_SECS: u64 = 45;
const LAN_FRESH: Duration = Duration::from_secs(10);
const P2P_FRESH: Duration = Duration::from_secs(60);
const DIRECT_FRESH: Duration = Duration::from_secs(15);
const PEER_POLL: Duration = Duration::from_secs(2);
const DIRECT_POLL: Duration = Duration::from_secs(5);
const CLIPBOARD_POLL: Duration = Duration::from_millis(800);
const RECENT_TTL: Duration = Duration::from_secs(300);
const MAX_HANDSHAKE_FRAME: usize = 64 * 1024;
const MAX_CTRL_FRAME: usize = 1024 * 1024;
const MAX_BLOB_FRAME: usize = 1024 * 1024;
const CHUNK: usize = 256 * 1024;
const MAX_MB: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// 载荷与线路格式
// ---------------------------------------------------------------------------

/// 一条待发送的剪切板内容。文件可以带路径（本机剪切板）或字节（测试/内存来源）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardFile {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardContent {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    /// PNG 字节。桌面侧由 arboard 的 RGBA 转 PNG；移动端由 Kotlin 转 PNG。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ClipboardFile>,
}

impl ClipboardContent {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.html.is_empty() && self.image.is_none() && self.files.is_empty()
    }

    /// 内容指纹：文件只算「名字 + 大小」，**不算路径/修改时间**。
    /// 走到对端后文件会被改名落盘，若把路径算进指纹，回声抑制就永远对不上；
    /// 文本/图片则按实际字节算，远端写回后本机读到的指纹与发送时完全一致。
    pub fn signature(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"text\0");
        hasher.update(self.text.as_bytes());
        hasher.update(b"\0html\0");
        hasher.update(self.html.as_bytes());
        hasher.update(b"\0image\0");
        if let Some(image) = &self.image {
            hasher.update((image.len() as u64).to_le_bytes());
            hasher.update(image);
        }
        for file in &self.files {
            hasher.update(b"\0file\0");
            hasher.update(file.name.as_bytes());
            hasher.update((file_size(file).unwrap_or(0)).to_le_bytes());
        }
        hex(&hasher.finalize())
    }
}

fn file_size(file: &ClipboardFile) -> Option<u64> {
    if let Some(bytes) = &file.bytes {
        return Some(bytes.len() as u64);
    }
    file.path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hello {
    v: u32,
    app: String,
    device: String,
    name: String,
    nonce: String,
    tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelloAck {
    ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    device: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    nonce: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoverPacket {
    v: u32,
    app: String,
    device: String,
    name: String,
    port: u16,
    tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireFile {
    name: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireHeader {
    v: u32,
    app: String,
    /// `clipboard` | `ping`
    kind: String,
    id: String,
    origin: String,
    #[serde(default)]
    origin_name: String,
    #[serde(default)]
    at: u64,
    #[serde(default)]
    text_size: u64,
    #[serde(default)]
    html_size: u64,
    #[serde(default)]
    image_size: u64,
    #[serde(default)]
    files: Vec<WireFile>,
    #[serde(default)]
    total_size: u64,
    #[serde(default)]
    sig: String,
}

struct SessionInfo {
    key: [u8; 32],
    aad: Vec<u8>,
    peer_id: String,
    peer_name: String,
}

// ---------------------------------------------------------------------------
// 加解密与帧
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    hex(&buffer)
}

fn group_key(code: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"kxtodo-clipboard/v1/group/");
    hasher.update(code.as_bytes());
    hasher.finalize().into()
}

fn group_tag(key: &[u8; 32], label: &[u8]) -> String {
    hex(&hmac_sha256(key, label))
}

fn session_key(group: &[u8; 32], client_nonce: &str, server_nonce: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(group);
    hasher.update(b"|session|");
    hasher.update(client_nonce.as_bytes());
    hasher.update(b"|");
    hasher.update(server_nonce.as_bytes());
    hasher.finalize().into()
}

fn session_aad(client_nonce: &str, server_nonce: &str) -> Vec<u8> {
    format!("kxtodo-clipboard/v1|{client_nonce}|{server_nonce}").into_bytes()
}

fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> CoreResult<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad })
        .map_err(|_| CoreError::internal("剪切板加密失败"))?;
    let mut out = Vec::with_capacity(12 + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> CoreResult<Vec<u8>> {
    if sealed.len() < 12 + 16 {
        return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "加密帧长度不合法"));
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (nonce, body) = sealed.split_at(12);
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: body, aad })
        .map_err(|_| CoreError::validation("CLIPBOARD_DECRYPT", "剪切板内容解密失败：口令不一致？"))
}

async fn write_raw<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> CoreResult<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| CoreError::validation("CLIPBOARD_FRAME_TOO_LARGE", "帧过大"))?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(bytes).await?;
    Ok(())
}

async fn read_raw<R: AsyncRead + Unpin>(reader: &mut R, max: usize) -> CoreResult<Vec<u8>> {
    let len = reader.read_u32().await? as usize;
    if len == 0 || len > max {
        return Err(CoreError::validation(
            "CLIPBOARD_BAD_FRAME",
            format!("帧长度不合法：{len}"),
        ));
    }
    let mut buffer = vec![0u8; len];
    reader.read_exact(&mut buffer).await?;
    Ok(buffer)
}

async fn write_json<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> CoreResult<()> {
    let bytes = serde_json::to_vec(value)?;
    write_raw(writer, &bytes).await
}

async fn read_json<R: AsyncRead + Unpin>(reader: &mut R, max: usize) -> CoreResult<Value> {
    let bytes = read_raw(reader, max).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_FRAME", format!("JSON 帧不合法：{error}")))
}

async fn write_sealed<W: AsyncWrite + Unpin>(
    writer: &mut W,
    session: &SessionInfo,
    plain: &[u8],
) -> CoreResult<()> {
    let sealed = seal(&session.key, &session.aad, plain)?;
    write_raw(writer, &sealed).await
}

async fn read_sealed<R: AsyncRead + Unpin>(
    reader: &mut R,
    session: &SessionInfo,
    max: usize,
) -> CoreResult<Vec<u8>> {
    let sealed = read_raw(reader, max).await?;
    open(&session.key, &session.aad, &sealed)
}

async fn write_sealed_json<W: AsyncWrite + Unpin>(
    writer: &mut W,
    session: &SessionInfo,
    value: &Value,
) -> CoreResult<()> {
    let bytes = serde_json::to_vec(value)?;
    write_sealed(writer, session, &bytes).await
}

async fn read_sealed_json<R: AsyncRead + Unpin>(
    reader: &mut R,
    session: &SessionInfo,
    max: usize,
) -> CoreResult<Value> {
    let bytes = read_sealed(reader, session, max).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_FRAME", format!("JSON 帧不合法：{error}")))
}

// ---------------------------------------------------------------------------
// 身份与网络配置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardNet {
    pub relay: Option<String>,
    pub directory_url: String,
}

impl ClipboardNet {
    fn from_settings(settings: &SettingsFile) -> Self {
        let own = settings.transfer.relay.trim().to_string();
        let relay = if own.is_empty() {
            let shared = settings.sync.p2p_relay.trim().to_string();
            if shared.is_empty() { None } else { Some(shared) }
        } else {
            Some(own)
        };
        let custom = settings.sync.p2p_directory.trim().to_string();
        let directory_url = if custom.is_empty() {
            directory::DEFAULT_DIRECTORY_URL.to_string()
        } else {
            custom
        };
        Self { relay, directory_url }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IdentityFile {
    #[serde(default)]
    secret: String,
}

fn identity_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join("clipboard-identity.json")
}

fn identity_secret(layout: &Layout) -> CoreResult<SecretKey> {
    let path = identity_path(layout);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<IdentityFile>(&raw) {
            if let Some(secret) = decode_secret(&parsed.secret) {
                return Ok(secret);
            }
        }
    }
    let secret = SecretKey::generate();
    let payload = json!({ "secret": hex(&secret.to_bytes()) }).to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = crate::repo::atomic_write(&path, &payload);
    Ok(secret)
}

fn decode_secret(raw: &str) -> Option<SecretKey> {
    let raw = raw.trim();
    if raw.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for index in 0..32 {
        bytes[index] = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(SecretKey::from_bytes(&bytes))
}

fn default_device_name() -> String {
    let platform = match std::env::consts::OS {
        "windows" => "Windows",
        "android" => "Android",
        "linux" => "Linux",
        "macos" => "macOS",
        "ios" => "iOS",
        other => other,
    };
    format!("KXToDo·{platform}")
}

fn normalized(settings: &ClipboardSettings) -> ClipboardSettings {
    let mut value = settings.clone();
    value.code = value.code.trim().to_string();
    value.device_name = value.device_name.trim().to_string();
    value.direct_addresses = value.direct_addresses.trim().to_string();
    value.save_dir = value.save_dir.trim().to_string();
    value.limits.text_mb = value.limits.text_mb.clamp(1, 64);
    value.limits.html_mb = value.limits.html_mb.clamp(1, 64);
    value.limits.image_mb = value.limits.image_mb.clamp(1, 256);
    value.limits.files_mb = value.limits.files_mb.clamp(1, 1024);
    value
}

/// 一次写整份剪切板设置时的校验（GUI 的「保存并应用」走这里）。
pub fn normalize_settings(settings: &ClipboardSettings) -> CoreResult<ClipboardSettings> {
    let value = normalized(settings);
    // 关闭状态允许留一个还没填完的口令：用户清空口令后先关掉同步是正常操作，
    // 不必逼他先凑够 8 位。真正启动服务前 reconcile 会再检查一次。
    if value.enabled && !value.code.is_empty() && value.code.chars().count() < CODE_MIN_CHARS {
        return Err(CoreError::validation(
            "CLIPBOARD_CODE_TOO_SHORT",
            format!("同步口令至少 {CODE_MIN_CHARS} 位"),
        ));
    }
    Ok(value)
}

fn limit_bytes(limits: &ClipboardLimits) -> (u64, u64, u64, u64) {
    (
        limits.text_mb as u64 * MAX_MB,
        limits.html_mb as u64 * MAX_MB,
        limits.image_mb as u64 * MAX_MB,
        limits.files_mb as u64 * MAX_MB,
    )
}

fn parse_direct_addresses(raw: &str) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for item in raw.split(|ch: char| ch == ',' || ch == '\n' || ch == ';' || ch.is_whitespace()) {
        let text = item.trim();
        if text.is_empty() {
            continue;
        }
        let address = if let Ok(address) = text.parse::<SocketAddr>() {
            Some(address)
        } else if let Ok(ip) = text.parse::<IpAddr>() {
            Some(SocketAddr::new(ip, LAN_PORT))
        } else {
            let unbracketed = text.trim_start_matches('[').trim_end_matches(']');
            format!("[{unbracketed}]:{LAN_PORT}").parse::<SocketAddr>().ok()
        };
        if let Some(address) = address {
            if !out.contains(&address) {
                out.push(address);
            }
            if out.len() >= 16 {
                break;
            }
        }
    }
    out
}

fn direct_addrs(endpoint: &Endpoint) -> Vec<SocketAddr> {
    endpoint
        .addr()
        .addrs
        .iter()
        .filter_map(|addr| match addr {
            TransportAddr::Ip(socket) => Some(*socket),
            _ => None,
        })
        .collect()
}

fn local_addresses() -> Vec<String> {
    // UDP connect 不发包，只让系统选一条出口地址；离线时为空。
    std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|socket| {
            socket.connect("8.8.8.8:80").ok()?;
            let ip = socket.local_addr().ok()?.ip();
            (!ip.is_unspecified()).then(|| ip.to_string())
        })
        .into_iter()
        .collect()
}

// ---------------------------------------------------------------------------
// 服务状态
// ---------------------------------------------------------------------------

struct LanPeer {
    name: String,
    addr: SocketAddr,
    last_seen: Instant,
}

struct P2pPeer {
    name: String,
    addr: EndpointAddr,
    last_seen: Instant,
}

struct DirectPeer {
    device_id: String,
    name: String,
    last_ok: Instant,
}

struct Inner {
    layout: Layout,
    settings: ClipboardSettings,
    net: ClipboardNet,
    backend: Option<Arc<dyn HostBackend>>,
    signature: String,
    cancel: AtomicBool,
    notify: Notify,
    secret: SecretKey,
    device_id: String,
    name: String,
    group: [u8; 32],
    save_dir: PathBuf,
    tcp_port: Mutex<u16>,
    direct_addrs: Vec<SocketAddr>,
    endpoint: Mutex<Option<Endpoint>>,
    lan_peers: Mutex<BTreeMap<String, LanPeer>>,
    p2p_peers: Mutex<BTreeMap<String, P2pPeer>>,
    direct_peers: Mutex<BTreeMap<SocketAddr, DirectPeer>>,
    recent: Mutex<BTreeMap<String, Instant>>,
    last_local: Mutex<String>,
    last_remote: Mutex<String>,
    last_error: Mutex<String>,
    last_event: Mutex<Option<Value>>,
    last_status: Mutex<String>,
    sent_count: AtomicU64,
    received_count: AtomicU64,
}

static RUNNING: OnceLock<Mutex<BTreeMap<String, Arc<Inner>>>> = OnceLock::new();

fn running() -> &'static Mutex<BTreeMap<String, Arc<Inner>>> {
    RUNNING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn slot_key(layout: &Layout) -> String {
    layout.runtime_dir().to_string_lossy().to_string()
}

fn runtime() -> CoreResult<Arc<Runtime>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Runtime>>>> = OnceLock::new();
    let slot = SLOT.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| CoreError::internal("剪切板同步运行时锁中毒"))?;
    if let Some(existing) = guard.as_ref() {
        return Ok(existing.clone());
    }
    let built = RuntimeBuilder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| CoreError::internal(format!("剪切板同步运行时启动失败：{error}")))?;
    let shared = Arc::new(built);
    *guard = Some(shared.clone());
    Ok(shared)
}

/// 按当前设置启停剪切板同步。幂等：设置没变就什么都不做。
pub fn reconcile(
    layout: &Layout,
    settings: &SettingsFile,
    backend: Option<Arc<dyn HostBackend>>,
) -> CoreResult<()> {
    let config = normalize_settings(&settings.clipboard)?;
    let net = ClipboardNet::from_settings(settings);
    // relay / 目录也进签名：改了传输或同步的 relay，P2P 端点必须重建。
    let signature = serde_json::to_string(&(&config, &net))?;
    let key = slot_key(layout);
    let enabled = config.enabled
        && config.code.chars().count() >= CODE_MIN_CHARS
        && (config.lan_enabled || config.p2p_enabled || config.direct_enabled);
    if !enabled {
        stop(&key);
        return Ok(());
    }
    if let Some(existing) = running().lock().ok().and_then(|guard| guard.get(&key).cloned()) {
        if existing.signature == signature {
            return Ok(());
        }
    }
    stop(&key);
    let secret = identity_secret(layout)?;
    let device_id = secret.public().to_z32();
    let name = if config.device_name.is_empty() {
        default_device_name()
    } else {
        directory::sanitize_name(&config.device_name)
    };
    let save_dir = if config.save_dir.is_empty() {
        backend
            .as_ref()
            .and_then(|backend| backend.clipboard_default_dir())
            .unwrap_or_else(|| layout.root.join("clipboard-files"))
    } else {
        PathBuf::from(config.save_dir.trim())
    };
    std::fs::create_dir_all(&save_dir)?;
    let inner = Arc::new(Inner {
        layout: layout.clone(),
        group: group_key(&config.code),
        direct_addrs: parse_direct_addresses(&config.direct_addresses),
        net,
        backend,
        settings: config,
        signature,
        cancel: AtomicBool::new(false),
        notify: Notify::new(),
        secret,
        device_id,
        name,
        save_dir,
        tcp_port: Mutex::new(0),
        endpoint: Mutex::new(None),
        lan_peers: Mutex::new(BTreeMap::new()),
        p2p_peers: Mutex::new(BTreeMap::new()),
        direct_peers: Mutex::new(BTreeMap::new()),
        recent: Mutex::new(BTreeMap::new()),
        last_local: Mutex::new(String::new()),
        last_remote: Mutex::new(String::new()),
        last_error: Mutex::new(String::new()),
        last_event: Mutex::new(None),
        last_status: Mutex::new(String::new()),
        sent_count: AtomicU64::new(0),
        received_count: AtomicU64::new(0),
    });
    if let Ok(mut guard) = running().lock() {
        guard.insert(key.clone(), inner.clone());
    }
    let runtime = runtime()?;
    runtime.spawn(async move {
        run_service(inner).await;
    });
    Ok(())
}

fn stop(key: &str) {
    let Some(inner) = running().lock().ok().and_then(|mut guard| guard.remove(key)) else {
        return;
    };
    inner.cancel.store(true, Ordering::SeqCst);
    // notify_waiters 只唤醒“此刻在等”的任务；run_service 可能还在重试绑端口，
    // 还没走到 await。notify_one 会留下一个 permit，保证它随后一定能醒。
    inner.notify.notify_waiters();
    inner.notify.notify_one();
    let endpoint = inner.endpoint.lock().ok().and_then(|mut slot| slot.take());
    if let Some(endpoint) = endpoint {
        if let Ok(runtime) = runtime() {
            runtime.spawn(async move {
                endpoint.close().await;
            });
        }
    }
}

pub fn stop_for_layout(layout: &Layout) {
    stop(&slot_key(layout));
}

/// 状态快照。服务没跑时也返回「为什么没跑」，UI 不用自己猜。
pub fn status(layout: &Layout) -> Value {
    if let Some(inner) = running().lock().ok().and_then(|guard| guard.get(&slot_key(layout)).cloned()) {
        return inner.status_snapshot();
    }
    let settings = crate::repo::Repository::open_readonly(layout.root.clone())
        .load_settings()
        .unwrap_or_default();
    let config = normalized(&settings.clipboard);
    let code_ready = config.code.chars().count() >= CODE_MIN_CHARS;
    let reason = if !config.enabled {
        "未启用"
    } else if !code_ready {
        "请设置同步口令（至少 8 位）"
    } else if !(config.lan_enabled || config.p2p_enabled || config.direct_enabled) {
        "请至少启用一种同步方式"
    } else {
        "未运行"
    };
    let device_id = identity_secret(layout)
        .map(|secret| secret.public().to_z32())
        .unwrap_or_default();
    json!({
        "running": false,
        "enabled": config.enabled,
        "codeReady": code_ready,
        "deviceId": device_id,
        "deviceName": if config.device_name.is_empty() { default_device_name() } else { config.device_name.clone() },
        "port": 0,
        "discoveryPort": DISCOVERY_PORT,
        "localAddresses": [],
        "transports": {
            "direct": config.direct_enabled,
            "lan": config.lan_enabled,
            "p2p": config.p2p_enabled,
        },
        "peers": [],
        "lastError": reason,
        "lastEvent": Value::Null,
        "sent": 0,
        "received": 0,
        "saveDir": config.save_dir.clone(),
    })
}

/// 移动端前台读到的剪切板：交给服务去发送。桌面侧由 Rust 轮询线程直接调用。
pub fn local_changed(layout: &Layout, content: ClipboardContent) -> CoreResult<Value> {
    let Some(inner) = running().lock().ok().and_then(|guard| guard.get(&slot_key(layout)).cloned()) else {
        return Ok(json!({ "sent": 0, "running": false }));
    };
    inner.on_local(content);
    Ok(json!({ "queued": true }))
}

impl Inner {
    fn status_snapshot(&self) -> Value {
        let port = self.tcp_port.lock().map(|slot| *slot).unwrap_or(0);
        json!({
            "running": !self.cancel.load(Ordering::SeqCst),
            "enabled": true,
            "codeReady": true,
            "deviceId": self.device_id,
            "deviceName": self.name,
            "port": port,
            "discoveryPort": DISCOVERY_PORT,
            "localAddresses": local_addresses(),
            "transports": {
                "direct": self.settings.direct_enabled,
                "lan": self.settings.lan_enabled,
                "p2p": self.settings.p2p_enabled,
            },
            "peers": self.peer_list(),
            "lastError": self.last_error.lock().map(|slot| slot.clone()).unwrap_or_default(),
            "lastEvent": self.last_event.lock().ok().and_then(|slot| slot.clone()).unwrap_or(Value::Null),
            "sent": self.sent_count.load(Ordering::Relaxed),
            "received": self.received_count.load(Ordering::Relaxed),
            "saveDir": self.save_dir.to_string_lossy(),
        })
    }

    fn set_error(&self, message: impl Into<String>) {
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = message.into();
        }
        self.emit_status();
    }

    fn set_event(&self, kind: &str, detail: impl Into<String>, peer: impl Into<String>) {
        if let Ok(mut slot) = self.last_event.lock() {
            *slot = Some(json!({
                "at": crate::time::now_iso(),
                "kind": kind,
                "detail": detail.into(),
                "peer": peer.into(),
            }));
        }
        self.emit_status();
    }

    fn emit(&self, payload: Value) {
        if let Some(backend) = self.backend.as_ref() {
            backend.emit("kxtodo://clipboard", payload);
        }
    }

    fn emit_status(&self) {
        let snapshot = self.status_snapshot();
        let rendered = match serde_json::to_string(&snapshot) {
            Ok(value) => value,
            Err(_) => return,
        };
        let changed = self
            .last_status
            .lock()
            .map(|mut slot| {
                if *slot == rendered {
                    false
                } else {
                    *slot = rendered.clone();
                    true
                }
            })
            .unwrap_or(false);
        if changed {
            self.emit(json!({ "kind": "status", "status": snapshot }));
        }
    }

    fn peer_list(&self) -> Vec<Value> {
        let mut merged: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        if let Ok(peers) = self.lan_peers.lock() {
            for (id, peer) in peers.iter() {
                if peer.last_seen.elapsed() <= LAN_FRESH {
                    let entry = merged.entry(id.clone()).or_insert_with(|| (peer.name.clone(), Vec::new()));
                    if entry.0.is_empty() {
                        entry.0 = peer.name.clone();
                    }
                    entry.1.push("lan".to_string());
                }
            }
        }
        if let Ok(peers) = self.p2p_peers.lock() {
            for (id, peer) in peers.iter() {
                if peer.last_seen.elapsed() <= P2P_FRESH {
                    let entry = merged.entry(id.clone()).or_insert_with(|| (peer.name.clone(), Vec::new()));
                    if entry.0.is_empty() {
                        entry.0 = peer.name.clone();
                    }
                    entry.1.push("p2p".to_string());
                }
            }
        }
        if let Ok(peers) = self.direct_peers.lock() {
            for peer in peers.values() {
                if peer.last_ok.elapsed() <= DIRECT_FRESH {
                    let entry = merged.entry(peer.device_id.clone()).or_insert_with(|| (peer.name.clone(), Vec::new()));
                    if entry.0.is_empty() {
                        entry.0 = peer.name.clone();
                    }
                    entry.1.push("direct".to_string());
                }
            }
        }
        let priority = ["direct", "lan", "p2p"];
        let mut out: Vec<Value> = merged
            .into_iter()
            .map(|(id, (name, transports))| {
                let mut transports = transports;
                transports.sort_by_key(|item| priority.iter().position(|value| value == item).unwrap_or(9));
                transports.dedup();
                json!({
                    "id": id,
                    "name": if name.is_empty() { "未命名设备" } else { name.as_str() },
                    "transports": transports,
                    "preferred": transports.first().cloned().unwrap_or_default(),
                    "online": true,
                })
            })
            .collect();
        out.sort_by(|left, right| {
            let a = left["name"].as_str().unwrap_or("");
            let b = right["name"].as_str().unwrap_or("");
            a.cmp(b).then_with(|| left["id"].as_str().unwrap_or("").cmp(right["id"].as_str().unwrap_or("")))
        });
        out
    }

    fn on_local(self: &Arc<Self>, content: ClipboardContent) {
        if content.is_empty() || self.cancel.load(Ordering::SeqCst) {
            return;
        }
        let signature = content.signature();
        let remote = self.last_remote.lock().map(|slot| slot.clone()).unwrap_or_default();
        {
            let Ok(mut slot) = self.last_local.lock() else { return };
            if *slot == signature {
                return;
            }
            *slot = signature.clone();
        }
        if remote == signature {
            return;
        }
        let inner = self.clone();
        if let Ok(runtime) = runtime() {
            runtime.spawn(async move {
                inner.send_all(content).await;
            });
        }
    }

    fn push_peer(&self, device_id: &str, name: &str, addr: SocketAddr) {
        if let Ok(mut peers) = self.lan_peers.lock() {
            let is_new = !peers.contains_key(device_id);
            peers.insert(
                device_id.to_string(),
                LanPeer { name: name.to_string(), addr, last_seen: Instant::now() },
            );
            if is_new {
                self.emit_status();
            }
        }
    }

    fn recent_seen(&self, id: &str) -> bool {
        let Ok(recent) = self.recent.lock() else { return false };
        recent
            .get(id)
            .is_some_and(|at| at.elapsed() < RECENT_TTL)
    }

    fn remember_recent(&self, id: &str) {
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() > 512 {
                recent.retain(|_, at| at.elapsed() < RECENT_TTL);
            }
            recent.insert(id.to_string(), Instant::now());
        }
    }

    fn forget_recent(&self, id: &str) {
        if let Ok(mut recent) = self.recent.lock() {
            recent.remove(id);
        }
    }
}

// ---------------------------------------------------------------------------
// 启动与传输循环
// ---------------------------------------------------------------------------

async fn run_service(inner: Arc<Inner>) {
    cleanup_inbox(&inner);
    if inner.settings.lan_enabled {
        // 设置保存 = 停旧服务再起新服务；旧监听协程可能还没跑到 drop。默认端口先重试
        // 一小段时间，避免每次保存都把端口换成一个随机值（直接 IP 的用户会莫名其妙连不上）。
        let mut listener = None;
        for _ in 0..6 {
            match TcpListener::bind(("0.0.0.0", LAN_PORT)).await {
                Ok(bound) => {
                    listener = Some(bound);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(120)).await,
            }
        }
        let listener = match listener {
            Some(listener) => Some(listener),
            None => TcpListener::bind(("0.0.0.0", 0)).await.ok(),
        };
        match listener {
            Some(listener) => {
                let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(LAN_PORT);
                if let Ok(mut slot) = inner.tcp_port.lock() {
                    *slot = port;
                }
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    lan_accept_loop(task, listener).await;
                });
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    discovery_loop(task).await;
                });
            }
            None => inner.set_error("局域网监听失败：端口不可用"),
        }
    }
    if inner.settings.direct_enabled && !inner.direct_addrs.is_empty() {
        let task = inner.clone();
        inner_runtime_spawn(async move {
            direct_loop(task).await;
        });
    }
    if inner.settings.p2p_enabled {
        let task = inner.clone();
        inner_runtime_spawn(async move {
            if let Err(error) = p2p_loop(task.clone()).await {
                if !task.cancel.load(Ordering::SeqCst) {
                    task.set_error(format!("P2P 启动失败：{}", error.message));
                }
            }
        });
    }
    if inner.backend.as_ref().is_some_and(|backend| backend.clipboard_available()) {
        spawn_native_poller(inner.clone());
    }
    inner.emit_status();
    if inner.cancel.load(Ordering::SeqCst) {
        return;
    }
    inner.notify.notified().await;
}

fn inner_runtime_spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(runtime) = runtime() {
        runtime.spawn(future);
    }
}

fn cleanup_inbox(inner: &Inner) {
    let inbox = inner.layout.runtime_dir().join("clipboard-inbox");
    let Ok(entries) = std::fs::read_dir(&inbox) else { return };
    for entry in entries.flatten() {
        if entry.metadata().ok().and_then(|meta| meta.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn spawn_native_poller(inner: Arc<Inner>) {
    if let Some(backend) = inner.backend.as_ref() {
        backend.clipboard_reset_cache();
    }
    let _ = std::thread::Builder::new()
        .name("kxtodo-clipboard".to_string())
        .spawn(move || {
            let mut primed = false;
            loop {
                if inner.cancel.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(backend) = inner.backend.as_ref() {
                    match backend.clipboard_read() {
                        Ok(Some(content)) if !content.is_empty() => {
                            let signature = content.signature();
                            if !primed {
                                primed = true;
                                if let Ok(mut slot) = inner.last_local.lock() {
                                    *slot = signature;
                                }
                            } else {
                                inner.on_local(content);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            // 剪切板偶尔被别的进程短暂占用是常态，不要把它写成大红错误；
                            // 连续失败由状态里的 lastError 兜底（只保留最后一条）。
                            if inner.last_error.lock().map(|slot| slot.is_empty()).unwrap_or(false) {
                                inner.set_error(format!("读取本机剪切板失败：{error}"));
                            }
                        }
                    }
                }
                std::thread::sleep(CLIPBOARD_POLL);
            }
        });
}

// ---------------------------------------------------------------------------
// 局域网发现
// ---------------------------------------------------------------------------

async fn discovery_loop(inner: Arc<Inner>) {
    if inner.cancel.load(Ordering::SeqCst) {
        return;
    }
    let mut socket = None;
    let mut last_error = None;
    for _ in 0..6 {
        match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
            Ok(bound) => {
                socket = Some(bound);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    }
    let Some(socket) = socket else {
        inner.set_error(format!(
            "局域网发现端口 {DISCOVERY_PORT} 不可用：{}",
            last_error.map(|error| error.to_string()).unwrap_or_default()
        ));
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.join_multicast_v4(Ipv4Addr::new(239, 255, 77, 53), Ipv4Addr::UNSPECIFIED);
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    let mut buffer = vec![0u8; 2048];
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            _ = interval.tick() => {
                let announce = self_announce(&inner);
                let bytes = serde_json::to_vec(&announce).unwrap_or_default();
                let _ = socket.send_to(&bytes, ("255.255.255.255", DISCOVERY_PORT)).await;
                let _ = socket.send_to(&bytes, ("239.255.77.53", DISCOVERY_PORT)).await;
                prune_lan(&inner);
            }
            result = socket.recv_from(&mut buffer) => {
                let Ok((size, source)) = result else { continue };
                let Ok(packet) = serde_json::from_slice::<DiscoverPacket>(&buffer[..size]) else { continue };
                if packet.v != PROTOCOL_VERSION || packet.app != APP || packet.device == inner.device_id {
                    continue;
                }
                if packet.tag != group_tag(&inner.group, b"discover") || packet.port == 0 {
                    continue;
                }
                let addr = SocketAddr::new(source.ip(), packet.port);
                let is_new = inner.lan_peers.lock().map(|peers| !peers.contains_key(&packet.device)).unwrap_or(false);
                inner.push_peer(&packet.device, &packet.name, addr);
                if is_new {
                    let reply = serde_json::to_vec(&self_announce(&inner)).unwrap_or_default();
                    let _ = socket.send_to(&reply, source).await;
                }
            }
        }
    }
}

fn self_announce(inner: &Inner) -> DiscoverPacket {
    DiscoverPacket {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        port: inner.tcp_port.lock().map(|slot| *slot).unwrap_or(0),
        tag: group_tag(&inner.group, b"discover"),
    }
}

fn prune_lan(inner: &Inner) {
    if let Ok(mut peers) = inner.lan_peers.lock() {
        let before = peers.len();
        peers.retain(|_, peer| peer.last_seen.elapsed() <= LAN_FRESH);
        if peers.len() != before {
            drop(peers);
            inner.emit_status();
        }
    }
}

// ---------------------------------------------------------------------------
// 局域网 TCP
// ---------------------------------------------------------------------------

async fn lan_accept_loop(inner: Arc<Inner>, listener: TcpListener) {
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            result = listener.accept() => {
                let Ok((stream, _addr)) = result else { continue };
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(300), async {
                        let (mut reader, mut writer) = stream.into_split();
                        if let Err(error) = serve_stream(&task, &mut reader, &mut writer).await {
                            let _ = write_json(&mut writer, &json!({ "kind": "error", "message": error.message })).await;
                        }
                    }).await;
                });
            }
        }
    }
}

async fn serve_stream<R, W>(inner: &Arc<Inner>, reader: &mut R, writer: &mut W) -> CoreResult<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let session = server_handshake(inner, reader, writer).await?;
    let header = read_sealed_json::<R>(reader, &session, MAX_CTRL_FRAME).await?;
    let parsed: WireHeader = serde_json::from_value(header.clone())
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_FRAME", format!("头部不合法：{error}")))?;
    if parsed.v != PROTOCOL_VERSION || parsed.app != APP {
        let _ = write_sealed_json(writer, &session, &json!({ "ok": false, "reason": "协议版本不匹配" })).await;
        return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "协议版本不匹配"));
    }
    if parsed.kind == "ping" {
        write_sealed_json(writer, &session, &json!({ "ok": true })).await?;
        let _ = writer.shutdown().await;
        return Ok(());
    }
    match receive_clipboard(inner, reader, &session, &parsed).await {
        Ok(accepted) => {
            write_sealed_json(writer, &session, &json!({ "ok": true, "accepted": accepted })).await?;
            let _ = writer.shutdown().await;
            Ok(())
        }
        Err(error) => {
            let _ = write_sealed_json(
                writer,
                &session,
                &json!({ "ok": false, "reason": error.message, "code": error.code }),
            )
            .await;
            let _ = writer.shutdown().await;
            Err(error)
        }
    }
}

async fn server_handshake<R, W>(
    inner: &Arc<Inner>,
    reader: &mut R,
    writer: &mut W,
) -> CoreResult<SessionInfo>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let hello: Hello = serde_json::from_value(read_json(reader, MAX_HANDSHAKE_FRAME).await?)
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_HELLO", format!("握手不合法：{error}")))?;
    if hello.v != PROTOCOL_VERSION || hello.app != APP {
        write_json(writer, &json!({ "ok": false, "reason": "协议版本不匹配" })).await?;
        return Err(CoreError::validation("CLIPBOARD_BAD_HELLO", "协议版本不匹配"));
    }
    if hello.tag != group_tag(&inner.group, b"hello") {
        write_json(writer, &json!({ "ok": false, "reason": "同步口令不一致" })).await?;
        return Err(CoreError::validation("CLIPBOARD_CODE_MISMATCH", "同步口令不一致"));
    }
    if hello.device == inner.device_id {
        return Err(CoreError::validation("CLIPBOARD_SELF", "不能与自己同步"));
    }
    let server_nonce = random_hex(16);
    let ack = HelloAck {
        ok: true,
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        nonce: server_nonce.clone(),
        reason: String::new(),
    };
    write_json(writer, &serde_json::to_value(&ack)?).await?;
    let key = session_key(&inner.group, &hello.nonce, &server_nonce);
    let aad = session_aad(&hello.nonce, &server_nonce);
    Ok(SessionInfo {
        key,
        aad,
        peer_id: hello.device,
        peer_name: if hello.name.is_empty() { "未命名设备".to_string() } else { hello.name },
    })
}

async fn client_handshake<R, W>(
    inner: &Inner,
    reader: &mut R,
    writer: &mut W,
) -> CoreResult<SessionInfo>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let client_nonce = random_hex(16);
    let hello = Hello {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        nonce: client_nonce.clone(),
        tag: group_tag(&inner.group, b"hello"),
    };
    write_json(writer, &serde_json::to_value(&hello)?).await?;
    let ack: HelloAck = serde_json::from_value(read_json(reader, MAX_HANDSHAKE_FRAME).await?)
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_HELLO", format!("握手应答不合法：{error}")))?;
    if !ack.ok {
        return Err(CoreError::validation(
            "CLIPBOARD_REJECTED",
            if ack.reason.is_empty() { "对方拒绝了连接".to_string() } else { ack.reason },
        ));
    }
    if ack.device.is_empty() || ack.nonce.is_empty() {
        return Err(CoreError::validation("CLIPBOARD_BAD_HELLO", "握手应答缺少设备信息"));
    }
    let key = session_key(&inner.group, &client_nonce, &ack.nonce);
    let aad = session_aad(&client_nonce, &ack.nonce);
    Ok(SessionInfo { key, aad, peer_id: ack.device, peer_name: ack.name })
}

async fn send_lan(
    inner: &Arc<Inner>,
    addr: SocketAddr,
    content: &ClipboardContent,
) -> CoreResult<(String, String)> {
    let stream = tokio::time::timeout(Duration::from_secs(6), TcpStream::connect(addr))
        .await
        .map_err(|_| CoreError::execution("CLIPBOARD_TIMEOUT", "连接对方超时"))??;
    let (mut reader, mut writer) = stream.into_split();
    let session = client_handshake(inner, &mut reader, &mut writer).await?;
    send_clipboard(inner, &mut reader, &mut writer, &session, content).await?;
    if let Ok(mut peers) = inner.direct_peers.lock() {
        peers.insert(
            addr,
            DirectPeer {
                device_id: session.peer_id.clone(),
                name: session.peer_name.clone(),
                last_ok: Instant::now(),
            },
        );
    }
    Ok((session.peer_id, session.peer_name))
}

// ---------------------------------------------------------------------------
// 发送
// ---------------------------------------------------------------------------

async fn send_clipboard<R, W>(
    inner: &Inner,
    reader: &mut R,
    writer: &mut W,
    session: &SessionInfo,
    content: &ClipboardContent,
) -> CoreResult<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (text_limit, html_limit, image_limit, files_limit) = limit_bytes(&inner.settings.limits);
    let mut outgoing = ClipboardContent::default();
    if !content.text.is_empty() && content.text.len() as u64 <= text_limit {
        outgoing.text = content.text.clone();
    }
    if !content.html.is_empty() && content.html.len() as u64 <= html_limit {
        outgoing.html = content.html.clone();
    }
    if let Some(image) = &content.image {
        if image.len() as u64 <= image_limit {
            outgoing.image = Some(image.clone());
        }
    }
    let mut total_files = 0u64;
    for file in &content.files {
        let Some(size) = file_size(file) else { continue };
        if total_files.saturating_add(size) > files_limit {
            break;
        }
        total_files += size;
        outgoing.files.push(file.clone());
    }
    if outgoing.is_empty() {
        return Err(CoreError::validation(
            "CLIPBOARD_OVERSIZED",
            "剪切板内容超过本机设置的类型/大小上限，未发送",
        ));
    }
    let files: Vec<WireFile> = outgoing
        .files
        .iter()
        .filter_map(|file| file_size(file).map(|size| WireFile { name: clipboard_file_name(file), size }))
        .collect();
    let header = WireHeader {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        kind: "clipboard".to_string(),
        id: random_hex(16),
        origin: inner.device_id.clone(),
        origin_name: inner.name.clone(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0),
        text_size: outgoing.text.len() as u64,
        html_size: outgoing.html.len() as u64,
        image_size: outgoing.image.as_ref().map(|image| image.len() as u64).unwrap_or(0),
        files,
        total_size: outgoing.text.len() as u64
            + outgoing.html.len() as u64
            + outgoing.image.as_ref().map(|image| image.len() as u64).unwrap_or(0)
            + total_files,
        sig: outgoing.signature(),
    };
    write_sealed_json(writer, session, &serde_json::to_value(&header)?).await?;
    send_blob(writer, session, outgoing.text.as_bytes()).await?;
    send_blob(writer, session, outgoing.html.as_bytes()).await?;
    if let Some(image) = &outgoing.image {
        send_blob(writer, session, image).await?;
    }
    for file in &outgoing.files {
        send_file(writer, session, file).await?;
    }
    // 半关闭写方向：iroh 的 SendStream 不 finish 就 drop 会把在途数据重置成
    // connection lost（TCP 上则让对端明确看到「我说完了」）。
    let _ = writer.shutdown().await;
    let ack = read_sealed_json::<R>(reader, session, MAX_CTRL_FRAME).await?;
    if ack["ok"].as_bool() != Some(true) {
        let reason = ack["reason"].as_str().unwrap_or("对方未接受");
        return Err(CoreError::execution("CLIPBOARD_REJECTED", reason.to_string()));
    }
    Ok(())
}

fn clipboard_file_name(file: &ClipboardFile) -> String {
    if !file.name.trim().is_empty() {
        return sanitize_file_name(&file.name);
    }
    file.path
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| sanitize_file_name(&name.to_string_lossy()))
        .unwrap_or_else(|| "file".to_string())
}

async fn send_blob<W>(writer: &mut W, session: &SessionInfo, bytes: &[u8]) -> CoreResult<()>
where
    W: AsyncWrite + Unpin,
{
    for chunk in bytes.chunks(CHUNK) {
        write_sealed(writer, session, chunk).await?;
    }
    Ok(())
}

async fn send_file<W>(writer: &mut W, session: &SessionInfo, file: &ClipboardFile) -> CoreResult<()>
where
    W: AsyncWrite + Unpin,
{
    if let Some(bytes) = &file.bytes {
        return send_blob(writer, session, bytes).await;
    }
    let Some(path) = file.path.as_ref() else {
        return Ok(());
    };
    let mut handle = tokio::fs::File::open(path)
        .await
        .map_err(|error| CoreError::io(format!("读取待发送文件失败：{error}")))?;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = handle
            .read(&mut buffer)
            .await
            .map_err(|error| CoreError::io(format!("读取待发送文件失败：{error}")))?;
        if read == 0 {
            break;
        }
        write_sealed(writer, session, &buffer[..read]).await?;
    }
    Ok(())
}

#[derive(Clone)]
struct Target {
    name: String,
    direct: Option<SocketAddr>,
    lan: Option<SocketAddr>,
    p2p: Option<EndpointAddr>,
}

impl Inner {
    fn targets(&self) -> Vec<Target> {
        let mut map: BTreeMap<String, Target> = BTreeMap::new();
        if let Ok(peers) = self.lan_peers.lock() {
            for (id, peer) in peers.iter() {
                if peer.last_seen.elapsed() > LAN_FRESH {
                    continue;
                }
                let entry = map.entry(id.clone()).or_insert_with(|| Target {
                    name: peer.name.clone(),
                    direct: None,
                    lan: None,
                    p2p: None,
                });
                if entry.name.is_empty() {
                    entry.name = peer.name.clone();
                }
                entry.lan = Some(peer.addr);
            }
        }
        if let Ok(peers) = self.p2p_peers.lock() {
            for (id, peer) in peers.iter() {
                if peer.last_seen.elapsed() > P2P_FRESH {
                    continue;
                }
                let entry = map.entry(id.clone()).or_insert_with(|| Target {
                    name: peer.name.clone(),
                    direct: None,
                    lan: None,
                    p2p: None,
                });
                if entry.name.is_empty() {
                    entry.name = peer.name.clone();
                }
                entry.p2p = Some(peer.addr.clone());
            }
        }
        if let Ok(peers) = self.direct_peers.lock() {
            for (addr, peer) in peers.iter() {
                if peer.last_ok.elapsed() > DIRECT_FRESH {
                    continue;
                }
                let entry = map.entry(peer.device_id.clone()).or_insert_with(|| Target {
                    name: peer.name.clone(),
                    direct: None,
                    lan: None,
                    p2p: None,
                });
                if entry.name.is_empty() {
                    entry.name = peer.name.clone();
                }
                entry.direct = Some(*addr);
            }
        }
        // 填了 IP 但还没 ping 通：也照发一次，连上后握手会告诉我们它是谁。
        // 先按「地址完全相同」把它并到已发现的 LAN/P2P 设备上——同一台机器同时被
        // 广播和手填 IP 找到时，只能发一份。地址不同（例如对方换了端口）才作为
        // 未知目标单列；接收端的消息 id 去重会兜住这类极端情况。
        for addr in &self.direct_addrs {
            let exact = self.lan_peers.lock().ok().and_then(|peers| {
                peers
                    .iter()
                    .find(|(_, peer)| peer.addr == *addr)
                    .map(|(id, _)| id.clone())
            });
            if let Some(id) = exact {
                if let Some(target) = map.get_mut(&id) {
                    target.direct = Some(*addr);
                }
                continue;
            }
            let known = self.direct_peers.lock().ok().map(|peers| peers.contains_key(addr)).unwrap_or(false);
            if known {
                continue;
            }
            map.entry(format!("direct:{addr}")).or_insert(Target {
                name: addr.to_string(),
                direct: Some(*addr),
                lan: None,
                p2p: None,
            });
        }
        map.into_values().collect()
    }

    async fn send_all(self: Arc<Self>, content: ClipboardContent) {        let targets = self.targets();
        if targets.is_empty() {
            self.set_event("sent", "没有在线设备，内容未发送", "");
            return;
        }
        let mut tasks = Vec::new();
        for target in targets {
            let inner = self.clone();
            let payload = content.clone();
            tasks.push(tokio::spawn(async move {
                let name = target.name.clone();
                match send_target(inner, &target, &payload).await {
                    Ok(()) => Ok(name),
                    Err(error) => Err((name, error.message)),
                }
            }));
        }
        let mut sent = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for task in tasks {
            match task.await {
                Ok(Ok(_)) => sent += 1,
                Ok(Err((name, error))) => failures.push(format!("{name}：{error}")),
                Err(_) => failures.push("发送任务异常".to_string()),
            }
        }
        self.sent_count.fetch_add(sent as u64, Ordering::Relaxed);
        if sent > 0 {
            self.set_event("sent", format!("已发送到 {sent} 台设备"), "");
        }
        if !failures.is_empty() {
            self.set_error(failures.join("；"));
        }
    }

}

async fn send_target(
    inner: Arc<Inner>,
    target: &Target,
    content: &ClipboardContent,
) -> CoreResult<()> {
    let mut last_error: Option<CoreError> = None;
    if let Some(addr) = target.direct {
        match send_lan(&inner, addr, content).await {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    if let Some(addr) = target.lan {
        if Some(addr) != target.direct {
            match send_lan(&inner, addr, content).await {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
    }
    if let Some(addr) = &target.p2p {
        match send_p2p(&inner, addr.clone(), content).await {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        CoreError::execution("CLIPBOARD_NO_ROUTE", format!("没有可用线路：{}", target.name))
    }))
}

async fn send_p2p(
    inner: &Inner,
    addr: EndpointAddr,
    content: &ClipboardContent,
) -> CoreResult<()> {
    let endpoint = inner
        .endpoint
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .ok_or_else(|| CoreError::execution("CLIPBOARD_P2P_OFFLINE", "P2P 端点未启动"))?;
    let connection = tokio::time::timeout(Duration::from_secs(25), endpoint.connect(addr, ALPN))
        .await
        .map_err(|_| CoreError::execution("CLIPBOARD_TIMEOUT", "P2P 拨号超时"))?
        .map_err(|error| CoreError::io(format!("P2P 拨号失败：{error:?}")))?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| CoreError::io(format!("P2P 流打开失败：{error:?}")))?;
    let session = client_handshake(inner, &mut recv, &mut send).await?;
    send_clipboard(inner, &mut recv, &mut send, &session, content).await?;
    let _ = send.finish();
    connection.close(0u32.into(), b"clipboard done");
    Ok(())
}

// ---------------------------------------------------------------------------
// 接收
// ---------------------------------------------------------------------------

async fn receive_clipboard<R>(
    inner: &Arc<Inner>,
    reader: &mut R,
    session: &SessionInfo,
    header: &WireHeader,
) -> CoreResult<usize>
where
    R: AsyncRead + Unpin,
{
    if header.origin == inner.device_id {
        return Ok(0);
    }
    if header.id.is_empty() || header.sig.is_empty() {
        return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "消息缺少 id/指纹"));
    }
    if inner.recent_seen(&header.id) {
        // 同一条消息从两条线路各来一遍：已经落过，直接回 ok。
        return Ok(0);
    }
    // 先占位再读：两条线路并发到达时只有一个能写；读/写失败要撤回占位，
    // 否则对方换一条线路重试会被当成「已经收过」。
    inner.remember_recent(&header.id);
    let result = receive_clipboard_claimed(inner, reader, session, header).await;
    if result.is_err() {
        inner.forget_recent(&header.id);
    }
    result
}

async fn receive_clipboard_claimed<R>(
    inner: &Arc<Inner>,
    reader: &mut R,
    session: &SessionInfo,
    header: &WireHeader,
) -> CoreResult<usize>
where
    R: AsyncRead + Unpin,
{
    let (text_limit, html_limit, image_limit, files_limit) = limit_bytes(&inner.settings.limits);
    let hard_cap = files_limit.max(image_limit).max(text_limit).max(html_limit).saturating_mul(2);
    let total = header
        .text_size
        .saturating_add(header.html_size)
        .saturating_add(header.image_size)
        .saturating_add(header.files.iter().map(|file| file.size).sum::<u64>());
    if total > hard_cap + 16 * MAX_MB {
        return Err(CoreError::validation("CLIPBOARD_OVERSIZED", "对方内容超过硬上限"));
    }
    let types = &inner.settings.types;
    let accept_text = types.text && header.text_size > 0 && header.text_size <= text_limit;
    let accept_html = types.html && header.html_size > 0 && header.html_size <= html_limit;
    let accept_image = types.image && header.image_size > 0 && header.image_size <= image_limit;
    let files_total: u64 = header.files.iter().map(|file| file.size).sum();
    let accept_files = types.files && !header.files.is_empty() && files_total <= files_limit;

    let mut content = ClipboardContent::default();
    let text_bytes = read_blob(reader, session, header.text_size, accept_text).await?;
    if let Some(bytes) = text_bytes {
        content.text = String::from_utf8_lossy(&bytes).to_string();
    }
    let html_bytes = read_blob(reader, session, header.html_size, accept_html).await?;
    if let Some(bytes) = html_bytes {
        content.html = String::from_utf8_lossy(&bytes).to_string();
    }
    let image_bytes = read_blob(reader, session, header.image_size, accept_image).await?;
    if let Some(bytes) = image_bytes {
        content.image = Some(bytes);
    }
    let mut files = Vec::new();
    if accept_files {
        tokio::fs::create_dir_all(&inner.save_dir)
            .await
            .map_err(|error| CoreError::io(format!("创建保存目录失败：{error}")))?;
    }
    for file in &header.files {
        let target = if accept_files {
            Some(unique_target(&inner.save_dir, &sanitize_file_name(&file.name)).await?)
        } else {
            None
        };
        if let Some(path) = target {
            let mut handle = tokio::fs::File::create(&path)
                .await
                .map_err(|error| CoreError::io(format!("写入接收文件失败：{error}")))?;
            let mut remaining = file.size;
            while remaining > 0 {
                let chunk = read_sealed(reader, session, MAX_BLOB_FRAME).await?;
                let take = chunk.len().min(remaining as usize);
                handle
                    .write_all(&chunk[..take])
                    .await
                    .map_err(|error| CoreError::io(format!("写入接收文件失败：{error}")))?;
                remaining -= take as u64;
                if take < chunk.len() {
                    return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "文件帧超出声明大小"));
                }
            }
            handle
                .flush()
                .await
                .map_err(|error| CoreError::io(format!("写入接收文件失败：{error}")))?;
            files.push(ClipboardFile {
                name: file.name.clone(),
                path: Some(path),
                bytes: None,
            });
        } else {
            let mut remaining = file.size;
            while remaining > 0 {
                let chunk = read_sealed(reader, session, MAX_BLOB_FRAME).await?;
                if chunk.len() as u64 > remaining {
                    return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "文件帧超出声明大小"));
                }
                remaining -= chunk.len() as u64;
            }
        }
    }
    content.files = files;
    if content.is_empty() {
        inner.set_event("ignored", "收到内容，但类型被关闭或超过上限", session.peer_name.clone());
        return Ok(0);
    }
    inner.apply_remote(content, &session.peer_name).await?;
    Ok(1)
}

async fn read_blob<R>(
    reader: &mut R,
    session: &SessionInfo,
    size: u64,
    keep: bool,
) -> CoreResult<Option<Vec<u8>>>
where
    R: AsyncRead + Unpin,
{
    let mut remaining = size;
    let mut bytes = keep.then(|| Vec::with_capacity(size.min(8 * MAX_MB) as usize));
    while remaining > 0 {
        let chunk = read_sealed(reader, session, MAX_BLOB_FRAME).await?;
        if chunk.len() as u64 > remaining {
            return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "数据帧超出声明大小"));
        }
        remaining -= chunk.len() as u64;
        if let Some(buffer) = bytes.as_mut() {
            buffer.extend_from_slice(&chunk);
        }
    }
    Ok(bytes)
}

async fn unique_target(root: &Path, name: &str) -> CoreResult<PathBuf> {
    let name = sanitize_file_name(name);
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name, String::new()),
    };
    for index in 0..1000 {
        let candidate = if index == 0 {
            root.join(format!("{stem}{extension}"))
        } else {
            root.join(format!("{stem} ({index}){extension}"))
        };
        if tokio::fs::metadata(&candidate).await.is_err() {
            return Ok(candidate);
        }
    }
    Err(CoreError::execution("CLIPBOARD_NAME_EXHAUSTED", "同名文件过多，无法保存"))
}

fn sanitize_file_name(raw: &str) -> String {
    let base = raw
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(raw)
        .chars()
        .filter(|ch| !ch.is_control() && !matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .take(120)
        .collect::<String>();
    let trimmed = base.trim().trim_matches('.').to_string();
    if trimmed.is_empty() { "file".to_string() } else { trimmed }
}

impl Inner {
    async fn apply_remote(&self, content: ClipboardContent, peer_name: &str) -> CoreResult<()> {
        // 总开关刚关掉时，可能还有一条已经握手完成的连接：不许它再改本机剪切板。
        if self.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("CLIPBOARD_DISABLED", "剪切板同步已关闭"));
        }
        let signature = content.signature();
        if let Ok(mut slot) = self.last_remote.lock() {
            *slot = signature.clone();
        }
        if let Ok(mut slot) = self.last_local.lock() {
            *slot = signature;
        }
        if let Some(backend) = self.backend.as_ref() {
            if backend.clipboard_available() {
                backend.clipboard_write(&content)?;
                self.received_count.fetch_add(1, Ordering::Relaxed);
                self.set_event(
                    "received",
                    describe_content(&content),
                    peer_name.to_string(),
                );
                return Ok(());
            }
        }
        // 移动端：把图片写到缓存里交给 Kotlin 桥（避免 10MB 图片 base64 塞进事件）。
        let image_path = if let Some(image) = &content.image {
            let inbox = self.layout.runtime_dir().join("clipboard-inbox");
            tokio::fs::create_dir_all(&inbox)
                .await
                .map_err(|error| CoreError::io(format!("创建剪切板缓存目录失败：{error}")))?;
            let path = inbox.join(format!("{}.png", random_hex(8)));
            tokio::fs::write(&path, image)
                .await
                .map_err(|error| CoreError::io(format!("写入剪切板图片失败：{error}")))?;
            Some(path.to_string_lossy().to_string())
        } else {
            None
        };
        let files: Vec<Value> = content
            .files
            .iter()
            .map(|file| {
                json!({
                    "name": file.name,
                    "path": file.path.as_ref().map(|path| path.to_string_lossy().to_string()),
                })
            })
            .collect();
        self.emit(json!({
            "kind": "remote",
            "content": {
                "text": content.text,
                "html": content.html,
                "imagePath": image_path,
                "files": files,
            },
            "peer": peer_name,
        }));
        self.received_count.fetch_add(1, Ordering::Relaxed);
        self.set_event("received", describe_content(&content), peer_name.to_string());
        Ok(())
    }
}

fn describe_content(content: &ClipboardContent) -> String {
    let mut parts = Vec::new();
    if !content.text.is_empty() {
        parts.push("文本");
    }
    if !content.html.is_empty() {
        parts.push("HTML");
    }
    if content.image.is_some() {
        parts.push("图片");
    }
    if !content.files.is_empty() {
        parts.push("文件");
    }
    parts.join(" + ")
}

// ---------------------------------------------------------------------------
// P2P（iroh + pkarr）
// ---------------------------------------------------------------------------

fn room_secret(code: &str) -> SecretKey {
    let mut hasher = Sha256::new();
    hasher.update(b"kxtodo-clipboard/v1/room/");
    hasher.update(code.as_bytes());
    SecretKey::from_bytes(&hasher.finalize().into())
}

async fn bind_endpoint(secret: &SecretKey, relay: Option<&str>) -> CoreResult<Endpoint> {
    let mut builder = Endpoint::builder(presets::N0)
        .secret_key(secret.clone())
        .alpns(vec![ALPN.to_vec()]);
    match relay.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("default") => {}
        Some("disabled") => builder = builder.relay_mode(RelayMode::Disabled),
        Some(url) => builder = builder.relay_mode(RelayMode::custom([net::parse_relay_url(url)?])),
    }
    builder
        .bind()
        .await
        .map_err(|error| CoreError::io(format!("剪切板 P2P 端点启动失败：{error:?}")))
}

async fn p2p_loop(inner: Arc<Inner>) -> CoreResult<()> {
    if inner.cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let endpoint = bind_endpoint(&inner.secret, inner.net.relay.as_deref()).await?;
    if inner.cancel.load(Ordering::SeqCst) {
        endpoint.close().await;
        return Ok(());
    }
    if let Ok(mut slot) = inner.endpoint.lock() {
        *slot = Some(endpoint.clone());
    }
    let client = Arc::new(directory::build_client(&endpoint, &inner.net.directory_url)?);
    let room = room_secret(&inner.settings.code);
    let self_id = endpoint.id();
    directory::publish_name(&client, &inner.secret, &inner.name).await.ok();
    directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await?;
    inner.emit_status();
    let mut last_publish = Instant::now();
    let mut names: HashMap<EndpointId, (String, Instant)> = HashMap::new();
    let mut accept_future = Box::pin(endpoint.accept());
    let mut poll = tokio::time::interval(PEER_POLL);
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            incoming = &mut accept_future => {
                let Some(incoming) = incoming else { break };
                accept_future = Box::pin(endpoint.accept());
                let Ok(accepting) = incoming.accept() else { continue };
                let Ok(connection) = accepting.await else { continue };
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    let Ok((mut send, mut recv)) = connection.accept_bi().await else { return };
                    let _ = tokio::time::timeout(Duration::from_secs(300), async {
                        if let Err(error) = serve_stream(&task, &mut recv, &mut send).await {
                            let _ = write_json(&mut send, &json!({ "kind": "error", "message": error.message })).await;
                        }
                    }).await;
                    // 等发送方读走最后一个 ack 再释放 Connection：QUIC 的流在
                    // Connection drop 时会一起被重置，早一毫秒都可能变成 connection lost。
                    let _ = tokio::time::timeout(Duration::from_secs(2), connection.closed()).await;
                });
            }
            _ = poll.tick() => {
                if last_publish.elapsed() >= Duration::from_secs(REPUBLISH_SECS) {
                    let _ = directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await;
                    last_publish = Instant::now();
                }
                let entries = directory::fetch(&client, room.public()).await;
                let now = directory::now_unix();
                let mut peers = BTreeMap::new();
                for entry in entries {
                    if entry.id == self_id || now.saturating_sub(entry.published_at) > DEVICE_FRESH_SECS {
                        continue;
                    }
                    let id = entry.id.to_z32();
                    let name = match names.get(&entry.id) {
                        Some((name, at)) if at.elapsed() < Duration::from_secs(20) => name.clone(),
                        _ => {
                            let name = directory::fetch_name(&client, entry.id).await.unwrap_or_default();
                            names.insert(entry.id, (name.clone(), Instant::now()));
                            name
                        }
                    };
                    let addr = if entry.addrs.is_empty() {
                        EndpointAddr::new(entry.id)
                    } else {
                        EndpointAddr {
                            id: entry.id,
                            addrs: entry.addrs.into_iter().map(TransportAddr::Ip).collect(),
                        }
                    };
                    peers.insert(id, P2pPeer { name, addr, last_seen: Instant::now() });
                }
                if let Ok(mut slot) = inner.p2p_peers.lock() {
                    *slot = peers;
                }
                inner.emit_status();
            }
        }
    }
    directory::unpublish(&client, &room, self_id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// 直接 IP
// ---------------------------------------------------------------------------

async fn direct_loop(inner: Arc<Inner>) {
    let mut interval = tokio::time::interval(DIRECT_POLL);
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            _ = interval.tick() => {
                for addr in inner.direct_addrs.clone() {
                    if inner.cancel.load(Ordering::SeqCst) { break; }
                    if let Ok((device, name)) = ping_lan(&inner, addr).await {
                        if let Ok(mut peers) = inner.direct_peers.lock() {
                            peers.insert(addr, DirectPeer {
                                device_id: device,
                                name,
                                last_ok: Instant::now(),
                            });
                        }
                    }
                }
                inner.emit_status();
            }
        }
    }
}

async fn ping_lan(inner: &Arc<Inner>, addr: SocketAddr) -> CoreResult<(String, String)> {
    let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr))
        .await
        .map_err(|_| CoreError::execution("CLIPBOARD_TIMEOUT", "连接对方超时"))??;
    let (mut reader, mut writer) = stream.into_split();
    let session = client_handshake(inner, &mut reader, &mut writer).await?;
    let header = WireHeader {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        kind: "ping".to_string(),
        id: random_hex(8),
        origin: inner.device_id.clone(),
        origin_name: inner.name.clone(),
        at: 0,
        text_size: 0,
        html_size: 0,
        image_size: 0,
        files: Vec::new(),
        total_size: 0,
        sig: String::new(),
    };
    write_sealed_json(&mut writer, &session, &serde_json::to_value(&header)?).await?;
    let _ = writer.shutdown().await;
    let ack = read_sealed_json(&mut reader, &session, MAX_CTRL_FRAME).await?;
    if ack["ok"].as_bool() != Some(true) {
        return Err(CoreError::execution("CLIPBOARD_REJECTED", "对方未回应 ping"));
    }
    Ok((session.peer_id, session.peer_name))
}


#[cfg(test)]
mod tests;

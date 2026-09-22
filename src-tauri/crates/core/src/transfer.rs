//! 文件传输助手（v0.8.3 起步，v0.8.4 按 LocalSend 的形态重做交互）。
//!
//! 两台设备凭**同一句口令**互传文件与文件夹。复用同步养熟的 iroh 基建（QUIC 打洞 +
//! 可换 relay + pkarr 目录），但**与同步账户、系统账号完全无关**：rendezvous 的 pkarr
//! 记录签在「口令派生的密钥」名下，pkarr 的 zone 就是那把公钥——知道口令才算得出 zone，
//! 才找得到对方（croc 的口令房间模型），传输本体走 QUIC 自带的加密与认证通道。
//!
//! v0.8.4 的结构变化（需求 1）：**从「一次一发」改成「一个常驻在线会话」**——
//! - 输完口令就 `go_online`：用**稳定的设备密钥**（runtime/transfer-identity.json）
//!   起端点，发布房间条目 + 自己的名字记录（`_name` TXT，复用同步那份），
//!   每 2 秒轮询房间把「匹配到的设备」推给界面，收到拨入就地开一个子会话处理；
//! - 发送是「挑一台设备发」：`send(target)` 用同一个端点拨号（不用再自己起端点轮询）；
//! - **接收确认**：收到文件清单先推 `request` 事件（对方名字 + 清单 + 总大小），
//!   等界面的 `decide`（或「自动接收」开着直接收），超时当拒绝；
//! - **文本消息**：不落盘、不进保存位置，走同一个握手（mode=text），收完推 `text` 事件；
//! - **历史**：每次结束（成功/取消/失败）记一条 runtime/transfer-history.json，
//!   设备历史按 device-id 去重——下次打开就能看到「上次跟谁传过」。
//!
//! 进度与状态全程经 `kxtodo://transfer` 事件推给前端（每个文件一条进度条）。
//! 取消是协作式的：置标志 + 断开连接，正在跑的读写下一轮循环自己退出。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointId, RelayMode, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use crate::error::{CoreError, CoreResult};
use crate::repo::Layout;
use crate::sync::crypto::hmac_sha256;
use crate::sync::p2p::directory::now_unix;
use crate::sync::p2p::{directory, net};

/// 本工具的 ALPN：与同步的 `kxtodo-p2p/1` 必须不同，否则同步的 accept 循环会抢连接
pub const ALPN: &[u8] = b"kxtodo-transfer/1";
/// 口令下限（需求：不少于 8 位）。口令就是房间钥匙，太短等于把房间挂在广场上
pub const CODE_MIN_CHARS: usize = 8;
/// 单帧 JSON 上限：清单可能含上千个文件，但 8MB 足够且挡住恶意巨帧
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// 读写块大小：进度事件按块节流，太小会把前端刷爆
const CHUNK: usize = 256 * 1024;
/// 房间轮询间隔（界面上的「匹配到的设备」就靠它刷新）
const PEER_POLL: std::time::Duration = std::time::Duration::from_secs(2);
/// 发送方等接收方上线的总时长（接收方发布目录约一秒内可见）
const PEER_WAIT: std::time::Duration = std::time::Duration::from_secs(120);
/// 房间条目重发间隔：目录里超过 15 分钟视为过期，20 秒重发一次又稳又省
const REPUBLISH_SECS: u64 = 20;
/// 本地判定「这台设备还在线」的条目年龄上限（发布间隔的约 2 倍）
const DEVICE_FRESH_SECS: u64 = 45;
/// 设备名的缓存时长：只在这段窗口首次见到某台设备时查一次公网目录
const NAME_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(20);
/// 接收确认卡的等待时长（超时当拒绝，别把发送方永远挂在那儿）
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// 历史保留条数（需求：最近 50 条）
const HISTORY_LIMIT: usize = 50;
/// 设备历史保留条数
const DEVICE_LIMIT: usize = 32;

// ---------------------------------------------------------------------------
// 口令派生
// ---------------------------------------------------------------------------

fn sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

/// 口令 → rendezvous 签名密钥。pkarr 的 zone 就是它的公钥：
/// 同一句口令在任意设备派生出同一把，于是双方无需任何账户就能互相找到。
pub fn room_secret(code: &str) -> SecretKey {
    SecretKey::from_bytes(&sha256(&format!("kxtodo-transfer/v1/room/{code}")))
}

/// 连接两端的握手令牌：HMAC(口令派生密钥, 两个 EndpointId 排序拼接)。
/// 双方各自都能算出同一个值；算不出 = 不知道口令 = 拒之门外。
/// 密钥分支与 rendezvous 分开派生，目录记录本身不泄露令牌。
fn handshake_token(code: &str, a: EndpointId, b: EndpointId) -> String {
    let (left, right) = if a.to_z32() <= b.to_z32() { (a, b) } else { (b, a) };
    let message = format!("{}|{}", left.to_z32(), right.to_z32());
    let key = sha256(&format!("kxtodo-transfer/v1/token/{code}"));
    hex(&hmac_sha256(&key, message.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 口令校验：去首尾空白后不少于 [`CODE_MIN_CHARS`] 个字符。
pub fn validate_code(raw: &str) -> CoreResult<String> {
    let code = raw.trim().to_string();
    if code.chars().count() < CODE_MIN_CHARS {
        return Err(CoreError::validation(
            "TRANSFER_CODE_TOO_SHORT",
            format!("配对口令至少 {CODE_MIN_CHARS} 位：它是两台设备之间的唯一暗号"),
        ));
    }
    Ok(code)
}

// ---------------------------------------------------------------------------
// 会话模型
// ---------------------------------------------------------------------------

/// 一个待传文件。`rel` 是相对传输根的路径（`/` 分隔），接收方按它还原目录结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferItem {
    pub rel: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferRole {
    Send,
    Receive,
}

/// 传输的网络配置：relay 与目录地址都跟 p2p 同步同一套语义
/// （空 = n0 公共服务，`disabled` = 不用 relay，其它 = 自部署地址）。
#[derive(Debug, Clone, Default)]
pub struct TransferNet {
    pub relay: Option<String>,
    pub directory_url: String,
}

struct Session {
    id: String,
    role: TransferRole,
    cancel: AtomicBool,
    endpoint: Mutex<Option<Endpoint>>,
    /// 会话自己的连接（建立后放进来）。取消时关它、收尾时丢掉它——
    /// **不要拿 `endpoint` 去关在线会话的共享端点**：iroh 的 `Endpoint::close()`
    /// 对任何一个 clone 调用都会关掉整个端点（没有引用计数保活），
    /// 发完一次就把整台设备弄下线。
    connection: Mutex<Option<iroh::endpoint::Connection>>,
}

static SESSIONS: OnceLock<Mutex<BTreeMap<String, Arc<Session>>>> = OnceLock::new();

fn sessions() -> &'static Mutex<BTreeMap<String, Arc<Session>>> {
    SESSIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// 传输共用的 tokio 运行时：端点绑定与目录发布都是异步的，而命令跑在
/// GUI 的异步任务 / CLI 主线程里，借调用方 block_on 会撞「异步上下文里 block_on」的雷。
fn runtime() -> CoreResult<Arc<Runtime>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Runtime>>>> = OnceLock::new();
    let slot = SLOT.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| CoreError::internal("传输运行时锁中毒"))?;
    if let Some(existing) = guard.as_ref() {
        return Ok(existing.clone());
    }
    let built = RuntimeBuilder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| CoreError::internal(format!("传输运行时启动失败：{error}")))?;
    let shared = Arc::new(built);
    *guard = Some(shared.clone());
    Ok(shared)
}

/// 本进程还活着的会话数
pub fn active_count() -> usize {
    sessions().lock().map(|guard| guard.len()).unwrap_or(0)
}

/// 取消一个会话：置标志并断开本会话的连接，正在跑的读写下一轮自己退出。
/// 接收会话的 `endpoint` 本来就是 None，发送会话也不持端点（共享的在线端点由
/// 在线会话自己管）——真正能把它从阻塞读写里叫醒的是 `connection.close()`。
pub fn cancel(session_id: &str) -> CoreResult<Value> {
    let session = sessions()
        .lock()
        .ok()
        .and_then(|mut guard| guard.remove(session_id))
        .ok_or_else(|| {
            CoreError::not_found("TRANSFER_NOT_FOUND", format!("没有会话 {session_id}"))
        })?;
    session.cancel.store(true, Ordering::SeqCst);
    close_endpoint(&session);
    close_connection(&session);
    Ok(json!({ "id": session_id, "cancelled": true }))
}

fn close_connection(session: &Session) {
    let connection = session
        .connection
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
    if let Some(connection) = connection {
        connection.close(0u32.into(), b"cancelled");
    }
}

/// `Endpoint::close` 是异步的，而取消/收尾发生在同步上下文里：
/// 丢给传输运行时去关，关掉之前端点自己还能把在途的包收尾。
fn close_endpoint(session: &Session) {
    let endpoint = session.endpoint.lock().ok().and_then(|mut guard| guard.take());
    let Some(endpoint) = endpoint else { return };
    if let Ok(runtime) = runtime() {
        runtime.handle().spawn(async move {
            endpoint.close().await;
        });
    }
}

// ---------------------------------------------------------------------------
// 事件
// ---------------------------------------------------------------------------

/// 进度/状态事件的出口：壳层把它接到 `AppHandle::emit("kxtodo://transfer")`，
/// 测试接一个记录器。传输不写任何领域文件，所以不需要走 Domain Core 的命令层。
pub type TransferSink = Arc<dyn Fn(Value) + Send + Sync + 'static>;

/// 推一条会话事件。`kind`：waiting / connected / progress / fileDone /
/// done / error / cancelled / rejected。
/// 前端按 sessionId + index 维护进度条。
fn emit(sink: &TransferSink, session: &Session, mut payload: Value) {
    if let Some(map) = payload.as_object_mut() {
        map.insert("sessionId".to_string(), json!(session.id));
        map.insert("role".to_string(), json!(session.role));
    }
    trace_event(&payload);
    sink(payload);
}

/// 事件轨迹（v0.8.6 需求 5.4；v0.8.7 需求 5 改成后台异步写）。
/// 每行一条 JSON 落在 `<data>/runtime/transfer-events.log`。
/// 「同一会话出现两张卡」这类问题只有拿到 core 实际发过的事件序列才能定案。
///
/// **必须是异步的**：运行时 `worker_threads(2)`，同步写盘会卡住 accept / 心跳 /
/// 其它传输（同文件里文件读写走 `tokio::fs`/`spawn_blocking` 的同一条纪律）。
/// `Handle::try_current` 兜底：runtime 外（个别单测直呼 emit）静默跳过，**不能 panic**
/// （直接 `tokio::spawn` 在 runtime 外就是 panic）。
/// 只记**会话生命周期**事件（progress 每 256KB 一条，写进来既没信息量又白占 IO）。
fn trace_event(payload: &Value) {
    const MAX_TRACE_BYTES: u64 = 2 * 1024 * 1024;
    if payload["kind"] == json!("progress") {
        return;
    }
    let Some(dir) = trace_dir() else { return };
    let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
    let mut line = serde_json::to_string(payload).unwrap_or_default();
    line.push('\n');
    handle.spawn(async move {
        let path = dir.join("transfer-events.log");
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            if meta.len() > MAX_TRACE_BYTES {
                // 超限就砍掉前一半（保留最近的），从行边界切，别留半行
                if let Ok(content) = tokio::fs::read(&path).await {
                    let start = content.len() / 2;
                    let cut = content[start..]
                        .iter()
                        .position(|&byte| byte == b'\n')
                        .map(|offset| start + offset + 1)
                        .unwrap_or(start);
                    let _ = tokio::fs::write(&path, &content[cut..]).await;
                }
            }
        }
        if let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            use tokio::io::AsyncWriteExt;
            let _ = file.write_all(line.as_bytes()).await;
        }
    });
}

/// 事件轨迹的落点（在线会话建立时设，离线清掉）。诊断用途：多份 Layout 并存时
/// （测试）以最后一个上线的为准，不影响任何功能路径。
fn trace_dir() -> Option<PathBuf> {
    TRACE_DIR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

static TRACE_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

// ---------------------------------------------------------------------------
// 帧
// ---------------------------------------------------------------------------

/// 连接层面的读写失败（对端掉线 / 连接被关）。
/// 单列一个错误码，界面据此把「对方离线了」与本地错误（写盘失败、路径越界……）
/// 分开显示——早先界面把所有 connected 之后的失败都写成「对方离线了」，
/// 本地真实原因（如 `TRANSFER_PATH_UNSAFE`）被整段吞掉（v0.8.6 需求 5.4）。
fn connection_lost(what: &str, error: impl std::fmt::Display) -> CoreError {
    CoreError::new(
        crate::error::ErrorKind::Io,
        "TRANSFER_CONNECTION_LOST",
        format!("{what}：{error}"),
    )
}

async fn write_frame(send: &mut SendStream, value: &Value) -> CoreResult<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| CoreError::internal(format!("传输帧序列化失败：{error}")))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CoreError::validation("TRANSFER_FRAME_TOO_BIG", "传输清单过大"));
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| connection_lost("传输帧写入失败", error))?;
    send.write_all(&bytes)
        .await
        .map_err(|error| connection_lost("传输帧写入失败", error))?;
    Ok(())
}

async fn read_frame(recv: &mut RecvStream) -> CoreResult<Value> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|error| connection_lost("传输帧长度读取失败", error))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(CoreError::validation("TRANSFER_FRAME_TOO_BIG", "对端发来的帧过大"));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|error| connection_lost("传输帧读取失败", error))?;
    serde_json::from_slice(&buf)
        .map_err(|error| CoreError::internal(format!("传输帧解析失败：{error}")))
}

/// 安全地拼接收路径：拒绝 `..`、盘符与绝对路径，防止对端用清单写穿保存目录。
fn safe_join(root: &Path, rel: &str) -> CoreResult<PathBuf> {
    let clean = rel.replace('\\', "/");
    let mut out = root.to_path_buf();
    for part in clean.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(CoreError::validation(
                "TRANSFER_PATH_UNSAFE",
                format!("传输路径越界：{rel}"),
            ));
        }
        out.push(part);
    }
    Ok(out)
}

fn source_path(root: Option<&Path>, item: &TransferItem) -> CoreResult<PathBuf> {
    match root {
        Some(root) => safe_join(root, &item.rel),
        None => Ok(PathBuf::from(&item.rel)),
    }
}

// ---------------------------------------------------------------------------
// 端点
// ---------------------------------------------------------------------------

async fn bind(net_config: &TransferNet, secret: &SecretKey) -> CoreResult<Endpoint> {
    let mut builder = Endpoint::builder(presets::N0)
        .secret_key(secret.clone())
        .alpns(vec![ALPN.to_vec()]);
    match net_config
        .relay
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        // `default` = n0 公共 relay（Endpoint::builder 的 presets::N0 默认就是它，什么都不用改）；
        // 空值在壳层就已经解析成「复用同步的 relay」了，到不了这里
        None | Some("default") => {}
        Some("disabled") => builder = builder.relay_mode(RelayMode::Disabled),
        Some(url) => builder = builder.relay_mode(RelayMode::custom([net::parse_relay_url(url)?])),
    }
    builder
        .bind()
        .await
        .map_err(|error| CoreError::io(format!("传输端点启动失败：{error:?}")))
}

fn direct_addrs(endpoint: &Endpoint) -> Vec<std::net::SocketAddr> {
    endpoint
        .addr()
        .addrs
        .iter()
        .filter_map(|addr| match addr {
            iroh::TransportAddr::Ip(socket) => Some(*socket),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 设备身份与凭证（runtime/）
// ---------------------------------------------------------------------------

/// 传输专用的稳定设备密钥：设备历史按 device-id 去重、名字记录挂在自己的 zone 上，
/// 都要求 EndpointId 跨会话稳定（v0.8.4 之前每次会话现生成，历史里每次都像新设备）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IdentityFile {
    #[serde(default)]
    secret: String,
}

fn identity_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join("transfer-identity.json")
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
    let payload = json!({ "secret": encode_secret(&secret) }).to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = crate::repo::atomic_write(&path, &payload);
    Ok(secret)
}

fn encode_secret(secret: &SecretKey) -> String {
    hex(&secret.to_bytes())
}

fn decode_secret(raw: &str) -> Option<SecretKey> {
    let bytes = raw.trim();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for index in 0..32 {
        out[index] = u8::from_str_radix(&bytes[index * 2..index * 2 + 2], 16).ok()?;
    }
    SecretKey::from_bytes(&out).into()
}

/// 记住的口令（runtime/transfer-code.json）：需求 1.2「口令记住，下次启动自动恢复在线」。
/// 明文，与 `sync-credentials.json` 同一条先例——它本来就是两人约定的暗号，不是账号密码。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransferCodeFile {
    #[serde(default)]
    pub code: String,
}

fn code_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join("transfer-code.json")
}

pub fn load_code(layout: &Layout) -> Value {
    let path = code_path(layout);
    let code = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<TransferCodeFile>(&raw).ok())
        .map(|file| file.code)
        .unwrap_or_default();
    json!({ "code": code })
}

pub fn save_code(layout: &Layout, code: &str) -> CoreResult<Value> {
    let clean = code.trim().to_string();
    let path = code_path(layout);
    if clean.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(json!({ "code": "" }));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::repo::atomic_write(
        &path,
        &json!({ "code": clean }).to_string(),
    )?;
    Ok(json!({ "code": clean }))
}

// ---------------------------------------------------------------------------
// 历史与设备名录（runtime/transfer-history.json，不进同步）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryEntry {
    id: String,
    #[serde(rename = "at", default)]
    at: String,
    /// "send" | "receive"
    direction: String,
    #[serde(rename = "peerId", default)]
    peer_id: String,
    #[serde(rename = "peerName", default)]
    peer_name: String,
    /// "done" | "cancelled" | "failed" | "text"
    status: String,
    #[serde(default)]
    files: u64,
    #[serde(default)]
    bytes: u64,
    /// 前几个文件名（界面上一行摘要足够）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeviceRecord {
    #[serde(default)]
    name: String,
    #[serde(rename = "lastAt", default)]
    last_at: String,
    #[serde(default)]
    count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryFile {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
    #[serde(default)]
    devices: BTreeMap<String, DeviceRecord>,
}

fn history_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join("transfer-history.json")
}

fn read_history(layout: &Layout) -> HistoryFile {
    std::fs::read_to_string(history_path(layout))
        .ok()
        .and_then(|raw| serde_json::from_str::<HistoryFile>(&raw).ok())
        .unwrap_or_default()
}

fn write_history(layout: &Layout, file: &HistoryFile) {
    let path = history_path(layout);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(file) {
        let _ = crate::repo::atomic_write(&path, &text);
    }
}

/// 历史文件的读-改-写必须成对：两个会话几乎同时完成时，各自读到同一份、
/// 各写各的，后写的会把先写的那条吞掉。
static HISTORY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn history_lock() -> &'static Mutex<()> {
    HISTORY_LOCK.get_or_init(|| Mutex::new(()))
}

/// 记一条历史 + 更新设备名录（设备历史按 device-id 去重）。
fn record_history(layout: &Layout, entry: HistoryEntry) {
    let _guard = history_lock().lock();
    let mut file = read_history(layout);
    if !entry.peer_id.is_empty() {
        let record = file.devices.entry(entry.peer_id.clone()).or_default();
        if !entry.peer_name.is_empty() {
            record.name = entry.peer_name.clone();
        }
        record.last_at = entry.at.clone();
        record.count += 1;
    }
    file.entries.insert(0, entry);
    file.entries.truncate(HISTORY_LIMIT);
    if file.devices.len() > DEVICE_LIMIT {
        let mut pairs: Vec<(String, DeviceRecord)> = file.devices.clone().into_iter().collect();
        pairs.sort_by(|a, b| b.1.last_at.cmp(&a.1.last_at));
        pairs.truncate(DEVICE_LIMIT);
        file.devices = pairs.into_iter().collect();
    }
    write_history(layout, &file);
}

pub fn history(layout: &Layout) -> Value {
    let file = read_history(layout);
    let devices: Vec<Value> = file
        .devices
        .iter()
        .map(|(id, record)| {
            json!({ "id": id, "name": record.name, "lastAt": record.last_at, "count": record.count })
        })
        .collect();
    json!({ "entries": file.entries, "devices": devices })
}

pub fn clear_history(layout: &Layout) -> Value {
    let _guard = history_lock().lock();
    write_history(layout, &HistoryFile::default());
    json!({ "entries": [], "devices": [] })
}

// ---------------------------------------------------------------------------
// 在线会话（单例）：发布自己 + 轮询房间 + 接听拨入
// ---------------------------------------------------------------------------

static ONLINE: OnceLock<Mutex<BTreeMap<String, Arc<Online>>>> = OnceLock::new();

fn online_slots() -> &'static Mutex<BTreeMap<String, Arc<Online>>> {
    ONLINE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn slot_key(layout: &Layout) -> String {
    layout.root.to_string_lossy().to_string()
}

/// 常驻的在线会话：口令房间里的「我」。发送与接收都从它出发。
struct Online {
    id: String,
    layout: Layout,
    sink: TransferSink,
    secret: SecretKey,
    code: String,
    save_dir: PathBuf,
    /// 当前设备名（界面上改了就更新，下一轮轮询重发）
    name: Mutex<String>,
    /// **已经发布出去的名字**（v0.8.6 需求 5.2）。早先 `name` 一个槽既当「当前名」
    /// 又当「已发布名」，`current != published` 恒为 false——改名后 republish 是死代码，
    /// 对端永远看不到新名字。拆成两个槽才是真的「改了才重发」。
    published_name: Mutex<String>,
    auto_accept: AtomicBool,
    net: TransferNet,
    cancel: AtomicBool,
    endpoint: Mutex<Option<Endpoint>>,
    /// 最近一次轮询到的房间成员（已过滤过期）；z32 字符串给界面用
    devices: Mutex<Vec<Value>>,
    /// 可拨号的对端地址（z32 → EndpointAddr）。**不要拿 z32 去 parse**：
    /// iroh 的 `FromStr` 只认 RFC4648 base32 与 hex，z32 是另一套字母表。
    peers: Mutex<Vec<(String, iroh::EndpointAddr)>>,
    /// 待确认的接收请求（接收确认卡）：按 request_id 多槽共存——
    /// 单槽会被第二个拨入顶掉，第一张的等待循环干等到超时。
    pending: Mutex<std::collections::HashMap<String, PendingRequest>>,
}

struct PendingRequest {
    accept: std::sync::atomic::AtomicI8,
}

/// 在线状态快照（界面顶部那个绿点用它）。
pub fn status(layout: &Layout) -> Value {
    let guard = online_slots().lock().ok();
    let online = guard
        .as_ref()
        .and_then(|slots| slots.get(&slot_key(layout)))
        .cloned();
    match online {
        Some(online) => json!({
            "online": true,
            "deviceId": online.secret.public().to_z32(),
            "code": online.code,
            "devices": online.devices.lock().map(|list| list.clone()).unwrap_or_default(),
        }),
        None => json!({ "online": false, "deviceId": "", "code": "", "devices": [] }),
    }
}

pub fn devices(layout: &Layout) -> Value {
    let guard = online_slots().lock().ok();
    let list = guard
        .as_ref()
        .and_then(|slots| slots.get(&slot_key(layout)))
        .and_then(|online| online.devices.lock().ok().map(|list| list.clone()))
        .unwrap_or_default();
    json!({ "devices": list })
}

pub fn set_auto_accept(layout: &Layout, value: bool) -> Value {
    if let Ok(guard) = online_slots().lock() {
        if let Some(online) = guard.get(&slot_key(layout)) {
            online.auto_accept.store(value, Ordering::SeqCst);
        }
    }
    json!({ "autoAccept": value })
}

/// 设备名为空时的兜底名（v0.8.6 需求 5.2）：房间里至少能认出「这是哪台机器」。
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

/// 更新本机在房间里发布的名字（设置里改了设备名时调一次）。
pub fn set_name(layout: &Layout, name: &str) {
    if let Ok(guard) = online_slots().lock() {
        if let Some(online) = guard.get(&slot_key(layout)) {
            if let Ok(mut slot) = online.name.lock() {
                *slot = directory::sanitize_name(name);
            }
        }
    }
}

/// 接收确认卡：用户点了接收 / 拒绝。
pub fn decide(layout: &Layout, request_id: &str, accept: bool) -> CoreResult<Value> {
    let guard = online_slots()
        .lock()
        .map_err(|_| CoreError::internal("传输在线会话锁中毒"))?;
    let online = guard
        .get(&slot_key(layout))
        .cloned()
        .ok_or_else(|| CoreError::not_found("TRANSFER_OFFLINE", "当前不在线"))?;
    let pending = online
        .pending
        .lock()
        .map_err(|_| CoreError::internal("传输确认锁中毒"))?;
    let Some(request) = pending.get(request_id) else {
        return Err(CoreError::not_found(
            "TRANSFER_NO_REQUEST",
            "这条接收请求已经不在等待了",
        ));
    };
    request
        .accept
        .store(if accept { 1 } else { 2 }, Ordering::SeqCst);
    Ok(json!({ "requestId": request_id, "accept": accept }))
}

/// 上线：起端点 → 发布房间条目与名字 → 轮询房间 → 接听拨入。
/// 已经在线时先下线（换口令 / 换保存目录都走这条路）。
pub fn go_online(
    sink: TransferSink,
    layout: &Layout,
    code: &str,
    save_dir: &Path,
    name: &str,
    auto_accept: bool,
    net_config: TransferNet,
) -> CoreResult<Value> {
    let code = validate_code(code)?;
    if !save_dir.is_dir() {
        return Err(CoreError::validation(
            "TRANSFER_SAVE_DIR_MISSING",
            "保存位置不存在或不是目录",
        ));
    }
    let _ = go_offline(layout);
    let secret = identity_secret(layout)?;
    let device_id = secret.public().to_z32();
    // 名字为空就兜底（v0.8.6 需求 5.2）：`publish_name` 会跳过空名字，对端就只剩
    // 「未命名的设备」可显示。自动恢复上线时前端可能还没水合出设置里的名字，
    // 这条兜底保证房间里的每台设备始终有一个可读的名字。
    let resolved_name = {
        let clean = directory::sanitize_name(name);
        if clean.is_empty() {
            default_device_name()
        } else {
            clean
        }
    };
    let online = Arc::new(Online {
        id: new_id(),
        layout: layout.clone(),
        sink,
        secret,
        code: code.clone(),
        save_dir: save_dir.to_path_buf(),
        name: Mutex::new(resolved_name),
        published_name: Mutex::new(String::new()),
        auto_accept: AtomicBool::new(auto_accept),
        net: net_config,
        cancel: AtomicBool::new(false),
        endpoint: Mutex::new(None),
        devices: Mutex::new(Vec::new()),
        peers: Mutex::new(Vec::new()),
        pending: Mutex::new(std::collections::HashMap::new()),
    });
    if let Ok(mut slots) = online_slots().lock() {
        slots.insert(slot_key(layout), online.clone());
    }
    if let Some(mut slot) = TRACE_DIR.get_or_init(|| Mutex::new(None)).lock().ok() {
        *slot = Some(layout.root.join("runtime"));
    }
    let runtime = runtime()?;
    let spawned = online.clone();
    runtime.handle().spawn(async move {
        let result = online_loop(&spawned).await;
        if let Ok(mut slots) = online_slots().lock() {
            // 只清自己那一份：期间可能已经被换成了新会话
            let key = spawned.layout.root.to_string_lossy().to_string();
            if slots.get(&key).map(|item| item.id.as_str()) == Some(spawned.id.as_str()) {
                slots.remove(&key);
            }
        }
        if let Err(error) = result {
            if !spawned.cancel.load(Ordering::SeqCst) {
                (spawned.sink)(json!({
                    "kind": "error",
                    "sessionId": spawned.id,
                    "role": "online",
                    "code": error.code,
                    "message": error.message,
                }));
            }
        }
    });
    Ok(json!({ "id": online.id, "deviceId": device_id, "code": code }))
}

pub fn go_offline(layout: &Layout) -> CoreResult<Value> {
    let taken = online_slots()
        .lock()
        .map_err(|_| CoreError::internal("传输在线会话锁中毒"))?
        .remove(&slot_key(layout));
    let Some(online) = taken else {
        return Ok(json!({ "online": false }));
    };
    online.cancel.store(true, Ordering::SeqCst);
    close_endpoint(&online_endpoint_handle(&online));
    let _ = (online.sink)(json!({ "kind": "offline", "sessionId": online.id, "role": "online" }));
    Ok(json!({ "online": false }))
}

fn online_endpoint_handle(online: &Online) -> Session {
    // 关端点只有一个用途：把 accept() 唤醒。借一个临时 Session 形状复用 close_endpoint。
    Session {
        id: online.id.clone(),
        role: TransferRole::Receive,
        cancel: AtomicBool::new(true),
        endpoint: Mutex::new(online.endpoint.lock().ok().and_then(|mut guard| guard.take())),
        connection: Mutex::new(None),
    }
}

/// 在线主循环：发布 → 轮询设备 → 接听拨入（每个拨入开一个子会话）。
async fn online_loop(online: &Arc<Online>) -> CoreResult<()> {
    let endpoint = bind(&online.net, &online.secret).await?;
    *online.endpoint.lock().unwrap() = Some(endpoint.clone());
    let client = Arc::new(directory::build_client(
        &endpoint,
        &online.net.directory_url,
    )?);
    let room = room_secret(&online.code);
    let self_id = endpoint.id();
    let name = online.name.lock().map(|slot| slot.clone()).unwrap_or_default();
    directory::publish_name(&client, &online.secret, &name).await.ok();
    if let Ok(mut slot) = online.published_name.lock() {
        *slot = name;
    }
    directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await?;
    (online.sink)(json!({
        "kind": "online",
        "sessionId": online.id,
        "role": "online",
        "deviceId": self_id.to_z32(),
    }));

    let mut last_publish = std::time::Instant::now();
    let mut last_seen: Vec<Value> = Vec::new();
    let mut name_cache: std::collections::HashMap<EndpointId, (String, std::time::Instant)> =
        std::collections::HashMap::new();
    let mut poll = tokio::time::interval(PEER_POLL);
    // accept() **不能一次只等 1ms**：QUIC 握完到 accept 返回之间可能超过那个窗口，
    // 取消一次就丢掉一个拨入（症状：发送方报「对方离线」，接收方全程没反应）。
    // 用 select 让同一个 future 一直挂着，轮询到点才换分支。
    let mut accept_future = Box::pin(endpoint.accept());
    loop {
        if online.cancel.load(Ordering::SeqCst) {
            directory::unpublish(&client, &room, self_id).await;
            return Ok(());
        }
        tokio::select! {
            incoming = &mut accept_future => {
                let Some(incoming) = incoming else {
                    return Err(CoreError::execution("TRANSFER_CLOSED", "传输端点已关闭"));
                };
                accept_future = Box::pin(endpoint.accept());
                let accepting = incoming
                    .accept()
                    .map_err(|error| CoreError::io(format!("传输连接接受失败：{error:?}")))?;
                let connection = accepting
                    .await
                    .map_err(|error| CoreError::io(format!("传输连接握手失败：{error:?}")))?;
                let session = Arc::new(Session {
                    id: new_id(),
                    role: TransferRole::Receive,
                    cancel: AtomicBool::new(false),
                    endpoint: Mutex::new(None),
                    // 会话持有自己的连接：取消要能从阻塞读写里把它叫醒
                    connection: Mutex::new(Some(connection.clone())),
                });
                register(session.clone());
                let owned = online.clone();
                let shared_client = client.clone();
                tokio::spawn(async move {
                    let result =
                        receive_one(&owned, &session, connection, &shared_client, self_id).await;
                    finish(&owned.sink, &session, result);
                });
                continue;
            }
            _ = poll.tick() => {}
        }
        if online.cancel.load(Ordering::SeqCst) {
            continue;
        }
        // 名字改了立刻重发一次（当前名 vs **已发布名**两个槽，见 Online::published_name）
        let current = online.name.lock().map(|slot| slot.clone()).unwrap_or_default();
        let published = published_name(online);
        if current != published {
            directory::publish_name(&client, &online.secret, &current).await.ok();
            if let Ok(mut slot) = online.published_name.lock() {
                *slot = current;
            }
        }
        if last_publish.elapsed() >= std::time::Duration::from_secs(REPUBLISH_SECS) {
            directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await.ok();
            last_publish = std::time::Instant::now();
        }
        // 轮询房间：把「匹配到的设备」推给界面（排除自己）
        let entries = directory::fetch(&client, room.public()).await;
        let now = now_unix();
        let mut list: Vec<Value> = Vec::new();
        let mut dialable: Vec<(String, iroh::EndpointAddr)> = Vec::new();
        for entry in entries {
            if entry.id == self_id {
                continue;
            }
            if now.saturating_sub(entry.published_at) > DEVICE_FRESH_SECS {
                continue;
            }
            // 名字缓存：每 2 秒对每台设备各查一次公网目录纯属浪费。
            // 名字改了对方会重发自己的名字记录，这里最多滞后一个缓存的时长。
            let peer_name = match name_cache.get(&entry.id) {
                Some((name, at)) if at.elapsed() < NAME_CACHE_TTL => name.clone(),
                _ => {
                    if name_cache.len() > 32 {
                        name_cache.clear();
                    }
                    let name = directory::fetch_name(&client, entry.id)
                        .await
                        .unwrap_or_default();
                    name_cache.insert(entry.id, (name.clone(), std::time::Instant::now()));
                    name
                }
            };
            let z32 = entry.id.to_z32();
            let addr = if entry.addrs.is_empty() {
                iroh::EndpointAddr::new(entry.id)
            } else {
                iroh::EndpointAddr {
                    id: entry.id,
                    addrs: entry
                        .addrs
                        .into_iter()
                        .map(iroh::TransportAddr::Ip)
                        .collect(),
                }
            };
            dialable.push((z32.clone(), addr));
            list.push(json!({
                "id": z32,
                "name": peer_name,
                "self": false,
            }));
        }
        if let Ok(mut slot) = online.peers.lock() {
            *slot = dialable;
        }
        if !same_devices(&list, &last_seen) {
            last_seen = list.clone();
            let _ = online.devices.lock().map(|mut slot| *slot = list.clone());
            (online.sink)(json!({
                "kind": "devices",
                "sessionId": online.id,
                "role": "online",
                "devices": list,
            }));
        }
    }
}

/// 上一次**发布出去**的名字（与「当前名」分开存：两者相同就说明没有待重发的改动）。
fn published_name(online: &Arc<Online>) -> String {
    online
        .published_name
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default()
}

fn same_devices(a: &[Value], b: &[Value]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(left, right)| {
        left["id"] == right["id"] && left["name"] == right["name"]
    })
}

/// 处理一次拨入：验令牌 → 读 hello（文件清单或文本）→ 文件先问确认 → 逐文件落盘。
async fn receive_one(
    online: &Arc<Online>,
    session: &Arc<Session>,
    connection: iroh::endpoint::Connection,
    client: &Arc<iroh::address_lookup::pkarr::PkarrRelayClient>,
    self_id: EndpointId,
) -> CoreResult<Value> {
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|error| CoreError::io(format!("传输流打开失败：{error:?}")))?;
    let hello = read_frame(&mut recv).await?;
    let token = hello["token"].as_str().unwrap_or_default().to_string();
    let expected = handshake_token(&online.code, self_id, connection.remote_id());
    if token != expected {
        let _ = write_frame(&mut send, &json!({ "kind": "reject", "reason": "口令不匹配" })).await;
        return Err(CoreError::validation(
            "TRANSFER_CODE_MISMATCH",
            "对方算不出正确的握手令牌：两边口令不一致",
        ));
    }
    let peer_id = connection.remote_id().to_z32();
    let peer_name = hello["name"].as_str().unwrap_or_default().to_string();
    let peer_name = if peer_name.is_empty() {
        directory::fetch_name(client, connection.remote_id())
            .await
            .unwrap_or_default()
    } else {
        peer_name
    };

    // 文本消息（需求 1.3）：不落盘、不进保存位置，收完推一条 text 事件
    if hello["mode"].as_str() == Some("text") {
        let text = hello["text"].as_str().unwrap_or_default().to_string();
        write_frame(&mut send, &json!({ "kind": "accept" })).await?;
        let end = read_frame(&mut recv).await?;
        if end["kind"] == "end" {
            let _ = write_frame(&mut send, &json!({ "kind": "endAck" })).await;
            let _ = send.finish();
        }
        emit(
            &online.sink,
            session,
            json!({
                "kind": "text",
                "text": text,
                "peer": { "id": peer_id, "name": peer_name },
                "received": true,
            }),
        );
        record_history(
            &online.layout,
            HistoryEntry {
                id: session.id.clone(),
                at: crate::time::now_iso(),
                direction: "receive".to_string(),
                peer_id,
                peer_name,
                status: "text".to_string(),
                files: 0,
                bytes: text.len() as u64,
                names: Vec::new(),
            },
        );
        return Ok(json!({ "kind": "text", "files": 0, "bytes": 0 }));
    }

    let items: Vec<TransferItem> = serde_json::from_value(hello["files"].clone()).map_err(|error| {
        CoreError::validation("TRANSFER_MANIFEST_BAD", format!("清单不合法：{error}"))
    })?;
    let total_bytes: u64 = items.iter().map(|item| item.size).sum();
    // 接收确认（需求 1.3）：自动接收开着就直接放行，否则挂一张确认卡等界面回话
    emit(
        &online.sink,
        session,
        json!({
            "kind": "request",
            "peer": { "id": peer_id, "name": peer_name },
            "files": items.iter().map(|item| json!({ "rel": item.rel, "size": item.size })).collect::<Vec<_>>(),
            "totalBytes": total_bytes,
            "auto": online.auto_accept.load(Ordering::SeqCst),
        }),
    );
    let request_id = session.id.clone();
    if !online.auto_accept.load(Ordering::SeqCst) {
        if let Ok(mut slot) = online.pending.lock() {
            slot.insert(
                request_id.clone(),
                PendingRequest {
                    accept: std::sync::atomic::AtomicI8::new(0),
                },
            );
        }
        let deadline = std::time::Instant::now() + REQUEST_TIMEOUT;
        let accepted = loop {
            if session.cancel.load(Ordering::SeqCst) {
                break false;
            }
            let state = online
                .pending
                .lock()
                .ok()
                .and_then(|slot| {
                    slot.get(&request_id)
                        .map(|item| item.accept.load(Ordering::SeqCst))
                })
                .unwrap_or(0);
            if state == 1 {
                break true;
            }
            if state == 2 {
                break false;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        };
        if let Ok(mut slot) = online.pending.lock() {
            slot.remove(&request_id);
        }
        if !accepted {
            let _ = write_frame(
                &mut send,
                &json!({ "kind": "reject", "reason": "对方拒绝了这次传输" }),
            )
            .await;
            // 拒绝是**用户主动决定**，不是错误（v0.8.6 需求 5.3/5.4）：历史记 `rejected`
            // 而不是 `cancelled`（与超时、断线区分开），并且**不发 error 事件**——
            // 早先这里返回 Err，界面在「从没建过卡」的会话上凭空画出一张「接收失败」。
            record_history(
                &online.layout,
                HistoryEntry {
                    id: session.id.clone(),
                    at: crate::time::now_iso(),
                    direction: "receive".to_string(),
                    peer_id: peer_id.clone(),
                    peer_name: peer_name.clone(),
                    status: "rejected".to_string(),
                    files: items.len() as u64,
                    bytes: total_bytes,
                    names: first_names(&items),
                },
            );
            // 信息性事件：界面若有对应会话卡（正常没有）把它收成「已拒绝」态
            emit(
                &online.sink,
                session,
                json!({
                    "kind": "rejected",
                    "peer": peer_id,
                    "peerName": peer_name,
                    "files": items.len(),
                    "totalBytes": total_bytes,
                }),
            );
            // 等对方把 reject 读走再收尾：连接句柄一 drop 就会发 CONNECTION_CLOSE，
            // 抢在对方读到之前断线的话，发送方只能报「连接断了」而不知道是被拒。
            // 对方读到 reject 就会自己关连接（正常几毫秒），超时也照常收尾。
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), connection.closed()).await;
            return Ok(json!({
                "kind": "rejected",
                "files": 0,
                "bytes": 0,
                "peerName": peer_name,
            }));
        }
    }
    write_frame(&mut send, &json!({ "kind": "accept" })).await?;
    emit(
        &online.sink,
        session,
        json!({
            "kind": "connected",
            "peer": peer_id,
            "peerName": peer_name,
            "files": items.len(),
            "totalBytes": total_bytes,
        }),
    );

    let save_dir = online.save_dir.clone();
    let mut received_files = 0u64;
    let mut received_bytes = 0u64;
    let mut written_names: Vec<String> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if session.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
        }
        let header = read_frame(&mut recv).await?;
        if header["kind"] != "file" || header["index"].as_u64() != Some(index as u64) {
            return Err(CoreError::execution("TRANSFER_PROTOCOL", "文件帧顺序不对"));
        }
        let target = safe_join(&save_dir, &item.rel)?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // 同名冲突：默认自动重命名 `照片(1).jpg`（需求 1.7，不做覆盖）
        let (target, renamed) = unique_target(&target)?;
        // 写盘走 tokio::fs（内部 spawn_blocking）：运行时只有 2 个 worker 线程，
        // 同步写盘会把 accept/心跳/其它传输全部卡住。BufWriter 把网络的小块读
        // 合成整块再落盘，减少阻塞线程的往返次数。
        let file = tokio::fs::File::create(&target).await?;
        let mut file = tokio::io::BufWriter::with_capacity(CHUNK, file);
        let mut taken = recv.take(item.size);
        let mut buffer = vec![0u8; CHUNK];
        let mut written = 0u64;
        loop {
            let read = taken
                .read(&mut buffer)
                .await
                .map_err(|error| CoreError::io(format!("传输读取失败：{error}")))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .await
                .map_err(|error| CoreError::io(format!("保存写入失败：{error}")))?;
            written += read as u64;
            emit(
                &online.sink,
                session,
                json!({
                    "kind": "progress",
                    "index": index,
                    "file": item.rel,
                    "sent": written,
                    "total": item.size,
                }),
            );
        }
        file.flush()
            .await
            .map_err(|error| CoreError::io(format!("保存落盘失败：{error}")))?;
        recv = taken.into_inner();
        write_frame(&mut send, &json!({ "kind": "fileDone", "index": index })).await?;
        received_files += 1;
        received_bytes += written;
        written_names.push(target.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default());
        emit(
            &online.sink,
            session,
            json!({
                "kind": "fileDone",
                "index": index,
                "file": item.rel,
                "sent": written,
                "total": item.size,
                "renamed": renamed,
                "savedAs": target.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default(),
            }),
        );
    }
    let end = read_frame(&mut recv).await?;
    if end["kind"] != "end" {
        return Err(CoreError::execution("TRANSFER_PROTOCOL", "缺少结束帧"));
    }
    write_frame(&mut send, &json!({ "kind": "endAck" })).await?;
    let _ = send.finish();
    record_history(
        &online.layout,
        HistoryEntry {
            id: session.id.clone(),
            at: crate::time::now_iso(),
            direction: "receive".to_string(),
            peer_id,
            peer_name,
            status: "done".to_string(),
            files: received_files,
            bytes: received_bytes,
            names: written_names.iter().take(3).cloned().collect(),
        },
    );
    Ok(json!({
        "files": received_files,
        "bytes": received_bytes,
        "dir": save_dir.to_string_lossy(),
    }))
}

/// 同名时自动重命名：`照片.jpg` → `照片(1).jpg` → `照片(2).jpg`…
/// 1000 个候选全被占就**报错而不是覆盖**：默认语义是绝不覆盖用户的文件。
fn unique_target(target: &Path) -> CoreResult<(PathBuf, bool)> {
    if !target.exists() {
        return Ok((target.to_path_buf(), false));
    }
    let parent = target.parent().unwrap_or(Path::new("."));
    let stem = target
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = target
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_default();
    for index in 1..1000 {
        let candidate = parent.join(format!("{stem}({index}){ext}"));
        if !candidate.exists() {
            return Ok((candidate, true));
        }
    }
    Err(CoreError::validation(
        "TRANSFER_NAME_EXHAUSTED",
        format!(
            "同名文件太多（「{}」已经存在 1000 个），先清理保存位置再传",
            target
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| target.display().to_string())
        ),
    ))
}

/// 结束帧的确认：**每个文件都已经单独确认过**，这一条只是收尾礼节——
/// 对方可能写完 ack 就释放连接（发送方读到 connection lost 也无所谓），
/// 所以给它一个短超时，拿不到就算了，不影响「传输成功」的结论。
async fn wait_end_ack(recv: &mut RecvStream) -> CoreResult<()> {
    match tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(recv)).await {
        Ok(Ok(frame)) if frame["kind"] == "endAck" => Ok(()),
        Ok(Ok(_)) => Err(CoreError::execution("TRANSFER_PROTOCOL", "缺少结束确认")),
        Ok(Err(error)) => Err(error),
        Err(_) => Ok(()),
    }
}

fn first_names(items: &[TransferItem]) -> Vec<String> {
    items
        .iter()
        .take(3)
        .map(|item| {
            Path::new(&item.rel)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| item.rel.clone())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 发送（挑一台设备发）
// ---------------------------------------------------------------------------

/// 发送内容：文件清单（可选 root，文件夹走这条）或一段文本。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum TransferPayload {
    Files {
        #[serde(default)]
        root: Option<String>,
        items: Vec<TransferItem>,
    },
    Text {
        text: String,
    },
}

/// 向在线会话里选中的那台设备发起传输。`target` 是设备列表里的 id（z32）。
pub fn send(layout: &Layout, target: &str, payload: TransferPayload) -> CoreResult<String> {
    let mut items = match &payload {
        TransferPayload::Files { items, .. } => items.clone(),
        TransferPayload::Text { .. } => Vec::new(),
    };
    let root = match &payload {
        TransferPayload::Files { root, .. } => root.clone(),
        TransferPayload::Text { .. } => None,
    };
    if matches!(payload, TransferPayload::Files { .. }) && items.is_empty() {
        return Err(CoreError::validation("TRANSFER_NOTHING_TO_SEND", "没有要发送的文件"));
    }
    // 发送清单的 tripwire（v0.8.6 需求 5.1）：`rel` 一律是相对传输根的路径。
    // `root = None` 却拿到绝对路径，说明调用方根本没做归一——Windows 上接收端
    // 会在 `safe_join` 的 `:` 判定上整单被拒（TRANSFER_PATH_UNSAFE），Linux 上
    // 更糟：不报错，直接把绝对路径镜像成一棵目录树。在连接之前 fail-fast。
    if root.is_none() {
        if let Some(item) = items
            .iter()
            .find(|item| item.rel.starts_with('/') || item.rel.starts_with('\\') || item.rel.contains(':'))
        {
            return Err(CoreError::validation(
                "TRANSFER_MANIFEST_ABSOLUTE_PATH",
                format!("发送清单组装错误：rel 必须是相对路径，收到 {}", item.rel),
            ));
        }
    }
    if let TransferPayload::Text { text } = &payload {
        if text.trim().is_empty() {
            return Err(CoreError::validation("TRANSFER_EMPTY_TEXT", "文本是空的"));
        }
    }
    for item in &mut items {
        let source = source_path(root.as_deref().map(Path::new), item)?;
        // 大小以落盘元数据为准（清单里的 size 只是界面预显）：进度条的「总」不能靠对方报。
        // 「不存在」必须报 TRANSFER_FILE_MISSING 而不是笼统的 IO_ERROR——界面上
        // 「文件没了」与「读不出来」是两种完全不同的处置（前者让用户重选，后者要看权限）。
        let meta = match std::fs::metadata(&source) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(CoreError::validation(
                    "TRANSFER_FILE_MISSING",
                    format!("要发送的文件不存在：{}", source.display()),
                ));
            }
            Err(error) => {
                return Err(CoreError::io(format!(
                    "读取文件信息失败（{}）：{error}",
                    source.display()
                )));
            }
        };
        if !meta.is_file() {
            return Err(CoreError::validation(
                "TRANSFER_NOT_A_FILE",
                format!("要发送的不是普通文件：{}", source.display()),
            ));
        }
        item.size = meta.len();
    }
    // 清单校验完才看在线状态：错误语义更准（「没东西可发」与「没上线」是两回事）
    let online = online_for(layout)
        .ok_or_else(|| CoreError::not_found("TRANSFER_OFFLINE", "先输入口令上线再发送"))?;
    let endpoint = online.endpoint.lock().ok().and_then(|slot| slot.clone()).ok_or_else(|| {
        CoreError::not_found("TRANSFER_OFFLINE", "传输端点还没起来，稍后再试")
    })?;
    let session = Arc::new(Session {
        id: new_id(),
        role: TransferRole::Send,
        cancel: AtomicBool::new(false),
        // 发送会话**不持端点**：`endpoint.connect` 用的是在线会话的共享端点，
        // iroh 关掉任何一个 clone 就是关掉整个端点——发完一次会把整台设备弄下线。
        // 连接建立后存进 `connection`，取消靠关它。
        endpoint: Mutex::new(None),
        connection: Mutex::new(None),
    });
    register(session.clone());
    let session_id = session.id.clone();
    let runtime = runtime()?;
    let owned = online.clone();
    let name = online.name.lock().map(|slot| slot.clone()).unwrap_or_default();
    let client = directory::build_client(&endpoint, &online.net.directory_url)?;
    let target_id = target.to_string();
    let text: Option<String> = match &payload {
        TransferPayload::Text { text } => Some(text.clone()),
        TransferPayload::Files { .. } => None,
    };
    let target_addr = online
        .peers
        .lock()
        .ok()
        .and_then(|slot| slot.iter().find(|(id, _)| id == target).map(|(_, addr)| addr.clone()));
    let sink = online.sink.clone();
    runtime.handle().spawn(async move {
        let code = owned.code.clone();
        let result = send_loop(
            target_addr,
            &sink,
            &session,
            &code,
            &name,
            &endpoint,
            &client,
            root.as_deref().map(Path::new),
            items,
            text.as_deref(),
            &target_id,
        )
        .await;
        if let Ok(summary) = &result {
            record_history(
                &owned.layout,
                HistoryEntry {
                    id: session.id.clone(),
                    at: crate::time::now_iso(),
                    direction: "send".to_string(),
                    peer_id: target_id.clone(),
                    peer_name: summary["peerName"].as_str().unwrap_or_default().to_string(),
                    status: "done".to_string(),
                    files: summary["files"].as_u64().unwrap_or(0),
                    bytes: summary["bytes"].as_u64().unwrap_or(0),
                    names: summary["names"]
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        } else if let Err(error) = &result {
            let cancelled = session.cancel.load(Ordering::SeqCst);
            record_history(
                &owned.layout,
                HistoryEntry {
                    id: session.id.clone(),
                    at: crate::time::now_iso(),
                    direction: "send".to_string(),
                    peer_id: target_id.clone(),
                    peer_name: String::new(),
                    // 「对方拒绝」与「传输失败」是两回事（v0.8.6 需求 5.3）：
                    // 前者用户没做错什么，历史里要分开看
                    status: if cancelled {
                        "cancelled"
                    } else if error.code == "TRANSFER_REJECTED" {
                        "rejected"
                    } else {
                        "failed"
                    }
                    .to_string(),
                    files: 0,
                    bytes: 0,
                    names: Vec::new(),
                },
            );
        }
        finish(&sink, &session, result);
    });
    Ok(session_id)
}

fn online_for(layout: &Layout) -> Option<Arc<Online>> {
    online_slots()
        .lock()
        .ok()
        .and_then(|slots| slots.get(&slot_key(layout)).cloned())
}

#[allow(clippy::too_many_arguments)]
async fn send_loop(
    target_addr: Option<iroh::EndpointAddr>,
    sink: &TransferSink,
    session: &Arc<Session>,
    code: &str,
    name: &str,
    endpoint: &Endpoint,
    client: &iroh::address_lookup::pkarr::PkarrRelayClient,
    root: Option<&Path>,
    items: Vec<TransferItem>,
    text: Option<&str>,
    target: &str,
) -> CoreResult<Value> {
    let self_id = endpoint.id();
    let room = room_secret(code);
    emit(sink, session, json!({ "kind": "waiting" }));
    // 目标地址优先用在线会话轮询到的直连地址（带得动打洞）；没有就自己再等一轮，
    // 最后退回「只拿 id 去连」（走 relay）。
    let mut addr = target_addr;
    if addr.is_none() {
        // `target` 是界面传来的 z32 字符串，**不能拿去 parse**（iroh 的 FromStr 只认
        // RFC4648 base32 与 hex，z32 是另一套字母表）——在房间条目里按 to_z32 比对。
        // fresh 过滤与设备列表同一个口径：列表里看不见的设备，发送也如实说「不在线」。
        let deadline = std::time::Instant::now() + PEER_WAIT;
        loop {
            if session.cancel.load(Ordering::SeqCst) {
                return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
            }
            let entries = directory::fetch(client, room.public()).await;
            let now = now_unix();
            let found = entries.into_iter().find(|entry| {
                entry.id.to_z32() == target
                    && now.saturating_sub(entry.published_at) <= DEVICE_FRESH_SECS
            });
            if let Some(entry) = found {
                addr = Some(if entry.addrs.is_empty() {
                    iroh::EndpointAddr::new(entry.id)
                } else {
                    iroh::EndpointAddr {
                        id: entry.id,
                        addrs: entry
                            .addrs
                            .into_iter()
                            .map(iroh::TransportAddr::Ip)
                            .collect(),
                    }
                });
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(CoreError::execution(
                    "TRANSFER_PEER_OFFLINE",
                    "对方不在线了：确认对方还开着这个工具",
                ));
            }
            tokio::time::sleep(PEER_POLL).await;
        }
    }
    let target_addr = addr.expect("上面两条路都保证有地址");
    let connection = endpoint
        .connect(target_addr, ALPN)
        .await
        .map_err(|error| CoreError::io(format!("传输拨号失败：{error:?}")))?;
    if let Ok(mut slot) = session.connection.lock() {
        *slot = Some(connection.clone());
    }
    if session.cancel.load(Ordering::SeqCst) {
        let _ = connection.close(0u32.into(), b"cancelled");
        return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
    }
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| CoreError::io(format!("传输流打开失败：{error:?}")))?;
    let token = handshake_token(code, self_id, connection.remote_id());
    let peer_name = directory::fetch_name(client, connection.remote_id())
        .await
        .unwrap_or_default();
    let hello = match text {
        Some(text) => json!({
            "kind": "hello",
            "token": token,
            "name": name,
            "mode": "text",
            "text": text,
        }),
        None => json!({
            "kind": "hello",
            "token": token,
            "name": name,
            "mode": "files",
            "files": items,
        }),
    };
    write_frame(&mut send, &hello).await?;
    let answer = read_frame(&mut recv).await?;
    if answer["kind"] != "accept" {
        let reason = answer["reason"].as_str().unwrap_or("对方拒绝了这次传输");
        return Err(CoreError::execution("TRANSFER_REJECTED", reason.to_string()));
    }
    if text.is_some() {
        write_frame(&mut send, &json!({ "kind": "end" })).await?;
        let _ = wait_end_ack(&mut recv).await;
        let _ = send.finish();
        emit(
            sink,
            session,
            json!({ "kind": "text", "text": text, "peer": { "id": target, "name": peer_name }, "received": false }),
        );
        return Ok(json!({ "files": 0, "bytes": 0, "peerName": peer_name }));
    }
    emit(
        sink,
        session,
        json!({
            "kind": "connected",
            "peer": connection.remote_id().to_z32(),
            "peerName": peer_name,
            "files": items.len(),
            "totalBytes": items.iter().map(|item| item.size).sum::<u64>(),
        }),
    );

    for (index, item) in items.iter().enumerate() {
        if session.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
        }
        let source = source_path(root, item)?;
        write_frame(&mut send, &json!({ "kind": "file", "index": index })).await?;
        // 读源文件同理走 tokio::fs：2 个 worker 线程不能被磁盘读卡住
        let mut file = tokio::fs::File::open(&source).await?;
        let mut buffer = vec![0u8; CHUNK];
        let mut sent = 0u64;
        loop {
            let read = file
                .read(&mut buffer)
                .await
                .map_err(|error| CoreError::io(format!("发送读取失败：{error}")))?;
            if read == 0 {
                break;
            }
            send.write_all(&buffer[..read])
                .await
                .map_err(|error| CoreError::io(format!("发送写入失败：{error}")))?;
            sent += read as u64;
            emit(
                sink,
                session,
                json!({
                    "kind": "progress",
                    "index": index,
                    "file": item.rel,
                    "sent": sent,
                    "total": item.size,
                }),
            );
        }
        // 等对方落盘确认再发下一个
        let ack = read_frame(&mut recv).await?;
        if ack["kind"] != "fileDone" || ack["index"].as_u64() != Some(index as u64) {
            return Err(CoreError::execution("TRANSFER_PROTOCOL", "对端确认帧不对"));
        }
        emit(
            sink,
            session,
            json!({
                "kind": "fileDone",
                "index": index,
                "file": item.rel,
                "sent": sent,
                "total": item.size,
            }),
        );
    }
    write_frame(&mut send, &json!({ "kind": "end" })).await?;
    let _ = wait_end_ack(&mut recv).await;
    let _ = send.finish();
    Ok(json!({
        "files": items.len(),
        "bytes": items.iter().map(|item| item.size).sum::<u64>(),
        "peerName": peer_name,
        "names": first_names(&items),
    }))
}


// 收尾
// ---------------------------------------------------------------------------

fn register(session: Arc<Session>) {
    if let Ok(mut guard) = sessions().lock() {
        guard.insert(session.id.clone(), session);
    }
}

fn finish(sink: &TransferSink, session: &Arc<Session>, result: CoreResult<Value>) {
    if let Ok(mut guard) = sessions().lock() {
        guard.remove(&session.id);
    }
    close_endpoint(session);
    // 本会话的连接句柄：收尾丢掉即可（正常结束时两端都已 ack 完）。
    // **不要在这里 close 在线会话的端点**——发送会话根本不持端点。
    if let Ok(mut slot) = session.connection.lock() {
        let _ = slot.take();
    }
    match result {
        Ok(mut summary) => {
            // 汇总里显式带了 kind（如「接收方拒绝」这种主动收尾）就不覆盖：
            // 一律当成 done 会让界面把拒绝画成「接收完成」
            if summary.get("kind").is_none() {
                if let Some(map) = summary.as_object_mut() {
                    map.insert("kind".to_string(), json!("done"));
                }
            }
            emit(sink, session, summary);
        }
        Err(error) => {
            let cancelled = session.cancel.load(Ordering::SeqCst);
            emit(
                sink,
                session,
                json!({
                    "kind": if cancelled { "cancelled" } else { "error" },
                    "code": error.code,
                    "message": error.message
                }),
            );
        }
    }
}

fn new_id() -> String {
    crate::ids::gen_id("transfer")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同名自动重命名（需求 1.7：绝不覆盖）；1000 个候选全被占时报错而不是回退到覆盖。
    #[test]
    fn unique_target_renames_and_refuses_to_overwrite_when_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("照片.jpg");
        std::fs::write(&base, b"x").unwrap();
        let (next, renamed) = unique_target(&base).unwrap();
        assert!(renamed);
        assert_eq!(next.file_name().unwrap().to_string_lossy(), "照片(1).jpg");

        // 占满 1000 个候选：必须报 TRANSFER_NAME_EXHAUSTED，绝不能返回原路径（= 覆盖）
        std::fs::write(next, b"x").unwrap();
        for index in 2..1000 {
            std::fs::write(dir.path().join(format!("照片({index}).jpg")), b"x").unwrap();
        }
        let error = unique_target(&base).expect_err("候选用尽应当报错");
        assert_eq!(error.code, "TRANSFER_NAME_EXHAUSTED");
    }

    /// 发送清单的 tripwire（v0.8.6 需求 5.1）：`root = None` + 绝对路径的 `rel`
    /// 必须在连接之前 fail-fast，而不是让接收端去拒（Windows）或镜像目录树（Linux）。
    #[test]
    fn send_rejects_absolute_rel_without_root() {
        let layout = Layout::new(std::env::temp_dir());
        let cases = [
            (r"C:\tmp\照片.jpg", "Windows 盘符"),
            (r"\\server\share\a.txt", "UNC 前缀"),
            ("/home/user/a.txt", "POSIX 绝对路径"),
        ];
        for (rel, label) in cases {
            let payload = TransferPayload::Files {
                root: None,
                items: vec![TransferItem { rel: rel.to_string(), size: 1 }],
            };
            let error = send(&layout, "target-device", payload)
                .expect_err(&format!("{label} 应当被 tripwire 拦下"));
            assert_eq!(error.code, "TRANSFER_MANIFEST_ABSOLUTE_PATH", "{label}");
        }
        // 纯文件名的相对 rel 不走 tripwire（会往下走到「没上线」这一步）
        let payload = TransferPayload::Files {
            root: None,
            items: vec![TransferItem { rel: "照片.jpg".to_string(), size: 1 }],
        };
        let error = send(&layout, "target-device", payload).expect_err("离线时不应当发送成功");
        assert_eq!(error.code, "TRANSFER_FILE_MISSING");
    }

    /// 带 root 的清单不受 tripwire 影响：`root` 已经给了基准目录，rel 里的 `:`
    /// 会在 `safe_join` 那层被当作越界段拒绝（那条纵深防御保持不动）。
    #[test]
    fn safe_join_still_rejects_drive_segments() {
        let root = Path::new("/tmp/save");
        let error = safe_join(root, r"C:\evil.txt").expect_err("盘符段必须被拒");
        assert_eq!(error.code, "TRANSFER_PATH_UNSAFE");
        assert!(safe_join(root, "sub/照片.jpg").is_ok());
    }
}

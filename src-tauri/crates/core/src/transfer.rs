//! 文件传输助手（v0.8.3）：两台设备凭**同一句口令**互传文件与文件夹。
//!
//! 复用同步已经养熟的 iroh 基建（QUIC 打洞 + 可换 relay + pkarr 目录），但**与同步账户、
//! 系统账号完全无关**：rendezvous 的 pkarr 记录签在「口令派生的密钥」名下，pkarr 的 zone
//! 就是那把公钥——知道口令才算得出 zone，才找得到对方。这与 croc 的口令房间是同一个
//! 模型（口令即房间钥匙），传输本体走 QUIC 自带的加密与认证通道。
//!
//! 角色与流程（LocalSend 式的发送/接收两栏，croc 式的口令配对）：
//! - **接收方**：起端点 → 把自己的 EndpointId 与直连地址发布到口令 zone → 等拨入；
//!   连上后先验握手令牌，再收清单，然后逐文件落盘（文件夹 = 多条相对路径，各自一条进度）。
//! - **发送方**：起端点 → 轮询口令 zone 直到看见对方 → 拨号 → 发清单 → 逐文件推字节，
//!   每个文件等对方落盘确认再发下一个（进度条才有「这一条真的到了」的语义）。
//!
//! 进度与状态全程经 `kxtodo://transfer` 事件推给前端（每个文件一条进度条）。
//! 取消是协作式的：置标志 + 关端点，正在跑的读写下一轮循环自己退出。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointId, RelayMode, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use crate::error::{CoreError, CoreResult};
use crate::sync::crypto::hmac_sha256;
use crate::sync::p2p::{directory, net};

/// 本工具的 ALPN：与同步的 `kxtodo-p2p/1` 必须不同，否则同步的 accept 循环会抢连接
pub const ALPN: &[u8] = b"kxtodo-transfer/1";
/// 口令下限（需求：不少于 8 位）。口令就是房间钥匙，太短等于把房间挂在广场上
pub const CODE_MIN_CHARS: usize = 8;
/// 单帧 JSON 上限：清单可能含上千个文件，但 8MB 足够且挡住恶意巨帧
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// 读写块大小：进度事件按块节流，太小会把前端刷爆
const CHUNK: usize = 256 * 1024;
/// 发送方等接收方上线的轮询间隔与总时长（接收方发布目录约一秒内可见）
const PEER_POLL: std::time::Duration = std::time::Duration::from_secs(2);
const PEER_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

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
    code: String,
    cancel: AtomicBool,
    endpoint: Mutex<Option<Endpoint>>,
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

/// 取消一个会话：置标志并关端点，正在跑的读写下一轮循环自己退出。
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
    Ok(json!({ "id": session_id, "cancelled": true }))
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
/// done / error / cancelled。前端按 sessionId + index 维护进度条。
fn emit(sink: &TransferSink, session: &Session, mut payload: Value) {
    if let Some(map) = payload.as_object_mut() {
        map.insert("sessionId".to_string(), json!(session.id));
        map.insert("role".to_string(), json!(session.role));
    }
    sink(payload);
}

// ---------------------------------------------------------------------------
// 帧
// ---------------------------------------------------------------------------

async fn write_frame(send: &mut SendStream, value: &Value) -> CoreResult<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| CoreError::internal(format!("传输帧序列化失败：{error}")))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CoreError::validation("TRANSFER_FRAME_TOO_BIG", "传输清单过大"));
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| CoreError::io(format!("传输帧写入失败：{error}")))?;
    send.write_all(&bytes)
        .await
        .map_err(|error| CoreError::io(format!("传输帧写入失败：{error}")))?;
    Ok(())
}

async fn read_frame(recv: &mut RecvStream) -> CoreResult<Value> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|error| CoreError::io(format!("传输帧长度读取失败：{error}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(CoreError::validation("TRANSFER_FRAME_TOO_BIG", "对端发来的帧过大"));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|error| CoreError::io(format!("传输帧读取失败：{error}")))?;
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

async fn bind(net_config: &TransferNet) -> CoreResult<Endpoint> {
    let mut builder = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .alpns(vec![ALPN.to_vec()]);
    match net_config
        .relay
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => {}
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
// 接收
// ---------------------------------------------------------------------------

/// 开始接收：发布自己到口令 zone，等发送方拨入，逐文件写进 `save_dir`。
/// 命令立刻返回 sessionId，实际传输在后台跑、进度走事件。
pub fn receive(
    sink: TransferSink,
    code: &str,
    save_dir: &Path,
    net_config: TransferNet,
) -> CoreResult<String> {
    let code = validate_code(code)?;
    if !save_dir.is_dir() {
        return Err(CoreError::validation(
            "TRANSFER_SAVE_DIR_MISSING",
            "保存位置不存在或不是目录",
        ));
    }
    let runtime = runtime()?;
    let session = Arc::new(Session {
        id: new_id(),
        role: TransferRole::Receive,
        code,
        cancel: AtomicBool::new(false),
        endpoint: Mutex::new(None),
    });
    register(session.clone());
    let session_id = session.id.clone();
    let save_dir = save_dir.to_path_buf();
    runtime.handle().spawn(async move {
        let result = receive_loop(&sink, &session, &save_dir, &net_config).await;
        finish(&sink, &session, result);
    });
    Ok(session_id)
}

async fn receive_loop(
    sink: &TransferSink,
    session: &Arc<Session>,
    save_dir: &Path,
    net_config: &TransferNet,
) -> CoreResult<Value> {
    let endpoint = bind(net_config).await?;
    *session.endpoint.lock().unwrap() = Some(endpoint.clone());
    let client = directory::build_client(&endpoint, &net_config.directory_url)?;
    let room = room_secret(&session.code);
    let self_id = endpoint.id();

    emit(sink, session, json!({ "kind": "waiting" }));
    // 发布自己：发送方轮询这个 zone 就能拨进来。首轮失败通常意味着目录服务不可达，
    // 直接报错比让用户干等两分钟好。
    directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await?;

    let mut received_files = 0u64;
    let mut received_bytes = 0u64;
    loop {
        if session.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
        }
        // 等拨入：accept() 没有超时，取消靠关端点把它唤醒
        let Some(incoming) = endpoint.accept().await else {
            return Err(CoreError::execution("TRANSFER_CLOSED", "传输端点已关闭"));
        };
        let accepting = incoming
            .accept()
            .map_err(|error| CoreError::io(format!("传输连接接受失败：{error:?}")))?;
        let connection = accepting
            .await
            .map_err(|error| CoreError::io(format!("传输连接握手失败：{error:?}")))?;
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(|error| CoreError::io(format!("传输流打开失败：{error:?}")))?;

        let hello = read_frame(&mut recv).await?;
        let token = hello["token"].as_str().unwrap_or_default().to_string();
        let expected = handshake_token(&session.code, self_id, connection.remote_id());
        if token != expected {
            let _ = write_frame(&mut send, &json!({ "kind": "reject", "reason": "口令不匹配" }))
                .await;
            return Err(CoreError::validation(
                "TRANSFER_CODE_MISMATCH",
                "对方算不出正确的握手令牌：两边口令不一致",
            ));
        }
        let items: Vec<TransferItem> =
            serde_json::from_value(hello["files"].clone()).map_err(|error| {
                CoreError::validation("TRANSFER_MANIFEST_BAD", format!("清单不合法：{error}"))
            })?;
        write_frame(&mut send, &json!({ "kind": "accept" })).await?;
        emit(
            sink,
            session,
            json!({
                "kind": "connected",
                "peer": connection.remote_id().to_z32(),
                "files": items.len(),
                "totalBytes": items.iter().map(|item| item.size).sum::<u64>()
            }),
        );

        for (index, item) in items.iter().enumerate() {
            if session.cancel.load(Ordering::SeqCst) {
                return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
            }
            let header = read_frame(&mut recv).await?;
            if header["kind"] != "file" || header["index"].as_u64() != Some(index as u64) {
                return Err(CoreError::execution("TRANSFER_PROTOCOL", "文件帧顺序不对"));
            }
            let target = safe_join(save_dir, &item.rel)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::io::BufWriter::new(std::fs::File::create(&target)?);
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
                    .map_err(|error| CoreError::io(format!("保存写入失败：{error}")))?;
                written += read as u64;
                emit(
                    sink,
                    session,
                    json!({
                        "kind": "progress",
                        "index": index,
                        "file": item.rel,
                        "sent": written,
                        "total": item.size
                    }),
                );
            }
            file.flush()
                .map_err(|error| CoreError::io(format!("保存落盘失败：{error}")))?;
            recv = taken.into_inner();
            write_frame(&mut send, &json!({ "kind": "fileDone", "index": index })).await?;
            received_files += 1;
            received_bytes += written;
            emit(
                sink,
                session,
                json!({
                    "kind": "fileDone",
                    "index": index,
                    "file": item.rel,
                    "sent": written,
                    "total": item.size
                }),
            );
        }
        let end = read_frame(&mut recv).await?;
        if end["kind"] != "end" {
            return Err(CoreError::execution("TRANSFER_PROTOCOL", "缺少结束帧"));
        }
        write_frame(&mut send, &json!({ "kind": "endAck" })).await?;
        directory::unpublish(&client, &room, self_id).await;
        return Ok(json!({ "files": received_files, "bytes": received_bytes }));
    }
}

// ---------------------------------------------------------------------------
// 发送
// ---------------------------------------------------------------------------

/// 开始发送：`root` 为 None 时 `items[].rel` 就是本机绝对路径（多选文件走这条）；
/// 为 Some 时是相对 root 的路径（文件夹传输走这条，接收方按它还原目录结构）。
pub fn send(
    sink: TransferSink,
    code: &str,
    root: Option<&Path>,
    items: Vec<TransferItem>,
    net_config: TransferNet,
) -> CoreResult<String> {
    let code = validate_code(code)?;
    if items.is_empty() {
        return Err(CoreError::validation("TRANSFER_NOTHING_TO_SEND", "没有要发送的文件"));
    }
    let mut items = items;
    for item in &mut items {
        let source = source_path(root, item)?;
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
    let runtime = runtime()?;
    let session = Arc::new(Session {
        id: new_id(),
        role: TransferRole::Send,
        code,
        cancel: AtomicBool::new(false),
        endpoint: Mutex::new(None),
    });
    register(session.clone());
    let session_id = session.id.clone();
    let root = root.map(PathBuf::from);
    runtime.handle().spawn(async move {
        let result = send_loop(&sink, &session, root.as_deref(), items, &net_config).await;
        finish(&sink, &session, result);
    });
    Ok(session_id)
}

async fn send_loop(
    sink: &TransferSink,
    session: &Arc<Session>,
    root: Option<&Path>,
    items: Vec<TransferItem>,
    net_config: &TransferNet,
) -> CoreResult<Value> {
    let endpoint = bind(net_config).await?;
    *session.endpoint.lock().unwrap() = Some(endpoint.clone());
    let client = directory::build_client(&endpoint, &net_config.directory_url)?;
    let room = room_secret(&session.code);
    let self_id = endpoint.id();

    emit(sink, session, json!({ "kind": "waiting" }));
    // 等接收方上线：轮询口令 zone。接收方可能刚点「开始接收」还没发布完。
    let deadline = std::time::Instant::now() + PEER_WAIT;
    let peer = loop {
        if session.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
        }
        let entries = directory::fetch(&client, room.public()).await;
        if let Some(entry) = entries.into_iter().find(|entry| entry.id != self_id) {
            break entry;
        }
        if std::time::Instant::now() >= deadline {
            return Err(CoreError::execution(
                "TRANSFER_PEER_NOT_FOUND",
                "两分钟内没等到对方上线：确认两边输的是同一句口令、且对方已点开始接收",
            ));
        }
        tokio::time::sleep(PEER_POLL).await;
    };

    let target = if peer.addrs.is_empty() {
        iroh::EndpointAddr::new(peer.id)
    } else {
        iroh::EndpointAddr {
            id: peer.id,
            addrs: peer
                .addrs
                .into_iter()
                .map(iroh::TransportAddr::Ip)
                .collect(),
        }
    };
    let connection = endpoint
        .connect(target, ALPN)
        .await
        .map_err(|error| CoreError::io(format!("传输拨号失败：{error:?}")))?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| CoreError::io(format!("传输流打开失败：{error:?}")))?;

    let token = handshake_token(&session.code, self_id, connection.remote_id());
    write_frame(
        &mut send,
        &json!({ "kind": "hello", "token": token, "files": items }),
    )
    .await?;
    let answer = read_frame(&mut recv).await?;
    if answer["kind"] != "accept" {
        let reason = answer["reason"].as_str().unwrap_or("对方拒绝了这次传输");
        return Err(CoreError::execution("TRANSFER_REJECTED", reason.to_string()));
    }
    emit(
        sink,
        session,
        json!({
            "kind": "connected",
            "peer": connection.remote_id().to_z32(),
            "files": items.len(),
            "totalBytes": items.iter().map(|item| item.size).sum::<u64>()
        }),
    );

    for (index, item) in items.iter().enumerate() {
        if session.cancel.load(Ordering::SeqCst) {
            return Err(CoreError::execution("TRANSFER_CANCELLED", "已取消"));
        }
        let source = source_path(root, item)?;
        write_frame(&mut send, &json!({ "kind": "file", "index": index })).await?;
        let mut file = std::io::BufReader::new(std::fs::File::open(&source)?);
        let mut buffer = vec![0u8; CHUNK];
        let mut sent = 0u64;
        loop {
            let read = file
                .read(&mut buffer)
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
                    "total": item.size
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
                "total": item.size
            }),
        );
    }
    write_frame(&mut send, &json!({ "kind": "end" })).await?;
    let ack = read_frame(&mut recv).await?;
    if ack["kind"] != "endAck" {
        return Err(CoreError::execution("TRANSFER_PROTOCOL", "缺少结束确认"));
    }
    let _ = send.finish();
    Ok(json!({
        "files": items.len(),
        "bytes": items.iter().map(|item| item.size).sum::<u64>()
    }))
}

// ---------------------------------------------------------------------------
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
    match result {
        Ok(mut summary) => {
            if let Some(map) = summary.as_object_mut() {
                map.insert("kind".to_string(), json!("done"));
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

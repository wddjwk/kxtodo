//! 剪切板同步（v0.8.10 重构）。
//!
//! 配对模型不再有「同步口令」：
//! - **局域网**：UDP 广播/组播发现所有开启该功能的设备；双方各自在界面上勾选后同步。
//! - **广域网统一服务器**：同一 kxtodo-server 上同一账户的设备互相发现，再按勾选同步。
//! - **广域网 P2P**：账户名 + 密码派生一个 pkarr 房间，同账户设备互相发现，同样按勾选同步。
//!
//! 安全：
//! - 设备身份 = `runtime/clipboard-identity.json` 里持久化的 Ed25519 私钥；
//!   局域网握手用它对「角色|设备|nonce|临时公钥」签名，对端按设备 id 验签。
//! - 每次连接生成一对临时 X25519 密钥，会话密钥 = SHA256(共享密钥 + 双方 nonce + 双方设备 id)；
//!   之后所有帧用 ChaCha20-Poly1305，AAD 绑定本次握手。抓包看不到明文。
//! - 广域网走 iroh，本身已是端到端加密的 QUIC；仍然复用同一套握手做设备级校验。
//!
//! **锁纪律**：任何 `Mutex` 都不许在持锁期间调用 `emit_status` / 网络 / 文件 I/O。
//! v0.8.9 的「发现第一台设备就整机卡死」就是 `push_peer` 持着 `lan_peers` 锁去
//! `emit_status` → `status_snapshot` → 再次锁 `lan_peers`（std Mutex 不可重入）。
//! 状态快照一律先克隆、放锁、再拼 JSON。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey, Signature, TransportAddr};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use tokio::sync::Notify;
use x25519_dalek::{PublicKey as XPublicKey, StaticSecret as XSecret};

use crate::error::{CoreError, CoreResult};
use crate::host::HostBackend;
use crate::model::{ClipboardLimits, ClipboardSettings, SettingsFile};
use crate::repo::Layout;
use crate::sync::crypto::derive_keys;
use crate::sync::p2p::{directory, net};
use crate::sync::transport::SyncClient;

/// 独立的 ALPN（与同步 `kxtodo-p2p/1`、传输 `kxtodo-transfer/1` 不同）。
pub const ALPN: &[u8] = b"kxtodo-clipboard/2";
/// 局域网发现端口（固定，所有设备一致）。
pub const DISCOVERY_PORT: u16 = 52178;
/// 局域网 TCP 默认端口；被占用时自动换临时端口并在广播里带实际端口。
pub const LAN_PORT: u16 = 52179;

/// 诊断用：允许探针/同机多实例改端口（`KXTODO_CLIPBOARD_DISCOVERY_PORT` /
/// `KXTODO_CLIPBOARD_LAN_PORT`）。正常应用不设置这两个环境变量，端口仍是固定值。
fn discovery_port() -> u16 {
    std::env::var("KXTODO_CLIPBOARD_DISCOVERY_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DISCOVERY_PORT)
}

fn preferred_lan_port() -> u16 {
    std::env::var("KXTODO_CLIPBOARD_LAN_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(LAN_PORT)
}
/// kxtodo-server 默认端口（用户只填 IP 时补这个）。
pub const SERVER_DEFAULT_PORT: u16 = 52177;

const APP: &str = "kxtodo-clipboard";
const PROTOCOL_VERSION: u32 = 2;
const DISCOVERY_TICK: Duration = Duration::from_secs(2);
const LAN_FRESH: Duration = Duration::from_secs(10);
const WAN_TICK: Duration = Duration::from_secs(5);
const WAN_FRESH: Duration = Duration::from_secs(60);
const REPUBLISH_SECS: u64 = 20;
const PEER_NAME_FRESH: Duration = Duration::from_secs(30);
const CLIPBOARD_POLL: Duration = Duration::from_millis(800);
const RECENT_TTL: Duration = Duration::from_secs(300);
const DIRECTORY_FRESH_SECS: u64 = 60;
const MAX_HANDSHAKE_FRAME: usize = 64 * 1024;
const MAX_CTRL_FRAME: usize = 1024 * 1024;
const MAX_BLOB_FRAME: usize = 1024 * 1024;
const CHUNK: usize = 256 * 1024;
const MAX_MB: u64 = 1024 * 1024;
const MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 53);

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
    /// 走到对端后文件会被改名落盘，若把路径算进指纹，回声抑制就永远对不上。
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
            hasher.update((file_size(file).unwrap_or(0) as u64).to_le_bytes());
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

// ---------------------------------------------------------------------------
// 位图编解码纯逻辑（v0.9.1，桌面壳 clipboard_os.rs 读写剪切板共用；有单测）
// ---------------------------------------------------------------------------

/// RGBA 的 alpha 全为 0 时整体置 255。
///
/// Windows 上大量程序（截图工具、部分浏览器）往 CF_DIBV5 里塞带 alpha 掩码
/// 但 alpha 字节全 0 的位图；按「真透明」消费就是一张全透明的图——粘贴到哪里
/// 都看不见，也就是「收不到图」的一种形态。全透明的剪切板图片不可能是本意，
/// 恢复成不透明；只要有任何一个非 0 alpha 就完全不动（那是真透明）。
pub fn repair_zero_alpha(rgba: &mut [u8]) {
    if rgba.is_empty() || rgba.len() % 4 != 0 {
        return;
    }
    let has_alpha = rgba.iter().skip(3).step_by(4).any(|alpha| *alpha != 0);
    if !has_alpha {
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }
}

/// RGBA → CF_DIBV5（BITMAPV5HEADER + 自下而上的 BGRA），与 arboard 写入的
/// 布局一致：BI_BITFIELDS + 四个掩码 + LCS_sRGB + 正高度（Word/WordPad 贴不了
/// 负高度的图）。写系统剪切板用（多格式一次会话写入时需要自己构造 DIB）。
pub fn cf_dibv5_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    const HEADER: usize = 124;
    let mut out = vec![0u8; HEADER + w * h * 4];
    let mut put = |offset: usize, bytes: &[u8]| {
        out[offset..offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, &(HEADER as u32).to_le_bytes()); // bV5Size
    put(4, &width.to_le_bytes()); // bV5Width
    put(8, &height.to_le_bytes()); // bV5Height（正 = 自下而上）
    put(12, &1u16.to_le_bytes()); // bV5Planes
    put(14, &32u16.to_le_bytes()); // bV5BitCount
    put(16, &3u32.to_le_bytes()); // bV5Compression = BI_BITFIELDS
    put(20, &((w * h * 4) as u32).to_le_bytes()); // bV5SizeImage
    put(40, &0x00ff_0000u32.to_le_bytes()); // bV5RedMask
    put(44, &0x0000_ff00u32.to_le_bytes()); // bV5GreenMask
    put(48, &0x0000_00ffu32.to_le_bytes()); // bV5BlueMask
    put(52, &0xff00_0000u32.to_le_bytes()); // bV5AlphaMask
    put(56, &0x7352_4742u32.to_le_bytes()); // bV5CSType = LCS_sRGB
    put(108, &4u32.to_le_bytes()); // bV5Intent = LCS_GM_IMAGES
    drop(put);
    for y in 0..h {
        // 自下而上：存储的第 0 行是图像的最底行。
        let src_row = (h - 1 - y) * w * 4;
        let dst_row = HEADER + y * w * 4;
        for x in 0..w {
            let s = src_row + x * 4;
            let d = dst_row + x * 4;
            out[d] = rgba[s + 2]; // B
            out[d + 1] = rgba[s + 1]; // G
            out[d + 2] = rgba[s]; // R
            out[d + 3] = rgba[s + 3]; // A
        }
    }
    Some(out)
}

/// 解码 CF_DIB / CF_DIBV5（BITMAPINFOHEADER 家族 + 像素）成 RGBA。
///
/// arboard 3.7 在 Windows 上只认「PNG 注册格式 → CF_DIBV5」，只给 CF_DIB /
/// CF_BITMAP 的应用（老截图工具、看图软件、远程桌面类）会被整张静默跳过——
/// 这是「收不到图」的另一大来源（CF_BITMAP 系统会自动合成 CF_DIB，读 CF_DIB
/// 即覆盖）。支持真实世界会出现的全部品种：32bpp BI_RGB、32bpp BI_BITFIELDS
/// （40 字节头后随掩码，或 DIBV5 头内掩码）、24bpp BI_RGB；负高度 = 顶向下。
/// 调色板 / RLE / 16bpp 不支持（返回 None，调用方继续走别的格式）。
pub fn rgba_from_cf_dib(dib: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if dib.len() < 40 {
        return None;
    }
    let u32_at = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(dib.get(offset..offset + 4)?.try_into().ok()?))
    };
    let u16_at = |offset: usize| -> Option<u16> {
        Some(u16::from_le_bytes(dib.get(offset..offset + 2)?.try_into().ok()?))
    };
    let header_size = u32_at(0)? as usize;
    if header_size < 40 || dib.len() < header_size {
        return None;
    }
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bit_count = u16_at(14)?;
    let compression = u32_at(16)?;
    if width <= 0 || height == 0 {
        return None;
    }
    let width = width as usize;
    let top_down = height < 0;
    let height = height.unsigned_abs() as usize;
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    if compression != BI_RGB && compression != BI_BITFIELDS {
        return None;
    }
    // 掩码来源：DIBV5（头 ≥ 56 字节且掩码非 0）在头内；40 字节头的 BI_BITFIELDS
    // 紧跟头后 12 字节；BI_RGB 用默认 BGRA 排布。
    // 32bpp BI_RGB 的 alpha 字节按「有意义」处理（Chrome 系会塞真 alpha；
    // 全 0 的坏数据交给 repair_zero_alpha 兜底）。
    let (mask_r, mask_g, mask_b, mask_a, pixel_offset) = match (bit_count, compression) {
        (32, BI_RGB) => (0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000, header_size),
        (32, BI_BITFIELDS) if header_size >= 56 && u32_at(40)? != 0 => {
            (u32_at(40)?, u32_at(44)?, u32_at(48)?, u32_at(52).unwrap_or(0), header_size)
        }
        (32, BI_BITFIELDS) if header_size == 40 && dib.len() >= header_size + 12 => (
            u32_at(header_size)?,
            u32_at(header_size + 4)?,
            u32_at(header_size + 8)?,
            0,
            header_size + 12,
        ),
        (24, BI_RGB) => (0, 0, 0, 0, header_size),
        _ => return None,
    };
    let bytes_per_pixel = if bit_count == 32 { 4 } else { 3 };
    let stride = ((width * bytes_per_pixel + 3) / 4) * 4;
    let pixels_len = stride.checked_mul(height)?;
    if dib.len() < pixel_offset + pixels_len {
        return None;
    }
    if width.checked_mul(height)? > 64 * 1024 * 1024 {
        return None; // 64M 像素以上的「剪切板图片」是坏头，别分配几百 MB
    }
    let shift_of = |mask: u32| -> Option<usize> {
        if mask == 0 {
            return None;
        }
        let shift = mask.trailing_zeros() as usize;
        (mask >> shift == 0xff).then_some(shift)
    };
    let mut rgba = vec![0u8; width * height * 4];
    for row in 0..height {
        let src_row = pixel_offset + if top_down { row } else { height - 1 - row } * stride;
        let dst_row = row * width * 4;
        for col in 0..width {
            let dst = dst_row + col * 4;
            if bit_count == 24 {
                let s = src_row + col * 3;
                rgba[dst] = dib[s + 2]; // R
                rgba[dst + 1] = dib[s + 1]; // G
                rgba[dst + 2] = dib[s]; // B
                rgba[dst + 3] = 255;
            } else {
                let s = src_row + col * 4;
                let value = u32::from_le_bytes([dib[s], dib[s + 1], dib[s + 2], dib[s + 3]]);
                let channel = |mask: u32, shift: Option<usize>, fallback: u8| -> u8 {
                    match shift {
                        Some(shift) => ((value & mask) >> shift) as u8,
                        None => fallback,
                    }
                };
                rgba[dst] = channel(mask_r, shift_of(mask_r), 0);
                rgba[dst + 1] = channel(mask_g, shift_of(mask_g), 0);
                rgba[dst + 2] = channel(mask_b, shift_of(mask_b), 0);
                rgba[dst + 3] = channel(mask_a, shift_of(mask_a), 255);
            }
        }
    }
    Some((width as u32, height as u32, rgba))
}

/// 发送端因**本机**类型上限要舍弃的部分（与 `send_clipboard` 的逐段闸同口径）。
/// 返回值拼进「已发送」事件的 detail——超限的大图/大文件绝不无声消失
/// （v0.9.1 之前：图片超 imageMb 时只发剩余部分，两端都没有任何提示）。
fn local_limit_note(content: &ClipboardContent, limits: &ClipboardLimits) -> String {
    let (text_limit, image_limit, files_limit) = limit_bytes(limits);
    let mut dropped: Vec<&str> = Vec::new();
    if !content.text.is_empty() && content.text.len() as u64 > text_limit {
        dropped.push("文本超上限");
    }
    if content
        .image
        .as_ref()
        .is_some_and(|image| image.len() as u64 > image_limit)
    {
        dropped.push("图片超上限");
    }
    let mut total = 0u64;
    let mut files_dropped = false;
    for file in &content.files {
        let Some(size) = file_size(file) else { continue };
        if total.saturating_add(size) > files_limit {
            files_dropped = true;
            break;
        }
        total += size;
    }
    if !files_dropped
        && !content.html.is_empty()
        && total.saturating_add(content.html.len() as u64) > files_limit
    {
        files_dropped = true;
    }
    if files_dropped {
        dropped.push("部分文件超上限");
    }
    if dropped.is_empty() {
        String::new()
    } else {
        format!("（{}未发送）", dropped.join("、"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hello {
    v: u32,
    app: String,
    device: String,
    name: String,
    nonce: String,
    eph: String,
    sig: String,
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
    eph: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sig: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresenceBody {
    device_id: String,
    name: String,
    endpoint_id: String,
    #[serde(default)]
    addrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerPeer {
    device_id: String,
    name: String,
    endpoint_id: String,
    #[serde(default)]
    addrs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ServerPeersResponse {
    #[serde(default)]
    peers: Vec<ServerPeer>,
}

struct SessionInfo {
    key: [u8; 32],
    aad: Vec<u8>,
    peer_id: String,
    peer_name: String,
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(raw: &str) -> Option<Vec<u8>> {
    let raw = raw.trim();
    if raw.len() % 2 != 0 || raw.is_empty() {
        return None;
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&raw[index..index + 2], 16).ok())
        .collect()
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    hex(&buffer)
}

fn random_secret() -> XSecret {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    XSecret::from(bytes)
}

fn session_key(
    shared: &[u8; 32],
    client_nonce: &str,
    server_nonce: &str,
    client_device: &str,
    server_device: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"kxtodo-clipboard/2/session\0");
    hasher.update(shared);
    hasher.update(b"\0");
    hasher.update(client_nonce.as_bytes());
    hasher.update(b"\0");
    hasher.update(server_nonce.as_bytes());
    hasher.update(b"\0");
    hasher.update(client_device.as_bytes());
    hasher.update(b"\0");
    hasher.update(server_device.as_bytes());
    hasher.finalize().into()
}

fn session_aad(client_nonce: &str, server_nonce: &str) -> Vec<u8> {
    format!("kxtodo-clipboard/2|{client_nonce}|{server_nonce}").into_bytes()
}

fn hello_message(role: &str, device: &str, nonce: &str, eph: &str) -> Vec<u8> {
    format!("kxtodo-clipboard/2/{role}|{device}|{nonce}|{eph}").into_bytes()
}

fn verify_device_signature(device: &str, message: &[u8], sig_hex: &str) -> CoreResult<EndpointId> {
    let id = EndpointId::from_z32(device)
        .map_err(|_| CoreError::validation("CLIPBOARD_BAD_DEVICE", "对方设备 id 不合法"))?;
    let bytes = unhex(sig_hex)
        .ok_or_else(|| CoreError::validation("CLIPBOARD_BAD_SIGNATURE", "握手签名格式不对"))?;
    let signature = Signature::try_from(bytes.as_slice())
        .map_err(|_| CoreError::validation("CLIPBOARD_BAD_SIGNATURE", "握手签名长度不对"))?;
    id.verify(message, &signature)
        .map_err(|_| CoreError::validation("CLIPBOARD_BAD_SIGNATURE", "握手签名校验失败"))?;
    Ok(id)
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
        .map_err(|_| CoreError::validation("CLIPBOARD_DECRYPT", "剪切板内容解密失败"))
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
// 身份、命名与设置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardNet {
    pub relay: Option<String>,
    pub directory_url: String,
    pub server_url: String,
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
        Self {
            relay,
            directory_url,
            server_url: settings.clipboard.server_url.clone(),
        }
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
            if let Some(bytes) = unhex(&parsed.secret).and_then(|bytes| bytes.try_into().ok()) {
                return Ok(SecretKey::from_bytes(&bytes));
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

fn platform_label() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "android" => "Android",
        "linux" => "Linux",
        "macos" => "macOS",
        "ios" => "iOS",
        other => other,
    }
}

/// 设备显示名：自定义前缀 + `·平台`；没填前缀时是 `kxtodo·平台(设备 id 前 8 位)`。
pub fn device_display_name(device_id: &str, base: &str) -> String {
    let platform = platform_label();
    let base = directory::sanitize_name(base);
    if base.is_empty() {
        let suffix: String = device_id.chars().take(8).collect();
        if suffix.is_empty() {
            format!("kxtodo·{platform}")
        } else {
            format!("kxtodo·{platform}({suffix})")
        }
    } else {
        format!("{base}·{platform}")
    }
}

fn normalize_server_url(raw: &str) -> CoreResult<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let candidate = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let mut url = url::Url::parse(&candidate).map_err(|error| {
        CoreError::validation(
            "CLIPBOARD_SERVER_URL_INVALID",
            format!("服务器地址不合法（{raw}）：{error}"),
        )
    })?;
    // 用户只填 IP 是常态：补 kxtodo-server 默认端口；显式写了端口就尊重。
    if url.scheme() == "http" && url.port().is_none() {
        let _ = url.set_port(Some(SERVER_DEFAULT_PORT));
    }
    let mut out = url.to_string();
    if out.ends_with('/') {
        out.pop();
    }
    Ok(out)
}

fn normalized(settings: &ClipboardSettings) -> ClipboardSettings {
    let mut value = settings.clone();
    value.device_name = directory::sanitize_name(&value.device_name);
    value.server_url = value.server_url.trim().to_string();
    value.account = value.account.trim().to_string();
    value.password = value.password.trim().to_string();
    value.save_dir = value.save_dir.trim().to_string();
    value.limits.text_mb = value.limits.text_mb.clamp(1, 64);
    value.limits.image_mb = value.limits.image_mb.clamp(1, 256);
    value.limits.files_mb = value.limits.files_mb.clamp(1, 1024);
    value
}

fn requires_wan(value: &ClipboardSettings) -> bool {
    value.server_enabled || value.p2p_enabled
}

/// 一次写整份剪切板设置时的校验（GUI「保存并应用」与 CLI config set 都走这里）。
pub fn normalize_settings(settings: &ClipboardSettings) -> CoreResult<ClipboardSettings> {
    let mut value = normalized(settings);
    value.server_url = normalize_server_url(&value.server_url)?;
    // 这里刻意**不**因为缺少账户/密码/传输方式而拒绝保存：旧配置升级、CLI 分步配置、
    // 用户先把开关打开再填凭据都应当保留设置；能启动的部分照常启动，缺的那块由
    // 状态里的 lastError 说明（GUI 的保存按钮另有即时校验）。
    Ok(value)
}

fn wan_ready(value: &ClipboardSettings) -> bool {
    requires_wan(value) && !value.account.is_empty() && !value.password.is_empty()
}

fn limit_bytes(limits: &ClipboardLimits) -> (u64, u64, u64) {
    (
        limits.text_mb as u64 * MAX_MB,
        limits.image_mb as u64 * MAX_MB,
        limits.files_mb as u64 * MAX_MB,
    )
}

// ---------------------------------------------------------------------------
// 每台设备的勾选（本机偏好，单独落 runtime 文件）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SelectionFile {
    #[serde(default)]
    disabled: Vec<String>,
}

fn selection_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join("clipboard-selection.json")
}

fn load_disabled(layout: &Layout) -> BTreeSet<String> {
    let Ok(raw) = std::fs::read_to_string(selection_path(layout)) else {
        return BTreeSet::new();
    };
    let Ok(parsed) = serde_json::from_str::<SelectionFile>(&raw) else {
        return BTreeSet::new();
    };
    parsed
        .disabled
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .take(2048)
        .collect()
}

fn save_disabled(layout: &Layout, disabled: &BTreeSet<String>) -> CoreResult<()> {
    let payload = serde_json::to_string_pretty(&SelectionFile {
        disabled: disabled.iter().cloned().collect(),
    })?;
    let path = selection_path(layout);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::repo::atomic_write(&path, &payload)?;
    Ok(())
}

/// 界面上勾选/取消一台设备。立刻作用于正在跑的服务，不做重启。
pub fn set_peer_selected(layout: &Layout, device_id: &str, selected: bool) -> CoreResult<()> {
    let id = device_id.trim();
    if id.is_empty() {
        return Err(CoreError::validation("CLIPBOARD_BAD_DEVICE", "缺少设备 id"));
    }
    let mut disabled = load_disabled(layout);
    if selected {
        disabled.remove(id);
    } else {
        if disabled.len() >= 2048 {
            disabled.pop_first();
        }
        disabled.insert(id.to_string());
    }
    save_disabled(layout, &disabled)?;
    if let Some(inner) = running_slot(layout) {
        if let Ok(mut slot) = inner.disabled.lock() {
            *slot = disabled;
        }
        inner.emit_status();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 服务状态
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct LanPeer {
    name: String,
    addr: SocketAddr,
    last_seen: Instant,
}

#[derive(Clone)]
struct WanPeer {
    name: String,
    addr: EndpointAddr,
    server_seen: Option<Instant>,
    p2p_seen: Option<Instant>,
}

impl WanPeer {
    fn fresh(&self, now: Instant) -> bool {
        self.server_seen
            .is_some_and(|at| now.duration_since(at) <= WAN_FRESH)
            || self.p2p_seen
                .is_some_and(|at| now.duration_since(at) <= WAN_FRESH)
    }

    fn transports(&self, now: Instant) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.server_seen.is_some_and(|at| now.duration_since(at) <= WAN_FRESH) {
            out.push("server");
        }
        if self.p2p_seen.is_some_and(|at| now.duration_since(at) <= WAN_FRESH) {
            out.push("p2p");
        }
        out
    }
}

struct Inner {
    layout: Layout,
    settings: ClipboardSettings,
    net: ClipboardNet,
    backend: Option<Arc<dyn HostBackend>>,
    signature: String,
    cancel: AtomicBool,
    /// 停止信号（notify_waiters + notify_one，防止重试窗口丢唤醒）。
    notify: Notify,
    /// 「刷新」：立刻广播 / 拉一次发现结果，不等下一轮定时器。
    refresh: Notify,
    secret: SecretKey,
    device_id: String,
    name: String,
    save_dir: PathBuf,
    tcp_port: Mutex<u16>,
    endpoint: Mutex<Option<Endpoint>>,
    lan_peers: Mutex<BTreeMap<String, LanPeer>>,
    wan_peers: Mutex<BTreeMap<String, WanPeer>>,
    disabled: Mutex<BTreeSet<String>>,
    recent: Mutex<BTreeMap<String, Instant>>,
    last_local: Mutex<String>,
    last_remote: Mutex<String>,
    last_error: Mutex<String>,
    lan_error: Mutex<String>,
    endpoint_error: Mutex<String>,
    server_error: Mutex<String>,
    p2p_error: Mutex<String>,
    last_event: Mutex<Option<Value>>,
    last_status: Mutex<String>,
    server_token: Mutex<Option<String>>,
    server_ok: AtomicBool,
    p2p_ok: AtomicBool,
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

fn running_slot(layout: &Layout) -> Option<Arc<Inner>> {
    let guard = running().lock().ok()?;
    guard.get(&slot_key(layout)).cloned()
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
    let signature = serde_json::to_string(&(&config, &net))?;
    let key = slot_key(layout);
    // 广域网开关开着但凭据还没填：局域网仍然要能跑，不能把它一起拖停。
    let enabled = config.enabled && (config.lan_enabled || wan_ready(&config));
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
    let name = device_display_name(&device_id, &config.device_name);
    let save_dir = if config.save_dir.is_empty() {
        backend
            .as_ref()
            .and_then(|backend| backend.clipboard_default_dir())
            .unwrap_or_else(|| layout.root.join("clipboard-files"))
    } else {
        PathBuf::from(config.save_dir.trim())
    };
    std::fs::create_dir_all(&save_dir)?;
    let disabled = load_disabled(layout);
    let inner = Arc::new(Inner {
        layout: layout.clone(),
        settings: config,
        net,
        backend,
        signature,
        cancel: AtomicBool::new(false),
        notify: Notify::new(),
        refresh: Notify::new(),
        secret,
        device_id,
        name,
        save_dir,
        tcp_port: Mutex::new(0),
        endpoint: Mutex::new(None),
        lan_peers: Mutex::new(BTreeMap::new()),
        wan_peers: Mutex::new(BTreeMap::new()),
        disabled: Mutex::new(disabled),
        recent: Mutex::new(BTreeMap::new()),
        last_local: Mutex::new(String::new()),
        last_remote: Mutex::new(String::new()),
        last_error: Mutex::new(String::new()),
        lan_error: Mutex::new(String::new()),
        endpoint_error: Mutex::new(String::new()),
        server_error: Mutex::new(String::new()),
        p2p_error: Mutex::new(String::new()),
        last_event: Mutex::new(None),
        last_status: Mutex::new(String::new()),
        server_token: Mutex::new(None),
        server_ok: AtomicBool::new(false),
        p2p_ok: AtomicBool::new(false),
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
    // notify_waiters 只唤醒此刻在等的人；run_service 可能还在重试绑端口，
    // notify_one 留下 permit，保证它随后一定能醒。
    inner.notify.notify_waiters();
    inner.notify.notify_one();
    let endpoint = inner.endpoint.lock().ok().and_then(|mut slot| slot.take());
    if let Some(endpoint) = endpoint {
        if let Ok(runtime) = runtime() {
            runtime.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(3), endpoint.close()).await;
            });
        }
    }
}

pub fn stop_for_layout(layout: &Layout) {
    stop(&slot_key(layout));
}

fn pick_error(slots: &[String]) -> String {
    slots
        .iter()
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// 状态快照。服务没跑时也返回「为什么没跑」，UI 不用自己猜。
pub fn status(layout: &Layout, settings: &SettingsFile) -> Value {
    if let Some(inner) = running_slot(layout) {
        return inner.status_value();
    }
    let config = normalized(&settings.clipboard);
    let reason = if !config.enabled {
        "未启用"
    } else if !(config.lan_enabled || requires_wan(&config)) {
        "请至少开启一种同步方式"
    } else if requires_wan(&config) && (config.account.is_empty() || config.password.is_empty()) {
        "请填写广域网账户和密码"
    } else if config.server_enabled && config.server_url.is_empty() {
        "请填写统一服务器地址"
    } else {
        "未运行"
    };
    let device_id = identity_secret(layout)
        .map(|secret| secret.public().to_z32())
        .unwrap_or_default();
    json!({
        "running": false,
        "enabled": config.enabled,
        "deviceId": device_id,
        "deviceName": device_display_name(&device_id, &config.device_name),
        "port": 0,
        "discoveryPort": discovery_port(),
        "localAddresses": local_ipv4s(),
        "transports": {
            "lan": config.lan_enabled,
            "server": config.server_enabled,
            "p2p": config.p2p_enabled,
        },
        "serverRegistered": false,
        "p2pActive": false,
        "peers": [],
        "lastError": reason,
        "lastEvent": Value::Null,
        "sent": 0,
        "received": 0,
        "saveDir": config.save_dir,
    })
}

/// 立刻触发一轮发现（局域网广播 + 服务器/P2P 拉取），不等定时器。
pub fn refresh(layout: &Layout, settings: &SettingsFile) -> Value {
    if let Some(inner) = running_slot(layout) {
        inner.refresh.notify_waiters();
        inner.emit_status();
        return inner.status_value();
    }
    status(layout, settings)
}

/// 移动端前台读到的剪切板：交给服务去发送。桌面侧由 Rust 轮询线程直接调用。
pub fn local_changed(layout: &Layout, content: ClipboardContent) -> CoreResult<Value> {
    let Some(inner) = running_slot(layout) else {
        return Ok(json!({ "sent": 0, "running": false }));
    };
    inner.on_local(content);
    Ok(json!({ "queued": true }))
}

impl Inner {
    fn set_error_slot(slot: &Mutex<String>, message: impl Into<String>) {
        if let Ok(mut value) = slot.lock() {
            *value = message.into();
        }
    }

    fn set_lan_error(&self, message: impl Into<String>) {
        Self::set_error_slot(&self.lan_error, message);
        self.emit_status();
    }

    fn set_endpoint_error(&self, message: impl Into<String>) {
        Self::set_error_slot(&self.endpoint_error, message);
        self.emit_status();
    }

    fn set_server_error(&self, message: impl Into<String>) {
        Self::set_error_slot(&self.server_error, message);
        self.emit_status();
    }

    fn set_p2p_error(&self, message: impl Into<String>) {
        Self::set_error_slot(&self.p2p_error, message);
        self.emit_status();
    }

    fn set_error(&self, message: impl Into<String>) {
        Self::set_error_slot(&self.last_error, message);
        self.emit_status();
    }

    fn clear_send_error(&self) {
        Self::set_error_slot(&self.last_error, "");
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
        // 先完整取快照（内部逐个短锁、放锁），再序列化、比较、发送。
        let snapshot = self.status_value();
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

    fn lane_error(&self) -> String {
        pick_error(&[
            self.last_error.lock().map(|v| v.clone()).unwrap_or_default(),
            self.lan_error.lock().map(|v| v.clone()).unwrap_or_default(),
            self.endpoint_error.lock().map(|v| v.clone()).unwrap_or_default(),
            self.server_error.lock().map(|v| v.clone()).unwrap_or_default(),
            self.p2p_error.lock().map(|v| v.clone()).unwrap_or_default(),
        ])
    }

    fn peer_selected(&self, device_id: &str) -> bool {
        self.disabled
            .lock()
            .map(|slot| !slot.contains(device_id))
            .unwrap_or(true)
    }

    fn status_value(&self) -> Value {
        let port = self.tcp_port.lock().map(|slot| *slot).unwrap_or(0);
        let lan_peers = self
            .lan_peers
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let wan_peers = self
            .wan_peers
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let disabled = self
            .disabled
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let last_event = self
            .last_event
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .unwrap_or(Value::Null);
        let now = Instant::now();
        let mut merged: BTreeMap<String, (String, Vec<&'static str>)> = BTreeMap::new();
        for (id, peer) in &lan_peers {
            if now.duration_since(peer.last_seen) > LAN_FRESH {
                continue;
            }
            let entry = merged
                .entry(id.clone())
                .or_insert_with(|| (peer.name.clone(), Vec::new()));
            if entry.0.is_empty() {
                entry.0 = peer.name.clone();
            }
            entry.1.push("lan");
        }
        for (id, peer) in &wan_peers {
            if !peer.fresh(now) {
                continue;
            }
            let entry = merged
                .entry(id.clone())
                .or_insert_with(|| (peer.name.clone(), Vec::new()));
            if entry.0.is_empty() {
                entry.0 = peer.name.clone();
            }
            entry.1.extend(peer.transports(now));
        }
        let mut peers: Vec<Value> = merged
            .into_iter()
            .map(|(id, (name, mut transports))| {
                transports.dedup();
                json!({
                    "id": id,
                    "name": if name.is_empty() { "未命名设备" } else { name.as_str() },
                    "transports": transports,
                    "selected": !disabled.contains(&id),
                })
            })
            .collect();
        peers.sort_by(|left, right| {
            let a = left["name"].as_str().unwrap_or("");
            let b = right["name"].as_str().unwrap_or("");
            a.cmp(b)
                .then_with(|| left["id"].as_str().unwrap_or("").cmp(right["id"].as_str().unwrap_or("")))
        });
        json!({
            "running": !self.cancel.load(Ordering::SeqCst),
            "enabled": true,
            "deviceId": self.device_id,
            "deviceName": self.name,
            "port": port,
            "discoveryPort": discovery_port(),
            "localAddresses": local_ipv4s(),
            "transports": {
                "lan": self.settings.lan_enabled,
                "server": self.settings.server_enabled,
                "p2p": self.settings.p2p_enabled,
            },
            "serverRegistered": self.server_ok.load(Ordering::Relaxed),
            "p2pActive": self.p2p_ok.load(Ordering::Relaxed),
            "peers": peers,
            "lastError": self.lane_error(),
            "lastEvent": last_event,
            "sent": self.sent_count.load(Ordering::Relaxed),
            "received": self.received_count.load(Ordering::Relaxed),
            "saveDir": self.save_dir.to_string_lossy(),
        })
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

    fn push_lan_peer(&self, device_id: &str, name: &str, addr: SocketAddr) -> bool {
        let is_new = {
            let Ok(mut peers) = self.lan_peers.lock() else { return false };
            let is_new = !peers.contains_key(device_id);
            if let Some(existing) = peers.get_mut(device_id) {
                existing.name = name.to_string();
                existing.addr = addr;
                existing.last_seen = Instant::now();
            } else {
                peers.insert(
                    device_id.to_string(),
                    LanPeer {
                        name: name.to_string(),
                        addr,
                        last_seen: Instant::now(),
                    },
                );
            }
            is_new
        };
        // 锁已放：emit_status 会重新锁 lan_peers 取快照。
        self.emit_status();
        is_new
    }

    fn prune_lan(&self) {
        let changed = {
            let Ok(mut peers) = self.lan_peers.lock() else { return };
            let before = peers.len();
            peers.retain(|_, peer| peer.last_seen.elapsed() <= LAN_FRESH);
            peers.len() != before
        };
        if changed {
            self.emit_status();
        }
    }

    fn apply_wan_source(&self, source: WanSource, entries: Vec<(String, String, EndpointAddr)>) {
        let now = Instant::now();
        {
            let Ok(mut peers) = self.wan_peers.lock() else { return };
            for peer in peers.values_mut() {
                match source {
                    WanSource::Server => peer.server_seen = None,
                    WanSource::P2p => peer.p2p_seen = None,
                }
            }
            for (id, name, addr) in entries {
                if id == self.device_id {
                    continue;
                }
                let peer = peers.entry(id.clone()).or_insert_with(|| WanPeer {
                    name: name.clone(),
                    addr: addr.clone(),
                    server_seen: None,
                    p2p_seen: None,
                });
                if !name.is_empty() {
                    peer.name = name;
                }
                if !addr.addrs.is_empty() {
                    peer.addr = addr;
                }
                match source {
                    WanSource::Server => peer.server_seen = Some(now),
                    WanSource::P2p => peer.p2p_seen = Some(now),
                }
            }
            peers.retain(|_, peer| peer.fresh(now));
        }
        self.emit_status();
    }

    fn recent_seen(&self, id: &str) -> bool {
        let Ok(recent) = self.recent.lock() else { return false };
        recent.get(id).is_some_and(|at| at.elapsed() < RECENT_TTL)
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

    fn targets(&self) -> Vec<Target> {
        let lan = self
            .lan_peers
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let wan = self
            .wan_peers
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let disabled = self
            .disabled
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        let now = Instant::now();
        let mut map: BTreeMap<String, Target> = BTreeMap::new();
        for (id, peer) in lan {
            if now.duration_since(peer.last_seen) > LAN_FRESH {
                continue;
            }
            let entry = map.entry(id).or_insert_with(|| Target {
                name: peer.name.clone(),
                lan: None,
                wan: None,
            });
            if entry.name.is_empty() {
                entry.name = peer.name;
            }
            entry.lan = Some(peer.addr);
        }
        for (id, peer) in wan {
            if !peer.fresh(now) {
                continue;
            }
            let entry = map.entry(id).or_insert_with(|| Target {
                name: peer.name.clone(),
                lan: None,
                wan: None,
            });
            if entry.name.is_empty() {
                entry.name = peer.name;
            }
            entry.wan = Some(peer.addr);
        }
        map.into_iter()
            .filter(|(id, _)| !disabled.contains(id))
            .map(|(_, target)| target)
            .collect()
    }
}

enum WanSource {
    Server,
    P2p,
}

#[derive(Clone)]
struct Target {
    name: String,
    lan: Option<SocketAddr>,
    wan: Option<EndpointAddr>,
}

// ---------------------------------------------------------------------------
// 启动与传输循环
// ---------------------------------------------------------------------------

async fn run_service(inner: Arc<Inner>) {
    cleanup_inbox(&inner);
    if inner.settings.lan_enabled {
        let task = inner.clone();
        inner_runtime_spawn(async move {
            lan_listener_loop(task).await;
        });
        let task = inner.clone();
        inner_runtime_spawn(async move {
            lan_discovery_loop(task).await;
        });
    }
    if wan_ready(&inner.settings) {
        let task = inner.clone();
        inner_runtime_spawn(async move {
            wan_loop(task).await;
        });
    } else if requires_wan(&inner.settings) {
        inner.set_endpoint_error("广域网未启动：请填写账户名和密码");
    }
    if inner
        .backend
        .as_ref()
        .is_some_and(|backend| backend.clipboard_available())
    {
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
        if entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
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
                        Ok(_) => {
                            // 空剪切板同样算「基线已建」（v0.9.1）：基线的目的是不发
                            // 服务启动前就躺在剪切板里的旧内容；剪切板本来是空的时
                            // 没有旧内容可防，若仍把下一次读取当基线，用户开启同步后
                            // 的第一次复制（剪切板为空时启用是最常见路径）会被整次吞掉
                            // ——「有时收不到图」的真实缺陷之一。
                            primed = true;
                        }
                        Err(error) => {
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
// 局域网：UDP 发现
// ---------------------------------------------------------------------------

fn bind_discovery_socket() -> std::io::Result<std::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, discovery_port())).into())?;
    Ok(socket.into())
}

fn local_ipv4s() -> Vec<String> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if interface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = interface.addr {
                if !v4.ip.is_unspecified() {
                    out.push(v4.ip);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out.into_iter().map(|ip| ip.to_string()).collect()
}

fn broadcast_targets() -> Vec<SocketAddr> {
    let mut ips: Vec<Ipv4Addr> = vec![Ipv4Addr::BROADCAST];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if interface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = interface.addr {
                let broadcast = v4.broadcast.unwrap_or_else(|| {
                    let ip = u32::from(v4.ip);
                    let mask = u32::from(v4.netmask);
                    Ipv4Addr::from(ip | !mask)
                });
                if !broadcast.is_unspecified() {
                    ips.push(broadcast);
                }
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips.into_iter().map(|ip| SocketAddr::from((ip, discovery_port()))).collect()
}

async fn lan_discovery_loop(inner: Arc<Inner>) {
    let socket = loop {
        if inner.cancel.load(Ordering::SeqCst) {
            return;
        }
        match bind_discovery_socket() {
            Ok(std_socket) => match UdpSocket::from_std(std_socket) {
                Ok(socket) => break socket,
                Err(error) => {
                    inner.set_lan_error(format!("局域网发现套接字创建失败：{error}"));
                }
            },
            Err(error) => {
                inner.set_lan_error(format!(
                    "局域网发现端口 {} 暂不可用：{error}（稍后重试）", discovery_port()
                ));
            }
        }
        tokio::select! {
            _ = inner.notify.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
    };
    let _ = socket.set_broadcast(true);
    // 组播：Windows 需要逐个接口 join；Linux/Android 允许 INADDR_ANY。失败不致命，
    // 广播仍然是主路径。
    let mut joined = false;
    for ip in local_ipv4s() {
        if let Ok(addr) = ip.parse::<Ipv4Addr>() {
            if socket.join_multicast_v4(MULTICAST_V4, addr).is_ok() {
                joined = true;
            }
        }
    }
    if !joined {
        let _ = socket.join_multicast_v4(MULTICAST_V4, Ipv4Addr::UNSPECIFIED);
    }
    inner.set_lan_error("");
    let mut interval = tokio::time::interval(DISCOVERY_TICK);
    let mut buffer = vec![0u8; 4096];
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            _ = inner.refresh.notified() => {
                announce(&inner, &socket).await;
            }
            _ = interval.tick() => {
                announce(&inner, &socket).await;
                inner.prune_lan();
            }
            result = socket.recv_from(&mut buffer) => {
                let Ok((size, source)) = result else { continue };
                let Ok(packet) = serde_json::from_slice::<DiscoverPacket>(&buffer[..size]) else { continue };
                if packet.v != PROTOCOL_VERSION || packet.app != APP {
                    continue;
                }
                if packet.device == inner.device_id || packet.device.is_empty() || packet.port == 0 {
                    continue;
                }
                // 发现包里的名字可能是旧的；端口可能因默认端口被占用而变化。
                let addr = SocketAddr::new(source.ip(), packet.port);
                let is_new = inner.push_lan_peer(&packet.device, &packet.name, addr);
                if is_new {
                    // 单播回一包：对方的广播可能被 AP 隔离/防火墙挡掉，但反向单播通常能通。
                    let reply = serde_json::to_vec(&self_announce(&inner)).unwrap_or_default();
                    let _ = socket.send_to(&reply, source).await;
                }
            }
        }
    }
    let _ = socket.leave_multicast_v4(MULTICAST_V4, Ipv4Addr::UNSPECIFIED);
}

fn self_announce(inner: &Inner) -> DiscoverPacket {
    DiscoverPacket {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        port: inner.tcp_port.lock().map(|slot| *slot).unwrap_or(0),
    }
}

async fn announce(inner: &Inner, socket: &UdpSocket) {
    if inner.tcp_port.lock().map(|slot| *slot == 0).unwrap_or(true) {
        return;
    }
    let bytes = match serde_json::to_vec(&self_announce(inner)) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    for target in broadcast_targets() {
        let _ = socket.send_to(&bytes, target).await;
    }
    let _ = socket
        .send_to(&bytes, SocketAddr::from((MULTICAST_V4, discovery_port())))
        .await;
}

// ---------------------------------------------------------------------------
// 局域网：TCP 监听
// ---------------------------------------------------------------------------

async fn lan_listener_loop(inner: Arc<Inner>) {
    let mut listener = None;
    for _ in 0..5 {
        match TcpListener::bind(("0.0.0.0", preferred_lan_port())).await {
            Ok(bound) => {
                listener = Some(bound);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(120)).await,
        }
    }
    let listener = match listener {
        Some(listener) => listener,
        None => match TcpListener::bind(("0.0.0.0", 0)).await {
            Ok(listener) => listener,
            Err(error) => {
                inner.set_lan_error(format!("局域网监听失败：{error}"));
                return;
            }
        },
    };
    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(preferred_lan_port());
    if let Ok(mut slot) = inner.tcp_port.lock() {
        *slot = port;
    }
    inner.set_lan_error("");
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            result = listener.accept() => {
                let Ok((stream, _addr)) = result else { continue };
                let _ = stream.set_nodelay(true);
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(300), async {
                        let (mut reader, mut writer) = stream.into_split();
                        if let Err(error) = serve_stream(&task, &mut reader, &mut writer).await {
                            let _ = write_json(
                                &mut writer,
                                &json!({ "kind": "error", "message": error.message }),
                            )
                            .await;
                        }
                    })
                    .await;
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 握手（局域网 TCP 与广域网 iroh 共用）
// ---------------------------------------------------------------------------

async fn serve_stream<R, W>(inner: &Arc<Inner>, reader: &mut R, writer: &mut W) -> CoreResult<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let session = server_handshake(inner, reader, writer).await?;
    if !inner.peer_selected(&session.peer_id) {
        let _ = write_sealed_json(
            writer,
            &session,
            &json!({ "ok": false, "reason": "对方未勾选与本机同步" }),
        )
        .await;
        return Err(CoreError::validation(
            "CLIPBOARD_NOT_SELECTED",
            "未与对方勾选同步",
        ));
    }
    let header = read_sealed_json::<R>(reader, &session, MAX_CTRL_FRAME).await?;
    let parsed: WireHeader = serde_json::from_value(header.clone())
        .map_err(|error| CoreError::validation("CLIPBOARD_BAD_FRAME", format!("头部不合法：{error}")))?;
    if parsed.v != PROTOCOL_VERSION || parsed.app != APP {
        let _ = write_sealed_json(
            writer,
            &session,
            &json!({ "ok": false, "reason": "协议版本不匹配" }),
        )
        .await;
        return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "协议版本不匹配"));
    }
    if parsed.kind != "clipboard" {
        return Err(CoreError::validation("CLIPBOARD_BAD_FRAME", "未知消息类型"));
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
    if hello.device.is_empty() || hello.nonce.is_empty() || hello.eph.is_empty() {
        return Err(CoreError::validation("CLIPBOARD_BAD_HELLO", "握手字段不完整"));
    }
    verify_device_signature(
        &hello.device,
        &hello_message("hello", &hello.device, &hello.nonce, &hello.eph),
        &hello.sig,
    )?;
    if hello.device == inner.device_id {
        return Err(CoreError::validation("CLIPBOARD_SELF", "不能与自己同步"));
    }
    let eph_bytes = unhex(&hello.eph)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or_else(|| CoreError::validation("CLIPBOARD_BAD_HELLO", "临时公钥不合法"))?;
    let server_secret = random_secret();
    let server_public = XPublicKey::from(&server_secret);
    let server_nonce = random_hex(16);
    let server_eph = hex(server_public.as_bytes());
    let signature = inner.secret.sign(&hello_message(
        "ack",
        &inner.device_id,
        &server_nonce,
        &server_eph,
    ));
    let ack = HelloAck {
        ok: true,
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        nonce: server_nonce.clone(),
        eph: server_eph.clone(),
        sig: hex(&signature.to_bytes()),
        reason: String::new(),
    };
    write_json(writer, &serde_json::to_value(&ack)?).await?;
    let peer_public = XPublicKey::from(eph_bytes);
    let shared = server_secret.diffie_hellman(&peer_public);
    let key = session_key(
        shared.as_bytes(),
        &hello.nonce,
        &server_nonce,
        &hello.device,
        &inner.device_id,
    );
    Ok(SessionInfo {
        key,
        aad: session_aad(&hello.nonce, &server_nonce),
        peer_id: hello.device,
        peer_name: if hello.name.is_empty() {
            "未命名设备".to_string()
        } else {
            hello.name
        },
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
    let client_secret = random_secret();
    let client_public = XPublicKey::from(&client_secret);
    let client_nonce = random_hex(16);
    let client_eph = hex(client_public.as_bytes());
    let signature = inner.secret.sign(&hello_message(
        "hello",
        &inner.device_id,
        &client_nonce,
        &client_eph,
    ));
    let hello = Hello {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        device: inner.device_id.clone(),
        name: inner.name.clone(),
        nonce: client_nonce.clone(),
        eph: client_eph,
        sig: hex(&signature.to_bytes()),
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
    if ack.device.is_empty() || ack.nonce.is_empty() || ack.eph.is_empty() {
        return Err(CoreError::validation("CLIPBOARD_BAD_HELLO", "握手应答缺少设备信息"));
    }
    verify_device_signature(
        &ack.device,
        &hello_message("ack", &ack.device, &ack.nonce, &ack.eph),
        &ack.sig,
    )?;
    if ack.device == inner.device_id {
        return Err(CoreError::validation("CLIPBOARD_SELF", "不能与自己同步"));
    }
    let eph_bytes = unhex(&ack.eph)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or_else(|| CoreError::validation("CLIPBOARD_BAD_HELLO", "临时公钥不合法"))?;
    let peer_public = XPublicKey::from(eph_bytes);
    let shared = client_secret.diffie_hellman(&peer_public);
    let key = session_key(
        shared.as_bytes(),
        &client_nonce,
        &ack.nonce,
        &inner.device_id,
        &ack.device,
    );
    Ok(SessionInfo {
        key,
        aad: session_aad(&client_nonce, &ack.nonce),
        peer_id: ack.device,
        peer_name: if ack.name.is_empty() { "未命名设备".to_string() } else { ack.name },
    })
}

// ---------------------------------------------------------------------------
// 发送
// ---------------------------------------------------------------------------

async fn send_lan(
    inner: &Arc<Inner>,
    addr: SocketAddr,
    content: &ClipboardContent,
) -> CoreResult<()> {
    let stream = tokio::time::timeout(Duration::from_secs(6), TcpStream::connect(addr))
        .await
        .map_err(|_| CoreError::execution("CLIPBOARD_TIMEOUT", "连接对方超时"))??;
    let _ = stream.set_nodelay(true);
    let (mut reader, mut writer) = stream.into_split();
    let session = client_handshake(inner, &mut reader, &mut writer).await?;
    send_clipboard(inner, &mut reader, &mut writer, &session, content).await?;
    if let Ok(mut peers) = inner.lan_peers.lock() {
        if let Some(peer) = peers.get_mut(&session.peer_id) {
            peer.name = session.peer_name.clone();
            peer.addr = addr;
            peer.last_seen = Instant::now();
        }
    }
    Ok(())
}

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
    let (text_limit, image_limit, files_limit) = limit_bytes(&inner.settings.limits);
    let mut outgoing = ClipboardContent::default();
    if !content.text.is_empty() && content.text.len() as u64 <= text_limit {
        outgoing.text = content.text.clone();
    }
    // HTML 不再是独立通道（v0.9.0 bugfix 4.1）：读取端已经把 html 折成文件，
    // 这里即使有残留也按文件走，接收端落盘成一个 .html 文件
    if let Some(image) = &content.image {
        if image.len() as u64 <= image_limit {
            outgoing.image = Some(image.clone());
        }
    }
    let mut total_files = 0u64;
    // HTML 折成文件（v0.9.0 bugfix 4.1）：读取端一般已经折过，这里是兜底（老前端、
    // 或 CLI 直接喂 content 的路径），保证线上永远只有「文本 / 图片 / 文件」三档。
    let html_as_file = if content.html.is_empty() {
        None
    } else {
        Some(ClipboardFile {
            name: "clipboard.html".to_string(),
            path: None,
            bytes: Some(content.html.clone().into_bytes()),
        })
    };
    for file in content.files.iter().chain(html_as_file.iter()) {
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

impl Inner {
    async fn send_all(self: Arc<Self>, content: ClipboardContent) {
        let targets = self.targets();
        if targets.is_empty() {
            self.set_event("idle", "没有已勾选的在线设备", "");
            return;
        }
        // 因本机上限被舍弃的部分要拼进「已发送」事件：图片超限时只发剩余部分，
        // 之前两端都没有任何提示，用户看到的就是「发过去了但对方没收到图」。
        let limit_note = local_limit_note(&content, &self.settings.limits);
        let mut tasks = Vec::new();
        for target in targets {
            let inner = self.clone();
            let payload = content.clone();
            tasks.push(tokio::spawn(async move {
                let name = target.name.clone();
                match send_target(inner, &target, &payload).await {
                    Ok(()) => Ok(()),
                    Err(error) => Err((name, error.message)),
                }
            }));
        }
        let mut sent = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for task in tasks {
            match task.await {
                Ok(Ok(())) => sent += 1,
                Ok(Err((name, error))) => failures.push(format!("{name}：{error}")),
                Err(_) => failures.push("发送任务异常".to_string()),
            }
        }
        self.sent_count.fetch_add(sent as u64, Ordering::Relaxed);
        if sent > 0 {
            self.clear_send_error();
            self.set_event("sent", format!("已发送到 {sent} 台设备{limit_note}"), "");
        }
        if !failures.is_empty() {
            self.set_error(failures.join("；"));
        }
    }
}

async fn send_target(inner: Arc<Inner>, target: &Target, content: &ClipboardContent) -> CoreResult<()> {
    let mut last_error: Option<CoreError> = None;
    if let Some(addr) = target.lan {
        match send_lan(&inner, addr, content).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    if let Some(addr) = &target.wan {
        match send_wan(&inner, addr.clone(), content).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        CoreError::execution("CLIPBOARD_NO_ROUTE", format!("没有可用线路：{}", target.name))
    }))
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

async fn send_wan(
    inner: &Inner,
    addr: EndpointAddr,
    content: &ClipboardContent,
) -> CoreResult<()> {
    let endpoint = inner
        .endpoint
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .ok_or_else(|| CoreError::execution("CLIPBOARD_WAN_OFFLINE", "广域网端点未就绪"))?;
    let connection = tokio::time::timeout(Duration::from_secs(20), endpoint.connect(addr, ALPN))
        .await
        .map_err(|_| CoreError::execution("CLIPBOARD_TIMEOUT", "广域网拨号超时"))?
        .map_err(|error| CoreError::io(format!("广域网拨号失败：{error:?}")))?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| CoreError::io(format!("广域网流打开失败：{error:?}")))?;
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
        return Ok(0);
    }
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
    let (text_limit, image_limit, files_limit) = limit_bytes(&inner.settings.limits);
    let hard_cap = files_limit.max(image_limit).max(text_limit).saturating_mul(2);
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
    // HTML 当文件处理（v0.9.0 bugfix 4.1）：新对端的 html_size 恒为 0；老对端仍可能
    // 发 html 段，收到就落盘成一个 .html 文件进 files 通道——类型勾选里已经没有
    // HTML 这一档，它归「文件」管（大小也吃 files 的上限）。
    let accept_html_as_file = types.files && header.html_size > 0 && header.html_size <= files_limit;
    let accept_image = types.image && header.image_size > 0 && header.image_size <= image_limit;
    let files_total: u64 = header.files.iter().map(|file| file.size).sum();
    let accept_files = types.files && !header.files.is_empty() && files_total <= files_limit;
    // 被类型/上限挡掉的段落要拼进「收到」事件（v0.9.1）：之前图片超限被丢时，
    // 只要文本还在就照常显示「收到：文本」，用户完全不知道图被舍弃了。
    let mut dropped: Vec<&'static str> = Vec::new();
    if header.text_size > 0 && !accept_text {
        dropped.push(if types.text { "文本超上限" } else { "文本类型未开启" });
    }
    if header.image_size > 0 && !accept_image {
        dropped.push(if types.image { "图片超上限" } else { "图片类型未开启" });
    }
    if !header.files.is_empty() && !accept_files {
        dropped.push(if types.files { "文件超上限" } else { "文件类型未开启" });
    }
    if header.html_size > 0 && !accept_html_as_file {
        dropped.push(if types.files { "HTML 超上限" } else { "文件类型未开启（HTML 计入文件）" });
    }

    let mut content = ClipboardContent::default();
    let mut files: Vec<ClipboardFile> = Vec::new();
    if accept_files || accept_html_as_file {
        tokio::fs::create_dir_all(&inner.save_dir)
            .await
            .map_err(|error| CoreError::io(format!("创建保存目录失败：{error}")))?;
    }
    let text_bytes = read_blob(reader, session, header.text_size, accept_text).await?;
    if let Some(bytes) = text_bytes {
        content.text = String::from_utf8_lossy(&bytes).to_string();
    }
    let html_bytes = read_blob(reader, session, header.html_size, accept_html_as_file).await?;
    if let Some(bytes) = html_bytes {
        if !bytes.is_empty() {
            let target = unique_target(&inner.save_dir, &sanitize_file_name("clipboard.html")).await?;
            tokio::fs::write(&target, &bytes)
                .await
                .map_err(|error| CoreError::io(format!("写入接收文件失败：{error}")))?;
            let name = target
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "clipboard.html".to_string());
            files.push(ClipboardFile {
                name,
                path: Some(target),
                bytes: None,
            });
        }
    }
    let image_bytes = read_blob(reader, session, header.image_size, accept_image).await?;
    if let Some(bytes) = image_bytes {
        content.image = Some(bytes);
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
    inner.apply_remote(content, &session.peer_name, &dropped.join("、")).await?;
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
    /// `dropped` 是被本机类型/上限挡掉的段落说明（可为空串），拼进「收到」事件，
    /// 保证任何一段被舍弃都对用户可见。
    async fn apply_remote(&self, content: ClipboardContent, peer_name: &str, dropped: &str) -> CoreResult<()> {
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
        let detail = describe_content(&content);
        let detail = if dropped.is_empty() {
            detail
        } else {
            format!("{detail}（舍弃：{dropped}）")
        };
        if let Some(backend) = self.backend.as_ref() {
            if backend.clipboard_available() {
                backend.clipboard_write(&content)?;
                self.received_count.fetch_add(1, Ordering::Relaxed);
                self.set_event("received", detail, peer_name.to_string());
                return Ok(());
            }
        }
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
                "imagePath": image_path,
                "files": files,
            },
            "peer": peer_name,
        }));
        self.received_count.fetch_add(1, Ordering::Relaxed);
        self.set_event("received", detail, peer_name.to_string());
        Ok(())
    }
}

fn describe_content(content: &ClipboardContent) -> String {
    let mut parts = Vec::new();
    if !content.text.is_empty() {
        parts.push("文本");
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
// 广域网：iroh 端点 + 统一服务器 + P2P 房间
// ---------------------------------------------------------------------------

fn clipboard_room(account: &str, password: &str) -> CoreResult<SecretKey> {
    let keys = derive_keys(account, password)?;
    let mut hasher = Sha256::new();
    hasher.update(b"kxtodo-clipboard/2/room\0");
    hasher.update(account.trim().to_lowercase().as_bytes());
    hasher.update(b"\0");
    hasher.update(keys.dir_key);
    Ok(SecretKey::from_bytes(&hasher.finalize().into()))
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
        .map_err(|error| CoreError::io(format!("剪切板广域网端点启动失败：{error:?}")))
}

async fn wan_loop(inner: Arc<Inner>) {
    let account = inner.settings.account.clone();
    let password = inner.settings.password.clone();
    let keys = match tokio::task::spawn_blocking(move || derive_keys(&account, &password)).await {
        Ok(Ok(keys)) => keys,
        Ok(Err(error)) => {
            inner.set_endpoint_error(format!("广域网账户密钥派生失败：{}", error.message));
            return;
        }
        Err(_) => {
            inner.set_endpoint_error("广域网账户密钥派生任务异常");
            return;
        }
    };
    let room = match clipboard_room(&inner.settings.account, &inner.settings.password) {
        Ok(room) => room,
        Err(error) => {
            inner.set_endpoint_error(error.message);
            return;
        }
    };
    let relay = inner.net.relay.clone();
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            return;
        }
        match bind_endpoint(&inner.secret, relay.as_deref()).await {
            Ok(endpoint) => {
                if let Ok(mut slot) = inner.endpoint.lock() {
                    *slot = Some(endpoint.clone());
                }
                inner.set_endpoint_error("");
                let accept_task = {
                    let task = inner.clone();
                    let endpoint = endpoint.clone();
                    tokio::spawn(async move {
                        accept_loop(task, endpoint).await;
                    })
                };
                let mut extra = Vec::new();
                if inner.settings.server_enabled {
                    let task = inner.clone();
                    let endpoint = endpoint.clone();
                    let auth_key = keys.auth_key;
                    extra.push(tokio::spawn(async move {
                        server_presence_loop(task, endpoint, auth_key).await;
                    }));
                }
                if inner.settings.p2p_enabled {
                    let task = inner.clone();
                    let endpoint = endpoint.clone();
                    let room = room.clone();
                    extra.push(tokio::spawn(async move {
                        p2p_discovery_loop(task, endpoint, room).await;
                    }));
                }
                tokio::select! {
                    _ = inner.notify.notified() => {}
                }
                accept_task.abort();
                for task in extra {
                    task.abort();
                }
                if let Ok(mut slot) = inner.endpoint.lock() {
                    *slot = None;
                }
                let _ = tokio::time::timeout(Duration::from_secs(3), endpoint.close()).await;
                return;
            }
            Err(error) => {
                inner.set_endpoint_error(error.message);
                tokio::select! {
                    _ = inner.notify.notified() => return,
                    _ = tokio::time::sleep(Duration::from_secs(12)) => {}
                }
            }
        }
    }
}

async fn accept_loop(inner: Arc<Inner>, endpoint: Endpoint) {
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        let incoming = tokio::select! {
            _ = inner.notify.notified() => break,
            incoming = endpoint.accept() => incoming,
        };
        let Some(incoming) = incoming else { break };
        let Ok(accepting) = incoming.accept() else { continue };
        let Ok(connection) = accepting.await else { continue };
        let task = inner.clone();
        inner_runtime_spawn(async move {
            let Ok((mut send, mut recv)) = connection.accept_bi().await else { return };
            let _ = tokio::time::timeout(Duration::from_secs(300), async {
                if let Err(error) = serve_stream(&task, &mut recv, &mut send).await {
                    let _ = write_json(&mut send, &json!({ "kind": "error", "message": error.message })).await;
                }
            })
            .await;
            // 等发送方读走最后一个 ack 再释放 Connection（QUIC 流随 Connection drop 重置）。
            let _ = tokio::time::timeout(Duration::from_secs(2), connection.closed()).await;
        });
    }
}

fn presence_body(inner: &Inner, endpoint: &Endpoint) -> PresenceBody {
    PresenceBody {
        device_id: inner.device_id.clone(),
        name: inner.name.clone(),
        endpoint_id: endpoint.id().to_z32(),
        addrs: direct_addrs(endpoint).iter().map(ToString::to_string).collect(),
    }
}

fn is_auth_error(error: &CoreError) -> bool {
    error.code == "UNAUTHORIZED" || error.code == "SYNC_HTTP_401" || error.code == "CHALLENGE_INVALID"
}

fn server_step(
    client: &SyncClient,
    account: &str,
    auth_key: &[u8; 32],
    token: &mut Option<String>,
    body: &PresenceBody,
) -> CoreResult<Vec<ServerPeer>> {
    for attempt in 0..2 {
        if token.is_none() {
            // 账户可能已存在：先 register 再 login，register 的「已注册」冲突直接忽略。
            let _ = client.register(account, auth_key);
            let (fresh, _, _) = client.login(account, auth_key)?;
            *token = Some(fresh);
        }
        let current = token.clone().unwrap_or_default();
        let payload = serde_json::to_value(body)?;
        match client
            .clipboard_presence(&current, &payload)
            .and_then(|_| client.clipboard_peers(&current, &body.device_id))
        {
            Ok(value) => {
                let parsed: ServerPeersResponse = serde_json::from_value(value).map_err(|error| {
                    CoreError::io(format!("统一服务器设备列表格式不对：{error}"))
                })?;
                return Ok(parsed.peers);
            }
            Err(error) if attempt == 0 && is_auth_error(&error) => {
                *token = None;
            }
            Err(error) => return Err(error),
        }
    }
    Err(CoreError::io("统一服务器认证失败"))
}

fn server_entries(peers: Vec<ServerPeer>) -> Vec<(String, String, EndpointAddr)> {
    peers
        .into_iter()
        .filter_map(|peer| {
            let id = EndpointId::from_z32(&peer.endpoint_id).ok()?;
            let addrs: Vec<TransportAddr> = peer
                .addrs
                .iter()
                .filter_map(|raw| raw.parse::<SocketAddr>().ok())
                .map(TransportAddr::Ip)
                .collect();
            let addr = if addrs.is_empty() {
                EndpointAddr::new(id)
            } else {
                EndpointAddr {
                    id,
                    addrs: addrs.into_iter().collect(),
                }
            };
            Some((peer.device_id, peer.name, addr))
        })
        .collect()
}

async fn server_presence_loop(inner: Arc<Inner>, endpoint: Endpoint, auth_key: [u8; 32]) {
    let Ok(client) = SyncClient::new_with_timeout(&inner.net.server_url, 10) else {
        inner.set_server_error("统一服务器地址不合法");
        return;
    };
    let client = Arc::new(client);
    let account = inner.settings.account.clone();
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        let body = presence_body(&inner, &endpoint);
        let task_inner = inner.clone();
        let task_client = client.clone();
        let task_account = account.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let mut token = task_inner.server_token.lock().ok().and_then(|slot| slot.clone());
            let result = server_step(&task_client, &task_account, &auth_key, &mut token, &body);
            if let Ok(mut slot) = task_inner.server_token.lock() {
                *slot = token;
            }
            result
        })
        .await;
        match outcome {
            Ok(Ok(peers)) => {
                inner.apply_wan_source(WanSource::Server, server_entries(peers));
                inner.server_ok.store(true, Ordering::Relaxed);
                inner.set_server_error("");
            }
            Ok(Err(error)) => {
                inner.server_ok.store(false, Ordering::Relaxed);
                inner.set_server_error(format!("统一服务器：{}", error.message));
            }
            Err(_) => {
                inner.server_ok.store(false, Ordering::Relaxed);
                inner.set_server_error("统一服务器请求任务异常");
            }
        }
        if wait_tick(&inner, WAN_TICK).await {
            break;
        }
    }
    inner.server_ok.store(false, Ordering::Relaxed);
}

async fn wait_tick(inner: &Inner, duration: Duration) -> bool {
    // true = 收到停止信号。刷新只提前结束等待，不视为停止。
    tokio::select! {
        _ = inner.notify.notified() => true,
        _ = inner.refresh.notified() => false,
        _ = tokio::time::sleep(duration) => false,
    }
}

async fn p2p_discovery_loop(inner: Arc<Inner>, endpoint: Endpoint, room: SecretKey) {
    let Ok(client) = directory::build_client(&endpoint, &inner.net.directory_url) else {
        inner.set_p2p_error("P2P 目录客户端初始化失败");
        return;
    };
    let client = Arc::new(client);
    let self_id = endpoint.id();
    // 不能用 `Instant::now() - 20s`：设备刚开机几秒就启动服务时单调时钟会下溢 panic。
    let mut last_publish: Option<Instant> = None;
    let mut names: HashMap<EndpointId, (String, Instant)> = HashMap::new();
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        let mut published_ok = true;
        if last_publish.map_or(true, |at| at.elapsed() >= Duration::from_secs(REPUBLISH_SECS)) {
            let result = tokio::time::timeout(Duration::from_secs(15), async {
                // 名字记录必须用**设备私钥**签：pkarr 一个 key 只有一份包，
                // 拿房间私钥发 _name 会把同一份包里的房间设备目录整个覆盖掉。
                let _ = directory::publish_name(&client, &inner.secret, &inner.name).await;
                directory::publish(&client, &room, self_id, &direct_addrs(&endpoint)).await
            })
            .await;
            match result {
                Ok(Ok(())) => {
                    last_publish = Some(Instant::now());
                }
                Ok(Err(error)) => {
                    published_ok = false;
                    inner.set_p2p_error(format!("P2P 目录发布失败：{}", error.message));
                }
                Err(_) => {
                    published_ok = false;
                    inner.set_p2p_error("P2P 目录发布超时");
                }
            }
        }
        let fetch_client = client.clone();
        let room_id = room.public();
        let fetched = tokio::time::timeout(
            Duration::from_secs(15),
            directory::fetch_checked(&fetch_client, room_id),
        )
        .await;
        match fetched {
            Ok(Ok(entries)) => {
                let mut resolved = Vec::new();
                for entry in entries {
                    if entry.id == self_id
                        || directory::now_unix().saturating_sub(entry.published_at) > DIRECTORY_FRESH_SECS
                    {
                        continue;
                    }
                    let name = match names.get(&entry.id) {
                        Some((name, at)) if at.elapsed() < PEER_NAME_FRESH => name.clone(),
                        _ => {
                            let name = tokio::time::timeout(
                                Duration::from_secs(5),
                                directory::fetch_name(&fetch_client, entry.id),
                            )
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or_default();
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
                    resolved.push((entry.id.to_z32(), name, addr));
                }
                inner.apply_wan_source(WanSource::P2p, resolved);
                inner.p2p_ok.store(published_ok, Ordering::Relaxed);
                if published_ok {
                    inner.set_p2p_error("");
                }
            }
            Ok(Err(error)) => {
                inner.p2p_ok.store(false, Ordering::Relaxed);
                inner.set_p2p_error(format!("P2P 目录解析失败：{}", error.message));
            }
            Err(_) => {
                inner.p2p_ok.store(false, Ordering::Relaxed);
                inner.set_p2p_error("P2P 目录解析超时");
            }
        }
        if wait_tick(&inner, WAN_TICK).await {
            break;
        }
    }
    inner.p2p_ok.store(false, Ordering::Relaxed);
    let _ = tokio::time::timeout(
        Duration::from_secs(5),
        directory::unpublish(&client, &room, self_id),
    )
    .await;
}

#[cfg(test)]
mod tests;

//! 同步运行时状态（`<data>/runtime/`，0600）：
//! - `sync.json`：设备身份、登录 token、拉取水位、逐实体推送水位、主机身份与局域网端点缓存
//! - `sync-host.json`：本机内置主机的运行描述（宿主进程写，core 只读）
//!
//! 都不属于三个 domain 文件。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::CoreResult;
use crate::repo::{atomic_write, Layout};

pub const SYNC_STATE_FILE: &str = "sync.json";

/// 每个实体的服务端版本与本地已对账版本戳。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushedEntry {
    /// 服务端当前版本号（OCC 基线；409 时刷新）
    pub seq: u64,
    /// 本地最后一次与服务端对账的实体 updatedAt
    pub u: String,
    /// 该版本的来源设备（同毫秒冲突的稳定 tiebreak）
    pub by: String,
}

/// 局域网主机的地址缓存。
///
/// 主机在局域网里的**身份是名字**（用户从「发现」列表里点选的那台），ip:port 只是
/// 加速用的缓存：路由器给它换了 IP、或它的端口因占用自动上移，都能靠名字重新发现。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanEndpoint {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// 该主机的库身份，用来发现「还是那台主机，但库被重建了」
    #[serde(default)]
    pub instance_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStateFile {
    pub version: u32,
    pub device_id: String,
    pub token: String,
    pub token_expires_at: Option<String>,
    pub last_pulled_seq: u64,
    /// 图片 blob 流的拉取水位（与实体流共用服务端计数器，各自独立推进）
    #[serde(default)]
    pub last_pulled_image_seq: u64,
    /// 上一次同步的范围签名（`data|settings|schedules`）：范围一变就把两条水位归零重拉，
    /// 因为增量流是按范围过滤的，水位之下的记录不会再出现一次。
    #[serde(default)]
    pub scope_signature: String,
    /// 上一次连到的主机库身份（`/healthz` 的 `instanceId`）。
    ///
    /// 与本轮解析出来的不一致 = 换了主机，或主机的库被重建 → 必须把两条拉取水位、
    /// 推送台账与 token 全部清零重新播种：新库的 `current_seq` 从 1 开始而本地水位
    /// 停在几百，否则表现是**静默地**什么都拉不到、推的时候一路 OCC 409。
    #[serde(default)]
    pub server_instance_id: String,
    /// 局域网端点缓存：上次连上的主机地址。身份是**名字**，ip:port 只是缓存
    /// （名字对得上就直连，省去每轮广播；对不上再重新发现）。
    #[serde(default)]
    pub lan_endpoint: Option<LanEndpoint>,
    #[serde(default)]
    pub pushed: Map<String, Value>,
    pub last_sync_at: Option<String>,
    pub last_result: Option<Value>,
    /// 最近一次与服务端通信的结果缓存：设置面板据此显示 🟢/🔴，不做阻塞探测
    #[serde(default)]
    pub server_online: Option<bool>,
    /// 最近一次成功通信时间
    #[serde(default)]
    pub last_seen_at: Option<String>,
    /// 最近一次失败原因（掉线时展示）
    #[serde(default)]
    pub last_error: Option<String>,
}

impl SyncStateFile {
    pub fn fresh(device_id: String) -> Self {
        Self {
            version: 1,
            device_id,
            token: String::new(),
            token_expires_at: None,
            last_pulled_seq: 0,
            last_pulled_image_seq: 0,
            scope_signature: String::new(),
            server_instance_id: String::new(),
            lan_endpoint: None,
            pushed: Map::new(),
            last_sync_at: None,
            last_result: None,
            server_online: None,
            last_seen_at: None,
            last_error: None,
        }
    }

    pub fn entry(&self, id: &str) -> Option<PushedEntry> {
        self.pushed
            .get(id)
            .and_then(|value| serde_json::from_value::<PushedEntry>(value.clone()).ok())
    }

    pub fn set_entry(&mut self, id: &str, entry: PushedEntry) {
        if let Ok(value) = serde_json::to_value(entry) {
            self.pushed.insert(id.to_string(), value);
        }
    }

    pub fn set_entry_seq(&mut self, id: &str, seq: u64) {
        if let Some(existing) = self.pushed.get_mut(id) {
            if let Some(map) = existing.as_object_mut() {
                map.insert("seq".to_string(), Value::from(seq));
            }
        }
    }
}

pub fn state_path(layout: &Layout) -> std::path::PathBuf {
    layout.runtime_dir().join(SYNC_STATE_FILE)
}

pub fn load_state(layout: &Layout) -> SyncStateFile {
    let path = state_path(layout);
    let Ok(raw) = fs::read_to_string(&path) else {
        return SyncStateFile::fresh(crate::ids::gen_device_id());
    };
    serde_json::from_str(&raw).unwrap_or_else(|_| SyncStateFile::fresh(crate::ids::gen_device_id()))
}

pub fn save_state(layout: &Layout, state: &SyncStateFile) -> CoreResult<()> {
    fs::create_dir_all(layout.runtime_dir())?;
    let path = state_path(layout);
    let raw = serde_json::to_string_pretty(state)?;
    atomic_write(&path, &raw)?;
    restrict_permissions(&path);
    Ok(())
}

pub fn clear_state(layout: &Layout) -> CoreResult<()> {
    let path = state_path(layout);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 内置主机描述符（runtime/sync-host.json）
// ---------------------------------------------------------------------------

pub const SYNC_HOST_FILE: &str = "sync-host.json";

/// 内置主机（GUI/APK 进程内跑的 kxtodo-server）的运行描述。
///
/// 由**宿主进程写、core 只读**：只有宿主知道实际绑定到的端口（配置端口被占用时会
/// 自动上移）与库身份。这样 GUI 进程与另开的 CLI 进程都能解析出「本机就是主机」时
/// 该连哪儿——与 `runtime/host.json`（IPC 描述符）是同一个套路。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedHostState {
    /// 内置服务器是否在跑
    pub running: bool,
    /// 实际监听端口（永远是真实值，不是配置值）
    pub port: u16,
    /// 启动时配置的端口。与实际端口分开记，是为了让「用户改了端口」与
    /// 「端口被占用自动上移」能区分开——只比实际值会每轮都判定为需要重启。
    #[serde(default)]
    pub configured_port: u16,
    /// 展示名 = 这台主机在局域网里的身份
    pub name: String,
    /// 数据库身份：客户端据此判断「还是那台主机，但库被重建了」
    pub instance_id: String,
    /// 管理台地址（本机访问）
    pub admin_url: String,
    /// 管理台账号
    pub admin_user: String,
    /// 管理台密码。首次启用内置主机时自动生成，之后一直沿用。
    ///
    /// 明文落盘是刻意的：与 `settings.json` 里的同步密码、`sync-history.json` 里的
    /// 配对历史同级——都是「只存本机、0600、不上网、不参与同步」的凭据。不落盘的话
    /// 应用重启后谁都拿不回它（settings.json 里只有哈希），用户就进不去自己的管理台。
    pub admin_password: String,
    pub started_at: Option<String>,
    /// 宿主进程 pid（排查用；不用于判活——判活靠 /healthz，stale 文件自然会被识破）
    pub pid: u32,
    /// 勾选了「本机作为服务器」但没起来时的原因（UI 要能说出来）
    pub last_error: Option<String>,
}

pub fn host_state_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join(SYNC_HOST_FILE)
}

/// 读内置主机描述符；文件不存在或损坏都按「没在跑」处理（绝不因为描述符坏掉阻塞同步）。
pub fn load_host_state(layout: &Layout) -> EmbeddedHostState {
    let Ok(raw) = fs::read_to_string(host_state_path(layout)) else {
        return EmbeddedHostState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_host_state(layout: &Layout, state: &EmbeddedHostState) -> CoreResult<()> {
    fs::create_dir_all(layout.runtime_dir())?;
    let path = host_state_path(layout);
    let raw = serde_json::to_string_pretty(state)?;
    atomic_write(&path, &raw)?;
    restrict_permissions(&path);
    Ok(())
}

pub fn clear_host_state(layout: &Layout) -> CoreResult<()> {
    let path = host_state_path(layout);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
pub(crate) fn restrict_permissions(_path: &Path) {}

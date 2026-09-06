//! 端点解析：三种通信方式共用同一套 HTTP 协议与同一套合并逻辑，
//! 差别只在「**这一轮连哪儿**」——这个模块就只负责回答这一个问题。
//!
//! - **自建服务**：用户填的 `serverUrl`
//! - **局域网**：本机是主机就连自己的 localhost；否则按**名字**找选定的那台主机
//!   （先试缓存地址，对不上再退到 UDP 广播发现）
//! - **P2P**：v0.6.1（iroh 隧道里跑临时主机），这里先给出明确错误
//!
//! 解析结果里带主机的 `instance_id`：调用方（`engine`）据此判断主机库有没有换过，
//! 换过就必须把同步水位与推送台账清零重新播种——否则新库的 seq 从 1 开始而本地
//! 水位停在几百，表现是**静默地**什么都拉不到、推的时候一路 OCC 409。

use std::time::Duration;

use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::model::{SyncMode, SyncSettings};
use crate::repo::Layout;
use crate::sync::discovery::{discover, probe_health_with_timeout};
use crate::sync::state::{load_host_state, LanEndpoint, SyncStateFile};
use crate::sync::transport::normalize_base_url;

/// 局域网发现的等待时间（与 CLI `sync discover` 的默认值一致）
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(2500);
/// 自建服务可能在公网另一端，探测给宽一点
const CONFIGURED_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 局域网主机就在同一个网段，探不通就是真不通
const LAN_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// P2P 隧道刚建好，对端内置服务器的 /healthz 多给一点余量
const P2P_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// 本轮同步实际要连的端点。
#[derive(Debug, Clone)]
pub struct Resolved {
    /// HTTP base url，可直接喂给 `SyncClient::new`
    pub base_url: String,
    /// 主机库身份；拿不到就是空串（调用方据此跳过 epoch 判断）
    pub instance_id: String,
    /// 主机展示名
    pub name: String,
    /// 主机版本
    pub version: String,
    /// `/healthz` 原始响应（`sync probe` 直接复用，不必再探一次）
    pub health: Value,
    /// 解析途径（写进日志与错误信息，排查「为什么连上了这台」）
    pub source: EndpointSource,
    /// 需要写回 `runtime/sync.json` 的局域网端点缓存（其它模式为 None）
    pub lan_endpoint: Option<LanEndpoint>,
}

impl Resolved {
    /// 图片清单缓存的分区键：优先用主机库身份（换 IP 不变、换库才变，正是我们想要的
    /// 失效时机），拿不到才退回 base url。
    pub fn scope_key(&self) -> String {
        if self.instance_id.is_empty() {
            self.base_url.clone()
        } else {
            self.instance_id.clone()
        }
    }
}

/// 解析结果 + P2P 隧道的生命周期句柄。
///
/// P2P 的 base_url 是一条**本地临时隧道**（iroh 双向流 ↔ 对端内置服务器）：
/// 隧道一关地址就作废，所以调用方必须把 `tunnel` 活到本轮同步结束。
/// 其它通信方式没有隧道，字段恒为 None。
pub struct Resolution {
    pub resolved: Resolved,
    pub tunnel: Option<crate::sync::p2p::Tunnel>,
}

impl std::fmt::Debug for Resolution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resolution")
            .field("resolved", &self.resolved)
            .field("tunnel", &self.tunnel.is_some())
            .finish()
    }
}

/// 端点是怎么解析出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointSource {
    /// 自建服务：用户手填的地址
    Configured,
    /// 本机就是主机（内置 server 的 localhost；局域网勾选主机或 P2P 自己是枢纽）
    Embedded,
    /// 局域网：命中上次连过的地址缓存
    Cache,
    /// 局域网：这轮广播发现找到的
    Discovery,
    /// P2P：拨号目录里的枢纽设备，HTTP 走 iroh 隧道
    P2p,
}

impl EndpointSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            EndpointSource::Configured => "configured",
            EndpointSource::Embedded => "embedded",
            EndpointSource::Cache => "cache",
            EndpointSource::Discovery => "discovery",
            EndpointSource::P2p => "p2p",
        }
    }
}

/// 解析本轮同步要连的端点。失败一律返回 `ErrorKind::Io`（网络类）或明确的配置类错误，
/// 前者会被 `engine::run_sync` 记成掉线并按 `reconnectSeconds` 静默重连。
pub fn resolve(
    layout: &Layout,
    sync: &SyncSettings,
    state: &SyncStateFile,
) -> CoreResult<Resolution> {
    match sync.effective_mode() {
        SyncMode::Server => resolve_configured(sync),
        SyncMode::Lan => resolve_lan(layout, sync, state),
        SyncMode::P2p => resolve_p2p(layout, sync),
    }
}

fn plain(resolved: Resolved) -> Resolution {
    Resolution {
        resolved,
        tunnel: None,
    }
}

fn resolve_configured(sync: &SyncSettings) -> CoreResult<Resolution> {
    let base_url = normalize_base_url(&sync.server_url)?;
    let health = probe_health_with_timeout(&base_url, CONFIGURED_PROBE_TIMEOUT)?;
    Ok(plain(from_health(
        base_url,
        health,
        EndpointSource::Configured,
        None,
    )))
}

fn resolve_lan(
    layout: &Layout,
    sync: &SyncSettings,
    state: &SyncStateFile,
) -> CoreResult<Resolution> {
    // 本机就是主机：连自己的内置服务器（每轮只连一个 endpoint，自己是主机时就是 localhost）
    if sync.lan_host {
        let host = load_host_state(layout);
        if !host.running || host.port == 0 {
            let reason = host
                .last_error
                .as_deref()
                .filter(|message| !message.is_empty())
                .map(|message| format!("（上次启动失败：{message}）"))
                .unwrap_or_default();
            return Err(CoreError::conflict(
                "SYNC_HOST_NOT_RUNNING",
                format!(
                    "本机被设为局域网主机，但内置服务器没在运行{reason}。\
                     启动 KXToDo 应用即可；已在运行的话看设置页的主机状态。"
                ),
            ));
        }
        let base_url = format!("http://127.0.0.1:{}", host.port);
        let health = probe_health_with_timeout(&base_url, LAN_PROBE_TIMEOUT).map_err(|_| {
            CoreError::io(format!(
                "连不上本机的内置服务器（{base_url}）：描述符说在跑但探测失败，\
                 可能是上次异常退出留下的 runtime/sync-host.json"
            ))
        })?;
        return Ok(plain(from_health(
            base_url,
            health,
            EndpointSource::Embedded,
            None,
        )));
    }

    let wanted = sync.lan_peer.trim();
    if wanted.is_empty() {
        return Err(CoreError::conflict(
            "SYNC_LAN_HOST_NOT_SELECTED",
            "还没选定局域网主机：设置 → 数据同步 → 服务器地址右侧的放大镜（发现）→ 点选一台"
                .to_string(),
        ));
    }

    // 1. 缓存直连：地址还通、且 /healthz 报的名字仍然是选定的那台，就不必再广播一轮
    if let Some(cached) = state.lan_endpoint.as_ref() {
        if names_match(&cached.name, wanted) && !cached.host.is_empty() && cached.port > 0 {
            let base_url = format!("http://{}:{}", cached.host, cached.port);
            if let Ok(health) = probe_health_with_timeout(&base_url, LAN_PROBE_TIMEOUT) {
                if names_match(health_name(&health), wanted) {
                    return Ok(plain(from_health(
                        base_url,
                        health,
                        EndpointSource::Cache,
                        Some(cached.clone()),
                    )));
                }
            }
        }
    }

    // 2. 缓存失效（主机换了 IP / 换了端口 / 根本不是同一台）→ 重新广播发现，按名字精确匹配
    //
    // 广播本身失败（无可用网卡、容器里禁了 UDP、权限问题）按「没发现」处理：
    // 用户要听的是「找不到那台主机」，不是一个看不懂的 socket 错误；
    // 原因塞进消息里，排查时还在。
    let mut discovery_error: Option<String> = None;
    let servers = discover(DISCOVERY_TIMEOUT).unwrap_or_else(|error| {
        discovery_error = Some(error.message);
        Vec::new()
    });
    let Some(hit) = servers
        .iter()
        .find(|server| server.verified && names_match(&server.name, wanted))
    else {
        let seen: Vec<String> = servers
            .iter()
            .map(|server| server.name.clone())
            .filter(|name| !name.is_empty())
            .collect();
        let detail = if let Some(reason) = &discovery_error {
            format!("（广播发现失败：{reason}）")
        } else if seen.is_empty() {
            "（这次发现没有任何主机应答）".to_string()
        } else {
            format!("（发现的主机：{}）", seen.join("、"))
        };
        return Err(CoreError::new(
            crate::error::ErrorKind::Io,
            "SYNC_LAN_HOST_NOT_FOUND",
            format!(
                "局域网里找不到主机「{wanted}」{detail}。\
                 它可能没开机、没勾选「本机作为服务器」，或者跟本机不在同一个网段。"
            ),
        ));
    };
    let health = probe_health_with_timeout(&hit.url, LAN_PROBE_TIMEOUT)?;
    let endpoint = LanEndpoint {
        name: wanted.to_string(),
        host: hit.host.clone(),
        port: hit.port,
        instance_id: hit.instance_id.clone().unwrap_or_default(),
    };
    Ok(plain(from_health(
        hit.url.clone(),
        health,
        EndpointSource::Discovery,
        Some(endpoint),
    )))
}

/// P2P：枢纽规则——目录在线设备（含自己）里 **EndpointId 最小**的那台当枢纽，
/// 每轮只连一个端点。自己是枢纽就连自己的内置库（回环）；否则拨号枢纽，
/// 在 iroh 隧道里对它做一次完整的普通 HTTP 同步（被叫方把隧道接进自己的内置服务器）。
///
/// 枢纽下线时：拨号失败进 5 分钟冷却，本轮立刻试下一台；目录条目 15 分钟过期兜底。
/// 换枢纽 = 换主机库 = engine 的换档逻辑把水位/台账清零重新播种。
fn resolve_p2p(layout: &Layout, sync: &SyncSettings) -> CoreResult<Resolution> {
    let runtime = match crate::sync::p2p::current_for(layout) {
        Some(runtime) => runtime,
        // 配对发生在设置落盘**之前**（凭据还在请求里），等 Settings 域事件再启运行时
        // 就鸡生蛋了；所以这里按「将要生效」的设置就地起一个。常驻进程里 reconcile
        // 随后的 start 是幂等复用，CLI 单进程也因此能用 P2P。
        None => {
            let keys = crate::sync::crypto::derive_keys(&sync.username, &sync.secret)?;
            crate::sync::p2p::start(crate::sync::p2p::P2pConfig {
                layout: layout.clone(),
                keys,
                relay: if sync.p2p_relay.trim().is_empty() {
                    None
                } else {
                    Some(sync.p2p_relay.trim().to_string())
                },
                directory_url: sync.p2p_directory.trim().to_string(),
                serve: true,
            })?
        }
    };
    let entries = runtime.directory()?;
    let entry_count = entries.len();
    let self_id = runtime.device_id();
    let mut candidates: Vec<(iroh::EndpointId, Vec<std::net::SocketAddr>)> = entries
        .iter()
        .map(|entry| (entry.id, entry.addrs.clone()))
        .collect();
    candidates.push((self_id, Vec::new()));
    candidates.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    candidates.dedup_by(|left, right| left.0 == right.0);

    let mut last_error = "目录里没有任何设备记录".to_string();
    for (candidate, addrs) in candidates {
        if candidate != self_id && runtime.in_cooldown(candidate) {
            continue;
        }
        if candidate == self_id {
            // 本机是枢纽：连自己的内置库（P2P 模式下它只绑回环）
            let host = load_host_state(layout);
            if host.running && host.port > 0 {
                let base_url = format!("http://127.0.0.1:{}", host.port);
                if let Ok(health) = probe_health_with_timeout(&base_url, LAN_PROBE_TIMEOUT) {
                    return Ok(plain(from_health(
                        base_url,
                        health,
                        EndpointSource::Embedded,
                        None,
                    )));
                }
            }
            last_error = "本机是枢纽，但内置服务器没在跑".to_string();
            continue;
        }
        match runtime.dial(candidate, &addrs) {
            Ok(tunnel) => {
                let base_url = tunnel.base_url.clone();
                match probe_health_with_timeout(&base_url, P2P_PROBE_TIMEOUT) {
                    Ok(health) => {
                        let name = health_string(&health, "name");
                        crate::sync::p2p::identity::record_peer(
                            layout,
                            &candidate.to_z32(),
                            &name,
                            true,
                            None,
                        );
                        return Ok(Resolution {
                            resolved: from_health(base_url, health, EndpointSource::P2p, None),
                            tunnel: Some(tunnel),
                        });
                    }
                    Err(error) => {
                        runtime.note_cooldown(candidate);
                        crate::sync::p2p::identity::record_peer(
                            layout,
                            &candidate.to_z32(),
                            "",
                            false,
                            Some(error.message.clone()),
                        );
                        last_error = error.message;
                    }
                }
            }
            Err(error) => {
                runtime.note_cooldown(candidate);
                crate::sync::p2p::identity::record_peer(
                    layout,
                    &candidate.to_z32(),
                    "",
                    false,
                    Some(error.message.clone()),
                );
                last_error = error.message;
            }
        }
    }
    Err(CoreError::new(
        crate::error::ErrorKind::Io,
        "SYNC_P2P_NO_PEER",
        format!(
            "P2P 这轮找不到可用对端：{last_error}（目录 {entry_count} 条在线记录）。\
             对方不在线是正常情况——P2P 只在两台设备同时在线时同步，会按重连间隔静默重试"
        ),
    ))
}

fn from_health(
    base_url: String,
    health: Value,
    source: EndpointSource,
    lan_endpoint: Option<LanEndpoint>,
) -> Resolved {
    Resolved {
        instance_id: health_string(&health, "instanceId"),
        name: health_string(&health, "name"),
        version: health_string(&health, "version"),
        base_url,
        health,
        source,
        lan_endpoint,
    }
}

fn health_string(health: &Value, key: &str) -> String {
    health
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn health_name(health: &Value) -> &str {
    health.get("name").and_then(Value::as_str).unwrap_or_default()
}

/// 主机名比较：去空白 + ASCII 大小写不敏感（中文名逐字节相同才算同一个）。
pub fn names_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

/// 缺省主机名：机器名（Windows 的 COMPUTERNAME / unix 的 HOSTNAME），拿不到就 `kxtodo`。
///
/// 名字是主机在局域网里的身份，所以第一次勾选「本机作为服务器」时用它做可编辑的初值。
pub fn default_host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "kxtodo".to_string())
}

/// 将要使用的主机名：用户填过的名字，没填就用机器名。
pub fn desired_host_name(configured: &str) -> String {
    if configured.trim().is_empty() {
        default_host_name()
    } else {
        configured.trim().to_string()
    }
}

/// 启用「本机作为服务器」前查重：局域网内已经有同名主机就拒绝。
///
/// 名字是主机在局域网里的**身份**（客户端按名字选定主机），重名会让「连的是哪台」
/// 变成抛硬币。只在「不当主机 → 当主机」的转换时查：此时本机内置服务器还没起，
/// 不会应答自己的广播，因此不需要排除自身。
///
/// 调用方必须**先停掉本机已有的内置服务器再调它**（改名字重启的场景），
/// 否则查到的「同名主机」就是自己。
pub fn ensure_host_name_available(name: &str) -> CoreResult<()> {
    let wanted = name.trim();
    if wanted.is_empty() {
        return Err(CoreError::validation(
            "SYNC_HOST_NAME_REQUIRED",
            "主机名字不能为空（局域网内靠它认出这台主机）".to_string(),
        ));
    }
    // 广播跑不起来（无网卡/容器禁 UDP）就跳过查重：这种环境本来也没法被发现，
    // 为了一个小概率重名把用户挡在门外更糟。
    let Ok(servers) = discover(DISCOVERY_TIMEOUT) else {
        return Ok(());
    };
    if let Some(hit) = servers.iter().find(|server| names_match(&server.name, wanted)) {
        return Err(CoreError::conflict(
            "SYNC_HOST_NAME_TAKEN",
            format!(
                "局域网内已存在同名主机「{wanted}」（{}），换一个名字",
                hit.host
            ),
        ));
    }
    Ok(())
}

/// 配对目标：用户点「开始同步」时表单里的东西。
///
/// 三种通信方式共用一个「开始同步」按钮，所以这里不区分注册与登录——
/// 账户存不存在由服务端回答（见 `engine::pair_device`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PairRequest {
    pub mode: SyncMode,
    /// `mode = server`：用户填的地址
    pub server_url: String,
    /// `mode = lan`：从「发现」列表里选定的主机名（本机就是主机时留空）
    pub lan_peer: String,
    pub username: String,
    pub secret: String,
}

impl PairRequest {
    /// 归一化：去空白、去尾斜杠、用户名小写（与派生盐的口径一致）。
    pub fn normalized(&self) -> Self {
        Self {
            mode: self.mode,
            server_url: self.server_url.trim().trim_end_matches('/').to_string(),
            lan_peer: self.lan_peer.trim().to_string(),
            username: self.username.trim().to_lowercase(),
            secret: self.secret.clone(),
        }
    }

    /// 这个目标在用户眼里叫什么（历史列表、日志、状态栏）。
    pub fn label(&self) -> String {
        match self.mode {
            SyncMode::Lan => {
                if self.lan_peer.trim().is_empty() {
                    "本机（局域网主机）".to_string()
                } else {
                    format!("局域网主机「{}」", self.lan_peer.trim())
                }
            }
            SyncMode::Server => self.server_url.trim().to_string(),
            SyncMode::P2p => "P2P".to_string(),
        }
    }
}

/// 按「**将要**写入的配置」解析端点。
///
/// 配对时设置还没落盘，所以不能直接读 `settings.sync`：拿当前配置做底，
/// 覆盖上请求里的模式/地址/账户，再走一遍正常的解析。`lan_host`/`lan_port`/
/// `lan_name` 保持本机现有值——主机开关是另一条路径（`sync configure --lan-host`）管的。
pub fn resolve_pairing(
    layout: &Layout,
    current: &SyncSettings,
    request: &PairRequest,
    state: &SyncStateFile,
) -> CoreResult<Resolution> {
    let request = request.normalized();
    let mut prospective = current.clone();
    prospective.mode = Some(request.mode);
    prospective.server_url = request.server_url.clone();
    prospective.lan_peer = request.lan_peer.clone();
    prospective.username = request.username.clone();
    prospective.secret = request.secret.clone();
    resolve(layout, &prospective, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_names_match_ignoring_case_and_padding() {
        assert!(names_match("客厅的电脑", "客厅的电脑"));
        assert!(names_match("  Desk-01 ", "desk-01"));
        assert!(!names_match("Desk-01", "Desk-02"));
        // 空名字永远不匹配：否则「没选定主机」会被当成匹配上了随便哪台
        assert!(!names_match("", ""));
        assert!(!names_match("Desk-01", ""));
    }

    #[test]
    fn empty_host_name_is_rejected() {
        let error = ensure_host_name_available("   ").unwrap_err();
        assert_eq!(error.code, "SYNC_HOST_NAME_REQUIRED");
    }

    #[test]
    fn scope_key_prefers_instance_id_over_url() {
        let resolved = Resolved {
            base_url: "http://192.168.1.20:52177".to_string(),
            instance_id: "srv-abc".to_string(),
            name: "客厅".to_string(),
            version: "0.6.0".to_string(),
            health: json!({}),
            source: EndpointSource::Discovery,
            lan_endpoint: None,
        };
        // 主机换 IP 不该让图片清单缓存失效，换库才该
        assert_eq!(resolved.scope_key(), "srv-abc");
        let mut anonymous = Resolved {
            base_url: "http://host:1".to_string(),
            instance_id: String::new(),
            name: String::new(),
            version: String::new(),
            health: json!({}),
            source: EndpointSource::Configured,
            lan_endpoint: None,
        };
        assert_eq!(anonymous.scope_key(), "http://host:1");
        anonymous.instance_id = "srv-x".to_string();
        assert_eq!(anonymous.scope_key(), "srv-x");
    }

    #[test]
    fn lan_mode_without_a_selected_host_explains_what_to_do() {
        let layout = Layout::new(std::path::PathBuf::from("."));
        let mut sync = SyncSettings::default();
        sync.mode = Some(SyncMode::Lan);
        sync.username = "me".to_string();
        sync.secret = "pw".to_string();
        let state = SyncStateFile::fresh("dev".to_string());
        let error = resolve(&layout, &sync, &state).unwrap_err();
        assert_eq!(error.code, "SYNC_LAN_HOST_NOT_SELECTED");
    }

    #[test]
    fn lan_host_mode_without_a_running_server_says_so() {
        let dir = std::env::temp_dir().join(format!("kxtodo-endpoint-test-{}", std::process::id()));
        let layout = Layout::new(dir);
        let mut sync = SyncSettings::default();
        sync.mode = Some(SyncMode::Lan);
        sync.lan_host = true;
        sync.username = "me".to_string();
        sync.secret = "pw".to_string();
        let state = SyncStateFile::fresh("dev".to_string());
        let error = resolve(&layout, &sync, &state).unwrap_err();
        assert_eq!(error.code, "SYNC_HOST_NOT_RUNNING");
    }
}

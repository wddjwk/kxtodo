//! 局域网自动发现：客户端在固定 UDP 端口上广播/组播一次查询，局域网内的
//! kxtodo-server 单播应答自己的展示名与 TCP 端口。
//!
//! 为什么服务端用单播应答：Android 接收组播需要 MulticastLock（额外权限 +
//! 唤醒 Wi-Fi 芯片），而单播回包不需要——客户端只管发广播/组播，收单播即可。

use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::error::CoreResult;

/// 发现端口（UDP）。与服务器默认 TCP 端口同号，便于记忆与防火墙放行。
pub const DISCOVERY_PORT: u16 = 52177;
/// 服务器默认监听端口（TCP）。要用「发现」就必须让服务器监听这个端口。
pub const DEFAULT_SERVER_PORT: u16 = 52177;
/// 组播组（239.0.0.0/8 为本地管理范围，不会跨路由）。
pub const DISCOVERY_MULTICAST_GROUP: &str = "239.255.77.52";
/// 查询报文（定长魔数，服务端只认它）。
pub const DISCOVERY_QUERY: &[u8] = b"KXTODO-DISCOVER-v1";
/// 应答报文里的协议标识。
pub const DISCOVERY_PROTOCOL: &str = "kxtodo-discovery-v1";

/// 发现到的一台服务器。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServer {
    /// 展示名（服务器 `--name`）
    pub name: String,
    /// 应答报文的源 IP
    pub host: String,
    /// 服务器自报的 TCP 端口
    pub port: u16,
    /// 可直接填进「服务器地址」的 URL
    pub url: String,
    /// /healthz 探测通过（地址确实可用）
    pub verified: bool,
    pub version: Option<String>,
    /// 对方数据库的身份（建库时生成、重启不变）。客户端据此判断
    /// 「我连的还是不是同一台主机 / 它的库是不是被重建过」，变了要重置同步水位。
    #[serde(rename = "instanceId")]
    pub instance_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscoveryReply {
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    version: Option<String>,
}

/// 探测局域网内的 kxtodo-server。
///
/// 先发一轮广播 + 组播查询，然后在 `timeout` 内收集单播应答；
/// 每个候选再用短超时 `/healthz` 复核一次（过滤掉非本机服务的杂包）。
pub fn discover(timeout: Duration) -> CoreResult<Vec<DiscoveredServer>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;

    let targets = [
        SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT)),
        SocketAddr::from((
            multicast_group_octets(),
            DISCOVERY_PORT,
        )),
    ];
    // 发两轮：UDP 无重传，首轮常被 Wi-Fi 省电/ARP 未解析吃掉。
    for _ in 0..2 {
        for target in targets {
            let _ = socket.send_to(DISCOVERY_QUERY, target);
        }
    }

    let deadline = Instant::now() + timeout;
    // (host, port) → 应答；BTreeMap 让结果顺序稳定（便于 UI 与测试）
    let mut replies: BTreeMap<(String, u16), DiscoveryReply> = BTreeMap::new();
    let mut buffer = [0u8; 2048];
    while Instant::now() < deadline {
        let (len, from) = match socket.recv_from(&mut buffer) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        };
        let Ok(reply) = serde_json::from_slice::<DiscoveryReply>(&buffer[..len]) else {
            continue;
        };
        if reply.protocol != DISCOVERY_PROTOCOL {
            continue;
        }
        let host = from.ip().to_string();
        let port = reply.port.unwrap_or(DISCOVERY_PORT);
        replies
            .entry((host, port))
            .or_insert_with(|| DiscoveryReply {
                protocol: reply.protocol.clone(),
                name: reply.name.clone(),
                port: Some(port),
                version: reply.version.clone(),
            });
    }

    let mut out = Vec::new();
    for ((host, port), reply) in replies {
        let url = format!("http://{host}:{port}");
        let health = probe_health(&url);
        let (verified, version, health_name, instance_id) = match health {
            Ok(value) => (
                true,
                value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(reply.version.clone()),
                value
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string),
                value
                    .get("instanceId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string),
            ),
            Err(_) => (false, reply.version.clone(), None, None),
        };
        out.push(DiscoveredServer {
            name: health_name.unwrap_or(reply.name).trim().to_string(),
            host,
            port,
            url,
            verified,
            version,
            instance_id,
        });
    }
    // 可用优先，其次按名字/地址排序
    out.sort_by(|a, b| {
        b.verified
            .cmp(&a.verified)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.host.cmp(&b.host))
    });
    Ok(out)
}

fn multicast_group_octets() -> [u8; 4] {
    let mut octets = [239, 255, 77, 52];
    if let Ok(parts) = DISCOVERY_MULTICAST_GROUP
        .split('.')
        .map(|part| part.parse::<u8>())
        .collect::<Result<Vec<u8>, _>>()
    {
        if parts.len() == 4 {
            octets.copy_from_slice(&parts);
        }
    }
    octets
}

/// 短超时 /healthz（3 秒）：发现列表复核与「服务器是否在线」探测共用。
pub fn probe_health(url: &str) -> CoreResult<Value> {
    probe_health_with_timeout(url, Duration::from_secs(3))
}

/// 同上，超时可选：自建服务可能在公网另一端（放宽到 10 秒），局域网主机就在同一个
/// 网段（3 秒探不通就是真不通，别让用户等）。
pub fn probe_health_with_timeout(url: &str, timeout: Duration) -> CoreResult<Value> {
    let base = url.trim().trim_end_matches('/');
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent
        .get(&format!("{base}/healthz"))
        .call()
        .map_err(|error| crate::sync::transport::network_error(error))?;
    response
        .into_json::<Value>()
        .map_err(|error| crate::error::CoreError::io(format!("healthz 响应无效：{error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multicast_group_parses_to_four_octets() {
        assert_eq!(multicast_group_octets(), [239, 255, 77, 52]);
    }

    #[test]
    fn discovery_reply_ignores_foreign_packets() {
        // 非本协议的报文解析后 protocol 不匹配，调用方跳过
        let parsed: Result<DiscoveryReply, _> = serde_json::from_slice(b"{\"protocol\":\"other\"}");
        assert!(parsed.map(|reply| reply.protocol != DISCOVERY_PROTOCOL).unwrap_or(true));
    }
}

//! 账户派生的设备目录（pkarr over HTTPS）。
//!
//! 一条以**账户密钥**（`SyncKeys::dir_key`，HKDF 独立分支）签名的 pkarr TXT 记录，
//! 就是「这个账户现在有哪些设备在线」：每台设备把自己的 EndpointId 读-改-写进去，
//! 任何设备只凭用户名密码就能解析出兄弟设备——零额外配置。
//!
//! 记录格式（DNS 名 `_kxtodo`，每条 TXT ≤255 字节、整包 ≤1000 字节）：
//! - `v=1`
//! - 每设备一条 `d=<z32 EndpointId>.<unix 秒>`；超过 [`ENTRY_STALE_SECS`] 视为离线
//!   （设备退出时不一定能来得及撤条目，靠过期兜底）
//!
//! 并发：同一把密钥的发布按 8 字节微秒时间戳 LWW，撞车返回 **409** →
//! 读-改-写重试（最多 [`PUBLISH_RETRIES`] 次）。
//!
//! 安全：持有用户名密码的人本来就能解密全部数据，目录不新增暴露面；
//! 被动观察者只看到一个公钥与若干 EndpointId，看不到用户名。

use iroh::address_lookup::pkarr::{PkarrRelayClient, N0_DNS_PKARR_RELAY_PROD};
use iroh::{Endpoint, EndpointId, SecretKey};
use iroh_dns::pkarr::SignedPacket;
use url::Url;

use crate::error::{CoreError, CoreResult};
use crate::sync::crypto::SyncKeys;

/// 目录记录的 DNS 名（相对签名公钥的 zone）
pub const DIRECTORY_NAME: &str = "_kxtodo";
/// 默认目录服务：n0 的免费公共 pkarr relay（可换自部署，见 `sync.p2pDirectory`）
pub const DEFAULT_DIRECTORY_URL: &str = N0_DNS_PKARR_RELAY_PROD;
/// 目录条目超过这个年龄视为离线（发布间隔 4 分钟的约 4 倍）
pub const ENTRY_STALE_SECS: u64 = 15 * 60;
/// 常驻主机的发布间隔
pub const PUBLISH_INTERVAL_SECS: u64 = 4 * 60;
/// 409 撞车后的读-改-写重试次数
const PUBLISH_RETRIES: usize = 3;
/// 整包 1000 字节上限下最多容纳的设备数：带两个直连地址时每条 DNS 记录约 120 字节，
/// 8 条实测 1008 字节超包，6 条约 760 字节留足余量（目录只记「现在在线」，6 台足够）
const MAX_ENTRIES: usize = 6;
/// 每台设备最多带两个直连地址（回环 + 一个私网）
const MAX_ADDRS_PER_ENTRY: usize = 2;

/// 目录里的一台设备。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub id: EndpointId,
    pub published_at: u64,
    /// 发布方的直连地址（只放回环/私网地址，见 `net::direct_addrs` 的过滤）：
    /// 同局域网/同机可以免 relay 直连，测试也因此能完全离线拨号
    pub addrs: Vec<std::net::SocketAddr>,
}

/// pkarr relay 的 HTTP 客户端（iroh 自带，纯 HTTP，不需要第二个 endpoint）
pub type Client = PkarrRelayClient;

impl DirectoryEntry {
    pub fn encode(&self) -> String {
        let mut text = format!("d={}.{}", self.id.to_z32(), self.published_at);
        let addrs: Vec<String> = self
            .addrs
            .iter()
            .take(MAX_ADDRS_PER_ENTRY)
            .map(ToString::to_string)
            .collect();
        if !addrs.is_empty() {
            text.push('.');
            text.push_str(&addrs.join(","));
        }
        text
    }

    pub fn decode(value: &str) -> Option<Self> {
        let body = value.strip_prefix("d=")?;
        let mut parts = body.splitn(3, '.');
        // 注意：PublicKey 的 FromStr 走 base32hex，而目录里存的是 z-base-32，必须用 from_z32
        let id = EndpointId::from_z32(parts.next()?).ok()?;
        let published_at = parts.next()?.parse().ok()?;
        let addrs = parts
            .next()
            .map(|raw| {
                raw.split(',')
                    .filter_map(|chunk| chunk.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id,
            published_at,
            addrs,
        })
    }

    pub fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.published_at) <= ENTRY_STALE_SECS
    }
}

/// 账户目录的签名密钥：同一 (username, secret) 在任意设备派生出同一把。
pub fn account_secret(keys: &SyncKeys) -> SecretKey {
    SecretKey::from_bytes(&keys.dir_key)
}

pub fn parse_directory_url(raw: &str) -> CoreResult<Url> {
    let trimmed = raw.trim();
    let url = if trimmed.is_empty() {
        DEFAULT_DIRECTORY_URL
    } else {
        trimmed
    };
    Url::parse(url).map_err(|error| {
        CoreError::validation(
            "SYNC_P2P_DIRECTORY_INVALID",
            format!("P2P 目录地址不合法（{url}）：{error}"),
        )
    })
}

/// 目录客户端复用 endpoint 的 TLS 与 DNS 配置（iroh 已经配好系统 DNS 与根证书）。
pub fn build_client(endpoint: &Endpoint, directory_url: &str) -> CoreResult<PkarrRelayClient> {
    let url = parse_directory_url(directory_url)?;
    let tls_config = endpoint.tls_config().clone();
    let dns_resolver = endpoint
        .dns_resolver()
        .map_err(|error| CoreError::internal(format!("P2P 目录客户端初始化失败：{error:?}")))?
        .clone();
    Ok(PkarrRelayClient::new(url, tls_config, dns_resolver))
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// 解析目录。任何失败都按「目录里只有我自己」处理（fail-soft）：
/// 目录服务不可达时每台设备各自当自己的枢纽，本地数据不受影响，
/// 等网络恢复下一轮自然收敛——比把同步整轮报错更符合「静默重连」的约定。
pub async fn fetch(client: &PkarrRelayClient, account_id: EndpointId) -> Vec<DirectoryEntry> {
    let packet = match client.resolve(account_id).await {
        Ok(packet) => packet,
        Err(error) => {
            let mut chain = format!("{error}");
            let mut source = std::error::Error::source(&error);
            while let Some(next) = source {
                chain.push_str(" -> ");
                chain.push_str(&next.to_string());
                source = std::error::Error::source(next);
            }
            crate::sync::engine::debug_log(format!("p2p directory resolve failed: {chain}"));
            return Vec::new();
        }
    };
    let now = now_unix();
    let mut entries: Vec<DirectoryEntry> = packet
        .txt_records(DIRECTORY_NAME)
        .iter()
        .filter_map(|value| DirectoryEntry::decode(value))
        .filter(|entry| entry.is_fresh(now))
        .collect();
    entries.sort_by_key(|entry| entry.published_at);
    entries.dedup_by(|a, b| a.id == b.id);
    entries
}

/// 发布「我自己在线」：读-改-写合并别人的条目，409 就重来。
pub async fn publish(
    client: &PkarrRelayClient,
    account_secret: &SecretKey,
    self_id: EndpointId,
    addrs: &[std::net::SocketAddr],
) -> CoreResult<()> {
    let account_id = account_secret.public();
    let mut last_error = String::new();
    for _ in 0..PUBLISH_RETRIES {
        let now = now_unix();
        let mut entries = fetch(client, account_id).await;
        entries.retain(|entry| entry.id != self_id);
        entries.push(DirectoryEntry {
            id: self_id,
            published_at: now,
            addrs: addrs.to_vec(),
        });
        entries.sort_by_key(|entry| entry.published_at);
        if entries.len() > MAX_ENTRIES {
            entries.drain(0..entries.len() - MAX_ENTRIES);
        }
        let values: Vec<String> = std::iter::once("v=1".to_string())
            .chain(entries.iter().map(DirectoryEntry::encode))
            .collect();
        let packet = SignedPacket::from_txt_strings(account_secret, DIRECTORY_NAME, &values, 3600)
            .map_err(|error| {
                CoreError::internal(format!("P2P 目录记录签名失败：{error:?}"))
            })?;
        match client.publish(&packet).await {
            Ok(()) => {
                // pkarr 是整包 last-write-wins：并发发布可能互相覆盖。回读一次，
                // 自己的条目不在就说明刚被盖掉，立刻读-改-写重试把它补回来。
                let check = fetch(client, account_id).await;
                if check.iter().any(|entry| entry.id == self_id) {
                    return Ok(());
                }
                last_error = "发布被并发覆盖，重试".to_string();
            }
            Err(error) => {
                let conflicted = matches!(
                    &error,
                    iroh::address_lookup::pkarr::PkarrError::HttpRequest { status, .. }
                        if status.as_u16() == 409
                );
                last_error = format!("{error:?}");
                if !conflicted {
                    break;
                }
            }
        }
    }
    Err(CoreError::io(format!(
        "P2P 目录发布失败（{last_error}）：其它设备这轮看不到本机"
    )))
}

/// 从目录里撤掉自己（退出时尽力而为）。
pub async fn unpublish(client: &PkarrRelayClient, account_secret: &SecretKey, self_id: EndpointId) {
    let account_id = account_secret.public();
    let mut entries = fetch(client, account_id).await;
    entries.retain(|entry| entry.id != self_id);
    let values: Vec<String> = std::iter::once("v=1".to_string())
        .chain(entries.iter().map(DirectoryEntry::encode))
        .collect();
    let Ok(packet) = SignedPacket::from_txt_strings(account_secret, DIRECTORY_NAME, &values, 3600)
    else {
        return;
    };
    let _ = client.publish(&packet).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_roundtrip_and_staleness() {
        let id = SecretKey::generate().public();
        let entry = DirectoryEntry {
            id,
            published_at: 1_700_000_000,
            addrs: vec![
                "127.0.0.1:50061".parse().unwrap(),
                "192.168.31.144:50061".parse().unwrap(),
            ],
        };
        let text = entry.encode();
        assert!(text.starts_with("d="));
        assert!(text.len() <= 255, "单条 TXT 不能超过 255 字节");
        let decoded = DirectoryEntry::decode(&text).expect("roundtrip");
        assert_eq!(decoded, entry);
        // 没有地址的旧格式条目也要能读
        let bare = DirectoryEntry::decode(&format!("d={}.123", id.to_z32())).expect("bare");
        assert!(bare.addrs.is_empty());
        assert!(entry.is_fresh(1_700_000_000 + ENTRY_STALE_SECS));
        assert!(!entry.is_fresh(1_700_000_000 + ENTRY_STALE_SECS + 1));
        assert!(DirectoryEntry::decode("v=1").is_none());
        assert!(DirectoryEntry::decode("d=zz.12").is_none());
    }

    #[test]
    fn account_key_is_stable_and_distinct_from_transport_keys() {
        let keys = crate::sync::crypto::derive_keys("user", "secret").unwrap();
        let a = account_secret(&keys);
        let b = account_secret(&keys);
        assert_eq!(a.public(), b.public(), "同账户必须派生出同一把目录密钥");
        let other = crate::sync::crypto::derive_keys("user2", "secret").unwrap();
        assert_ne!(a.public(), account_secret(&other).public());
    }

    #[test]
    fn record_fits_the_dns_packet_budget() {
        // 8 台设备（各带两个直连地址）+ v=1 必须仍在 1000 字节的 DNS 包上限内
        let secret = SecretKey::generate();
        let now = now_unix();
        let addrs = vec![
            "127.0.0.1:50061".parse().unwrap(),
            "192.168.31.144:50061".parse().unwrap(),
        ];
        let mut values = vec!["v=1".to_string()];
        for index in 0..MAX_ENTRIES {
            values.push(
                DirectoryEntry {
                    id: SecretKey::from_bytes(&[index as u8; 32]).public(),
                    published_at: now,
                    addrs: addrs.clone(),
                }
                .encode(),
            );
        }
        let packet =
            SignedPacket::from_txt_strings(&secret, DIRECTORY_NAME, &values, 3600).unwrap();
        assert!(
            packet.as_bytes().len() <= SignedPacket::MAX_BYTES,
            "目录记录超包：{}",
            packet.as_bytes().len()
        );
    }
}

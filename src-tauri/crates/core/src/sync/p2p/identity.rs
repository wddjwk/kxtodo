//! P2P 本机身份与已知对端（`runtime/p2p.json`，0600）。
//!
//! - `secretKey`：本设备 iroh 端点的私钥（32 字节 hex）。**丢了 = 换身份**：
//!   目录里的旧条目自然过期，其它设备不会再拨一个不存在的 EndpointId。
//! - `knownPeers`：拨号成功/失败过的对端（id → 名字与最近一次结果），
//!   设置页的设备列表与 `sync peers` 据此展示——目录里只有 id，名字是
//!   拨号成功后从对方 `/healthz` 的 `name` 学到的。

use std::collections::BTreeMap;
use std::fs;

use iroh::SecretKey;
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::repo::{atomic_write, Layout};
use crate::sync::state::restrict_permissions;

pub const P2P_STATE_FILE: &str = "p2p.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownPeer {
    /// 对方 `/healthz` 报的展示名（没拨成功过就是空）
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub last_ok: Option<bool>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2pStateFile {
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub known_peers: BTreeMap<String, KnownPeer>,
}

pub fn state_path(layout: &Layout) -> std::path::PathBuf {
    layout.runtime_dir().join(P2P_STATE_FILE)
}

pub fn load(layout: &Layout) -> P2pStateFile {
    let Ok(raw) = fs::read_to_string(state_path(layout)) else {
        return P2pStateFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(layout: &Layout, state: &P2pStateFile) -> CoreResult<()> {
    fs::create_dir_all(layout.runtime_dir())?;
    let path = state_path(layout);
    let raw = serde_json::to_string_pretty(state)?;
    atomic_write(&path, &raw)?;
    restrict_permissions(&path);
    Ok(())
}

/// 本设备的端点私钥：第一次调用时生成并落盘，之后一直沿用（身份稳定，目录条目才稳定）。
pub fn device_secret(layout: &Layout) -> CoreResult<SecretKey> {
    let mut state = load(layout);
    if state.secret_key.trim().is_empty() {
        let secret = SecretKey::generate();
        state.secret_key = hex(&secret.to_bytes());
        save(layout, &state)?;
        return Ok(secret);
    }
    let bytes = unhex(state.secret_key.trim()).ok_or_else(|| {
        crate::error::CoreError::internal("runtime/p2p.json 里的端点私钥不是合法的 32 字节 hex")
    })?;
    Ok(SecretKey::from_bytes(&bytes))
}

/// 记录一次拨号结果（成功带名字，失败带原因）——设备列表与 `sync peers` 的数据源。
pub fn record_peer(layout: &Layout, id_z32: &str, name: &str, ok: bool, error: Option<String>) {
    let mut state = load(layout);
    let entry = state.known_peers.entry(id_z32.to_string()).or_default();
    if !name.is_empty() {
        entry.name = name.to_string();
    }
    entry.last_seen_at = Some(crate::time::now_iso());
    entry.last_ok = Some(ok);
    entry.last_error = if ok { None } else { error };
    let _ = save(layout, &state);
}

pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let bytes = [0xab; 32];
        assert_eq!(unhex(&hex(&bytes)), Some(bytes));
        assert!(unhex("zz").is_none());
        assert!(unhex(&"0".repeat(63)).is_none());
    }
}

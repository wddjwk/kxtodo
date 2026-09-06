//! 配对历史（`<data>/runtime/sync-history.json`，0600）：记住用过的
//! 「通信方式 + 地址或主机名 + 用户名 + 密码」，设置页「历史」按钮一键回填，
//! 免得每次重敲。
//!
//! 密码在这里是明文——但 `settings.json` 的 `sync.secret` 本来就是明文存储，
//! 历史文件用同一套 0600 权限，不额外降低安全等级。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::model::SyncMode;
use crate::repo::{atomic_write, Layout};
use crate::sync::endpoint::PairRequest;
use crate::time::now_iso;

pub const HISTORY_FILE: &str = "sync-history.json";
/// 只留最近用过的几条，避免文件无限增长
const MAX_ENTRIES: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// 通信方式（`lan` / `server` / `p2p`）。空 = v0.5.1 的旧记录，那会儿只有自建服务。
    #[serde(default)]
    pub mode: String,
    /// 自建服务的地址；局域网/P2P 模式下为空
    pub server_url: String,
    /// 局域网模式下选定的主机名（主机的身份是名字，不是 ip:port）
    #[serde(default)]
    pub lan_peer: String,
    pub username: String,
    pub secret: String,
    pub used_at: String,
}

impl HistoryEntry {
    /// 生效的通信方式：旧记录没有 mode 字段，那时候只有「自建服务」这一种。
    pub fn effective_mode(&self) -> SyncMode {
        SyncMode::parse(&self.mode).unwrap_or(SyncMode::Server)
    }

    /// 这条历史在用户眼里指向哪儿（浮层里的主标题）。
    pub fn target_label(&self) -> String {
        match self.effective_mode() {
            SyncMode::Lan => {
                if self.lan_peer.is_empty() {
                    "本机（局域网主机）".to_string()
                } else {
                    format!("局域网主机「{}」", self.lan_peer)
                }
            }
            SyncMode::Server => self.server_url.clone(),
            SyncMode::P2p => "P2P".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFile {
    #[serde(default = "default_version")]
    pub version: u32,
    /// 最近使用的排在最前
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
}

fn default_version() -> u32 {
    1
}

pub fn history_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join(HISTORY_FILE)
}

pub fn load_history(layout: &Layout) -> HistoryFile {
    let Ok(raw) = fs::read_to_string(history_path(layout)) else {
        return HistoryFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_history(layout: &Layout, file: &HistoryFile) -> CoreResult<()> {
    fs::create_dir_all(layout.runtime_dir())?;
    let path = history_path(layout);
    let raw = serde_json::to_string_pretty(file)?;
    atomic_write(&path, &raw)?;
    super::state::restrict_permissions(&path);
    Ok(())
}

/// 记住一次成功配对：同一 (方式, 目标, 用户名) 只留一条并提到最前。
pub fn remember(layout: &Layout, request: &PairRequest) -> CoreResult<()> {
    let request = request.normalized();
    let name = request.username.clone();
    if name.is_empty() || request.secret.is_empty() {
        return Ok(());
    }
    // 没有目标的记录回填不出东西。局域网模式下 lan_peer 为空 = 本机就是主机
    // （不是主机又没选主机的话 resolve 早就拒了，走不到这里），所以照样值得记。
    if request.mode == SyncMode::Server && request.server_url.is_empty() {
        return Ok(());
    }
    let mut file = load_history(layout);
    file.entries.retain(|entry| {
        !(entry.effective_mode() == request.mode
            && entry.server_url == request.server_url
            && entry.lan_peer == request.lan_peer
            && entry.username.eq_ignore_ascii_case(&name))
    });
    file.entries.insert(
        0,
        HistoryEntry {
            mode: request.mode.as_str().to_string(),
            server_url: request.server_url.clone(),
            lan_peer: request.lan_peer.clone(),
            username: name,
            secret: request.secret.clone(),
            used_at: now_iso(),
        },
    );
    file.entries.truncate(MAX_ENTRIES);
    file.version = default_version();
    save_history(layout, &file)
}

/// 按下标删除一条历史（设置页的 ✕）；返回删除后的列表。
pub fn remove(layout: &Layout, index: usize) -> CoreResult<HistoryFile> {
    let mut file = load_history(layout);
    if index < file.entries.len() {
        file.entries.remove(index);
        save_history(layout, &file)?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_layout() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().to_path_buf());
        (dir, layout)
    }

    fn server_pair(url: &str, username: &str, secret: &str) -> PairRequest {
        PairRequest {
            mode: SyncMode::Server,
            server_url: url.to_string(),
            lan_peer: String::new(),
            username: username.to_string(),
            secret: secret.to_string(),
        }
    }

    fn lan_pair(peer: &str, username: &str, secret: &str) -> PairRequest {
        PairRequest {
            mode: SyncMode::Lan,
            server_url: String::new(),
            lan_peer: peer.to_string(),
            username: username.to_string(),
            secret: secret.to_string(),
        }
    }

    #[test]
    fn remember_is_mru_and_deduped() {
        let (_guard, layout) = temp_layout();
        remember(&layout, &server_pair("http://a:1/", "Me", "s1")).unwrap();
        remember(&layout, &server_pair("http://b:1", "me", "s2")).unwrap();
        // 同一账户换密码：更新并提到最前，不留重复
        remember(&layout, &server_pair("http://a:1", "ME", "s3")).unwrap();
        let file = load_history(&layout);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].server_url, "http://a:1");
        assert_eq!(file.entries[0].username, "me");
        assert_eq!(file.entries[0].secret, "s3");
        assert_eq!(file.entries[0].mode, "server");
        assert_eq!(file.entries[1].server_url, "http://b:1");
    }

    #[test]
    fn lan_entries_are_keyed_by_host_name_not_address() {
        let (_guard, layout) = temp_layout();
        remember(&layout, &lan_pair("客厅的电脑", "me", "s1")).unwrap();
        remember(&layout, &lan_pair("客厅的电脑", "me", "s2")).unwrap();
        remember(&layout, &lan_pair("书房的电脑", "me", "s3")).unwrap();
        let file = load_history(&layout);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].lan_peer, "书房的电脑");
        assert_eq!(file.entries[0].mode, "lan");
        assert_eq!(file.entries[1].secret, "s2");
        // 同一个账户在局域网与自建服务下是两条不同的历史
        remember(&layout, &server_pair("http://a:1", "me", "s4")).unwrap();
        assert_eq!(load_history(&layout).entries.len(), 3);
    }

    #[test]
    fn entries_without_a_mode_are_treated_as_self_hosted() {
        let (_guard, layout) = temp_layout();
        // v0.5.1 写下的记录：没有 mode 字段
        let legacy = HistoryFile {
            version: 1,
            entries: vec![HistoryEntry {
                mode: String::new(),
                server_url: "http://a:1".to_string(),
                lan_peer: String::new(),
                username: "me".to_string(),
                secret: "old".to_string(),
                used_at: now_iso(),
            }],
        };
        save_history(&layout, &legacy).unwrap();
        assert_eq!(
            load_history(&layout).entries[0].effective_mode(),
            SyncMode::Server
        );
        // 新写法命中同一条，不会并存
        remember(&layout, &server_pair("http://a:1", "me", "new")).unwrap();
        let file = load_history(&layout);
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].secret, "new");
        assert_eq!(file.entries[0].mode, "server");
    }

    #[test]
    fn remember_caps_entries_and_ignores_blanks() {
        let (_guard, layout) = temp_layout();
        for index in 0..12 {
            remember(&layout, &server_pair(&format!("http://h{index}:1"), "u", "s")).unwrap();
        }
        assert_eq!(load_history(&layout).entries.len(), MAX_ENTRIES);
        remember(&layout, &server_pair("http://h99:1", "u", "")).unwrap();
        assert_eq!(load_history(&layout).entries.len(), MAX_ENTRIES);
        // 自建服务没填地址：回填不出东西，不记
        remember(&layout, &server_pair("", "u", "s")).unwrap();
        assert_eq!(load_history(&layout).entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn remove_by_index() {
        let (_guard, layout) = temp_layout();
        remember(&layout, &server_pair("http://a:1", "u", "s")).unwrap();
        remember(&layout, &server_pair("http://b:1", "u", "s")).unwrap();
        let file = remove(&layout, 1).unwrap();
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].server_url, "http://b:1");
        // 越界无副作用
        assert_eq!(remove(&layout, 99).unwrap().entries.len(), 1);
    }
}

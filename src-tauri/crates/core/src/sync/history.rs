//! 配对历史（`<data>/runtime/sync-history.json`，0600）：记住用过的
//! 「服务器地址 + 用户名 + 密码」，设置页「历史」按钮一键回填，免得每次重敲。
//!
//! 密码在这里是明文——但 `settings.json` 的 `sync.secret` 本来就是明文存储，
//! 历史文件用同一套 0600 权限，不额外降低安全等级。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::repo::{atomic_write, Layout};
use crate::time::now_iso;

pub const HISTORY_FILE: &str = "sync-history.json";
/// 只留最近用过的几条，避免文件无限增长
const MAX_ENTRIES: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub server_url: String,
    pub username: String,
    pub secret: String,
    pub used_at: String,
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

fn normalize_url(server_url: &str) -> String {
    server_url.trim().trim_end_matches('/').to_string()
}

/// 记住一次成功配对：同一 (地址, 用户名) 只留一条并提到最前。
pub fn remember(layout: &Layout, server_url: &str, username: &str, secret: &str) -> CoreResult<()> {
    let url = normalize_url(server_url);
    let name = username.trim().to_lowercase();
    if url.is_empty() || name.is_empty() || secret.is_empty() {
        return Ok(());
    }
    let mut file = load_history(layout);
    file.entries.retain(|entry| {
        !(entry.server_url == url && entry.username.eq_ignore_ascii_case(&name))
    });
    file.entries.insert(
        0,
        HistoryEntry {
            server_url: url,
            username: name,
            secret: secret.to_string(),
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

    #[test]
    fn remember_is_mru_and_deduped() {
        let (_guard, layout) = temp_layout();
        remember(&layout, "http://a:1/", "Me", "s1").unwrap();
        remember(&layout, "http://b:1", "me", "s2").unwrap();
        // 同一账户换密码：更新并提到最前，不留重复
        remember(&layout, "http://a:1", "ME", "s3").unwrap();
        let file = load_history(&layout);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].server_url, "http://a:1");
        assert_eq!(file.entries[0].username, "me");
        assert_eq!(file.entries[0].secret, "s3");
        assert_eq!(file.entries[1].server_url, "http://b:1");
    }

    #[test]
    fn remember_caps_entries_and_ignores_blanks() {
        let (_guard, layout) = temp_layout();
        for index in 0..12 {
            remember(&layout, &format!("http://h{index}:1"), "u", "s").unwrap();
        }
        assert_eq!(load_history(&layout).entries.len(), MAX_ENTRIES);
        remember(&layout, "http://h99:1", "u", "").unwrap();
        assert_eq!(load_history(&layout).entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn remove_by_index() {
        let (_guard, layout) = temp_layout();
        remember(&layout, "http://a:1", "u", "s").unwrap();
        remember(&layout, "http://b:1", "u", "s").unwrap();
        let file = remove(&layout, 1).unwrap();
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].server_url, "http://b:1");
        // 越界无副作用
        assert_eq!(remove(&layout, 99).unwrap().entries.len(), 1);
    }
}

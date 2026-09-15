//! 同步凭据的**明文兜底记录**（`<data>/runtime/sync-credentials.json`，0600）。
//!
//! 同步密码派生出加密密钥，忘了就真的解不开数据。用户点名要一份「最后兜底的
//! 找回方式」：账号、密码与用过的连接方式在本机明文留档，相信本地的数据安全
//! （0600、只存本机、不上网、不参与同步、不进任何同步 payload）。
//!
//! 与 `sync-history.json` 的分工：那份是**回填用**的 MRU（最多 8 条，用户能在
//! 设置页逐条删、解除配对也不动它）；这份是**留档**：解除配对（`sync unpair`
//! 会清掉 settings 里的密码）也照样留着，条数上限更宽——正是「早先配对过哪台、
//! 当时密码填的什么、后来想不起来」时的兜底。两个文件在配对成功时一起写。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::model::SyncMode;
use crate::repo::{atomic_write, Layout};
use crate::sync::endpoint::PairRequest;
use crate::time::now_iso;

pub const CREDENTIALS_FILE: &str = "sync-credentials.json";
/// 留档条数上限（比历史的 8 条宽得多：这是「翻旧账」用的）
const MAX_ENTRIES: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntry {
    /// 通信方式（lan / server / p2p）
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub lan_peer: String,
    pub username: String,
    /// **明文**同步密码——这个文件存在的全部意义就是它
    pub secret: String,
    /// 第一次记下这条的时间
    #[serde(default)]
    pub first_at: String,
    /// 最近一次用到（配对成功）的时间
    pub used_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    /// 最近使用的排在最前
    #[serde(default)]
    pub entries: Vec<CredentialEntry>,
}

fn default_version() -> u32 {
    1
}

pub fn credentials_path(layout: &Layout) -> PathBuf {
    layout.runtime_dir().join(CREDENTIALS_FILE)
}

pub fn load_credentials(layout: &Layout) -> CredentialsFile {
    let Ok(raw) = fs::read_to_string(credentials_path(layout)) else {
        return CredentialsFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_credentials(layout: &Layout, file: &CredentialsFile) -> CoreResult<()> {
    fs::create_dir_all(layout.runtime_dir())?;
    let path = credentials_path(layout);
    let raw = serde_json::to_string_pretty(file)?;
    atomic_write(&path, &raw)?;
    super::state::restrict_permissions(&path);
    Ok(())
}

/// 记下一次配对（与 `history::remember` 同一个调用点，口径也一致：
/// 同一 (方式, 目标, 用户名) 只留一条并提到最前，换过密码就更新密码）。
pub fn remember(layout: &Layout, request: &PairRequest) -> CoreResult<()> {
    let request = request.normalized();
    if request.username.is_empty() || request.secret.is_empty() {
        return Ok(());
    }
    if request.mode == SyncMode::Server && request.server_url.is_empty() {
        return Ok(());
    }
    let mut file = load_credentials(layout);
    let previous = file.entries.iter().find(|entry| {
        entry.mode == request.mode.as_str()
            && entry.server_url == request.server_url
            && entry.lan_peer == request.lan_peer
            && entry.username.eq_ignore_ascii_case(&request.username)
    });
    let first_at = previous
        .map(|entry| entry.first_at.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(now_iso);
    file.entries.retain(|entry| {
        !(entry.mode == request.mode.as_str()
            && entry.server_url == request.server_url
            && entry.lan_peer == request.lan_peer
            && entry.username.eq_ignore_ascii_case(&request.username))
    });
    file.entries.insert(
        0,
        CredentialEntry {
            mode: request.mode.as_str().to_string(),
            server_url: request.server_url.clone(),
            lan_peer: request.lan_peer.clone(),
            username: request.username,
            secret: request.secret.clone(),
            first_at,
            used_at: now_iso(),
        },
    );
    file.entries.truncate(MAX_ENTRIES);
    file.version = default_version();
    save_credentials(layout, &file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_layout() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().to_path_buf());
        (dir, layout)
    }

    fn pair(mode: SyncMode, url: &str, peer: &str, username: &str, secret: &str) -> PairRequest {
        PairRequest {
            mode,
            server_url: url.to_string(),
            lan_peer: peer.to_string(),
            username: username.to_string(),
            secret: secret.to_string(),
        }
    }

    #[test]
    fn keeps_plaintext_and_keeps_first_timestamp() {
        let (_guard, layout) = temp_layout();
        remember(
            &layout,
            &pair(SyncMode::Server, "http://a:1/", "", "Me", "secret-one"),
        )
        .unwrap();
        let first = load_credentials(&layout).entries[0].clone();
        assert_eq!(first.username, "me");
        assert_eq!(first.secret, "secret-one");

        // 同一账户换密码：更新密码、保留 firstAt、提到最前
        remember(
            &layout,
            &pair(SyncMode::Server, "http://a:1", "", "ME", "secret-two"),
        )
        .unwrap();
        let file = load_credentials(&layout);
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].secret, "secret-two");
        assert_eq!(file.entries[0].first_at, first.first_at);
    }

    #[test]
    fn survives_unpair_and_is_not_touched_by_history_removal() {
        let (_guard, layout) = temp_layout();
        remember(
            &layout,
            &pair(SyncMode::Lan, "", "客厅的电脑", "me", "secret-lan"),
        )
        .unwrap();
        // 历史文件删空（设置页里的 ✕）不影响留档
        crate::sync::history::remove(&layout, 0).unwrap();
        let file = load_credentials(&layout);
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].lan_peer, "客厅的电脑");
        assert_eq!(file.entries[0].secret, "secret-lan");
    }

    #[test]
    fn ignores_incomplete_credentials_and_caps_entries() {
        let (_guard, layout) = temp_layout();
        remember(&layout, &pair(SyncMode::Server, "http://a:1", "", "u", "")).unwrap();
        remember(&layout, &pair(SyncMode::Server, "", "", "u", "s")).unwrap();
        assert_eq!(load_credentials(&layout).entries.len(), 0);
        for index in 0..40 {
            remember(
                &layout,
                &pair(SyncMode::Server, &format!("http://h{index}:1"), "", "u", "s"),
            )
            .unwrap();
        }
        assert_eq!(load_credentials(&layout).entries.len(), MAX_ENTRIES);
    }
}

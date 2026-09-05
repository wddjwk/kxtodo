//! 同步运行时状态（`<data>/runtime/sync.json`，0600）：
//! 设备身份、登录 token、拉取水位与逐实体推送水位。不属于三个 domain 文件。

use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStateFile {
    pub version: u32,
    pub device_id: String,
    pub token: String,
    pub token_expires_at: Option<String>,
    pub last_pulled_seq: u64,
    #[serde(default)]
    pub pushed: Map<String, Value>,
    pub last_sync_at: Option<String>,
    pub last_result: Option<Value>,
}

impl SyncStateFile {
    pub fn fresh(device_id: String) -> Self {
        Self {
            version: 1,
            device_id,
            token: String::new(),
            token_expires_at: None,
            last_pulled_seq: 0,
            pushed: Map::new(),
            last_sync_at: None,
            last_result: None,
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

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

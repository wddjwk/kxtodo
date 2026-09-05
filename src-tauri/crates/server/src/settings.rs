//! server 持久化配置（`~/.local/share/kxtodo/server/settings.json`）：
//! 用户给过的启动参数都记下来；下次启动未指定的项从配置读取，显式指定则覆盖。
//! 管理员密码只存 Argon2id/SHA-256 哈希，绝不落明文。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{ServerError, ServerResult};
use crate::util;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    pub listen: String,
    pub db: String,
    #[serde(rename = "adminUser")]
    pub admin_user: String,
    #[serde(rename = "adminPasswordHash")]
    pub admin_password_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_password_salt: Option<String>,
    pub version: u32,
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn load(data_dir: &Path) -> ServerResult<Option<ServerSettings>> {
    let path = settings_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| ServerError::internal(format!("读取配置失败：{e}")))?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| ServerError::internal(format!("配置文件损坏（{}）：{e}", path.display())))
}

pub fn save(data_dir: &Path, settings: &ServerSettings) -> ServerResult<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = settings_path(data_dir);
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| ServerError::internal(format!("序列化配置失败：{e}")))?;
    // 0600：含管理员密码哈希
    std::fs::write(&path, raw)
        .map_err(|e| ServerError::internal(format!("写配置失败：{e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 管理员密码哈希：SHA-256(salt + password)，salt 随机 16 字节 hex。
/// （管理员账密只用于管理界面登录门禁，不参与任何客户端数据加密。）
pub fn hash_password(password: &str) -> (String, String) {
    let salt = util::random_hex(16);
    let digest = util::sha256_hex(format!("{salt}{password}").as_bytes());
    (digest, salt)
}

pub fn verify_password(password: &str, hash: &str, salt: &str) -> bool {
    let digest = util::sha256_hex(format!("{salt}{password}").as_bytes());
    util::constant_time_eq(&digest, hash)
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            listen: String::new(),
            db: String::new(),
            admin_user: String::new(),
            admin_password_hash: String::new(),
            admin_password_salt: None,
            version: 1,
        }
    }
}

impl ServerSettings {
    pub fn to_json(&self) -> Value {
        json!({
            "listen": self.listen,
            "db": self.db,
            "adminUser": self.admin_user,
            "version": self.version,
        })
    }
}

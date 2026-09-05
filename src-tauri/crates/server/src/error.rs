//! 服务端错误：统一 JSON 错误形态 `{error: {code, message}}` + HTTP 状态码。

use std::fmt;

pub type ServerResult<T> = Result<T, ServerError>;

pub struct ServerError {
    pub status: u16,
    pub code: String,
    pub message: String,
    /// 附加字段（如 409 的 currentSeq），合并进错误响应 JSON 顶层。
    pub details: Option<serde_json::Value>,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::fmt::Debug for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ServerError({} {})", self.status, self.code)
    }
}

impl ServerError {
    fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.to_string(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn account_exists() -> Self {
        Self::new(409, "ACCOUNT_EXISTS", "账户已注册")
    }

    pub fn account_not_found() -> Self {
        Self::new(404, "ACCOUNT_NOT_FOUND", "账户不存在")
    }

    pub fn auth_failed() -> Self {
        Self::new(401, "AUTH_FAILED", "账户不存在或同步密钥不正确")
    }

    pub fn unauthorized() -> Self {
        Self::new(401, "UNAUTHORIZED", "缺少或无效的登录令牌")
    }

    pub fn challenge_invalid() -> Self {
        Self::new(401, "CHALLENGE_INVALID", "登录挑战无效或已过期")
    }

    pub fn version_conflict(current_seq: u64) -> Self {
        Self::new(
            409,
            "VERSION_CONFLICT",
            format!("实体版本冲突，当前服务端版本 {current_seq}"),
        )
        .with_details(serde_json::json!({ "currentSeq": current_seq }))
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, "BAD_REQUEST", message)
    }

    pub fn not_found() -> Self {
        Self::new(404, "ENTITY_NOT_FOUND", "实体不存在")
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "INTERNAL", message)
    }
}

impl From<rusqlite::Error> for ServerError {
    fn from(error: rusqlite::Error) -> Self {
        ServerError::internal(format!("数据库错误：{error}"))
    }
}

impl From<std::io::Error> for ServerError {
    fn from(error: std::io::Error) -> Self {
        ServerError::internal(format!("IO 错误：{error}"))
    }
}

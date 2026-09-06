//! 同步传输层：HTTP 客户端（ureq）。
//!
//! **三种通信方式共用这一份实现**（v0.6.0）：自建服务、局域网内置主机、P2P 隧道
//! 说的都是同一套 `/api/v1` 协议，区别只在 base url 从哪来——那是
//! [`crate::sync::endpoint`] 的事。所以这里不感知「模式」，只认一个 base url。
//!
//! 服务器不理解业务数据：只保管密文与版本号，合并永远在客户端（见 `merge.rs`）。

use std::io::Read;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{CoreError, CoreResult};
use crate::sync::crypto::hmac_sha256;
use crate::sync::images::{ImageChangesPage, LocalImage};

/// 实体增量流的分页大小
pub const PAGE_LIMIT: usize = 500;
/// 图片元数据分页大小（只传元数据，密文按需逐张下载）
pub const IMAGE_PAGE_LIMIT: usize = 200;
const HTTP_TIMEOUT_SECS: u64 = 30;

pub struct SyncClient {
    agent: ureq::Agent,
    base: String,
}

/// 校验并规范化 base url（去尾斜杠，要求带 scheme）。
pub(crate) fn normalize_base_url(raw: &str) -> CoreResult<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(CoreError::validation(
            "SYNC_SERVER_REQUIRED",
            "同步服务器地址不能为空",
        ));
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(CoreError::validation(
            "SYNC_SERVER_URL_INVALID",
            format!("服务器地址应以 http:// 或 https:// 开头：{raw}"),
        ));
    }
    Ok(trimmed.to_string())
}

fn api_error(status: u16, body: String) -> CoreError {
    let mut code = format!("SYNC_HTTP_{status}");
    let mut message = body.clone();
    if let Ok(parsed) = serde_json::from_str::<Value>(&body) {
        if let Some(error) = parsed.get("error").and_then(|e| e.as_object()) {
            if let Some(c) = error.get("code").and_then(Value::as_str) {
                code = c.to_string();
            }
            if let Some(m) = error.get("message").and_then(Value::as_str) {
                message = m.to_string();
            }
        }
    }
    let error = CoreError::io(format!("同步服务错误（{status}）：{message}"));
    CoreError::new(error.kind, &code, error.message)
}

pub fn network_error(error: ureq::Error) -> CoreError {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            api_error(code, body)
        }
        _ => CoreError::io(format!("无法连接同步服务器：{error}")),
    }
}

impl SyncClient {
    pub fn new(base: &str) -> CoreResult<Self> {
        Ok(Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
                .build(),
            base: normalize_base_url(base)?,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub fn health(&self) -> CoreResult<Value> {
        let response = self
            .agent
            .get(&format!("{}/healthz", self.base))
            .call()
            .map_err(network_error)?;
        response
            .into_json::<Value>()
            .map_err(|e| CoreError::io(format!("healthz 响应无效：{e}")))
    }

    pub fn register(&self, username: &str, auth_key: &[u8; 32]) -> CoreResult<String> {
        let body = json!({
            "username": username,
            "authKey": crate::sync::crypto::to_hex(auth_key),
        });
        let response = self
            .agent
            .post(&format!("{}/api/v1/register", self.base))
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(409, _) => CoreError::conflict(
                    "ACCOUNT_EXISTS",
                    format!("账户 {username} 已注册，请改用 login 配对"),
                ),
                _ => network_error(error),
            })?;
        #[derive(Deserialize)]
        struct RegisterResponse {
            #[serde(rename = "userId")]
            user_id: String,
        }
        let parsed: RegisterResponse = response
            .into_json()
            .map_err(|e| CoreError::io(format!("register 响应无效：{e}")))?;
        Ok(parsed.user_id)
    }

    pub fn login_challenge(&self, username: &str) -> CoreResult<String> {
        let body = json!({ "username": username });
        let response = self
            .agent
            .post(&format!("{}/api/v1/login-challenge", self.base))
            .send_json(body)
            .map_err(network_error)?;
        #[derive(Deserialize)]
        struct ChallengeResponse {
            nonce: String,
        }
        let parsed: ChallengeResponse = response
            .into_json()
            .map_err(|e| CoreError::io(format!("login-challenge 响应无效：{e}")))?;
        Ok(parsed.nonce)
    }

    pub fn login(
        &self,
        username: &str,
        auth_key: &[u8; 32],
    ) -> CoreResult<(String, Option<String>, u64)> {
        let nonce = self.login_challenge(username)?;
        let proof = crate::sync::crypto::to_hex(&hmac_sha256(auth_key, nonce.as_bytes()));
        let body = json!({
            "username": username,
            "nonce": nonce,
            "proof": proof,
        });
        let response = self
            .agent
            .post(&format!("{}/api/v1/login", self.base))
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(401, _) => CoreError::conflict(
                    "AUTH_FAILED",
                    "登录失败：用户名不存在或密码不正确".to_string(),
                ),
                _ => network_error(error),
            })?;
        #[derive(Deserialize)]
        struct LoginResponse {
            token: String,
            #[serde(rename = "expiresAt")]
            expires_at: Option<String>,
            #[serde(rename = "currentSeq", default)]
            current_seq: u64,
        }
        let parsed: LoginResponse = response
            .into_json()
            .map_err(|e| CoreError::io(format!("login 响应无效：{e}")))?;
        Ok((parsed.token, parsed.expires_at, parsed.current_seq))
    }

    fn authed_get(&self, path: &str, token: &str) -> ureq::Request {
        self.agent
            .get(&format!("{}/api/v1/{path}", self.base))
            .set("Authorization", &format!("Bearer {token}"))
    }

    pub fn me(&self, token: &str) -> CoreResult<Value> {
        let response = self.authed_get("me", token).call().map_err(network_error)?;
        response
            .into_json::<Value>()
            .map_err(|e| CoreError::io(format!("me 响应无效：{e}")))
    }

    pub fn changes(&self, token: &str, since: u64) -> CoreResult<ChangesPage> {
        let response = self
            .authed_get(&format!("changes?since={since}&limit={PAGE_LIMIT}"), token)
            .call()
            .map_err(network_error)?;
        response
            .into_json::<ChangesPage>()
            .map_err(|e| CoreError::io(format!("changes 响应无效：{e}")))
    }

    pub fn get_entity(&self, token: &str, id: &str) -> CoreResult<Option<ChangeItem>> {
        let response = match self.authed_get(&format!("entities/{id}"), token).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(error) => return Err(network_error(error)),
        };
        response
            .into_json::<ChangeItem>()
            .map(Some)
            .map_err(|e| CoreError::io(format!("entities 响应无效：{e}")))
    }

    pub fn put_entity(
        &self,
        token: &str,
        id: &str,
        base: u64,
        nonce: &str,
        ciphertext: &str,
        hash: &str,
    ) -> Result<u64, PutError> {
        let body = json!({
            "base": base,
            "nonce": nonce,
            "ciphertext": ciphertext,
            "hash": hash,
        });
        let response = self
            .agent
            .put(&format!("{}/api/v1/entities/{id}", self.base))
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(409, response) => {
                    let current = response
                        .into_json::<Value>()
                        .ok()
                        .and_then(|value| value.get("currentSeq").and_then(Value::as_u64))
                        .unwrap_or(0);
                    PutError::Conflict(current)
                }
                _ => PutError::Api(network_error(error)),
            })?;
        #[derive(Deserialize)]
        struct PutResponse {
            seq: u64,
        }
        let parsed: PutResponse = response
            .into_json()
            .map_err(|e| PutError::Api(CoreError::io(format!("put 响应无效：{e}"))))?;
        Ok(parsed.seq)
    }

    // -- 图片 blob 通道（v0.5.0）-------------------------------------------
    // 图片是内容寻址的不可变 blob，没有 LWW/OCC：同名同内容只存一份，
    // 上传前先用 image_check 问一次「服务端缺哪些」，避免每轮重传。

    pub fn image_changes(&self, token: &str, since: u64) -> CoreResult<ImageChangesPage> {
        let response = self
            .authed_get(
                &format!("images/changes?since={since}&limit={IMAGE_PAGE_LIMIT}"),
                token,
            )
            .call()
            .map_err(network_error)?;
        response
            .into_json::<ImageChangesPage>()
            .map_err(|e| CoreError::io(format!("images/changes 响应无效：{e}")))
    }

    /// 提交本地 (id, 内容哈希) 清单，拿回服务端缺失或内容不一致的 id 列表。
    pub fn image_check(&self, token: &str, items: &[(String, String)]) -> CoreResult<Vec<String>> {
        let body = json!({
            "images": items
                .iter()
                .map(|(id, hash)| json!({ "id": id, "hash": hash }))
                .collect::<Vec<_>>(),
        });
        let response = self
            .agent
            .post(&format!("{}/api/v1/images/check", self.base))
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .map_err(network_error)?;
        #[derive(Deserialize)]
        struct CheckResponse {
            #[serde(default)]
            needed: Vec<String>,
        }
        let parsed: CheckResponse = response
            .into_json()
            .map_err(|e| CoreError::io(format!("images/check 响应无效：{e}")))?;
        Ok(parsed.needed)
    }

    /// 下载单张图片密文；nonce 在响应头（hex），密文是裸字节体。
    pub fn image_get(&self, token: &str, id: &str) -> CoreResult<Option<(String, Vec<u8>)>> {
        let response = match self.authed_get(&format!("images/{id}"), token).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(error) => return Err(network_error(error)),
        };
        let nonce = response
            .header("x-kxtodo-nonce")
            .unwrap_or_default()
            .to_string();
        let mut bytes = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| CoreError::io(format!("图片下载中断：{e}")))?;
        Ok(Some((nonce, bytes)))
    }

    /// 上传单张图片：元数据走 query（百分号编码），密文走裸字节体。
    pub fn image_put(
        &self,
        token: &str,
        image: &LocalImage,
        nonce_hex: &str,
        ciphertext: Vec<u8>,
        device_id: &str,
    ) -> CoreResult<u64> {
        let url = format!(
            "{}/api/v1/images/{}?kind={}&nodeId={}&filename={}&nonce={}&hash={}&updatedAt={}&updatedBy={}",
            self.base,
            image.id,
            encode_query(&image.kind),
            encode_query(&image.node_id),
            encode_query(&image.filename),
            encode_query(nonce_hex),
            encode_query(&image.hash),
            encode_query(&image.updated_at),
            encode_query(device_id),
        );
        let response = self
            .agent
            .put(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/octet-stream")
            .send_bytes(&ciphertext)
            .map_err(network_error)?;
        #[derive(Deserialize)]
        struct PutResponse {
            seq: u64,
        }
        let parsed: PutResponse = response
            .into_json()
            .map_err(|e| CoreError::io(format!("图片上传响应无效：{e}")))?;
        Ok(parsed.seq)
    }
}

/// query 值百分号编码：只保留 unreserved 字符，其余一律转义（文件名可能含空格/中文）。
fn encode_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeItem {
    pub id: String,
    pub seq: u64,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangesPage {
    pub entities: Vec<ChangeItem>,
    #[serde(rename = "currentSeq")]
    pub current_seq: u64,
}

#[derive(Debug)]
pub enum PutError {
    Conflict(u64),
    Api(CoreError),
}

//! 同步引擎：HTTP 客户端（ureq）+ pull → LWW merge → push 编排。
//!
//! 所有合并都在客户端完成；服务器只是「最新密文保管员」。
//! 每次同步幂等可中断：拉取水位只在完整走完后推进。

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::time::Duration;

use fs2::FileExt;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{CoreError, CoreResult};
use crate::repo::Repository;
use crate::sync::crypto::{derive_keys, hmac_sha256, open_entity, seal_entity, sha256_hex, SyncKeys};
use crate::sync::images::{ImageChangesPage, LocalImage};
use crate::sync::merge::{
    apply_data_record, apply_record_settings, apply_schedule_record, data_entity_stamp,
    normalize_data_orders, normalize_schedule_orders, remote_wins, schedule_entity_stamp,
    EntityRecord, Scopes, SETTINGS_ENTITY_ID, SyncEnvelope,
};
use crate::sync::state::{load_state, save_state, PushedEntry, SyncStateFile};
use crate::time::now_iso;

const PAGE_LIMIT: usize = 500;
/// 图片元数据分页大小（只传元数据，密文按需逐张下载）
pub const IMAGE_PAGE_LIMIT: usize = 200;
const HTTP_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// 同步互斥锁：同一数据目录同时只允许一个同步（CLI standalone / Host / GUI 并发）
// ---------------------------------------------------------------------------

pub struct SyncRunLock {
    file: File,
}

impl SyncRunLock {
    pub fn acquire(layout: &crate::repo::Layout) -> CoreResult<Self> {
        std::fs::create_dir_all(layout.runtime_dir())?;
        let path = layout.runtime_dir().join("sync.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|_| {
            CoreError::conflict(
                "SYNC_IN_PROGRESS",
                "另一个同步正在进行（CLI / GUI Host）".to_string(),
            )
            .with_hint("稍后重试，或等待自动同步完成")
        })?;
        Ok(Self { file })
    }
}

impl Drop for SyncRunLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

// ---------------------------------------------------------------------------
// HTTP 客户端
// ---------------------------------------------------------------------------

pub struct SyncClient {
    agent: ureq::Agent,
    base: String,
}

fn normalize_base_url(raw: &str) -> CoreResult<String> {
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

    pub fn register(&self, username: &str, email: &str, auth_key: &[u8; 32]) -> CoreResult<String> {
        let body = json!({
            "username": username,
            "email": email,
            "authKey": crate::sync::crypto::to_hex(auth_key),
        });
        let response = self
            .agent
            .post(&format!("{}/api/v1/register", self.base))
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(409, _) => CoreError::conflict(
                    "ACCOUNT_EXISTS",
                    format!("账户 {username} / {email} 已注册，请改用 login 配对"),
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

    pub fn login_challenge(&self, username: &str, email: &str) -> CoreResult<String> {
        let body = json!({ "username": username, "email": email });
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
        email: &str,
        auth_key: &[u8; 32],
    ) -> CoreResult<(String, Option<String>, u64)> {
        let nonce = self.login_challenge(username, email)?;
        let proof = crate::sync::crypto::to_hex(&hmac_sha256(auth_key, nonce.as_bytes()));
        let body = json!({
            "username": username,
            "email": email,
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
                    "登录失败：账户不存在或同步密钥不正确".to_string(),
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
        let response = match self
            .authed_get(&format!("entities/{id}"), token)
            .call()
        {
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

// ---------------------------------------------------------------------------
// 同步报告
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub started_at: String,
    pub finished_at: String,
    pub pulled: usize,
    pub applied: usize,
    pub pushed: usize,
    pub conflicts: usize,
    /// 本轮真正落盘的图片数（本地已有同内容的跳过不计）
    pub images_pulled: usize,
    /// 本轮上传的图片数
    pub images_pushed: usize,
    pub warnings: Vec<String>,
}

fn token_expired(expires_at: &Option<String>) -> bool {
    match expires_at
        .as_deref()
        .and_then(|raw| crate::time::parse_stored_instant(raw).ok())
    {
        Some(expiry) => expiry <= chrono::Utc::now(),
        None => true,
    }
}

/// 全新设备兜底：服务端数据缺系统节点时补默认（正常路径服务端一定带）。
fn backfill_system_nodes(file: &mut crate::model::DataFile) {
    let now = now_iso();
    for (index, id) in crate::model::SYSTEM_NODE_IDS.iter().enumerate() {
        if file.nodes.iter().any(|node| node.id == *id) {
            continue;
        }
        let default = crate::repo::default_data_file();
        if let Some(node) = default.nodes.into_iter().find(|node| node.id == *id) {
            let mut node = node;
            node.order = index as f64;
            node.created_at = now.clone();
            node.updated_at = Some(now.clone());
            file.nodes.push(node);
        }
    }
}

fn decrypt_envelope(enc_key: &[u8; 32], item: &ChangeItem) -> CoreResult<EntityRecord> {
    let plaintext = open_entity(enc_key, &item.id, &item.nonce, &item.ciphertext)?;
    let envelope: SyncEnvelope = serde_json::from_str(&plaintext)
        .map_err(|e| CoreError::io(format!("实体 {} 信封格式无效：{e}", item.id)))?;
    Ok(EntityRecord::from_envelope(envelope, item.seq))
}

fn seal_record(enc_key: &[u8; 32], record: &EntityRecord) -> CoreResult<(String, String, String)> {
    let envelope = record.to_envelope();
    let plaintext = serde_json::to_string(&envelope)?;
    let (nonce, ciphertext) = seal_entity(enc_key, &record.id, &plaintext)?;
    let hash = sha256_hex(format!("{nonce}:{ciphertext}").as_bytes());
    Ok((nonce, ciphertext, hash))
}

/// 本地版本戳 = (实体时间戳, 来源设备)；来源设备取 pushed 水位匹配值，否则视为本机新写。
fn local_stamp(state: &SyncStateFile, id: &str, ts: Option<String>) -> Option<(String, String)> {
    ts.map(|ts| {
        let by = state
            .entry(id)
            .filter(|entry| entry.u == ts)
            .map(|entry| entry.by.clone())
            .unwrap_or_else(|| state.device_id.clone());
        (ts, by)
    })
}

fn stamp_ref(stamp: &Option<(String, String)>) -> Option<(&str, &str)> {
    stamp
        .as_ref()
        .map(|(ts, by)| (ts.as_str(), by.as_str()))
}

fn debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("KXTODO_SYNC_DEBUG").map(|v| v != "0").unwrap_or(false)
    })
}

fn debug_log(message: String) {
    if debug_enabled() {
        eprintln!("[sync-debug] {message}");
    }
}

/// 在 write_data 闭包内做数据域的 LWW 应用；返回应用数。
fn merge_data_records(
    file: &mut crate::model::DataFile,
    records: &[EntityRecord],
    state: &SyncStateFile,
    warnings: &mut Vec<String>,
) -> usize {
    let mut applied = 0;
    for record in records {
        let stamp = local_stamp(state, &record.id, data_entity_stamp(file, &record.id));
        debug_log(format!(
            "merge data {} remote=({},{}) local={:?} -> {}",
            record.id,
            record.updated_at,
            record.updated_by,
            stamp,
            remote_wins(record, stamp_ref(&stamp))
        ));
        if remote_wins(record, stamp_ref(&stamp)) {
            match apply_data_record(record, file) {
                Ok(()) => applied += 1,
                Err(error) => warnings.push(format!("实体 {} 应用失败：{error}", record.id)),
            }
        }
    }
    if applied > 0 {
        normalize_data_orders(file);
    }
    applied
}

/// 完整同步：login（如需）→ pull → merge → push → 图片 → 推进水位。
///
/// 外层负责把「与服务端能不能通」的结论缓存进 runtime/sync.json：
/// 设置面板的 🟢/🔴 只读这份缓存，绝不为了显示状态而阻塞在网络上。
pub fn run_sync(repo: &Repository) -> CoreResult<SyncReport> {
    match run_sync_inner(repo) {
        Ok(report) => Ok(report),
        Err(error) => {
            if error.kind == crate::error::ErrorKind::Io {
                record_offline(repo, &error.message);
            }
            Err(error)
        }
    }
}

/// 把掉线结论写进状态文件（写失败静默：不能因为记录状态又抛一个新错误盖掉原因）。
fn record_offline(repo: &Repository, message: &str) {
    let mut state = load_state(&repo.layout);
    state.server_online = Some(false);
    state.last_error = Some(message.to_string());
    let _ = save_state(&repo.layout, &state);
}

/// 服务端没有图片 blob 路由（kxtodo-server < v0.5.0）：404/405/501 都按「不支持」处理。
fn server_lacks_image_api(error: &CoreError) -> bool {
    matches!(
        error.code.as_str(),
        "SYNC_HTTP_404" | "SYNC_HTTP_405" | "SYNC_HTTP_501"
    )
}

fn run_sync_inner(repo: &Repository) -> CoreResult<SyncReport> {
    let _guard = SyncRunLock::acquire(&repo.layout)?;
    let mut report = SyncReport {
        started_at: now_iso(),
        ..Default::default()
    };

    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    if !sync.enabled || sync.server_url.is_empty() {
        return Err(CoreError::conflict(
            "SYNC_NOT_CONFIGURED",
            "本机未配置同步（先 kxtodo-cli sync register / login）".to_string(),
        ));
    }
    let keys: SyncKeys = derive_keys(&sync.username, &sync.email, &sync.secret)?;
    let client = SyncClient::new(&sync.server_url)?;
    let mut state = load_state(&repo.layout);

    // 1. 确保 token
    if state.token.is_empty() || token_expired(&state.token_expires_at) {
        let (token, expires_at, _) = client.login(&sync.username, &sync.email, &keys.auth_key)?;
        state.token = token;
        state.token_expires_at = expires_at;
    }
    let token = state.token.clone();

    // 2. PULL（分页拉全量增量）
    let mut records: Vec<EntityRecord> = Vec::new();
    let mut current_seq = state.last_pulled_seq;
    loop {
        let page = client.changes(&token, current_seq)?;
        current_seq = page.current_seq;
        let count = page.entities.len();
        for item in page.entities {
            match decrypt_envelope(&keys.enc_key, &item) {
                Ok(record) => records.push(record),
                Err(error) => report
                    .warnings
                    .push(format!("实体 {} 拉取失败：{}", item.id, error.message)),
            }
        }
        if count < PAGE_LIMIT {
            break;
        }
    }
    report.pulled = records.len();

    let scopes = Scopes::from_settings(&settings);

    // 3. MERGE（按域分事务）
    let data_records: Vec<EntityRecord> = records
        .iter()
        .filter(|record| (record.kind == "node" || record.kind == "task") && scopes.data)
        .cloned()
        .collect();
    let settings_records: Vec<EntityRecord> = records
        .iter()
        .filter(|record| record.kind == "settings" && scopes.settings)
        .cloned()
        .collect();
    let schedule_records: Vec<EntityRecord> = records
        .iter()
        .filter(|record| record.kind == "schedule" && scopes.schedules)
        .cloned()
        .collect();

    if !data_records.is_empty() {
        let state_snapshot = state.clone();
        let records_snapshot = data_records.clone();
        let data_existed = repo.layout.data_file().exists();
        let mut applied = 0usize;
        let mut warnings: Vec<String> = Vec::new();
        let (_file, _outcome) = repo.write_data(None, None, "sync.pull", |file| {
            if !data_existed {
                // 全新设备：丢弃内存里的默认数据，直接落服务端内容
                file.nodes.clear();
                file.tasks.clear();
                file.backgrounds.clear();
            }
            applied = merge_data_records(file, &records_snapshot, &state_snapshot, &mut warnings);
            if !data_existed {
                backfill_system_nodes(file);
                if file
                    .nodes
                    .iter()
                    .any(|node| node.kind == crate::model::NodeKind::Entry)
                {
                    if file
                        .nodes
                        .iter()
                        .all(|node| node.id != file.selected_node_id)
                    {
                        let selected = file
                            .nodes
                            .iter()
                            .find(|node| node.kind == crate::model::NodeKind::Entry)
                            .map(|node| node.id.clone())
                            .unwrap_or_else(|| {
                                crate::model::SYSTEM_NODE_IDS[0].to_string()
                            });
                        file.selected_node_id = selected;
                    }
                } else {
                    file.selected_node_id =
                        crate::model::SYSTEM_NODE_IDS[0].to_string();
                }
            }
            Ok(json!({ "applied": applied, "pulled": records_snapshot.len() }))
        })?;
        report.applied += applied;
        report.warnings.extend(warnings);
    }

    if !settings_records.is_empty() {
        let state_snapshot = state.clone();
        let record = settings_records[0].clone();
        let mut applied = 0usize;
        let mut warnings: Vec<String> = Vec::new();
        let (_file, _outcome) = repo.write_settings(None, None, "sync.pull", |file| {
            let stamp = local_stamp(
                &state_snapshot,
                SETTINGS_ENTITY_ID,
                file.sync_updated_at.clone(),
            );
            if remote_wins(&record, stamp_ref(&stamp)) {
                match apply_record_settings(&record, file) {
                    Ok(()) => applied = 1,
                    Err(error) => warnings.push(format!("settings 实体应用失败：{error}")),
                }
            }
            Ok(json!({ "applied": applied }))
        })?;
        report.applied += applied;
        report.warnings.extend(warnings);
    }

    if !schedule_records.is_empty() {
        let state_snapshot = state.clone();
        let records_snapshot = schedule_records.clone();
        let mut applied = 0usize;
        let mut warnings: Vec<String> = Vec::new();
        let (_file, _outcome) = repo.write_schedule(None, None, "sync.pull", |file| {
            for record in &records_snapshot {
                let stamp = local_stamp(
                    &state_snapshot,
                    &record.id,
                    schedule_entity_stamp(file, &record.id),
                );
                if remote_wins(record, stamp_ref(&stamp)) {
                    match apply_schedule_record(record, file) {
                        Ok(()) => applied += 1,
                        Err(error) => warnings
                            .push(format!("schedule 实体 {} 应用失败：{error}", record.id)),
                    }
                }
            }
            if applied > 0 {
                normalize_schedule_orders(file);
            }
            Ok(json!({ "applied": applied }))
        })?;
        report.applied += applied;
        report.warnings.extend(warnings);
    }

    // 更新已拉取实体的对账水位（必须在全部合并事务之后，读的是合并后的最新文件）：
    // - 本地当前版本 == 拉到的版本（刚应用或本就相同）→ 以远端 (u, by, seq) 对账；
    // - 本地版本更新（本地胜出）→ 保留本地 (u, by)，仅刷新 seq 作为 OCC 基线。
    let data_after = repo.load_data()?;
    let settings_after = repo.load_settings()?;
    let schedule_after = repo.load_schedule()?;
    for record in &records {
        let local_ts = match record.kind.as_str() {
            "node" | "task" => data_entity_stamp(&data_after, &record.id),
            "schedule" => schedule_entity_stamp(&schedule_after, &record.id),
            "settings" => settings_after.sync_updated_at.clone(),
            _ => None,
        };
        debug_log(format!(
            "reconcile {} record=({},{}) local_ts={:?} seq={}",
            record.id, record.updated_at, record.updated_by, local_ts, record.seq
        ));
        if local_ts.as_deref() == Some(record.updated_at.as_str()) {
            state.set_entry(
                &record.id,
                PushedEntry {
                    seq: record.seq,
                    u: record.updated_at.clone(),
                    by: record.updated_by.clone(),
                },
            );
        } else if let Some(entry) = state.entry(&record.id) {
            // 本地胜出：保留本地 (u, by)，仅刷新 seq 作为下次推送的 OCC 基线
            if entry.seq != record.seq {
                state.set_entry_seq(&record.id, record.seq);
            }
        }
        // 其余情形（本地无实体/墓碑，或本地更新但从未对账）不登记：
        // 前者与推送无关，后者必须视为脏实体走推送（base=0 → 409 → 冲突自愈刷新基线）。
    }

    // 4. PUSH：本地版本戳 != 已对账版本戳 → 推送
    let data_existed = repo.layout.data_file().exists();
    let data = data_after;
    let settings_now = settings_after;
    let schedule = schedule_after;
    let local_entities = crate::sync::merge::extract_entities(
        &data,
        &settings_now,
        &schedule,
        &scopes,
        &state.device_id,
    )
    .into_iter()
    .filter(|entity| {
        // data.json 不存在（全新设备且服务端无数据域实体）→ 不推送内存默认数据
        if !data_existed && (entity.kind == "node" || entity.kind == "task") {
            return false;
        }
        true
    })
    .collect::<Vec<_>>();
    for entity in local_entities {
        let entry = state.entry(&entity.id);
        if entry
            .as_ref()
            .map(|e| e.u == entity.updated_at)
            .unwrap_or(false)
        {
            continue;
        }
        let base = entry.as_ref().map(|e| e.seq).unwrap_or(0);
        let (nonce, ciphertext, hash) = seal_record(&keys.enc_key, &entity)?;
        match client.put_entity(&token, &entity.id, base, &nonce, &ciphertext, &hash) {
            Ok(seq) => {
                state.set_entry(
                    &entity.id,
                    PushedEntry {
                        seq,
                        u: entity.updated_at.clone(),
                        by: state.device_id.clone(),
                    },
                );
                report.pushed += 1;
            }
            Err(PutError::Conflict(current_seq)) => {
                // 服务器版本比我们掌握的新：拉单实体重合并，本地仍胜则重推一次
                match client.get_entity(&token, &entity.id) {
                    Ok(Some(item)) => match decrypt_envelope(&keys.enc_key, &item) {
                        Ok(remote) => {
                            resolve_conflict(
                                repo, &client, &token, &keys, &mut state, &mut report,
                                &entity, remote, current_seq, &scopes,
                            )?;
                        }
                        Err(error) => {
                            report.conflicts += 1;
                            report
                                .warnings
                                .push(format!("冲突实体 {} 拉取失败：{}", entity.id, error.message));
                        }
                    },
                    Ok(None) => {
                        // 服务器上不存在（不应发生）：按新建重推
                        match client.put_entity(&token, &entity.id, 0, &nonce, &ciphertext, &hash) {
                            Ok(seq) => {
                                state.set_entry(
                                    &entity.id,
                                    PushedEntry {
                                        seq,
                                        u: entity.updated_at.clone(),
                                        by: state.device_id.clone(),
                                    },
                                );
                                report.pushed += 1;
                            }
                            Err(_) => report.conflicts += 1,
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(PutError::Api(error)) => return Err(error),
        }
    }

    // 4.5 图片 blob：markdown 插图 / 列表背景 / 头像的文件本体
    if settings.sync.sync_images {
        let device_id = state.device_id.clone();
        let scope = format!("{}|{}", sync.server_url, sync.username);
        match crate::sync::images::sync_images(
            &client,
            &token,
            &keys.enc_key,
            &device_id,
            &scope,
            &repo.layout,
            &mut state,
        ) {
            Ok(tally) => {
                report.images_pulled = tally.pulled;
                report.images_pushed = tally.pushed;
                report.warnings.extend(tally.warnings);
            }
            // 滚动升级：老服务器（< v0.5.0）没有图片路由。数据同步必须照常，
            // 否则用户一升级客户端就整条同步断掉。
            Err(error) if server_lacks_image_api(&error) => {
                report.warnings.push(
                    "服务器不支持图片同步（需升级 kxtodo-server 到 v0.5.0+），本轮只同步了数据"
                        .to_string(),
                );
            }
            Err(error) => return Err(error),
        }
    }

    // 5. 保存水位与报告
    state.last_pulled_seq = current_seq;
    state.last_sync_at = Some(now_iso());
    state.server_online = Some(true);
    state.last_seen_at = state.last_sync_at.clone();
    state.last_error = None;
    report.finished_at = now_iso();
    state.last_result = Some(serde_json::to_value(&report)?);
    save_state(&repo.layout, &state)?;
    Ok(report)
}

/// 409 冲突自愈：单实体重合并 → 远端胜出则以远端对账，本地胜出则重推一次。
#[allow(clippy::too_many_arguments)]
fn resolve_conflict(
    repo: &Repository,
    client: &SyncClient,
    token: &str,
    keys: &SyncKeys,
    state: &mut SyncStateFile,
    report: &mut SyncReport,
    entity: &EntityRecord,
    remote: EntityRecord,
    current_seq: u64,
    scopes: &Scopes,
) -> CoreResult<()> {
    let state_snapshot = state.clone();
    let mut warnings: Vec<String> = Vec::new();
    match entity.kind.as_str() {
        "node" | "task" => {
            let records = vec![remote.clone()];
            let _ = repo.write_data(None, None, "sync.conflict", |file| {
                merge_data_records(file, &records, &state_snapshot, &mut warnings);
                Ok(json!({}))
            });
        }
        "schedule" => {
            let record = remote.clone();
            let _ = repo.write_schedule(None, None, "sync.conflict", |file| {
                let stamp = local_stamp(
                    &state_snapshot,
                    &record.id,
                    schedule_entity_stamp(file, &record.id),
                );
                if remote_wins(&record, stamp_ref(&stamp)) {
                    let _ = apply_schedule_record(&record, file);
                }
                Ok(json!({}))
            });
        }
        "settings" => {
            let record = remote.clone();
            let _ = repo.write_settings(None, None, "sync.conflict", |file| {
                let stamp = local_stamp(
                    &state_snapshot,
                    SETTINGS_ENTITY_ID,
                    file.sync_updated_at.clone(),
                );
                if remote_wins(&record, stamp_ref(&stamp)) {
                    let _ = apply_record_settings(&record, file);
                }
                Ok(json!({}))
            });
        }
        _ => {}
    }
    report.warnings.extend(warnings);

    // 重新提取该实体：远端胜出 → 以远端对账；本地仍胜 → 重推一次
    let data = repo.load_data()?;
    let settings = repo.load_settings()?;
    let schedule = repo.load_schedule()?;
    let entities = crate::sync::merge::extract_entities(
        &data,
        &settings,
        &schedule,
        scopes,
        &state.device_id,
    );
    let Some(entity_now) = entities.iter().find(|e| e.id == entity.id) else {
        // 本地实体消失（远端墓碑胜出）→ 以远端对账
        state.set_entry(
            &entity.id,
            PushedEntry {
                seq: current_seq,
                u: remote.updated_at.clone(),
                by: remote.updated_by.clone(),
            },
        );
        return Ok(());
    };
    if entity_now.updated_at == remote.updated_at {
        state.set_entry(
            &entity.id,
            PushedEntry {
                seq: current_seq,
                u: remote.updated_at.clone(),
                by: remote.updated_by.clone(),
            },
        );
        return Ok(());
    }
    let (nonce, ciphertext, hash) = seal_record(&keys.enc_key, entity_now)?;
    match client.put_entity(token, &entity.id, current_seq, &nonce, &ciphertext, &hash) {
        Ok(seq) => {
            state.set_entry(
                &entity.id,
                PushedEntry {
                    seq,
                    u: entity_now.updated_at.clone(),
                    by: state.device_id.clone(),
                },
            );
            report.pushed += 1;
        }
        Err(PutError::Conflict(_)) | Err(PutError::Api(_)) => {
            report.conflicts += 1;
            report
                .warnings
                .push(format!("实体 {} 与远端连续冲突，本轮跳过（下次同步重试）", entity.id));
        }
    }
    Ok(())
}

/// 注册新账户并配对（register = 创建账户 + login + 首次同步）。
/// 全新设备注册新账户时先播种默认数据（收集箱等），作为账户的初始内容。
pub fn register_device(
    repo: &Repository,
    server_url: &str,
    username: &str,
    email: &str,
    secret: &str,
    scopes: Option<Scopes>,
) -> CoreResult<(String, SyncReport)> {
    let keys = derive_keys(username, email, secret)?;
    let client = SyncClient::new(server_url)?;
    let user_id = client.register(username, email, &keys.auth_key)?;
    if !repo.layout.data_file().exists() {
        repo.ensure_initialized()?;
    }
    let (device_id, report) = pair_device(repo, server_url, username, email, secret, scopes)?;
    let _ = user_id;
    Ok((device_id, report))
}

/// 登录既有账户并配对（login = challenge-response + 首次同步）。
pub fn pair_device(
    repo: &Repository,
    server_url: &str,
    username: &str,
    email: &str,
    secret: &str,
    scopes: Option<Scopes>,
) -> CoreResult<(String, SyncReport)> {
    let keys = derive_keys(username, email, secret)?;
    let client = SyncClient::new(server_url)?;
    let (token, expires_at, _) = client.login(username, email, &keys.auth_key)?;
    let device_id = crate::ids::gen_device_id();
    // 重新配对：进程内「图片已齐全」的旧结论作废（服务端数据可能被删过）
    crate::sync::images::invalidate_manifest_cache();
    // 已有本地设置才推送设置实体；全新设备不把默认设置推上去覆盖服务端。
    let settings_existed = repo.layout.settings_file().exists();

    let (_file, _outcome) = repo.write_settings(None, None, "sync.pair", |file| {
        file.sync.enabled = true;
        file.sync.server_url = server_url.trim().trim_end_matches('/').to_string();
        file.sync.username = username.trim().to_lowercase();
        file.sync.email = email.trim().to_lowercase();
        file.sync.secret = secret.to_string();
        if let Some(scopes) = scopes {
            file.sync.sync_data = scopes.data;
            file.sync.sync_settings = scopes.settings;
            file.sync.sync_schedules = scopes.schedules;
        }
        if file.sync.sync_settings && file.sync_updated_at.is_none() && settings_existed {
            file.sync_updated_at = Some(now_iso());
        }
        Ok(json!({ "paired": true }))
    })?;

    let mut state = SyncStateFile::fresh(device_id.clone());
    state.token = token;
    state.token_expires_at = expires_at;
    save_state(&repo.layout, &state)?;

    let report = run_sync(repo)?;
    // 空账户 + 全新设备：兜底播种默认数据（下一次同步会上传）
    if !repo.layout.data_file().exists() {
        repo.ensure_initialized()?;
    }
    Ok((device_id, report))
}

/// 用当前 token（必要时重新登录）查询账户信息。
pub fn fetch_me(repo: &Repository) -> CoreResult<Option<Value>> {
    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    if !sync.enabled || sync.server_url.is_empty() {
        return Ok(None);
    }
    let keys = derive_keys(&sync.username, &sync.email, &sync.secret)?;
    let client = SyncClient::new(&sync.server_url)?;
    let mut state = load_state(&repo.layout);
    if state.token.is_empty() || token_expired(&state.token_expires_at) {
        let (token, expires_at, _) = client.login(&sync.username, &sync.email, &keys.auth_key)?;
        state.token = token;
        state.token_expires_at = expires_at;
        save_state(&repo.layout, &state)?;
    }
    Ok(Some(client.me(&state.token)?))
}

/// 轻量连通性探测（设置面板用）：短超时 /healthz，通过后再取一次 /me。
///
/// 结论写进 `runtime/sync.json`（serverOnline / lastSeenAt / lastError），
/// 于是 `sync status` 可以完全不碰网络——打开设置界面不再被卡住。
pub fn probe_connection(repo: &Repository) -> CoreResult<Value> {
    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    if !sync.enabled || sync.server_url.is_empty() {
        return Err(CoreError::conflict(
            "SYNC_NOT_CONFIGURED",
            "本机未配置同步（先 kxtodo-cli sync register / login）".to_string(),
        ));
    }
    let mut state = load_state(&repo.layout);
    let mut out = json!({ "serverUrl": sync.server_url });
    let health = crate::sync::discovery::probe_health(&sync.server_url);
    let online = health.is_ok();
    match health {
        Ok(value) => {
            state.server_online = Some(true);
            state.last_seen_at = Some(now_iso());
            state.last_error = None;
            out["server"] = value;
        }
        Err(error) => {
            state.server_online = Some(false);
            state.last_error = Some(error.message.clone());
            out["serverError"] = json!(error.message);
        }
    }
    let _ = save_state(&repo.layout, &state);
    if online {
        // 账户信息失败（token 过期/密钥不符）不代表服务器掉线，单独报
        match fetch_me(repo) {
            Ok(Some(me)) => {
                out["account"] = me;
            }
            Ok(None) => {}
            Err(error) => {
                out["accountError"] = json!(error.message);
            }
        }
    }
    out["online"] = json!(online);
    Ok(out)
}

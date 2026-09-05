//! HTTP API（/api/v1）：注册、挑战应答登录、实体读写。
//! 服务器不理解业务数据：只保管密文与版本号。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db::{entity_to_json, image_to_json, Db, UserRow};
use crate::error::{ServerError, ServerResult};
use crate::logging::Logger;
use crate::settings::ServerSettings;
use crate::util;

pub const APP_VERSION: &str = env!("KXTODO_VERSION");
/// 登录 token 有效期固定 30 天（个人同步服务，无需可调）
pub const TOKEN_TTL_DAYS: i64 = 30;
/// 单张图片密文上限（客户端侧有一份同样的限制）
const MAX_IMAGE_BYTES: usize = 96 * 1024 * 1024;

pub struct AppState {
    pub db: Db,
    pub logger: Mutex<Logger>,
    pub settings: ServerSettings,
    /// 进程内运行指标（本次运行的请求/写入/每用户活动），供管理台展示
    pub metrics: crate::metrics::Metrics,
    /// 登录挑战 nonce → (user_id, 过期时间)
    pub challenges: Mutex<HashMap<String, (String, String)>>,
    /// 管理界面 session token → 过期时间
    pub admin_sessions: Mutex<HashMap<String, String>>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    /// 关键操作：stdout + 持久化日志 + 管理台「操作日志」。
    pub fn log(&self, kind: &str, message: &str) {
        if let Ok(mut logger) = self.logger.lock() {
            logger.log(kind, message);
        }
    }

    /// 高频噪音（每请求访问行、周期空同步、发现应答）：只进 stdout，不落盘。
    pub fn console(&self, kind: &str, message: &str) {
        if let Ok(mut logger) = self.logger.lock() {
            logger.console(kind, message);
        }
    }
}

pub fn router(state: SharedState) -> Router {
    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/register", post(register))
        .route("/api/v1/login-challenge", post(login_challenge))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/me", get(me))
        .route("/api/v1/changes", get(changes))
        .route("/api/v1/entities/{id}", get(get_entity).put(put_entity))
        .route("/api/v1/images/changes", get(image_changes))
        .route("/api/v1/images/check", post(image_check))
        .route("/api/v1/images/{id}", get(get_image).put(put_image))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_log,
        ))
        .layer(DefaultBodyLimit::max(128 * 1024 * 1024))
        .with_state(state.clone());
    // 管理台自带 /admin 前缀（同时注册 /admin 与 /admin/），所以用 merge 而不是 nest
    api.merge(
        crate::admin::router(state.clone())
            .layer(middleware::from_fn_with_state(state, request_log)),
    )
}

/// 请求访问行：method path status 耗时 客户端IP。
///
/// **只进 stdout**：客户端自动同步最短 5 秒一轮，每轮至少两三个请求，
/// 落盘的话一天就是十几万行纯噪音（持久化日志只留关键写操作）。
async fn request_log(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let client_ip = client_ip(request.headers(), Some(&connect));
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    state.metrics.hit(method.as_str(), &path, status < 400);
    // 健康检查连 stdout 都不刷（探测/发现复核会频繁打它）
    if path != "/healthz" {
        state.console(
            "req",
            &format!(
                "{method} {path} -> {status}（{}ms）{client_ip}",
                started.elapsed().as_millis()
            ),
        );
    }
    response
}

/// X-Forwarded-For 里的第一个地址（反代场景），没有则空。
fn client_ip_value(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|forwarded| forwarded.split(',').next())
        .map(|first| first.trim().to_string())
        .filter(|first| !first.is_empty())
        .unwrap_or_default()
}

/// 真实来源 IP：反代给的 XFF 优先，否则用 TCP 连接的对端地址
/// （局域网直连没有 XFF，靠 ConnectInfo 才拿得到）。
fn peer_ip(headers: &HeaderMap, connect: Option<&ConnectInfo<SocketAddr>>) -> String {
    let forwarded = client_ip_value(headers);
    if !forwarded.is_empty() {
        return forwarded;
    }
    connect
        .map(|info| info.0.ip().to_string())
        .unwrap_or_default()
}

fn client_ip(headers: &HeaderMap, connect: Option<&ConnectInfo<SocketAddr>>) -> String {
    let ip = peer_ip(headers, connect);
    if ip.is_empty() {
        String::new()
    } else {
        format!(" ip={ip}")
    }
}

// ---------------------------------------------------------------------------
// 错误 → HTTP 响应
// ---------------------------------------------------------------------------

fn error_response(error: ServerError) -> Response {
    let mut body = json!({
        "error": {
            "code": error.code,
            "message": error.message,
        }
    });
    if let Some(details) = &error.details {
        if let (Some(target), Some(source)) = (body.as_object_mut(), details.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    (
        StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(body),
    )
        .into_response()
}

type ApiResult<T> = Result<T, ServerError>;

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        error_response(self)
    }
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

async fn healthz(State(state): State<SharedState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "name": state.settings.name,
        "version": APP_VERSION,
        "serverTime": util::now_iso(),
    }))
}

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    #[serde(rename = "authKey")]
    auth_key: String,
}

async fn register(
    State(state): State<SharedState>,
    Json(body): Json<RegisterBody>,
) -> ApiResult<Response> {
    let username = body.username.trim().to_lowercase();
    if username.is_empty() {
        return Err(ServerError::bad_request("username 不能为空"));
    }
    if body.auth_key.len() != 64 || !body.auth_key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ServerError::bad_request("authKey 应为 64 位 hex"));
    }
    let user_id = state.db.create_user(&username, &body.auth_key)?;
    state.log("op", &format!("注册账户：{username}（{user_id}）"));
    Ok((StatusCode::CREATED, Json(json!({ "userId": user_id }))).into_response())
}

#[derive(Deserialize)]
struct AccountBody {
    username: String,
}

async fn login_challenge(
    State(state): State<SharedState>,
    Json(body): Json<AccountBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let username = body.username.trim().to_lowercase();
    let user = state
        .db
        .find_user(&username)?
        .ok_or_else(ServerError::account_not_found)?;
    let nonce = util::random_hex(32);
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    state
        .challenges
        .lock()
        .unwrap()
        .insert(nonce.clone(), (user.id, expires));
    Ok(Json(json!({ "nonce": nonce })))
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    nonce: String,
    proof: String,
}

async fn login(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let username = body.username.trim().to_lowercase();
    let ip = peer_ip(&headers, Some(&connect));
    let challenge = state
        .challenges
        .lock()
        .unwrap()
        .remove(&body.nonce)
        .ok_or_else(ServerError::challenge_invalid)?;
    let (_, expires) = &challenge;
    if *expires < util::now_iso() {
        return Err(ServerError::challenge_invalid());
    }
    let user = state
        .db
        .find_user(&username)?
        .ok_or_else(ServerError::auth_failed)?;
    if user.id != challenge.0 {
        return Err(ServerError::challenge_invalid());
    }
    // proof = hex(HMAC-SHA256(auth_key, nonce))；常数时间比较
    let auth_key_bytes = hex::decode(&user.auth_key)
        .map_err(|_| ServerError::internal("auth_key 存储损坏"))?;
    let expected = hex::encode(kxtodo_core::sync::crypto::hmac_sha256(
        &auth_key_bytes,
        body.nonce.as_bytes(),
    ));
    if !util::constant_time_eq(&expected, &body.proof) {
        state.log("op", &format!("登录失败（proof 不匹配）：{username}，ip={ip}"));
        return Err(ServerError::auth_failed());
    }

    let token = util::random_hex(32);
    let token_hash = util::sha256_hex(token.as_bytes());
    let expires_at = util::iso_after_days(TOKEN_TTL_DAYS);
    state.db.insert_token(&token_hash, &user.id, &expires_at)?;
    state.metrics.user_event(
        &user.id,
        &user.username,
        crate::metrics::UserEvent::Login,
        &ip,
    );
    state.log(
        "op",
        &format!("登录成功：{username}（{}），token 有效期至 {expires_at}", user.id),
    );
    Ok(Json(json!({
        "token": token,
        "expiresAt": expires_at,
        "currentSeq": user.current_seq,
        "serverTime": util::now_iso(),
    })))
}

fn auth_user(state: &SharedState, headers: &HeaderMap) -> ServerResult<UserRow> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(ServerError::unauthorized)?;
    if token.is_empty() {
        return Err(ServerError::unauthorized());
    }
    let token_hash = util::sha256_hex(token.as_bytes());
    state
        .db
        .user_by_token(&token_hash)?
        .ok_or_else(|| ServerError::unauthorized())
}

async fn logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(ServerError::unauthorized)?;
    state.db.delete_token(&util::sha256_hex(token.as_bytes()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    let entities = state.db.entity_count(&user.id)?;
    let entity_bytes = state.db.storage_bytes(&user.id)?;
    let images = state.db.image_count(&user.id)?;
    let image_bytes = state.db.image_bytes(&user.id)?;
    Ok(Json(json!({
        "userId": user.id,
        "username": user.username,
        "currentSeq": user.current_seq,
        "entityCount": entities,
        "imageCount": images,
        "imageBytes": image_bytes,
        "storageBytes": entity_bytes + image_bytes,
        "serverTime": util::now_iso(),
        "serverVersion": APP_VERSION,
    })))
}

#[derive(Deserialize)]
struct ChangesQuery {
    since: Option<u64>,
    limit: Option<u64>,
}

async fn changes(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(500).clamp(1, 2000);
    let (rows, current_seq) = state.db.changes_since(&user.id, since, limit)?;
    state.metrics.user_event(
        &user.id,
        &user.username,
        crate::metrics::UserEvent::Pull,
        &peer_ip(&headers, Some(&connect)),
    );
    Ok(Json(json!({
        "entities": rows.iter().map(entity_to_json).collect::<Vec<_>>(),
        "currentSeq": current_seq,
    })))
}

async fn get_entity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    let row = state
        .db
        .get_entity(&user.id, &id)?
        .ok_or_else(ServerError::not_found)?;
    Ok(Json(entity_to_json(&row)))
}

#[derive(Deserialize)]
struct PutBody {
    base: u64,
    nonce: String,
    ciphertext: String,
    hash: String,
}

async fn put_entity(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PutBody>,
) -> ApiResult<Response> {
    let user = auth_user(&state, &headers)?;
    if body.nonce.is_empty() || body.ciphertext.is_empty() || body.hash.is_empty() {
        return Err(ServerError::bad_request("nonce/ciphertext/hash 不能为空"));
    }
    if id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(ServerError::bad_request("实体 ID 只允许字母数字-_."));
    }
    match state
        .db
        .put_entity(&user.id, &id, body.base, &body.nonce, &body.ciphertext, &body.hash)
    {
        Ok((seq, changed)) => {
            if changed {
                state.metrics.wrote();
                state.metrics.user_event(
                    &user.id,
                    &user.username,
                    crate::metrics::UserEvent::Push,
                    &peer_ip(&headers, Some(&connect)),
                );
                // 只有真的改动了才落持久化日志：周期同步里的幂等重放不值得留痕
                state.log(
                    "op",
                    &format!(
                        "写入实体：{id}（{}，base={} → seq={seq}，密文 {} 字节）by {}",
                        if body.base == 0 { "新建" } else { "更新" },
                        body.base,
                        body.ciphertext.len(),
                        user.username,
                    ),
                );
            }
            Ok(Json(json!({ "seq": seq, "changed": changed })).into_response())
        }
        Err(error) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// 图片 blob 通道（v0.5.0）
//
// 图片是内容寻址的不可变 blob：没有 OCC/base，靠 content_hash 幂等。
// 密文走裸字节体（不做 base64 膨胀），元数据走 query 参数与响应头。
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ImageChangesQuery {
    since: Option<u64>,
    limit: Option<u64>,
}

async fn image_changes(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(query): Query<ImageChangesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let (rows, current_seq) = state.db.image_changes_since(&user.id, since, limit)?;
    Ok(Json(json!({
        "images": rows.iter().map(image_to_json).collect::<Vec<_>>(),
        "currentSeq": current_seq,
    })))
}

#[derive(Deserialize)]
struct ImageCheckBody {
    #[serde(default)]
    images: Vec<ImageCheckItem>,
}

#[derive(Deserialize)]
struct ImageCheckItem {
    id: String,
    #[serde(default)]
    hash: String,
}

/// 客户端上报本地 (id, 内容哈希) 清单，服务端回「我缺哪些」。
async fn image_check(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<ImageCheckBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    if body.images.len() > 20_000 {
        return Err(ServerError::bad_request("单次对账清单过大"));
    }
    let mut items = Vec::with_capacity(body.images.len());
    for item in body.images {
        validate_image_id(&item.id)?;
        items.push((item.id, item.hash));
    }
    let needed = state.db.images_needed(&user.id, &items)?;
    Ok(Json(json!({ "needed": needed })))
}

async fn get_image(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let user = auth_user(&state, &headers)?;
    validate_image_id(&id)?;
    let row = state
        .db
        .get_image(&user.id, &id)?
        .ok_or_else(ServerError::not_found)?;
    state.metrics.user_event(
        &user.id,
        &user.username,
        crate::metrics::UserEvent::ImagePull,
        &peer_ip(&headers, Some(&connect)),
    );
    let ciphertext = row.ciphertext.unwrap_or_default();
    let mut response_headers = HeaderMap::new();
    let nonce = axum::http::HeaderValue::from_str(&row.nonce)
        .map_err(|_| ServerError::internal("nonce 存储损坏"))?;
    response_headers.insert("x-kxtodo-nonce", nonce);
    if let Ok(hash) = axum::http::HeaderValue::from_str(&row.content_hash) {
        response_headers.insert("x-kxtodo-hash", hash);
    }
    response_headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/octet-stream"),
    );
    Ok((response_headers, ciphertext).into_response())
}

#[derive(Deserialize)]
struct ImagePutQuery {
    kind: String,
    #[serde(rename = "nodeId", default)]
    node_id: String,
    filename: String,
    nonce: String,
    hash: String,
    #[serde(rename = "updatedAt", default)]
    updated_at: Option<String>,
    #[serde(rename = "updatedBy", default)]
    updated_by: Option<String>,
}

async fn put_image(
    State(state): State<SharedState>,
    connect: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ImagePutQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    let user = auth_user(&state, &headers)?;
    validate_image_id(&id)?;
    validate_image_kind(&query.kind)?;
    validate_path_component(&query.filename, "filename", false)?;
    // 只有 markdown 插图带条目 ID，其余类别必须为空
    let node_id = if query.kind == "entry" {
        validate_path_component(&query.node_id, "nodeId", false)?;
        query.node_id.clone()
    } else {
        String::new()
    };
    if !is_hex(&query.nonce) || query.nonce.len() != 48 {
        return Err(ServerError::bad_request("nonce 应为 48 位 hex（24 字节）"));
    }
    if !is_hex(&query.hash) || query.hash.len() != 64 {
        return Err(ServerError::bad_request("hash 应为 64 位 hex"));
    }
    if body.is_empty() || body.len() > MAX_IMAGE_BYTES {
        return Err(ServerError::bad_request("图片密文为空或超出上限"));
    }
    let updated_at = query.updated_at.unwrap_or_else(util::now_iso);
    let updated_by = query.updated_by.unwrap_or_else(|| user.id.clone());
    let (seq, changed) = state.db.put_image(
        &user.id,
        &id,
        &query.kind,
        &node_id,
        &query.filename,
        &query.nonce,
        &body,
        &query.hash,
        &updated_at,
        &updated_by,
    )?;
    if changed {
        state.metrics.wrote();
        state.metrics.user_event(
            &user.id,
            &user.username,
            crate::metrics::UserEvent::ImagePush,
            &peer_ip(&headers, Some(&connect)),
        );
        state.log(
            "op",
            &format!(
                "写入图片：{}（{}{}，密文 {} 字节 → seq={seq}）by {}",
                query.filename,
                query.kind,
                if node_id.is_empty() {
                    String::new()
                } else {
                    format!(" / {node_id}")
                },
                body.len(),
                user.username,
            ),
        );
    }
    Ok(Json(json!({ "seq": seq, "changed": changed })).into_response())
}

fn is_hex(raw: &str) -> bool {
    !raw.is_empty() && raw.chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_image_id(id: &str) -> ServerResult<()> {
    if id.len() != 64 || !is_hex(id) {
        return Err(ServerError::bad_request(
            "图片 ID 应为 64 位 hex（sha256(kind|nodeId|filename)）",
        ));
    }
    Ok(())
}

fn validate_image_kind(kind: &str) -> ServerResult<()> {
    match kind {
        "entry" | "background" | "avatar" => Ok(()),
        _ => Err(ServerError::bad_request(
            "图片类别只允许 entry/background/avatar",
        )),
    }
}

/// 客户端会拿这些字段拼本地路径，必须挡掉穿越与分隔符。
fn validate_path_component(raw: &str, what: &str, allow_empty: bool) -> ServerResult<()> {
    if raw.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(ServerError::bad_request(format!("{what} 不能为空")))
        };
    }
    if raw.len() > 160
        || raw.contains('/')
        || raw.contains('\\')
        || raw.contains("..")
        || raw.starts_with('.')
        || raw.chars().any(|c| c.is_control())
    {
        return Err(ServerError::bad_request(format!("{what} 含非法字符")));
    }
    Ok(())
}

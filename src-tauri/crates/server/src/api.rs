//! HTTP API（/api/v1）：注册、挑战应答登录、实体读写。
//! 服务器不理解业务数据：只保管密文与版本号。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db::{entity_to_json, Db, UserRow};
use crate::error::{ServerError, ServerResult};
use crate::util;

pub const APP_VERSION: &str = env!("KXTODO_VERSION");

pub struct AppState {
    pub db: Db,
    pub token_ttl_days: i64,
    /// 登录挑战 nonce → (user_id, 过期时间)
    pub challenges: Mutex<HashMap<String, (String, String)>>,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/register", post(register))
        .route("/api/v1/login-challenge", post(login_challenge))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/me", get(me))
        .route("/api/v1/changes", get(changes))
        .route("/api/v1/entities/{id}", get(get_entity).put(put_entity))
        .with_state(state)
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

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "version": APP_VERSION,
        "serverTime": util::now_iso(),
    }))
}

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    email: String,
    #[serde(rename = "authKey")]
    auth_key: String,
}

async fn register(
    State(state): State<SharedState>,
    Json(body): Json<RegisterBody>,
) -> ApiResult<Response> {
    let username = body.username.trim().to_lowercase();
    let email = body.email.trim().to_lowercase();
    if username.is_empty() || email.is_empty() {
        return Err(ServerError::bad_request("username/email 不能为空"));
    }
    if body.auth_key.len() != 64 || !body.auth_key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ServerError::bad_request("authKey 应为 64 位 hex"));
    }
    let user_id = state.db.create_user(&username, &email, &body.auth_key)?;
    Ok((StatusCode::CREATED, Json(json!({ "userId": user_id }))).into_response())
}

#[derive(Deserialize)]
struct AccountBody {
    username: String,
    email: String,
}

async fn login_challenge(
    State(state): State<SharedState>,
    Json(body): Json<AccountBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let username = body.username.trim().to_lowercase();
    let email = body.email.trim().to_lowercase();
    let user = state
        .db
        .find_user(&username, &email)?
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
    email: String,
    nonce: String,
    proof: String,
}

async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let username = body.username.trim().to_lowercase();
    let email = body.email.trim().to_lowercase();
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
        .find_user(&username, &email)?
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
        return Err(ServerError::auth_failed());
    }

    let token = util::random_hex(32);
    let token_hash = util::sha256_hex(token.as_bytes());
    let expires_at = util::iso_after_days(state.token_ttl_days);
    state.db.insert_token(&token_hash, &user.id, &expires_at)?;
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
    let bytes = state.db.storage_bytes(&user.id)?;
    Ok(Json(json!({
        "userId": user.id,
        "username": user.username,
        "email": user.email,
        "currentSeq": user.current_seq,
        "entityCount": entities,
        "storageBytes": bytes,
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
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = auth_user(&state, &headers)?;
    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(500).clamp(1, 2000);
    let (rows, current_seq) = state.db.changes_since(&user.id, since, limit)?;
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
        Ok(seq) => Ok(Json(json!({ "seq": seq, "changed": true })).into_response()),
        Err(error) => Err(error),
    }
}

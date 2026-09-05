//! 管理界面（/admin）：内嵌单页控制台 + 管理 API。
//!
//! 登录：管理员账密（启动参数指定并持久化哈希到 settings.json）→ 签发 session cookie。
//! 能力：概览与实时活动、用户与遗留账户、每用户的实体/图片明细、数据库内容与体积、
//! 持久化操作日志、在线 token、删除用户及其全部数据。
//! 服务器只保管密文，管理台看到的也只是元数据与体积——不解密任何内容。

use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::api::SharedState;
use crate::error::{ServerError, ServerResult};
use crate::settings;
use crate::util;

/// 管理会话有效期 12 小时
const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);

/// 管理台路由。**用绝对路径 + 上层 merge**，不用 `nest("/admin", …)`：
/// nest 只匹配 `/admin`，用户手输 `/admin/` 会 404，这里两个都注册。
pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/admin", get(admin_page))
        .route("/admin/", get(admin_page))
        .route("/admin/login", post(admin_login))
        .route("/admin/logout", post(admin_logout))
        .route("/admin/api/dashboard", get(dashboard))
        .route("/admin/api/users/{id}/entities", get(user_entities))
        .route("/admin/api/users/{id}/images", get(user_images))
        .route("/admin/api/users/{id}", axum::routing::delete(delete_user))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// session 管理
// ---------------------------------------------------------------------------

fn issue_session(state: &SharedState) -> (String, String) {
    let token = util::random_hex(32);
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(SESSION_TTL.as_secs() as i64))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    state
        .admin_sessions
        .lock()
        .unwrap()
        .insert(token.clone(), expires);
    // cookie：HttpOnly + SameSite=Lax；管理面与 API 同源
    let cookie = format!("kxtodo_admin={token}; Path=/admin; HttpOnly; SameSite=Lax; Max-Age={}", SESSION_TTL.as_secs());
    (token, cookie)
}

fn session_token(headers: &HeaderMap) -> &str {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("kxtodo_admin="))
        .next()
        .unwrap_or("")
}

fn session_ok(state: &SharedState, headers: &HeaderMap) -> bool {
    let token = session_token(headers);
    if token.is_empty() {
        return false;
    }
    let sessions = state.admin_sessions.lock().unwrap();
    match sessions.get(token) {
        Some(expires) => *expires > util::now_iso(),
        None => false,
    }
}

fn require_admin(state: &SharedState, headers: &HeaderMap) -> ServerResult<()> {
    if session_ok(state, headers) {
        Ok(())
    } else {
        Err(ServerError::unauthorized())
    }
}

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

async fn admin_page(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !session_ok(&state, &headers) {
        return Html(include_str!("admin_login.html")).into_response();
    }
    Html(include_str!("admin_console.html")).into_response()
}

#[derive(Deserialize)]
struct AdminLoginBody {
    username: String,
    password: String,
}

async fn admin_login(
    State(state): State<SharedState>,
    Json(body): Json<AdminLoginBody>,
) -> ServerResult<Response> {
    let expected_user = state.settings.admin_user.clone();
    let expected_hash = state.settings.admin_password_hash.clone();
    let salt = state.settings.admin_password_salt.clone().unwrap_or_default();
    let ok = !expected_user.is_empty()
        && util::constant_time_eq(body.username.trim(), &expected_user)
        && settings::verify_password(&body.password, &expected_hash, &salt);
    if !ok {
        state.log(
            "op",
            &format!("管理员登录失败（用户名 `{}`）", body.username.trim()),
        );
        return Err(ServerError::auth_failed());
    }
    let (_, cookie) = issue_session(&state);
    state.log("op", "管理员登录成功");
    Ok((
        StatusCode::OK,
        [("set-cookie", cookie)],
        Json(json!({ "ok": true })),
    )
        .into_response())
}

async fn admin_logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ServerResult<StatusCode> {
    let token = session_token(&headers).to_string();
    if !token.is_empty() {
        state.admin_sessions.lock().unwrap().remove(&token);
    }
    state.log("op", "管理员退出登录");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 管理 API
// ---------------------------------------------------------------------------

/// 一次取齐控制台首屏需要的全部数据（个人服务器，数据量小，一个往返最省事）。
async fn dashboard(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let db_path = std::path::PathBuf::from(&state.settings.db);
    let wal_path = db_path.with_extension("db-wal");
    Ok(Json(json!({
        "version": crate::api::APP_VERSION,
        "serverTime": util::now_iso(),
        "settings": state.settings.to_json(),
        "stats": state.db.overview()?,
        "dbStats": state.db.db_stats()?,
        "files": {
            "dbPath": db_path.display().to_string(),
            "dbBytes": file_size(&db_path),
            "walBytes": file_size(&wal_path),
            "logDir": db_path.parent().map(|p| p.join("log").display().to_string().replace('\\', "/")).unwrap_or_default(),
        },
        "users": state.db.list_users()?,
        "legacyUsers": state.db.list_legacy_users()?,
        "tokens": state.db.list_tokens()?,
        "ops": recent_ops(&state),
        "metrics": state.metrics.snapshot(),
    })))
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn recent_ops(state: &SharedState) -> Vec<serde_json::Value> {
    let Ok(logger) = state.logger.lock() else {
        return Vec::new();
    };
    logger
        .recent()
        .into_iter()
        .map(|line| {
            json!({ "at": line.at, "kind": line.kind, "message": line.message })
        })
        .collect()
}

async fn user_entities(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let entities = state.db.list_user_entities(&id)?;
    Ok(Json(json!({ "entities": entities })))
}

async fn user_images(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let images = state.db.list_user_images(&id)?;
    Ok(Json(json!({ "images": images })))
}

async fn delete_user(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let (entities, images) = state.db.delete_user(&id)?;
    state.metrics.forget_user(&id);
    state.log(
        "op",
        &format!("管理员删除用户 {id}（清除实体 {entities} 个、图片 {images} 张）"),
    );
    Ok(Json(json!({
        "deleted": true,
        "entitiesRemoved": entities,
        "imagesRemoved": images,
    })))
}

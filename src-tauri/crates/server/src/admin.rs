//! 管理界面（/admin）：内嵌单页 HTML + 管理 API。
//! 登录：管理员账密（启动参数指定并持久化哈希到 settings.json）→ 签发 session cookie。
//! 能力：概览（用户/存储统计）、用户列表、实体浏览（密文元数据）、在线 token、
//! 危险操作（删除用户及其全部数据）。只管理服务器自身数据，不触碰密文内容。

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

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(admin_page))
        .route("/login", post(admin_login))
        .route("/logout", post(admin_logout))
        .route("/api/overview", get(admin_overview))
        .route("/api/users", get(admin_users))
        .route("/api/users/{id}", axum::routing::delete(delete_user))
        .route("/api/users/{id}/entities", get(user_entities))
        .route("/api/tokens", get(admin_tokens))
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

fn session_ok(state: &SharedState, headers: &HeaderMap) -> bool {
    let Some(cookie_header) = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let token = cookie_header
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("kxtodo_admin="))
        .next()
        .unwrap_or_default();
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
        return Html(login_page()).into_response();
    }
    Html(console_page()).into_response()
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
    if let Some(cookie_header) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        let token = cookie_header
            .split(';')
            .filter_map(|part| part.trim().strip_prefix("kxtodo_admin="))
            .next()
            .unwrap_or_default();
        if !token.is_empty() {
            state.admin_sessions.lock().unwrap().remove(token);
        }
    }
    state.log("op", "管理员退出登录");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 管理 API
// ---------------------------------------------------------------------------

async fn admin_overview(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let stats = state.db.overview()?;
    Ok(Json(json!({
        "version": crate::api::APP_VERSION,
        "serverTime": util::now_iso(),
        "settings": state.settings.to_json(),
        "stats": stats,
    })))
}

async fn admin_users(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let users = state.db.list_users()?;
    Ok(Json(json!({ "users": users })))
}

async fn delete_user(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let removed = state.db.delete_user(&id)?;
    state.log("op", &format!("管理员删除用户 {id}（{removed} 个实体已清除）"));
    Ok(Json(json!({ "deleted": true, "entitiesRemoved": removed })))
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

async fn admin_tokens(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ServerResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let tokens = state.db.list_tokens()?;
    Ok(Json(json!({ "tokens": tokens })))
}

// ---------------------------------------------------------------------------
// 内嵌页面（无外部资产的单页）
// ---------------------------------------------------------------------------

fn login_page() -> String {
    r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>KXToDo Server 管理</title>
<style>
body{font-family:system-ui,sans-serif;background:#f0f0f0;display:flex;justify-content:center;align-items:center;height:100vh;margin:0}
.card{background:#fff;border-radius:12px;padding:32px;box-shadow:0 2px 12px rgba(0,0,0,.08);width:320px}
h1{font-size:18px;margin:0 0 24px}
input{width:100%;box-sizing:border-box;padding:10px;border:1px solid #d0d0d0;border-radius:8px;margin-bottom:12px;font-size:14px}
button{width:100%;padding:10px;background:#2564cf;color:#fff;border:0;border-radius:8px;font-size:14px;cursor:pointer}
button:hover{background:#1d4fa8}
.err{color:#c0392b;font-size:13px;min-height:18px;margin-bottom:8px}
</style></head><body>
<div class="card">
<h1>KXToDo Server 管理登录</h1>
<div class="err" id="err"></div>
<input id="u" placeholder="管理员用户名" autocomplete="username">
<input id="p" type="password" placeholder="密码" autocomplete="current-password">
<button onclick="doLogin()">登录</button>
</div>
<script>
async function doLogin(){
  const err=document.getElementById('err');
  try{
    const r=await fetch('/admin/login',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({username:document.getElementById('u').value,password:document.getElementById('p').value})});
    if(r.ok){location.reload();return}
    const j=await r.json().catch(()=>({}));
    err.textContent=j.error?j.error.message:'登录失败';
  }catch(e){err.textContent='网络错误：'+e}
}
document.getElementById('p').addEventListener('keydown',e=>{if(e.key==='Enter')doLogin()});
</script></body></html>"#.to_string()
}

fn console_page() -> String {
    r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>KXToDo Server 管理</title>
<style>
*{box-sizing:border-box}
body{font-family:system-ui,sans-serif;background:#f0f0f0;margin:0;color:#222}
header{background:#2564cf;color:#fff;padding:14px 24px;display:flex;justify-content:space-between;align-items:center}
header h1{font-size:16px;margin:0}
header button{background:none;border:1px solid rgba(255,255,255,.5);color:#fff;border-radius:6px;padding:6px 14px;cursor:pointer}
main{max-width:1000px;margin:24px auto;padding:0 16px}
section{background:#fff;border-radius:12px;padding:20px;margin-bottom:16px;box-shadow:0 1px 4px rgba(0,0,0,.06)}
h2{font-size:14px;color:#666;margin:0 0 14px;font-weight:600}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:12px}
.stat{background:#f7f9fc;border-radius:10px;padding:14px}
.stat b{display:block;font-size:22px}
.stat span{font-size:12px;color:#888}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid #eee}
th{color:#888;font-weight:500}
td.mono{font-family:ui-monospace,monospace;font-size:12px;color:#555}
button.danger{background:#c0392b;color:#fff;border:0;border-radius:6px;padding:5px 10px;font-size:12px;cursor:pointer}
button.link{background:#eef3fb;color:#2564cf;border:0;border-radius:6px;padding:5px 10px;font-size:12px;cursor:pointer}
#detail{font-size:13px;max-height:320px;overflow:auto;background:#f7f9fc;border-radius:8px;padding:12px;font-family:ui-monospace,monospace;white-space:pre-wrap}
</style></head><body>
<header><h1>KXToDo Server 管理控制台</h1><button onclick="logout()">退出登录</button></header>
<main>
<section><h2>概览</h2><div class="grid" id="overview"></div>
<div style="margin-top:12px;font-size:13px;color:#666" id="settings"></div></section>
<section><h2>用户</h2><table><thead><tr><th>用户名</th><th>邮箱</th><th>实体数</th><th>存储</th><th>当前 seq</th><th>创建时间</th><th></th></tr></thead>
<tbody id="users"></tbody></table></section>
<section id="entitySection" style="display:none"><h2 id="entityTitle"></h2><div id="detail"></div></section>
<section><h2>在线 Token</h2><table><thead><tr><th>用户</th><th>token 摘要</th><th>过期时间</th></tr></thead>
<tbody id="tokens"></tbody></table></section>
</main>
<script>
const fmt=b=>b>1048576?(b/1048576).toFixed(1)+' MB':b>1024?(b/1024).toFixed(1)+' KB':b+' B';
async function api(path,opts){const r=await fetch(path,opts);if(r.status===401){location.reload();throw new Error('未登录')}return r}
async function load(){
  const o=await(await api('/admin/api/overview')).json();
  document.getElementById('overview').innerHTML=
    `<div class="stat"><b>${o.stats.users}</b><span>用户</span></div>`+
    `<div class="stat"><b>${o.stats.entities}</b><span>实体</span></div>`+
    `<div class="stat"><b>${fmt(o.stats.storageBytes)}</b><span>密文存储</span></div>`+
    `<div class="stat"><b>${o.stats.tokens}</b><span>在线 token</span></div>`+
    `<div class="stat"><b>v${o.version}</b><span>服务版本</span></div>`;
  document.getElementById('settings').textContent=
    `监听 ${o.settings.listen} · 数据库 ${o.settings.db} · 管理员 ${o.settings.adminUser}`;
  const u=await(await api('/admin/api/users')).json();
  document.getElementById('users').innerHTML=u.users.map(x=>
    `<tr><td>${x.username}</td><td>${x.email}</td><td>${x.entityCount}</td><td>${fmt(x.storageBytes)}</td>
     <td class="mono">${x.currentSeq}</td><td class="mono">${x.createdAt}</td>
     <td><button class="link" onclick="showEntities('${x.id}','${x.username}')">实体</button>
     <button class="danger" onclick="delUser('${x.id}','${x.username}')">删除</button></td></tr>`).join('');
  const t=await(await api('/admin/api/tokens')).json();
  document.getElementById('tokens').innerHTML=t.tokens.map(x=>
    `<tr><td>${x.username}</td><td class="mono">${x.tokenDigest}</td><td class="mono">${x.expiresAt}</td></tr>`).join('');
}
async function showEntities(id,name){
  const r=await(await api(`/admin/api/users/${id}/entities`)).json();
  document.getElementById('entitySection').style.display='';
  document.getElementById('entityTitle').textContent=`用户 ${name} 的实体（${r.entities.length}，密文）`;
  document.getElementById('detail').textContent=
    r.entities.map(e=>`${e.entityId}  seq=${e.seq}  ${e.ciphertextBytes}B  更新于 ${e.updatedAt}`).join('\n')||'（无实体）';
}
async function delUser(id,name){
  if(!confirm(`确定删除用户 ${name} 及其全部同步数据？此操作不可恢复！`))return;
  await api(`/admin/api/users/${id}`,{method:'DELETE'});
  load();
}
async function logout(){await api('/admin/logout',{method:'POST'});location.reload()}
load();
</script></body></html>"#.to_string()
}

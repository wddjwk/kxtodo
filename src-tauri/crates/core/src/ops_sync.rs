//! sync 命令域：register / login / status / probe / discover / now / configure /
//! unpair / history。

use serde_json::{json, Value};

use crate::core::{ExecContext, Invocation};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::repo::Domain;
use crate::sync::engine;
use crate::sync::merge::Scopes;
use crate::sync::state::{clear_state, load_state};
use crate::time::now_iso;

fn param_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 配对三要素：服务器地址 + 用户名 + 密码（v0.5.1 起不再有邮箱）。
fn required_pairing_params(params: &Value) -> CoreResult<(String, String, String)> {
    let server_url = param_str(params, "serverUrl")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --server（同步服务器地址）"))?;
    let username = param_str(params, "username")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --username"))?;
    let secret = param_str(params, "secret")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --secret（同步密码）"))?;
    Ok((server_url, username.trim().to_lowercase(), secret))
}

fn scopes_from_params(params: &Value) -> Option<Scopes> {
    let data = params.get("syncData").and_then(Value::as_bool);
    let settings = params.get("syncSettings").and_then(Value::as_bool);
    let schedules = params.get("syncSchedules").and_then(Value::as_bool);
    if data.is_none() && settings.is_none() && schedules.is_none() {
        return None;
    }
    Some(Scopes {
        data: data.unwrap_or(true),
        settings: settings.unwrap_or(true),
        schedules: schedules.unwrap_or(false),
    })
}

fn emit_sync_domains(ctx: &ExecContext, meta: &mut Meta, changed: &[Domain], ids: Vec<String>) {
    let Some(host) = ctx.host else {
        return;
    };
    for domain in changed {
        host.emit_domain_event(*domain, 0, ids.clone());
    }
    let _ = meta;
}

/// sync 域写 settings.json 的动作（register/login/unpair/configure）都要通知前端回刷：
/// 自动同步循环订阅 appSettings 来重排定时器，不发事件的话「用 CLI 改间隔」
/// 对正在运行的 GUI 永远不生效（GUI 自己面板的路径靠显式 refreshFromCore 兜着）。
fn notify_settings_changed(ctx: &ExecContext) {
    let Some(host) = ctx.host else {
        return;
    };
    host.emit_domain_event(Domain::Settings, 0, Vec::new());
}

pub fn sync_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "register" => sync_register(inv, ctx),
        "login" => sync_login(inv, ctx),
        "status" => sync_status(ctx),
        "probe" => sync_probe(ctx),
        "discover" => sync_discover(inv),
        "now" => sync_now(inv, ctx, meta),
        "unpair" => sync_unpair(ctx),
        "configure" => sync_configure(inv, ctx),
        "history" => sync_history(ctx),
        "historyRemove" => sync_history_remove(inv, ctx),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 sync 动作 `{other}`"),
        )),
    }
}

fn sync_register(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    // 新设备不预置默认数据：首次拉取直接落服务端内容，避免与服务端数据并集出重复实体。
    let (server_url, username, secret) = required_pairing_params(&inv.params)?;
    let scopes = scopes_from_params(&inv.params);
    let (device_id, report) =
        engine::register_device(ctx.repo, &server_url, &username, &secret, scopes)?;
    notify_settings_changed(ctx);
    Ok(json!({
        "registered": true,
        "deviceId": device_id,
        "serverUrl": server_url.trim().trim_end_matches('/'),
        "username": username,
        "sync": report,
    }))
}

fn sync_login(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let (server_url, username, secret) = required_pairing_params(&inv.params)?;
    let scopes = scopes_from_params(&inv.params);
    let (device_id, report) =
        engine::pair_device(ctx.repo, &server_url, &username, &secret, scopes)?;
    notify_settings_changed(ctx);
    Ok(json!({
        "paired": true,
        "deviceId": device_id,
        "serverUrl": server_url.trim().trim_end_matches('/'),
        "username": username,
        "sync": report,
    }))
}

/// 纯本地读：配对信息 + 最近同步结果 + 缓存的在线状态。
///
/// 绝不碰网络——服务器掉线时打开设置界面不能把 UI 卡住（要刷新状态走 `sync probe`）。
fn sync_status(ctx: &ExecContext) -> CoreResult<Value> {
    let settings = ctx.repo.load_settings()?;
    let sync = &settings.sync;
    let state = load_state(&ctx.repo.layout);
    let paired = sync.is_paired();
    Ok(json!({
        "paired": paired,
        "enabled": sync.enabled,
        // 已配对但被用户暂停（配置保留，恢复即继续）
        "paused": paired && !sync.enabled,
        "serverUrl": sync.server_url,
        "username": sync.username,
        "scopes": {
            "data": sync.sync_data,
            "settings": sync.sync_settings,
            "schedules": sync.sync_schedules,
        },
        "intervalSeconds": sync.interval_seconds,
        "reconnectSeconds": sync.reconnect_seconds,
        "deviceId": state.device_id,
        "lastPulledSeq": state.last_pulled_seq,
        "lastPulledImageSeq": state.last_pulled_image_seq,
        "lastSyncAt": state.last_sync_at,
        "lastResult": state.last_result,
        // null = 还没探测过；前端据此显示「未知」而不是误报掉线
        "online": state.server_online,
        "lastSeenAt": state.last_seen_at,
        "lastError": state.last_error,
    }))
}

/// 短超时探测服务器，把在线结论写进状态缓存（设置面板打开时后台调用）。
fn sync_probe(ctx: &ExecContext) -> CoreResult<Value> {
    engine::probe_connection(ctx.repo)
}

/// 局域网自动发现 kxtodo-server（UDP 广播/组播查询 + /healthz 复核）。
fn sync_discover(inv: &Invocation) -> CoreResult<Value> {
    let timeout_ms = inv
        .params
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(2500)
        .clamp(500, 10_000);
    let servers = crate::sync::discovery::discover(std::time::Duration::from_millis(timeout_ms))?;
    Ok(json!({
        "servers": servers,
        "count": servers.len(),
        "discoveryPort": crate::sync::discovery::DISCOVERY_PORT,
        "timeoutMs": timeout_ms,
    }))
}

fn sync_now(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let report = engine::run_sync(ctx.repo)?;
    // 拉到新图片也要回刷：前端图片缓存里解析失败的引用需要重渲染才会重新解析
    if report.applied > 0 || report.images_pulled > 0 {
        let mut domains = vec![Domain::Data, Domain::Settings, Domain::Schedule];
        domains.dedup();
        emit_sync_domains(ctx, meta, &domains, Vec::new());
    }
    let _ = meta;
    Ok(serde_json::to_value(&report)?)
}

fn sync_unpair(ctx: &ExecContext) -> CoreResult<Value> {
    clear_state(&ctx.repo.layout)?;
    let (_file, outcome) = ctx
        .repo
        .write_settings(None, None, "sync.unpair", |file| {
            file.sync.enabled = false;
            file.sync.secret = String::new();
            Ok(json!({ "unpaired": true }))
        })?;
    let _ = outcome;
    notify_settings_changed(ctx);
    Ok(json!({ "unpaired": true, "at": now_iso() }))
}

fn sync_configure(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let params = &inv.params;
    let data = params.get("syncData").and_then(Value::as_bool);
    let settings_scope = params.get("syncSettings").and_then(Value::as_bool);
    let schedules = params.get("syncSchedules").and_then(Value::as_bool);
    let enabled = params.get("enabled").and_then(Value::as_bool);
    let interval = params.get("intervalSeconds").and_then(Value::as_u64);
    let reconnect = params.get("reconnectSeconds").and_then(Value::as_u64);
    if data.is_none()
        && settings_scope.is_none()
        && schedules.is_none()
        && enabled.is_none()
        && interval.is_none()
        && reconnect.is_none()
    {
        return Err(CoreError::validation(
            "MISSING_PARAM",
            "至少提供一个配置项（syncData/syncSettings/syncSchedules/enabled/intervalSeconds/reconnectSeconds）",
        ));
    }
    let (_file, _outcome) = ctx
        .repo
        .write_settings(None, None, "sync.configure", |file| {
            if let Some(value) = enabled {
                file.sync.enabled = value;
            }
            if let Some(value) = data {
                file.sync.sync_data = value;
            }
            if let Some(value) = settings_scope {
                file.sync.sync_settings = value;
            }
            if let Some(value) = schedules {
                file.sync.sync_schedules = value;
            }
            // 低于下限的间隔按下限生效（用户要的是「至少 5 秒」，不是报错）
            if let Some(value) = interval {
                file.sync.interval_seconds = value.clamp(5, 86400) as u32;
            }
            if let Some(value) = reconnect {
                file.sync.reconnect_seconds = value.clamp(5, 86400) as u32;
            }
            // 不在此处刷新 syncUpdatedAt：开启设置同步的设备应先收敛到服务端版本，
            // 本地设置只有真的变化后（config.set 触发）才会推送。
            Ok(json!({ "configured": true }))
        })?;
    // 范围变化后需要全量重拉的水位由同步引擎自己对账（runtime/sync.json 里记范围签名），
    // 这里不再逐条重置——否则 config set 改范围就会漏掉重置。
    notify_settings_changed(ctx);
    Ok(json!({ "configured": true }))
}

/// 配对历史（本机 `runtime/sync-history.json`）：设置页「历史」一键回填。
fn sync_history(ctx: &ExecContext) -> CoreResult<Value> {
    let file = crate::sync::history::load_history(&ctx.repo.layout);
    Ok(json!({ "entries": file.entries, "count": file.entries.len() }))
}

fn sync_history_remove(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let index = inv
        .params
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 index（历史条目下标）"))?;
    let file = crate::sync::history::remove(&ctx.repo.layout, index as usize)?;
    Ok(json!({
        "removed": true,
        "index": index,
        "entries": file.entries,
        "count": file.entries.len(),
    }))
}

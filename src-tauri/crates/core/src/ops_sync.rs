//! sync 命令域：register / login / status / now / unpair / configure。

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

fn required_pairing_params(params: &Value) -> CoreResult<(String, String, String, String)> {
    let server_url = param_str(params, "serverUrl")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --server（同步服务器地址）"))?;
    let username = param_str(params, "username")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --username"))?;
    let email = param_str(params, "email")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --email"))?;
    let secret = param_str(params, "secret")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --secret（同步密钥）"))?;
    Ok((
        server_url,
        username.trim().to_lowercase(),
        email.trim().to_lowercase(),
        secret,
    ))
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
        settings: settings.unwrap_or(false),
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
        "now" => sync_now(inv, ctx, meta),
        "unpair" => sync_unpair(ctx),
        "configure" => sync_configure(inv, ctx),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 sync 动作 `{other}`"),
        )),
    }
}

fn sync_register(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    // 新设备不预置默认数据：首次拉取直接落服务端内容，避免与服务端数据并集出重复实体。
    let (server_url, username, email, secret) = required_pairing_params(&inv.params)?;
    let scopes = scopes_from_params(&inv.params);
    let (device_id, report) =
        engine::register_device(ctx.repo, &server_url, &username, &email, &secret, scopes)?;
    Ok(json!({
        "registered": true,
        "deviceId": device_id,
        "serverUrl": server_url.trim().trim_end_matches('/'),
        "username": username,
        "email": email,
        "sync": report,
    }))
}

fn sync_login(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let (server_url, username, email, secret) = required_pairing_params(&inv.params)?;
    let scopes = scopes_from_params(&inv.params);
    let (device_id, report) =
        engine::pair_device(ctx.repo, &server_url, &username, &email, &secret, scopes)?;
    Ok(json!({
        "paired": true,
        "deviceId": device_id,
        "serverUrl": server_url.trim().trim_end_matches('/'),
        "username": username,
        "email": email,
        "sync": report,
    }))
}

fn sync_status(ctx: &ExecContext) -> CoreResult<Value> {
    let settings = ctx.repo.load_settings()?;
    let sync = &settings.sync;
    let state = load_state(&ctx.repo.layout);
    let mut out = json!({
        "paired": sync.enabled && !sync.server_url.is_empty(),
        "enabled": sync.enabled,
        "serverUrl": sync.server_url,
        "username": sync.username,
        "email": sync.email,
        "scopes": {
            "data": sync.sync_data,
            "settings": sync.sync_settings,
            "schedules": sync.sync_schedules,
        },
        "intervalMinutes": sync.interval_minutes,
        "deviceId": state.device_id,
        "lastPulledSeq": state.last_pulled_seq,
        "lastSyncAt": state.last_sync_at,
        "lastResult": state.last_result,
    });
    if sync.enabled && !sync.server_url.is_empty() {
        match engine::fetch_me(ctx.repo) {
            Ok(Some(me)) => {
                out["account"] = me;
            }
            Ok(None) => {}
            Err(error) => {
                out["serverError"] = json!(error.message);
            }
        }
        if let Ok(health) = engine::probe_server(&sync.server_url) {
            out["server"] = health;
        }
    }
    Ok(out)
}

fn sync_now(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let report = engine::run_sync(ctx.repo)?;
    if report.applied > 0 {
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
    Ok(json!({ "unpaired": true, "at": now_iso() }))
}

fn sync_configure(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let params = &inv.params;
    let data = params.get("syncData").and_then(Value::as_bool);
    let settings_scope = params.get("syncSettings").and_then(Value::as_bool);
    let schedules = params.get("syncSchedules").and_then(Value::as_bool);
    let enabled = params.get("enabled").and_then(Value::as_bool);
    let interval = params.get("intervalMinutes").and_then(Value::as_u64);
    if data.is_none()
        && settings_scope.is_none()
        && schedules.is_none()
        && enabled.is_none()
        && interval.is_none()
    {
        return Err(CoreError::validation(
            "MISSING_PARAM",
            "至少提供一个配置项（syncData/syncSettings/syncSchedules/enabled/intervalMinutes）",
        ));
    }
    let settings_before = ctx.repo.load_settings()?;
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
            if let Some(value) = interval {
                if !(1..=1440).contains(&value) {
                    return Err(CoreError::validation(
                        "INVALID_INTERVAL",
                        "intervalMinutes 应在 1-1440 之间",
                    ));
                }
                file.sync.interval_minutes = value as u32;
            }
            // 不在此处刷新 syncUpdatedAt：开启设置同步的设备应先收敛到服务端版本，
            // 本地设置只有真的变化后（config.set 触发）才会推送。
            Ok(json!({ "configured": true }))
        })?;
    // 开启某个 scope 时重置拉取水位：增量流里被旧 scope 过滤掉的记录必须全量重拉，
    // 否则历史实体会永远消失在水位之下。
    let scopes_opened = (data.unwrap_or(false) && !settings_before.sync.sync_data)
        || (settings_scope.unwrap_or(false) && !settings_before.sync.sync_settings)
        || (schedules.unwrap_or(false) && !settings_before.sync.sync_schedules);
    if scopes_opened {
        let mut state = load_state(&ctx.repo.layout);
        state.last_pulled_seq = 0;
        crate::sync::state::save_state(&ctx.repo.layout, &state)?;
    }
    Ok(json!({ "configured": true, "fullRepull": scopes_opened }))
}

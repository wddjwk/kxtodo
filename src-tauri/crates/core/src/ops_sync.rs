//! sync 命令域：pair / status / probe / discover / now / configure / unpair / history。
//!
//! v0.6.0 起注册与登录合并成一个 `pair`（用户视角就是「填账户密码，点开始同步」），
//! 通信方式（局域网 / 自建服务 / P2P）由 `sync.mode` 决定，见 `sync/endpoint.rs`。

use serde_json::{json, Value};

use crate::core::{ExecContext, Invocation};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::model::{SyncMode, SyncSettings};
use crate::repo::Domain;
use crate::sync::engine;
use crate::sync::endpoint;
use crate::sync::merge::Scopes;
use crate::sync::state::{clear_state, load_host_state, load_state};
use crate::time::now_iso;

fn param_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

/// 「开始同步」表单 → 配对目标。三种通信方式共用一个按钮，所以按模式取对应的字段。
fn pair_request(params: &Value, current: &SyncSettings) -> CoreResult<endpoint::PairRequest> {
    let mode = match param_str(params, "mode") {
        Some(raw) => SyncMode::parse(&raw).ok_or_else(|| {
            CoreError::validation(
                "SYNC_MODE_INVALID",
                format!("未知的通信方式 `{raw}`（可选 lan / server / p2p）"),
            )
        })?,
        // 没显式给就沿用本机配置（设置面板总会带上，CLI 可以省略）
        None => current.effective_mode(),
    };
    let username = param_str(params, "username")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --username"))?;
    let secret = param_str(params, "secret")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 --secret（同步密码）"))?;
    let server_url = param_str(params, "serverUrl").unwrap_or_else(|| current.server_url.clone());
    let lan_peer = param_str(params, "lanPeer").unwrap_or_else(|| current.lan_peer.clone());
    match mode {
        SyncMode::Server => {
            if server_url.trim().is_empty() {
                return Err(CoreError::validation(
                    "MISSING_PARAM",
                    "缺少 --server（同步服务器地址）",
                ));
            }
        }
        SyncMode::Lan => {
            // 本机不是主机时，必须已经选定了一台主机（身份是名字，不是 ip:port）
            if lan_peer.trim().is_empty() && !current.lan_host {
                return Err(CoreError::validation(
                    "MISSING_PARAM",
                    "缺少 --lan-peer（局域网主机名）：先 `sync discover` 看看有哪些主机，\
                     或者勾选「本机作为服务器」"
                        .to_string(),
                ));
            }
        }
        // P2P 的对端由账户凭据派生的目录解析出来，不需要地址
        SyncMode::P2p => {}
    }
    Ok(endpoint::PairRequest {
        mode,
        server_url,
        lan_peer,
        username,
        secret,
    }
    .normalized())
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
        "pair" => sync_pair(inv, ctx),
        "status" => sync_status(inv, ctx),
        "probe" => sync_probe(ctx),
        "discover" => sync_discover(inv),
        "now" => sync_now(inv, ctx, meta),
        "unpair" => sync_unpair(ctx),
        "configure" => sync_configure(inv, ctx),
        "peers" => sync_peers(ctx),
        "history" => sync_history(ctx),
        "historyRemove" => sync_history_remove(inv, ctx),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 sync 动作 `{other}`"),
        )),
    }
}

/// 统一「开始同步」：登录既有账户，账户不存在就当场注册（由服务端回答区别）。
fn sync_pair(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let current = ctx.repo.load_settings()?;
    let request = pair_request(&inv.params, &current.sync)?;
    let scopes = scopes_from_params(&inv.params);
    // P2P 的内置回环库与 iroh 运行时由常驻进程启停，而配对早于设置落盘：
    // 先按「将要生效」的设置请宿主起好，否则解析端点时自己这个枢纽还没法服务。
    if request.mode == SyncMode::P2p {
        if let Some(host) = ctx.host {
            let mut prospective = current.sync.clone();
            prospective.mode = Some(SyncMode::P2p);
            prospective.username = request.username.clone();
            prospective.secret = request.secret.clone();
            host.ensure_p2p_services(&ctx.repo.layout.root, &prospective)?;
        }
    }
    // 新设备不预置默认数据：首次拉取直接落服务端内容，避免与服务端数据并集出重复实体
    // （只有新建账户时 engine 才播种默认数据，作为该账户的初始内容）。
    let (device_id, report, registered) = engine::pair_device(ctx.repo, &request, scopes)?;
    notify_settings_changed(ctx);
    Ok(json!({
        "paired": true,
        "registered": registered,
        "deviceId": device_id,
        "mode": request.mode.as_str(),
        "modeLabel": request.mode.label(),
        "target": request.label(),
        "serverUrl": request.server_url,
        "lanPeer": request.lan_peer,
        "username": request.username,
        "sync": report,
    }))
}

/// 纯本地读：配对信息 + 通信方式 + 主机状态 + 最近同步结果 + 缓存的在线状态。
///
/// 绝不碰网络——服务器掉线时打开设置界面不能把 UI 卡住（要刷新状态走 `sync probe`）。
/// 凭据（同步密码、内置主机自动生成的管理台密码）默认不输出，`showSecrets` 才给。
fn sync_status(inv: &Invocation, ctx: &ExecContext) -> CoreResult<Value> {
    let show_secrets = inv
        .params
        .get("showSecrets")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let settings = ctx.repo.load_settings()?;
    let sync = &settings.sync;
    let state = load_state(&ctx.repo.layout);
    let host = load_host_state(&ctx.repo.layout);
    let paired = sync.is_paired();
    let mode = sync.effective_mode();
    let mut out = json!({
        "paired": paired,
        "enabled": sync.enabled,
        // 已配对但被用户暂停（配置保留，恢复即继续）
        "paused": paired && !sync.enabled,
        "mode": mode.as_str(),
        "modeLabel": mode.label(),
        "serverUrl": sync.server_url,
        // 局域网：本机作为主机 / 主机名（身份）/ 选定的远端主机名
        "lanHost": sync.lan_host,
        "lanPort": sync.lan_port,
        "lanName": sync.lan_name,
        "lanPeer": sync.lan_peer,
        // P2P 高级覆盖（空 = n0 免费公共服务）
        "p2pRelay": sync.p2p_relay,
        "p2pDirectory": sync.p2p_directory,
        // P2P 概览：纯本地读（目录只取进程内缓存，不碰网络；要刷新走 sync probe / sync peers）
        "p2p": p2p_status_snapshot(&ctx.repo.layout),
        // 本机内置主机的运行状况（管理台密码要 --show-secrets 才给）
        "host": {
            "wanted": sync.lan_host,
            "running": host.running,
            "port": host.port,
            "name": host.name,
            "instanceId": host.instance_id,
            "adminUrl": host.admin_url,
            "adminUser": host.admin_user,
            "startedAt": host.started_at,
            "lastError": host.last_error,
        },
        // 上一轮真正连到的主机身份与地址缓存
        "serverInstanceId": state.server_instance_id,
        "lanEndpoint": state.lan_endpoint,
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
    });
    if show_secrets {
        out["secret"] = json!(sync.secret);
        out["host"]["adminPassword"] = json!(host.admin_password);
    }
    Ok(out)
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
    let mode = match param_str(params, "mode") {
        Some(raw) => Some(SyncMode::parse(&raw).ok_or_else(|| {
            CoreError::validation(
                "SYNC_MODE_INVALID",
                format!("未知的通信方式 `{raw}`（可选 lan / server / p2p）"),
            )
        })?),
        None => None,
    };
    let lan_host = params.get("lanHost").and_then(Value::as_bool);
    let lan_port = params.get("lanPort").and_then(Value::as_u64);
    let lan_name = param_str(params, "lanName");
    let lan_peer = param_str(params, "lanPeer");
    let p2p_relay = param_str(params, "p2pRelay");
    let p2p_directory = param_str(params, "p2pDirectory");
    if data.is_none()
        && settings_scope.is_none()
        && schedules.is_none()
        && enabled.is_none()
        && interval.is_none()
        && reconnect.is_none()
        && mode.is_none()
        && lan_host.is_none()
        && lan_port.is_none()
        && lan_name.is_none()
        && lan_peer.is_none()
        && p2p_relay.is_none()
        && p2p_directory.is_none()
    {
        return Err(CoreError::validation(
            "MISSING_PARAM",
            "至少提供一个配置项（mode/lanHost/lanName/lanPort/lanPeer/p2pRelay/p2pDirectory/\
             syncData/syncSettings/syncSchedules/enabled/intervalSeconds/reconnectSeconds）",
        ));
    }
    if let Some(value) = &lan_name {
        if value.trim().is_empty() {
            return Err(CoreError::validation(
                "SYNC_HOST_NAME_REQUIRED",
                "主机名字不能为空（局域网内靠它认出这台主机）",
            ));
        }
    }

    let current = ctx.repo.load_settings()?;
    // 勾选「本机作为服务器」之前先在局域网里查一次重名：名字就是主机的**身份**，
    // 客户端按名字选定主机，重名会让「连的是哪台」变成抛硬币。
    // 只在「不当主机 → 当主机」的转换时查——此时本机内置服务器还没起，不会应答自己的广播。
    if lan_host == Some(true) && !current.sync.lan_host {
        let wanted = lan_name
            .clone()
            .unwrap_or_else(|| current.sync.lan_name.clone());
        endpoint::ensure_host_name_available(&endpoint::desired_host_name(&wanted))?;
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
            // 勾选主机开关就等于选了局域网模式；主机与客户端二选一；主机必有名字。
            // 这些不变式收在 SyncSettings::apply_lan_role 里，与 config set 共用一条口径。
            file.sync.apply_lan_role(lan_host, lan_name.as_deref(), lan_peer.as_deref());
            if let Some(value) = lan_port {
                file.sync.lan_port = value.clamp(1, 65535) as u16;
            }
            // 显式给的通信方式最后写：它比 apply_lan_role 推断出来的更有权威
            if let Some(value) = mode {
                file.sync.mode = Some(value);
            }
            // P2P 高级覆盖：空串 = 撤掉覆盖、回到 n0 免费公共服务
            if let Some(value) = p2p_relay {
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() && trimmed != "disabled" {
                    crate::sync::p2p::net::parse_relay_url(&trimmed)?;
                }
                file.sync.p2p_relay = trimmed;
            }
            if let Some(value) = p2p_directory {
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    crate::sync::p2p::directory::parse_directory_url(&trimmed)?;
                }
                file.sync.p2p_directory = trimmed;
            }
            // 不在此处刷新 syncUpdatedAt：开启设置同步的设备应先收敛到服务端版本，
            // 本地设置只有真的变化后（config.set 触发）才会推送。
            Ok(json!({ "configured": true }))
        })?;
    // 范围变化后需要全量重拉的水位由同步引擎自己对账（runtime/sync.json 里记范围签名），
    // 这里不再逐条重置——否则 config set 改范围就会漏掉重置。
    // 主机开关的变化同样靠这个事件生效：GUI 侧订阅 Settings 域，看到 lanHost 变了
    // 就启动/停掉内置服务器（core 自己起不了 axum，也不该起）。
    notify_settings_changed(ctx);
    Ok(json!({ "configured": true }))
}

/// P2P 概览的纯本地快照：目录只读进程内缓存（`sync status` 绝不碰网络）。
///
/// 没缓存（刚启动/缓存过期）时 `hubIsSelf` 为 null——前端显示「未知」，
/// 而不是拿过期目录瞎报角色。
fn p2p_status_snapshot(layout: &crate::repo::Layout) -> Value {
    let Some(runtime) = crate::sync::p2p::current_for(layout) else {
        return json!({ "running": false, "hubIsSelf": null, "onlinePeers": 0 });
    };
    let self_id = runtime.device_id();
    let cached = runtime.directory_cached();
    let hub_is_self = cached.as_ref().map(|entries| {
        entries
            .iter()
            .all(|entry| self_id.as_bytes() <= entry.id.as_bytes())
    });
    let hub_id = cached
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry.id)
                .chain(std::iter::once(self_id))
                .min_by_key(|id| *id.as_bytes())
                .map(|id| id.to_z32())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let known = crate::sync::p2p::identity::load(layout).known_peers;
    let peers: Vec<Value> = cached
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            let id = entry.id.to_z32();
            json!({
                "id": id,
                "name": known.get(&id).map(|peer| peer.name.clone()).unwrap_or_default(),
            })
        })
        .collect();
    json!({
        "running": true,
        "selfId": self_id.to_z32(),
        "serving": runtime.serve(),
        "hubIsSelf": hub_is_self,
        "hubId": hub_id,
        "onlinePeers": peers.len(),
        "peers": peers,
    })
}

/// P2P 设备列表：账户目录里的在线条目 + 本机拨号历史里学到的名字与最近结果。
///
/// 枢纽角色也在这里算（目录含自己里 EndpointId 最小者），设置页与 `sync peers` 共用。
fn sync_peers(ctx: &ExecContext) -> CoreResult<Value> {
    let settings = ctx.repo.load_settings()?;
    if settings.sync.effective_mode() != SyncMode::P2p {
        return Err(CoreError::conflict(
            "SYNC_MODE_MISMATCH",
            "当前通信方式不是 P2P（设置 → 数据同步 → 通信方式）".to_string(),
        ));
    }
    let runtime = crate::sync::p2p::current_for(&ctx.repo.layout);
    let entries = runtime
        .as_ref()
        .map(|runtime| runtime.directory())
        .transpose()?
        .unwrap_or_default();
    let known = crate::sync::p2p::identity::load(&ctx.repo.layout).known_peers;
    // 名字优先取「对方自己发布的名字记录」（不用拨号就能显示），其次才是拨号历史里学到的
    let names = runtime
        .as_ref()
        .map(|runtime| {
            let ids: Vec<iroh::EndpointId> = entries.iter().map(|entry| entry.id).collect();
            runtime.resolve_peer_names(&ids)
        })
        .unwrap_or_default();
    let self_endpoint = runtime.as_ref().map(|runtime| runtime.device_id());
    let self_id = self_endpoint
        .map(|id| id.to_z32())
        .unwrap_or_default();
    // 枢纽口径与 endpoint.rs 完全一致：按 EndpointId **字节序**最小（z32 字符串序不保序）
    let hub_id = entries
        .iter()
        .map(|entry| entry.id)
        .chain(self_endpoint)
        .min_by_key(|id| *id.as_bytes())
        .map(|id| id.to_z32())
        .unwrap_or_default();
    let peers: Vec<Value> = entries
        .iter()
        .map(|entry| {
            let id = entry.id.to_z32();
            let record = known.get(&id);
            let name = names
                .get(&id)
                .cloned()
                .or_else(|| record.map(|peer| peer.name.clone()))
                .unwrap_or_default();
            json!({
                "id": id,
                "name": name,
                "publishedAt": entry.published_at,
                "lastOk": record.and_then(|peer| peer.last_ok),
                "lastSeenAt": record.and_then(|peer| peer.last_seen_at.clone()),
                "lastError": record.and_then(|peer| peer.last_error.clone()),
            })
        })
        .collect();
    Ok(json!({
        "selfId": self_id,
        "hubId": hub_id,
        "hubIsSelf": hub_id == self_id && !self_id.is_empty(),
        "serving": runtime.as_ref().map(|runtime| runtime.serve()).unwrap_or(false),
        "peers": peers,
        "count": peers.len(),
    }))
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

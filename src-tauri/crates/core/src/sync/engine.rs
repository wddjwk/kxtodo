//! 同步引擎：pull → LWW merge → push 的编排。
//!
//! HTTP 传输在 [`crate::sync::transport`]，「连哪儿」在 [`crate::sync::endpoint`]，
//! 合并规则在 [`crate::sync::merge`]。所有合并都在客户端完成，服务器只是
//! 「最新密文保管员」——正因为如此，同一套编排能跑在自建服务、局域网内置主机
//! 与 P2P 隧道上，换通信方式不用动这里。
//! 每次同步幂等可中断：拉取水位只在完整走完后推进。

use std::fs::{File, OpenOptions};

use fs2::FileExt;
use serde_json::{json, Value};

use crate::error::{CoreError, CoreResult};
use crate::repo::Repository;
use crate::sync::crypto::{derive_keys, open_entity, seal_entity, sha256_hex, SyncKeys};
use crate::sync::endpoint;
use crate::sync::merge::{
    apply_data_record, apply_record_settings, apply_schedule_record, data_entity_stamp,
    normalize_data_orders, normalize_schedule_orders, remote_wins, schedule_entity_stamp,
    EntityRecord, Scopes, SETTINGS_ENTITY_ID, SyncEnvelope,
};
use crate::sync::state::{load_state, save_state, PushedEntry, SyncStateFile};
use crate::sync::transport::{ChangeItem, PutError, SyncClient, PAGE_LIMIT};
use crate::time::now_iso;

// ---------------------------------------------------------------------------
// 同步互斥锁：同一数据目录同时只允许一个同步（CLI standalone / Host / GUI 并发）
// ---------------------------------------------------------------------------

#[derive(Debug)]
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

/// 登录；账户在这台主机上不存在就当场注册（并发撞车则退回登录）。
///
/// 「账户不存在」在配对之后仍然是正常情况：主机的库被重建、或用户改连了另一台主机，
/// 新库里根本没有这个账户。此时拒绝同步会把设备晾在一边——而本机持有全量副本，
/// 注册后紧接着的推送就能把新主机喂满（水位已由 instance epoch 归零）。
/// 返回 `(token, 过期时间, 是否新建了账户)`。
fn ensure_login(
    client: &SyncClient,
    username: &str,
    auth_key: &[u8; 32],
) -> CoreResult<(String, Option<String>, bool)> {
    match client.login(username, auth_key) {
        Ok((token, expires_at, _)) => Ok((token, expires_at, false)),
        Err(error) if error.code == "ACCOUNT_NOT_FOUND" => {
            match client.register(username, auth_key) {
                Ok(_user_id) => {}
                // 别的设备抢先注册了同一个账户：直接登录就行
                Err(race) if race.code == "ACCOUNT_EXISTS" => {}
                Err(race) => return Err(race),
            }
            let (token, expires_at, _) = client.login(username, auth_key)?;
            Ok((token, expires_at, true))
        }
        Err(error) => Err(error),
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

pub(crate) fn debug_log(message: String) {
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
    if !sync.is_paired() {
        return Err(CoreError::conflict(
            "SYNC_NOT_CONFIGURED",
            "本机未配置同步（先 kxtodo-cli sync register / login）".to_string(),
        ));
    }
    if !sync.enabled {
        return Err(CoreError::conflict(
            "SYNC_PAUSED",
            "同步已暂停（设置 → 数据同步 → 恢复同步，或 kxtodo-cli sync configure --enabled true）"
                .to_string(),
        ));
    }
    let keys: SyncKeys = derive_keys(&sync.username, &sync.secret)?;
    let mut state = load_state(&repo.layout);
    let scopes = Scopes::from_settings(&settings);

    // 「连哪儿」由通信方式决定（自建服务 / 局域网内置主机或选定主机 / P2P 枢纽）。
    // 从这一行往下，三种方式走的是完全相同的代码——这是 v0.6.0 分层的意义。
    // P2P 的 base_url 是一条本地临时隧道，句柄必须活到本轮结束，所以 resolution 一直持有。
    let resolution = endpoint::resolve(&repo.layout, sync, &state)?;
    let resolved = &resolution.resolved;
    debug_log(format!(
        "endpoint {} via {} instance={}",
        resolved.base_url,
        resolved.source.as_str(),
        resolved.instance_id
    ));

    // 换档：水位/推送台账/token 都是**按主机库**成立的（P2P 换枢纽 = 换库，局域网换主机
    // 或主机库被重建同理）。载入目标库的那一份；没有记录就是零值 = 全量重新播种。
    if state.switch_peer(&resolved.instance_id) {
        crate::sync::images::invalidate_manifest_cache();
        report.warnings.push(format!(
            "同步主机已更换（现在是{}），本机已全量重新对账",
            if resolved.name.is_empty() {
                resolved.base_url.clone()
            } else {
                format!("「{}」", resolved.name)
            }
        ));
    }

    // 范围签名自愈：增量流是按范围过滤的，改范围后水位之下的记录永远不会再来一次，
    // 所以签名一变就把实体与图片水位归零全量重拉（LWW 合并，重拉是安全的）。
    // 放在这里而不是 sync configure 里，是为了让 config set 改范围也同样生效。
    let scope_signature = format!(
        "{}|{}|{}",
        scopes.data, scopes.settings, scopes.schedules
    );
    if state.scope_signature != scope_signature {
        state.last_pulled_seq = 0;
        state.last_pulled_image_seq = 0;
        state.scope_signature = scope_signature;
    }

    let client = SyncClient::new(&resolved.base_url)?;
    if let Some(lan_endpoint) = resolved.lan_endpoint.clone() {
        state.lan_endpoint = Some(lan_endpoint);
    }

    // 1. 确保 token（账户不在这台主机上就当场注册：主机是可替换的）
    if state.token.is_empty() || token_expired(&state.token_expires_at) {
        let (token, expires_at, _) = ensure_login(&client, &sync.username, &keys.auth_key)?;
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

    // 4.5 图片 blob：markdown 插图跟「同步数据」，列表背景与头像跟「同步设置」
    if scopes.data || scopes.settings {
        let device_id = state.device_id.clone();
        let scope = format!("{}|{}", resolved.scope_key(), sync.username);
        match crate::sync::images::sync_images(
            &client,
            &token,
            &keys.enc_key,
            &device_id,
            &scope,
            &repo.layout,
            &scopes,
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

/// 统一「开始同步」：先按既有账户登录，账户不存在就当场注册，然后配对 + 首次同步。
///
/// 用户不需要区分注册与登录——三种通信方式都是「填账户密码，点开始同步」。
/// 区别由服务端回答：账户不存在 → 自动注册；账户存在但密码不符 → `AUTH_FAILED`；
/// 用户名撞车只在并发注册时发生，那时返回 `ACCOUNT_EXISTS`。
///
/// `request` 描述「往哪儿配」（通信方式 + 地址或局域网主机名 + 账户），
/// 主机开关（本机是否作为服务器、端口、名字）不在这里，那是 `sync configure` 的事。
///
/// 返回 `(deviceId, 本轮同步报告, 是否新建了账户)`。
pub fn pair_device(
    repo: &Repository,
    request: &endpoint::PairRequest,
    scopes: Option<Scopes>,
) -> CoreResult<(String, SyncReport, bool)> {
    let request = request.normalized();
    if request.username.is_empty() {
        return Err(CoreError::validation(
            "SYNC_USERNAME_REQUIRED",
            "用户名不能为空".to_string(),
        ));
    }
    if request.secret.is_empty() {
        return Err(CoreError::validation(
            "SYNC_SECRET_REQUIRED",
            "密码不能为空".to_string(),
        ));
    }
    // 全新设备还没有 settings.json：load_settings 会给默认值，这里再兜一层
    let current = repo.load_settings().unwrap_or_default();
    let state = load_state(&repo.layout);
    let resolution = endpoint::resolve_pairing(&repo.layout, &current.sync, &request, &state)?;
    let resolved = &resolution.resolved;
    let keys = derive_keys(&request.username, &request.secret)?;
    let client = SyncClient::new(&resolved.base_url)?;

    // 统一「开始同步」：账户不存在就当场注册，存在就登录（与每轮同步同一条路径）
    let (token, expires_at, registered) = ensure_login(&client, &request.username, &keys.auth_key)?;

    // 新账户 + 全新设备：先播种默认数据（收集箱等），作为这个账户的初始内容。
    // 登录既有账户时**绝不**播种——全新设备首拉要直接落服务端内容，不与本地默认数据并集。
    if registered && !repo.layout.data_file().exists() {
        repo.ensure_initialized()?;
    }

    let device_id = crate::ids::gen_device_id();
    // 重新配对：进程内「图片已齐全」的旧结论作废（服务端数据可能被删过）
    crate::sync::images::invalidate_manifest_cache();
    // 已有本地设置才推送设置实体；全新设备不把默认设置推上去覆盖服务端。
    let settings_existed = repo.layout.settings_file().exists();

    let (_file, _outcome) = repo.write_settings(None, None, "sync.pair", |file| {
        file.sync.enabled = true;
        file.sync.mode = Some(request.mode);
        // 只写本次模式对应的地址字段：局域网配对不该抹掉用户存过的自建服务地址，
        // 反之亦然——配置是持久化的，切换方式不该逼用户重填另一种。
        match request.mode {
            crate::model::SyncMode::Server => file.sync.server_url = request.server_url.clone(),
            crate::model::SyncMode::Lan => {
                file.sync.lan_peer = request.lan_peer.clone();
                // 选定了一台远端主机 = 本机是客户端（角色二选一，与 sync configure 同一条不变式）
                if !request.lan_peer.is_empty() {
                    file.sync.lan_host = false;
                }
            }
            // P2P 的对端由账户凭据派生的目录解析，没有地址可写
            crate::model::SyncMode::P2p => {}
        }
        file.sync.username = request.username.clone();
        file.sync.secret = request.secret.clone();
        if let Some(scopes) = scopes {
            file.sync.sync_data = scopes.data;
            file.sync.sync_settings = scopes.settings;
            file.sync.sync_schedules = scopes.schedules;
        }
        if file.sync.sync_settings && file.sync_updated_at.is_none() && settings_existed {
            file.sync_updated_at = Some(now_iso());
        }
        Ok(json!({ "paired": true, "registered": registered }))
    })?;
    // 登录成功才记历史：设置页「历史」按钮据此一键回填方式/地址或主机名/用户名/密码
    crate::sync::history::remember(&repo.layout, &request)?;

    let mut state = SyncStateFile::fresh(device_id.clone());
    state.token = token;
    state.token_expires_at = expires_at;
    // 记下刚解析到的主机身份与地址缓存：紧接着的 run_sync 才不会把它误判成「换了主机」
    state.server_instance_id = resolved.instance_id.clone();
    state.lan_endpoint = resolved.lan_endpoint.clone();
    save_state(&repo.layout, &state)?;

    let report = run_sync(repo)?;
    // 空账户 + 全新设备：兜底播种默认数据（下一次同步会上传）
    if !repo.layout.data_file().exists() {
        repo.ensure_initialized()?;
    }
    Ok((device_id, report, registered))
}

/// 取账户信息（复用已解析的端点，免得为了显示一次账户又广播一轮发现）。
fn fetch_account(
    repo: &Repository,
    resolved: &endpoint::Resolved,
    state: &mut SyncStateFile,
) -> CoreResult<Value> {
    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    let keys = derive_keys(&sync.username, &sync.secret)?;
    let client = SyncClient::new(&resolved.base_url)?;
    if state.token.is_empty() || token_expired(&state.token_expires_at) {
        let (token, expires_at, _) = ensure_login(&client, &sync.username, &keys.auth_key)?;
        state.token = token;
        state.token_expires_at = expires_at;
        save_state(&repo.layout, state)?;
    }
    client.me(&state.token)
}

/// 用当前 token（必要时重新登录）查询账户信息。
pub fn fetch_me(repo: &Repository) -> CoreResult<Option<Value>> {
    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    if !sync.is_paired() {
        return Ok(None);
    }
    let mut state = load_state(&repo.layout);
    let resolution = endpoint::resolve(&repo.layout, sync, &state)?;
    Ok(Some(fetch_account(repo, &resolution.resolved, &mut state)?))
}

/// 轻量连通性探测（设置面板用）：解析端点（自带短超时 /healthz），通过后再取一次 /me。
///
/// 结论写进 `runtime/sync.json`（serverOnline / lastSeenAt / lastError），
/// 于是 `sync status` 可以完全不碰网络——打开设置界面不再被卡住。
pub fn probe_connection(repo: &Repository) -> CoreResult<Value> {
    let settings = repo.load_settings()?;
    let sync = &settings.sync;
    if !sync.is_paired() {
        return Err(CoreError::conflict(
            "SYNC_NOT_CONFIGURED",
            "本机未配置同步（先在设置 → 数据同步里点「开始同步」）".to_string(),
        ));
    }
    let mut state = load_state(&repo.layout);
    let mode = sync.effective_mode();
    let mut out = json!({ "mode": mode.as_str(), "modeLabel": mode.label() });
    if mode == crate::model::SyncMode::P2p {
        return probe_p2p(&repo.layout, &mut state, out);
    }
    let resolution = match endpoint::resolve(&repo.layout, sync, &state) {
        Ok(resolution) => {
            let resolved = &resolution.resolved;
            out["serverUrl"] = json!(resolved.base_url);
            out["server"] = resolved.health.clone();
            out["endpoint"] = json!({
                "name": resolved.name,
                "instanceId": resolved.instance_id,
                "version": resolved.version,
                "source": resolved.source.as_str(),
            });
            state.server_online = Some(true);
            state.last_seen_at = Some(now_iso());
            state.last_error = None;
            if let Some(lan_endpoint) = resolved.lan_endpoint.clone() {
                state.lan_endpoint = Some(lan_endpoint);
            }
            Some(resolution)
        }
        Err(error) => {
            // 局域网模式还没选定主机时给的是配置类错误，不是掉线
            if error.kind == crate::error::ErrorKind::Io {
                state.server_online = Some(false);
                state.last_error = Some(error.message.clone());
            }
            out["serverError"] = json!(error.message);
            out["serverErrorCode"] = json!(error.code);
            None
        }
    };
    let _ = save_state(&repo.layout, &state);
    let online = resolution.is_some();
    if let Some(resolution) = resolution {
        // 账户信息失败（token 过期/密钥不符）不代表服务器掉线，单独报
        match fetch_account(repo, &resolution.resolved, &mut state) {
            Ok(me) => {
                out["account"] = me;
            }
            Err(error) => {
                out["accountError"] = json!(error.message);
            }
        }
    }
    out["online"] = json!(online);
    Ok(out)
}

/// P2P 的连通性探测：只读缓存与目录（60s TTL），**不拨号**。
///
/// 对端不在线是 P2P 的常态，为了一次面板显示拨号最坏要等 25s；
/// 真正的在线结论由每轮同步写进 runtime/sync.json，这里直接复用。
fn probe_p2p(layout: &crate::repo::Layout, state: &mut SyncStateFile, mut out: Value) -> CoreResult<Value> {
    let runtime = crate::sync::p2p::current();
    let peers = runtime
        .as_ref()
        .map(|runtime| runtime.directory().unwrap_or_default())
        .unwrap_or_default();
    let self_id = runtime
        .as_ref()
        .map(|runtime| runtime.device_id())
        .unwrap_or_else(|| iroh::SecretKey::generate().public());
    let hub_is_self = peers
        .iter()
        .all(|entry| self_id.as_bytes() <= entry.id.as_bytes());
    let host = crate::sync::state::load_host_state(layout);
    out["endpoint"] = json!({
        "source": "p2p",
        "name": if hub_is_self { "本机（主设备）".to_string() } else { String::new() },
        "instanceId": if hub_is_self { host.instance_id.clone() } else { String::new() },
    });
    out["p2p"] = json!({
        "selfId": self_id.to_z32(),
        "hubIsSelf": hub_is_self,
        "onlinePeers": peers.len(),
        "serving": runtime.as_ref().map(|runtime| runtime.serve()).unwrap_or(false),
    });
    let online = state.server_online.unwrap_or(false);
    out["online"] = json!(online);
    if !online {
        if let Some(error) = state.last_error.clone() {
            out["serverError"] = json!(error);
        }
    }
    let _ = save_state(layout, state);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::state::PushedEntry;

    /// 同一数据目录同时只允许一个同步：CLI 与 GUI Host 并发时必须有一个被拒。
    /// 局域网方式下同机可能跑两个实例，这条最容易踩，此前却没有测试覆盖。
    #[test]
    fn sync_run_lock_is_exclusive_per_data_dir() {
        let dir = std::env::temp_dir().join(format!("kxtodo-sync-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let layout = crate::repo::Layout::new(dir.clone());
        let first = SyncRunLock::acquire(&layout).expect("第一个同步应拿到锁");
        let error = SyncRunLock::acquire(&layout).expect_err("第二个同步必须被拒");
        assert_eq!(error.code, "SYNC_IN_PROGRESS");
        drop(first);
        // 释放后立刻能再拿：前端「立即同步」撞上自动同步时靠短等待重试
        let _second = SyncRunLock::acquire(&layout).expect("释放后应能重新拿到锁");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 换主机 / 主机的库被重建 / P2P 换枢纽 → 水位、推送台账、token 必须换成目标库的那一份。
    /// 端到端验证在 crates/server/tests/sync_e2e.rs 的 recreated_host_store_reseeds_clients。
    #[test]
    fn host_identity_change_clears_all_reconciliation_state() {
        let mut state = SyncStateFile::fresh("dev-1".to_string());
        state.last_pulled_seq = 42;
        state.last_pulled_image_seq = 7;
        state.token = "tok".to_string();
        state.token_expires_at = Some("2030-01-01T00:00:00Z".to_string());
        state.set_entry(
            "task-1",
            PushedEntry {
                seq: 9,
                u: "2026-01-01T00:00:00Z".to_string(),
                by: "dev-1".to_string(),
            },
        );
        state.server_instance_id = "srv-old".to_string();

        // 同一台主机：什么都不动，否则每轮都全量重推
        assert!(!state.switch_peer("srv-old"));
        assert_eq!(state.last_pulled_seq, 42);
        assert!(state.entry("task-1").is_some());

        // 换了主机：载入一份空白状态 = 全部清零，并报告「这是一次换主机」
        assert!(state.switch_peer("srv-new"));
        assert_eq!(state.last_pulled_seq, 0);
        assert_eq!(state.last_pulled_image_seq, 0);
        assert!(state.pushed.is_empty(), "台账不清新库就永远喂不满");
        assert!(state.token.is_empty(), "旧库签发的 token 在新库里不存在");
        assert!(state.token_expires_at.is_none());
        assert_eq!(state.server_instance_id, "srv-new");

        // 换回旧主机：旧水位原样回来（逐主机库存状态，不必重新全量对账）
        state.store_current_peer();
        assert!(state.switch_peer("srv-old"));
        assert_eq!(state.last_pulled_seq, 42);
        assert_eq!(state.token, "tok");
        assert!(state.entry("task-1").is_some());

        // 首次对账（此前没有身份）：载入空白但不算「换主机」，不该吓用户
        let mut blank = SyncStateFile::fresh("dev-2".to_string());
        blank.last_pulled_seq = 5;
        assert!(!blank.switch_peer("srv-first"));
        assert_eq!(blank.last_pulled_seq, 0);
        assert_eq!(blank.server_instance_id, "srv-first");

        // 主机没报身份（v0.5.x 的老服务器）→ 不要瞎重置
        let mut legacy = SyncStateFile::fresh("dev-3".to_string());
        legacy.last_pulled_seq = 11;
        legacy.server_instance_id = "srv-x".to_string();
        assert!(!legacy.switch_peer(""));
        assert_eq!(legacy.last_pulled_seq, 11);
        assert_eq!(legacy.server_instance_id, "srv-x");
    }
}

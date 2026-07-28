//! Executor: the single entry point that runs a parsed invocation against the
//! Domain Core. CLI, IPC Host and the GUI bridge all funnel through here (§4.1).

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::domain::envelope::{failure, success, Meta};
use crate::domain::error::{CoreError, CoreResult, ErrorKind};
use crate::domain::model::NodeKind;
use crate::domain::ops_config as config_ops;
use crate::domain::ops_schedule as schedule_ops;
use crate::domain::ops_task as task_ops;
use crate::domain::repo::{Domain, Repository};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Side capabilities only available inside a running Host (GUI or hidden).
pub trait HostServices: Send + Sync {
    /// Show a desktop notification; returns the notification id.
    fn show_notification(&self, payload: Value) -> CoreResult<Value>;
    /// Apply native side effects (autostart, shortcuts, zoom, closeToTray).
    fn apply_native_effect(
        &self,
        name: &str,
        settings: &crate::domain::model::SettingsFile,
    ) -> CoreResult<()>;
    /// Enqueue an immediate schedule run; waits for completion when `wait`.
    fn run_schedule_now(&self, id: &str, wait: bool) -> CoreResult<Value>;
    /// Stop a running schedule.
    fn stop_schedule(&self, id: &str) -> CoreResult<Value>;
    /// Host/scheduler status snapshot.
    fn host_status(&self) -> Value;
    /// Actual OS autostart registration when the backend can query it.
    fn autostart_status(&self) -> Option<bool>;
    /// Emit a domain-changed event to GUI clients.
    fn emit_domain_event(&self, domain: Domain, revision: u64, ids: Vec<String>);
}

pub struct ExecContext<'a> {
    pub repo: &'a Repository,
    pub cwd: PathBuf,
    pub host: Option<&'a dyn HostServices>,
    /// When the CLI used a non-default --data-dir (echoed in meta).
    pub custom_data_dir: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Controls {
    pub dry_run: bool,
    pub yes: bool,
    pub idempotency_key: Option<String>,
    pub if_revision: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub command: String,
    pub params: Value,
    pub controls: Controls,
}

impl Invocation {
    pub fn new(command: impl Into<String>, params: Value) -> Self {
        Self {
            command: command.into(),
            params,
            controls: Controls::default(),
        }
    }
}

pub struct ExecOutcome {
    pub code: i32,
    pub envelope: Value,
}

pub fn execute(inv: &Invocation, ctx: &ExecContext) -> ExecOutcome {
    let mut meta = Meta::default();
    if ctx.custom_data_dir {
        meta.data_dir = Some(ctx.repo.layout.root.display().to_string());
    }
    match run(inv, ctx, &mut meta) {
        Ok(data) => ExecOutcome {
            code: 0,
            envelope: success(&inv.command, data, meta),
        },
        Err(error) => ExecOutcome {
            code: error.exit_code(),
            envelope: failure(&inv.command, &error, meta),
        },
    }
}

fn run(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let segments: Vec<&str> = inv.command.split('.').collect();
    match segments.as_slice() {
        ["version"] => cmd_version(),
        ["notify"] => cmd_notify(inv, ctx, meta),
        ["task", action] => task_dispatch(action, inv, ctx, meta),
        ["schedule", action] => schedule_dispatch(action, inv, ctx, meta),
        ["schedule", "runtime", action] => schedule_runtime_dispatch(action, inv, ctx, meta),
        ["config", action] => config_dispatch(action, inv, ctx, meta),
        ["doctor"] => crate::domain::doctor::run_doctor(inv, ctx, meta),
        ["gui", action] => crate::domain::ops_gui::gui_dispatch(action, inv, ctx, meta),
        _ => Err(CoreError::validation(
            "UNKNOWN_COMMAND",
            format!("未知命令 `{}`", inv.command),
        )),
    }
}

fn cmd_version() -> CoreResult<Value> {
    Ok(json!({
        "version": APP_VERSION,
        "schemaVersions": {
            "data": crate::domain::model::DATA_SCHEMA_VERSION,
            "settings": crate::domain::model::SETTINGS_SCHEMA_VERSION,
            "schedule": crate::domain::model::SCHEDULE_SCHEMA_VERSION,
        }
    }))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn param_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

fn param_bool(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

fn required_str(params: &Value, key: &str) -> CoreResult<String> {
    param_str(params, key)
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", format!("缺少必填参数 --{key}")))
}

fn parse_kind(raw: &str) -> CoreResult<NodeKind> {
    match raw {
        "category" => Ok(NodeKind::Category),
        "entry" => Ok(NodeKind::Entry),
        "system" => Ok(NodeKind::System),
        "item" => Err(CoreError::validation(
            "TYPE_NOT_NODE",
            "item 不是节点类型，请使用对应的 item 参数",
        )),
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`，支持 category/entry/item/system"),
        )),
    }
}

fn require_confirmation(controls: &Controls, message: String, details: Value) -> CoreResult<()> {
    if controls.yes || controls.dry_run {
        return Ok(());
    }
    Err(CoreError::confirmation(message)
        .with_hint("确认风险后追加 --yes 执行；或先用 --dry-run 查看影响范围")
        .with_details(details))
}

fn idem_summary(resource: &Value) -> Value {
    resource.clone()
}

fn set_read_revision(meta: &mut Meta, domain: Domain, revision: u64) {
    meta.revision_domain = Some(domain);
    meta.revision = Some(revision);
}

fn warn_custom_startup(meta: &mut Meta, ctx: &ExecContext) {
    if ctx.custom_data_dir {
        meta.warnings.push(json!({
            "code": "CUSTOM_DATA_DIR_NO_AUTOSTART",
            "message": "自定义 --data-dir 的 Host 不会自动注册系统开机启动；重启后需显式启动该 Host",
        }));
    }
}

fn apply_write_outcome(
    meta: &mut Meta,
    domain: Domain,
    outcome: &crate::domain::repo::WriteOutcome,
) {
    meta.revision_domain = Some(domain);
    meta.revision = Some(outcome.revision);
    meta.replayed = outcome.replayed;
    meta.warnings.extend(outcome.warnings.clone());
}

// ---------------------------------------------------------------------------
// notify
// ---------------------------------------------------------------------------

fn cmd_notify(inv: &Invocation, ctx: &ExecContext, _meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let message = required_str(params, "message")?;
    let title = param_str(params, "title");
    let duration = param_str(params, "duration");
    let tone = param_str(params, "tone");
    let position = param_str(params, "position");
    if let Some(raw) = &duration {
        crate::domain::time::parse_duration_ms(raw)?;
    }
    if let Some(raw) = &tone {
        if !matches!(raw.as_str(), "info" | "success" | "warning" | "error") {
            return Err(CoreError::validation(
                "INVALID_TONE",
                format!("无效 tone `{raw}`，支持 info|success|warning|error"),
            ));
        }
    }
    if let Some(raw) = &position {
        if !matches!(
            raw.as_str(),
            "bottom-right" | "top-right" | "bottom-left" | "top-left"
        ) {
            return Err(CoreError::validation(
                "INVALID_POSITION",
                format!("无效 position `{raw}`"),
            ));
        }
    }
    let payload = json!({
        "title": title.unwrap_or_else(|| "KXToDo".to_string()),
        "message": message,
        "duration": duration,
        "tone": tone.unwrap_or_else(|| "info".to_string()),
        "position": position,
        "wait": param_bool(params, "wait").unwrap_or(false),
    });
    let host = ctx
        .host
        .ok_or_else(|| CoreError::internal("notify 必须由 Background Host 执行"))?;
    host.show_notification(payload)
}

// ---------------------------------------------------------------------------
// task domain
// ---------------------------------------------------------------------------

fn task_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => task_add(inv, ctx, meta),
        "get" => task_get(inv, ctx, meta),
        "list" => task_list(inv, ctx, meta),
        "find" => task_find(inv, ctx, meta),
        "modify" => task_modify(inv, ctx, meta),
        "remove" => task_remove(inv, ctx, meta),
        "tree" => task_tree(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 task 动作 `{other}`"),
        )),
    }
}

fn task_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let kind_raw = required_str(params, "type")?;
    let data = ctx.repo.load_data()?;
    match kind_raw.as_str() {
        "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            let add = task_ops::AddNodeParams {
                name: param_str(params, "name").unwrap_or_default(),
                parent_id: param_str(params, "parentId"),
                icon: param_str(params, "icon"),
                collapsed: param_bool(params, "collapsed"),
            };
            if inv.controls.dry_run {
                let mut preview = data.clone();
                let node = task_ops::add_node(&mut preview, kind, add)?;
                return Ok(json!({
                    "dryRun": true,
                    "action": "add",
                    "resource": task_ops::node_view(&preview, &node, false),
                }));
            }
            let mut created = Value::Null;
            let (file, outcome) = ctx.repo.write_data(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let node = task_ops::add_node(file, kind, add)?;
                    let view = task_ops::node_view(file, &node, false);
                    created = view.clone();
                    Ok(idem_summary(&view))
                },
            )?;
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(ctx, Domain::Data, outcome.revision, vec![]);
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(created));
            }
            let _ = file;
            Ok(created)
        }
        "item" => {
            let entry_id = required_str(params, "entryId")?;
            let markdown = required_str(params, "markdown")?;
            let mut add = task_ops::AddItemParams {
                entry_id,
                markdown,
                completed: param_bool(params, "completed").unwrap_or(false),
                important: param_bool(params, "important").unwrap_or(false),
                my_day: param_bool(params, "myDay").unwrap_or(false),
                planned_date: None,
                due_date: None,
                tags: Vec::new(),
                emojis: Vec::new(),
            };
            if let Some(raw) = param_str(params, "plannedDate") {
                add.planned_date = Some(crate::domain::time::parse_date(&raw)?);
            }
            if let Some(raw) = param_str(params, "dueDate") {
                add.due_date = Some(crate::domain::time::parse_date(&raw)?);
            }
            for raw in params
                .get("tags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let raw = raw.as_str().unwrap_or_default();
                add.tags.push(task_ops::parse_tag_input(raw)?);
            }
            for raw in params
                .get("emojis")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if let Some(emoji) = raw.as_str() {
                    if !emoji.trim().is_empty() {
                        add.emojis.push(emoji.to_string());
                    }
                }
            }
            if inv.controls.dry_run {
                let mut preview = data.clone();
                let item = task_ops::add_item(&mut preview, add)?;
                return Ok(json!({
                    "dryRun": true,
                    "action": "add",
                    "resource": task_ops::item_view(&preview, &item),
                }));
            }
            let mut created = Value::Null;
            let (file, outcome) = ctx.repo.write_data(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let item = task_ops::add_item(file, add)?;
                    let view = task_ops::item_view(file, &item);
                    created = view.clone();
                    Ok(idem_summary(&view))
                },
            )?;
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(ctx, Domain::Data, outcome.revision, vec![]);
            let _ = file;
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(created));
            }
            Ok(created)
        }
        "system" => Err(CoreError::validation(
            "SYSTEM_NODE_READONLY",
            "系统节点为内置视图，不允许创建",
        )),
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`，支持 category/entry/item"),
        )),
    }
}

fn task_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let kind_raw = required_str(params, "type")?;
    let id = required_str(params, "id")?;
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    match kind_raw.as_str() {
        "item" => {
            let item = task_ops::get_item_typed(&data, &id)?;
            Ok(task_ops::item_view(&data, item))
        }
        "system" | "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            let node = task_ops::get_node_typed(&data, &id, kind)?;
            Ok(task_ops::node_view(&data, node, true))
        }
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`"),
        )),
    }
}

fn build_item_filter(params: &Value) -> CoreResult<task_ops::ItemFilter> {
    let mut filter = task_ops::ItemFilter {
        entry_id: param_str(params, "entryId"),
        category_id: param_str(params, "categoryId"),
        recursive: param_bool(params, "recursive").unwrap_or(false),
        status: match param_str(params, "status").as_deref() {
            Some("open") => task_ops::StatusFilter::Open,
            Some("completed") => task_ops::StatusFilter::Completed,
            Some("all") | None => task_ops::StatusFilter::All,
            Some(other) => {
                return Err(CoreError::validation(
                    "INVALID_STATUS",
                    format!("无效 --status `{other}`，支持 open|completed|all"),
                ))
            }
        },
        important: param_bool(params, "important"),
        my_day: param_bool(params, "myDay"),
        tags: params
            .get("tags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        query: param_str(params, "query"),
        descending: match param_str(params, "order").as_deref() {
            Some("desc") => true,
            Some("asc") | None => false,
            Some(other) => {
                return Err(CoreError::validation(
                    "INVALID_ORDER",
                    format!("无效 --order `{other}`，支持 asc|desc"),
                ))
            }
        },
        ..Default::default()
    };
    macro_rules! time_range {
        ($field:ident, $key:expr) => {
            filter.$field = param_str(params, $key);
        };
    }
    time_range!(created_from, "createdFrom");
    time_range!(created_to, "createdTo");
    time_range!(updated_from, "updatedFrom");
    time_range!(updated_to, "updatedTo");
    time_range!(changed_from, "changedFrom");
    time_range!(changed_to, "changedTo");
    time_range!(planned_from, "plannedFrom");
    time_range!(planned_to, "plannedTo");
    time_range!(due_from, "dueFrom");
    time_range!(due_to, "dueTo");
    time_range!(completed_from, "completedFrom");
    time_range!(completed_to, "completedTo");
    filter.sort = match param_str(params, "sort").as_deref() {
        Some("createdAt") => task_ops::SortKey::CreatedAt,
        Some("updatedAt") | None => task_ops::SortKey::UpdatedAt,
        Some("dueDate") => task_ops::SortKey::DueDate,
        Some("completedAt") => task_ops::SortKey::CompletedAt,
        Some("name") => task_ops::SortKey::Name,
        Some("position") => task_ops::SortKey::Position,
        Some(other) => {
            return Err(CoreError::validation(
                "INVALID_SORT",
                format!("无效 --sort `{other}`"),
            ))
        }
    };
    Ok(filter)
}

fn page_from(params: &Value) -> CoreResult<task_ops::Page> {
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(task_ops::DEFAULT_PAGE_LIMIT);
    let offset = task_ops::parse_cursor(param_str(params, "cursor").as_deref())?;
    let all = param_bool(params, "all").unwrap_or(false);
    Ok(task_ops::Page { offset, limit, all })
}

fn task_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let kind_raw = required_str(params, "type")?;
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    let page = page_from(params)?;
    match kind_raw.as_str() {
        "system" | "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            let nodes = task_ops::list_nodes(
                &data,
                kind,
                param_str(params, "parentId").as_deref(),
                param_bool(params, "recursive").unwrap_or(false),
            )?;
            let (nodes, next_cursor, total) = task_ops::paginate(nodes, &page);
            meta.count = Some(total);
            meta.next_cursor = next_cursor;
            let items: Vec<Value> = nodes
                .into_iter()
                .map(|node| task_ops::node_view(&data, node, true))
                .collect();
            Ok(json!({ "items": items }))
        }
        "item" => {
            let filter = build_item_filter(params)?;
            let items = task_ops::filter_items(&data, &filter)?;
            let (items, next_cursor, total) = task_ops::paginate(items, &page);
            meta.count = Some(total);
            meta.next_cursor = next_cursor;
            let orphans = task_ops::orphan_item_ids(&data);
            let views: Vec<Value> = items
                .into_iter()
                .map(|item| task_ops::item_view(&data, item))
                .collect();
            let mut data_out = json!({ "items": views });
            if !orphans.is_empty() {
                data_out["integrityIssues"] = json!([{
                    "type": "orphan-item",
                    "message": "以下 item 归属的 entry 不存在，可用 task modify --type item --entry-id 修复",
                    "itemIds": orphans,
                }]);
            }
            Ok(data_out)
        }
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`"),
        )),
    }
}

fn task_find(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let query = required_str(params, "query")?;
    if query.trim().is_empty() {
        return Err(CoreError::validation("QUERY_REQUIRED", "--query 不能为空"));
    }
    let kind_raw = param_str(params, "type").unwrap_or_else(|| "all".to_string());
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    let page = page_from(params)?;
    let mut results: Vec<Value> = Vec::new();
    match kind_raw.as_str() {
        "system" | "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            for node in task_ops::find_nodes(&data, &query, Some(kind)) {
                results.push(task_ops::node_view(&data, node, true));
            }
        }
        "item" => {
            let mut filter = build_item_filter(params)?;
            filter.query = Some(query.clone());
            for item in task_ops::filter_items(&data, &filter)? {
                results.push(task_ops::item_view(&data, item));
            }
        }
        "all" => {
            for node in task_ops::find_nodes(&data, &query, None) {
                results.push(task_ops::node_view(&data, node, true));
            }
            let mut filter = build_item_filter(params)?;
            filter.query = Some(query.clone());
            for item in task_ops::filter_items(&data, &filter)? {
                results.push(task_ops::item_view(&data, item));
            }
        }
        other => {
            return Err(CoreError::validation(
                "INVALID_TYPE",
                format!("无效 --type `{other}`，支持 system/category/entry/item/all"),
            ))
        }
    }
    let (items, next_cursor, total) = task_ops::paginate(results, &page);
    meta.count = Some(total);
    meta.next_cursor = next_cursor;
    Ok(json!({ "items": items }))
}

fn task_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let kind_raw = required_str(params, "type")?;
    let id = required_str(params, "id")?;
    match kind_raw.as_str() {
        "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            let mut changes = task_ops::NodeChanges {
                name: param_str(params, "name"),
                icon: param_str(params, "icon"),
                parent_id: None,
                collapsed: param_bool(params, "collapsed"),
            };
            if let Some(parent) = params.get("parentId") {
                changes.parent_id = Some(if parent.is_null() {
                    None
                } else {
                    parent.as_str().map(str::to_string)
                });
            }
            let mutate = move |file: &mut crate::domain::model::DataFile| -> CoreResult<Value> {
                let node = task_ops::modify_node(file, &id, kind, changes)?;
                let view = task_ops::node_view(file, &node, true);
                Ok(view)
            };
            if inv.controls.dry_run {
                let mut preview = ctx.repo.load_data()?;
                let view = mutate(&mut preview)?;
                return Ok(json!({ "dryRun": true, "action": "modify", "resource": view }));
            }
            let mut updated = Value::Null;
            let (_file, outcome) = ctx.repo.write_data(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let view = mutate(file)?;
                    updated = view.clone();
                    Ok(idem_summary(&view))
                },
            )?;
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(
                ctx,
                Domain::Data,
                outcome.revision,
                vec![required_str(params, "id")?],
            );
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(updated));
            }
            Ok(updated)
        }
        "item" => {
            let mut changes = task_ops::ItemChanges {
                entry_id: param_str(params, "entryId"),
                markdown: param_str(params, "markdown"),
                completed: param_bool(params, "completed"),
                important: param_bool(params, "important"),
                my_day: param_bool(params, "myDay"),
                planned_date: None,
                due_date: None,
                add_tags: Vec::new(),
                remove_tag_ids: Vec::new(),
                replace_tags: None,
                add_emojis: Vec::new(),
                remove_emojis: Vec::new(),
                replace_emojis: None,
            };
            if param_bool(params, "clearPlannedDate").unwrap_or(false) {
                changes.planned_date = Some(None);
            } else if let Some(raw) = param_str(params, "plannedDate") {
                changes.planned_date = Some(Some(crate::domain::time::parse_date(&raw)?));
            }
            if param_bool(params, "clearDueDate").unwrap_or(false) {
                changes.due_date = Some(None);
            } else if let Some(raw) = param_str(params, "dueDate") {
                changes.due_date = Some(Some(crate::domain::time::parse_date(&raw)?));
            }
            for raw in params
                .get("addTags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                changes
                    .add_tags
                    .push(task_ops::parse_tag_input(raw.as_str().unwrap_or_default())?);
            }
            for raw in params
                .get("removeTags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if let Some(id) = raw.as_str() {
                    changes.remove_tag_ids.push(id.to_string());
                }
            }
            if let Some(list) = params.get("replaceTags").and_then(Value::as_array) {
                let mut tags = Vec::new();
                for raw in list {
                    tags.push(task_ops::parse_tag_input(raw.as_str().unwrap_or_default())?);
                }
                changes.replace_tags = Some(tags);
            }
            let strings = |key: &str| -> Vec<String> {
                params
                    .get(key)
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            changes.add_emojis = strings("addEmojis");
            changes.remove_emojis = strings("removeEmojis");
            if let Some(list) = params.get("replaceEmojis").and_then(Value::as_array) {
                changes.replace_emojis = Some(
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect(),
                );
            }
            let expected_updated_at = param_str(params, "expectedUpdatedAt");
            let mutate = move |file: &mut crate::domain::model::DataFile| -> CoreResult<Value> {
                if let Some(expected) = &expected_updated_at {
                    let current = task_ops::get_item_typed(file, &id)?;
                    if current.updated_at.as_deref() != Some(expected.as_str()) {
                        return Err(CoreError::conflict(
                            "ITEM_CONFLICT",
                            "外部版本已变化，本次未保存",
                        )
                        .with_details(json!({
                            "expectedUpdatedAt": expected,
                            "currentUpdatedAt": current.updated_at,
                        })));
                    }
                }
                let item = task_ops::modify_item(file, &id, changes)?;
                Ok(task_ops::item_view(file, &item))
            };
            if inv.controls.dry_run {
                let mut preview = ctx.repo.load_data()?;
                let view = mutate(&mut preview)?;
                return Ok(json!({ "dryRun": true, "action": "modify", "resource": view }));
            }
            let mut updated = Value::Null;
            let (_file, outcome) = ctx.repo.write_data(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let view = mutate(file)?;
                    updated = view.clone();
                    Ok(idem_summary(&view))
                },
            )?;
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(
                ctx,
                Domain::Data,
                outcome.revision,
                vec![required_str(params, "id")?],
            );
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(updated));
            }
            Ok(updated)
        }
        "system" => Err(CoreError::validation(
            "SYSTEM_NODE_READONLY",
            "系统节点为内置视图，不允许修改",
        )),
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`"),
        )),
    }
}

fn task_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let kind_raw = required_str(params, "type")?;
    let id = required_str(params, "id")?;
    let cascade = param_bool(params, "cascade").unwrap_or(false);
    if !inv.controls.dry_run {
        if let Some((revision, summary)) = ctx
            .repo
            .lookup_data_idempotency(&inv.command, inv.controls.idempotency_key.as_deref())?
        {
            set_read_revision(meta, Domain::Data, revision);
            meta.replayed = true;
            return Ok(summary);
        }
    }
    let data = ctx.repo.load_data()?;

    match kind_raw.as_str() {
        "item" => {
            let item = task_ops::get_item_typed(&data, &id)?;
            let plan = json!({
                "type": "item",
                "id": id,
                "itemIds": [id],
            });
            require_confirmation(
                &inv.controls,
                format!("删除任务 {id}（属于条目 {}）", item.node_id),
                plan.clone(),
            )?;
            if inv.controls.dry_run {
                return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
            }
            let (_file, outcome) = ctx.repo.write_data(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    task_ops::remove_item(file, &id)?;
                    Ok(json!({ "removed": plan }))
                },
            )?;
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(ctx, Domain::Data, outcome.revision, vec![id.clone()]);
            return Ok(json!({
                "removed": plan,
                "revision": outcome.revision,
            }));
        }
        "category" | "entry" => {
            let kind = parse_kind(&kind_raw)?;
            let plan = task_ops::plan_remove_node(&data, &id, kind)?;
            if !cascade {
                if let Some(error) = task_ops::non_empty_remove_error(&data, &id, kind) {
                    return Err(error);
                }
            }
            let plan_json = json!({
                "type": kind.as_str(),
                "id": id,
                "nodeIds": plan.node_ids,
                "itemIds": plan.item_ids,
                "imageDirs": plan.image_dirs,
                "backgroundKeys": plan.background_keys,
                "totals": {
                    "nodes": plan.node_ids.len(),
                    "items": plan.item_ids.len(),
                    "imageDirs": plan.image_dirs.len(),
                }
            });
            require_confirmation(
                &inv.controls,
                format!("级联删除 {} {id}", kind.as_str()),
                plan_json.clone(),
            )?;
            if inv.controls.dry_run {
                return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan_json }));
            }
            let image_dirs = plan.image_dirs.clone();
            let removed_ids = plan.node_ids.clone();
            let recovery = json!({
                "kind": "delete-entry-images",
                "resourceId": id,
                "entryIds": image_dirs.clone(),
            });
            let (_file, outcome, recovery_id) = ctx.repo.write_data_with_recovery(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                recovery,
                |file| {
                    task_ops::apply_remove_node(file, &plan)?;
                    Ok(json!({ "removed": plan_json }))
                },
            )?;
            if outcome.replayed {
                apply_write_outcome(meta, Domain::Data, &outcome);
                return Ok(outcome
                    .replay_summary
                    .clone()
                    .unwrap_or_else(|| json!({ "removed": plan_json })));
            }
            let mut pending_paths = Vec::new();
            let mut image_errors = Vec::new();
            for entry_id in &image_dirs {
                let dir = ctx.repo.layout.entry_img_dir(entry_id);
                if dir.exists() {
                    if let Err(error) = std::fs::remove_dir_all(&dir) {
                        pending_paths.push(entry_id.clone());
                        image_errors.push(format!("无法删除图片目录 {}：{error}", dir.display()));
                    }
                }
            }
            if let Some(recovery_id) = recovery_id.as_deref() {
                let recovery_error = if image_errors.is_empty() {
                    None
                } else {
                    Some(image_errors.join("；"))
                };
                if let Err(error) =
                    ctx.repo
                        .finish_recovery(recovery_id, recovery_error.as_deref(), &pending_paths)
                {
                    meta.warnings.push(json!({
                        "code": "RECOVERY_RECORD_FINALIZE_FAILED",
                        "message": error.message,
                        "recoveryId": recovery_id,
                    }));
                }
                if let Some(message) = recovery_error {
                    meta.warnings.push(json!({
                        "code": "ATTACHMENT_DELETE_PENDING",
                        "message": message,
                        "recoveryId": recovery_id,
                    }));
                }
            }
            apply_write_outcome(meta, Domain::Data, &outcome);
            notify_host(ctx, Domain::Data, outcome.revision, removed_ids);
            Ok(json!({
                "removed": plan_json,
                "revision": outcome.revision,
                "recoveryId": recovery_id,
            }))
        }
        "system" => Err(CoreError::validation(
            "SYSTEM_NODE_READONLY",
            "系统节点为内置视图，不允许删除",
        )),
        other => Err(CoreError::validation(
            "INVALID_TYPE",
            format!("无效 --type `{other}`"),
        )),
    }
}

fn task_tree(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    let depth = params
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(usize::MAX);
    let include_counts = param_bool(params, "includeCounts").unwrap_or(true);
    task_ops::tree_view(
        &data,
        param_str(params, "rootId").as_deref(),
        depth,
        include_counts,
    )
}

// ---------------------------------------------------------------------------
// schedule domain
// ---------------------------------------------------------------------------

fn read_payload(params: &Value, key: &str) -> CoreResult<Value> {
    params
        .get(key)
        .cloned()
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", format!("缺少 --{key} 输入")))
}

fn schedule_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => schedule_add(inv, ctx, meta),
        "validate" => schedule_validate(inv, ctx, meta),
        "get" => schedule_get(inv, ctx, meta),
        "list" | "find" => schedule_list_find(action == "find", inv, ctx, meta),
        "modify" => schedule_modify(inv, ctx, meta),
        "remove" => schedule_remove(inv, ctx, meta),
        "enable" => schedule_set_enabled(true, inv, ctx, meta),
        "disable" => schedule_set_enabled(false, inv, ctx, meta),
        "run" => schedule_run(inv, ctx, meta),
        "stop" => schedule_stop(inv, ctx, meta),
        "logs" => schedule_logs(inv, ctx, meta),
        "status" => schedule_status(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 schedule 动作 `{other}`"),
        )),
    }
}

fn schedule_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let spec_raw = read_payload(&inv.params, "spec")?;
    let enabled = spec_raw
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_code = spec_raw
        .get("action")
        .and_then(|action| action.get("type"))
        .and_then(Value::as_str)
        .map(|kind| kind != "notification")
        .unwrap_or(false);
    if enabled && is_code {
        require_confirmation(
            &inv.controls,
            "创建并启用包含代码执行的定时任务".to_string(),
            json!({ "spec": spec_raw }),
        )?;
    }
    if inv.controls.dry_run {
        let file = ctx.repo.load_schedule()?;
        let mut preview = file.clone();
        let outcome = schedule_ops::add_schedule(&mut preview, &spec_raw, &ctx.cwd)?;
        return Ok(json!({
            "dryRun": true,
            "action": "add",
            "normalizedSpec": outcome.entry.spec,
            "warnings": outcome.warnings,
            "risks": if outcome.entry.spec.enabled && outcome.entry.spec.action.is_code_execution() {
                vec!["将启用包含代码执行的定时任务"]
            } else {
                vec![]
            },
        }));
    }
    let mut created_id = String::new();
    let mut warnings: Vec<String> = Vec::new();
    let (file, outcome) = ctx.repo.write_schedule(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let added = schedule_ops::add_schedule(file, &spec_raw, &ctx.cwd)?;
            created_id = added.entry.id.clone();
            warnings = added.warnings.clone();
            let view = schedule_ops::schedule_view(&added.entry);
            Ok(view)
        },
    )?;
    apply_write_outcome(meta, Domain::Schedule, &outcome);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(Value::Null));
    }
    notify_host(
        ctx,
        Domain::Schedule,
        outcome.revision,
        vec![created_id.clone()],
    );
    let entry = schedule_ops::require_entry(&file, &created_id)?;
    let mut result = schedule_ops::schedule_view(entry);
    if entry.spec.enabled {
        warn_custom_startup(meta, ctx);
    }
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    Ok(result)
}

fn schedule_validate(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    if let Some(spec_raw) = inv.params.get("spec") {
        let mut spec_json = spec_raw.clone();
        crate::domain::ops_schedule::normalize_spec_paths(&mut spec_json, &ctx.cwd);
        let validation = schedule_ops::validate_spec_value(&spec_json, &file.runtimes)?;
        return Ok(json!({
            "valid": true,
            "normalizedSpec": validation.spec,
            "warnings": validation.warnings,
        }));
    }
    let id = required_str(&inv.params, "id")?;
    let patch = read_payload(&inv.params, "patch")?;
    let entry = schedule_ops::require_entry(&file, &id)?;
    let current = serde_json::to_value(&entry.spec)?;
    let mut merged = schedule_ops::apply_patch(&current, &patch)?;
    schedule_ops::normalize_spec_paths(&mut merged, &ctx.cwd);
    let validation = schedule_ops::validate_spec_value(&merged, &file.runtimes)?;
    Ok(json!({
        "valid": true,
        "normalizedSpec": validation.spec,
        "warnings": validation.warnings,
    }))
}

fn schedule_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    let entry = schedule_ops::require_entry(&file, &id)?;
    Ok(schedule_ops::schedule_view(entry))
}

fn schedule_list_find(
    is_find: bool,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    let query = if is_find {
        Some(required_str(params, "query")?)
    } else {
        None
    };
    let descending = match param_str(params, "order").as_deref() {
        Some("desc") => true,
        Some("asc") | None => false,
        Some(other) => {
            return Err(CoreError::validation(
                "INVALID_ORDER",
                format!("无效 --order `{other}`，支持 asc|desc"),
            ))
        }
    };
    let filter = schedule_ops::ScheduleFilter {
        enabled: param_bool(params, "enabled"),
        status: param_str(params, "status"),
        trigger_type: param_str(params, "triggerType"),
        query,
        sort: param_str(params, "sort"),
        descending,
        created_from: param_str(params, "createdFrom"),
        created_to: param_str(params, "createdTo"),
        updated_from: param_str(params, "updatedFrom"),
        updated_to: param_str(params, "updatedTo"),
        last_run_from: param_str(params, "lastRunFrom"),
        last_run_to: param_str(params, "lastRunTo"),
        next_run_from: param_str(params, "nextRunFrom"),
        next_run_to: param_str(params, "nextRunTo"),
    };
    let entries = schedule_ops::filter_schedules(&file, &filter)?;
    let page = page_from(params)?;
    let (entries, next_cursor, total) = task_ops::paginate(entries, &page);
    meta.count = Some(total);
    meta.next_cursor = next_cursor;
    let items: Vec<Value> = entries
        .into_iter()
        .map(schedule_ops::schedule_view)
        .collect();
    Ok(json!({ "items": items }))
}

fn schedule_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let patch = read_payload(&inv.params, "patch")?;
    if inv.controls.dry_run {
        let file = ctx.repo.load_schedule()?;
        let mut preview = file.clone();
        let outcome = schedule_ops::modify_schedule(&mut preview, &id, &patch, &ctx.cwd)?;
        return Ok(json!({
            "dryRun": true,
            "action": "modify",
            "normalizedSpec": outcome.entry.spec,
            "warnings": outcome.warnings,
        }));
    }
    let mut warnings = Vec::new();
    let (_file, outcome) = ctx.repo.write_schedule(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let modified = schedule_ops::modify_schedule(file, &id, &patch, &ctx.cwd)?;
            warnings = modified.warnings.clone();
            Ok(schedule_ops::schedule_view(&modified.entry))
        },
    )?;
    apply_write_outcome(meta, Domain::Schedule, &outcome);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(Value::Null));
    }
    notify_host(ctx, Domain::Schedule, outcome.revision, vec![id.clone()]);
    let file = ctx.repo.load_schedule()?;
    let entry = schedule_ops::require_entry(&file, &id)?;
    let mut result = schedule_ops::schedule_view(entry);
    if entry.spec.enabled {
        warn_custom_startup(meta, ctx);
    }
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    Ok(result)
}

fn schedule_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    if !inv.controls.dry_run {
        if let Some((revision, summary)) = ctx
            .repo
            .lookup_schedule_idempotency(&inv.command, inv.controls.idempotency_key.as_deref())?
        {
            set_read_revision(meta, Domain::Schedule, revision);
            meta.replayed = true;
            return Ok(summary);
        }
    }
    let file = ctx.repo.load_schedule()?;
    let entry = schedule_ops::require_entry(&file, &id)?;
    require_confirmation(
        &inv.controls,
        format!("删除定时任务 {id}（{}）", entry.spec.name),
        json!({ "id": id, "name": entry.spec.name, "running": entry.state.running }),
    )?;
    if inv.controls.dry_run {
        return Ok(json!({
            "dryRun": true,
            "action": "remove",
            "plan": { "id": id, "name": entry.spec.name },
        }));
    }
    if let Some(host) = ctx.host {
        // Stop the live run slot even if the persisted running flag has not
        // reached disk yet (run --wait=false returns immediately after queueing).
        host.stop_schedule(&id)?;
    }
    let (_file, outcome) = ctx.repo.write_schedule(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            schedule_ops::remove_schedule(file, &id)?;
            Ok(json!({ "removed": { "id": id } }))
        },
    )?;
    apply_write_outcome(meta, Domain::Schedule, &outcome);
    notify_host(ctx, Domain::Schedule, outcome.revision, vec![id.clone()]);
    Ok(json!({ "removed": { "id": id }, "revision": outcome.revision }))
}

fn schedule_set_enabled(
    enabled: bool,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_schedule()?;
    let entry = schedule_ops::require_entry(&file, &id)?;
    if enabled && entry.spec.action.is_code_execution() {
        require_confirmation(
            &inv.controls,
            format!("启用包含代码执行的定时任务 {id}"),
            json!({ "id": id, "action": entry.spec.action.kind_str() }),
        )?;
    }
    if inv.controls.dry_run {
        return Ok(json!({
            "dryRun": true,
            "action": if enabled { "enable" } else { "disable" },
            "id": id,
        }));
    }
    let mut warnings = Vec::new();
    let (_file, outcome) = ctx.repo.write_schedule(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let result = schedule_ops::set_enabled(file, &id, enabled)?;
            warnings = result.warnings.clone();
            Ok(schedule_ops::schedule_view(&result.entry))
        },
    )?;
    apply_write_outcome(meta, Domain::Schedule, &outcome);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(Value::Null));
    }
    notify_host(ctx, Domain::Schedule, outcome.revision, vec![id.clone()]);
    let file = ctx.repo.load_schedule()?;
    let entry = schedule_ops::require_entry(&file, &id)?;
    let mut result = schedule_ops::schedule_view(entry);
    if enabled {
        warn_custom_startup(meta, ctx);
    }
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    Ok(result)
}

fn schedule_run(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let wait = param_bool(&inv.params, "wait").unwrap_or(false);
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    let entry = schedule_ops::require_entry(&file, &id)?;
    if entry.spec.action.is_code_execution() {
        require_confirmation(
            &inv.controls,
            format!("立即执行定时任务 {id}（含代码执行）"),
            json!({ "id": id, "action": entry.spec.action.kind_str() }),
        )?;
    }
    if inv.controls.dry_run {
        return Ok(json!({ "dryRun": true, "action": "run", "id": id }));
    }
    let host = ctx.host.ok_or_else(|| {
        CoreError::new(
            ErrorKind::Execution,
            "HOST_REQUIRED",
            "schedule run 需要 Background Host 运行",
        )
    })?;
    let result = host.run_schedule_now(&id, wait)?;
    if wait {
        if let Ok(file) = ctx.repo.load_schedule() {
            set_read_revision(meta, Domain::Schedule, file.meta.revision);
        }
    }
    Ok(result)
}

fn schedule_stop(inv: &Invocation, ctx: &ExecContext, _meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    if inv.controls.dry_run {
        return Ok(json!({ "dryRun": true, "action": "stop", "id": id }));
    }
    let host = ctx.host.ok_or_else(|| {
        CoreError::new(
            ErrorKind::Execution,
            "HOST_REQUIRED",
            "schedule stop 需要 Background Host 运行",
        )
    })?;
    host.stop_schedule(&id)
}

fn schedule_logs(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let limit = inv
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(20);
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    schedule_ops::require_entry(&file, &id)?;
    let history = crate::domain::history::read_history(&ctx.repo.layout.schedule_history())?;
    let mut runs: Vec<Value> = history
        .into_iter()
        .filter(|entry| entry.get("taskId").and_then(Value::as_str) == Some(id.as_str()))
        .collect();
    let total = runs.len();
    if runs.len() > limit {
        runs = runs.split_off(runs.len() - limit);
    }
    Ok(json!({
        "id": id,
        "runs": runs,
        "total": total,
    }))
}

fn schedule_status(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_schedule()?;
    set_read_revision(meta, Domain::Schedule, file.meta.revision);
    let running: Vec<Value> = file
        .tasks
        .iter()
        .filter(|entry| entry.state.running)
        .map(|entry| json!({ "id": entry.id, "name": entry.spec.name }))
        .collect();
    let mut next_wake: Option<(String, String)> = None;
    for entry in file.tasks.iter().filter(|entry| entry.spec.enabled) {
        if let Some(next) = &entry.state.next_run_at {
            let better = next_wake
                .as_ref()
                .map(|(_, current)| next < current)
                .unwrap_or(true);
            if better {
                next_wake = Some((entry.id.clone(), next.clone()));
            }
        }
    }
    let host_status = ctx
        .host
        .map(|host| host.host_status())
        .unwrap_or_else(|| json!({ "state": "absent" }));
    let last_missed_at = file
        .tasks
        .iter()
        .filter_map(|entry| entry.state.last_missed_at.clone())
        .max();
    let missed_count: u64 = file
        .tasks
        .iter()
        .map(|entry| entry.state.missed_count)
        .sum();
    let settings = ctx.repo.load_settings().ok();
    let configured = settings
        .as_ref()
        .map(|settings| settings.lifecycle.launch_at_startup)
        .unwrap_or(false);
    let custom = ctx.custom_data_dir;
    let registered = if custom {
        None
    } else {
        ctx.host.and_then(|host| host.autostart_status())
    };
    let startup_recovery = if custom {
        json!({
            "available": false,
            "configured": false,
            "registered": null,
            "dataDirKind": "custom",
            "limitation": "v9 不会为自定义 --data-dir 自动注册系统开机启动",
            "hint": "机器重启后请显式使用同一 --data-dir 启动 KXToDo Host",
        })
    } else {
        json!({
            "available": configured && registered.unwrap_or(false),
            "configured": configured,
            "registered": registered,
            "dataDirKind": "default",
            "hint": if configured && registered != Some(true) {
                "在设置中重新启用开机启动，或保持 KXToDo Host 运行"
            } else {
                ""
            },
        })
    };
    let _ = inv;
    Ok(json!({
        "host": host_status,
        "running": running,
        "nextWake": next_wake.map(|(id, at)| json!({ "id": id, "at": at })),
        "runtimes": schedule_ops::runtime_view(&file)["runtimes"],
        "lastMissedAt": last_missed_at,
        "missedCount": missed_count,
        "startupRecovery": startup_recovery,
        "tasks": {
            "total": file.tasks.len(),
            "enabled": file.tasks.iter().filter(|entry| entry.spec.enabled).count(),
        }
    }))
}

fn schedule_runtime_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let file = ctx.repo.load_schedule()?;
    if action == "list" {
        set_read_revision(meta, Domain::Schedule, file.meta.revision);
    }
    match action {
        "list" => Ok(schedule_ops::runtime_view(&file)),
        "detect" => {
            let (_file, outcome) = ctx.repo.write_schedule(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    schedule_ops::detect_runtimes(file);
                    Ok(json!({
                        "runtimes": schedule_ops::runtime_view(file)["runtimes"],
                    }))
                },
            )?;
            apply_write_outcome(meta, Domain::Schedule, &outcome);
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(Value::Null));
            }
            notify_host(ctx, Domain::Schedule, outcome.revision, vec![]);
            let file = ctx.repo.load_schedule()?;
            Ok(json!({
                "runtimes": schedule_ops::runtime_view(&file)["runtimes"],
            }))
        }
        "set" => {
            let name = required_str(&inv.params, "name")?;
            let raw_path = required_str(&inv.params, "path")?;
            let path = if raw_path.trim().is_empty() {
                String::new()
            } else if !raw_path.contains('/') && !raw_path.contains('\\') {
                let found = crate::domain::exec::find_executable(&[raw_path.as_str()], &[]);
                if found.is_empty() {
                    schedule_ops::normalize_path(&raw_path, &ctx.cwd)
                } else {
                    schedule_ops::normalize_path(&found, &ctx.cwd)
                }
            } else {
                schedule_ops::normalize_path(&raw_path, &ctx.cwd)
            };
            let (_file, outcome) = ctx.repo.write_schedule(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let runtime = schedule_ops::set_runtime(file, &name, &path)?;
                    Ok(json!({
                        "runtime": runtime,
                        "runtimes": schedule_ops::runtime_view(file)["runtimes"],
                    }))
                },
            )?;
            apply_write_outcome(meta, Domain::Schedule, &outcome);
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(Value::Null));
            }
            notify_host(ctx, Domain::Schedule, outcome.revision, vec![]);
            let file = ctx.repo.load_schedule()?;
            Ok(json!({
                "runtime": {
                    "name": name,
                    "path": path,
                },
                "runtimes": schedule_ops::runtime_view(&file)["runtimes"],
            }))
        }
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 runtime 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// config domain
// ---------------------------------------------------------------------------

fn load_settings_with_raw(
    ctx: &ExecContext,
) -> CoreResult<(crate::domain::model::SettingsFile, Value)> {
    let settings = ctx.repo.load_settings()?;
    let raw = crate::domain::repo::read_json_value(&ctx.repo.layout.settings_file())?;
    Ok((settings, raw))
}

fn config_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "list" => {
            let (settings, raw) = load_settings_with_raw(ctx)?;
            set_read_revision(meta, Domain::Settings, settings.meta.revision);
            let prefix = param_str(&inv.params, "prefix");
            let items = config_ops::list_values(&settings, &raw, prefix.as_deref())?;
            meta.count = Some(items.len());
            Ok(json!({ "items": items }))
        }
        "get" => {
            let (settings, raw) = load_settings_with_raw(ctx)?;
            set_read_revision(meta, Domain::Settings, settings.meta.revision);
            let path = required_str(&inv.params, "path")?;
            config_ops::get_value(
                &settings,
                &raw,
                &path,
                param_str(&inv.params, "mapKey").as_deref(),
            )
        }
        "set" => {
            let path = required_str(&inv.params, "path")?;
            let value = inv
                .params
                .get("value")
                .cloned()
                .ok_or_else(|| CoreError::validation("MISSING_VALUE", "缺少配置值"))?;
            let map_key = param_str(&inv.params, "mapKey");
            if inv.controls.dry_run {
                let (mut preview, _raw) = load_settings_with_raw(ctx)?;
                let outcome =
                    config_ops::set_value(&mut preview, &path, value, map_key.as_deref())?;
                return Ok(json!({
                    "dryRun": true,
                    "action": "set",
                    "path": path,
                    "previous": outcome.previous,
                    "value": outcome.value,
                    "nativeEffects": outcome.native_effects,
                }));
            }
            let mut native_effects: Vec<&'static str> = Vec::new();
            let (file, outcome) = ctx.repo.write_settings(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    let result = config_ops::set_value(file, &path, value, map_key.as_deref())?;
                    native_effects = result.native_effects.clone();
                    Ok(json!({
                        "path": path,
                        "previous": result.previous,
                        "value": result.value,
                    }))
                },
            )?;
            apply_write_outcome(meta, Domain::Settings, &outcome);
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(Value::Null));
            }
            notify_host(ctx, Domain::Settings, outcome.revision, vec![path.clone()]);
            let applied = apply_native_effects(ctx, &file, &native_effects);
            Ok(json!({
                "path": path,
                "value": config_ops::get_value_public(&file, &path)?,
                "nativeEffects": applied,
                "revision": outcome.revision,
            }))
        }
        "unset" => {
            let path = required_str(&inv.params, "path")?;
            let map_key = param_str(&inv.params, "mapKey");
            if inv.controls.dry_run {
                return Ok(json!({ "dryRun": true, "action": "unset", "path": path }));
            }
            let mut previous = Value::Null;
            let (_file, outcome) = ctx.repo.write_settings(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    previous = config_ops::unset_value(file, &path, map_key.as_deref())?;
                    Ok(json!({
                        "path": path,
                        "mapKey": map_key,
                        "previous": previous,
                    }))
                },
            )?;
            apply_write_outcome(meta, Domain::Settings, &outcome);
            if outcome.replayed {
                return Ok(outcome.replay_summary.unwrap_or(Value::Null));
            }
            notify_host(ctx, Domain::Settings, outcome.revision, vec![path.clone()]);
            Ok(json!({
                "path": path,
                "mapKey": map_key,
                "previous": previous,
                "revision": outcome.revision,
            }))
        }
        "reset" => {
            let prefix = param_str(&inv.params, "prefix");
            require_confirmation(
                &inv.controls,
                format!("重置配置 {}", prefix.as_deref().unwrap_or("(全部)")),
                json!({ "prefix": prefix }),
            )?;
            if inv.controls.dry_run {
                let (mut preview, _raw) = load_settings_with_raw(ctx)?;
                let changes = config_ops::reset_values(&mut preview, prefix.as_deref())?;
                return Ok(json!({
                    "dryRun": true,
                    "action": "reset",
                    "changes": changes,
                }));
            }
            if let Some((revision, summary)) = ctx.repo.lookup_settings_idempotency(
                &inv.command,
                inv.controls.idempotency_key.as_deref(),
            )? {
                set_read_revision(meta, Domain::Settings, revision);
                meta.replayed = true;
                return Ok(summary);
            }
            ctx.repo.backup("config-reset")?;
            let mut changes = Vec::new();
            let (file, outcome) = ctx.repo.write_settings(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    changes = config_ops::reset_values(file, prefix.as_deref())?;
                    Ok(json!({ "changes": changes.clone() }))
                },
            )?;
            apply_write_outcome(meta, Domain::Settings, &outcome);
            notify_host(ctx, Domain::Settings, outcome.revision, vec![]);
            let applied = apply_native_effects(
                ctx,
                &file,
                &["autostart", "closeToTray", "webviewZoom", "globalShortcut"],
            );
            Ok(json!({
                "changes": changes,
                "nativeEffects": applied,
                "revision": outcome.revision,
            }))
        }
        "path" => {
            let settings = ctx.repo.load_settings()?;
            set_read_revision(meta, Domain::Settings, settings.meta.revision);
            crate::domain::doctor::config_paths(ctx)
        }
        "validate" => {
            let (settings, _raw) = load_settings_with_raw(ctx)?;
            set_read_revision(meta, Domain::Settings, settings.meta.revision);
            let issues = config_ops::validate_settings(&ctx.repo.layout, &settings);
            Ok(json!({
                "valid": issues.is_empty(),
                "issues": issues,
            }))
        }
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 config 动作 `{other}`"),
        )),
    }
}

fn apply_native_effects(
    ctx: &ExecContext,
    settings: &crate::domain::model::SettingsFile,
    effects: &[&'static str],
) -> Value {
    let mut results = Vec::new();
    for effect in effects {
        let status = match ctx.host {
            Some(host) => match host.apply_native_effect(effect, settings) {
                Ok(()) => json!({ "name": effect, "status": "applied" }),
                Err(error) => json!({ "name": effect, "status": "failed", "error": error.message }),
            },
            None => json!({
                "name": effect,
                "status": "pending",
                "note": "Host 未运行，原生副作用将在下次启动时应用",
            }),
        };
        results.push(status);
    }
    Value::Array(results)
}

fn notify_host(ctx: &ExecContext, domain: Domain, revision: u64, ids: Vec<String>) {
    if let Some(host) = ctx.host {
        host.emit_domain_event(domain, revision, ids);
    }
}

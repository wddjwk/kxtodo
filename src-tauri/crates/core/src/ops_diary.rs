//! diary domain: 日记条目的业务操作（GUI 与 CLI 共用同一条命令层）。
//!
//! 日记与 task 是平行的两类内容，但同住 `data.json`：同一个域事件、同一把文件锁、
//! 同一个同步范围（「同步数据」），只是同步实体 kind 为 `diary`。

use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_str, require_confirmation, required_str,
    set_read_revision, ExecContext, Invocation,
};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{DataFile, DiaryEntry, Tag};
use crate::ops_task::{build_tag, parse_tag_input};
use crate::repo::Domain;
use crate::time::{now_iso, parse_date, today_local};

pub fn diary_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => diary_add(inv, ctx, meta),
        "get" => diary_get(inv, ctx, meta),
        "list" => diary_list(inv, ctx, meta),
        "modify" => diary_modify(inv, ctx, meta),
        "remove" => diary_remove(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 diary 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub fn find_diary<'a>(data: &'a DataFile, id: &str) -> Option<&'a DiaryEntry> {
    data.diaries.iter().find(|entry| entry.id == id)
}

fn not_found(id: &str) -> CoreError {
    CoreError::not_found("DIARY_NOT_FOUND", format!("未找到日记 {id}"))
        .with_hint("先运行 kxtodo-cli diary list 获取稳定 ID")
}

/// 空标题 + 空正文的日记没有意义（GUI 的编辑器也按这条规则不落盘）。
fn ensure_has_content(title: &str, markdown: &str) -> CoreResult<()> {
    if title.trim().is_empty() && markdown.trim().is_empty() {
        return Err(CoreError::validation(
            "DIARY_EMPTY",
            "标题与正文不能同时为空",
        ));
    }
    Ok(())
}

fn tags_param(params: &Value, key: &str) -> CoreResult<Option<Vec<Tag>>> {
    let Some(list) = params.get(key).and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut tags = Vec::new();
    for raw in list {
        tags.push(build_tag(&parse_tag_input(raw.as_str().unwrap_or_default())?));
    }
    Ok(Some(tags))
}

pub fn diary_view(entry: &DiaryEntry) -> Value {
    json!({
        "id": entry.id,
        "date": entry.date,
        "title": entry.title,
        "markdown": entry.markdown,
        "mood": entry.mood,
        "weather": entry.weather,
        "tags": entry
            .tags
            .iter()
            .map(|tag| json!({
                "id": tag.id,
                "color": tag.color.as_str(),
                "text": tag.text,
            }))
            .collect::<Vec<_>>(),
        "expanded": entry.expanded,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
    })
}

/// 日记的稳定展示顺序：日期由近及远，同一天内按写作先后（早的在上）。
fn sort_diaries(entries: &mut [DiaryEntry]) {
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

fn diary_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let date = match param_str(params, "date") {
        Some(raw) => parse_date(&raw)?,
        None => today_local(),
    };
    let title = param_str(params, "title").unwrap_or_default();
    let markdown = param_str(params, "markdown").unwrap_or_default();
    ensure_has_content(&title, &markdown)?;
    let mood = param_str(params, "mood").unwrap_or_default();
    let weather = param_str(params, "weather").unwrap_or_default();
    let tags = tags_param(params, "tags")?.unwrap_or_default();

    let mut created = Value::Null;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let now = now_iso();
            let entry = DiaryEntry {
                id: gen_id("diary"),
                date,
                title,
                markdown,
                mood,
                weather,
                tags,
                expanded: None,
                created_at: now.clone(),
                updated_at: Some(now),
                extra: Map::new(),
            };
            let view = diary_view(&entry);
            file.diaries.push(entry);
            created = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(created));
    }
    Ok(created)
}

fn diary_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    let entry = find_diary(&data, &id).ok_or_else(|| not_found(&id))?;
    Ok(diary_view(entry))
}

fn diary_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let data = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, data.meta.revision);
    let date = param_str(params, "date").map(|raw| parse_date(&raw)).transpose()?;
    let from = param_str(params, "from").map(|raw| parse_date(&raw)).transpose()?;
    let to = param_str(params, "to").map(|raw| parse_date(&raw)).transpose()?;
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize);

    let mut entries: Vec<DiaryEntry> = data
        .diaries
        .iter()
        .filter(|entry| {
            if let Some(date) = &date {
                if &entry.date != date {
                    return false;
                }
            }
            if let Some(from) = &from {
                if entry.date.as_str() < from.as_str() {
                    return false;
                }
            }
            if let Some(to) = &to {
                if entry.date.as_str() > to.as_str() {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();
    sort_diaries(&mut entries);
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    let total = data.diaries.len();
    Ok(json!({
        "total": total,
        "returned": entries.len(),
        "items": entries.iter().map(diary_view).collect::<Vec<_>>(),
    }))
}

fn diary_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let date = param_str(params, "date").map(|raw| parse_date(&raw)).transpose()?;
    let title = param_str(params, "title");
    let markdown = param_str(params, "markdown");
    let mood = param_str(params, "mood");
    let weather = param_str(params, "weather");
    let tags = tags_param(params, "replaceTags")?;

    let mut updated = Value::Null;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let entry = file
                .diaries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| not_found(&id))?;
            if let Some(date) = date {
                entry.date = date;
            }
            if let Some(title) = &title {
                entry.title = title.chars().take(120).collect();
            }
            if let Some(markdown) = &markdown {
                entry.markdown = markdown.clone();
            }
            if let Some(mood) = &mood {
                entry.mood = mood.trim().to_string();
            }
            if let Some(weather) = &weather {
                entry.weather = weather.trim().to_string();
            }
            if let Some(tags) = tags {
                entry.tags = tags;
            }
            ensure_has_content(&entry.title, &entry.markdown)?;
            entry.updated_at = Some(now_iso());
            let view = diary_view(entry);
            updated = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![id.clone()]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}

fn diary_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let data = ctx.repo.load_data()?;
    let entry = find_diary(&data, &id).ok_or_else(|| not_found(&id))?;
    let plan = json!({
        "type": "diary",
        "id": id,
        "date": entry.date,
        "diaryIds": [id],
    });
    require_confirmation(
        &inv.controls,
        format!("删除 {} 的日记 {id}", entry.date),
        plan.clone(),
    )?;
    if inv.controls.dry_run {
        set_read_revision(meta, Domain::Data, data.meta.revision);
        return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
    }
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let index = file
                .diaries
                .iter()
                .position(|entry| entry.id == id)
                .ok_or_else(|| not_found(&id))?;
            file.diaries.remove(index);
            // 删除必须显式传播：服务器只见密文，没有墓碑对端会把它推回来。
            file.meta.record_tombstone(&id, "diary", &now_iso());
            Ok(json!({ "removed": plan.clone() }))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![id.clone()]);
    Ok(json!({ "removed": plan, "revision": outcome.revision }))
}

//! calendar domain: 日程的业务操作（GUI 与 CLI 共用同一条命令层）。
//!
//! 日程住在自己的 `calendar.json` 里（独立领域文件），写路径不抬高 data 域的
//! revision、也不和任务写入抢同一把幂等台账。同步上有自己独立的范围勾选
//! （`sync.syncCalendar`，默认开）。
//!
//! **命名纪律**：代码里一律 `calendar`——`schedule` 这个名字已经被「定时任务」
//! （tasks.json / ops_schedule.rs / CLI `schedule.*`）占用，界面上才叫「日程」。

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_bool, param_str, require_confirmation,
    required_str, set_read_revision, unbounded_page_from, ExecContext, Invocation,
};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{CalendarEvent, CalendarFile, Reminder};
use crate::ops_task::paginate;
use crate::reminders::{parse_rules_opt, resolve};
use crate::repo::Domain;
use crate::time::{now_iso, parse_clock, parse_date, today_local};

/// 全天事件的「开始时刻」折算（提醒用）：业界通行做法是按当天上午 9 点。
pub const ALL_DAY_REMINDER_CLOCK: &str = "09:00";

pub fn calendar_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => calendar_add(inv, ctx, meta),
        "get" => calendar_get(inv, ctx, meta),
        "list" => calendar_list(inv, ctx, meta),
        "modify" => calendar_modify(inv, ctx, meta),
        "remove" => calendar_remove(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 calendar 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub fn find_event<'a>(file: &'a CalendarFile, id: &str) -> Option<&'a CalendarEvent> {
    file.events.iter().find(|event| event.id == id)
}

fn not_found(id: &str) -> CoreError {
    CoreError::not_found("CALENDAR_NOT_FOUND", format!("未找到日程 {id}"))
        .with_hint("先运行 kxtodo-cli calendar list 获取稳定 ID")
}

pub fn calendar_view(event: &CalendarEvent) -> Value {
    json!({
        "id": event.id,
        "title": event.title,
        "details": event.details,
        "startDate": event.start_date,
        "startTime": event.start_time,
        "endDate": event.end_date,
        "endTime": event.end_time,
        "allDay": event.all_day,
        "reminders": serde_json::to_value(&event.reminders).unwrap_or(Value::Array(Vec::new())),
        "taskId": event.task_id,
        "createdAt": event.created_at,
        "updatedAt": event.updated_at,
    })
}

/// 提醒解析用的有效开始时刻：全天事件（或没写时刻）按 09:00 折算。
pub fn effective_start_time(event: &CalendarEvent) -> &str {
    if event.start_time.is_empty() {
        ALL_DAY_REMINDER_CLOCK
    } else {
        &event.start_time
    }
}

/// 日程的稳定展示顺序：开始早的在前；同刻开始的全天在前、再按创建先后与 id。
pub fn sort_events(events: &mut [CalendarEvent]) {
    events.sort_by(|a, b| {
        a.start_date
            .cmp(&b.start_date)
            .then_with(|| b.all_day.cmp(&a.all_day))
            .then_with(|| a.start_time.cmp(&b.start_time))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// 校验并归一时间字段（add 与 modify 共用一份口径）：
/// - 日期必须是合法的 YYYY-MM-DD；时刻必须是 HH:MM
/// - 没写开始时刻 = 全天事件（`all_day` 自动置真、时刻清空）
/// - 定时事件没写结束时刻 = 开始 + 1 小时（跨午夜自动进位到第二天）
/// - 结束不得早于开始
fn normalize_times(
    start_date: &mut String,
    start_time: &mut String,
    end_date: &mut String,
    end_time: &mut String,
    all_day: &mut bool,
) -> CoreResult<()> {
    *start_date = parse_date(start_date)?;
    *end_date = parse_date(end_date)?;
    if *all_day || start_time.is_empty() {
        *all_day = true;
        start_time.clear();
        end_time.clear();
        if end_date < start_date {
            return Err(CoreError::validation(
                "CALENDAR_RANGE_INVALID",
                "结束日期不能早于开始日期",
            ));
        }
        return Ok(());
    }
    *start_time = parse_clock(start_time)?;
    if end_time.is_empty() {
        let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .and_then(|date| NaiveTime::parse_from_str(start_time, "%H:%M").map(|t| date.and_time(t)))
            .map_err(|_| CoreError::validation("CALENDAR_RANGE_INVALID", "开始时间无效"))?;
        let end = start + chrono::Duration::hours(1);
        *end_date = end.date().format("%Y-%m-%d").to_string();
        *end_time = end.time().format("%H:%M").to_string();
    } else {
        *end_time = parse_clock(end_time)?;
    }
    let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
        .and_then(|date| NaiveTime::parse_from_str(start_time, "%H:%M").map(|t| date.and_time(t)))
        .map_err(|_| CoreError::validation("CALENDAR_RANGE_INVALID", "开始时间无效"))?;
    let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
        .and_then(|date| NaiveTime::parse_from_str(end_time, "%H:%M").map(|t| date.and_time(t)))
        .map_err(|_| CoreError::validation("CALENDAR_RANGE_INVALID", "结束时间无效"))?;
    if end < start {
        return Err(CoreError::validation(
            "CALENDAR_RANGE_INVALID",
            "结束时间不能早于开始时间",
        ));
    }
    Ok(())
}

/// 提醒规则的写入前验证（与任务的 `reminders::normalize_metadata` 同一条纪律）：
/// 绝对时刻规范化成 UTC；**新增或改动**的规则必须落在未来（原样保留的过时规则不算错）；
/// 同一瞬时只留一条。`BeforeDue` 依附的是「事件开始时刻」（全天按 09:00 折算）。
fn normalize_reminders(
    rules: &mut Vec<Reminder>,
    event: &CalendarEvent,
    previous: Option<&CalendarEvent>,
    now: chrono::DateTime<chrono::Utc>,
) -> CoreResult<()> {
    let date = Some(event.start_date.as_str());
    let time = effective_start_time(event);
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::with_capacity(rules.len());
    for rule in rules.drain(..) {
        let mut rule = rule;
        if let Reminder::Absolute { at } = &mut rule {
            *at = crate::time::parse_instant(at)?;
        }
        let at = resolve(&rule, date, time)?;
        let unchanged = previous.is_some_and(|old| {
            old.reminders.iter().any(|old_rule| {
                old_rule == &rule
                    && resolve(
                        old_rule,
                        Some(old.start_date.as_str()),
                        effective_start_time(old),
                    )
                    .ok() == Some(at)
            })
        });
        if at <= now && !unchanged {
            return Err(CoreError::validation(
                "REMINDER_IN_PAST",
                "新增或修改的提醒必须在未来；已经错过的提醒不会补发",
            ));
        }
        if seen.insert(at.timestamp_millis()) {
            normalized.push(rule);
        }
    }
    *rules = normalized;
    Ok(())
}

/// 日期区间交叠过滤（`from`/`to` 都是闭区间，None = 不限）：
/// 事件的 [startDate, endDate] 与查询区间有交集即命中。
fn overlaps_range(event: &CalendarEvent, from: Option<&str>, to: Option<&str>) -> bool {
    if let Some(from) = from {
        if event.end_date.as_str() < from {
            return false;
        }
    }
    if let Some(to) = to {
        if event.start_date.as_str() > to {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

/// 日程侧保存后的详情插图清理（与日记同一套路）：对着**写入后**的日程文件扫
/// `img/data/calendar/`，删掉没有任何一条日程再引用的图片。清理失败一律吞掉。
fn sweep_calendar_images(ctx: &ExecContext, file: &CalendarFile) {
    let dir = ctx
        .repo
        .layout
        .entry_img_dir(crate::model::CALENDAR_IMAGE_NODE);
    let markdowns: Vec<&str> = file
        .events
        .iter()
        .map(|event| event.details.as_str())
        .collect();
    crate::image_gc::sweep_unreferenced(&dir, markdowns);
}

fn calendar_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let title = param_str(params, "title").unwrap_or_default();
    let details = param_str(params, "details").unwrap_or_default();
    let mut start_date = param_str(params, "startDate").unwrap_or_else(today_local);
    let mut start_time = param_str(params, "startTime").unwrap_or_default();
    let mut end_date = param_str(params, "endDate").unwrap_or_else(|| start_date.clone());
    let mut end_time = param_str(params, "endTime").unwrap_or_default();
    let mut all_day = param_bool(params, "allDay").unwrap_or(false);
    let mut reminders = parse_rules_opt(params.get("reminders"))?.unwrap_or_default();
    let task_id = param_str(params, "taskId").unwrap_or_default();
    normalize_times(&mut start_date, &mut start_time, &mut end_date, &mut end_time, &mut all_day)?;

    let mut created = Value::Null;
    let (file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let now = now_iso();
            let mut event = CalendarEvent {
                id: gen_id("cal"),
                title: title.chars().take(200).collect(),
                details,
                start_date,
                start_time,
                end_date,
                end_time,
                all_day,
                reminders: Vec::new(),
                task_id,
                created_at: now.clone(),
                updated_at: Some(now),
                extra: Map::new(),
            };
            normalize_reminders(&mut reminders, &event, None, chrono::Utc::now())?;
            event.reminders = reminders;
            let view = calendar_view(&event);
            file.events.push(event);
            sort_events(&mut file.events);
            created = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![]);
    sweep_calendar_images(ctx, &file);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(created));
    }
    Ok(created)
}

fn calendar_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_calendar()?;
    set_read_revision(meta, Domain::Calendar, file.meta.revision);
    let event = find_event(&file, &id).ok_or_else(|| not_found(&id))?;
    Ok(calendar_view(event))
}

fn calendar_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_calendar()?;
    set_read_revision(meta, Domain::Calendar, file.meta.revision);
    let date = param_str(params, "date").map(|raw| parse_date(&raw)).transpose()?;
    let from = param_str(params, "from").map(|raw| parse_date(&raw)).transpose()?;
    let to = param_str(params, "to").map(|raw| parse_date(&raw)).transpose()?;
    let task_id = param_str(params, "taskId");
    let page = unbounded_page_from(params)?;

    let mut events: Vec<CalendarEvent> = file
        .events
        .iter()
        .filter(|event| {
            if let Some(task_id) = &task_id {
                if &event.task_id != task_id {
                    return false;
                }
            }
            if let Some(date) = &date {
                return event.start_date <= *date && *date <= event.end_date;
            }
            overlaps_range(event, from.as_deref(), to.as_deref())
        })
        .cloned()
        .collect();
    sort_events(&mut events);
    // 与 diary/ledger list 同口径：data.total = 过滤后条数，不传 --limit 返回全部
    let (events, next_cursor, total) = paginate(events, &page);
    meta.count = Some(total);
    meta.next_cursor = next_cursor;
    Ok(json!({
        "total": total,
        "returned": events.len(),
        "items": events.iter().map(calendar_view).collect::<Vec<_>>(),
    }))
}

fn calendar_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let title = param_str(params, "title");
    let details = param_str(params, "details");
    let start_date = param_str(params, "startDate").map(|raw| parse_date(&raw)).transpose()?;
    let start_time = match param_str(params, "startTime") {
        Some(raw) => Some(if raw.trim().is_empty() {
            String::new()
        } else {
            parse_clock(&raw)?
        }),
        None => None,
    };
    let end_date = param_str(params, "endDate").map(|raw| parse_date(&raw)).transpose()?;
    let end_time = match param_str(params, "endTime") {
        Some(raw) => Some(if raw.trim().is_empty() {
            String::new()
        } else {
            parse_clock(&raw)?
        }),
        None => None,
    };
    let all_day = param_bool(params, "allDay");
    // reminders 的语义分界与 task modify 一致：没给 = 不动，空数组 = 清空
    let reminders = parse_rules_opt(params.get("reminders"))?;
    let task_id = param_str(params, "taskId");
    // 「挪动」语义：只改开始（日期或时刻）而没提结束时，结束跟着平移、保持原时长——
    // 与日历应用拖动事件的行为一致；要改时长就把结束一起给。
    let shift_end = (start_date.is_some() || start_time.is_some())
        && end_date.is_none()
        && end_time.is_none();

    let pre = ctx.repo.load_calendar()?;
    let previous = find_event(&pre, &id).ok_or_else(|| not_found(&id))?.clone();

    let mut updated = Value::Null;
    let (file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let event = file
                .events
                .iter_mut()
                .find(|event| event.id == id)
                .ok_or_else(|| not_found(&id))?;
            if let Some(title) = &title {
                event.title = title.chars().take(200).collect();
            }
            if let Some(details) = &details {
                event.details = details.clone();
            }
            if let Some(start_date) = &start_date {
                event.start_date = start_date.clone();
            }
            if let Some(start_time) = &start_time {
                event.start_time = start_time.clone();
            }
            if let Some(end_date) = &end_date {
                event.end_date = end_date.clone();
            }
            if let Some(end_time) = &end_time {
                event.end_time = end_time.clone();
            }
            if let Some(all_day) = all_day {
                event.all_day = all_day;
            }
            if let Some(task_id) = &task_id {
                event.task_id = task_id.clone();
            }
            if shift_end && all_day != Some(true) {
                shift_end_with(&previous, event);
            }
            normalize_times(
                &mut event.start_date,
                &mut event.start_time,
                &mut event.end_date,
                &mut event.end_time,
                &mut event.all_day,
            )?;
            if let Some(mut rules) = reminders.clone() {
                normalize_reminders(&mut rules, event, Some(&previous), chrono::Utc::now())?;
                event.reminders = rules;
            }
            event.updated_at = Some(now_iso());
            let view = calendar_view(event);
            updated = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![id.clone()]);
    sweep_calendar_images(ctx, &file);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}

/// 把 `event` 的结束时间按 `previous` 的时长平移（开始已经被这次 modify 改过）。
/// 定时事件按分钟差平移（跨午夜自动进位）；全天/无时刻事件按天数差平移。
fn shift_end_with(previous: &CalendarEvent, event: &mut CalendarEvent) {
    let timed_before = !previous.all_day && !previous.start_time.is_empty();
    let timed_now = !event.all_day && !event.start_time.is_empty();
    if timed_before && timed_now {
        let old_start = event_datetime(&previous.start_date, &previous.start_time);
        let old_end = event_datetime(&previous.end_date, &previous.end_time);
        let new_start = event_datetime(&event.start_date, &event.start_time);
        if let (Some(old_start), Some(old_end), Some(new_start)) = (old_start, old_end, new_start) {
            let duration = old_end - old_start;
            if duration > chrono::Duration::zero() {
                let new_end = new_start + duration;
                event.end_date = new_end.date().format("%Y-%m-%d").to_string();
                event.end_time = new_end.time().format("%H:%M").to_string();
                return;
            }
        }
    }
    // 全天（或退化成全天）：按天数差平移结束日期
    let old_start = NaiveDate::parse_from_str(&previous.start_date, "%Y-%m-%d").ok();
    let old_end = NaiveDate::parse_from_str(&previous.end_date, "%Y-%m-%d").ok();
    let new_start = NaiveDate::parse_from_str(&event.start_date, "%Y-%m-%d").ok();
    if let (Some(old_start), Some(old_end), Some(new_start)) = (old_start, old_end, new_start) {
        let span = old_end - old_start;
        if span >= chrono::Duration::zero() {
            event.end_date = (new_start + span).format("%Y-%m-%d").to_string();
        }
    }
}

fn event_datetime(date: &str, time: &str) -> Option<NaiveDateTime> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let time = NaiveTime::parse_from_str(time, "%H:%M").ok()?;
    Some(date.and_time(time))
}

fn calendar_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_calendar()?;
    let event = find_event(&file, &id).ok_or_else(|| not_found(&id))?;
    let label = if event.title.trim().is_empty() {
        format!("{} 的日程", event.start_date)
    } else {
        format!("日程「{}」", event.title)
    };
    let plan = json!({
        "type": "calendar",
        "id": id,
        "startDate": event.start_date,
        "calendarIds": [id],
    });
    require_confirmation(&inv.controls, format!("删除 {label}"), plan.clone())?;
    if inv.controls.dry_run {
        set_read_revision(meta, Domain::Calendar, file.meta.revision);
        return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
    }
    let (file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let index = file
                .events
                .iter()
                .position(|event| event.id == id)
                .ok_or_else(|| not_found(&id))?;
            file.events.remove(index);
            // 删除必须显式传播：服务器只见密文，没有墓碑对端会把它推回来。
            file.meta.record_tombstone(&id, "calendar", &now_iso());
            Ok(json!({ "removed": plan }))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![id.clone()]);
    sweep_calendar_images(ctx, &file);
    Ok(json!({ "removed": plan, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, start_date: &str, start_time: &str, end_date: &str, end_time: &str) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            title: id.to_string(),
            details: String::new(),
            start_date: start_date.to_string(),
            start_time: start_time.to_string(),
            end_date: end_date.to_string(),
            end_time: end_time.to_string(),
            all_day: start_time.is_empty(),
            reminders: Vec::new(),
            task_id: String::new(),
            created_at: "2026-10-01T00:00:00.000Z".to_string(),
            updated_at: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn normalize_times_marks_date_only_as_all_day() {
        let (mut sd, mut st, mut ed, mut et, mut all_day) = (
            "2026-10-25".to_string(),
            String::new(),
            "2026-10-25".to_string(),
            String::new(),
            false,
        );
        normalize_times(&mut sd, &mut st, &mut ed, &mut et, &mut all_day).unwrap();
        assert!(all_day);
        assert!(st.is_empty() && et.is_empty());
    }

    #[test]
    fn normalize_times_defaults_end_to_one_hour() {
        let (mut sd, mut st, mut ed, mut et, mut all_day) = (
            "2026-10-25".to_string(),
            "23:30".to_string(),
            "2026-10-25".to_string(),
            String::new(),
            false,
        );
        normalize_times(&mut sd, &mut st, &mut ed, &mut et, &mut all_day).unwrap();
        assert!(!all_day);
        assert_eq!(ed, "2026-10-26");
        assert_eq!(et, "00:30");
    }

    #[test]
    fn normalize_times_rejects_inverted_range() {
        let (mut sd, mut st, mut ed, mut et, mut all_day) = (
            "2026-10-25".to_string(),
            "10:00".to_string(),
            "2026-10-25".to_string(),
            "09:00".to_string(),
            false,
        );
        let error = normalize_times(&mut sd, &mut st, &mut ed, &mut et, &mut all_day).unwrap_err();
        assert_eq!(error.code, "CALENDAR_RANGE_INVALID");
    }

    #[test]
    fn overlaps_range_uses_inclusive_dates() {
        let multi = event("e", "2026-10-20", "", "2026-10-24", "");
        assert!(overlaps_range(&multi, Some("2026-10-24"), Some("2026-10-30")));
        assert!(overlaps_range(&multi, Some("2026-10-15"), Some("2026-10-20")));
        assert!(!overlaps_range(&multi, Some("2026-10-25"), Some("2026-10-30")));
        assert!(!overlaps_range(&multi, Some("2026-10-15"), Some("2026-10-19")));
    }

    #[test]
    fn sort_events_puts_all_day_first_within_a_day() {
        let mut events = vec![
            event("timed", "2026-10-25", "09:00", "2026-10-25", "10:00"),
            event("allday", "2026-10-25", "", "2026-10-25", ""),
            event("earlier", "2026-10-24", "18:00", "2026-10-24", "19:00"),
        ];
        sort_events(&mut events);
        let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["earlier", "allday", "timed"]);
    }
}

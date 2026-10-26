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
use crate::model::{
    CalendarCategory, CalendarEvent, CalendarFile, CalendarRepeat, Reminder, RepeatEnd, RepeatFreq,
    REPEAT_COUNT_MAX,
};
use crate::ops_config::is_hex_color;
use crate::ops_task::paginate;
use crate::recurrence;
use crate::reminders::{parse_rules_opt, resolve};
use crate::repo::Domain;
use crate::time::{now_iso, parse_clock, parse_date, today_local};

/// 全天事件的「开始时刻」折算（提醒用）：业界通行做法是按当天上午 9 点。
pub const ALL_DAY_REMINDER_CLOCK: &str = "09:00";

/// 只给 `--from` 不给 `--to` 时，重复日程按一年为界展开（开区间没法展开到永远）。
const OPEN_ENDED_HORIZON_DAYS: i64 = 366;

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
        "categories" => calendar_categories(inv, ctx, meta),
        "categoryAdd" => calendar_category_add(inv, ctx, meta),
        "categoryModify" => calendar_category_modify(inv, ctx, meta),
        "categoryRemove" => calendar_category_remove(inv, ctx, meta),
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
        "location": event.location,
        "categoryId": event.category_id,
        "repeat": serde_json::to_value(&event.repeat).unwrap_or(Value::Null),
        "repeatText": event.repeat.as_ref().map(recurrence::describe).unwrap_or_default(),
        "exceptDates": event.except_dates,
        "taskId": event.task_id,
        "createdAt": event.created_at,
        "updatedAt": event.updated_at,
    })
}

/// 列表中的一次**出现**（instance）：重复事件按 occurrence 平移后的日期，
/// 非重复事件的 occurrence 就是它自己的开始日期。
fn instance_view(event: &CalendarEvent, occurrence: NaiveDate) -> Value {
    let start = recurrence::parse_date(&event.start_date).unwrap_or(occurrence);
    let shift = (occurrence - start).num_days();
    let mut view = calendar_view(event);
    if shift != 0 {
        let map = view.as_object_mut().expect("calendar_view 产出对象");
        if let Some(date) = shift_date(&event.start_date, shift) {
            map.insert("startDate".to_string(), Value::String(date));
        }
        if let Some(date) = shift_date(&event.end_date, shift) {
            map.insert("endDate".to_string(), Value::String(date));
        }
    }
    let map = view.as_object_mut().expect("calendar_view 产出对象");
    map.insert(
        "occurrence".to_string(),
        Value::String(occurrence.format("%Y-%m-%d").to_string()),
    );
    map.insert("recurring".to_string(), Value::Bool(event.repeat.is_some()));
    view
}

fn shift_date(raw: &str, days: i64) -> Option<String> {
    let date = recurrence::parse_date(raw)?;
    Some(
        date.checked_add_signed(chrono::Duration::days(days))?
            .format("%Y-%m-%d")
            .to_string(),
    )
}

pub fn category_view(category: &CalendarCategory) -> Value {
    json!({
        "id": category.id,
        "name": category.name,
        "color": category.color,
        "isDefault": category.is_default,
        "order": category.order,
        "createdAt": category.created_at,
        "updatedAt": category.updated_at,
    })
}

/// 分类的稳定展示顺序：order 升序，再按创建先后与 id（与记账分类同款口径）。
pub fn sort_categories(categories: &mut [CalendarCategory]) {
    categories.sort_by(|a, b| {
        a.order
            .partial_cmp(&b.order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
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

/// 重复规则的预设串（CLI 与 agent 都能直接写「每周」这类词）。
/// 结构化对象走 serde，预设走这里；`none` = 不重复。
fn preset_repeat(raw: &str) -> CoreResult<Option<CalendarRepeat>> {
    let base = |freq: RepeatFreq, interval: u32| CalendarRepeat {
        freq,
        interval,
        by_weekday: Vec::new(),
        by_month_day: Vec::new(),
        end: RepeatEnd::Never,
        count: 0,
        until: String::new(),
        extra: Map::new(),
    };
    let text = raw.trim().to_ascii_lowercase();
    let repeat = match text.as_str() {
        "" | "none" | "不重复" => return Ok(None),
        "day" | "daily" | "每天" | "每日" => base(RepeatFreq::Day, 1),
        "weekdays" | "workdays" | "工作日" => {
            let mut item = base(RepeatFreq::Week, 1);
            item.by_weekday = vec![1, 2, 3, 4, 5];
            item
        }
        "week" | "weekly" | "每周" => base(RepeatFreq::Week, 1),
        "biweekly" | "fortnightly" | "每两周" | "隔周" => base(RepeatFreq::Week, 2),
        "month" | "monthly" | "每月" => base(RepeatFreq::Month, 1),
        "year" | "yearly" | "每年" => base(RepeatFreq::Year, 1),
        other => {
            return Err(CoreError::validation(
                "CALENDAR_REPEAT_INVALID",
                format!("未知重复预设 `{other}`"),
            )
            .with_hint("支持 none|daily|weekdays|weekly|biweekly|monthly|yearly，或传结构化 repeat 对象"))
        }
    };
    Ok(Some(repeat))
}

/// `repeat` 参数：字符串 = 预设，对象 = 结构化规则，缺省 = 不动。
/// 额外用 `repeatEvery` / `repeatDays` / `repeatMonthDays` / `repeatTimes` / `repeatUntil`
/// 细化预设（CLI 的 `--repeat-every` 等就是折成这几个键）。
fn parse_repeat(params: &Value) -> CoreResult<Option<Option<CalendarRepeat>>> {
    let raw = match params.get("repeat") {
        None => return Ok(None),
        Some(Value::Null) => return Ok(Some(None)),
        Some(value) => value,
    };
    let repeat = match raw {
        Value::String(text) => preset_repeat(text)?,
        other => {
            let parsed: CalendarRepeat =
                serde_json::from_value(other.clone()).map_err(|error| {
                    CoreError::validation(
                        "CALENDAR_REPEAT_INVALID",
                        format!("重复规则无效：{error}"),
                    )
                })?;
            Some(parsed)
        }
    };
    let Some(mut repeat) = repeat else {
        return Ok(Some(None));
    };
    if let Some(value) = params.get("repeatEvery").and_then(Value::as_u64) {
        repeat.interval = u32::try_from(value).unwrap_or(1);
    }
    if let Some(list) = params.get("repeatDays").and_then(Value::as_array) {
        repeat.by_weekday = list
            .iter()
            .filter_map(Value::as_u64)
            .filter_map(|value| u8::try_from(value).ok())
            .filter(|value| *value <= 6)
            .collect();
    }
    if let Some(list) = params.get("repeatMonthDays").and_then(Value::as_array) {
        repeat.by_month_day = list
            .iter()
            .filter_map(Value::as_u64)
            .filter_map(|value| u8::try_from(value).ok())
            .filter(|value| (1..=31).contains(value))
            .collect();
    }
    if let Some(value) = params.get("repeatTimes").and_then(Value::as_u64) {
        repeat.end = RepeatEnd::Count;
        repeat.count = u32::try_from(value).unwrap_or(REPEAT_COUNT_MAX);
    }
    if let Some(raw) = params.get("repeatUntil").and_then(Value::as_str) {
        if !raw.trim().is_empty() {
            repeat.end = RepeatEnd::Until;
            repeat.until = raw.trim().to_string();
        }
    }
    Ok(Some(Some(repeat)))
}

/// 分类解析：接受 ID 或名字（与记账分类同一条口径）。空串 = 无分类。
fn resolve_category_id(file: &CalendarFile, raw: &str) -> CoreResult<String> {
    let key = raw.trim();
    if key.is_empty() {
        return Ok(String::new());
    }
    file.categories
        .iter()
        .find(|item| item.id == key || item.name == key)
        .map(|item| item.id.clone())
        .ok_or_else(|| {
            CoreError::not_found(
                "CALENDAR_CATEGORY_NOT_FOUND",
                format!("未找到日程分类 `{raw}`"),
            )
            .with_hint("先运行 kxtodo-cli calendar categories 查看现有分类，或用 calendar category-add 新建")
        })
}

fn normalized_except_dates(raw: &Value) -> CoreResult<Vec<String>> {
    let Some(list) = raw.as_array() else {
        return Err(CoreError::validation(
            "CALENDAR_REPEAT_INVALID",
            "exceptDates 应是日期数组",
        ));
    };
    let mut out = Vec::new();
    for item in list {
        let Some(text) = item.as_str() else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let date = parse_date(text)?;
        if !out.contains(&date) {
            out.push(date);
        }
    }
    out.sort();
    Ok(out)
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
    let location = param_str(params, "location").unwrap_or_default().trim().to_string();
    normalize_times(&mut start_date, &mut start_time, &mut end_date, &mut end_time, &mut all_day)?;
    // 重复规则要在时间归一之后校验（until 与开始日期比）
    let mut repeat = match parse_repeat(params)? {
        Some(repeat) => recurrence::normalize_repeat(repeat, &start_date)?,
        None => None,
    };
    let except_dates = match params.get("exceptDates") {
        Some(raw) => normalized_except_dates(raw)?,
        None => Vec::new(),
    };
    if repeat.is_none() && !except_dates.is_empty() {
        return Err(CoreError::validation(
            "CALENDAR_REPEAT_INVALID",
            "不重复的日程不能有例外日期",
        ));
    }
    let pre = ctx.repo.load_calendar()?;
    // 「默认分类」由界面预选（用户看得见），core 不在写入时偷偷塞——
    // 否则 CLI 建的日程会带上用户从没选过的分类。
    let category_id = resolve_category_id(&pre, &param_str(params, "categoryId").unwrap_or_default())?;

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
                location,
                category_id,
                repeat: repeat.take(),
                except_dates,
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
    let category_id = param_str(params, "categoryId")
        .map(|raw| resolve_category_id(&file, &raw))
        .transpose()?;
    let page = unbounded_page_from(params)?;
    // 重复事件要有**闭区间**才能展开。只给 --from 时按一年为界（「每天重复到永远」
    // 展开到保险丝上限等于骗人，但只给起点的查询又确实该看到后面的实例）；
    // 只给 --to 或什么都不给时不展开，按系列本身输出（occurrence = 开始日期）。
    let window = match (&date, &from, &to) {
        (Some(day), _, _) => Some((day.clone(), day.clone())),
        (None, Some(start), Some(end)) => Some((start.clone(), end.clone())),
        (None, Some(start), None) => shift_date(start, OPEN_ENDED_HORIZON_DAYS)
            .map(|end| (start.clone(), end)),
        _ => None,
    };
    let expand = param_bool(params, "expand").unwrap_or(true) && window.is_some();

    let mut instances: Vec<(CalendarEvent, NaiveDate)> = Vec::new();
    for event in file.events.iter() {
        if let Some(task_id) = &task_id {
            if &event.task_id != task_id {
                continue;
            }
        }
        if let Some(category_id) = &category_id {
            if &event.category_id != category_id {
                continue;
            }
        }
        let Some(start) = recurrence::parse_date(&event.start_date) else {
            continue;
        };
        let bounded = window.as_ref().filter(|_| expand);
        let Some((from_raw, to_raw)) = bounded else {
            // 不展开：沿用 v0.9.0 的区间交叠过滤（--date 按闭区间命中那一天）
            let hit = match &date {
                Some(day) => event.start_date <= *day && *day <= event.end_date,
                None => overlaps_range(event, from.as_deref(), to.as_deref()),
            };
            if hit {
                instances.push((event.clone(), start));
            }
            continue;
        };
        let (Some(from_date), Some(to_date)) = (
            recurrence::parse_date(from_raw),
            recurrence::parse_date(to_raw),
        ) else {
            continue;
        };
        if event.repeat.is_none() {
            if overlaps_range(event, Some(from_raw.as_str()), Some(to_raw.as_str())) {
                instances.push((event.clone(), start));
            }
            continue;
        }
        // 跨天事件：开始日在窗口之前、身体还在窗口里的也要算（往前多扫一个身长）
        let span = span_days(event, start);
        let scan_from = shift_date(from_raw, -span)
            .and_then(|raw| recurrence::parse_date(&raw))
            .unwrap_or(from_date);
        for occurrence in recurrence::occurrences(event, scan_from, to_date) {
            let Some(end) = occurrence.checked_add_signed(chrono::Duration::days(span)) else {
                continue;
            };
            if end < from_date || occurrence > to_date {
                continue;
            }
            instances.push((event.clone(), occurrence));
        }
    }
    sort_instances(&mut instances);
    let total = instances.len();
    // 与 diary/ledger list 同口径：data.total = 过滤后条数，不传 --limit 返回全部
    let (instances, next_cursor, _) = paginate(instances, &page);
    meta.count = Some(total);
    meta.next_cursor = next_cursor;
    Ok(json!({
        "total": total,
        "returned": instances.len(),
        "items": instances
            .iter()
            .map(|(event, occurrence)| instance_view(event, *occurrence))
            .collect::<Vec<_>>(),
    }))
}

/// 事件的跨天天数（endDate - startDate，最小 0）。
fn span_days(event: &CalendarEvent, start: NaiveDate) -> i64 {
    recurrence::parse_date(&event.end_date)
        .map(|end| (end - start).num_days().max(0))
        .unwrap_or(0)
}

/// 实例的稳定展示顺序：出现日早的在前；同日全天在前、再按开始时刻与创建先后。
fn sort_instances(instances: &mut [(CalendarEvent, NaiveDate)]) {
    instances.sort_by(|(a, a_at), (b, b_at)| {
        a_at.cmp(b_at)
            .then_with(|| b.all_day.cmp(&a.all_day))
            .then_with(|| a.start_time.cmp(&b.start_time))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
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
    let location = param_str(params, "location").map(|raw| raw.trim().to_string());
    let repeat_param = parse_repeat(params)?;
    let except_dates = params
        .get("exceptDates")
        .map(normalized_except_dates)
        .transpose()?;
    // 「挪动」语义：只改开始（日期或时刻）而没提结束时，结束跟着平移、保持原时长——
    // 与日历应用拖动事件的行为一致；要改时长就把结束一起给。
    let shift_end = (start_date.is_some() || start_time.is_some())
        && end_date.is_none()
        && end_time.is_none();

    let pre = ctx.repo.load_calendar()?;
    let previous = find_event(&pre, &id).ok_or_else(|| not_found(&id))?.clone();
    // 分类在闭包外解析：闭包里 file 已经是 &mut，再借一次读会打架
    let category_id = params
        .get("categoryId")
        .map(|raw| resolve_category_id(&pre, raw.as_str().unwrap_or_default()))
        .transpose()?;

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
            if let Some(location) = &location {
                event.location = location.clone();
            }
            if let Some(category_id) = &category_id {
                event.category_id = category_id.clone();
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
            // 重复规则在时间之后归一：until 要跟（可能刚被改过的）开始日期比
            if let Some(next) = repeat_param.clone() {
                event.repeat = recurrence::normalize_repeat(next, &event.start_date)?;
                if event.repeat.is_none() {
                    event.except_dates.clear();
                }
            }
            if let Some(dates) = except_dates.clone() {
                if event.repeat.is_none() && !dates.is_empty() {
                    return Err(CoreError::validation(
                        "CALENDAR_REPEAT_INVALID",
                        "不重复的日程不能有例外日期",
                    ));
                }
                event.except_dates = dates;
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

/// 删除的三档语义（业界日历通行做法）：
/// - 不给 occurrence = 删整个系列；
/// - `occurrence` = 只删这一次（写进 exceptDates，系列还在）；
/// - `fromOccurrence` = 删这次及以后（把 repeat.until 收到它前一天；收到开始日之前就整条删）。
fn calendar_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let occurrence = param_str(params, "occurrence")
        .map(|raw| parse_date(&raw))
        .transpose()?;
    let from_occurrence = param_str(params, "fromOccurrence")
        .map(|raw| parse_date(&raw))
        .transpose()?;
    if occurrence.is_some() && from_occurrence.is_some() {
        return Err(CoreError::validation(
            "CALENDAR_OCCURRENCE_CONFLICT",
            "occurrence 与 fromOccurrence 只能给一个",
        ));
    }
    let file = ctx.repo.load_calendar()?;
    let event = find_event(&file, &id).ok_or_else(|| not_found(&id))?;
    let repeating = event.repeat.is_some();
    if (occurrence.is_some() || from_occurrence.is_some()) && !repeating {
        return Err(CoreError::validation(
            "CALENDAR_NOT_RECURRING",
            "这条日程不重复，不能按「某一次」删除",
        ));
    }
    // 「这次及以后」收到开始日之前 = 整个系列都没了 → 直接按系列删
    let truncate_at = from_occurrence
        .as_ref()
        .and_then(|raw| recurrence::parse_date(raw)?.checked_sub_signed(chrono::Duration::days(1)));
    let whole_series = occurrence.is_none()
        && truncate_at
            .map(|until| {
                recurrence::parse_date(&event.start_date)
                    .map(|start| until < start)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
    let scope = if whole_series {
        "series"
    } else if occurrence.is_some() {
        "occurrence"
    } else {
        "tail"
    };
    let base_label = if event.title.trim().is_empty() {
        format!("{} 的日程", event.start_date)
    } else {
        format!("日程「{}」", event.title)
    };
    let label = match scope {
        "occurrence" => format!(
            "删除 {base_label} 的 {} 这一次",
            occurrence.clone().unwrap_or_default()
        ),
        "tail" => format!(
            "删除 {base_label} 从 {} 起的后续全部",
            from_occurrence.clone().unwrap_or_default()
        ),
        _ => format!("删除 {base_label}"),
    };
    let plan = json!({
        "type": "calendar",
        "id": id,
        "scope": scope,
        "startDate": event.start_date,
        "occurrence": occurrence,
        "fromOccurrence": from_occurrence,
        "calendarIds": if whole_series { json!([id]) } else { json!([]) },
    });
    require_confirmation(&inv.controls, label, plan.clone())?;
    if inv.controls.dry_run {
        set_read_revision(meta, Domain::Calendar, file.meta.revision);
        return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
    }
    let except_date = occurrence.clone();
    let new_until = truncate_at.map(|date| date.format("%Y-%m-%d").to_string());
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
            if whole_series {
                file.events.remove(index);
                // 删除必须显式传播：服务器只见密文，没有墓碑对端会把它推回来。
                file.meta.record_tombstone(&id, "calendar", &now_iso());
                return Ok(json!({ "removed": plan }));
            }
            let target = &mut file.events[index];
            if let Some(date) = &except_date {
                if !target.except_dates.contains(date) {
                    target.except_dates.push(date.clone());
                    target.except_dates.sort();
                }
            }
            if let Some(until) = &new_until {
                if let Some(repeat) = target.repeat.as_mut() {
                    repeat.end = RepeatEnd::Until;
                    repeat.until = until.clone();
                    repeat.count = 0;
                }
            }
            target.updated_at = Some(now_iso());
            Ok(json!({ "removed": plan }))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![id.clone()]);
    sweep_calendar_images(ctx, &file);
    Ok(json!({ "removed": plan, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// 分类（v0.9.1）
// ---------------------------------------------------------------------------

/// 分类名上限：够写「团队周会」这类名字，又不至于把色块挤爆。
const CATEGORY_NAME_MAX: usize = 24;

fn calendar_categories(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let _ = inv;
    let file = ctx.repo.load_calendar()?;
    set_read_revision(meta, Domain::Calendar, file.meta.revision);
    let mut categories = file.categories.clone();
    sort_categories(&mut categories);
    let items: Vec<Value> = categories
        .iter()
        .map(|category| {
            let mut view = category_view(category);
            let used = file
                .events
                .iter()
                .filter(|event| event.category_id == category.id)
                .count();
            view["used"] = json!(used);
            view
        })
        .collect();
    let total = items.len();
    Ok(json!({ "total": total, "returned": total, "items": items }))
}

fn validate_category_name(raw: &str) -> CoreResult<String> {
    let name: String = raw.trim().chars().take(CATEGORY_NAME_MAX).collect();
    if name.is_empty() {
        return Err(CoreError::validation(
            "CALENDAR_CATEGORY_NAME_EMPTY",
            "分类名不能为空",
        ));
    }
    Ok(name)
}

fn validate_category_color(raw: &str) -> CoreResult<String> {
    let color = raw.trim();
    if color.is_empty() {
        return Ok(String::new());
    }
    if !is_hex_color(color) {
        return Err(CoreError::validation(
            "CALENDAR_CATEGORY_COLOR_INVALID",
            format!("分类颜色 `{color}` 无效，应为 #rrggbb"),
        ));
    }
    Ok(color.to_ascii_lowercase())
}

fn category_not_found(id: &str) -> CoreError {
    CoreError::not_found(
        "CALENDAR_CATEGORY_NOT_FOUND",
        format!("未找到日程分类 {id}"),
    )
    .with_hint("先运行 kxtodo-cli calendar categories 查看现有分类")
}

fn calendar_category_add(
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let params = &inv.params;
    let name = validate_category_name(&required_str(params, "name")?)?;
    let color = validate_category_color(&param_str(params, "color").unwrap_or_default())?;
    let is_default = param_bool(params, "isDefault").unwrap_or(false);
    let mut created = Value::Null;
    let (_file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if file.categories.iter().any(|item| item.name == name) {
                return Err(CoreError::validation(
                    "CALENDAR_CATEGORY_EXISTS",
                    format!("日程分类 `{name}` 已存在"),
                ));
            }
            let now = now_iso();
            let order = file
                .categories
                .iter()
                .fold(0.0_f64, |max, item| max.max(item.order))
                + 1.0;
            let category = CalendarCategory {
                id: gen_id("ccat"),
                name: name.clone(),
                color: color.clone(),
                is_default,
                order,
                created_at: now.clone(),
                updated_at: Some(now),
                extra: Map::new(),
            };
            if is_default {
                for item in file.categories.iter_mut() {
                    item.is_default = false;
                }
            }
            let view = category_view(&category);
            file.categories.push(category);
            sort_categories(&mut file.categories);
            created = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(created));
    }
    Ok(created)
}

fn calendar_category_modify(
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let name = param_str(params, "name")
        .map(|raw| validate_category_name(&raw))
        .transpose()?;
    let color = param_str(params, "color")
        .map(|raw| validate_category_color(&raw))
        .transpose()?;
    let is_default = param_bool(params, "isDefault");
    let order = params.get("order").and_then(Value::as_f64);
    let pre = ctx.repo.load_calendar()?;
    if !pre.categories.iter().any(|item| item.id == id) {
        return Err(category_not_found(&id));
    }
    let mut updated = Value::Null;
    let (_file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if let Some(name) = &name {
                if file
                    .categories
                    .iter()
                    .any(|item| item.id != id && item.name == *name)
                {
                    return Err(CoreError::validation(
                        "CALENDAR_CATEGORY_EXISTS",
                        format!("日程分类 `{name}` 已存在"),
                    ));
                }
            }
            // 「设为默认」是全局互斥的：先把别的都摘掉
            if is_default == Some(true) {
                for item in file.categories.iter_mut() {
                    item.is_default = false;
                }
            }
            let category = file
                .categories
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| category_not_found(&id))?;
            if let Some(name) = name {
                category.name = name;
            }
            if let Some(color) = color {
                category.color = color;
            }
            if let Some(is_default) = is_default {
                category.is_default = is_default;
            }
            if let Some(order) = order {
                category.order = order;
            }
            category.updated_at = Some(now_iso());
            let view = category_view(category);
            updated = view.clone();
            sort_categories(&mut file.categories);
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![id.clone()]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}

/// 删分类不删日程：用它的日程退回「无分类」（各视图用日程主题色）。
/// 与记账「名下有账的账户不让删」不同——分类只是外观归属，不该拦着用户整理。
fn calendar_category_remove(
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_calendar()?;
    let category = file
        .categories
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| category_not_found(&id))?;
    let used = file
        .events
        .iter()
        .filter(|event| event.category_id == id)
        .count();
    let plan = json!({
        "type": "calendarCategory",
        "id": id,
        "name": category.name,
        "detachedEvents": used,
    });
    require_confirmation(
        &inv.controls,
        format!("删除日程分类「{}」", category.name),
        plan.clone(),
    )?;
    if inv.controls.dry_run {
        set_read_revision(meta, Domain::Calendar, file.meta.revision);
        return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
    }
    let (_file, outcome) = ctx.repo.write_calendar(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let index = file
                .categories
                .iter()
                .position(|item| item.id == id)
                .ok_or_else(|| category_not_found(&id))?;
            file.categories.remove(index);
            let now = now_iso();
            for event in file.events.iter_mut() {
                if event.category_id == id {
                    event.category_id.clear();
                    event.updated_at = Some(now.clone());
                }
            }
            file.meta.record_tombstone(&id, "calendarCategory", &now);
            Ok(json!({ "removed": plan }))
        },
    )?;
    apply_write_outcome(meta, Domain::Calendar, &outcome);
    notify_host(ctx, Domain::Calendar, outcome.revision, vec![id.clone()]);
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
            location: String::new(),
            category_id: String::new(),
            repeat: None,
            except_dates: Vec::new(),
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

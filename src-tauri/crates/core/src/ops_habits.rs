//! habit domain: 习惯打卡的业务操作（GUI 与 CLI 共用同一条命令层，v0.9.3）。
//!
//! 习惯住在 **data.json** 里（`DataFile.habits`，与草稿纸同谱系：它是内容不是配置），
//! 写路径抬高 data 域 revision；同步是逐习惯一个 LWW 实体（kind = habit），
//! 搭「同步数据」范围的车，没有独立勾选。统计（连续/累计/完成率）不落盘，前端现算。

use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_str, require_confirmation, required_str,
    set_read_revision, ExecContext, Invocation,
};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::ledger_icons::is_known_icon;
use crate::model::{Habit, HabitFreq, HabitFreqKind, HabitStatus, Reminder};
use crate::ops_config::is_hex_color;
use crate::reminders::{parse_rules_opt, resolve};
use crate::repo::Domain;
use crate::time::{now_iso, parse_clock, parse_date, today_local};

pub fn habit_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => habit_add(inv, ctx, meta),
        "get" => habit_get(inv, ctx, meta),
        "list" => habit_list(inv, ctx, meta),
        "modify" => habit_modify(inv, ctx, meta),
        "remove" => habit_remove(inv, ctx, meta),
        "check" => habit_check(inv, ctx, meta, true),
        "uncheck" => habit_check(inv, ctx, meta, false),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 habit 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn not_found(id: &str) -> CoreError {
    CoreError::not_found("HABIT_NOT_FOUND", format!("未找到习惯 {id}"))
        .with_hint("先运行 kxtodo-cli habit list 获取稳定 ID")
}

fn habit_view(habit: &Habit) -> Value {
    serde_json::to_value(habit).unwrap_or(Value::Null)
}

fn validate_name(raw: &str) -> CoreResult<String> {
    let name: String = raw.trim().chars().take(40).collect();
    if name.is_empty() {
        return Err(CoreError::validation(
            "HABIT_NAME_INVALID",
            "习惯名称不能为空",
        ));
    }
    Ok(name)
}

fn validate_color(raw: &str) -> CoreResult<String> {
    let color = raw.trim();
    if color.is_empty() {
        return Ok(String::new());
    }
    if !is_hex_color(color) {
        return Err(CoreError::validation(
            "HABIT_COLOR_INVALID",
            format!("习惯颜色 `{color}` 无效，应为 #rrggbb"),
        ));
    }
    Ok(color.to_ascii_lowercase())
}

fn validate_icon(raw: &str) -> CoreResult<String> {
    let icon = raw.trim();
    if icon.is_empty() {
        return Ok(String::new());
    }
    if !is_known_icon(icon) {
        return Err(CoreError::validation(
            "HABIT_ICON_INVALID",
            format!("图标 `{icon}` 不在图标目录里"),
        ));
    }
    Ok(icon.to_string())
}

fn validate_time(raw: &str) -> CoreResult<String> {
    let time = raw.trim();
    if time.is_empty() {
        return Ok(String::new());
    }
    parse_clock(time).map_err(|_| {
        CoreError::validation("HABIT_TIME_INVALID", format!("计划时刻 `{time}` 无效，应为 HH:MM"))
    })
}

/// freq 载荷：{kind: daily|weekly, weekdays: [0-6]}；weekly 必须选至少一天。
fn parse_freq(value: Option<&Value>) -> CoreResult<HabitFreq> {
    let Some(value) = value else {
        return Ok(HabitFreq::default());
    };
    let kind = match value.get("kind").and_then(Value::as_str).unwrap_or("daily") {
        "daily" => HabitFreqKind::Daily,
        "weekly" => HabitFreqKind::Weekly,
        other => {
            return Err(CoreError::validation(
                "HABIT_FREQ_INVALID",
                format!("频率 `{other}` 无效，应为 daily 或 weekly"),
            ))
        }
    };
    let mut weekdays: Vec<u8> = Vec::new();
    if let Some(raw) = value.get("weekdays").and_then(Value::as_array) {
        for day in raw {
            let Some(day) = day.as_u64() else {
                return Err(CoreError::validation(
                    "HABIT_FREQ_INVALID",
                    "weekdays 只能是 0-6 的整数（0=周日）",
                ));
            };
            if day > 6 {
                return Err(CoreError::validation(
                    "HABIT_FREQ_INVALID",
                    "weekdays 只能是 0-6 的整数（0=周日）",
                ));
            }
            let day = day as u8;
            if !weekdays.contains(&day) {
                weekdays.push(day);
            }
        }
    }
    weekdays.sort_unstable();
    if kind == HabitFreqKind::Weekly && weekdays.is_empty() {
        return Err(CoreError::validation(
            "HABIT_FREQ_INVALID",
            "每周习惯至少要选一个星期几",
        ));
    }
    Ok(HabitFreq { kind, weekdays })
}

fn parse_status(value: Option<&Value>) -> CoreResult<Option<HabitStatus>> {
    match value.and_then(Value::as_str) {
        None => Ok(None),
        Some("active") => Ok(Some(HabitStatus::Active)),
        Some("paused") => Ok(Some(HabitStatus::Paused)),
        Some("done") => Ok(Some(HabitStatus::Done)),
        Some(other) => Err(CoreError::validation(
            "HABIT_STATUS_INVALID",
            format!("状态 `{other}` 无效，应为 active / paused / done"),
        )),
    }
}

/// 提醒校验：Absolute 必须还在未来（与日程同口径）；BeforeDue 依附计划时刻恒可解析。
fn validate_reminders(rules: &[Reminder], habit: &Habit) -> CoreResult<()> {
    let now = chrono::Utc::now();
    for rule in rules {
        if let Reminder::Absolute { .. } = rule {
            let at = resolve(rule, Some(&today_local()), habit.effective_time())?;
            if at <= now {
                return Err(CoreError::validation(
                    "REMINDER_IN_PAST",
                    "自定义提醒时刻已经过去",
                ));
            }
        }
    }
    Ok(())
}

pub fn sort_habits(habits: &mut [Habit]) {
    habits.sort_by(|a, b| {
        a.order
            .partial_cmp(&b.order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

fn habit_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let name = validate_name(&required_str(params, "name")?)?;
    let color = validate_color(&param_str(params, "color").unwrap_or_default())?;
    let icon = validate_icon(&param_str(params, "icon").unwrap_or_default())?;
    let freq = parse_freq(params.get("freq"))?;
    let time = validate_time(&param_str(params, "time").unwrap_or_default())?;
    let reminders = parse_rules_opt(params.get("reminders"))?.unwrap_or_default();
    let status = parse_status(params.get("status"))?.unwrap_or(HabitStatus::Active);
    let mut created = Value::Null;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if file.habits.iter().any(|item| item.name == name) {
                return Err(CoreError::validation(
                    "HABIT_EXISTS",
                    format!("习惯 `{name}` 已存在"),
                ));
            }
            let now = now_iso();
            let order = file.habits.iter().fold(0.0_f64, |max, item| max.max(item.order)) + 1.0;
            let habit = Habit {
                id: gen_id("habit"),
                name: name.clone(),
                color: color.clone(),
                icon: icon.clone(),
                freq: freq.clone(),
                time: time.clone(),
                reminders: reminders.clone(),
                status,
                order,
                checkins: Vec::new(),
                created_at: now.clone(),
                updated_at: Some(now),
                extra: Map::new(),
            };
            validate_reminders(&habit.reminders, &habit)?;
            let view = habit_view(&habit);
            file.habits.push(habit);
            sort_habits(&mut file.habits);
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

fn habit_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, file.meta.revision);
    let habit = file.habits.iter().find(|item| item.id == id).ok_or_else(|| not_found(&id))?;
    Ok(habit_view(habit))
}

fn habit_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let status = parse_status(inv.params.get("status"))?;
    let file = ctx.repo.load_data()?;
    set_read_revision(meta, Domain::Data, file.meta.revision);
    let habits: Vec<Value> = file
        .habits
        .iter()
        .filter(|habit| status.map_or(true, |want| habit.status == want))
        .map(habit_view)
        .collect();
    Ok(json!({ "habits": habits, "count": habits.len() }))
}

fn habit_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let name = match params.get("name") {
        Some(value) => Some(validate_name(value.as_str().unwrap_or_default())?),
        None => None,
    };
    let color = match params.get("color") {
        Some(value) => Some(validate_color(value.as_str().unwrap_or_default())?),
        None => None,
    };
    let icon = match params.get("icon") {
        Some(value) => Some(validate_icon(value.as_str().unwrap_or_default())?),
        None => None,
    };
    let freq = match params.get("freq") {
        Some(value) => Some(parse_freq(Some(value))?),
        None => None,
    };
    let time = match params.get("time") {
        Some(value) => Some(validate_time(value.as_str().unwrap_or_default())?),
        None => None,
    };
    let reminders = parse_rules_opt(params.get("reminders"))?;
    let status = parse_status(params.get("status"))?;
    let order = params.get("order").and_then(Value::as_f64);
    let mut updated = Value::Null;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if let Some(name) = &name {
                if file.habits.iter().any(|item| item.id != id && item.name == *name) {
                    return Err(CoreError::validation(
                        "HABIT_EXISTS",
                        format!("习惯 `{name}` 已存在"),
                    ));
                }
            }
            let habit = file
                .habits
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| not_found(&id))?;
            if let Some(name) = &name {
                habit.name = name.clone();
            }
            if let Some(color) = &color {
                habit.color = color.clone();
            }
            if let Some(icon) = &icon {
                habit.icon = icon.clone();
            }
            if let Some(freq) = &freq {
                habit.freq = freq.clone();
            }
            if let Some(time) = &time {
                habit.time = time.clone();
            }
            if let Some(reminders) = &reminders {
                habit.reminders = reminders.clone();
            }
            if let Some(status) = status {
                habit.status = status;
            }
            if let Some(order) = order {
                habit.order = order;
            }
            validate_reminders(&habit.reminders, habit)?;
            habit.updated_at = Some(now_iso());
            let view = habit_view(habit);
            sort_habits(&mut file.habits);
            updated = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}

fn habit_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let existing = ctx.repo.load_data()?;
    let habit = existing
        .habits
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| not_found(&id))?;
    let label = format!("删除习惯「{}」（{} 次打卡记录一并删除）", habit.name, habit.checkins.len());
    let plan = json!({ "id": habit.id, "name": habit.name, "checkins": habit.checkins.len() });
    require_confirmation(&inv.controls, label, plan.clone())?;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let before = file.habits.len();
            file.habits.retain(|item| item.id != id);
            if file.habits.len() == before {
                return Err(not_found(&id));
            }
            file.meta.record_tombstone(&id, "habit", &now_iso());
            Ok(idem_summary(&plan))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![]);
    Ok(json!({ "id": id, "removed": true }))
}

/// check / uncheck：打卡日期默认今天；只有 active 且当天在计划内才能打卡。
fn habit_check(
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
    checked: bool,
) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let date = match param_str(params, "date") {
        Some(raw) if !raw.trim().is_empty() => parse_date(&raw)?,
        _ => today_local(),
    };
    // 已是目标态就不写盘（幂等读路径）：否则连点两次打卡会白抬 revision + 灌审计
    let existing = ctx.repo.load_data()?;
    let habit = existing
        .habits
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| not_found(&id))?;
    if habit.status != HabitStatus::Active {
        return Err(CoreError::validation(
            "HABIT_NOT_ACTIVE",
            "已暂停/已完成的习惯不能打卡，先恢复进行中",
        ));
    }
    if !habit.scheduled_on(&date) {
        return Err(CoreError::validation(
            "HABIT_NOT_SCHEDULED",
            format!("{date} 不在习惯「{}」的计划内", habit.name),
        ));
    }
    let had = habit.checkins.iter().any(|item| item == &date);
    if had == checked {
        set_read_revision(meta, Domain::Data, existing.meta.revision);
        return Ok(habit_view(habit));
    }
    let mut updated = Value::Null;
    let (_file, outcome) = ctx.repo.write_data(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let habit = file
                .habits
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| not_found(&id))?;
            if checked {
                habit.checkins.push(date.clone());
                habit.checkins.sort();
                habit.checkins.dedup();
            } else {
                habit.checkins.retain(|item| item != &date);
            }
            habit.updated_at = Some(now_iso());
            let view = habit_view(habit);
            updated = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Data, &outcome);
    notify_host(ctx, Domain::Data, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}
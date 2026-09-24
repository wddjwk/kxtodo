//! Pure trigger planning, local-day expiry and due-time window checks.

use std::str::FromStr;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Utc};
use cron::Schedule;
use crate::error::{CoreError, CoreResult};
use crate::model::{ScheduleEntry, ScheduleSpec, Trigger};
use crate::time::{format_instant, parse_duration_ms, parse_stored_instant};

/// Five-field cron uses standard weekdays (0/7 Sunday); the cron crate uses 1 Sunday.
/// Six/seven-field expressions retain the native cron crate syntax.
pub fn normalize_cron(expr: &str) -> CoreResult<String> {
    let mut fields: Vec<String> = expr.split_whitespace().map(str::to_string).collect();
    match fields.len() {
        5 => {
            fields[4] = standard_weekdays(&fields[4])?;
            Ok(format!("0 {}", fields.join(" ")))
        }
        6 | 7 => Ok(fields.join(" ")),
        _ => Err(CoreError::validation("INVALID_CRON", "cron 应为 5、6 或 7 段")),
    }
}

fn standard_weekdays(raw: &str) -> CoreResult<String> {
    if raw == "*" || raw == "?" { return Ok(raw.to_string()); }
    let invalid = || CoreError::validation("INVALID_CRON", "cron 星期必须为 0–7 或 SUN–SAT");
    let number = |s: &str| -> CoreResult<u8> {
        let named = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        named.iter().position(|n| *n == s.to_ascii_uppercase())
            .map(|n| n as u8).or_else(|| s.parse::<u8>().ok().filter(|n| *n <= 7))
            .ok_or_else(invalid)
    };
    let mut days = [false; 7];
    for part in raw.split(',') {
        let mut step_parts = part.split('/');
        let range = step_parts.next().unwrap_or("");
        let step = match step_parts.next() {
            Some(s) => s.parse::<usize>().ok().filter(|n| *n > 0 && *n <= 7).ok_or_else(invalid)?,
            None => 1,
        };
        if step_parts.next().is_some() { return Err(invalid()); }
        let (start, end) = if range == "*" { (0, 6) }
            else if let Some((a, b)) = range.split_once('-') { (number(a)?, number(b)?) }
            else { let n = number(range)?; (n, if part.contains('/') { 7 } else { n }) };
        if start > end { return Err(invalid()); }
        for n in (start..=end).step_by(step) { days[(n % 7) as usize] = true; }
    }
    Ok(days.iter().enumerate().filter(|(_, yes)| **yes)
        .map(|(n, _)| (n + 1).to_string()).collect::<Vec<_>>().join(","))
}

pub fn validate_cron(expr: &str) -> CoreResult<()> {
    Schedule::from_str(&normalize_cron(expr)?).map_err(|error|
        CoreError::validation("INVALID_CRON", format!("cron 无效：{error}")))?;
    Ok(())
}

pub fn validate_timezone(tz: &str) -> CoreResult<chrono_tz::Tz> {
    tz.parse().map_err(|_| CoreError::validation("INVALID_TIMEZONE", format!("无效 IANA 时区：{tz}")))
}

/// Checked before conversion to chrono/i64; no wrapping casts for hostile durations.
pub fn schedule_duration(raw: &str) -> CoreResult<Duration> {
    let ms = parse_duration_ms(raw)?;
    let ms = i64::try_from(ms).map_err(|_| CoreError::validation("INVALID_DURATION", "时长过大"))?;
    let duration = Duration::try_milliseconds(ms)
        .ok_or_else(|| CoreError::validation("INVALID_DURATION", "时长过大"))?;
    Utc::now().checked_add_signed(duration)
        .ok_or_else(|| CoreError::validation("INVALID_DURATION", "时长超出日期范围"))?;
    Ok(duration)
}

pub fn until_date(raw: &str) -> CoreResult<NaiveDate> {
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| CoreError::validation("INVALID_UNTIL", "until 应为 YYYY-MM-DD"))?;
    if date.format("%Y-%m-%d").to_string() != raw || date.succ_opt().is_none() {
        return Err(CoreError::validation("INVALID_UNTIL", "until 应为有效 YYYY-MM-DD"));
    }
    Ok(date)
}

fn local_parts(spec: &ScheduleSpec, now: DateTime<Utc>) -> CoreResult<(NaiveDate, u32)> {
    if let Trigger::Calendar { timezone, .. } = &spec.trigger {
        let local = now.with_timezone(&validate_timezone(timezone)?);
        Ok((local.date_naive(), local.hour() * 60 + local.minute()))
    } else {
        let local = now.with_timezone(&Local);
        Ok((local.date_naive(), local.hour() * 60 + local.minute()))
    }
}

/// Comparing local dates implements inclusive-day expiry, including DST midnight transitions.
pub fn is_expired(spec: &ScheduleSpec, now: DateTime<Utc>) -> CoreResult<bool> {
    match &spec.until {
        Some(until) => Ok(local_parts(spec, now)?.0 > until_date(until)?),
        None => Ok(false),
    }
}

pub fn window_minute(raw: &str) -> CoreResult<u32> {
    let bytes = raw.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' || !bytes.iter().enumerate()
        .all(|(i, b)| i == 2 || b.is_ascii_digit()) {
        return Err(CoreError::validation("INVALID_GATE_WINDOW", "时间窗口应使用 HH:MM"));
    }
    let h: u32 = raw[..2].parse().unwrap();
    let m: u32 = raw[3..].parse().unwrap();
    if h > 23 || m > 59 { return Err(CoreError::validation("INVALID_GATE_WINDOW", "时间窗口时刻超出范围")); }
    Ok(h * 60 + m)
}

/// Windows are OR; probe (if any) is AND. Start inclusive, end exclusive.
/// An overnight window uses the weekday on which it started.
pub fn gate_window_allows(spec: &ScheduleSpec, now: DateTime<Utc>) -> CoreResult<bool> {
    let Some(gate) = &spec.gate else { return Ok(true); };
    if gate.windows.is_empty() { return Ok(true); }
    let (date, minute) = local_parts(spec, now)?;
    let weekday = date.weekday().num_days_from_sunday() as u8;
    for window in &gate.windows {
        let start = window_minute(&window.start)?;
        let end = window_minute(&window.end)?;
        let (in_window, day) = if start < end { (minute >= start && minute < end, weekday) }
            else { (minute >= start || minute < end, if minute < end { (weekday + 6) % 7 } else { weekday }) };
        if in_window && window.weekdays.as_ref().map(|days| days.contains(&day)).unwrap_or(true) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_duration(base: DateTime<Utc>, duration: Duration) -> CoreResult<DateTime<Utc>> {
    base.checked_add_signed(duration).ok_or_else(|| CoreError::validation("INVALID_DURATION", "下次运行超出日期范围"))
}

pub fn compute_next_run(entry: &ScheduleEntry, now: DateTime<Utc>) -> CoreResult<Option<DateTime<Utc>>> {
    if !entry.spec.enabled || is_expired(&entry.spec, now)? { return Ok(None); }
    let next = match &entry.spec.trigger {
        Trigger::Once { at, .. } => {
            if entry.state.run_count > 0 { return Ok(None); }
            parse_stored_instant(at)?
        }
        Trigger::Interval { every, max_runs, .. } => {
            if max_runs.map(|max| entry.state.run_count >= max).unwrap_or(false) { return Ok(None); }
            let step = schedule_duration(every)?;
            let base = entry.state.last_run_at.as_deref()
                .or_else(|| (!entry.updated_at.is_empty()).then_some(entry.updated_at.as_str()))
                .or_else(|| (!entry.created_at.is_empty()).then_some(entry.created_at.as_str()));
            let base = base.map(parse_stored_instant).transpose()?.unwrap_or(now);
            let first = add_duration(base, step)?;
            if first > now { first } else {
                // Advance without overflow-prone multiplication of elapsed steps.
                let remainder = (now - base).num_milliseconds() % step.num_milliseconds();
                add_duration(now, Duration::milliseconds(step.num_milliseconds() - remainder))?
            }
        }
        Trigger::Calendar { cron, timezone, .. } => {
            let schedule = Schedule::from_str(&normalize_cron(cron)?).map_err(|e|
                CoreError::validation("INVALID_CRON", e.to_string()))?;
            let tz = validate_timezone(timezone)?;
            let Some(next) = schedule.after(&now.with_timezone(&tz)).next() else { return Ok(None); };
            next.with_timezone(&Utc)
        }
        Trigger::Condition { every, cooldown, .. } => {
            let step = schedule_duration(every)?;
            let base = entry.state.last_probe.as_ref()
                .and_then(|probe| parse_stored_instant(&probe.at).ok()).unwrap_or(now);
            let mut next = add_duration(base, step)?;
            if let (Some(cooldown), Some(last)) = (cooldown, &entry.state.last_run_at) {
                next = next.max(add_duration(parse_stored_instant(last)?, step.max(schedule_duration(cooldown)?))?);
            }
            if next <= now { add_duration(now, step)? } else { next }
        }
    };
    if is_expired(&entry.spec, next)? { Ok(None) } else { Ok(Some(next)) }
}

pub fn compute_next_run_iso(entry: &ScheduleEntry, now: DateTime<Utc>) -> CoreResult<Option<String>> {
    Ok(compute_next_run(entry, now)?.map(format_instant))
}

/// A rejected gate consumes only this tick, never a run or the last execution status.
pub fn advance_skipped_gate(entry: &mut ScheduleEntry, now: DateTime<Utc>) -> CoreResult<()> {
    if matches!(entry.spec.trigger, Trigger::Once { .. }) {
        entry.spec.enabled = false;
        entry.updated_at = format_instant(now);
        entry.state.next_run_at = None;
    } else {
        entry.state.next_run_at = compute_next_run_iso(entry, now)?;
    }
    Ok(())
}

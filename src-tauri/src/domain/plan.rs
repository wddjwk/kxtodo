//! Trigger planning: next-run computation shared by migration and the scheduler engine.

use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use cron::Schedule;

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::model::{ScheduleEntry, Trigger};
use crate::domain::time::{format_instant, parse_duration_ms, parse_stored_instant};

/// Normalize a cron expression: 5-field standard cron gets `0 ` seconds prepended.
pub fn normalize_cron(expr: &str) -> CoreResult<String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {}", fields.join(" "))),
        6 | 7 => Ok(fields.join(" ")),
        _ => Err(CoreError::validation(
            "INVALID_CRON",
            format!("cron 表达式 `{expr}` 应为 5 段（分 时 日 月 周）"),
        )),
    }
}

pub fn validate_cron(expr: &str) -> CoreResult<()> {
    let normalized = normalize_cron(expr)?;
    Schedule::from_str(&normalized).map_err(|error| {
        CoreError::validation(
            "INVALID_CRON",
            format!("cron 表达式 `{expr}` 无效：{error}"),
        )
    })?;
    Ok(())
}

pub fn validate_timezone(tz: &str) -> CoreResult<chrono_tz::Tz> {
    tz.parse::<chrono_tz::Tz>().map_err(|_| {
        CoreError::validation(
            "INVALID_TIMEZONE",
            format!("时区 `{tz}` 不是有效的 IANA 名称（如 Asia/Shanghai）"),
        )
    })
}

/// Next run instant after `now`, or None when the schedule should not run again.
pub fn compute_next_run(
    entry: &ScheduleEntry,
    now: DateTime<Utc>,
) -> CoreResult<Option<DateTime<Utc>>> {
    if !entry.spec.enabled {
        return Ok(None);
    }
    match &entry.spec.trigger {
        Trigger::Once { at, .. } => {
            if entry.state.run_count > 0 {
                return Ok(None);
            }
            let at = parse_stored_instant(at)?;
            Ok(Some(at))
        }
        Trigger::Interval {
            every, max_runs, ..
        } => {
            if let Some(max) = max_runs {
                if entry.state.run_count >= *max {
                    return Ok(None);
                }
            }
            let every_ms = parse_duration_ms(every)?;
            let base = entry
                .state
                .last_run_at
                .as_deref()
                .or(if entry.updated_at.is_empty() {
                    None
                } else {
                    Some(entry.updated_at.as_str())
                })
                .or(if entry.created_at.is_empty() {
                    None
                } else {
                    Some(entry.created_at.as_str())
                });
            let base = match base {
                Some(value) => parse_stored_instant(value)?,
                None => now,
            };
            let step = Duration::milliseconds(every_ms as i64);
            let mut next = base + step;
            if next <= now {
                let elapsed = now - base;
                let steps = elapsed.num_milliseconds() / step.num_milliseconds() + 1;
                next = base + Duration::milliseconds(steps * step.num_milliseconds());
            }
            Ok(Some(next))
        }
        Trigger::Calendar { cron, timezone, .. } => {
            let normalized = normalize_cron(cron)?;
            let schedule = Schedule::from_str(&normalized).map_err(|error| {
                CoreError::validation(
                    "INVALID_CRON",
                    format!("cron 表达式 `{cron}` 无效：{error}"),
                )
            })?;
            let tz = validate_timezone(timezone)?;
            Ok(schedule
                .upcoming(tz)
                .next()
                .map(|at| at.with_timezone(&Utc)))
        }
        Trigger::Condition { every, .. } => {
            let every_ms = parse_duration_ms(every)?;
            let base = entry
                .state
                .last_probe
                .as_ref()
                .map(|probe| probe.at.as_str())
                .and_then(|value| parse_stored_instant(value).ok())
                .unwrap_or(now);
            Ok(Some(base + Duration::milliseconds(every_ms as i64)))
        }
    }
}

pub fn compute_next_run_iso(
    entry: &ScheduleEntry,
    now: DateTime<Utc>,
) -> CoreResult<Option<String>> {
    Ok(compute_next_run(entry, now)?.map(format_instant))
}

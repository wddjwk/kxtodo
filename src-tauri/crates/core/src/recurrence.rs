//! 日程重复规则的展开（v0.9.1）。
//!
//! **前端有一份等价的实现**（`src/lib/calendar.ts` 的 `expandRecurring`）：视图渲染
//! 用 TS 那份，提醒与 CLI 用这份。两边的口径必须一致，`tests/recurrence.rs` 与
//! `src/lib/__tests__/calendar.spec.ts` 钉的是**同一张用例表**。
//!
//! 语义（与 `model::CalendarRepeat` 的文档同源）：
//! - 周次以开始日期所在周的**周一**为锚（ISO 周），与用户的「一周从周几开始」显示偏好无关；
//! - 目标月没有那一天（2 月 30 日）/ 非闰年的 2 月 29 日 → **跳过该次**，不折算；
//! - `count` 数的是规则生成的次数（含首次），`except_dates` 里的日期同样占名额；
//! - 生成日期早于开始日期的（周频率选了开始日之前的星期）不产出、也不占名额。

use chrono::{Datelike, NaiveDate};

use crate::model::{CalendarEvent, CalendarRepeat, RepeatEnd, RepeatFreq, REPEAT_COUNT_MAX};

/// 展开的保险丝：步数与产出条数都封顶，坏规则（interval = 0 之类）不许把进程拖死。
const MAX_STEPS: u32 = 4_000;
const MAX_RESULTS: usize = 1_000;

pub fn parse_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

/// 展开 `[from, to]`（闭区间）内的**开始日期**，升序、已去掉例外日期。
pub fn occurrences(event: &CalendarEvent, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let Some(start) = parse_date(&event.start_date) else {
        return Vec::new();
    };
    let Some(repeat) = event.repeat.as_ref() else {
        return if from <= start && start <= to {
            vec![start]
        } else {
            Vec::new()
        };
    };
    occurrences_with(repeat, start, &event.except_dates, from, to)
}

/// 从 `from` 起（含）的第一次出现；没有则 None。重复事件的上界给到 20 年，
/// 保证「永不结束」的规则也能找到一个未来实例（提醒计划用）。
pub fn next_occurrence(event: &CalendarEvent, from: NaiveDate) -> Option<NaiveDate> {
    let to = from.checked_add_days(chrono::Days::new(366 * 20))?;
    occurrences(event, from, to).first().copied()
}

fn occurrences_with(
    repeat: &CalendarRepeat,
    start: NaiveDate,
    except: &[String],
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<NaiveDate> {
    if to < from || to < start {
        return Vec::new();
    }
    let interval = repeat.interval.max(1) as i64;
    let until = match repeat.end {
        RepeatEnd::Until => match parse_date(&repeat.until) {
            Some(date) => {
                if date < start {
                    return Vec::new();
                }
                Some(date.min(to))
            }
            // 写路径已经归一过；万一读到坏值就按「永不结束」处理，别整条丢掉
            None => Some(to),
        },
        _ => Some(to),
    };
    let Some(limit) = until else { return Vec::new() };
    let count = match repeat.end {
        RepeatEnd::Count => Some(repeat.count.clamp(1, REPEAT_COUNT_MAX)),
        _ => None,
    };
    // count 结束的次数必须从头数（名额是全局的）；其余情况可以直接跳到区间附近
    let first_step = match count {
        Some(_) => 0,
        None => skip_ahead(repeat, start, from, interval),
    };

    let weekdays = normalized_weekdays(repeat, start);
    let month_days = normalized_month_days(repeat, start);
    let mut out = Vec::new();
    let mut index: u32 = 0;
    let mut step = first_step;
    // 保险丝数的是**迭代次数**而不是步号：skip_ahead 之后 step 可能已经远大于 MAX_STEPS
    for _ in 0..MAX_STEPS {
        let dates = step_dates(repeat, start, step, interval, &weekdays, &month_days);
        step += 1;
        if dates.is_empty() {
            // 该步全部落在不存在的日号上（2 月 30 日 / 平年的 2 月 29 日）：看下一步
            continue;
        }
        if dates[0] > limit {
            break;
        }
        for date in dates {
            if date < start {
                continue;
            }
            if count.is_some_and(|max| index >= max) {
                return out;
            }
            if date > limit {
                return out;
            }
            index += 1;
            if date < from {
                continue;
            }
            if except.iter().any(|raw| parse_date(raw) == Some(date)) {
                continue;
            }
            out.push(date);
            if out.len() >= MAX_RESULTS {
                return out;
            }
        }
    }
    out
}

/// 跳到「可能落进 `from` 之后」的第一步（保守回退 2 步，避免边界漏掉）。
fn skip_ahead(repeat: &CalendarRepeat, start: NaiveDate, from: NaiveDate, interval: i64) -> u32 {
    if from <= start {
        return 0;
    }
    let days = (from - start).num_days().max(0);
    let per_step = match repeat.freq {
        RepeatFreq::Day => interval,
        RepeatFreq::Week => 7 * interval,
        RepeatFreq::Month => 28 * interval,
        RepeatFreq::Year => 365 * interval,
    };
    let step = (days / per_step.max(1)).saturating_sub(2).max(0);
    u32::try_from(step).unwrap_or(0)
}

/// 第 `step` 步生成的日期（升序）。月/年频率遇到不存在的日号会少产出甚至不产出。
fn step_dates(
    repeat: &CalendarRepeat,
    start: NaiveDate,
    step: u32,
    interval: i64,
    weekdays: &[u32],
    month_days: &[u32],
) -> Vec<NaiveDate> {
    let k = i64::from(step) * interval;
    match repeat.freq {
        RepeatFreq::Day => start
            .checked_add_days(chrono::Days::new(u64::try_from(k).unwrap_or(u64::MAX)))
            .map(|date| vec![date])
            .unwrap_or_default(),
        RepeatFreq::Week => {
            // 周一为锚：start 所在周的周一 + k 周
            let anchor = start
                .checked_sub_days(chrono::Days::new(u64::from(start.weekday().num_days_from_monday())))
                .unwrap_or(start);
            let week = match anchor.checked_add_days(chrono::Days::new(
                u64::try_from(k * 7).unwrap_or(u64::MAX),
            )) {
                Some(date) => date,
                None => return Vec::new(),
            };
            weekdays
                .iter()
                .filter_map(|offset| {
                    week.checked_add_days(chrono::Days::new(u64::from(*offset)))
                })
                .collect()
        }
        RepeatFreq::Month => month_days
            .iter()
            .filter_map(|day| add_months(start, k as i32, *day))
            .collect(),
        RepeatFreq::Year => month_days
            .iter()
            .filter_map(|day| add_months(start, (k * 12) as i32, *day))
            .collect(),
    }
}

/// 按月份偏移并强制日号；目标月没有这一天返回 None（跳过该次）。
fn add_months(start: NaiveDate, months: i32, day: u32) -> Option<NaiveDate> {
    let total = i64::from(start.year()) * 12 + i64::from(start.month0()) + i64::from(months);
    let year = i32::try_from(total.div_euclid(12)).ok()?;
    let month = u32::try_from(total.rem_euclid(12) + 1).ok()?;
    let days_in_month = days_in_month(year, month)?;
    if day > days_in_month {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = first
        .checked_add_months(chrono::Months::new(1))
        .or_else(|| NaiveDate::from_ymd_opt(year + 1, 1, 1))?;
    Some(u32::try_from((next - first).num_days()).ok()?)
}

/// week 频率的星期集合 → 「相对周一的偏移」升序；空 = 只按开始日期的星期。
fn normalized_weekdays(repeat: &CalendarRepeat, start: NaiveDate) -> Vec<u32> {
    if repeat.freq != RepeatFreq::Week {
        return vec![u32::from(start.weekday().num_days_from_monday())];
    }
    let mut set: Vec<u32> = repeat
        .by_weekday
        .iter()
        .filter(|value| **value <= 6)
        .map(|value| {
            // 0 = 周日 … 6 = 周六 → 相对周一的偏移（周日 = 6）
            (u32::from(*value) + 6) % 7
        })
        .collect();
    if set.is_empty() {
        set.push(u32::from(start.weekday().num_days_from_monday()));
    }
    set.sort_unstable();
    set.dedup();
    set
}

/// month / year 频率的日号集合；空 = 按开始日期的日号。
fn normalized_month_days(repeat: &CalendarRepeat, start: NaiveDate) -> Vec<u32> {
    if !matches!(repeat.freq, RepeatFreq::Month | RepeatFreq::Year) {
        return Vec::new();
    }
    let mut set: Vec<u32> = repeat
        .by_month_day
        .iter()
        .filter(|value| (1..=31).contains(*value))
        .map(|value| u32::from(*value))
        .collect();
    if set.is_empty() {
        set.push(start.day());
    }
    set.sort_unstable();
    set.dedup();
    set
}

/// 写入前归一（`ops_calendar` 用）：把界面/CLI 传来的规则收敛成合法且唯一的形态，
/// 展开逻辑因此可以少写一堆防御分支。返回 `None` = 「不重复」（freq 认不出来时）。
pub fn normalize_repeat(
    raw: Option<CalendarRepeat>,
    start_date: &str,
) -> Result<Option<CalendarRepeat>, crate::error::CoreError> {
    let Some(mut repeat) = raw else { return Ok(None) };
    if repeat.interval == 0 {
        repeat.interval = 1;
    }
    if repeat.interval > 999 {
        repeat.interval = 999;
    }
    match repeat.freq {
        RepeatFreq::Week => {
            let mut set: Vec<u8> = repeat
                .by_weekday
                .iter()
                .filter(|value| **value <= 6)
                .copied()
                .collect();
            set.sort_unstable();
            set.dedup();
            repeat.by_weekday = set;
            repeat.by_month_day.clear();
        }
        RepeatFreq::Month => {
            let mut set: Vec<u8> = repeat
                .by_month_day
                .iter()
                .filter(|value| (1..=31).contains(*value))
                .copied()
                .collect();
            set.sort_unstable();
            set.dedup();
            repeat.by_month_day = set;
            repeat.by_weekday.clear();
        }
        RepeatFreq::Day | RepeatFreq::Year => {
            repeat.by_weekday.clear();
            if repeat.freq == RepeatFreq::Day {
                repeat.by_month_day.clear();
            } else {
                // 年频率按「开始日期的月日」重复：日号集合留着给 2 月 29 日这类情况用
                let mut set: Vec<u8> = repeat
                    .by_month_day
                    .iter()
                    .filter(|value| (1..=31).contains(*value))
                    .copied()
                    .collect();
                set.sort_unstable();
                set.dedup();
                repeat.by_month_day = set;
                repeat.by_weekday.clear();
            }
        }
    }
    match repeat.end {
        RepeatEnd::Count => {
            if repeat.count == 0 || repeat.count > REPEAT_COUNT_MAX {
                return Err(crate::error::CoreError::validation(
                    "CALENDAR_REPEAT_INVALID",
                    format!("重复次数应在 1..={REPEAT_COUNT_MAX} 之间"),
                ));
            }
            repeat.until.clear();
        }
        RepeatEnd::Until => {
            let until = parse_date(&repeat.until).ok_or_else(|| {
                crate::error::CoreError::validation(
                    "CALENDAR_REPEAT_INVALID",
                    "重复截止日期应为 YYYY-MM-DD",
                )
            })?;
            if let Some(start) = parse_date(start_date) {
                if until < start {
                    return Err(crate::error::CoreError::validation(
                        "CALENDAR_REPEAT_INVALID",
                        "重复截止日期不能早于开始日期",
                    ));
                }
            }
            repeat.until = until.format("%Y-%m-%d").to_string();
            repeat.count = 0;
        }
        RepeatEnd::Never => {
            repeat.count = 0;
            repeat.until.clear();
        }
    }
    repeat.extra.clear();
    Ok(Some(repeat))
}

/// 重复规则的中文展示串（CLI、通知文案与界面共用同一口径）。
pub fn describe(repeat: &CalendarRepeat) -> String {
    let base = |unit: &str| {
        if repeat.interval <= 1 {
            format!("每{unit}")
        } else {
            format!("每 {} {}", repeat.interval, unit)
        }
    };
    let text = match repeat.freq {
        RepeatFreq::Day => base("天"),
        RepeatFreq::Week => {
            let names = ["日", "一", "二", "三", "四", "五", "六"];
            let days: Vec<&str> = repeat
                .by_weekday
                .iter()
                .filter_map(|value| names.get(usize::from(*value)).copied())
                .collect();
            if days.is_empty() {
                base("周")
            } else {
                format!("{}的周{}", base("周"), days.join("、"))
            }
        }
        RepeatFreq::Month => {
            let days: Vec<String> = repeat
                .by_month_day
                .iter()
                .map(|day| format!("{day} 日"))
                .collect();
            if days.is_empty() {
                base("个月")
            } else {
                format!("{}的 {}", base("个月"), days.join("、"))
            }
        }
        RepeatFreq::Year => base("年"),
    };
    match repeat.end {
        RepeatEnd::Never => text,
        RepeatEnd::Count => format!("{text}（共 {} 次）", repeat.count),
        RepeatEnd::Until => format!("{text}（至 {}）", repeat.until),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepeatEnd;
    use serde_json::Map;

    fn date(raw: &str) -> NaiveDate {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d").unwrap()
    }

    fn event(start: &str, repeat: CalendarRepeat) -> CalendarEvent {
        CalendarEvent {
            id: "cal-1".to_string(),
            title: "站会".to_string(),
            details: String::new(),
            start_date: start.to_string(),
            start_time: "09:00".to_string(),
            end_date: start.to_string(),
            end_time: "09:30".to_string(),
            all_day: false,
            reminders: Vec::new(),
            location: String::new(),
            category_id: String::new(),
            repeat: Some(repeat),
            except_dates: Vec::new(),
            task_id: String::new(),
            created_at: "2026-10-01T00:00:00.000Z".to_string(),
            updated_at: None,
            extra: Map::new(),
        }
    }

    fn repeat(freq: RepeatFreq, interval: u32) -> CalendarRepeat {
        CalendarRepeat {
            freq,
            interval,
            by_weekday: Vec::new(),
            by_month_day: Vec::new(),
            end: RepeatEnd::Never,
            count: 0,
            until: String::new(),
            extra: Map::new(),
        }
    }

    fn dates(list: &[NaiveDate]) -> Vec<String> {
        list.iter().map(|d| d.format("%Y-%m-%d").to_string()).collect()
    }

    #[test]
    fn daily_within_range() {
        let ev = event("2026-10-01", repeat(RepeatFreq::Day, 1));
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-05"), date("2026-10-08"))),
            vec!["2026-10-05", "2026-10-06", "2026-10-07", "2026-10-08"]
        );
    }

    #[test]
    fn every_third_day_skips_ahead_correctly() {
        let ev = event("2026-10-01", repeat(RepeatFreq::Day, 3));
        // 10-01, 10-04, 10-07, 10-10 …（跳过 ahead 之后仍要对齐系列锚点）
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-05"), date("2026-10-12"))),
            vec!["2026-10-07", "2026-10-10"]
        );
    }

    #[test]
    fn weekly_workdays_anchored_to_monday() {
        // 2026-10-07 是周三
        let mut rule = repeat(RepeatFreq::Week, 1);
        rule.by_weekday = vec![1, 2, 3, 4, 5];
        let ev = event("2026-10-07", rule);
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-05"), date("2026-10-18"))),
            vec![
                "2026-10-07", "2026-10-08", "2026-10-09",
                "2026-10-12", "2026-10-13", "2026-10-14", "2026-10-15", "2026-10-16"
            ]
        );
    }

    #[test]
    fn weekly_before_start_weekday_is_not_emitted_and_does_not_consume_count() {
        // 开始于周三，只重复周一 → 第一次出现是下周一（本周周一早于开始日期，不算）
        let mut rule = repeat(RepeatFreq::Week, 1);
        rule.by_weekday = vec![1];
        let ev = event("2026-10-07", rule);
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-01"), date("2026-10-31"))),
            vec!["2026-10-12", "2026-10-19", "2026-10-26"]
        );
    }

    #[test]
    fn biweekly_keeps_the_anchor_week_parity() {
        let mut rule = repeat(RepeatFreq::Week, 2);
        rule.by_weekday = vec![1, 4];
        let ev = event("2026-10-07", rule);
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-05"), date("2026-11-08"))),
            vec![
                "2026-10-08", "2026-10-19", "2026-10-22",
                "2026-11-02", "2026-11-05"
            ]
        );
    }

    #[test]
    fn monthly_on_the_31st_skips_short_months() {
        let mut rule = repeat(RepeatFreq::Month, 1);
        rule.by_month_day = vec![31];
        let ev = event("2026-01-31", rule);
        assert_eq!(
            dates(&occurrences(&ev, date("2026-01-01"), date("2026-08-31"))),
            vec!["2026-01-31", "2026-03-31", "2026-05-31", "2026-07-31", "2026-08-31"]
        );
    }

    #[test]
    fn monthly_defaults_to_the_start_day_number() {
        let ev = event("2026-10-07", repeat(RepeatFreq::Month, 1));
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-01"), date("2027-01-31"))),
            vec!["2026-10-07", "2026-11-07", "2026-12-07", "2027-01-07"]
        );
    }

    #[test]
    fn yearly_skips_non_leap_feb_29() {
        let ev = event("2024-02-29", repeat(RepeatFreq::Year, 1));
        assert_eq!(
            dates(&occurrences(&ev, date("2024-01-01"), date("2032-12-31"))),
            vec!["2024-02-29", "2028-02-29", "2032-02-29"]
        );
    }

    #[test]
    fn count_limits_total_occurrences_including_those_before_the_range() {
        let mut rule = repeat(RepeatFreq::Day, 1);
        rule.end = RepeatEnd::Count;
        rule.count = 3;
        let ev = event("2026-10-01", rule);
        // 只有 10-01/02/03 三次；查询区间在后面 → 空
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-02"), date("2026-10-31"))),
            vec!["2026-10-02", "2026-10-03"]
        );
        assert!(occurrences(&ev, date("2026-11-01"), date("2026-11-30")).is_empty());
    }

    #[test]
    fn except_dates_are_skipped_but_consume_count() {
        let mut rule = repeat(RepeatFreq::Day, 1);
        rule.end = RepeatEnd::Count;
        rule.count = 3;
        let mut ev = event("2026-10-01", rule);
        ev.except_dates = vec!["2026-10-02".to_string()];
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-01"), date("2026-10-31"))),
            vec!["2026-10-01", "2026-10-03"]
        );
    }

    #[test]
    fn until_is_inclusive() {
        let mut rule = repeat(RepeatFreq::Day, 1);
        rule.end = RepeatEnd::Until;
        rule.until = "2026-10-05".to_string();
        let ev = event("2026-10-01", rule);
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-01"), date("2026-10-31"))),
            vec![
                "2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05"
            ]
        );
    }

    #[test]
    fn non_recurring_event_is_a_single_occurrence() {
        let mut ev = event("2026-10-07", repeat(RepeatFreq::Day, 1));
        ev.repeat = None;
        assert_eq!(
            dates(&occurrences(&ev, date("2026-10-01"), date("2026-10-31"))),
            vec!["2026-10-07"]
        );
        assert!(occurrences(&ev, date("2026-11-01"), date("2026-11-30")).is_empty());
    }

    #[test]
    fn next_occurrence_finds_a_future_instance() {
        let ev = event("2026-10-01", repeat(RepeatFreq::Month, 1));
        assert_eq!(
            next_occurrence(&ev, date("2026-10-15")),
            Some(date("2026-11-01"))
        );
    }

    #[test]
    fn normalize_repeat_clears_cross_frequency_fields() {
        let mut rule = repeat(RepeatFreq::Week, 0);
        rule.by_weekday = vec![9, 3, 3, 1];
        rule.by_month_day = vec![15];
        rule.end = RepeatEnd::Count;
        rule.count = 5;
        rule.until = "2026-10-10".to_string();
        let normalized = normalize_repeat(Some(rule), "2026-10-07").unwrap().unwrap();
        assert_eq!(normalized.interval, 1);
        assert_eq!(normalized.by_weekday, vec![1, 3]);
        assert!(normalized.by_month_day.is_empty());
        assert_eq!(normalized.end, RepeatEnd::Count);
        assert!(normalized.until.is_empty());
    }

    #[test]
    fn normalize_repeat_rejects_bad_until_and_count() {
        let mut rule = repeat(RepeatFreq::Day, 1);
        rule.end = RepeatEnd::Until;
        rule.until = "2026-09-01".to_string();
        assert_eq!(
            normalize_repeat(Some(rule.clone()), "2026-10-07")
                .unwrap_err()
                .code,
            "CALENDAR_REPEAT_INVALID"
        );
        rule.end = RepeatEnd::Count;
        rule.count = 0;
        assert_eq!(
            normalize_repeat(Some(rule), "2026-10-07").unwrap_err().code,
            "CALENDAR_REPEAT_INVALID"
        );
    }

    #[test]
    fn describe_reads_naturally() {
        let mut rule = repeat(RepeatFreq::Week, 2);
        rule.by_weekday = vec![1, 3, 5];
        assert_eq!(describe(&rule), "每 2 周的周一、三、五");
        rule.end = RepeatEnd::Count;
        rule.count = 10;
        assert_eq!(describe(&rule), "每 2 周的周一、三、五（共 10 次）");
        let plain = repeat(RepeatFreq::Day, 1);
        assert_eq!(describe(&plain), "每天");
        let monthly = repeat(RepeatFreq::Month, 2);
        assert_eq!(describe(&monthly), "每 2 个月");
        let every_day3 = repeat(RepeatFreq::Day, 3);
        assert_eq!(describe(&every_day3), "每 3 天");
    }
}

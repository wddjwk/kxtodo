//! Single source of truth for durations, dates and instants (requirements §3.2, §3.5.2).

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use crate::domain::error::{CoreError, CoreResult};

/// Parse `<正整数><ms|s|m|h|d>` into milliseconds. The only duration parser in the codebase.
pub fn parse_duration_ms(raw: &str) -> CoreResult<u64> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(bad_duration(raw));
    }
    let split_at = value
        .find(|ch: char| ch.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let (digits, unit) = value.split_at(split_at);
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(bad_duration(raw));
    }
    let amount: u64 = digits.parse().map_err(|_| bad_duration(raw))?;
    if amount == 0 {
        return Err(bad_duration(raw));
    }
    let factor: u64 = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(bad_duration(raw)),
    };
    amount
        .checked_mul(factor)
        .ok_or_else(|| bad_duration(raw))
}

fn bad_duration(raw: &str) -> CoreError {
    CoreError::validation(
        "INVALID_DURATION",
        format!("无效时长 `{raw}`，应为 <正整数><ms|s|m|h|d>，例如 5s、10m、1h、2d"),
    )
}

/// Render milliseconds using the largest evenly divisible unit (d→h→m→s→ms).
pub fn format_duration(ms: u64) -> String {
    const UNITS: [(u64, &str); 5] = [
        (86_400_000, "d"),
        (3_600_000, "h"),
        (60_000, "m"),
        (1_000, "s"),
        (1, "ms"),
    ];
    for (factor, suffix) in UNITS {
        if ms % factor == 0 {
            return format!("{}{}", ms / factor, suffix);
        }
    }
    format!("{ms}ms")
}

/// Current time as ISO 8601 with offset, millisecond precision (UTC, `Z`).
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Validate / normalize a `YYYY-MM-DD` date. Accepts relative forms `+Nd`.
pub fn parse_date(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    if let Some(rel) = value.strip_prefix('+') {
        if let Some(days) = rel.strip_suffix('d') {
            let days: i64 = days
                .parse()
                .map_err(|_| bad_date(raw))?;
            let date = Local::now().date_naive() + Duration::days(days);
            return Ok(date.format("%Y-%m-%d").to_string());
        }
        return Err(bad_date(raw));
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| bad_date(raw))?;
    Ok(date.format("%Y-%m-%d").to_string())
}

fn bad_date(raw: &str) -> CoreError {
    CoreError::validation(
        "INVALID_DATE",
        format!("无效日期 `{raw}`，应为 YYYY-MM-DD 或相对写法 +Nd"),
    )
}

/// Parse an instant: ISO 8601 with timezone, or relative `+Ns|Nm|Nh|Nd` (from now).
/// Returns normalized ISO 8601 UTC string.
pub fn parse_instant(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    if let Some(rel) = value.strip_prefix('+') {
        let ms = parse_duration_ms(rel)?;
        let at = Utc::now() + Duration::milliseconds(ms as i64);
        return Ok(at.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| bad_instant(raw))?;
    Ok(parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn bad_instant(raw: &str) -> CoreError {
    CoreError::validation(
        "INVALID_TIME",
        format!("无效时间 `{raw}`，应为带时区的 ISO 8601（如 2026-07-31T17:30:00+08:00）或相对写法 +2d/+1h"),
    )
}

/// Parse any stored instant (accepts `Z` or numeric offset). Returns UTC DateTime.
pub fn parse_stored_instant(raw: &str) -> CoreResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            CoreError::validation(
                "INVALID_TIME",
                format!("无法解析时间 `{raw}`，应为带时区的 ISO 8601"),
            )
        })
}

/// Format a UTC DateTime as canonical stored ISO string.
pub fn format_instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Milliseconds since Unix epoch for a stored instant.
pub fn instant_millis(raw: &str) -> CoreResult<i64> {
    Ok(parse_stored_instant(raw)?.timestamp_millis())
}

/// Resolve the host IANA timezone name (best effort).
pub fn local_timezone() -> Option<Tz> {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
}

/// Migration helper: interpret a legacy `runAt` string.
/// - Has offset/Z: normalize as instant.
/// - Missing offset: treat as local wall clock in `tz`, applying DST rules:
///   ambiguous → earlier instant (warning), gap → first valid instant after (warning).
pub fn migrate_legacy_local_time(raw: &str, tz: Tz) -> (Option<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let value = raw.trim();
    if value.is_empty() {
        return (None, warnings);
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return (
            Some(
                parsed
                    .with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
            ),
            warnings,
        );
    }
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ];
    let naive = formats
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(value, fmt).ok());
    let Some(naive) = naive else {
        warnings.push(format!("无法解析的旧时间 `{value}`"));
        return (None, warnings);
    };
    use chrono::offset::LocalResult;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(at) => (
            Some(
                at.with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
            ),
            warnings,
        ),
        LocalResult::Ambiguous(early, _) => {
            warnings.push(format!(
                "旧时间 `{value}` 在 {tz} 处于 DST 重叠，已选择较早时刻"
            ));
            (
                Some(
                    early
                        .with_timezone(&Utc)
                        .to_rfc3339_opts(SecondsFormat::Millis, true),
                ),
                warnings,
            )
        }
        LocalResult::None => {
            // DST gap: advance minute by minute until valid (up to 3h).
            let mut candidate = naive;
            let mut resolved = None;
            for _ in 0..180 {
                candidate += Duration::minutes(1);
                if let LocalResult::Single(at) = tz.from_local_datetime(&candidate) {
                    resolved = Some(at);
                    break;
                }
            }
            match resolved {
                Some(at) => {
                    warnings.push(format!(
                        "旧时间 `{value}` 在 {tz} 处于 DST 空洞，已顺延到第一个有效时刻"
                    ));
                    (
                        Some(
                            at.with_timezone(&Utc)
                                .to_rfc3339_opts(SecondsFormat::Millis, true),
                        ),
                        warnings,
                    )
                }
                None => {
                    warnings.push(format!("旧时间 `{value}` 无法映射到 {tz} 的有效时刻"));
                    (None, warnings)
                }
            }
        }
    }
}

/// Compute local-zone now in a named timezone (for cron evaluation).
pub fn now_in_tz(tz: Tz) -> DateTime<Tz> {
    Utc::now().with_timezone(&tz)
}

pub fn fixed_offset_now(offset_seconds: i32) -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(offset_seconds).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    Utc::now().with_timezone(&offset)
}

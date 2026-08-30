//! Bounded NDJSON histories: schedule runs and audit trail (§4.2.3).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::error::CoreResult;

/// Append one JSON line; enforce total byte cap (oldest-first trimming) and an
/// optional per-`taskId` entry cap (used by schedule history, §3.5.5).
pub fn append_bounded_jsonl(
    path: &Path,
    entry: &Value,
    max_bytes: u64,
    per_task_cap: Option<usize>,
) -> CoreResult<()> {
    static HISTORY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = HISTORY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|error| crate::error::CoreError::internal(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(entry)?;
    line.push(b'\n');
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(&line)?;
    }
    trim_history(path, max_bytes, per_task_cap)
}

pub fn trim_history(path: &Path, max_bytes: u64, per_task_cap: Option<usize>) -> CoreResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path)?;
    let mut lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();

    if let Some(cap) = per_task_cap {
        // Keep only the newest `cap` entries per taskId.
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut keep: Vec<bool> = vec![true; lines.len()];
        for (index, line) in lines.iter().enumerate().rev() {
            let task_id = serde_json::from_str::<Value>(line).ok().and_then(|value| {
                value
                    .get("taskId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            if let Some(task_id) = task_id {
                let count = counts.entry(task_id).or_insert(0);
                *count += 1;
                if *count > cap {
                    keep[index] = false;
                }
            }
        }
        if keep.iter().any(|flag| !flag) {
            lines = lines
                .into_iter()
                .zip(keep.iter())
                .filter_map(|(line, flag)| if *flag { Some(line) } else { None })
                .collect();
        }
    }

    // Byte cap: drop oldest lines until under the limit.
    let mut total: u64 = lines.iter().map(|line| line.len() as u64 + 1).sum();
    let mut start = 0;
    while total > max_bytes && start < lines.len() {
        total -= lines[start].len() as u64 + 1;
        start += 1;
    }
    let lines = &lines[start..];

    let mut output = String::with_capacity(total as usize + 16);
    for line in lines {
        output.push_str(line);
        output.push('\n');
    }
    crate::repo::atomic_write(path, &output)
}

pub fn read_history(path: &Path) -> CoreResult<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    Ok(raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// Schedule run history entry (§3.5.5).
pub fn schedule_run_record(
    task_id: &str,
    scheduled_at: Option<&str>,
    started_at: &str,
    finished_at: &str,
    kind: &str,
    status: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    stdout_was_truncated: bool,
    stderr_was_truncated: bool,
    stop_reason: Option<&str>,
    missed_count: u64,
) -> Value {
    let (stdout_out, stdout_truncated) =
        truncate(stdout, crate::repo::SCHEDULE_OUTPUT_MAX_BYTES);
    let (stderr_out, stderr_truncated) =
        truncate(stderr, crate::repo::SCHEDULE_OUTPUT_MAX_BYTES);
    let stdout_truncated = stdout_was_truncated || stdout_truncated;
    let stderr_truncated = stderr_was_truncated || stderr_truncated;
    serde_json::json!({
        "taskId": task_id,
        "kind": kind,
        "scheduledAt": scheduled_at,
        "startedAt": started_at,
        "finishedAt": finished_at,
        "status": status,
        "exitCode": exit_code,
        "stdout": stdout_out,
        "stderr": stderr_out,
        "stdoutTruncated": stdout_truncated,
        "stderrTruncated": stderr_truncated,
        "stopReason": stop_reason,
        "missedCount": missed_count,
    })
}

pub fn truncate(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

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
    // 追加后仍在字节上限内、且没有 per-task 条数上限要执行时，不必把整个文件读回来重写。
    // 审计是每次业务写都会走的高频路径（上限 10MB），触顶之前它本该只是一次 O(1) 追加；
    // 早先无条件调 trim_history，等于每条审计都要读全文 + 重建 + 原子重写 + fsync，
    // 用起来越久越慢（触顶后就是每次写读写 10MB）。调度历史带 per-task 上限、
    // 且只在定时任务真正跑起来时才写，保持原来的完整 trim。
    let needs_trim = match per_task_cap {
        Some(_) => true,
        None => {
            let existing = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            existing + line.len() as u64 > max_bytes
        }
    };
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(&line)?;
    }
    if needs_trim {
        trim_history(path, max_bytes, per_task_cap)?;
    }
    Ok(())
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

    // Byte cap: drop oldest lines until well under the limit.
    //
    // 目标刻意不是「刚好等于上限」而是上限的 80%：修剪是一次全文重写，若只裁到刚好达标，
    // 文件满之后几乎每追加一行就要再重写一次（10MB 上限 = 每次点击读写 10MB）。留出 20%
    // 余量，一次重写能摊到之后上千次追加。上限本身仍然是硬约束（文件永远不超过 max_bytes）。
    let target = max_bytes * 4 / 5;
    let mut total: u64 = lines.iter().map(|line| line.len() as u64 + 1).sum();
    let mut start = 0;
    while total > target && start < lines.len() {
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

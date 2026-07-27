//! doctor: diagnostics for data dir, integrity, host and runtimes (§3.7, §4.2.3).

use serde_json::{json, Value};

use crate::domain::core::{ExecContext, Invocation};
use crate::domain::envelope::Meta;
use crate::domain::error::CoreResult;
use crate::domain::model::{NodeKind, ScheduleSpec};
use crate::domain::repo::{
    cleanup_temp_files, read_json_value, AUDIT_MAX_BYTES, BACKUP_KEEP,
    SCHEDULE_HISTORY_MAX_BYTES, SCHEDULE_HISTORY_PER_TASK, SCHEDULE_OUTPUT_MAX_BYTES,
};

/// `config path` — full layout descriptor with live status (§3.6, §4.2.3).
pub fn config_paths(ctx: &ExecContext) -> CoreResult<Value> {
    let layout = &ctx.repo.layout;
    let entry = |path: std::path::PathBuf, kind: &str| {
        let exists = path.exists();
        let readonly = exists
            && path
                .metadata()
                .map(|meta| meta.permissions().readonly())
                .unwrap_or(false);
        json!({
            "path": path.display().to_string(),
            "kind": kind,
            "exists": exists,
            "readonly": readonly,
        })
    };
    let host = layout.host_descriptor();
    let host_state = match crate::domain::ipc::read_host_descriptor(&host) {
        Some(descriptor) => json!({
            "state": "descriptor-present",
            "pid": descriptor.pid,
            "endpoint": descriptor.endpoint,
        }),
        None => json!({ "state": "absent" }),
    };
    Ok(json!({
        "dataDir": layout.root.display().to_string(),
        "paths": {
            "data": entry(layout.data_file(), "domain-data"),
            "settings": entry(layout.settings_file(), "domain-settings"),
            "schedule": entry(layout.schedule_file(), "domain-schedule"),
            "img": entry(layout.img_dir(), "images"),
            "historySchedule": entry(layout.schedule_history(), "history"),
            "historyAudit": entry(layout.audit_history(), "history"),
            "backups": entry(layout.backup_dir(), "backups"),
            "runtimeHost": entry(host, "host-descriptor"),
            "lock": entry(layout.lock_file(), "lock"),
        },
        "limits": {
            "idempotencyPerDomain": crate::domain::repo::IDEMPOTENCY_MAX_RECORDS,
            "idempotencyMaxAgeDays": crate::domain::repo::IDEMPOTENCY_MAX_AGE_DAYS,
            "scheduleHistoryPerTask": SCHEDULE_HISTORY_PER_TASK,
            "scheduleHistoryMaxBytes": SCHEDULE_HISTORY_MAX_BYTES,
            "scheduleOutputMaxBytes": SCHEDULE_OUTPUT_MAX_BYTES,
            "auditMaxBytes": AUDIT_MAX_BYTES,
            "backupKeep": BACKUP_KEEP,
        },
        "host": host_state,
        "skills": crate::domain::skills::cmd_path().unwrap_or_default(),
    }))
}

/// `doctor` — read-only diagnostics.
pub fn run_doctor(_inv: &Invocation, ctx: &ExecContext, _meta: &mut Meta) -> CoreResult<Value> {
    let layout = &ctx.repo.layout;
    let mut checks: Vec<Value> = Vec::new();

    // 1. Data dir writability.
    let writable = layout.root.is_dir()
        && !layout
            .root
            .metadata()
            .map(|meta| meta.permissions().readonly())
            .unwrap_or(false);
    checks.push(check(
        "data-dir",
        writable,
        format!("数据目录 {}", layout.root.display()),
        if writable { None } else { Some("目录不存在或只读") },
    ));

    // 2. Domain files: presence, parse, schema versions, revisions.
    for (name, path, expected) in [
        ("data", layout.data_file(), crate::domain::model::DATA_SCHEMA_VERSION),
        ("settings", layout.settings_file(), crate::domain::model::SETTINGS_SCHEMA_VERSION),
        ("schedule", layout.schedule_file(), crate::domain::model::SCHEDULE_SCHEMA_VERSION),
    ] {
        if !path.exists() {
            checks.push(check(name, true, format!("{} 不存在（首次使用时创建）", path.display()), None));
            continue;
        }
        match read_json_value(&path) {
            Ok(value) => {
                let version = value
                    .get("schemaVersion")
                    .and_then(Value::as_u64)
                    .or_else(|| {
                        value
                            .get("_meta")
                            .and_then(|meta| meta.get("schemaVersion"))
                            .and_then(Value::as_u64)
                    });
                let revision = value
                    .get("_meta")
                    .and_then(|meta| meta.get("revision"))
                    .and_then(Value::as_u64);
                let ok = version == Some(expected as u64) && revision.is_some();
                checks.push(check(
                    name,
                    ok,
                    format!(
                        "{}：schemaVersion={:?} revision={:?}",
                        path.display(),
                        version,
                        revision
                    ),
                    if ok {
                        None
                    } else {
                        Some("版本或 revision 缺失，运行任意 kxtodo 命令触发迁移")
                    },
                ));
            }
            Err(error) => {
                checks.push(check(
                    name,
                    false,
                    format!("{} 无法解析", path.display()),
                    Some(match error.hint.as_deref() {
                        Some(hint) => hint,
                        None => "从 backups/ 恢复最近备份",
                    }),
                ));
            }
        }
    }

    // 3. Data integrity: orphan items + dangling parents.
    if let Ok(data) = ctx.repo.load_data() {
        let orphans = crate::domain::ops_task::orphan_item_ids(&data);
        checks.push(check(
            "orphan-items",
            orphans.is_empty(),
            format!("孤儿 item（entry 缺失）：{} 个", orphans.len()),
            if orphans.is_empty() {
                None
            } else {
                Some("用 kxtodo task modify --type item --id <id> --entry-id <entry> 修复")
            },
        ));
        if !orphans.is_empty() {
            checks.push(json!({
                "name": "orphan-items-detail",
                "ok": false,
                "detail": orphans,
            }));
        }
        let dangling: Vec<String> = data
            .nodes
            .iter()
            .filter(|node| {
                node.parent_id
                    .as_deref()
                    .map(|parent| {
                        crate::domain::ops_task::find_node(&data, parent)
                            .map(|p| p.kind != NodeKind::Category)
                            .unwrap_or(true)
                    })
                    .unwrap_or(false)
            })
            .map(|node| node.id.clone())
            .collect();
        checks.push(check(
            "dangling-parents",
            dangling.is_empty(),
            format!("父级悬空节点：{} 个", dangling.len()),
            if dangling.is_empty() { None } else { Some("检查 nodes 的 parentId") },
        ));
        // Orphan images (report only, §4.2).
        let img_data = layout.img_dir().join("data");
        if img_data.is_dir() {
            let mut orphan_dirs = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&img_data) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let referenced = crate::domain::ops_task::find_node(&data, &name)
                        .map(|node| node.kind == NodeKind::Entry)
                        .unwrap_or(false);
                    if !referenced {
                        orphan_dirs.push(name);
                    }
                }
            }
            checks.push(check(
                "orphan-images",
                true,
                format!("未被条目引用的图片目录：{} 个（仅报告，不自动删除）", orphan_dirs.len()),
                None,
            ));
        }
    }

    // 4. Schedule spec validity.
    if let Ok(file) = ctx.repo.load_schedule() {
        let mut invalid = Vec::new();
        for entry in &file.tasks {
            let raw = serde_json::to_value(&entry.spec).unwrap_or(Value::Null);
            if serde_json::from_value::<ScheduleSpec>(raw).is_err() {
                invalid.push(entry.id.clone());
            }
        }
        checks.push(check(
            "schedule-specs",
            invalid.is_empty(),
            format!("定时任务定义无效：{} 个", invalid.len()),
            if invalid.is_empty() { None } else { Some("kxtodo schedule validate --id <id> --patch ...") },
        ));
        let enabled = file.tasks.iter().filter(|entry| entry.spec.enabled).count();
        checks.push(check(
            "schedule-summary",
            true,
            format!("定时任务 {} 个，启用 {} 个", file.tasks.len(), enabled),
            None,
        ));
    }

    // 5. Lock health + stale temp files.
    match crate::domain::repo::RepoLock::try_acquire(layout) {
        Ok(Some(_lock)) => checks.push(check("lock", true, "仓库锁可用".to_string(), None)),
        Ok(None) => checks.push(check(
            "lock",
            false,
            "仓库锁被其他进程持有".to_string(),
            Some("若有 KXToDo 进程正常运行则属正常；否则检查是否有卡死进程"),
        )),
        Err(error) => checks.push(check("lock", false, format!("锁错误：{error}"), None)),
    }
    match cleanup_temp_files(&layout.root) {
        Ok(removed) => checks.push(check(
            "temp-files",
            true,
            format!("清理原子写残留临时文件 {} 个", removed.len()),
            None,
        )),
        Err(error) => checks.push(check("temp-files", false, format!("临时文件清理失败：{error}"), None)),
    }

    // 6. Backups.
    let backups = layout.backup_dir();
    let backup_count = if backups.is_dir() {
        std::fs::read_dir(&backups)
            .map(|entries| entries.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    } else {
        0
    };
    checks.push(check(
        "backups",
        true,
        format!("备份集 {} 组（保留最近 {} 组）", backup_count, BACKUP_KEEP),
        None,
    ));

    // 7. History bounds.
    for (name, path, max) in [
        ("history-schedule", layout.schedule_history(), SCHEDULE_HISTORY_MAX_BYTES),
        ("history-audit", layout.audit_history(), AUDIT_MAX_BYTES),
    ] {
        let size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
        checks.push(check(
            name,
            size <= max,
            format!("{}：{} / {} 字节", path.display(), size, max),
            if size <= max { None } else { Some("将在下次写入时截断") },
        ));
    }

    // 8. Host descriptor.
    let host_path = layout.host_descriptor();
    match crate::domain::ipc::read_host_descriptor(&host_path) {
        Some(descriptor) => {
            let alive = crate::domain::ipc::host_process_alive(descriptor.pid);
            checks.push(check(
                "host",
                true,
                format!(
                    "Host 描述符：pid={} endpoint={}（{}）",
                    descriptor.pid,
                    descriptor.endpoint,
                    if alive { "存活" } else { "已失效，将按需清理" }
                ),
                None,
            ));
        }
        None => checks.push(check("host", true, "Host 未运行".to_string(), None)),
    }

    // 9. Runtimes.
    if let Ok(file) = ctx.repo.load_schedule() {
        let view = crate::domain::ops_schedule::runtime_view(&file);
        let runtimes = view["runtimes"].as_array().cloned().unwrap_or_default();
        let missing: Vec<String> = runtimes
            .iter()
            .filter(|item| item.get("available").and_then(Value::as_bool) == Some(false))
            .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        checks.push(check(
            "runtimes",
            true,
            if missing.is_empty() {
                "全部运行时可用".to_string()
            } else {
                format!("缺失运行时：{}（仅影响对应脚本任务）", missing.join(", "))
            },
            if missing.is_empty() {
                None
            } else {
                Some("kxtodo schedule runtime detect 或 runtime set <name> <path>")
            },
        ));
    }

    // 10. Settings validation.
    if let Ok(settings) = ctx.repo.load_settings() {
        let issues = crate::domain::ops_config::validate_settings(layout, &settings);
        checks.push(check(
            "settings",
            issues.is_empty(),
            format!("配置问题：{} 个", issues.len()),
            None,
        ));
        for issue in issues {
            checks.push(json!({ "name": "settings-detail", "ok": false, "detail": issue }));
        }
    }

    let failed = checks
        .iter()
        .filter(|item| item.get("ok").and_then(Value::as_bool) == Some(false))
        .count();
    Ok(json!({
        "healthy": failed == 0,
        "failedChecks": failed,
        "checks": checks,
        "note": "doctor 只诊断不修复；任何清理均为独立 high-risk-write",
    }))
}

fn check(name: &str, ok: bool, message: String, hint: Option<&str>) -> Value {
    let mut out = json!({
        "name": name,
        "ok": ok,
        "message": message,
    });
    if let Some(hint) = hint {
        out["hint"] = json!(hint);
    }
    out
}

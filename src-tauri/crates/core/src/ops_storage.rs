//! storage domain: 数据目录的占用统计与清理（v0.7.3，设置页「释放空间」的核心）。
//!
//! **边界（clean 永远不碰的东西）**：五个领域 JSON（data/settings/tasks/diary/ledger）、
//! `runtime/`（同步状态与 IPC 描述符）、`history/`、`backups/`（只统计不删）、服务器的
//! `settings.json` / `data.db` / `data.db-wal`，以及任何账户/令牌数据。服务器库虽然是
//! 中转缓存（每台设备都持全量副本），但账户哈希与登录令牌只存在那里，删库等于把所有
//! 设备登出去——那不属于「释放空间」，本命令不做。
//!
//! 清理只删四类东西：无人引用的图片（插图/背景/头像）、崩溃残留的临时文件
//! （数据目录根的 `.*.tmp` 与 `img/` 下的 `.part` 半截产物）、过期的服务器日志
//! （保留最新两份且绝不删今天的）。**每一个单独的 IO 失败都吞成一条 warning**，
//! 命令绝不半途硬失败——清不动的那部分留给下一次。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::{require_confirmation, ExecContext, Invocation};
use crate::diary_archive::{is_image_name, is_safe_image_name};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::model::{DIARY_IMAGE_NODE, LEDGER_IMAGE_NODE};
use crate::repo::Domain;

pub fn storage_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "usage" => storage_usage(inv, ctx, meta),
        "clean" => storage_clean(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 storage 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// 扫描
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    count: u64,
    bytes: u64,
}

impl Tally {
    fn add(&mut self, bytes: u64) {
        self.count += 1;
        self.bytes += bytes;
    }
    fn view(&self) -> Value {
        json!({ "count": self.count, "bytes": self.bytes })
    }
}

/// 一次扫描的全部结论：usage 直接报数，clean 按文件清单动手。
#[derive(Default)]
struct Scan {
    total_bytes: u64,
    images: Tally,
    orphan_images: Vec<(PathBuf, u64)>,
    backgrounds: Tally,
    orphan_backgrounds: Vec<(PathBuf, u64)>,
    avatars: Tally,
    orphan_avatars: Vec<(PathBuf, u64)>,
    temp_files: Vec<(PathBuf, u64)>,
    /// img/ 下残留的 `.part` 半截产物（原子写被打断的遗留）
    part_files: Vec<(PathBuf, u64)>,
    server_logs: Tally,
    cleanable_logs: Vec<(PathBuf, u64)>,
    backups: Tally,
    /// 孤儿图删完后可以顺手清掉的空 `img/data/<id>` 目录（id 不是现存节点也不是 diary）
    prunable_dirs: Vec<PathBuf>,
}

/// 列出目录下的文件（`recursive` 时含子目录）；读不动的目录按空处理。
fn collect_files(dir: &Path, recursive: bool) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if recursive {
                out.extend(collect_files(&path, true));
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        out.push((path, meta.len()));
    }
    out
}

/// `img:<文件名>` 形式引用的本地文件名；http(s)/data URL 与空串都返回 None。
fn img_file_ref(raw: &str) -> Option<&str> {
    raw.trim()
        .strip_prefix("img:")
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// 头像引用：历史约定是**裸文件名**（`ops_config::validate_settings` 同口径），
/// 也容忍 `img:` 前缀；data URL 与 http(s) 链接不指向本地文件。
fn avatar_file_ref(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("data:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return None;
    }
    Some(img_file_ref(trimmed).unwrap_or(trimmed))
}

/// 日志文件名 `server-YYYYMMDD.log` 里的日期段。
fn log_date(name: &str) -> Option<&str> {
    let date = name.strip_prefix("server-")?.strip_suffix(".log")?;
    (date.len() == 8 && date.chars().all(|ch| ch.is_ascii_digit())).then_some(date)
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// 扫一遍数据目录。领域文件读不出来时直接报错——引用集合不完整就动手，
/// 会把还有人指着的图片当孤儿删掉，宁可让命令失败。
fn scan(ctx: &ExecContext) -> CoreResult<Scan> {
    let layout = &ctx.repo.layout;
    let data = ctx.repo.load_data()?;
    let diary = ctx.repo.load_diary()?;
    let ledger = ctx.repo.load_ledger()?;
    let settings = ctx.repo.load_settings()?;
    let mut out = Scan::default();

    for (path, len) in collect_files(&layout.root, true) {
        let _ = path;
        out.total_bytes += len;
    }

    // ---- 插图（img/data/<id>/，含日记伪条目）----
    let node_ids: HashSet<&str> = data.nodes.iter().map(|node| node.id.as_str()).collect();
    if let Ok(entries) = std::fs::read_dir(layout.entry_img_root()) {
        let mut dir_ids: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(id) = path.file_name().and_then(|raw| raw.to_str()) else {
                continue;
            };
            dir_ids.push(id.to_string());
        }
        // read_dir 顺序不稳定；排序让孤儿清单（以及测试）确定性
        dir_ids.sort();
        for id in dir_ids {
            let dir = layout.entry_img_dir(&id);
            // 引用集合：现存节点 = 归属它的任务 markdown；diary = 全部日记；
            // ledger = 全部账目 images 列表的并集（裸文件名，不是 markdown）；
            // 都不是 = 目录整个是孤儿（条目删掉后同步/崩溃留下的残骸）
            let referenced: HashSet<String> = if id == DIARY_IMAGE_NODE {
                let markdowns: Vec<&str> = diary
                    .entries
                    .iter()
                    .map(|entry| entry.markdown.as_str())
                    .collect();
                crate::image_gc::referenced_basenames(markdowns)
            } else if id == LEDGER_IMAGE_NODE {
                ledger
                    .entries
                    .iter()
                    .flat_map(|entry| entry.images.iter().cloned())
                    .collect()
            } else if node_ids.contains(id.as_str()) {
                let markdowns: Vec<&str> = data
                    .tasks
                    .iter()
                    .filter(|item| item.node_id == id)
                    .map(|item| item.markdown.as_str())
                    .collect();
                crate::image_gc::referenced_basenames(markdowns)
            } else {
                HashSet::new()
            };
            for (path, len) in collect_files(&dir, false) {
                out.images.add(len);
                let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else {
                    continue;
                };
                // 与保存后清理同一套护栏：只认图片扩展名 + 安全文件名
                if is_image_name(name) && is_safe_image_name(name) && !referenced.contains(name) {
                    out.orphan_images.push((path, len));
                }
            }
            if id != DIARY_IMAGE_NODE && id != LEDGER_IMAGE_NODE && !node_ids.contains(id.as_str()) {
                out.prunable_dirs.push(dir);
            }
        }
    }

    // ---- 背景（img/background/）----
    let mut bg_refs: HashSet<String> = HashSet::new();
    for value in data.backgrounds.values() {
        if let Some(name) = value.get("image").and_then(Value::as_str).and_then(img_file_ref) {
            bg_refs.insert(name.to_string());
        }
    }
    for raw in [
        &settings.appearance.new_node_defaults.background_image,
        &settings.diary.background_image,
        &settings.ledger.background_image,
    ] {
        if let Some(name) = img_file_ref(raw) {
            bg_refs.insert(name.to_string());
        }
    }
    let mut bg_files = collect_files(&layout.background_img_dir(), false);
    bg_files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, len) in bg_files {
        out.backgrounds.add(len);
        let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else {
            continue;
        };
        if is_image_name(name) && is_safe_image_name(name) && !bg_refs.contains(name) {
            out.orphan_backgrounds.push((path, len));
        }
    }

    // ---- 头像（img/avator/，历史拼写）----
    let avatar_ref = avatar_file_ref(&settings.profile.avatar).map(str::to_string);
    let mut avatar_files = collect_files(&layout.avatar_img_dir(), false);
    avatar_files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, len) in avatar_files {
        out.avatars.add(len);
        let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else {
            continue;
        };
        if is_image_name(name) && is_safe_image_name(name) && avatar_ref.as_deref() != Some(name) {
            out.orphan_avatars.push((path, len));
        }
    }

    // ---- 临时文件与 img/ 下的 .part 残留 ----
    if let Ok(paths) = crate::repo::list_temp_files(&layout.root) {
        for path in paths {
            if let Ok(meta) = std::fs::metadata(&path) {
                out.temp_files.push((path, meta.len()));
            }
        }
    }
    fn collect_parts(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_parts(&path, out);
                continue;
            }
            let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else {
                continue;
            };
            if name.ends_with(".part") {
                if let Ok(meta) = std::fs::metadata(&path) {
                    out.push((path, meta.len()));
                }
            }
        }
    }
    collect_parts(&layout.img_dir(), &mut out.part_files);
    out.part_files.sort_by(|a, b| a.0.cmp(&b.0));

    // ---- 服务器日志（<dataDir>/server/log/server-YYYYMMDD.log）----
    let log_dir = layout.root.join("server").join("log");
    let today = chrono::Local::now().format("%Y%m%d").to_string();
    let mut logs: Vec<(String, PathBuf, u64)> = Vec::new();
    for (path, len) in collect_files(&log_dir, false) {
        let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else {
            continue;
        };
        let Some(date) = log_date(name) else { continue };
        out.server_logs.add(len);
        logs.push((date.to_string(), path, len));
    }
    // 保留最新两份，且今天的永远不删（正在被服务进程写着）
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    for (index, (date, path, len)) in logs.into_iter().enumerate() {
        if index < 2 || date == today {
            continue;
        }
        out.cleanable_logs.push((path, len));
    }

    // ---- 备份（只统计）----
    for (_path, len) in collect_files(&layout.backup_dir(), true) {
        out.backups.add(len);
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

/// 只读统计：设置页「释放空间」卡片的数据源。键名是与前端的契约，别改。
fn storage_usage(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let data = ctx.repo.load_data()?;
    crate::core::set_read_revision(meta, Domain::Data, data.meta.revision);
    let scan = scan(ctx)?;
    Ok(json!({
        "dataDir": ctx.repo.layout.root.display().to_string(),
        "totalBytes": scan.total_bytes,
        "images": scan.images.view(),
        "orphanImages": orphan_view(&scan.orphan_images),
        "backgrounds": scan.backgrounds.view(),
        "orphanBackgrounds": orphan_view(&scan.orphan_backgrounds),
        "avatars": scan.avatars.view(),
        "orphanAvatars": orphan_view(&scan.orphan_avatars),
        "tempFiles": orphan_view(&scan.temp_files),
        "serverLogs": scan.server_logs.view(),
        "cleanableLogs": orphan_view(&scan.cleanable_logs),
        "backups": scan.backups.view(),
    }))
}

fn orphan_view(files: &[(PathBuf, u64)]) -> Value {
    let bytes: u64 = files.iter().map(|(_, len)| len).sum();
    json!({ "count": files.len(), "bytes": bytes })
}

/// 清理：孤儿图片 + 临时文件/`.part` 残留 + 过期服务器日志。高风险写（不可恢复），
/// 未带 --yes 报确认门；单个文件删不动只进 warnings，绝不半途硬失败。
fn storage_clean(inv: &Invocation, ctx: &ExecContext, _meta: &mut Meta) -> CoreResult<Value> {
    let scan = scan(ctx)?;
    let part_bytes: u64 = scan.part_files.iter().map(|(_, len)| len).sum();
    let temp_bytes: u64 = scan.temp_files.iter().map(|(_, len)| len).sum();
    let image_bytes: u64 = scan.orphan_images.iter().map(|(_, len)| len).sum();
    let bg_bytes: u64 = scan.orphan_backgrounds.iter().map(|(_, len)| len).sum();
    let avatar_bytes: u64 = scan.orphan_avatars.iter().map(|(_, len)| len).sum();
    let log_bytes: u64 = scan.cleanable_logs.iter().map(|(_, len)| len).sum();
    let freed = image_bytes + bg_bytes + avatar_bytes + temp_bytes + part_bytes + log_bytes;
    let plan = json!({
        "type": "storage-clean",
        "orphanImages": scan.orphan_images.len(),
        "orphanBackgrounds": scan.orphan_backgrounds.len(),
        "orphanAvatars": scan.orphan_avatars.len(),
        "tempFiles": scan.temp_files.len() + scan.part_files.len(),
        "staleLogs": scan.cleanable_logs.len(),
        "estimatedFreedBytes": freed,
    });
    require_confirmation(
        &inv.controls,
        format!(
            "清理数据目录：删除 {} 张无引用插图、{} 张无引用背景图、{} 个无引用头像、{} 个临时文件、{} 份过期服务器日志，约释放 {}（不可恢复）",
            scan.orphan_images.len(),
            scan.orphan_backgrounds.len(),
            scan.orphan_avatars.len(),
            scan.temp_files.len() + scan.part_files.len(),
            scan.cleanable_logs.len(),
            human_bytes(freed),
        ),
        plan.clone(),
    )?;
    if inv.controls.dry_run {
        return Ok(json!({ "dryRun": true, "action": "clean", "plan": plan }));
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut freed_bytes = 0u64;
    let mut removed = |files: Vec<(PathBuf, u64)>, what: &str, counter: &mut u64| {
        for (path, len) in files {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    freed_bytes += len;
                    *counter += 1;
                }
                Err(error) => warnings.push(format!(
                    "无法删除{what} {}：{error}",
                    path.file_name()
                        .map(|raw| raw.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string())
                )),
            }
        }
    };

    let mut removed_images = 0u64;
    let mut removed_backgrounds = 0u64;
    let mut removed_avatars = 0u64;
    let mut removed_temp = 0u64;
    let mut removed_logs = 0u64;
    removed(scan.orphan_images, "无引用插图", &mut removed_images);
    removed(scan.orphan_backgrounds, "无引用背景图", &mut removed_backgrounds);
    removed(scan.orphan_avatars, "无引用头像", &mut removed_avatars);
    removed(scan.temp_files, "临时文件", &mut removed_temp);
    removed(scan.part_files, "残留临时文件", &mut removed_temp);
    removed(scan.cleanable_logs, "过期日志", &mut removed_logs);

    // 孤儿目录顺手清掉：remove_dir 只对空目录成功，非空即失败——天然的护栏。
    // diary 伪条目与现存节点的目录留着（它们随时会有新图进来）。
    for dir in &scan.prunable_dirs {
        // 目录非空（还有非图片文件）或读不动：不是错误，不值得一条 warning
        let _ = std::fs::remove_dir(dir);
    }

    Ok(json!({
        "freedBytes": freed_bytes,
        "removedImages": removed_images,
        "removedBackgrounds": removed_backgrounds,
        "removedAvatars": removed_avatars,
        "removedTempFiles": removed_temp,
        "removedLogs": removed_logs,
        "warnings": warnings,
    }))
}

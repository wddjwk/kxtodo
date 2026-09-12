//! diary domain: 日记的业务操作（GUI 与 CLI 共用同一条命令层）。
//!
//! 日记住在自己的 `diary.json` 里（第四个领域文件），于是写一篇日记既不抬高 data 域的
//! revision，也不和任务写入抢同一把幂等台账。同步上它仍然搭「同步数据」的范围。

use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_str, require_confirmation, required_str,
    set_read_revision, ExecContext, Invocation,
};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{DiaryEntry, DiaryFile, Tag};
use crate::ops_task::{build_tag, parse_tag_input};
use crate::repo::Domain;
use crate::time::{now_iso, parse_date, today_local};

pub fn diary_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "add" => diary_add(inv, ctx, meta),
        "get" => diary_get(inv, ctx, meta),
        "list" => diary_list(inv, ctx, meta),
        "modify" => diary_modify(inv, ctx, meta),
        "remove" => diary_remove(inv, ctx, meta),
        "import" => diary_import(inv, ctx, meta),
        "export" => diary_export(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 diary 动作 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub fn find_diary<'a>(file: &'a DiaryFile, id: &str) -> Option<&'a DiaryEntry> {
    file.entries.iter().find(|entry| entry.id == id)
}

fn not_found(id: &str) -> CoreError {
    CoreError::not_found("DIARY_NOT_FOUND", format!("未找到日记 {id}"))
        .with_hint("先运行 kxtodo-cli diary list 获取稳定 ID")
}

/// 空标题 + 空正文的日记没有意义（GUI 的编辑器也按这条规则不落盘）。
fn ensure_has_content(title: &str, markdown: &str) -> CoreResult<()> {
    if title.trim().is_empty() && markdown.trim().is_empty() {
        return Err(CoreError::validation(
            "DIARY_EMPTY",
            "标题与正文不能同时为空",
        ));
    }
    Ok(())
}

fn tags_param(params: &Value, key: &str) -> CoreResult<Option<Vec<Tag>>> {
    let Some(list) = params.get(key).and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut tags = Vec::new();
    for raw in list {
        tags.push(build_tag(&parse_tag_input(raw.as_str().unwrap_or_default())?));
    }
    Ok(Some(tags))
}

pub fn diary_view(entry: &DiaryEntry) -> Value {
    json!({
        "id": entry.id,
        "date": entry.date,
        "title": entry.title,
        "markdown": entry.markdown,
        "mood": entry.mood,
        "weather": entry.weather,
        "tags": entry
            .tags
            .iter()
            .map(|tag| json!({
                "id": tag.id,
                "color": tag.color.as_str(),
                "text": tag.text,
            }))
            .collect::<Vec<_>>(),
        "expanded": entry.expanded,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
    })
}

/// 日记的稳定展示顺序：日期由近及远，同一天内按写作先后（早的在上）。
pub fn sort_entries(entries: &mut [DiaryEntry]) {
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// 日期区间过滤（`from`/`to` 都是闭区间，None = 不限）。
fn in_range(date: &str, from: Option<&str>, to: Option<&str>) -> bool {
    if let Some(from) = from {
        if date < from {
            return false;
        }
    }
    if let Some(to) = to {
        if date > to {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

/// 日记侧保存后的插图清理（v0.7.2）：对着**写入后**的日记文件扫 `img/data/diary/`，
/// 删掉没有任何一篇日记再引用的图片（宽限窗保护在途图片，见 `image_gc`）。
/// 清理失败一律吞掉——绝不让保存本身因为清理而失败。
fn sweep_diary_images(ctx: &ExecContext, file: &crate::model::DiaryFile) {
    let dir = ctx
        .repo
        .layout
        .entry_img_dir(crate::model::DIARY_IMAGE_NODE);
    let markdowns: Vec<&str> = file
        .entries
        .iter()
        .map(|entry| entry.markdown.as_str())
        .collect();
    crate::image_gc::sweep_unreferenced(&dir, markdowns, crate::image_gc::GRACE);
}

fn diary_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let date = match param_str(params, "date") {
        Some(raw) => parse_date(&raw)?,
        None => today_local(),
    };
    let title = param_str(params, "title").unwrap_or_default();
    let markdown = param_str(params, "markdown").unwrap_or_default();
    ensure_has_content(&title, &markdown)?;
    let mood = param_str(params, "mood").unwrap_or_default();
    let weather = param_str(params, "weather").unwrap_or_default();
    let tags = tags_param(params, "tags")?.unwrap_or_default();

    let mut created = Value::Null;
    let (file, outcome) = ctx.repo.write_diary(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let now = now_iso();
            let entry = DiaryEntry {
                id: gen_id("diary"),
                date,
                title,
                markdown,
                mood,
                weather,
                tags,
                expanded: None,
                created_at: now.clone(),
                updated_at: Some(now),
                extra: Map::new(),
            };
            let view = diary_view(&entry);
            file.entries.push(entry);
            created = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Diary, &outcome);
    notify_host(ctx, Domain::Diary, outcome.revision, vec![]);
    sweep_diary_images(ctx, &file);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(created));
    }
    Ok(created)
}

fn diary_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_diary()?;
    set_read_revision(meta, Domain::Diary, file.meta.revision);
    let entry = find_diary(&file, &id).ok_or_else(|| not_found(&id))?;
    Ok(diary_view(entry))
}

fn diary_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_diary()?;
    set_read_revision(meta, Domain::Diary, file.meta.revision);
    let date = param_str(params, "date").map(|raw| parse_date(&raw)).transpose()?;
    let from = param_str(params, "from").map(|raw| parse_date(&raw)).transpose()?;
    let to = param_str(params, "to").map(|raw| parse_date(&raw)).transpose()?;
    let limit = params.get("limit").and_then(Value::as_u64).map(|v| v as usize);

    let mut entries: Vec<DiaryEntry> = file
        .entries
        .iter()
        .filter(|entry| {
            if let Some(date) = &date {
                return entry.date == *date;
            }
            in_range(&entry.date, from.as_deref(), to.as_deref())
        })
        .cloned()
        .collect();
    sort_entries(&mut entries);
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    Ok(json!({
        "total": file.entries.len(),
        "returned": entries.len(),
        "items": entries.iter().map(diary_view).collect::<Vec<_>>(),
    }))
}

fn diary_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let date = param_str(params, "date").map(|raw| parse_date(&raw)).transpose()?;
    let title = param_str(params, "title");
    let markdown = param_str(params, "markdown");
    let mood = param_str(params, "mood");
    let weather = param_str(params, "weather");
    let tags = tags_param(params, "replaceTags")?;

    let mut updated = Value::Null;
    let (file, outcome) = ctx.repo.write_diary(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let entry = file
                .entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| not_found(&id))?;
            if let Some(date) = date {
                entry.date = date;
            }
            if let Some(title) = &title {
                entry.title = title.chars().take(120).collect();
            }
            if let Some(markdown) = &markdown {
                entry.markdown = markdown.clone();
            }
            if let Some(mood) = &mood {
                entry.mood = mood.trim().to_string();
            }
            if let Some(weather) = &weather {
                entry.weather = weather.trim().to_string();
            }
            if let Some(tags) = tags {
                entry.tags = tags;
            }
            ensure_has_content(&entry.title, &entry.markdown)?;
            entry.updated_at = Some(now_iso());
            let view = diary_view(entry);
            updated = view.clone();
            Ok(idem_summary(&view))
        },
    )?;
    apply_write_outcome(meta, Domain::Diary, &outcome);
    notify_host(ctx, Domain::Diary, outcome.revision, vec![id.clone()]);
    sweep_diary_images(ctx, &file);
    if outcome.replayed {
        return Ok(outcome.replay_summary.unwrap_or(updated));
    }
    Ok(updated)
}

fn diary_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_diary()?;
    let entry = find_diary(&file, &id).ok_or_else(|| not_found(&id))?;
    let plan = json!({
        "type": "diary",
        "id": id,
        "date": entry.date,
        "diaryIds": [id],
    });
    require_confirmation(
        &inv.controls,
        format!("删除 {} 的日记 {id}", entry.date),
        plan.clone(),
    )?;
    if inv.controls.dry_run {
        set_read_revision(meta, Domain::Diary, file.meta.revision);
        return Ok(json!({ "dryRun": true, "action": "remove", "plan": plan }));
    }
    let (file, outcome) = ctx.repo.write_diary(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let index = file
                .entries
                .iter()
                .position(|entry| entry.id == id)
                .ok_or_else(|| not_found(&id))?;
            file.entries.remove(index);
            // 删除必须显式传播：服务器只见密文，没有墓碑对端会把它推回来。
            file.meta.record_tombstone(&id, "diary", &now_iso());
            Ok(json!({ "removed": plan }))
        },
    )?;
    apply_write_outcome(meta, Domain::Diary, &outcome);
    notify_host(ctx, Domain::Diary, outcome.revision, vec![id.clone()]);
    sweep_diary_images(ctx, &file);
    Ok(json!({ "removed": plan, "revision": outcome.revision }))
}

/// 导入压缩包（`zipBase64`）里的日记与插图。**解析在 core 侧完成**：插图字节要和
/// entries 一起拿到，才能把 `images/` 里的图落回 `img/data/diary/`。
///
/// **同一天已有日记不算冲突**：日记本来就允许一天多篇，导入进来的直接追加成另一篇，
/// 既不合并正文也不去重——用户的两篇就是两篇。
fn diary_import(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let encoded = params
        .get("zipBase64")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 zipBase64 压缩包内容"))?;
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| CoreError::validation("DIARY_IMPORT_INVALID", format!("压缩包内容解码失败：{error}")))?;
    let archive = crate::diary_archive::parse_zip(&raw)?;
    let raw_entries = &archive.entries;

    // 先在事务外把每一条都校验一遍：一条坏数据不该让整个导入半途而废
    let mut drafts: Vec<(String, String, String, String, String, Vec<Tag>, String)> = Vec::new();
    let mut skipped = 0usize;
    for item in raw_entries {
        let date = match item.get("date").and_then(Value::as_str) {
            Some(raw) => match parse_date(raw) {
                Ok(date) => date,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            },
            None => {
                skipped += 1;
                continue;
            }
        };
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect::<String>();
        let markdown = item
            .get("markdown")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if ensure_has_content(&title, &markdown).is_err() {
            skipped += 1;
            continue;
        }
        let mood = item
            .get("mood")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let weather = item
            .get("weather")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut tags = Vec::new();
        for raw_tag in item
            .get("tags")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if let Ok(tag) = parse_tag_input(raw_tag.as_str().unwrap_or_default()) {
                tags.push(build_tag(&tag));
            }
        }
        let created = item
            .get("createdAt")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(now_iso);
        drafts.push((date, title, markdown, mood, weather, tags, created));
    }
    if drafts.is_empty() && !raw_entries.is_empty() {
        return Err(CoreError::validation(
            "DIARY_IMPORT_EMPTY",
            "压缩包里没有任何可导入的日记（每篇至少要有一个合法日期，且标题与正文不同时为空）",
        ));
    }

    let count = drafts.len();
    // 导入是追加而不是合并：同一个包跑两遍就会得到两份，所以要走确认门
    //（GUI 桥接默认 yes=true，不受影响）。
    require_confirmation(
        &inv.controls,
        format!("导入 {count} 篇日记（跳过 {skipped} 条无效记录）；同一天已有日记会并存为多篇"),
        json!({ "type": "diary-import", "imported": count, "skipped": skipped }),
    )?;
    if inv.controls.dry_run {
        let file = ctx.repo.load_diary()?;
        set_read_revision(meta, Domain::Diary, file.meta.revision);
        return Ok(json!({
            "dryRun": true,
            "action": "import",
            "imported": count,
            "skipped": skipped,
        }));
    }

    // 插图落回 img/data/diary/：正文里的引用是裸文件名，图不在就等于日记丢了图。
    // 已存在的同名文件不覆盖——图片是内容寻址的不可变 blob，重导同一个包必须幂等。
    let mut images_written = 0usize;
    if !archive.images.is_empty() {
        let image_dir = ctx.repo.layout.entry_img_dir(crate::model::DIARY_IMAGE_NODE);
        std::fs::create_dir_all(&image_dir)?;
        for (name, bytes) in &archive.images {
            let target = image_dir.join(name);
            if target.exists() {
                continue;
            }
            std::fs::write(&target, bytes)
                .map_err(|error| CoreError::io(format!("无法写入插图 {}：{error}", target.display())))?;
            images_written += 1;
        }
    }

    let mut imported_ids: Vec<String> = Vec::new();
    let (_file, outcome) = ctx.repo.write_diary(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            for (date, title, markdown, mood, weather, tags, created_at) in drafts {
                let id = gen_id("diary");
                file.entries.push(DiaryEntry {
                    id: id.clone(),
                    date,
                    title,
                    markdown,
                    mood,
                    weather,
                    tags,
                    expanded: None,
                    updated_at: Some(created_at.clone()),
                    created_at,
                    extra: Map::new(),
                });
                imported_ids.push(id);
            }
            sort_entries(&mut file.entries);
            Ok(json!({ "imported": imported_ids.len() }))
        },
    )?;
    apply_write_outcome(meta, Domain::Diary, &outcome);
    notify_host(ctx, Domain::Diary, outcome.revision, imported_ids.clone());
    Ok(json!({
        "imported": if outcome.replayed { 0 } else { count },
        "skipped": skipped,
        "images": images_written,
        "ids": imported_ids,
        "revision": outcome.revision,
    }))
}

/// 导出成压缩包：`年/月/YYYYMMDD[_序号][_标题].md`，元数据写进 YAML front-matter。
/// `--out` 给路径就直接落盘（CLI/桌面），不给就返回 base64（移动端交给分享桥）。
fn diary_export(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_diary()?;
    set_read_revision(meta, Domain::Diary, file.meta.revision);
    let from = param_str(params, "from").map(|raw| parse_date(&raw)).transpose()?;
    let to = param_str(params, "to").map(|raw| parse_date(&raw)).transpose()?;

    let entries: Vec<&DiaryEntry> = {
        let mut picked: Vec<&DiaryEntry> = file
            .entries
            .iter()
            .filter(|entry| in_range(&entry.date, from.as_deref(), to.as_deref()))
            .collect();
        // 导出顺序 = 阅读顺序：日期由远及近，同一天内按写作先后
        picked.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        picked
    };
    // 插图随包带走：正文里引用到的本地图从 img/data/diary/ 读字节，读不到的引用原样保留
    let image_dir = ctx.repo.layout.entry_img_dir(crate::model::DIARY_IMAGE_NODE);
    let archive = crate::diary_archive::build_zip(&entries, &|name| {
        if !crate::diary_archive::is_safe_image_name(name) {
            return None;
        }
        std::fs::read(image_dir.join(name)).ok()
    })?;
    let name = crate::diary_archive::archive_name(from.as_deref(), to.as_deref());

    if let Some(path) = param_str(params, "out") {
        let target = std::path::Path::new(&path);
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(target, &archive)
            .map_err(|error| CoreError::io(format!("无法写入 {}：{error}", target.display())))?;
        return Ok(json!({
            "path": target.display().to_string(),
            "entries": entries.len(),
            "bytes": archive.len(),
            "name": name,
        }));
    }
    use base64::Engine as _;
    Ok(json!({
        "name": name,
        "entries": entries.len(),
        "bytes": archive.len(),
        "base64": base64::engine::general_purpose::STANDARD.encode(&archive),
    }))
}

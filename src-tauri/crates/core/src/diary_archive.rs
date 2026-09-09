//! 日记的压缩包格式：`年/月/YYYYMMDD[_序号][_标题].md`，元数据写在 YAML front-matter 里。
//!
//! 这个格式是**给人看的**：解压出来就是一棵按年月分好的 Markdown 树，任何编辑器都能读，
//! 不依赖 KXToDo 也能翻。所以导出用最朴素的 YAML 标量与流式列表，导入则尽量宽容
//! （字段缺了就退回文件名里的日期，值解析不了就按空处理，正文永远保住）。

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::sync::OnceLock;

use chrono::{DateTime, Local, TimeZone};
use regex::Regex;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{CoreError, CoreResult};
use crate::model::DiaryEntry;

/// 导入护栏：压缩包炸弹不该把内存吃光。
const MAX_ARCHIVE_BYTES: usize = 200 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// 文件名里的标题上限（按字符数，中文标题 48 字已经很长）
const TITLE_CHARS_IN_NAME: usize = 48;
/// 压缩包内插图统一放这个目录（解压出来与 年/月 同级）
pub(crate) const IMAGES_DIR: &str = "images";

fn image_ref_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"!\[([^\]]*)\]\(([^)]+)\)").expect("valid regex"))
}

fn is_remote_src(src: &str) -> bool {
    src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:")
}

/// 逐处改写 Markdown 里的图片引用。回调返回 None 表示这一处不动。
/// 远程链接与 data: 内联图永远不碰。
pub(crate) fn rewrite_image_refs(markdown: &str, mut map: impl FnMut(&str) -> Option<String>) -> String {
    image_ref_pattern()
        .replace_all(markdown, |caps: &regex::Captures<'_>| {
            let alt = &caps[1];
            let src = caps[2].trim();
            if is_remote_src(src) {
                return caps[0].to_string();
            }
            match map(src) {
                Some(next) => format!("![{alt}]({next})"),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

/// 取引用里的最后一段文件名（容忍 `../../images/x.png` 与手写 md 的相对路径）。
pub(crate) fn ref_basename(src: &str) -> &str {
    src.rsplit(['/', '\\']).next().unwrap_or(src)
}

// ---------------------------------------------------------------------------
// 导出
// ---------------------------------------------------------------------------

/// 压缩包默认名。带日期范围时把范围写进名字，免得下载目录里一堆同名文件分不清。
pub fn archive_name(from: Option<&str>, to: Option<&str>) -> String {
    match (from, to) {
        (None, None) => format!("kxtodo-diary-{}.zip", crate::core::APP_VERSION),
        (Some(from), Some(to)) => format!("kxtodo-diary-{from}-{to}.zip"),
        (Some(from), None) => format!("kxtodo-diary-from-{from}.zip"),
        (None, Some(to)) => format!("kxtodo-diary-to-{to}.zip"),
    }
}

/// 构建压缩包字节。`entries` 必须已经按（日期升序，同天内写作先后）排好——
/// 序号就是从这个顺序数出来的。
///
/// `read_image` 按文件名取日记插图字节（调用方从 `img/data/diary/` 读）：
/// 取得到的随包带走、正文引用改写成 `images/<文件名>` 的相对路径，取不到的引用原样保留。
pub fn build_zip(
    entries: &[&DiaryEntry],
    read_image: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> CoreResult<Vec<u8>> {
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in entries {
        *totals.entry(entry.date.as_str()).or_insert(0) += 1;
    }

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut seen_per_day: BTreeMap<&str, usize> = BTreeMap::new();
    let mut used_paths: HashSet<String> = HashSet::new();
    let mut images: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    for entry in entries {
        let index = seen_per_day.entry(entry.date.as_str()).or_insert(0);
        *index += 1;
        let total = totals.get(entry.date.as_str()).copied().unwrap_or(1);
        let path = entry_path(entry, *index, total, &mut used_paths);
        let body = render_markdown_with_images(entry, read_image, &mut images);
        writer
            .start_file(path, options)
            .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
        writer
            .write_all(body.as_bytes())
            .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
    }

    // 图片本身已是压缩格式，二次 Deflate 只烧 CPU 不换体积
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in &images {
        writer
            .start_file(format!("{IMAGES_DIR}/{name}"), stored)
            .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
        writer
            .write_all(bytes)
            .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
    }

    let cursor = writer
        .finish()
        .map_err(|error| CoreError::internal(format!("收尾压缩包失败：{error}")))?;
    Ok(cursor.into_inner())
}

/// 一篇日记的导出文本：正文里读得到的本地图引用改写成包内相对路径，字节收进 images 表。
fn render_markdown_with_images(
    entry: &DiaryEntry,
    read_image: &dyn Fn(&str) -> Option<Vec<u8>>,
    images: &mut BTreeMap<String, Vec<u8>>,
) -> String {
    let body = rewrite_image_refs(entry.markdown.trim_end(), |src| {
        let name = ref_basename(src);
        if !is_safe_image_name(name) {
            return None;
        }
        if !images.contains_key(name) {
            let Some(bytes) = read_image(name) else {
                return None;
            };
            images.insert(name.to_string(), bytes);
        }
        Some(format!("{IMAGES_DIR}/{name}"))
    });
    let mut text = render_front_matter(entry);
    text.push_str(&body);
    text.push('\n');
    text
}

/// `年/月/YYYYMMDD[_序号][_标题].md`。一天只有一篇就不写序号，没有标题就不写标题。
fn entry_path(entry: &DiaryEntry, index: usize, total: usize, used: &mut HashSet<String>) -> String {
    let year = entry.date.get(0..4).unwrap_or("0000");
    let month = entry.date.get(5..7).unwrap_or("01");
    let compact = entry.date.replace('-', "");
    let mut stem = if total > 1 {
        format!("{compact}_{index}")
    } else {
        compact
    };
    let title = sanitize_for_filename(&entry.title);
    if !title.is_empty() {
        stem.push('_');
        stem.push_str(&title);
    }
    let path = format!("{year}/{month}/{stem}.md");
    if used.insert(path.clone()) {
        return path;
    }
    // 同一天两篇标题清洗后撞车（例如都叫「无题?」）：拿 id 尾巴区分，绝不互相覆盖
    let suffix = entry.id.chars().rev().take(6).collect::<Vec<_>>();
    let unique = format!("{year}/{month}/{stem}_{}.md", suffix.iter().rev().collect::<String>());
    used.insert(unique.clone());
    unique
}

/// 文件名安全化：去掉路径分隔符与 Windows 保留字符、前导点、控制字符，压掉连续空白。
pub(crate) fn sanitize_for_filename(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let mut joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // 前导点在 Unix 下是隐藏文件，在同步盘上常被忽略。
    // 要反复剥：标题像「.. .. x」这样点与空格交错时，单调一次 trim_start_matches 剩下的仍以点开头
    loop {
        let stripped = joined.trim_start().strip_prefix('.');
        match stripped {
            Some(rest) => joined = rest.to_string(),
            None => break,
        }
    }
    joined
        .trim()
        .chars()
        .take(TITLE_CHARS_IN_NAME)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// 一篇日记 → 带 front-matter 的 Markdown 文本。空字段直接不写（文件是给人读的）。
pub fn render_markdown(entry: &DiaryEntry) -> String {
    let mut out = render_front_matter(entry);
    out.push_str(entry.markdown.trim_end());
    out.push('\n');
    out
}

/// 只产出 `---` 包裹的元数据块（正文由调用方拼，导出带图时要先改写引用）。
fn render_front_matter(entry: &DiaryEntry) -> String {
    let mut out = String::from("---\n");
    put_scalar(&mut out, "title", &entry.title);
    out.push_str(&format!("date: {}\n", entry.date));
    put_scalar(&mut out, "time", &time_of(&entry.created_at));
    put_scalar(&mut out, "mood", &entry.mood);
    put_scalar(&mut out, "weather", &entry.weather);
    if !entry.tags.is_empty() {
        let items = entry
            .tags
            .iter()
            .map(|tag| match tag.text.as_deref().filter(|text| !text.is_empty()) {
                Some(text) => yaml_quote(&format!("{}:{}", tag.color.as_str(), text)),
                None => yaml_quote(tag.color.as_str()),
            })
            .collect::<Vec<_>>();
        out.push_str(&format!("tags: [{}]\n", items.join(", ")));
    }
    put_scalar(&mut out, "createdAt", &entry.created_at);
    out.push_str("---\n\n");
    out
}

fn put_scalar(out: &mut String, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    out.push_str(key);
    out.push_str(": ");
    out.push_str(&yaml_quote(trimmed));
    out.push('\n');
}

/// 双引号标量：转义反斜杠、双引号与控制字符（含换行——标题里不该有，但用户手改过就说不准）。
fn yaml_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// ISO 时间戳 → 本地 HH:MM（解析不出返回空串，front-matter 里就不写这一行）。
fn time_of(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|parsed| parsed.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

/// 解析结果：日记草稿数组 + 包内插图（按文件名索引，导入时落回 `img/data/diary/`）。
#[derive(Default)]
pub struct ParsedArchive {
    pub entries: Vec<Value>,
    pub images: BTreeMap<String, Vec<u8>>,
}

/// 解析压缩包。**不落盘、不解压到文件系统**，全部在内存里读，所以没有 zip-slip 一说。
pub fn parse_zip(bytes: &[u8]) -> CoreResult<ParsedArchive> {
    if bytes.is_empty() {
        return Err(CoreError::validation("DIARY_IMPORT_INVALID", "压缩包是空的"));
    }
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(CoreError::validation(
            "DIARY_IMPORT_TOO_LARGE",
            format!("压缩包超过 {}MB 上限", MAX_ARCHIVE_BYTES / 1024 / 1024),
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        CoreError::validation(
            "DIARY_IMPORT_INVALID",
            format!("不是有效的 zip 压缩包：{error}"),
        )
    })?;
    if archive.len() > MAX_FILES {
        return Err(CoreError::validation(
            "DIARY_IMPORT_TOO_LARGE",
            format!("压缩包内文件数超过 {MAX_FILES} 上限"),
        ));
    }

    let mut out = ParsedArchive::default();
    let mut total_bytes: u64 = 0;

    // 第一遍只收插图：导出包里 images/ 排在所有 md 之后，
    // 单遍扫描的话 md 先被处理，引用归一永远看不到包里的图。
    for index in 0..archive.len() {
        let Ok(mut file) = archive.by_index(index) else { continue };
        if file.is_dir() || file.size() > MAX_FILE_BYTES {
            continue;
        }
        let name = file.name().to_string();
        if !is_image_name(&name) {
            continue;
        }
        let mut raw: Vec<u8> = Vec::new();
        if file.read_to_end(&mut raw).is_err() {
            continue;
        }
        total_bytes += raw.len() as u64;
        if total_bytes > MAX_ARCHIVE_BYTES as u64 {
            return Err(CoreError::validation(
                "DIARY_IMPORT_TOO_LARGE",
                "解压后体积超过上限（疑似压缩包炸弹）",
            ));
        }
        let base = ref_basename(&name).to_string();
        if is_safe_image_name(&base) {
            out.images.entry(base).or_insert(raw);
        }
    }

    // 第二遍解析日记正文，并把引用归一回裸文件名
    for index in 0..archive.len() {
        let Ok(mut file) = archive.by_index(index) else { continue };
        if file.is_dir() || file.size() > MAX_FILE_BYTES {
            continue;
        }
        let name = file.name().to_string();
        if !is_markdown(&name) {
            continue;
        }
        let mut raw: Vec<u8> = Vec::new();
        if file.read_to_end(&mut raw).is_err() {
            continue;
        }
        total_bytes += raw.len() as u64;
        if total_bytes > MAX_ARCHIVE_BYTES as u64 {
            return Err(CoreError::validation(
                "DIARY_IMPORT_TOO_LARGE",
                "解压后体积超过上限（疑似压缩包炸弹）",
            ));
        }
        // 用户可能在别的编辑器里存成 GBK：lossy 解码保住能读的部分，别整篇丢掉
        let text = String::from_utf8_lossy(&raw);
        let mut draft = parse_markdown(&text, &name);
        if let Some(markdown) = draft.get("markdown").and_then(Value::as_str).map(str::to_string) {
            let rewritten = rewrite_image_refs(&markdown, |src| {
                let base = ref_basename(src);
                // 包里有这张图才归一回裸文件名；没有就原样保留（别把外链改坏）
                out.images.contains_key(base).then(|| base.to_string())
            });
            draft["markdown"] = Value::String(rewritten);
        }
        out.entries.push(draft);
    }
    Ok(out)
}

pub(crate) fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".txt")
}

pub(crate) fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
}

/// 落盘前的文件名护栏：不带路径分隔符与 `..`、没有控制字符、长度有界。
/// 包内名字来自外部输入，拼进本地目录前必须过这一道。
pub fn is_safe_image_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && !name.starts_with('.')
        && !name.chars().any(|ch| ch.is_control())
}

/// 从 zip 内路径取最后一段文件名（也容忍用户解压后重新打包多出来的一层目录）。
fn file_stem(path: &str) -> &str {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    last.rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(last)
}

/// 一篇 Markdown → 导入草稿。日期优先取 front-matter，其次文件名里的 YYYYMMDD，
/// 再次 zip 路径里的 年/月；都拿不到就给空串，交给 `diary.import` 判为无效并跳过。
fn parse_markdown(text: &str, path: &str) -> Value {
    let (front, body) = split_front_matter(text);
    let fields = parse_front_matter(&front);
    let scalar = |key: &str| fields.scalars.get(key).cloned().unwrap_or_default();

    let mut date = normalize_date(&scalar("date"));
    if date.is_empty() {
        date = date_from_path(path).unwrap_or_default();
    }
    let created = {
        let explicit = scalar("createdAt");
        if DateTime::parse_from_rfc3339(&explicit).is_ok() {
            explicit
        } else {
            compose_timestamp(&date, &scalar("time")).unwrap_or_default()
        }
    };
    let tags = fields
        .lists
        .get("tags")
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    if item.contains(':') {
                        return item.clone();
                    }
                    // 没冒号：可能本来就是「只有颜色」的标签（导出时就写成 "green"），
                    // 也可能是用户手改成的纯文字——后者归到灰色，别把整个标签丢掉
                    if crate::model::TagColor::parse(item).is_some() {
                        item.clone()
                    } else {
                        format!("gray:{item}")
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "date": date,
        "title": scalar("title"),
        "markdown": body,
        "mood": scalar("mood"),
        "weather": scalar("weather"),
        "tags": tags,
        "createdAt": created,
    })
}

/// 切出 front-matter 与正文。没有 front-matter 时整篇都是正文（用户手写 md 也要能导）。
fn split_front_matter(text: &str) -> (String, String) {
    let cleaned = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = cleaned.lines().collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return (String::new(), strip_leading_blank(cleaned));
    }
    let Some(offset) = lines.iter().skip(1).position(|line| line.trim() == "---") else {
        // 只有开头一条 ---：当成正文，别把整篇吞掉
        return (String::new(), strip_leading_blank(cleaned));
    };
    let front = lines[1..=offset].join("\n");
    let body = lines[offset + 2..].join("\n");
    (front, strip_leading_blank(&body))
}

fn strip_leading_blank(text: &str) -> String {
    text.trim_start_matches(['\r', '\n']).to_string()
}

#[derive(Default)]
struct FrontMatter {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
}

/// 极简 YAML 读取：只认我们自己写出来的那几种形状——`key: 标量`、`key: [a, b]`
/// 与 `key:` 后跟 `- 项` 的块列表。认不出的一律忽略，绝不因为格式怪就整篇失败。
fn parse_front_matter(front: &str) -> FrontMatter {
    let mut out = FrontMatter::default();
    let mut last_key: Option<String> = None;
    for line in front.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            if let Some(key) = &last_key {
                out.lists
                    .entry(key.clone())
                    .or_default()
                    .push(unquote(item.trim()));
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();
        if value.is_empty() {
            // 后面可能跟块列表
            last_key = Some(key);
            continue;
        }
        if value.starts_with('[') && value.ends_with(']') {
            let items = split_flow_sequence(&value[1..value.len() - 1]);
            out.lists.insert(key.clone(), items);
            last_key = Some(key);
            continue;
        }
        out.scalars.insert(key.clone(), unquote(value));
        last_key = Some(key);
    }
    out
}

/// 流式列表拆分：逗号分隔，但要跳过引号里的逗号。
fn split_flow_sequence(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match quote {
            Some(open) if ch == open => {
                quote = None;
                current.push(ch);
            }
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            None if ch == ',' => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    items.push(unquote(trimmed));
                }
                current.clear();
            }
            None => current.push(ch),
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        items.push(unquote(trimmed));
    }
    items
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                match ch {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => out.push(other),
                }
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            out.push(ch);
        }
        return out;
    }
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        return value[1..value.len() - 1].replace("''", "'");
    }
    value.to_string()
}

fn normalize_date(raw: &str) -> String {
    let trimmed = raw.trim();
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() == 8 {
        // 过一遍真实日历：「2026-13-45」这种手改出来的值不能当日期用
        return valid_date_from_digits(&digits).unwrap_or_default();
    }
    if trimmed.len() == 10 && trimmed.as_bytes()[4] == b'-' && trimmed.as_bytes()[7] == b'-' {
        let compact: String = trimmed.chars().filter(|ch| *ch != '-').collect();
        return valid_date_from_digits(&compact).unwrap_or_default();
    }
    String::new()
}

/// 文件名 `20260908_1_标题` 或路径 `2026/09/...` 里兜底捞日期。
fn date_from_path(path: &str) -> Option<String> {
    let stem = file_stem(path);
    let leading: String = stem.chars().take(8).collect();
    if let Some(date) = valid_date_from_digits(&leading) {
        return Some(date);
    }
    // 退一步：目录层级 年/月 + 文件名开头两位当日
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    if parts.len() < 3 {
        return None;
    }
    let year = parts[parts.len() - 3];
    let month = parts[parts.len() - 2];
    let day: String = stem.chars().take(2).collect();
    if year.len() != 4 || !is_digits(year) || !is_digits(month) || month.is_empty() || month.len() > 2 || day.len() != 2 || !is_digits(&day)
    {
        return None;
    }
    valid_date_from_digits(&format!("{year}{month:0>2}{day}"))
}

fn is_digits(raw: &str) -> bool {
    !raw.is_empty() && raw.chars().all(|ch| ch.is_ascii_digit())
}

/// `YYYYMMDD` → `YYYY-MM-DD`，并且真的过一遍日历（2 月 30 日这种要拒掉）。
fn valid_date_from_digits(digits: &str) -> Option<String> {
    if digits.len() != 8 || !is_digits(digits) {
        return None;
    }
    let candidate = format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
    chrono::NaiveDate::parse_from_str(&candidate, "%Y-%m-%d")
        .ok()
        .map(|_| candidate)
}

/// 用 front-matter 的 date + time 拼一个本地时间戳（导入时保留原来的写作时刻）。
fn compose_timestamp(date: &str, time: &str) -> Option<String> {
    if date.is_empty() {
        return None;
    }
    let (hour, minute) = match time.split_once(':') {
        Some((h, m)) => (h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?),
        None => (0, 0),
    };
    let naive = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(hour, minute, 0)?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|parsed| parsed.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Tag, TagColor};

    fn entry(id: &str, date: &str, title: &str, markdown: &str, created: &str) -> DiaryEntry {
        DiaryEntry {
            id: id.to_string(),
            date: date.to_string(),
            title: title.to_string(),
            markdown: markdown.to_string(),
            mood: String::new(),
            weather: String::new(),
            tags: Vec::new(),
            expanded: None,
            created_at: created.to_string(),
            updated_at: Some(created.to_string()),
            extra: serde_json::Map::new(),
        }
    }

    fn names_of(bytes: &[u8]) -> Vec<String> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn file_naming_follows_the_day_sequence_and_title_rules() {
        // 同一天两篇 → 带序号；单独一天一篇 → 不带序号；没标题 → 只有日期
        let a = entry("d1", "2026-09-08", "早上的想法", "正文一", "2026-09-08T01:00:00+08:00");
        let b = entry("d2", "2026-09-08", "", "正文二", "2026-09-08T09:00:00+08:00");
        let c = entry("d3", "2026-01-05", "跨年", "正文三", "2026-01-05T20:00:00+08:00");
        let mut names = names_of(&build_zip(&[&a, &b, &c], &|_| None).unwrap());
        names.sort();
        assert_eq!(
            names,
            vec![
                "2026/01/20260105_跨年.md",
                "2026/09/20260908_1_早上的想法.md",
                "2026/09/20260908_2.md",
            ]
        );
    }

    #[test]
    fn titles_are_made_filename_safe_and_collisions_get_an_id_suffix() {
        let nasty = entry(
            "d1",
            "2026-09-08",
            "../../etc/passwd: 谁写/的\\标题?|<>*",
            "正文",
            "2026-09-08T01:00:00+08:00",
        );
        let names = names_of(&build_zip(&[&nasty], &|_| None).unwrap());
        assert_eq!(names.len(), 1);
        let name = &names[0];
        assert!(name.starts_with("2026/09/20260908_"), "{name}");
        for banned in ['/', '\\', ':', '?', '*', '<', '>', '|', '"'] {
            assert!(
                !name[8..].contains(banned),
                "文件名部分不能带 {banned:?}（路径分隔符与 Windows 保留字符）：{name}"
            );
        }
        let stem = name.rsplit('/').next().unwrap();
        assert!(!stem.starts_with('.'), "不能是隐藏文件：{name}");
        // 只有一层 年/月/文件，绝不允许标题把层级撑开
        assert_eq!(name.matches('/').count(), 2, "{name}");

        // 两篇标题清洗后撞车 → 用 id 尾巴区分，谁也不覆盖谁
        let x = entry("diary-aaaaaa111111", "2026-09-08", "???", "一", "2026-09-08T01:00:00+08:00");
        let y = entry("diary-bbbbbb222222", "2026-09-08", "???", "二", "2026-09-08T02:00:00+08:00");
        let names = names_of(&build_zip(&[&x, &y], &|_| None).unwrap());
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1], "撞名必须被区分开：{names:?}");
    }

    #[test]
    fn front_matter_round_trips_every_field() {
        let mut source = entry(
            "d1",
            "2026-09-08",
            "带\"引号\"和: 冒号的标题",
            "第一行\n\n第二段的正文。",
            "2026-09-08T14:30:00+08:00",
        );
        source.mood = "🙂".to_string();
        source.weather = "☀️".to_string();
        source.tags = vec![
            Tag {
                id: "tag-1".to_string(),
                color: TagColor::Blue,
                text: Some("工作".to_string()),
                extra: serde_json::Map::new(),
            },
            Tag {
                id: "tag-2".to_string(),
                color: TagColor::Green,
                text: None,
                extra: serde_json::Map::new(),
            },
        ];

        let text = render_markdown(&source);
        assert!(text.starts_with("---\n"), "{text}");
        assert!(text.contains("date: 2026-09-08\n"));
        assert!(text.contains("mood: \"🙂\""));
        assert!(text.contains("tags: [\"blue:工作\", \"green\"]"));
        // 标题里的引号与冒号必须被转义，否则 YAML 就断了
        assert!(text.contains(r#"title: "带\"引号\"和: 冒号的标题""#), "{text}");

        let parsed = parse_markdown(&text, "2026/09/20260908_x.md");
        assert_eq!(parsed["date"], "2026-09-08");
        assert_eq!(parsed["title"], "带\"引号\"和: 冒号的标题");
        assert_eq!(parsed["mood"], "🙂");
        assert_eq!(parsed["weather"], "☀️");
        assert_eq!(parsed["markdown"], "第一行\n\n第二段的正文。");
        let tags = parsed["tags"].as_array().unwrap();
        assert_eq!(tags[0], "blue:工作");
        assert_eq!(tags[1], "green");
        assert_eq!(parsed["createdAt"], "2026-09-08T14:30:00+08:00");
    }

    #[test]
    fn a_whole_archive_round_trips_through_zip() {
        let a = entry("d1", "2026-09-08", "早", "早上的正文", "2026-09-08T08:00:00+08:00");
        let b = entry("d2", "2026-09-08", "晚", "晚上的正文", "2026-09-08T22:00:00+08:00");
        let bytes = build_zip(&[&a, &b], &|_| None).unwrap();
        let parsed = parse_zip(&bytes).unwrap().entries;
        assert_eq!(parsed.len(), 2);
        let titles: Vec<&str> = parsed.iter().map(|p| p["title"].as_str().unwrap()).collect();
        assert!(titles.contains(&"早") && titles.contains(&"晚"));
        assert!(parsed.iter().all(|p| p["date"] == "2026-09-08"));
    }

    #[test]
    fn import_tolerates_hand_written_markdown_and_junk() {
        // 用户自己写的 md：没有 front-matter，日期只能从目录与文件名兜底
        let handwritten = "# 手写的一篇\n\n正文内容。\n";
        let (front, body) = split_front_matter(handwritten);
        assert_eq!(front, "");
        assert!(body.starts_with("# 手写的一篇"));
        let parsed = parse_markdown(handwritten, "kxtodo-diary/2026/09/20260908_手写.md");
        assert_eq!(parsed["date"], "2026-09-08");
        assert_eq!(parsed["title"], "");
        assert!(parsed["markdown"].as_str().unwrap().contains("正文内容"));

        // 日期在文件名里缺失时退回 年/月 + 文件名开头两位
        assert_eq!(
            date_from_path("2026/09/08-something.md").as_deref(),
            Some("2026-09-08")
        );
        // 2 月 30 日这种不存在的日期必须被拒
        assert_eq!(date_from_path("2026/02/30-nope.md"), None);
        assert_eq!(normalize_date("2026-13-45"), "");
    }

    #[test]
    fn non_utf8_and_non_markdown_entries_are_skipped_not_fatal() {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("2026/09/20260908.md", options).unwrap();
        writer.write_all("---\ndate: 2026-09-08\n---\n能读的一篇\n".as_bytes()).unwrap();
        writer.start_file("2026/09/readme.txt", options).unwrap();
        writer.write_all("也算文本，应该被收进来".as_bytes()).unwrap();
        writer.start_file("2026/09/photo.png", options).unwrap();
        writer.write_all(&[0x89, 0x50, 0x4e, 0x47, 0xff, 0xfe]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let parsed = parse_zip(&bytes).unwrap().entries;
        // .md 与 .txt 收进来（用户可能把日记存成 txt），二进制图片跳过而不是整体报错
        assert_eq!(parsed.len(), 2, "{parsed:?}");
        assert!(parsed.iter().any(|p| p["markdown"].as_str().unwrap().contains("能读的一篇")));
    }

    #[test]
    fn images_round_trip_through_the_archive() {
        let png: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
        let with_image = entry(
            "d1",
            "2026-09-08",
            "带图的一篇",
            "看图：\n\n![截图](md-111.png)\n\n![外链](https://example.com/a.png)\n\n![丢了](md-missing.png)",
            "2026-09-08T08:00:00+08:00",
        );
        let store = [("md-111.png".to_string(), png.clone())].into_iter().collect::<BTreeMap<_, _>>();
        let bytes = build_zip(&[&with_image], &|name| store.get(name).cloned()).unwrap();

        let names = names_of(&bytes);
        assert!(names.contains(&"images/md-111.png".to_string()), "{names:?}");

        let parsed = parse_zip(&bytes).unwrap();
        assert_eq!(parsed.images.get("md-111.png"), Some(&png));
        let markdown = parsed.entries[0]["markdown"].as_str().unwrap();
        // 包内有图：引用归一回裸文件名（KXToDo 内部约定）
        assert!(markdown.contains("![截图](md-111.png)"), "{markdown}");
        // 远程链接永远不碰
        assert!(markdown.contains("![外链](https://example.com/a.png)"), "{markdown}");
        // 包里没有的图：原样保留，不当坏引用处理
        assert!(markdown.contains("![丢了](md-missing.png)"), "{markdown}");
    }

    #[test]
    fn exported_markdown_points_at_the_in_archive_image_path() {
        let png: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 9];
        let with_image = entry("d1", "2026-09-08", "图", "![截图](md-111.png)", "2026-09-08T08:00:00+08:00");
        let store = [("md-111.png".to_string(), png)].into_iter().collect::<BTreeMap<_, _>>();
        let bytes = build_zip(&[&with_image], &|name| store.get(name).cloned()).unwrap();

        // 解压出来 年/月/x.md 与 images/ 同级，相对路径要能直接显示
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        archive
            .by_name("2026/09/20260908_图.md")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("![截图](images/md-111.png)"), "{text}");
    }
}

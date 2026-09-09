//! 一般卡片条目的 Markdown 压缩包（v0.6.10）：包根下一堆 `<日期>_<正文前10字>.md`
//! + `images/<文件名>`。导出把卡片正文原样写进 md（本地图引用改写成包内相对路径），
//! 导入把每个 md 还原成一张卡片（引用归一回裸文件名，包里的图一并落盘）。
//!
//! 与日记压缩包同一条哲学：格式是给人看的，解压出来任何编辑器都能读；解析尽量宽容，
//! 护栏（体积/文件数/文件名穿越）与日记共用同一套口径。

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::diary_archive::{
    is_image_name, is_markdown, is_safe_image_name, ref_basename, rewrite_image_refs,
    sanitize_for_filename, IMAGES_DIR,
};
use crate::error::{CoreError, CoreResult};
use crate::model::Item;

/// 导入护栏：与日记压缩包同一套口径（压缩包炸弹不该把内存吃光）
const MAX_ARCHIVE_BYTES: usize = 200 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// 文件名里取正文的前几个字符（用户口径：日期 + 正文前 10 个字符）
const BODY_CHARS_IN_NAME: usize = 10;

/// 压缩包默认名（带条目名，免得下载目录里一堆同名文件分不清）。
pub fn archive_name(node_name: &str) -> String {
    let stem = sanitize_for_filename(node_name);
    if stem.is_empty() {
        "kxtodo-cards.zip".to_string()
    } else {
        format!("kxtodo-cards-{stem}.zip")
    }
}

/// 构建压缩包字节。`tasks` 按导出顺序（阅读顺序）传入；序号/撞名在包内解决。
///
/// `read_image` 按文件名取条目插图字节（调用方从 `img/data/<nodeId>/` 读）：
/// 取得到的随包带走、正文引用改写成 `images/<文件名>`，取不到的引用原样保留。
pub fn build_zip(
    tasks: &[&Item],
    read_image: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> CoreResult<Vec<u8>> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut used_paths: HashSet<String> = HashSet::new();
    let mut images: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    for task in tasks {
        let path = card_path(task, &mut used_paths);
        let body = rewrite_image_refs(task.markdown.trim_end(), |src| {
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

/// 一张卡片的包内路径：`<YYYYMMDD>_<正文前10字>.md`，撞名加序号。
fn card_path(task: &Item, used: &mut HashSet<String>) -> String {
    let compact = task
        .created_at
        .get(0..10)
        .unwrap_or("0000-00-00")
        .replace('-', "");
    let head: String = task
        .markdown
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(BODY_CHARS_IN_NAME)
        .collect();
    let sanitized = sanitize_for_filename(&head);
    let stem = if sanitized.is_empty() {
        compact.clone()
    } else {
        format!("{compact}_{sanitized}")
    };
    let mut path = format!("{stem}.md");
    let mut index = 2;
    while !used.insert(path.clone()) {
        path = format!("{stem}_{index}.md");
        index += 1;
    }
    path
}

/// 解析结果：每张卡片一段正文（引用已归一回裸文件名）+ 包内插图字节。
#[derive(Default, Debug)]
pub struct ParsedCards {
    pub cards: Vec<String>,
    pub images: BTreeMap<String, Vec<u8>>,
}

/// 解析压缩包。**不落盘、不解压到文件系统**，全部在内存里读，所以没有 zip-slip 一说。
pub fn parse_zip(bytes: &[u8]) -> CoreResult<ParsedCards> {
    if bytes.is_empty() {
        return Err(CoreError::validation("CARDS_IMPORT_INVALID", "压缩包是空的"));
    }
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(CoreError::validation(
            "CARDS_IMPORT_TOO_LARGE",
            format!("压缩包超过 {}MB 上限", MAX_ARCHIVE_BYTES / 1024 / 1024),
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        CoreError::validation(
            "CARDS_IMPORT_INVALID",
            format!("不是有效的 zip 压缩包：{error}"),
        )
    })?;
    if archive.len() > MAX_FILES {
        return Err(CoreError::validation(
            "CARDS_IMPORT_TOO_LARGE",
            format!("压缩包内文件数超过 {MAX_FILES} 上限"),
        ));
    }

    let mut out = ParsedCards::default();
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
                "CARDS_IMPORT_TOO_LARGE",
                "解压后体积超过上限（疑似压缩包炸弹）",
            ));
        }
        let base = ref_basename(&name).to_string();
        if is_safe_image_name(&base) {
            out.images.entry(base).or_insert(raw);
        }
    }

    // 第二遍读 md，一个文件一张卡片；包里有对应插图才把引用归一回裸文件名
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
                "CARDS_IMPORT_TOO_LARGE",
                "解压后体积超过上限（疑似压缩包炸弹）",
            ));
        }
        // 用户可能在别的编辑器里存成 GBK：lossy 解码保住能读的部分，别整篇丢掉
        let text = String::from_utf8_lossy(&raw);
        let markdown = rewrite_image_refs(text.trim(), |src| {
            let base = ref_basename(src);
            // 包里有这张图才归一回裸文件名；没有就原样保留（网络图片不必理会）
            out.images.contains_key(base).then(|| base.to_string())
        });
        if !markdown.trim().is_empty() {
            out.cards.push(markdown);
        }
    }
    Ok(out)
}

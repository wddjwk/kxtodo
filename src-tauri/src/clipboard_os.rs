//! 桌面系统剪切板的读写（v0.8.9 剪切板同步）。
//!
//! - 文本 / HTML / 图片：arboard；Windows 的 CF_HDROP 文件列表由 clipboard-win 补齐。
//! - Linux 文件列表：很多文件管理器只提供 `text/uri-list`，arboard 不暴露任意格式；
//!   这里从文本里识别「整段都是 file:// 行」的情况，写回时也退化为 URI 列表。
//! - 图片统一转成 PNG 在网络上传输，避免 RGBA 原始字节跨端解释不一致。
//! - 读剪切板失败（被别的进程占用）是常态：返回 None，不把轮询线程弄崩。

use std::path::PathBuf;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use kxtodo_core::clipboard_sync::{ClipboardContent, ClipboardFile};

/// 收到文件的默认保存位置：下载目录下的 kxtodo-clipboard。
/// Android 的 download_dir() 是外部文件目录（不需要权限、文件管理器可见），
/// 与传输助手同一个先例。
pub fn default_clipboard_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .download_dir()
        .ok()
        .or_else(|| app.path().app_data_dir().ok())
        .map(|dir| dir.join("kxtodo-clipboard"))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn png_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use image::{ImageBuffer, ImageFormat, Rgba};
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "剪切板图片尺寸不合法".to_string())?;
    let mut out = std::io::Cursor::new(Vec::new());
    buffer
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|error| format!("剪切板图片编码失败：{error}"))?;
    Ok(out.into_inner())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn rgba_from_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    use image::ImageFormat;
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|error| format!("剪切板图片解码失败：{error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn available() -> bool {
    true
}

#[cfg(windows)]
static LAST_CLIPBOARD_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 新的轮询线程启动时清掉序列号缓存：否则服务重启后的第一次 read 会因「序号没变」
/// 返回 None，poller 一直没建基线，用户下一次复制会被当成首次读取而吞掉。
#[cfg(windows)]
pub fn reset_read_cache() {
    LAST_CLIPBOARD_SEQ.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(windows))]
pub fn reset_read_cache() {}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn read() -> Result<Option<ClipboardContent>, String> {
    #[cfg(windows)]
    {
        use std::sync::atomic::Ordering;
        let seq = clipboard_win::seq_num().map(|value| value.get());
        if let Some(seq) = seq {
            if LAST_CLIPBOARD_SEQ.load(Ordering::Relaxed) == seq {
                return Ok(None);
            }
            LAST_CLIPBOARD_SEQ.store(seq, Ordering::Relaxed);
        }
    }
    // 文件列表优先：资源管理器复制文件时同时带 CF_HDROP 与文本，文本只是路径串。
    if let Some(files) = read_file_list() {
        if !files.is_empty() {
            return Ok(Some(ClipboardContent {
                files: files
                    .into_iter()
                    .map(|path| ClipboardFile {
                        name: path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".to_string()),
                        path: Some(path),
                        bytes: None,
                    })
                    .collect(),
                ..Default::default()
            }));
        }
    }
    let text = {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard.get_text().unwrap_or_default()
    };
    let html = read_html();
    #[cfg(not(windows))]
    if html.is_empty() && !text.is_empty() {
        let files = files_from_uri_text(&text);
        if !files.is_empty() {
            return Ok(Some(ClipboardContent {
                files: files
                    .into_iter()
                    .map(|path| ClipboardFile {
                        name: path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".to_string()),
                        path: Some(path),
                        bytes: None,
                    })
                    .collect(),
                ..Default::default()
            }));
        }
    }
    // 浏览器「复制图片」经常同时给 CF_BITMAP + HTML/文本。三种都带上，
    // 接收端按「文件 > 图片 > HTML > 文本」选最丰富的一档。
    // Windows 用序列号跳过了未变化的轮询；Linux 没有序列号，文本/HTML 已有时
    // 不再每 800ms 读一次图片（大图重编码 PNG 很贵），图片单独复制时文本为空，照读。
    let read_image = cfg!(windows) || (text.is_empty() && html.is_empty());
    let image = if read_image {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard.get_image().ok().filter(|image| image.bytes.len() <= 256 * 1024 * 1024)
    } else {
        None
    };
    let image_png = match image {
        Some(image) => {
            // 解码/编码失败不该把整条文本也丢掉：图片只是其中一种表示。
            png_from_rgba(image.width as u32, image.height as u32, &image.bytes).ok()
        }
        None => None,
    };
    if !text.is_empty() || !html.is_empty() || image_png.is_some() {
        return Ok(Some(ClipboardContent { text, html, image: image_png, ..Default::default() }));
    }
    Ok(None)
}

/// arboard 只能写 HTML、不能读（3.7 的 API 里没有 get_html）。Windows 的
/// CF_HTML 由 clipboard-win 解析成片段；其它平台本期只有纯文本兜底。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn read_html() -> String {
    #[cfg(windows)]
    {
        use clipboard_win::formats;
        let Some(html) = formats::Html::new() else { return String::new() };
        return clipboard_win::get_clipboard::<String, _>(html).unwrap_or_default();
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}


#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn write(content: &ClipboardContent) -> Result<(), String> {
    if !content.files.is_empty() {
        let paths: Vec<PathBuf> = content
            .files
            .iter()
            .filter_map(|file| file.path.clone())
            .filter(|path| path.exists())
            .collect();
        if !paths.is_empty() {
            return write_file_list(&paths);
        }
    }
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    if let Some(image) = &content.image {
        let (width, height, rgba) = rgba_from_png(image)?;
        clipboard
            .set_image(arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: rgba.into(),
            })
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    if !content.html.is_empty() {
        clipboard
            .set_html(
                content.html.as_str(),
                if content.text.is_empty() { None } else { Some(content.text.as_str()) },
            )
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    if !content.text.is_empty() {
        clipboard
            .set_text(&content.text)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}


#[cfg(windows)]
fn read_file_list() -> Option<Vec<PathBuf>> {
    use clipboard_win::formats;
    let paths = clipboard_win::get_clipboard::<Vec<PathBuf>, _>(formats::FileList).ok()?;
    Some(paths.into_iter().filter(|path| path.is_file()).collect())
}

#[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
fn read_file_list() -> Option<Vec<PathBuf>> {
    // 非 Windows：不复用 read() 里的 arboard 实例（那会二次打开），
    // 由 write/read 的文本分支识别 URI 列表更稳；这里返回 None。
    None
}

#[cfg(windows)]
fn write_file_list(paths: &[PathBuf]) -> Result<(), String> {
    use clipboard_win::formats;
    use clipboard_win::Setter;
    let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().to_string()).collect();
    let _clip = clipboard_win::Clipboard::new_attempts(10)
        .map_err(|error| format!("打开剪切板失败：{error}"))?;
    formats::FileList
        .write_clipboard(paths.as_slice())
        .map_err(|error| format!("写入文件剪切板失败：{error}"))
}

#[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
fn write_file_list(paths: &[PathBuf]) -> Result<(), String> {
    // Linux 的文件管理器普遍认 text/uri-list；arboard 只暴露文本，所以写回 URI 列表。
    // 这不是原生文件剪切板，但比拒绝接收好：多数桌面环境能从文本 URI 重建文件。
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let uris: Vec<String> = paths
        .iter()
        .map(|path| format!("file://{}", path.to_string_lossy().replace(' ', "%20")))
        .collect();
    clipboard
        .set_text(&uris.join("\n"))
        .map_err(|error| error.to_string())
}

#[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
pub fn files_from_uri_text(text: &str) -> Vec<PathBuf> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    if lines.is_empty() || !lines.iter().all(|line| line.starts_with("file://")) {
        return Vec::new();
    }
    lines
        .iter()
        .filter_map(|line| {
            let raw = line.trim_start_matches("file://");
            let raw = raw.strip_prefix("localhost").unwrap_or(raw);
            let decoded = percent_encoding::percent_decode_str(raw).decode_utf8().ok()?;
            let path = PathBuf::from(decoded.as_ref());
            path.exists().then_some(path)
        })
        .collect()
}

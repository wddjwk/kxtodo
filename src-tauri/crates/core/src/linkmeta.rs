//! 网页链接元数据（v0.7.7 的超链接自动标题与预览卡片）。
//!
//! 只认 http(s)，抓页面开头 256KB（`<head>` 一定在前部）解析标题/摘要/站点名。
//! 手写小解析器而不是引 HTML 解析库：就取几个标签，几百行正则式扫描足够，
//! 包体积与风格都不值。中文站一半是 GBK——先看 content-type，再看
//! `<meta charset>`，用 encoding_rs 正确解码（乱码的标题比没有标题更糟）。
//!
//! 结果是**纯缓存**：进程内 HashMap + `runtime/linkmeta.json`（丢了重抓即可，
//! 不进同步、不参与任何数据语义）。失败一律 Err，由调用方决定退回原样链接。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{CoreError, CoreResult};

/// 只读这么多字节：`<head>` 一定在前部，够解析标题与摘要
const MAX_BYTES: u64 = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) KXToDo/1.0";
/// 缓存落盘文件名（runtime/ 下）
const CACHE_FILE: &str = "linkmeta.json";
/// 缓存条数上限：超了淘汰一半（纯缓存，不承诺 LRU）。
const CACHE_CAP: usize = 500;
/// v3 统一 HTTP/渲染结果的 URL、文本与图标安全边界，旧缓存重新抓取。
const CACHE_VERSION: u32 = 3;
pub const META_MAX_BYTES: usize = 16 * 1024;
const URL_MAX: usize = 4096;
const TITLE_MAX: usize = 200;
const DESC_MAX: usize = 400;

/// 一个链接的元数据。`title` / `description` / `icon` 都可能为空串（页面没写）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LinkMeta {
    /// 规范化后的链接（去掉 fragment）
    pub url: String,
    /// 站点名：og:site_name 优先，退回主机名（去掉 www.）
    pub site: String,
    pub title: String,
    pub description: String,
    /// 网页自己的图标（`<link rel="icon">`；没声明就试 `<origin>/favicon.ico`）。
    /// 前端直接当 `<img src>` 用，取不到就回退默认的链接图标。
    #[serde(default)]
    pub icon: String,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    version: u32,
    entries: HashMap<String, LinkMeta>,
}

struct CacheState {
    dir: Option<PathBuf>,
    entries: HashMap<String, LinkMeta>,
}

fn cache() -> &'static Mutex<CacheState> {
    static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(CacheState {
            dir: None,
            entries: HashMap::new(),
        })
    })
}

fn load_cache(state: &mut CacheState, dir: &Path) {
    if state.dir.as_deref() == Some(dir) {
        return;
    }
    state.dir = Some(dir.to_path_buf());
    state.entries.clear();
    let Ok(raw) = std::fs::read_to_string(dir.join(CACHE_FILE)) else {
        return;
    };
    if let Ok(file) = serde_json::from_str::<CacheFile>(&raw) {
        if file.version == CACHE_VERSION {
            for (key, meta) in file.entries.into_iter().take(CACHE_CAP) {
                if let Ok(meta) = normalize_metadata(&key, meta) {
                    state.entries.insert(meta.url.clone(), meta);
                }
            }
        }
    }
}

fn save_cache(state: &CacheState) {
    let Some(dir) = &state.dir else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(raw) = serde_json::to_string(&CacheFile {
        version: CACHE_VERSION,
        entries: state.entries.clone(),
    }) else {
        return;
    };
    // 原子写：临时文件 + rename，读侧永远看到完整 JSON
    let target = dir.join(CACHE_FILE);
    let temp = dir.join(format!("{CACHE_FILE}.tmp"));
    if std::fs::write(&temp, raw).is_ok() {
        let _ = std::fs::rename(&temp, &target);
    }
}

/// Shared by HTTP, cache writes and the native webviews. Keep fragments for navigation.
pub fn external_url(raw: &str) -> CoreResult<Url> {
    let invalid = || CoreError::validation("LINK_URL_INVALID", "只支持外部 http/https 链接");
    let raw = raw.trim();
    if raw.len() > URL_MAX || raw.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err(invalid());
    }
    let url = Url::parse(raw).map_err(|_| invalid())?;
    let host = url.host_str().unwrap_or("").trim_end_matches('.');
    if !matches!(url.scheme(), "http" | "https")
        || host.is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || host == "localhost"
        || host.ends_with(".localhost")
        || matches!(host, "[::1]" | "[::]")
        || host.starts_with("127.")
        || host.starts_with("0.")
        || url.as_str().len() > URL_MAX
    {
        return Err(invalid());
    }
    Ok(url)
}

pub fn normalize_url(raw: &str) -> CoreResult<String> {
    let mut url = external_url(raw)?;
    url.set_fragment(None);
    Ok(url.to_string())
}

fn host_of(url: &Url) -> String {
    url.host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string()
}

/// Text from a rendered DOM is already entity-decoded; never decode it twice.
fn bounded_text(raw: &str, max: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    truncate(
        &cleaned.split_whitespace().collect::<Vec<_>>().join(" "),
        max,
    )
}

pub fn normalize_metadata(request_url: &str, mut meta: LinkMeta) -> CoreResult<LinkMeta> {
    let key = normalize_url(request_url)?;
    if [
        &meta.url,
        &meta.site,
        &meta.title,
        &meta.description,
        &meta.icon,
    ]
    .iter()
    .map(|s| s.len())
    .sum::<usize>()
        > META_MAX_BYTES
    {
        return Err(CoreError::validation(
            "LINK_META_TOO_LARGE",
            "链接元数据过大",
        ));
    }
    // Only the explicit request identifies the cache entry. A remote page cannot write another key.
    let page = external_url(&meta.url).unwrap_or(external_url(&key)?);
    meta.url = key;
    meta.site = bounded_text(&meta.site, 100);
    if meta.site.is_empty() {
        meta.site = host_of(&external_url(&meta.url)?);
    }
    meta.title = bounded_text(&meta.title, TITLE_MAX);
    meta.description = bounded_text(&meta.description, DESC_MAX);
    let challenge = meta.title.to_ascii_lowercase();
    if (meta.title.is_empty() && meta.description.is_empty())
        || matches!(
            challenge.trim_end_matches(['.', '…', '!']),
            "just a moment" | "checking your browser" | "verify you are human" | "loading"
        )
        || challenge == "attention required! | cloudflare"
    {
        return Err(CoreError::execution(
            "LINK_META_EMPTY",
            "页面没有可用的标题或摘要（可能需要验证）",
        ));
    }
    meta.icon = if meta.icon.trim().is_empty() {
        page.join("/favicon.ico")
            .map(|u| u.to_string())
            .unwrap_or_default()
    } else {
        page.join(meta.icon.trim())
            .ok()
            .and_then(|u| external_url(u.as_str()).ok())
            .map(|u| u.to_string())
            .unwrap_or_default()
    };
    Ok(meta)
}

fn insert_cache(state: &mut CacheState, meta: LinkMeta) {
    if !state.entries.contains_key(&meta.url) && state.entries.len() >= CACHE_CAP {
        let drop_keys: Vec<String> = state.entries.keys().take(CACHE_CAP / 2).cloned().collect();
        for key in drop_keys {
            state.entries.remove(&key);
        }
    }
    state.entries.insert(meta.url.clone(), meta);
}

/// Both HTTP and rendered-page writes use exactly the same validation, key and eviction path.
pub fn put_link_meta(
    runtime_dir: &Path,
    request_url: &str,
    meta: LinkMeta,
) -> CoreResult<LinkMeta> {
    let meta = normalize_metadata(request_url, meta)?;
    let mut state = cache().lock().unwrap_or_else(|poison| poison.into_inner());
    load_cache(&mut state, runtime_dir);
    insert_cache(&mut state, meta.clone());
    save_cache(&state);
    Ok(meta)
}

/// 取一个链接的元数据（命中缓存直接回）。`runtime_dir` 是缓存落盘位置。
pub fn link_meta(runtime_dir: &Path, url: &str) -> CoreResult<LinkMeta> {
    let key = normalize_url(url)?;
    {
        let mut state = cache().lock().unwrap_or_else(|poison| poison.into_inner());
        load_cache(&mut state, runtime_dir);
        if let Some(hit) = state.entries.get(&key) {
            return Ok(hit.clone());
        }
    }
    put_link_meta(runtime_dir, &key, fetch(&external_url(&key)?)?)
}

fn fetch(url: &Url) -> CoreResult<LinkMeta> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .timeout_write(READ_TIMEOUT)
        .timeout(READ_TIMEOUT)
        .redirects(0)
        .user_agent(USER_AGENT)
        .build();
    let mut page = url.clone();
    let mut redirects = 0;
    let response = loop {
        let response = agent.get(page.as_str()).call().map_err(|error| {
            CoreError::execution("LINK_FETCH_FAILED", format!("抓取失败：{error}"))
        })?;
        if !(300..400).contains(&response.status()) {
            break response;
        }
        if redirects >= 5 {
            return Err(CoreError::execution("LINK_FETCH_FAILED", "页面重定向过多"));
        }
        let next = response
            .header("location")
            .and_then(|s| page.join(s).ok())
            .ok_or_else(|| CoreError::execution("LINK_FETCH_FAILED", "无效的重定向"))?;
        page = external_url(next.as_str())?;
        redirects += 1;
    };
    let content_type = response
        .header("content-type")
        .unwrap_or("")
        .to_ascii_lowercase();
    // 图片 / PDF / 二进制抓来也没有标题可读，直接当失败
    if !content_type.is_empty() && !content_type.contains("html") && !content_type.contains("text/")
    {
        return Err(CoreError::execution(
            "LINK_FETCH_FAILED",
            format!("不是网页（{content_type}）"),
        ));
    }

    let mut buffer = Vec::with_capacity(64 * 1024);
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut buffer)
        .map_err(|error| CoreError::execution("LINK_FETCH_FAILED", format!("读取失败：{error}")))?;

    let charset = sniff_charset(&buffer, &content_type);
    let encoding =
        encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (html, _, _) = encoding.decode(&buffer);
    let html = html.into_owned();

    let title = extract_meta_content(&html, &["og:title", "twitter:title"])
        .or_else(|| extract_title(&html))
        .unwrap_or_default();
    let description = extract_meta_content(
        &html,
        &["og:description", "description", "twitter:description"],
    )
    .unwrap_or_default();
    let site = extract_meta_content(&html, &["og:site_name"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| host_of(&page));
    let icon = extract_icon(&html, &page);

    Ok(LinkMeta {
        url: page.to_string(),
        site: truncate(&site, 100),
        title: truncate(&title, TITLE_MAX),
        description: truncate(&description, DESC_MAX),
        icon,
    })
}

/// 编码嗅探：content-type 里的 charset 优先，其次前 2KB 里的 `<meta charset>`，
/// 都没有就按 UTF-8 解（lossy，解不出来也有个能看的近似）。
fn sniff_charset(buffer: &[u8], content_type: &str) -> String {
    if let Some(index) = content_type.find("charset=") {
        let value = &content_type[index + "charset=".len()..];
        let value = value
            .split([';', ' '])
            .next()
            .unwrap_or("")
            .trim_matches('"');
        if !value.is_empty() {
            return value.to_string();
        }
    }
    let head = String::from_utf8_lossy(&buffer[..buffer.len().min(2048)]).to_ascii_lowercase();
    if let Some(index) = head.find("charset=") {
        let value = &head[index + "charset=".len()..];
        let value = value
            .trim_start_matches(['"', '\''])
            .split(['"', '\'', ';', ' ', '/', '>'])
            .next()
            .unwrap_or("");
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "utf-8".to_string()
}

/// 网页图标：`<link rel="icon" href="…">`（含 shortcut icon / apple-touch-icon），
/// 相对地址按页面 URL 解成绝对地址；页面没声明就试同源的 `/favicon.ico`
/// （绝大多数站点都有，加载不出来由前端回退默认的链接图标）。
fn extract_icon(html: &str, page: &Url) -> String {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    let mut fallback = String::new();
    while let Some(offset) = lower[cursor..].find("<link") {
        let start = cursor + offset;
        let Some(tag_end) = lower[start..].find('>').map(|value| start + value) else {
            break;
        };
        cursor = tag_end + 1;
        let attrs = parse_attrs(&html[start..tag_end]);
        let rel = attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if !rel
            .split_whitespace()
            .any(|token| token == "icon" || token == "apple-touch-icon")
        {
            continue;
        }
        let Some(href) = attrs.get("href").filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let Ok(resolved) = page.join(href.trim()) else {
            continue;
        };
        if !matches!(resolved.scheme(), "http" | "https") {
            continue;
        }
        let value = resolved.to_string();
        // 优先 rel 里带 icon 且不是 apple-touch 的那条（后者常是 180×180 的大图，
        // 卡片里的小圆点用不上），没有就先用 apple 的占位
        if rel.split_whitespace().any(|token| token == "icon") {
            return value;
        }
        if fallback.is_empty() {
            fallback = value;
        }
    }
    if !fallback.is_empty() {
        return fallback;
    }
    page.join("/favicon.ico")
        .map(|u| u.to_string())
        .unwrap_or_default()
}

/// `<title>…</title>`
fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start;
    let close = lower[open_end..].find("</title")? + open_end;
    let text = clean_text(&html[open_end + 1..close]);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// `<meta property|name="key" content="…">`：按顺序找第一个命中的 key。
fn extract_meta_content(html: &str, keys: &[&str]) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find("<meta") {
        let start = cursor + offset;
        let Some(tag_end) = lower[start..].find('>').map(|value| start + value) else {
            break;
        };
        let attrs = parse_attrs(&html[start..tag_end]);
        let name = attrs
            .get("property")
            .or_else(|| attrs.get("name"))
            .map(|value| value.to_ascii_lowercase());
        if let Some(name) = name {
            if keys.iter().any(|key| key.eq_ignore_ascii_case(&name)) {
                if let Some(content) = attrs.get("content") {
                    let text = clean_text(content);
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
        }
        cursor = tag_end + 1;
    }
    None
}

/// 迷你属性解析：`a=b`、`a="b"`、`a='b'` 都认，键统一小写。
/// 只服务于 `<meta>` 这一种标签，畸形输入跳过即可。
fn parse_attrs(tag: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        // 键
        while index < bytes.len() && !bytes[index].is_ascii_alphabetic() && bytes[index] != b'_' {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric()
                || bytes[index] == b'_'
                || bytes[index] == b'-'
                || bytes[index] == b':')
        {
            index += 1;
        }
        if key_start == index {
            break;
        }
        let key = tag[key_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let quote = bytes[index];
        let value;
        if quote == b'"' || quote == b'\'' {
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            value = tag[value_start..index].to_string();
            index += 1;
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            value = tag[value_start..index].to_string();
        }
        attrs.entry(key).or_insert(value);
    }
    attrs
}

/// 文本清洗：实体解码 + 空白折叠 + 去首尾空白。
fn clean_text(raw: &str) -> String {
    let decoded = decode_entities(raw);
    let mut out = String::with_capacity(decoded.len());
    let mut last_space = false;
    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// 常见实体 + 数字引用（`&#8211;`、`&#x2013;`）；认不出的原样保留。
fn decode_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        // 找实体结尾：按字符走，绝不劈开多字节字符（`tail[..12]` 这种字节切片会 panic）；
        // 撞上下一个 `&` 说明这不是实体，原样吐出去
        let mut end = None;
        for (offset, ch) in tail.char_indices().skip(1) {
            if offset > 24 {
                break;
            }
            if ch == ';' {
                end = Some(offset);
                break;
            }
            if ch == '&' {
                break;
            }
        }
        let Some(end) = end else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => {
                if let Some(number) = entity.strip_prefix('#') {
                    let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        number.parse::<u32>().ok()
                    };
                    code.and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 按字符截断（不劈开多字节字符），超长补省略号。
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_cache_keys_and_icons_are_untrusted() {
        let meta = normalize_metadata(
            "https://www.example.com:8443/a#section",
            LinkMeta {
                url: "https://victim.example/posts/final".into(),
                title: "  Page\n title  ".into(),
                icon: "../favicon.png".into(),
                ..LinkMeta::default()
            },
        )
        .unwrap();
        assert_eq!(meta.url, "https://www.example.com:8443/a");
        assert_eq!(meta.title, "Page title");
        assert_eq!(meta.site, "example.com");
        assert_eq!(meta.icon, "https://victim.example/favicon.png");
        for icon in [
            "javascript:alert(1)",
            "data:image/svg+xml,test",
            "file:///private",
            "http://asset.localhost/a",
        ] {
            let meta = normalize_metadata(
                "https://example.com",
                LinkMeta {
                    title: "Title".into(),
                    icon: icon.into(),
                    ..LinkMeta::default()
                },
            )
            .unwrap();
            assert!(meta.icon.is_empty());
        }
        assert_eq!(
            normalize_metadata(
                "https://example.com:8443/a",
                LinkMeta {
                    title: "Title".into(),
                    ..LinkMeta::default()
                }
            )
            .unwrap()
            .icon,
            "https://example.com:8443/favicon.ico"
        );
    }

    #[test]
    fn rejects_native_urls_oversized_payloads_and_challenges() {
        for url in [
            "file:///secret",
            "tauri://localhost",
            "http://tauri.localhost/",
            "http://localhost./",
            "http://127.0.0.1:1420",
            "http://2130706433/",
            "http://[::1]/",
            "https://u:p@example.com",
            "https://example.com/a\nb",
        ] {
            assert!(external_url(url).is_err(), "{url}");
        }
        assert_eq!(
            normalize_url("https://EXAMPLE.com:443/a#b").unwrap(),
            "https://example.com/a"
        );
        assert!(normalize_metadata("https://example.com", LinkMeta::default()).is_err());
        assert!(normalize_metadata(
            "https://example.com",
            LinkMeta {
                title: "Just a moment...".into(),
                ..LinkMeta::default()
            }
        )
        .is_err());
        assert!(normalize_metadata(
            "https://example.com",
            LinkMeta {
                title: "字".repeat(META_MAX_BYTES / 2),
                ..LinkMeta::default()
            }
        )
        .is_err());
        let meta = normalize_metadata(
            "https://example.com",
            LinkMeta {
                title: "𠮷".repeat(201),
                description: "&amp; <literal>".into(),
                ..LinkMeta::default()
            },
        )
        .unwrap();
        assert_eq!(meta.title, format!("{}…", "𠮷".repeat(200)));
        assert_eq!(meta.description, "&amp; <literal>");
    }

    #[test]
    fn cache_write_read_normalization_and_eviction_share_one_path() {
        let dir = tempfile::tempdir().unwrap();
        let result = put_link_meta(
            dir.path(),
            "https://EXAMPLE.com:443/article#one",
            LinkMeta {
                url: "https://victim.example/".into(),
                title: "Cached".into(),
                ..LinkMeta::default()
            },
        )
        .unwrap();
        // No network: differently-spelled request hits the same normalized cache key.
        assert_eq!(
            link_meta(dir.path(), "https://example.com/article#two").unwrap(),
            result
        );
        let mut restored = CacheState {
            dir: None,
            entries: HashMap::new(),
        };
        load_cache(&mut restored, dir.path());
        assert_eq!(restored.entries.len(), 1);
        assert!(restored.entries.contains_key("https://example.com/article"));
        assert!(!restored.entries.contains_key("https://victim.example/"));
        for i in 0..CACHE_CAP {
            insert_cache(
                &mut restored,
                LinkMeta {
                    url: format!("https://entry.example/{i}"),
                    ..result.clone()
                },
            );
        }
        assert!(restored.entries.len() <= CACHE_CAP);
        assert!(restored
            .entries
            .contains_key(&format!("https://entry.example/{}", CACHE_CAP - 1)));
    }

    #[test]
    fn frontend_metadata_limits_match() {
        let frontend = include_str!("../../../../src/lib/linkMeta.ts");
        assert!(frontend.contains("LINK_META_MAX_BYTES = 16 * 1024"));
        assert!(frontend.contains(&format!("URL_MAX = {URL_MAX}")));
        assert!(frontend.contains(&format!("text(input.title, {TITLE_MAX})")));
        assert!(frontend.contains(&format!("text(input.description, {DESC_MAX})")));
    }

    #[test]
    fn extracts_title_and_meta() {
        let html = r#"<html><head>
            <meta charset="utf-8">
            <title>  一段   标题 &amp; 更多 </title>
            <meta property="og:description" content="摘要里也有 &lt;实体&gt; &quot;引号&quot;">
            <meta name="og:site_name" content="示例站">
            </head><body>正文</body></html>"#;
        let title = extract_title(html).unwrap();
        assert_eq!(title, "一段 标题 & 更多");
        let description = extract_meta_content(html, &["og:description", "description"]).unwrap();
        assert_eq!(description, "摘要里也有 <实体> \"引号\"");
        let site = extract_meta_content(html, &["og:site_name"]).unwrap();
        assert_eq!(site, "示例站");
    }

    #[test]
    fn meta_attribute_order_and_quotes() {
        let html = r#"<meta content='倒着写的' name=description><meta name="x" content="y">"#;
        assert_eq!(
            extract_meta_content(html, &["description"]).unwrap(),
            "倒着写的"
        );
    }

    #[test]
    fn numeric_entities_and_truncate() {
        assert_eq!(decode_entities("A &#8211; B &#x2764;"), "A – B ❤");
        assert_eq!(truncate("一二三四五", 3), "一二三…");
        assert_eq!(truncate("abc", 5), "abc");
    }

    #[test]
    fn charset_sniffing() {
        let html = br#"<meta charset="gbk"><title></title>"#;
        assert_eq!(sniff_charset(html, "text/html"), "gbk");
        assert_eq!(sniff_charset(html, "text/html; charset=UTF-8"), "UTF-8");
        assert_eq!(sniff_charset(b"<html>", ""), "utf-8");
        // GBK 页面的中文标题能正确解出来（乱码的标题不如没有）
        let gbk = b"<title>\xbf\xec\xc0\xd6</title>";
        let (decoded, _, _) = encoding_rs::GBK.decode(gbk);
        assert_eq!(extract_title(&decoded).unwrap(), "快乐");
    }

    #[test]
    fn icons_from_link_tags_with_fallbacks() {
        let page = Url::parse("https://news.example.com/a/b?c=1").unwrap();
        // 相对地址按页面解成绝对地址
        assert_eq!(
            extract_icon(r#"<link rel="icon" href="/static/logo.png">"#, &page),
            "https://news.example.com/static/logo.png"
        );
        assert_eq!(
            extract_icon(r#"<link rel="shortcut icon" href="../fav.png">"#, &page),
            "https://news.example.com/fav.png"
        );
        // apple-touch-icon 只当兜底，别抢 rel=icon 的位置
        assert_eq!(
            extract_icon(
                r#"<link rel="apple-touch-icon" href="/big.png"><link rel="icon" href="/small.png">"#,
                &page
            ),
            "https://news.example.com/small.png"
        );
        // 没声明 icon：试同源的 /favicon.ico
        assert_eq!(
            extract_icon(r#"<link rel="stylesheet" href="/a.css">"#, &page),
            "https://news.example.com/favicon.ico"
        );
    }
}

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
/// 缓存条数上限：超了丢最早写入的那一半（重新抓一次而已）
const CACHE_CAP: usize = 500;
const TITLE_MAX: usize = 200;
const DESC_MAX: usize = 400;

/// 一个链接的元数据。`title` / `description` 都可能为空串（页面没写）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LinkMeta {
    /// 规范化后的链接（去掉 fragment）
    pub url: String,
    /// 站点名：og:site_name 优先，退回主机名（去掉 www.）
    pub site: String,
    pub title: String,
    pub description: String,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
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
        state.entries = file.entries;
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

/// 规范化：去 fragment（`#a` 与 `#b` 是同一个页面），其余原样。
fn normalize(url: &Url) -> String {
    let mut copy = url.clone();
    copy.set_fragment(None);
    copy.to_string()
}

fn host_of(url: &Url) -> String {
    url.host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string()
}

/// 取一个链接的元数据（命中缓存直接回）。`runtime_dir` 是缓存落盘位置。
pub fn link_meta(runtime_dir: &Path, url: &str) -> CoreResult<LinkMeta> {
    let parsed = Url::parse(url).map_err(|_| {
        CoreError::validation("LINK_URL_INVALID", format!("不是合法的链接：{url}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CoreError::validation(
            "LINK_URL_INVALID",
            format!("只解析 http/https 链接：{url}"),
        ));
    }
    let key = normalize(&parsed);
    {
        let mut state = cache().lock().unwrap_or_else(|poison| poison.into_inner());
        load_cache(&mut state, runtime_dir);
        if let Some(hit) = state.entries.get(&key) {
            return Ok(hit.clone());
        }
    }

    let mut meta = fetch(&parsed)?;
    if meta.title.is_empty() && meta.description.is_empty() {
        return Err(CoreError::execution(
            "LINK_META_EMPTY",
            "页面里没有可用的标题或摘要",
        ));
    }
    meta.url = key.clone();

    let mut state = cache().lock().unwrap_or_else(|poison| poison.into_inner());
    load_cache(&mut state, runtime_dir);
    if state.entries.len() >= CACHE_CAP {
        // 简单淘汰：清掉最早的一半（HashMap 无序，够用——这只是一层缓存）
        let drop_keys: Vec<String> = state
            .entries
            .keys()
            .take(CACHE_CAP / 2)
            .cloned()
            .collect();
        for key in drop_keys {
            state.entries.remove(&key);
        }
    }
    state.entries.insert(key, meta.clone());
    save_cache(&state);
    Ok(meta)
}

fn fetch(url: &Url) -> CoreResult<LinkMeta> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .timeout_write(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build();
    let response = match agent.get(url.as_str()).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => {
            return Err(CoreError::execution(
                "LINK_FETCH_FAILED",
                format!("页面返回 {code}"),
            ))
        }
        Err(error) => {
            return Err(CoreError::execution(
                "LINK_FETCH_FAILED",
                format!("抓取失败：{error}"),
            ))
        }
    };
    let content_type = response
        .header("content-type")
        .unwrap_or("")
        .to_ascii_lowercase();
    // 图片 / PDF / 二进制抓来也没有标题可读，直接当失败
    if !content_type.is_empty()
        && !content_type.contains("html")
        && !content_type.contains("text/")
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
    let encoding = encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (html, _, _) = encoding.decode(&buffer);
    let html = html.into_owned();

    let title = extract_meta_content(&html, &["og:title", "twitter:title"])
        .or_else(|| extract_title(&html))
        .unwrap_or_default();
    let description = extract_meta_content(&html, &["og:description", "description", "twitter:description"])
        .unwrap_or_default();
    let site = extract_meta_content(&html, &["og:site_name"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| host_of(url));

    Ok(LinkMeta {
        url: url.to_string(),
        site,
        title: truncate(&title, TITLE_MAX),
        description: truncate(&description, DESC_MAX),
    })
}

/// 编码嗅探：content-type 里的 charset 优先，其次前 2KB 里的 `<meta charset>`，
/// 都没有就按 UTF-8 解（lossy，解不出来也有个能看的近似）。
fn sniff_charset(buffer: &[u8], content_type: &str) -> String {
    if let Some(index) = content_type.find("charset=") {
        let value = &content_type[index + "charset=".len()..];
        let value = value.split([';', ' ']).next().unwrap_or("").trim_matches('"');
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
            && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' || bytes[index] == b'-' || bytes[index] == b':')
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
}

//! 图片文件同步（v0.5.0）：markdown 插图、列表背景、头像三类文件本体。
//!
//! 数据域实体里只有图片**引用**（`![](md-….png)` / `img:bg-….png` / 头像文件名），
//! 文件本体另走一条 blob 通道：内容寻址（sha256）、不可变、无 LWW——同名文件
//! 在两台设备上内容一致时服务端只存一份，不一致时以最后上传者为准。
//!
//! 存储位置（与服务端 SQLite 的 kind/nodeId/filename 三元组一一对应）：
//! - `img/data/<nodeId>/<file>`  markdown 插图（kind=entry）
//! - `img/background/<file>`     列表背景（kind=background）
//! - `img/avator/<file>`         头像（kind=avatar，目录名是历史拼写）

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::error::{CoreError, CoreResult};
use crate::repo::Layout;
use crate::sync::crypto::{open_bytes, seal_bytes, sha256_hex, to_hex};
use crate::sync::engine::SyncClient;
use crate::sync::merge::Scopes;
use crate::sync::state::SyncStateFile;

pub const KIND_ENTRY: &str = "entry";
pub const KIND_BACKGROUND: &str = "background";
pub const KIND_AVATAR: &str = "avatar";

/// 图片类别归属哪个同步范围（v0.5.1 起没有独立的「同步图片」开关）：
/// markdown 插图属于数据，列表背景与头像属于设置（配色/背景/个人资料）。
fn kind_enabled(kind: &str, scopes: &Scopes) -> bool {
    match kind {
        KIND_ENTRY => scopes.data,
        KIND_BACKGROUND | KIND_AVATAR => scopes.settings,
        _ => false,
    }
}

/// 单张图片下载体积上限（防御服务端返回异常大响应）。
const MAX_IMAGE_BYTES: u64 = 96 * 1024 * 1024;

/// 服务端返回的图片元数据（不含密文）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageMeta {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageChangesPage {
    #[serde(default)]
    pub images: Vec<ImageMeta>,
    pub current_seq: u64,
}

/// 本机磁盘上的一张图片。
#[derive(Debug, Clone)]
pub struct LocalImage {
    pub id: String,
    pub kind: String,
    pub node_id: String,
    pub filename: String,
    pub path: PathBuf,
    pub size: u64,
    pub hash: String,
    pub updated_at: String,
}

#[derive(Debug, Default)]
pub struct ImageTally {
    pub pulled: usize,
    pub pushed: usize,
    pub warnings: Vec<String>,
}

/// 图片身份 = sha256(kind|nodeId|filename)：跨设备确定性一致，且不含路径分隔符，
/// 可以安全放进 URL 路径。命名冲突由文件名本身（`md-<纳秒>-<计数器>.<ext>`）保证。
pub fn image_id(kind: &str, node_id: &str, filename: &str) -> String {
    sha256_hex(format!("{kind}|{node_id}|{filename}").as_bytes())
}

/// 路径组件白名单校验：服务端给的 nodeId/filename 会参与本地路径拼接，
/// 必须挡掉穿越（`..`、分隔符、绝对路径、控制字符）。
fn safe_component(raw: &str, what: &str) -> CoreResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 160 {
        return Err(CoreError::validation("IMAGE_NAME_INVALID", format!("{what}无效")));
    }
    if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.starts_with('.')
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err(CoreError::validation(
            "IMAGE_NAME_INVALID",
            format!("{what}含非法字符：{trimmed}"),
        ));
    }
    Ok(trimmed.to_string())
}

/// 服务端图片元数据 → 本地落盘路径（含穿越校验）。
pub fn target_path(layout: &Layout, kind: &str, node_id: &str, filename: &str) -> CoreResult<PathBuf> {
    let filename = safe_component(filename, "图片文件名")?;
    match kind {
        KIND_AVATAR => Ok(layout.avatar_img_dir().join(&filename)),
        KIND_BACKGROUND => Ok(layout.background_img_dir().join(&filename)),
        KIND_ENTRY => {
            let node_id = safe_component(node_id, "条目 ID")?;
            Ok(layout.entry_img_dir(&node_id).join(&filename))
        }
        other => Err(CoreError::validation(
            "IMAGE_KIND_INVALID",
            format!("未知图片类别 `{other}`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// 本机清单
// ---------------------------------------------------------------------------

/// 内容哈希缓存：自动同步最短 5s 一轮，每轮重读全部图片算 sha256 会白烧 IO。
/// 键是路径，命中条件是 mtime + size 都没变。
fn hash_cache() -> &'static Mutex<HashMap<PathBuf, (u64, u64, String)>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, (u64, u64, String)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_hash(path: &Path, mtime: u64, size: u64) -> CoreResult<String> {
    if let Ok(cache) = hash_cache().lock() {
        if let Some((cached_mtime, cached_size, hash)) = cache.get(path) {
            if *cached_mtime == mtime && *cached_size == size {
                return Ok(hash.clone());
            }
        }
    }
    let bytes = fs::read(path)?;
    let hash = sha256_hex(&bytes);
    if let Ok(mut cache) = hash_cache().lock() {
        cache.insert(path.to_path_buf(), (mtime, size, hash.clone()));
    }
    Ok(hash)
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn iso_from_unix(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|time| {
            time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .unwrap_or_else(crate::time::now_iso)
}

fn is_syncable_file(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with(".tmp") && !name.ends_with(".part")
}

fn collect_dir(
    dir: &Path,
    kind: &str,
    node_id: &str,
    out: &mut Vec<LocalImage>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_syncable_file(filename) || meta.len() > MAX_IMAGE_BYTES {
            continue;
        }
        let mtime = mtime_secs(&meta);
        let Ok(hash) = file_hash(&path, mtime, meta.len()) else {
            continue;
        };
        out.push(LocalImage {
            id: image_id(kind, node_id, filename),
            kind: kind.to_string(),
            node_id: node_id.to_string(),
            filename: filename.to_string(),
            path,
            size: meta.len(),
            hash,
            updated_at: iso_from_unix(mtime),
        });
    }
}

/// 枚举本机全部可同步图片（只含当前同步范围允许的类别）。
pub fn inventory(layout: &Layout, scopes: &Scopes) -> Vec<LocalImage> {
    let mut out = Vec::new();
    if kind_enabled(KIND_AVATAR, scopes) {
        collect_dir(&layout.avatar_img_dir(), KIND_AVATAR, "", &mut out);
    }
    if kind_enabled(KIND_BACKGROUND, scopes) {
        collect_dir(&layout.background_img_dir(), KIND_BACKGROUND, "", &mut out);
    }
    if !kind_enabled(KIND_ENTRY, scopes) {
        return out;
    }
    if let Ok(entries) = fs::read_dir(layout.entry_img_root()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(node_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if safe_component(node_id, "条目 ID").is_err() {
                continue;
            }
            collect_dir(&path, KIND_ENTRY, node_id, &mut out);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 同步编排
// ---------------------------------------------------------------------------

/// 拉取 + 推送图片。水位（`last_pulled_image_seq`）逐行推进，中途失败不丢已完成的进度。
pub fn sync_images(
    client: &SyncClient,
    token: &str,
    enc_key: &[u8; 32],
    device_id: &str,
    scope: &str,
    layout: &Layout,
    scopes: &Scopes,
    state: &mut SyncStateFile,
) -> CoreResult<ImageTally> {
    let mut tally = ImageTally::default();
    let local = inventory(layout, scopes);

    // 1. PULL：增量元数据 → 本地缺失/哈希不符的才下载密文
    let mut cursor = state.last_pulled_image_seq;
    loop {
        let page = client.image_changes(token, cursor)?;
        let count = page.images.len();
        let mut max_seq = cursor;
        for meta in &page.images {
            // 不在当前范围内的类别直接跳过（水位仍要越过它）
            if kind_enabled(&meta.kind, scopes) {
                match pull_one(client, token, enc_key, layout, meta, &local) {
                    Ok(true) => tally.pulled += 1,
                    Ok(false) => {}
                    Err(error) => tally.warnings.push(error.message),
                }
            }
            max_seq = max_seq.max(meta.seq);
        }
        let last_page = count == 0 || count < crate::sync::engine::IMAGE_PAGE_LIMIT;
        cursor = if last_page { page.current_seq } else { max_seq };
        state.last_pulled_image_seq = cursor;
        if last_page {
            break;
        }
    }

    // 2. PUSH：问服务端哪些缺/不一致，只上传这些
    let mut candidates: Vec<(String, String)> = local
        .iter()
        .map(|image| (image.id.clone(), image.hash.clone()))
        .collect();
    // read_dir 顺序不保证稳定：排序后再算指纹，否则同一批图片每轮指纹都不同
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    if !candidates.is_empty() {
        let fingerprint = manifest_fingerprint(&candidates);
        if manifest_settled(scope, &fingerprint) {
            // 本地清单没变、上次对账结论是「服务端已齐全」→ 跳过这次 check 请求
            return Ok(tally);
        }
        let needed = client.image_check(token, &candidates)?;
        let mut failures = 0usize;
        if !needed.is_empty() {
            let needed_set: HashMap<&str, ()> =
                needed.iter().map(|id| (id.as_str(), ())).collect();
            for image in local.iter().filter(|image| needed_set.contains_key(image.id.as_str())) {
                match push_one(client, token, enc_key, device_id, image) {
                    Ok(()) => tally.pushed += 1,
                    Err(error) => {
                        failures += 1;
                        tally
                            .warnings
                            .push(format!("图片 {} 上传失败：{}", image.filename, error.message));
                    }
                }
            }
        }
        if failures == 0 {
            remember_manifest(scope, &fingerprint);
        }
    }
    Ok(tally)
}

/// 清单指纹：本地图片集合（id + 内容哈希）的摘要。
fn manifest_fingerprint(candidates: &[(String, String)]) -> String {
    let joined = candidates
        .iter()
        .map(|(id, hash)| format!("{id}:{hash}"))
        .collect::<Vec<_>>()
        .join(",");
    sha256_hex(joined.as_bytes())
}

/// 进程内缓存：上一次「服务端已齐全」的 (作用域, 清单指纹)。
/// 作用域 = server_url|username——同一批图片换个服务器/账户时必须重新对账，
/// 否则会被误判为已齐全而永远不上传。服务端只会新增图片、不会自己丢，
/// 所以同作用域下指纹不变时跳过对账是安全的。
fn manifest_cache() -> &'static Mutex<Option<(String, String)>> {
    static CACHE: std::sync::OnceLock<Mutex<Option<(String, String)>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// 重新配对（register/login）时清空：服务端数据可能被管理员删过，旧结论不再可信。
pub fn invalidate_manifest_cache() {
    if let Ok(mut cache) = manifest_cache().lock() {
        *cache = None;
    }
}

fn manifest_settled(scope: &str, fingerprint: &str) -> bool {
    manifest_cache()
        .lock()
        .map(|cache| {
            cache
                .as_ref()
                .map(|(cached_scope, cached_print)| {
                    cached_scope == scope && cached_print == fingerprint
                })
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn remember_manifest(scope: &str, fingerprint: &str) {
    if let Ok(mut cache) = manifest_cache().lock() {
        *cache = Some((scope.to_string(), fingerprint.to_string()));
    }
}

/// 拉取单张图片：返回 Ok(true) 表示本轮真的落盘了一张。
fn pull_one(
    client: &SyncClient,
    token: &str,
    enc_key: &[u8; 32],
    layout: &Layout,
    meta: &ImageMeta,
    local: &[LocalImage],
) -> CoreResult<bool> {
    if meta.deleted {
        // 删除不传播：孤儿图片由后续的「清理无引用图片」功能统一处理。
        return Ok(false);
    }
    if meta.id.len() != 64 {
        return Err(CoreError::io(format!(
            "图片 {} 元数据 ID 无效",
            meta.filename
        )));
    }
    // 本地已有同内容 → 跳过下载
    if let Some(existing) = local.iter().find(|image| image.id == meta.id) {
        if !meta.hash.is_empty() && existing.hash == meta.hash {
            return Ok(false);
        }
    }
    let path = target_path(layout, &meta.kind, &meta.node_id, &meta.filename)?;
    let (nonce_hex, cipher) = match client.image_get(token, &meta.id)? {
        Some(value) => value,
        None => return Ok(false),
    };
    if cipher.len() as u64 > MAX_IMAGE_BYTES {
        return Err(CoreError::io(format!("图片 {} 超出体积上限", meta.filename)));
    }
    let nonce = hex_decode(&nonce_hex)
        .map_err(|error| CoreError::io(format!("图片 {} nonce 无效：{error}", meta.filename)))?;
    let plain = open_bytes(enc_key, &meta.id, &nonce, &cipher)
        .map_err(|error| {
            CoreError::io(format!("图片 {} 解密失败：{}", meta.filename, error.message))
        })?;
    write_atomic(&path, &plain).map_err(|error| {
        CoreError::io(format!(
            "图片 {} 落盘失败：{}",
            meta.filename,
            error
        ))
    })?;
    Ok(true)
}

fn push_one(
    client: &SyncClient,
    token: &str,
    enc_key: &[u8; 32],
    device_id: &str,
    image: &LocalImage,
) -> CoreResult<()> {
    let bytes = fs::read(&image.path)?;
    let (nonce, cipher) = seal_bytes(enc_key, &image.id, &bytes)?;
    client.image_put(
        token,
        image,
        &to_hex(&nonce),
        cipher,
        device_id,
    )?;
    Ok(())
}

fn hex_decode(raw: &str) -> Result<Vec<u8>, String> {
    if raw.len() % 2 != 0 {
        return Err("长度不是偶数".to_string());
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&raw[index..index + 2], 16)
                .map_err(|error| format!("{error}"))
        })
        .collect()
}

/// 原子落盘：先写 `.part` 再 rename，避免同步中断留下半截图片被渲染成破图。
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.part",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("img")
    ));
    fs::write(&tmp, bytes)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_id_is_deterministic_and_path_safe() {
        let a = image_id(KIND_ENTRY, "node-1", "md-1-2.png");
        let b = image_id(KIND_ENTRY, "node-1", "md-1-2.png");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, image_id(KIND_ENTRY, "node-2", "md-1-2.png"));
        assert_ne!(a, image_id(KIND_BACKGROUND, "", "md-1-2.png"));
    }

    #[test]
    fn target_path_rejects_traversal() {
        let layout = Layout::new(PathBuf::from("/tmp/kxtodo-test"));
        assert!(target_path(&layout, KIND_ENTRY, "../../etc", "x.png").is_err());
        assert!(target_path(&layout, KIND_ENTRY, "node-1", "../x.png").is_err());
        assert!(target_path(&layout, KIND_ENTRY, "node-1", "a/b.png").is_err());
        assert!(target_path(&layout, KIND_ENTRY, "node\\1", "a.png").is_err());
        assert!(target_path(&layout, "other", "", "a.png").is_err());
        let ok = target_path(&layout, KIND_ENTRY, "node-1", "md-1-2.png").unwrap();
        assert_eq!(ok, layout.entry_img_dir("node-1").join("md-1-2.png"));
        assert_eq!(
            target_path(&layout, KIND_AVATAR, "", "avatar-1.png").unwrap(),
            layout.avatar_img_dir().join("avatar-1.png")
        );
        assert_eq!(
            target_path(&layout, KIND_BACKGROUND, "", "bg-1.png").unwrap(),
            layout.background_img_dir().join("bg-1.png")
        );
    }

    #[test]
    fn inventory_finds_three_kinds_and_skips_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path().to_path_buf());
        layout.ensure().unwrap();
        fs::write(layout.avatar_img_dir().join("avatar-1.png"), b"avatar").unwrap();
        fs::write(layout.background_img_dir().join("bg-1.png"), b"bg").unwrap();
        fs::write(layout.background_img_dir().join("bg-2.png.part"), b"junk").unwrap();
        let entry = layout.entry_img_dir("node-1");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("md-1-1.png"), b"md").unwrap();

        let all = Scopes {
            data: true,
            settings: true,
            schedules: false,
        };
        let images = inventory(&layout, &all);
        assert_eq!(images.len(), 3, "临时文件不计入清单");
        let kinds: Vec<&str> = images.iter().map(|image| image.kind.as_str()).collect();
        assert!(kinds.contains(&KIND_AVATAR));
        assert!(kinds.contains(&KIND_BACKGROUND));
        assert!(kinds.contains(&KIND_ENTRY));
        let md = images.iter().find(|image| image.kind == KIND_ENTRY).unwrap();
        assert_eq!(md.node_id, "node-1");
        assert_eq!(md.id, image_id(KIND_ENTRY, "node-1", "md-1-1.png"));

        // 插图跟数据范围走，背景/头像跟设置范围走
        let data_only = Scopes {
            data: true,
            settings: false,
            schedules: false,
        };
        let data_images = inventory(&layout, &data_only);
        let kinds: Vec<&str> = data_images.iter().map(|image| image.kind.as_str()).collect();
        assert_eq!(kinds, vec![KIND_ENTRY]);
        let settings_only = Scopes {
            data: false,
            settings: true,
            schedules: false,
        };
        let settings_images = inventory(&layout, &settings_only);
        let mut kinds: Vec<&str> = settings_images
            .iter()
            .map(|image| image.kind.as_str())
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec![KIND_AVATAR, KIND_BACKGROUND]);
    }
}

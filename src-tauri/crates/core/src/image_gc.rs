//! 无引用插图清理（v0.7.2）：保存后扫条目/日记的插图目录，删掉没有任何 markdown
//! 再引用的图片——从源头掐断孤儿图（图片同步的删除本就不传播，事后清理不如不生）。
//!
//! **宽限窗是硬性要求**：编辑器的流程是「图片先落盘、引用它的正文后保存」，同步也可能
//! 在引用它的实体合并之前/之中把图片先送到本机。只按「此刻无人引用」判定会杀掉在途图片，
//! 所以只删 mtime 超过 `grace` 的文件：生产调用点传 5 分钟（`GRACE`），测试传 `Duration::ZERO`。
//!
//! **清理永远不许弄失败保存本身**：目录不存在直接返回 0，一切 IO 错误吞掉，只回报删除张数。
//! 开销刻意压到最低：一次 `read_dir` + 扫描本来就在内存里的 markdown 字符串。

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use crate::diary_archive::{is_image_name, is_safe_image_name, ref_basename, rewrite_image_refs};

/// 生产宽限窗：5 分钟，足够盖住「图片先落盘、正文后保存」与同步的到达顺序差。
pub const GRACE: Duration = Duration::from_secs(300);

/// 收集一批 markdown 里引用到的插图裸文件名。
/// `images/x.png`、`./x.png`、`../../深层/相对/x.png` 都由 `ref_basename` 归一成 `x.png`；
/// 远程链接与 data: 内联图不会被 `rewrite_image_refs` 送进来。
pub fn referenced_basenames<'a>(markdowns: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    let mut referenced = HashSet::new();
    for markdown in markdowns {
        rewrite_image_refs(markdown, |src| {
            let base = ref_basename(src);
            if is_safe_image_name(base) {
                referenced.insert(base.to_string());
            }
            None // 只收集引用，不改正文
        });
    }
    referenced
}

/// 扫 `dir` 里的图片文件，删掉「无人引用且 mtime 已超过 `grace`」的那些；返回删除张数。
/// 目录不存在/读不动 = 0；单个文件删不掉就跳过不计。非图片文件与名字过不了
/// `is_safe_image_name` 护栏的一律不碰（用户可能往目录里放了别的东西）。
pub fn sweep_unreferenced<'a>(
    dir: &Path,
    markdowns: impl IntoIterator<Item = &'a str>,
    grace: Duration,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let referenced = referenced_basenames(markdowns);
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|raw| raw.to_str()) else { continue };
        if !is_image_name(name) || !is_safe_image_name(name) {
            continue;
        }
        if referenced.contains(name) {
            continue;
        }
        // mtime 在宽限窗内（或读不到、或在未来）= 可能是刚落盘的在途图片，不动
        let Ok(metadata) = std::fs::metadata(&path) else { continue };
        let Ok(mtime) = metadata.modified() else { continue };
        let Ok(age) = mtime.elapsed() else { continue };
        if age < grace {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

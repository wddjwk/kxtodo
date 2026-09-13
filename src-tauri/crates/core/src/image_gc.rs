//! 无引用插图清理（v0.7.2，v0.7.3 去掉宽限窗）：保存后扫条目/日记的插图目录，
//! 删掉没有任何 markdown 再引用的图片——从源头掐断孤儿图（图片同步的删除本就不传播，
//! 事后清理不如不生）。
//!
//! **立即删是安全的，因为清理只跟在「本地写」后面**：三个生产调用点（`core.rs` 的
//! `sweep_entry_images`、`ops_diary.rs` 的 `sweep_diary_images` 与 `ops_ledger.rs` 的
//! `sweep_ledger_images`）都只在 task/diary/ledger 的 add/modify/remove **本地保存成功后**
//! 运行，传入的正是刚写盘那份文件里该目录归属实体的全部引用（task/diary 是 markdown，
//! 记账是条目的 image 字段）——对这个目录来说引用集合就是权威的。同步拉取的合并走
//! `sync/engine.rs` 里的 `repo::write_*`，**从不触发清理**，所以「图片先于引用它的
//! 实体到达本机」的在途窗口根本不暴露给这把扫帚；编辑器「图片先落盘、正文后保存」
//! 的流程也一样——正文保存那一刻引用集合已经完整。
//!
//! **清理永远不许弄失败保存本身**：目录不存在直接返回 0，一切 IO 错误吞掉，只回报删除张数。
//! 开销刻意压到最低：一次 `read_dir` + 扫描本来就在内存里的 markdown 字符串。

use std::collections::HashSet;
use std::path::Path;

use crate::diary_archive::{is_image_name, is_safe_image_name, ref_basename, rewrite_image_refs};

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

/// 扫 `dir` 里的图片文件，删掉无人引用的那些（不看 mtime——调用点保证引用集合是
/// 刚写盘的权威值，见模块注释）；返回删除张数。
/// 目录不存在/读不动 = 0；单个文件删不掉就跳过不计。非图片文件与名字过不了
/// `is_safe_image_name` 护栏的一律不碰（用户可能往目录里放了别的东西）。
pub fn sweep_unreferenced<'a>(
    dir: &Path,
    markdowns: impl IntoIterator<Item = &'a str>,
) -> usize {
    let referenced = referenced_basenames(markdowns);
    sweep_unreferenced_by_names(dir, &referenced)
}

/// `sweep_unreferenced` 的「引用集合已算好」版本：记账条目的附图不在 markdown 里，
/// 而是各条目的 `image` 字段（裸文件名），调用点直接把这份集合传进来。护栏与语义完全一致。
pub fn sweep_unreferenced_by_names(dir: &Path, referenced: &HashSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
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
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

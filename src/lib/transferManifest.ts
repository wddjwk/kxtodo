/**
 * 传输清单的路径语义（v0.8.6 需求 5.1，纯逻辑、有单测）。
 *
 * 协议里的 `rel` 一律是**相对传输根的路径**，接收端拿 `root + rel` 还原目录结构：
 * - 单文件：`root` = 文件所在目录，`rel` = 文件名；
 * - 文件夹：`root` = 文件夹自身，`rel` = 包内相对路径（由 `transfer_list_folder` 列）；
 * - 暂存/剪贴板图片：`root` = outbox 目录，`rel` = 文件名。
 *
 * 曾经的实现直接把绝对路径当 `rel`、`root` 恒为 null：Windows 接收端被 `safe_join`
 * 的 `:` 判定整单拒绝，Linux 接收端把绝对路径镜像成一棵目录树。core 侧有同名口径的
 * tripwire（`transfer.rs` 的 `TRANSFER_MANIFEST_ABSOLUTE_PATH`）兜底。
 */

export type SendKind = "file" | "folder" | "text" | "clipboard";

export type PickedItem = {
  key: string;
  /** 相对 `root` 的路径；文本项里就是正文本身 */
  rel: string;
  size: number;
  kind: SendKind;
  preview: string;
  /** 该文件的基准目录；文本项为 null */
  root: string | null;
};

/** 路径最后一段（`\` 与 `/` 都认）。 */
export function baseName(path: string): string {
  const clean = path.replace(/[\\/]+$/, "");
  const cut = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  return cut < 0 ? clean : clean.slice(cut + 1);
}

/** 路径的父目录；没有分隔符时给 `.`，盘根（`C:\`）与 POSIX 根（`/`）都保留。 */
export function dirName(path: string): string {
  const clean = path.replace(/[\\/]+$/, "");
  const cut = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  if (cut < 0) return ".";
  if (cut === 0) return clean.slice(0, 1);
  if (clean[cut - 1] === ":") return clean.slice(0, cut + 1);
  return clean.slice(0, cut);
}

/** 路径看起来像绝对路径（与 core tripwire 同一口径）。 */
export function isAbsoluteRel(rel: string): boolean {
  return rel.startsWith("/") || rel.startsWith("\\") || rel.includes(":");
}

/** 绝对路径 → 一条可发送的文件项（`root` = 所在目录，`rel` = 文件名）。 */
export function filePick(path: string, size: number): PickedItem {
  const name = baseName(path) || path;
  return { key: `f:${path}`, rel: name, size, kind: "file", preview: name, root: dirName(path) };
}

/** 暂存/剪贴板图片：字节已经写进 outbox，`rel` 就是落盘时的文件名。 */
export function spoolPick(root: string, name: string, size: number, kind: SendKind = "file"): PickedItem {
  return { key: `${kind === "clipboard" ? "c" : "f"}:${root}/${name}`, rel: name, size, kind, preview: name, root };
}

/** 文件夹项：`rel` 与 `root` 都是文件夹路径，清单在发送时才列。 */
export function folderPick(dir: string, size: number): PickedItem {
  const name = baseName(dir) || dir;
  return { key: `d:${dir}`, rel: dir, size, kind: "folder", preview: name, root: dir };
}

/** 文本项（打字与剪贴板文本同款）：正文住在 `rel`，没有 root。 */
export function textPick(text: string, kind: SendKind = "text"): PickedItem {
  const prefix = kind === "clipboard" ? "c" : "t";
  return { key: `${prefix}:${Date.now()}`, rel: text, size: text.length, kind, preview: text.slice(0, 40), root: null };
}

/** 走文本通道的项：打字与剪贴板文本。剪贴板**图片**有 root，是文件，别被卷进来。 */
export function isTextItem(item: PickedItem): boolean {
  return (item.kind === "text" || item.kind === "clipboard") && item.root === null;
}

export function isFileItem(item: PickedItem): boolean {
  return item.kind === "file" || (item.kind === "clipboard" && item.root !== null);
}

/**
 * 文件按 `root` 分组：一次挑选可能跨目录（多选来自不同目录、桌面文件 + 剪贴板图片），
 * 而一次 `transferSend` 只能有一个 root——拆成多次发送，绝不把绝对路径塞进 `rel`。
 */
export function groupByRoot(files: PickedItem[]): Array<[string | null, PickedItem[]]> {
  const groups = new Map<string | null, PickedItem[]>();
  for (const item of files) {
    const list = groups.get(item.root);
    if (list) list.push(item);
    else groups.set(item.root, [item]);
  }
  return [...groups.entries()];
}

/**
 * 拼绝对路径（移动端「打开最后接收的文件」用）：`done` 事件给保存目录、`fileDone`
 * 给落盘文件名，两者拼起来才是能交给系统打开的路径。
 * 分隔符跟着目录本身走（Windows 用 `\`、Linux/Android 用 `/`），目录末尾已有分隔符就不重复。
 */
export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  if (/[\\/]$/.test(dir)) return `${dir}${name}`;
  return `${dir}${dir.includes("\\") ? "\\" : "/"}${name}`;
}

/**
 * 中间省略显示用：把路径切成「头（可截断的目录段，含分隔符）+ 尾（最后一段，保留）」。
 * 头段配 `text-overflow: ellipsis`、尾段 `flex: 0 0 auto`，纯 CSS 就能做到
 * 「头尾可见、中间省略」——不写 JS 截字符串，缩放/字号变化时也不会算错。
 */
export function splitPathTail(path: string): { head: string; tail: string } {
  const clean = path.replace(/[\\/]+$/, "");
  const cut = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  if (cut < 0) return { head: "", tail: clean };
  return { head: clean.slice(0, cut + 1), tail: clean.slice(cut + 1) };
}

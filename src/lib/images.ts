import { writable, get, type Writable } from "svelte/store";
import { backgroundImageUrl, avatarImageUrl, mdImageUrl } from "./backend";

const LOCAL_PREFIX = "img:";

/**
 * 读图失败的退避节奏。**一次读不到不等于永远读不到**：同步是「引用先到、字节后到」的
 * 常态（手机端刚配对完那一轮尤其明显——设置里的背景引用合并进来了，图片 blob 还在路上），
 * 早先失败就静默放弃且不重试，于是背景图「有时候显示、有时候不显示」，而同步完的桌面端
 * 一切正常（它走 asset 协议，浏览器自己会重试请求）。
 */
const RETRY_DELAYS = [1200, 4000, 10000];

// ---------------------------------------------------------------------------
// 头像缩略图缓存（冷启动第一帧就有头像，不闪默认圆圈）
// ---------------------------------------------------------------------------

/**
 * 桌面端头像是**文件名**，要经一次 IPC 才解析得出可显示的 URL——冷启动第一帧
 * 注定是空的（默认圆圈），解析到了再跳成头像。把解析结果压成小缩略图存进
 * localStorage，模块初始化时同步种进 avatarCache：第一帧直接用缩略图渲染，
 * 头像从此不参与「先默认再跳变」。显示尺寸最大不到 100px，160px 缩略图肉眼无差。
 */
const AVATAR_THUMB_KEY = "kxtodo-avatar-thumbs-v1";
const AVATAR_THUMB_EDGE = 160;
const AVATAR_THUMB_MAX = 8;
/** 解析结果短于这个长度就原样存（Windows 的 asset 协议 URL 只有百来字节）；
 *  更长的（Linux/安卓的 base64 dataURL）先压成缩略图，不然几个头像就顶爆配额。 */
const AVATAR_THUMB_INLINE_MAX = 40_000;

function readAvatarThumbs(): Record<string, string> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(AVATAR_THUMB_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Record<string, string> = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === "string" && value) out[key] = value;
    }
    return out;
  } catch {
    return {};
  }
}

/** 把任意可显示 URL 压成缩略图 dataURL；跨源画布污染、解码失败一律回 null（宁缺毋假）。 */
async function toThumbDataUrl(url: string, maxEdge: number): Promise<string | null> {
  try {
    const img = new Image();
    if (!url.startsWith("data:")) img.crossOrigin = "anonymous";
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("image load failed"));
      img.src = url;
    });
    const natural = Math.max(img.naturalWidth, img.naturalHeight);
    if (!natural) return null;
    const scale = Math.min(1, maxEdge / natural);
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(img.naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(img.naturalHeight * scale));
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.drawImage(img, 0, 0, canvas.width, canvas.height);
    // 可能带透明通道的格式保 PNG（铺成 JPEG 透明区会变黑），照片类出 JPEG
    const mime = /^data:image\/(png|webp|gif)/i.test(url) || /\.(png|webp|gif)([?#]|$)/i.test(url) ? "image/png" : "image/jpeg";
    return canvas.toDataURL(mime, 0.88);
  } catch {
    return null;
  }
}

function persistAvatarThumb(filename: string, url: string): void {
  if (typeof localStorage === "undefined") return;
  void (async () => {
    let thumb = url;
    if (url.length > AVATAR_THUMB_INLINE_MAX) {
      const shrunk = await toThumbDataUrl(url, AVATAR_THUMB_EDGE);
      if (!shrunk) return;
      thumb = shrunk;
    }
    const map = readAvatarThumbs();
    delete map[filename];
    map[filename] = thumb;
    for (const key of Object.keys(map)) {
      if (Object.keys(map).length <= AVATAR_THUMB_MAX) break;
      delete map[key];
    }
    try {
      localStorage.setItem(AVATAR_THUMB_KEY, JSON.stringify(map));
    } catch {
      // 存不下就算了：下次冷启动退回异步解析路径
    }
  })();
}

/**
 * 旧安装的大头像一次性收缩：移动端头像是直接存进 settings 的 dataURL，v0.8.1 之前
 * 上传的没有压缩闸，几 MB 的原文会撑爆 localStorage 的资料缓存（头像缓存不进去 →
 * 冷启动闪默认头像），还把 settings.json 与每轮同步载荷拖大。水合后发现超过阈值就
 * 压到与新上传同一口径（256px）再写回——显示尺寸下肉眼无差，用户无感。
 */
export async function oversizedAvatarShrink(avatar: string): Promise<string | null> {
  if (!avatar.startsWith("data:") || avatar.length <= 400_000) return null;
  return toThumbDataUrl(avatar, 256);
}

// filename -> resolved displayable URL (asset-protocol URL, not base64)
export const imageCache = writable<Record<string, string>>({});
// 头像缓存用缩略图做同步种子：冷启动第一帧就能画出头像（见上）
export const avatarCache = writable<Record<string, string>>(readAvatarThumbs());
export const mdImageCache = writable<Record<string, string>>({});

const pending = new Set<string>();
const attempts = new Map<string, number>();

/**
 * 缓存写入的字节预算（按字符数估，base64 一个字符就是一字节）。
 *
 * 三个缓存早先只增不减：`dataUrlImages` 平台（Linux 桌面 + Android）缓存的是 base64 正文，
 * 一张几 MB 的图就是几 MB 的 JS 堆，一次长会话滚过几百张插图能吃到几百 MB 且永不释放。
 * **Windows / macOS 不受影响**——那两边缓存的是 asset 协议的短 URL（百来字节），
 * 预算永远碰不到，行为与从前逐字节一致。
 *
 * 预算刻意给得很宽：淘汰会让 `resolveMarkdownImages` 回退成裸文件名（一帧破图）并立刻
 * 触发重新加载，预算小于「当前可见的工作集」就会来回抖动。
 */
const MD_CACHE_BUDGET = 64 * 1024 * 1024;
const BACKGROUND_CACHE_BUDGET = 32 * 1024 * 1024;
const AVATAR_CACHE_BUDGET = 8 * 1024 * 1024;

/** 写一条缓存，超出预算就从最旧的键开始淘汰（对象的字符串键保持插入顺序）。
 *  刚写入的那一条永远保留——哪怕它自己就超预算（一张图不可能被拆小）。 */
function boundedPut(
  cache: Writable<Record<string, string>>,
  key: string,
  url: string,
  budget: number
): void {
  cache.update((map) => {
    const next = { ...map, [key]: url };
    let total = 0;
    for (const value of Object.values(next)) total += value.length;
    if (total > budget) {
      for (const candidate of Object.keys(next)) {
        if (total <= budget || candidate === key) continue;
        total -= next[candidate].length;
        delete next[candidate];
      }
    }
    return next;
  });
}

function load(key: string, loader: () => Promise<string>, apply: (url: string) => void): void {
  if (pending.has(key)) return;
  pending.add(key);
  loader()
    .then((url) => {
      attempts.delete(key);
      apply(url);
    })
    .catch(() => {
      const tries = attempts.get(key) ?? 0;
      const delay = RETRY_DELAYS[tries];
      attempts.set(key, tries + 1);
      // 三次都读不到才算放弃：等下一次 retryFailedImages()（同步收尾时调）再从头来
      if (delay !== undefined) window.setTimeout(() => load(key, loader, apply), delay);
    })
    .finally(() => pending.delete(key));
}

/**
 * 同步一轮结束后调：清掉失败计数并轻推一下三个缓存 store（内容不变、只换对象身份），
 * 于是订阅它们的组件重算一次 resolve*，还没解析出来的图（多半是刚才字节没到）重新试一轮。
 * 已经缓存住的一张都不动——不会每轮同步把所有图重新解码一遍。
 */
export function retryFailedImages(): void {
  if (attempts.size === 0) return;
  attempts.clear();
  imageCache.update((map) => ({ ...map }));
  avatarCache.update((map) => ({ ...map }));
  mdImageCache.update((map) => ({ ...map }));
}

export function isLocalImageRef(ref?: string): ref is string {
  return typeof ref === "string" && ref.startsWith(LOCAL_PREFIX);
}

export function localImageFilename(ref: string): string {
  return ref.slice(LOCAL_PREFIX.length);
}

export function localImageRef(filename: string): string {
  return `${LOCAL_PREFIX}${filename}`;
}

/** Seed the cache immediately after an upload so the image renders without a round-trip. */
export function primeImageCache(filename: string, url: string): void {
  boundedPut(imageCache, filename, url, BACKGROUND_CACHE_BUDGET);
}

function ensureLoaded(filename: string): void {
  if (!filename || filename in get(imageCache)) return;
  load(
    `bg:${filename}`,
    () => backgroundImageUrl(filename),
    (url) => boundedPut(imageCache, filename, url, BACKGROUND_CACHE_BUDGET)
  );
}

/**
 * Resolve a background image reference to a displayable URL.
 * Local refs (`img:<file>`) are served from disk via the asset protocol (no base64);
 * pass the current cache (from the imageCache store) so this stays reactive.
 * Returns "" while a local image resolves.
 */
export function resolveImageSrc(ref: string | undefined, cache: Record<string, string>): string {
  if (!ref) {
    return "";
  }
  if (isLocalImageRef(ref)) {
    const filename = localImageFilename(ref);
    const cached = cache[filename];
    if (cached) {
      return cached;
    }
    ensureLoaded(filename);
    return "";
  }
  return ref;
}

// -- Avatar resolution --

function ensureAvatarLoaded(filename: string): void {
  if (!filename || filename in get(avatarCache)) return;
  load(
    `av:${filename}`,
    () => avatarImageUrl(filename),
    (url) => {
      boundedPut(avatarCache, filename, url, AVATAR_CACHE_BUDGET);
      persistAvatarThumb(filename, url);
    }
  );
}

/** 上传头像成功后立刻种缓存（不等下一次冷启动解析），并把缩略图落进首帧缓存。 */
export function primeAvatarCache(filename: string, url: string): void {
  boundedPut(avatarCache, filename, url, AVATAR_CACHE_BUDGET);
  persistAvatarThumb(filename, url);
}

export function isAvatarFilename(ref?: string): ref is string {
  if (!ref || ref.startsWith("data:") || ref.startsWith("http:") || ref.startsWith("https:")) return false;
  return !ref.includes("/") && !ref.includes("\\");
}

export function resolveAvatarSrc(ref: string | undefined, cache: Record<string, string>): string {
  if (!ref) return "";
  if (ref.startsWith("data:") || ref.startsWith("http:") || ref.startsWith("https:")) return ref;
  if (isAvatarFilename(ref)) {
    const cached = cache[ref];
    if (cached) return cached;
    ensureAvatarLoaded(ref);
    return "";
  }
  return ref;
}

// -- Markdown image resolution --

function mdCacheKey(nodeId: string, filename: string): string {
  return `${nodeId}/${filename}`;
}

function ensureMdImageLoaded(nodeId: string, filename: string): void {
  const key = mdCacheKey(nodeId, filename);
  if (!filename || key in get(mdImageCache)) return;
  load(
    `md:${key}`,
    () => mdImageUrl(nodeId, filename),
    (url) => boundedPut(mdImageCache, key, url, MD_CACHE_BUDGET)
  );
}

export function primeMdImageCache(nodeId: string, filename: string, url: string): void {
  const key = mdCacheKey(nodeId, filename);
  boundedPut(mdImageCache, key, url, MD_CACHE_BUDGET);
}

/**
 * 触发 markdown 里全部本地插图的异步加载（**不改写文本**）。
 *
 * 渲染走占位通道之后（见 markdown.ts::transformLocalImages），卡片挂载时调它预热：
 * 等用户展开卡片，字节多半已经在缓存里，占位符一挂上就被填掉。幂等——已缓存或
 * 在途的直接跳过。
 */
export function preloadMarkdownImages(markdown: string, nodeId: string): void {
  if (!markdown.includes("![")) return;
  for (const match of markdown.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)) {
    const src = match[1];
    if (!src || /^(https?:|data:|blob:)/i.test(src)) continue;
    ensureMdImageLoaded(nodeId, src);
  }
}

/**
 * 给 markdownWire 填图用：缓存里有就返回 URL；没有就触发（或经退避重试）加载并返回 null。
 * key 即 `data-md-img` 的值（`nodeId/filename`，两者都不含 `/`）。
 */
export function mdImageSrcForKey(key: string): string | null {
  const cached = get(mdImageCache)[key];
  if (cached) return cached;
  const at = key.indexOf("/");
  if (at <= 0 || at === key.length - 1) return null;
  ensureMdImageLoaded(key.slice(0, at), key.slice(at + 1));
  return null;
}

// -- 上传前的本地压缩 --

function readFileAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
    reader.readAsDataURL(file);
  });
}

/**
 * 把用户挑的图片压成小 dataURL（长边 ≤ maxEdge）。
 *
 * 移动端没有原生文件对话框，挑图走的就是 dataURL 这条路：一张手机原图能有几 MB，
 * 直接往上传的后果是三重的——① 几十 MB 的字符串过一次 IPC；② 落盘后每次冷启动都要
 * 读出来重新 base64（表现成「背景图先空一帧再出现」）；③ 头像那种直接存进 settings.json
 * 的还会撑爆 localStorage 的资料缓存，把名字邮箱一起拖去闪默认值。
 *
 * 出 PNG 还是 JPEG 看**源格式**（`opaque` 为真时一律 JPEG）：PNG/WebP/GIF 可能带透明
 * 通道，铺成 JPEG 会把透明背景变黑。解码失败、画布拿不到、压完反而更大，一律原样返回
 * ——压缩是优化，不许把用户的图弄丢。
 */
async function compressImageToDataUrl(
  file: File,
  maxEdge: number,
  { quality = 0.9, opaque = false }: { quality?: number; opaque?: boolean } = {}
): Promise<string> {
  const original = await readFileAsDataUrl(file);
  const keepAlpha = !opaque && /png|webp|gif|svg/i.test(file.type || "");
  try {
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, maxEdge / Math.max(bitmap.width, bitmap.height));
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return original;
    context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    const compressed = canvas.toDataURL(keepAlpha ? "image/png" : "image/jpeg", quality);
    // 「压完更大」也算失败：PNG 截图转 PNG 常常反而涨，那就原样留着
    return compressed && compressed.length < original.length ? compressed : original;
  } catch {
    return original;
  }
}

/** 头像：显示尺寸只有几十像素，256px 足够；带透明通道的源格式仍出 PNG。 */
export function compressAvatarImage(file: File): Promise<string> {
  return compressImageToDataUrl(file, 256);
}

/** 列表背景：铺满窗口，2560 长边在手机与桌面上都看不出差别；背景不透明，一律 JPEG。 */
export function compressBackgroundImage(file: File): Promise<string> {
  return compressImageToDataUrl(file, BACKGROUND_MAX_EDGE, { opaque: true });
}

/** 背景图长边上限。与 Rust 侧 `ImageGate::Background` 同一个值，改要一起改。 */
export const BACKGROUND_MAX_EDGE = 2560;

/**
 * Resolve markdown ![](filename) references to asset-protocol URLs.
 * Called with the raw markdown and the node ID to resolve local image references.
 */
export function resolveMarkdownImages(markdown: string, nodeId: string, cache: Record<string, string>): string {
  return markdown.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (match, alt, src) => {
    if (!src || src.startsWith("http://") || src.startsWith("https://") || src.startsWith("data:")) {
      return match;
    }
    const key = mdCacheKey(nodeId, src);
    const cached = cache[key];
    if (cached) return `![${alt}](${cached})`;
    ensureMdImageLoaded(nodeId, src);
    return `![${alt}](${src})`;
  });
}

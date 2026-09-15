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

// filename -> resolved displayable URL (asset-protocol URL, not base64)
export const imageCache = writable<Record<string, string>>({});
export const avatarCache = writable<Record<string, string>>({});
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
    (url) => boundedPut(avatarCache, filename, url, AVATAR_CACHE_BUDGET)
  );
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

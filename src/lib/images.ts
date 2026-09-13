import { writable, get } from "svelte/store";
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
  imageCache.update((map) => ({ ...map, [filename]: url }));
}

function ensureLoaded(filename: string): void {
  if (!filename || filename in get(imageCache)) return;
  load(
    `bg:${filename}`,
    () => backgroundImageUrl(filename),
    (url) => imageCache.update((map) => ({ ...map, [filename]: url }))
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
    (url) => avatarCache.update((map) => ({ ...map, [filename]: url }))
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
    (url) => mdImageCache.update((map) => ({ ...map, [key]: url }))
  );
}

export function primeMdImageCache(nodeId: string, filename: string, url: string): void {
  const key = mdCacheKey(nodeId, filename);
  mdImageCache.update((map) => ({ ...map, [key]: url }));
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

import { writable, get } from "svelte/store";
import { backgroundImageUrl, avatarImageUrl, mdImageUrl } from "./backend";

const LOCAL_PREFIX = "img:";

// filename -> resolved displayable URL (asset-protocol URL, not base64)
export const imageCache = writable<Record<string, string>>({});

const pending = new Set<string>();

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
  if (!filename || pending.has(filename) || filename in get(imageCache)) {
    return;
  }
  pending.add(filename);
  backgroundImageUrl(filename)
    .then((url) => imageCache.update((map) => ({ ...map, [filename]: url })))
    .catch(() => {
      /* missing image file; leave unresolved */
    })
    .finally(() => pending.delete(filename));
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

const avatarPending = new Set<string>();

export const avatarCache = writable<Record<string, string>>({});

export function isAvatarFilename(ref?: string): ref is string {
  if (!ref || ref.startsWith("data:") || ref.startsWith("http:") || ref.startsWith("https:")) return false;
  return !ref.includes("/") && !ref.includes("\\");
}

function ensureAvatarLoaded(filename: string): void {
  if (!filename || avatarPending.has(filename) || filename in get(avatarCache)) return;
  avatarPending.add(filename);
  avatarImageUrl(filename)
    .then((url) => avatarCache.update((map) => ({ ...map, [filename]: url })))
    .catch(() => {})
    .finally(() => avatarPending.delete(filename));
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

const mdPending = new Set<string>();
export const mdImageCache = writable<Record<string, string>>({});

function mdCacheKey(nodeId: string, filename: string): string {
  return `${nodeId}/${filename}`;
}

function ensureMdImageLoaded(nodeId: string, filename: string): void {
  const key = mdCacheKey(nodeId, filename);
  if (!filename || mdPending.has(key) || key in get(mdImageCache)) return;
  mdPending.add(key);
  mdImageUrl(nodeId, filename)
    .then((url) => mdImageCache.update((map) => ({ ...map, [key]: url })))
    .catch(() => {})
    .finally(() => mdPending.delete(key));
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

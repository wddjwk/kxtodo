import { writable, get } from "svelte/store";
import { backgroundImageUrl } from "./backend";

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

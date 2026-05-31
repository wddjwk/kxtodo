import { writable, get } from "svelte/store";
import { loadBackgroundImage } from "./backend";

const LOCAL_PREFIX = "img:";

// filename -> resolved displayable data URL
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
export function primeImageCache(filename: string, dataUrl: string): void {
  imageCache.update((map) => ({ ...map, [filename]: dataUrl }));
}

function ensureLoaded(filename: string): void {
  if (!filename || pending.has(filename) || filename in get(imageCache)) {
    return;
  }
  pending.add(filename);
  loadBackgroundImage(filename)
    .then((dataUrl) => imageCache.update((map) => ({ ...map, [filename]: dataUrl })))
    .catch(() => {
      /* missing image file; leave unresolved */
    })
    .finally(() => pending.delete(filename));
}

/**
 * Resolve a background image reference to a displayable URL.
 * Local refs (`img:<file>`) are loaded lazily; pass the current cache (from the
 * imageCache store) so this stays reactive. Returns "" while a local image loads.
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

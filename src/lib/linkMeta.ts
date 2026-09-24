/** Pure, DOM/component-free boundary for untrusted HTTP and rendered-page metadata. */
export type LinkMeta = {
  url: string;
  site: string;
  title: string;
  description: string;
  icon?: string;
};

export const LINK_META_MAX_BYTES = 16 * 1024;
const URL_MAX = 4096;

/** External web URLs only; never expose the application origin or native protocols. */
export function externalLinkUrl(value: unknown, base?: string): string | null {
  if (typeof value !== "string" || value.length > URL_MAX || /[\u0000-\u0020\u007f]/.test(value.trim())) return null;
  try {
    const url = base ? new URL(value.trim(), base) : new URL(value.trim());
    const host = url.hostname.toLowerCase().replace(/\.$/, "");
    if (!/^https?:$/.test(url.protocol) || !host || url.username || url.password) return null;
    if (host === "localhost" || host.endsWith(".localhost") || host === "[::1]" || host === "[::]" || /^(127\.|0\.)/.test(host)) return null;
    return url.href.length <= URL_MAX ? url.href : null;
  } catch {
    return null;
  }
}

export function normalizeLinkUrl(value: unknown): string | null {
  const safe = externalLinkUrl(value);
  if (!safe) return null;
  const url = new URL(safe);
  url.hash = "";
  return url.href;
}

function text(value: unknown, max: number): string {
  if (typeof value !== "string") return "";
  const cleaned = value.replace(/[\u0000-\u001f\u007f]/g, " ").replace(/\s+/g, " ").trim();
  const chars = Array.from(cleaned);
  return chars.length > max ? chars.slice(0, max).join("") + "…" : cleaned;
}

/** `raw.url` can supply an icon base after redirects, never the cache key/link target. */
export function normalizeLinkMeta(requestUrl: string, raw: unknown): LinkMeta | null {
  const url = normalizeLinkUrl(requestUrl);
  if (!url) return null;
  if (typeof raw === "string") {
    if (raw.length > LINK_META_MAX_BYTES || new TextEncoder().encode(raw).length > LINK_META_MAX_BYTES) return null;
    try { raw = JSON.parse(raw); } catch { return null; }
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const input = raw as Record<string, unknown>;
  const fields = [input.url, input.site, input.title, input.description, input.icon];
  // Bound before allocating/truncating (also applies to already-decoded IPC values).
  if (fields.some((v) => v != null && (typeof v !== "string" || v.length > LINK_META_MAX_BYTES))) return null;
  if (fields.reduce<number>((n, v) => n + (typeof v === "string" ? new TextEncoder().encode(v).length : 0), 0) > LINK_META_MAX_BYTES) return null;
  const title = text(input.title, 200);
  const description = text(input.description, 400);
  if (!title && !description) return null;
  // A normally rendered challenge is not page metadata. Do not attempt to solve it.
  if (/^(just a moment[.…]*|checking your browser[.…]*|verify you are human[.!…]*|loading[.…]*|attention required!?\s*\|\s*cloudflare)$/i.test(title)) return null;
  const base = externalLinkUrl(input.url) ?? url;
  const icon = typeof input.icon === "string" && input.icon.trim()
    ? externalLinkUrl(input.icon, base) ?? ""
    : new URL("/favicon.ico", base).href;
  return { url, title, description, site: text(input.site, 100) || new URL(url).hostname.replace(/^www\./, ""), icon };
}

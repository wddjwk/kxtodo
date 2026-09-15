/**
 * 超链接增强（v0.7.7），两个特性开关控制：
 * ① **自动解析标题**（features.autoLinkTitle，默认开）：只认「裸链接」——
 *    用户敲的是地址本身（GFM 自动链接），抓来网页标题按 [标题](链接) 的样子渲染，
 *    标题最长 30 字、超出补省略号；抓不到就原样留着。**用户手写的 [文字](链接)
 *    一律不动**——他写什么就是什么。
 * ② **渲染为卡片**（features.linkCards，默认关）：所有 http(s) 超链接都换成一张
 *    预览卡（[链接图标] 站点 + 复制链接按钮 / 标题 / 正文预览）。抓不到元数据就
 *    退回原样链接。
 *
 * 元数据在核心侧抓（`gui.link-meta`，Rust 抓 + 解析 + 落盘缓存）；浏览器 dev 预览
 * 没有 Tauri，直接在页面里 fetch（测试可用 Playwright 的 route 打桩）。
 * 这里再叠一层会话缓存与并发闸门：一篇文档里几十个链接也只按 4 条并发地抓，
 * 失败的结果 60 秒内不重试（免得每次重渲都去戳网络）。
 */
import { get } from "svelte/store";
import { isTauriRuntime, coreDispatch, type CoreEnvelope } from "./backend";
import { appSettings, coreMode } from "./stores";
import { copyText } from "./clipboard";

export type LinkMeta = {
  url: string;
  site: string;
  title: string;
  description: string;
};

/** 标题上限：超了补省略号（用户点名 30 字） */
const TITLE_MAX = 30;
/** 并发闸门：同时最多抓这么多条 */
const MAX_INFLIGHT = 4;
/** 失败结果的重试间隔（成功结果本会话不再重抓） */
const FAIL_TTL_MS = 60_000;

type CacheEntry = { value: LinkMeta | null; at: number };

const metaCache = new Map<string, CacheEntry>();
const pending = new Map<string, Promise<LinkMeta | null>>();
let inflight = 0;
const waiters: Array<() => void> = [];

async function withSlot<T>(task: () => Promise<T>): Promise<T> {
  if (inflight >= MAX_INFLIGHT) {
    await new Promise<void>((resolve) => waiters.push(resolve));
  }
  inflight += 1;
  try {
    return await task();
  } finally {
    inflight -= 1;
    waiters.shift()?.();
  }
}

/** 浏览器 dev 预览的兜底抓取（核心侧那份解析器在 crates/core/src/linkmeta.rs） */
async function fetchInPage(url: string): Promise<LinkMeta | null> {
  try {
    const response = await fetch(url, { redirect: "follow" });
    if (!response.ok) return null;
    const html = await response.text();
    const doc = new DOMParser().parseFromString(html, "text/html");
    const pick = (selector: string): string =>
      (doc.querySelector(selector)?.getAttribute("content") ?? "").trim();
    const title =
      pick("meta[property='og:title']") ||
      pick("meta[name='twitter:title']") ||
      (doc.querySelector("title")?.textContent ?? "").trim();
    const description =
      pick("meta[property='og:description']") || pick("meta[name='description']");
    const site = pick("meta[property='og:site_name']") || hostOf(url);
    if (!title && !description) return null;
    return { url, site, title, description };
  } catch {
    return null;
  }
}

/** 取一个链接的元数据（会话缓存 + 失败 60 秒内不重试；失败不入标记位，
 *  等缓存过期后下一轮 enhanceLinks 会再试一次，不成功则一直保持原样链接）。 */
export async function loadLinkMeta(url: string): Promise<LinkMeta | null> {
  const cached = metaCache.get(url);
  if (cached && (cached.value !== null || Date.now() - cached.at < FAIL_TTL_MS)) {
    return cached.value;
  }
  const running = pending.get(url);
  if (running) return running;
  const task = withSlot(async (): Promise<LinkMeta | null> => {
    if (coreMode) {
      try {
        const envelope: CoreEnvelope<LinkMeta> = await coreDispatch<LinkMeta>("gui.link-meta", { url });
        return envelope.data ?? null;
      } catch {
        return null;
      }
    }
    if (!isTauriRuntime) return fetchInPage(url);
    return null;
  })
    .then((meta) => {
      metaCache.set(url, { value: meta, at: Date.now() });
      return meta;
    })
    .finally(() => pending.delete(url));
  pending.set(url, task);
  return task;
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** 裸链接 = 文字就是地址本身（用户没写 [文字](链接)）。 */
function isBareLink(anchor: HTMLAnchorElement): boolean {
  const href = anchor.getAttribute("href") ?? "";
  const text = (anchor.textContent ?? "").trim();
  if (!text || !href) return false;
  return normalizeUrlText(text) === normalizeUrlText(href);
}

function normalizeUrlText(value: string): string {
  return value
    .replace(/^https?:\/\//i, "")
    .replace(/^www\./i, "")
    .replace(/\/+$/, "")
    .toLowerCase();
}

function truncateTitle(text: string): string {
  const chars = [...text];
  if (chars.length <= TITLE_MAX) return text;
  return chars.slice(0, TITLE_MAX).join("") + "…";
}

type LinkMode = "card" | "title" | "none";

function modeFor(anchor: HTMLAnchorElement, features: { autoLinkTitle: boolean; linkCards: boolean }): LinkMode {
  const href = anchor.getAttribute("href") ?? "";
  if (!/^https?:\/\//i.test(href)) return "none";
  if (anchor.closest(".kx-link-card")) return "none";
  // 图片链接（[![](img)](url)）跳过：换成卡片等于把图丢了
  if (anchor.querySelector("img")) return "none";
  if (features.linkCards) return "card";
  if (features.autoLinkTitle && isBareLink(anchor)) return "title";
  return "none";
}

function linkIcon(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of [
    "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71",
    "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"
  ]) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    svg.appendChild(path);
  }
  return svg;
}

function line(className: string, text: string): HTMLSpanElement {
  const el = document.createElement("span");
  el.className = className;
  el.textContent = text;
  return el;
}

/** 把一条链接换成预览卡片：站点 + 复制按钮 / 标题 / 正文预览。
 *  标题与摘要一律走 textContent——网页内容是不可信输入，绝不拼 HTML。 */
function renderCard(anchor: HTMLAnchorElement, meta: LinkMeta): void {
  const href = anchor.getAttribute("href") ?? meta.url;
  const card = document.createElement("span");
  card.className = "kx-link-card";
  card.dataset.kxLink = "card";

  const main = document.createElement("a");
  main.className = "kx-link-card-main";
  main.href = href;
  main.title = href;
  main.dataset.kxLink = "card";
  const head = document.createElement("span");
  head.className = "kx-link-card-head";
  head.appendChild(linkIcon());
  head.appendChild(line("kx-link-card-site", meta.site || hostOf(href)));
  main.appendChild(head);
  if (meta.title) main.appendChild(line("kx-link-card-title", meta.title));
  if (meta.description) main.appendChild(line("kx-link-card-desc", meta.description));

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "kx-link-card-copy";
  copy.textContent = "复制链接";
  copy.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    void copyText(href).then((ok) => {
      copy.textContent = ok ? "已复制" : "复制失败";
      window.setTimeout(() => (copy.textContent = "复制链接"), 1500);
    });
  });

  card.append(main, copy);
  anchor.replaceWith(card);
}

/** 处理容器里的所有超链接（幂等：处理过的节点带 data-kx-link 标记；
 *  在途的带 data-kx-pending——同一条链接不会因为重渲被并发处理两遍）。 */
export async function enhanceLinks(root: HTMLElement): Promise<void> {
  const features = get(appSettings).features;
  if (!features) return;
  const anchors = [...root.querySelectorAll<HTMLAnchorElement>("a[href]")];
  for (const anchor of anchors) {
    const mode = modeFor(anchor, features);
    if (mode === "none" || anchor.dataset.kxLink === mode) continue;
    if (anchor.dataset.kxPending) continue;
    anchor.dataset.kxPending = "1";
    void applyTo(anchor, mode);
  }
}

async function applyTo(anchor: HTMLAnchorElement, mode: LinkMode): Promise<void> {
  const href = anchor.getAttribute("href") ?? "";
  try {
    const meta = await loadLinkMeta(href);
    // 抓的这会儿文档可能已经重渲过：节点没了就别动（也不标处理位，
    // 新节点会在下一轮 enhanceLinks 里重来一遍）
    if (!anchor.isConnected) return;
    if (!meta) return;
    if (mode === "card") {
      renderCard(anchor, meta);
      return;
    }
    if (meta.title) {
      anchor.dataset.kxLink = "title";
      anchor.textContent = truncateTitle(meta.title);
      anchor.title = href;
    }
  } finally {
    delete anchor.dataset.kxPending;
  }
}

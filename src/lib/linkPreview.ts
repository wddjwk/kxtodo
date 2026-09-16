/**
 * 超链接增强（v0.7.7）：由**一个三档单选**（features.linkRender）控制，默认「卡片」。
 * 早先「自动解析标题」与「渲染为卡片」是两个独立勾选框，四种组合里有两组意思一样
 * （都勾 = 只有卡片生效），语义不可预期；现在合成一档，不存在「都选」这种状态：
 * - `off`：原样链接；
 * - `title`：只认「裸链接」——用户敲的是地址本身（GFM 自动链接），抓来网页标题按
 *   [标题](链接) 的样子渲染，最长 60 字、超出补省略号。**用户手写的 [文字](链接)
 *   一律不动**——他写什么就是什么；
 * - `card`：所有 http(s) 超链接都换成一张预览卡（[链接图标] 站点 + 复制链接按钮 /
 *   标题 / 正文预览）。抓不到元数据就退回原样链接。
 *
 * 三档随时可以来回拨：增强是**可逆**的（见 `revertUnwanted`），拨完就地重跑一遍即可，
 * 不必等 markdown 重渲——渲染有记忆化，等重渲等于「设置改了、画面不动」。
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
import type { Settings } from "./types";

export type LinkMeta = {
  url: string;
  site: string;
  title: string;
  description: string;
  /** 网页自己的图标；空串 = 用默认的链接图标 */
  icon?: string;
};

/** 标题上限：超了补省略号（用户点名 60 字） */
const TITLE_MAX = 60;
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
    return { url, site, title, description, icon: iconInPage(doc, url) };
  } catch {
    return null;
  }
}

/** 网页图标（浏览器预览版）：`<link rel="icon">` 优先，否则同源 /favicon.ico */
function iconInPage(doc: Document, url: string): string {
  try {
    const base = new URL(url);
    const links = [...doc.querySelectorAll("link[rel]")];
    let fallback = "";
    for (const link of links) {
      const rel = (link.getAttribute("rel") ?? "").toLowerCase().split(/\s+/);
      if (!rel.includes("icon") && !rel.includes("apple-touch-icon")) continue;
      const href = (link.getAttribute("href") ?? "").trim();
      if (!href) continue;
      const resolved = new URL(href, base);
      if (!/^https?:$/.test(resolved.protocol)) continue;
      if (rel.includes("icon")) return resolved.toString();
      if (!fallback) fallback = resolved.toString();
    }
    return fallback || `${base.origin}/favicon.ico`;
  } catch {
    return "";
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

function modeFor(anchor: HTMLAnchorElement, render: Settings["features"]["linkRender"]): LinkMode {
  const href = anchor.getAttribute("href") ?? "";
  if (!/^https?:\/\//i.test(href)) return "none";
  if (anchor.closest(".kx-link-card")) return "none";
  // 图片链接（[![](img)](url)）跳过：换成卡片等于把图丢了
  if (anchor.querySelector("img")) return "none";
  if (render === "card") return "card";
  if (render === "title" && isBareLink(anchor)) return "title";
  return "none";
}

function iconSvg(paths: string[]): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of paths) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    svg.appendChild(path);
  }
  return svg;
}

function linkIcon(): SVGSVGElement {
  return iconSvg([
    "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71",
    "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"
  ]);
}

/// 复制图标：两个错开叠放的空心方框（业界通行画法，Lucide 的 `copy` 同款）。
/// 早先那版是一块方框加一段无头无尾的弧，看着像画坏了。
function copyIcon(): SVGSVGElement {
  return iconSvg([
    "M9 9h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2z",
    "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"
  ]);
}

function checkIcon(): SVGSVGElement {
  return iconSvg(["M4 12.5l5 5L20 6.5"]);
}

function line(className: string, text: string): HTMLSpanElement {
  const el = document.createElement("span");
  el.className = className;
  el.textContent = text;
  return el;
}

/** 增强是**破坏性**的：卡片档把整个 anchor 换成卡片容器，标题档覆写 textContent。
 *  不留后路就退不回「原样链接」——档位往低拨时画面会一动不动（渲染缓存让 markdown
 *  不重渲，增强过的节点能一直留到用户改正文为止）。两张表各存各的退路：
 *  卡片存**原节点**（属性与文字原封不动放回原位），标题存被覆写前的文字与 title
 *  （用户手写的 `[地址](地址 "提示")` 也算裸链接，它的 tooltip 不能被顺手抹掉）。 */
const cardOriginalAnchor = new WeakMap<HTMLElement, HTMLAnchorElement>();
const titleOriginal = new WeakMap<HTMLAnchorElement, { text: string; title: string }>();

/** 把一条链接换成预览卡片：站点行（[网页图标] 站点 + 悬浮在右上角的复制按钮）/
 *  标题（最多两行）/ 正文预览（最多两行）。
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
  head.appendChild(favicon(meta.icon, href));
  head.appendChild(line("kx-link-card-site", meta.site || hostOf(href)));
  main.appendChild(head);
  if (meta.title) main.appendChild(line("kx-link-card-title", meta.title));
  if (meta.description) main.appendChild(line("kx-link-card-desc", meta.description));

  // 复制按钮：悬浮在卡片右上角（绝对定位，不占正文的宽度），图标不带文字
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "kx-link-card-copy";
  copy.title = "复制链接";
  copy.setAttribute("aria-label", "复制链接");
  copy.appendChild(copyIcon());
  copy.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    void copyText(href).then((ok) => {
      if (!ok) return;
      copy.replaceChildren(checkIcon());
      copy.classList.add("done");
      window.setTimeout(() => {
        copy.replaceChildren(copyIcon());
        copy.classList.remove("done");
      }, 1500);
    });
  });

  card.append(main, copy);
  cardOriginalAnchor.set(card, anchor);
  anchor.replaceWith(card);
}

/** 卡片图标：网页给了 favicon 就用它（加载不出来回退默认链接图标）。 */
function favicon(icon: string | undefined, href: string): Element {
  if (!icon) return linkIcon();
  const img = document.createElement("img");
  img.className = "kx-link-card-favicon";
  img.src = icon;
  img.alt = "";
  img.loading = "lazy";
  // 尺寸由 CSS 定死（1em 的方框），加载中不占位也不跳版；失败就换回默认图标
  img.addEventListener("error", () => img.replaceWith(linkIcon()), { once: true });
  return img;
}

/** 卡片退回原样链接：原节点整个放回卡片的位置（属性、文字都是原来的）。 */
function revertCards(root: HTMLElement): void {
  for (const card of [...root.querySelectorAll<HTMLElement>(".kx-link-card[data-kx-link='card']")]) {
    const original = cardOriginalAnchor.get(card);
    // 没有退路的卡片不是这一轮增强出来的，不动它（宁可按原样留着，也不能把链接删掉）
    if (original) card.replaceWith(original);
  }
}

/** 标题退回原样链接：还原被覆写的文字与 title。 */
function revertTitles(root: HTMLElement): void {
  for (const anchor of [...root.querySelectorAll<HTMLAnchorElement>("a[data-kx-link='title']")]) {
    const original = titleOriginal.get(anchor);
    if (!original) continue;
    anchor.textContent = original.text;
    if (original.title) anchor.title = original.title;
    else anchor.removeAttribute("title");
    delete anchor.dataset.kxLink;
  }
}

/** 按当前档位退回不该存在的增强。退回后还要重新增强一遍（标题档下，刚从卡片退回来的
 *  裸链接得再变成标题），所以这一步必须排在收集 anchor 之前。 */
function revertUnwanted(root: HTMLElement, render: Settings["features"]["linkRender"]): void {
  if (render !== "card") revertCards(root);
  if (render === "off") revertTitles(root);
}

/** 处理容器里的所有超链接（幂等：处理过的节点带 data-kx-link 标记；
 *  在途的带 data-kx-pending——同一条链接不会因为重渲被并发处理两遍）。
 *  档位往低拨时先退回旧增强，所以「拨完就地重跑」是真的能重跑出正确画面。 */
export async function enhanceLinks(root: HTMLElement): Promise<void> {
  const features = get(appSettings).features;
  if (!features) return;
  revertUnwanted(root, features.linkRender);
  const anchors = [...root.querySelectorAll<HTMLAnchorElement>("a[href]")];
  for (const anchor of anchors) {
    const mode = modeFor(anchor, features.linkRender);
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
    // 抓的这会儿档位也可能被拨走：只按**当前**档位落地，否则用户刚关掉的卡片
    // 会在抓取回来的那一刻又被画上去（而这一轮已经没有下一次重跑会去退它）
    if (modeFor(anchor, get(appSettings).features.linkRender) !== mode) return;
    if (mode === "card") {
      renderCard(anchor, meta);
      return;
    }
    if (meta.title) {
      titleOriginal.set(anchor, { text: anchor.textContent ?? "", title: anchor.getAttribute("title") ?? "" });
      anchor.dataset.kxLink = "title";
      anchor.textContent = truncateTitle(meta.title);
      anchor.title = href;
    }
  } finally {
    delete anchor.dataset.kxPending;
  }
}

/**
 * 全局悬浮提示（v0.9.1 需求 13）。
 *
 * 原来全应用的悬浮说明都是原生 `title`：延迟一秒、系统灰底小字、位置随机、
 * 移动端完全没有——「太原始」。但把几百处 `title=` 改成组件是巨大的改动面，
 * 而且以后每加一个按钮都要记得用新写法。
 *
 * 所以这里走**接管原生 title**的路子：一个 window 级的 `pointerover` 委托，
 * 命中带 `title` 的元素就把属性摘下来存进 `data-tip`（浏览器因此不会画原生提示），
 * 延迟 380ms 后画我们自己的浮层；`pointerout` 时原样还回去。
 * 对调用方零改动、对无障碍零损失（属性只在悬浮那一小段时间里不在 DOM 上）。
 *
 * 定位复用全应用唯一的一份浮层几何（`popover.ts::placePopover` + `popoverFrame`）：
 * 元素下方居中、放不下翻到上方、四边钳进壳内，坐标一律按壳的 `scale(uiScale)` 换算。
 */
import { get } from "svelte/store";
import { appSettings } from "./stores";
import { uiScaleValue } from "./styles";
import { placePopover, popoverFrame } from "./popover";

/** 出现延迟：比原生（约 1s）快，又不是一扫而过就闪 */
const SHOW_DELAY_MS = 380;
/** 与元素的间距（视觉像素） */
const GAP_PX = 7;
/** 视口边缘留白（逻辑像素，与菜单同口径） */
const MARGIN_PX = 8;

let installed = false;
let tipEl: HTMLElement | null = null;
let current: HTMLElement | null = null;
let showTimer: number | undefined;
let shownFor: HTMLElement | null = null;
/**
 * 「刚点完、指针没动」的那一小段时间不弹提示（`armed = false`）。
 *
 * 两个理由：① 用户刚点过的控件不需要再解释一遍（主流 UI 也是这个口径）；
 * ② 点完之后界面常在原地换成另一个控件（搜索按钮变「关闭搜索」），指针没动就会
 *    触发一次 pointerover——这时把 title 摘掉，任何按 `[title=…]` 找元素的代码
 *    （包括本仓库 170+ 处测试选择器）都会在那一瞬间找不到目标。
 * 指针真的移动超过 ARM_PX 之后立刻恢复。
 */
let armed = true;
let downAt: { x: number; y: number } | null = null;
const ARM_PX = 4;

function ensureTip(): HTMLElement | null {
  if (tipEl?.isConnected) return tipEl;
  // 挂进 .app-shell 才拿得到 --font-control 与壳的坐标系（与 ContextMenu 同一个约定）
  const host = document.querySelector<HTMLElement>(".app-shell") ?? document.body;
  const el = document.createElement("div");
  el.className = "kx-tooltip";
  el.setAttribute("role", "tooltip");
  host.appendChild(el);
  tipEl = el;
  return el;
}

/** 命中的目标：最近的、带 title（或已被我们接管）的元素 */
function targetOf(node: EventTarget | null): HTMLElement | null {
  if (!(node instanceof Element)) return null;
  const el = node.closest<HTMLElement>("[title], [data-tip]");
  if (!el || el.closest("[data-no-tooltip]")) return null;
  const text = el.getAttribute("data-tip") ?? el.getAttribute("title") ?? "";
  return text.trim() === "" ? null : el;
}

function restore(el: HTMLElement | null): void {
  if (!el) return;
  const stashed = el.getAttribute("data-tip");
  if (stashed !== null && !el.hasAttribute("title")) el.setAttribute("title", stashed);
  el.removeAttribute("data-tip");
}

function hideNow(): void {
  window.clearTimeout(showTimer);
  showTimer = undefined;
  restore(current);
  current = null;
  shownFor = null;
  if (tipEl) {
    tipEl.classList.remove("show");
    tipEl.style.visibility = "hidden";
  }
}

/**
 * 只是「开始悬停」：**先不动 title 属性**，等真要画我们的浮层时才摘（见 show）。
 * 这样快速扫过、点击都不改 DOM，`[title=...]` 选择器与无障碍名一直在。
 */
function adopt(el: HTMLElement): void {
  if (el === current) return;
  hideNow();
  const text = el.getAttribute("title") ?? "";
  if (text.trim() === "") return;
  current = el;
  showTimer = window.setTimeout(() => show(el, text), SHOW_DELAY_MS);
}

function show(el: HTMLElement, text: string): void {
  const tip = ensureTip();
  if (!tip || !el.isConnected) {
    hideNow();
    return;
  }
  // 到这一刻才摘掉 title：浏览器的原生提示就不会跟着冒出来（它有自己的 ~1s 延迟，拦不住）
  el.removeAttribute("title");
  el.setAttribute("data-tip", text);
  const scale = uiScaleValue(get(appSettings).appearance.uiScale);
  tip.textContent = text;
  tip.classList.remove("show");
  tip.style.visibility = "hidden";
  tip.style.left = "0px";
  tip.style.top = "0px";
  tip.style.maxHeight = "";

  const frame = popoverFrame(tip, scale);
  const width = tip.offsetWidth;
  const height = tip.offsetHeight;
  const rect = el.getBoundingClientRect();
  const centerX = (rect.left + rect.width / 2 - frame.x) / scale;
  const belowY = (rect.bottom + GAP_PX - frame.y) / scale;
  const aboveY = (rect.top - GAP_PX - frame.y) / scale;
  const size = { width, height };
  const view = { width: frame.width, height: frame.height };

  let placed = placePopover({ x: centerX, y: belowY }, size, view, { xAlign: "left", margin: MARGIN_PX });
  // 下方放不下（placePopover 翻上后 top 会小于锚点）：改锚到元素上沿，翻上去不压住元素
  if (placed.top < belowY) {
    placed = placePopover({ x: centerX, y: aboveY }, size, view, { xAlign: "left", margin: MARGIN_PX });
  }
  const left = Math.max(MARGIN_PX, Math.min(placed.left - width / 2, Math.max(MARGIN_PX, frame.width - MARGIN_PX - width)));
  tip.style.left = `${Math.round(left)}px`;
  tip.style.top = `${Math.round(placed.top)}px`;
  if (placed.maxHeight > 0) tip.style.maxHeight = `${Math.round(placed.maxHeight)}px`;
  tip.style.visibility = "visible";
  shownFor = el;
  // 下一帧再加 .show，过渡才有起点
  requestAnimationFrame(() => {
    if (shownFor === el) tip.classList.add("show");
  });
}

function handleOver(event: PointerEvent): void {
  // 触屏没有 hover：点一下弹个提示只会挡住内容
  if (event.pointerType !== "mouse") return;
  if (!armed) return;
  const el = targetOf(event.target);
  if (!el) {
    hideNow();
    return;
  }
  adopt(el);
}

function handleDown(event: PointerEvent): void {
  if (event.pointerType !== "mouse") return;
  armed = false;
  downAt = { x: event.clientX, y: event.clientY };
  hideNow();
}

function handleMove(event: PointerEvent): void {
  if (event.pointerType !== "mouse") return;
  if (armed) return;
  if (!downAt || Math.hypot(event.clientX - downAt.x, event.clientY - downAt.y) <= ARM_PX) return;
  armed = true;
  downAt = null;
  // 补一次采纳：一次「跳跃式」移动里 pointerover 可能先于 pointermove 到达，
  // 那时还没上保险，元素被放掉了——保险一上就把指针底下这个补回来。
  const el = targetOf(event.target);
  if (el) adopt(el);
}

function handleOut(event: PointerEvent): void {
  if (event.pointerType !== "mouse") return;
  const from = targetOf(event.target);
  if (!from) return;
  const to = targetOf(event.relatedTarget);
  if (to === from) return;
  hideNow();
}

/** 装上全局委托；返回卸载函数（App 只在挂载时装一次）。 */
export function installTooltips(): () => void {
  if (installed) return () => undefined;
  installed = true;
  window.addEventListener("pointerover", handleOver, true);
  window.addEventListener("pointerout", handleOut, true);
  window.addEventListener("pointerdown", handleDown, true);
  window.addEventListener("pointermove", handleMove, true);
  window.addEventListener("wheel", hideNow, true);
  window.addEventListener("scroll", hideNow, true);
  window.addEventListener("resize", hideNow);
  window.addEventListener("blur", hideNow);
  window.addEventListener("keydown", handleKeydown, true);
  document.addEventListener("visibilitychange", hideNow);
  return () => {
    installed = false;
    window.removeEventListener("pointerover", handleOver, true);
    window.removeEventListener("pointerout", handleOut, true);
    window.removeEventListener("pointerdown", handleDown, true);
    window.removeEventListener("pointermove", handleMove, true);
    window.removeEventListener("wheel", hideNow, true);
    window.removeEventListener("scroll", hideNow, true);
    window.removeEventListener("resize", hideNow);
    window.removeEventListener("blur", hideNow);
    window.removeEventListener("keydown", handleKeydown, true);
    document.removeEventListener("visibilitychange", hideNow);
    hideNow();
    tipEl?.remove();
    tipEl = null;
  };
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") hideNow();
}

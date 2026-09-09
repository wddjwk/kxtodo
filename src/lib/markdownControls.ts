//! markdown 渲染结果的交互接线：mermaid/markmap 异步填图、图的缩放/平移/捏合/全屏、
//! 代码块复制与折叠。组件只要在自己渲染 markdown 的容器上 `use:markdownWire`。
//!
//! 全部本地渲染：mermaid / markmap 走动态 import（只在真的出现对应代码块时才加载 chunk），
//! 不请求任何 CDN。

import type { Mermaid } from "mermaid";
import { decodeDiagramSource, isMarkmapLang } from "./markdown";
import { addBackInterceptor } from "./platform";

const diagramCache = new Map<string, string>();
let diagramSeq = 0;
let mermaidPromise: Promise<Mermaid> | null = null;

function mermaidInstance(): Promise<Mermaid> {
  if (!mermaidPromise) {
    mermaidPromise = import("mermaid").then((mod) => {
      const mermaid = mod.default;
      mermaid.initialize({ startOnLoad: false, theme: "neutral", securityLevel: "strict" });
      return mermaid;
    });
  }
  return mermaidPromise;
}

async function renderMermaid(source: string): Promise<string> {
  const mermaid = await mermaidInstance();
  diagramSeq += 1;
  const { svg } = await mermaid.render("kx-diagram-" + diagramSeq, source);
  return svg;
}

async function renderMarkmap(canvas: HTMLElement, source: string): Promise<void> {
  const [{ Transformer }, { Markmap }] = await Promise.all([import("markmap-lib"), import("markmap-view")]);
  const { root } = new Transformer().transform(source);
  canvas.innerHTML = '<svg class="markmap-svg"></svg>';
  const svg = canvas.querySelector("svg");
  if (!(svg instanceof SVGSVGElement)) return;
  // Markmap.create 把 d3 的缩放/平移/捏合挂在 svg 上——全屏不能克隆 DOM
  // （cloneNode 不带事件监听，克隆出来的脑图既拖不动也缩不了），必须拿源码重建 view。
  const view = Markmap.create(svg, { duration: 0, maxWidth: 240 }, root);
  view.fit();
}

async function fillDiagram(canvas: HTMLElement): Promise<void> {
  const lang = canvas.dataset.diagram ?? "";
  const source = decodeDiagramSource(canvas.dataset.source ?? "");
  if (canvas.dataset.rendered === "1") return;
  canvas.dataset.rendered = "1";
  try {
    if (isMarkmapLang(lang)) {
      await renderMarkmap(canvas, source);
      return;
    }
    const key = lang + "::" + source;
    let html = diagramCache.get(key);
    if (html === undefined) {
      html = await renderMermaid(source);
      diagramCache.set(key, html);
    }
    canvas.innerHTML = html;
    const svg = canvas.querySelector("svg");
    svg?.setAttribute("class", "diagram-svg");
  } catch (error) {
    canvas.textContent = "";
    const box = document.createElement("div");
    box.className = "diagram-error";
    box.textContent = "图渲染失败：" + String(error);
    canvas.appendChild(box);
  }
}

export function renderDiagramsIn(container: HTMLElement): void {
  container.querySelectorAll<HTMLElement>(".diagram-canvas[data-source]").forEach((canvas) => {
    void fillDiagram(canvas);
  });
}

// ---------------------------------------------------------------------------
// 缩放 / 平移 / 捏合
// ---------------------------------------------------------------------------

interface ViewState {
  scale: number;
  tx: number;
  ty: number;
}

const viewStates = new WeakMap<HTMLElement, ViewState>();

function viewState(canvas: HTMLElement): ViewState {
  let state = viewStates.get(canvas);
  if (!state) {
    state = { scale: 1, tx: 0, ty: 0 };
    viewStates.set(canvas, state);
  }
  return state;
}

function applyView(canvas: HTMLElement): void {
  const target = canvas.firstElementChild;
  if (!(target instanceof HTMLElement) && !(target instanceof SVGElement)) return;
  const state = viewState(canvas);
  (target as HTMLElement).style.transform =
    "translate(" + state.tx + "px, " + state.ty + "px) scale(" + state.scale + ")";
  (target as HTMLElement).style.transformOrigin = "center center";
}

function setScale(canvas: HTMLElement, scale: number): void {
  const state = viewState(canvas);
  state.scale = Math.min(5, Math.max(0.25, scale));
  applyView(canvas);
}

function zoom(canvas: HTMLElement, factor: number): void {
  setScale(canvas, viewState(canvas).scale * factor);
}

function resetView(canvas: HTMLElement): void {
  viewStates.set(canvas, { scale: 1, tx: 0, ty: 0 });
  applyView(canvas);
}

/** 拖拽/捏合期间禁止选中文本：指针在图上滑动时浏览器会顺手框选周围的字。 */
function setDragging(on: boolean): void {
  document.body.classList.toggle("kx-dragging", on);
}

// ---------------------------------------------------------------------------
// 全屏查看
// ---------------------------------------------------------------------------

let overlay: HTMLDivElement | null = null;

function ensureOverlay(): HTMLDivElement {
  if (overlay && overlay.isConnected) return overlay;
  overlay = document.createElement("div");
  overlay.className = "md-fullscreen-overlay";
  const bar = document.createElement("div");
  bar.className = "md-fullscreen-bar";
  const hint = document.createElement("span");
  hint.textContent = "滚轮或捏合缩放 · 拖拽平移 · Esc 退出";
  const close = document.createElement("button");
  close.type = "button";
  close.className = "md-fullscreen-close";
  close.textContent = "退出全屏";
  close.addEventListener("click", () => closeFullscreen());
  bar.append(hint, close);
  const stage = document.createElement("div");
  stage.className = "md-fullscreen-stage";
  overlay.append(bar, stage);
  // 挂进 app-shell（而不是 body）：移动端的安全区变量 --safe-inv 定义在它上面，
  // 工具栏才能避让状态栏；app-shell 不在（SSR/测试）才退回 body。
  const shell = document.querySelector(".app-shell");
  (shell ?? document.body).appendChild(overlay);
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || !overlay?.classList.contains("active")) return;
    // 这记 Esc 只属于全屏：不拦的话编辑器的 window keydown 会接着把编辑器也关掉
    event.preventDefault();
    event.stopPropagation();
    closeFullscreen();
  });
  // 移动端返回键/返回手势：全屏开着时先收全屏，不把它漏给历史栈（漏了会连编辑器一起弹掉，
  // 全屏罩子却还留在原地，界面看起来就是「卡住」）。
  addBackInterceptor(() => {
    if (overlay?.classList.contains("active")) {
      closeFullscreen();
      return true;
    }
    return false;
  });
  return overlay;
}

function closeFullscreen(): void {
  overlay?.classList.remove("active");
  setDragging(false);
}

function openFullscreen(canvas: HTMLElement): void {
  const box = ensureOverlay();
  const stage = box.querySelector(".md-fullscreen-stage");
  if (!stage) return;
  stage.innerHTML = "";
  const lang = canvas.dataset.diagram ?? "";
  if (isMarkmapLang(lang)) {
    // markmap 的交互是 d3 挂在 svg 上的监听，克隆 DOM 不带监听——全屏里拿源码重建
    const fresh = document.createElement("div");
    fresh.className = "diagram-canvas";
    fresh.dataset.diagram = lang;
    fresh.dataset.source = canvas.dataset.source ?? "";
    stage.appendChild(fresh);
    void fillDiagram(fresh);
  } else {
    const clone = canvas.cloneNode(true) as HTMLElement;
    clone.removeAttribute("data-rendered");
    clone.dataset.rendered = "1";
    stage.appendChild(clone);
  }
  box.classList.add("active");
  wireInteractions(box);
}

// ---------------------------------------------------------------------------
// 事件委托
// ---------------------------------------------------------------------------

const panSessions = new WeakMap<HTMLElement, { x: number; y: number }>();
const touchPoints = new WeakMap<HTMLElement, Map<number, { x: number; y: number }>>;
const pinchSessions = new WeakMap<
  HTMLElement,
  { dist: number; cx: number; cy: number; scale: number; tx: number; ty: number }
>();

function pointMap(canvas: HTMLElement): Map<number, { x: number; y: number }> {
  let map = touchPoints.get(canvas);
  if (!map) {
    map = new Map();
    touchPoints.set(canvas, map);
  }
  return map;
}

function canvasOf(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  return target.closest<HTMLElement>(".diagram-canvas");
}

function handleClick(event: MouseEvent, root: HTMLElement): void {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const button = target.closest("button");
  if (!button || !root.contains(button)) return;
  const frame = button.closest(".diagram-frame");
  const codeBlock = button.closest(".code-block");
  if (button.classList.contains("code-copy")) {
    const code = codeBlock?.querySelector("pre code");
    const text = code?.textContent ?? "";
    void copyText(text).then((ok) => {
      button.textContent = ok ? "已复制" : "复制失败";
      window.setTimeout(() => (button.textContent = "复制"), 1500);
    });
    event.stopPropagation();
    return;
  }
  if (button.classList.contains("code-toggle")) {
    const block = codeBlock;
    if (!block) return;
    const collapsed = block.classList.toggle("collapsed");
    button.textContent = collapsed ? "展开" : "收起";
    event.stopPropagation();
    return;
  }
  if (!frame) return;
  const canvas = frame.querySelector<HTMLElement>(".diagram-canvas");
  if (!canvas) return;
  if (button.classList.contains("diagram-zoom-in")) zoom(canvas, 1.2);
  else if (button.classList.contains("diagram-zoom-out")) zoom(canvas, 1 / 1.2);
  else if (button.classList.contains("diagram-reset")) resetView(canvas);
  else if (button.classList.contains("diagram-fullscreen")) openFullscreen(canvas);
  else if (button.classList.contains("diagram-source-toggle")) {
    const source = frame.querySelector("pre.diagram-source");
    if (source) {
      const hidden = source.hasAttribute("hidden");
      if (hidden) source.removeAttribute("hidden");
      else source.setAttribute("hidden", "");
      button.textContent = hidden ? "收起源码" : "源码";
    }
  }
  event.stopPropagation();
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      const ok = document.execCommand("copy");
      area.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

function handleWheel(event: WheelEvent, root: HTMLElement): void {
  const canvas = canvasOf(event.target);
  if (!canvas || !root.contains(canvas)) return;
  if (isMarkmapLang(canvas.dataset.diagram ?? "")) return; // markmap 自带缩放
  event.preventDefault();
  event.stopPropagation();
  zoom(canvas, Math.exp(-event.deltaY * 0.0012));
}

function handlePointerDown(event: PointerEvent, root: HTMLElement): void {
  const target = event.target;
  // 拖图片（渲染出来的插图）时同样不许框选文本
  const image = target instanceof Element ? target.closest("img") : null;
  const canvas = canvasOf(target);
  if (!canvas || !root.contains(canvas)) {
    if (image && root.contains(image)) {
      event.preventDefault();
      setDragging(true);
      const stop = (): void => {
        setDragging(false);
        window.removeEventListener("pointerup", stop);
        window.removeEventListener("pointercancel", stop);
      };
      window.addEventListener("pointerup", stop);
      window.addEventListener("pointercancel", stop);
    }
    return;
  }
  if (isMarkmapLang(canvas.dataset.diagram ?? "")) return; // markmap 自带拖拽/捏合
  if (event.button !== 0 && event.pointerType === "mouse") return;
  // 接管这次指针：阻止浏览器起文本选区 / 原生图片拖影
  event.preventDefault();
  setDragging(true);
  const points = pointMap(canvas);
  points.set(event.pointerId, { x: event.clientX, y: event.clientY });
  canvas.setPointerCapture?.(event.pointerId);
  if (points.size === 2) {
    const [a, b] = [...points.values()];
    const state = viewState(canvas);
    pinchSessions.set(canvas, {
      dist: Math.hypot(a.x - b.x, a.y - b.y) || 1,
      cx: (a.x + b.x) / 2,
      cy: (a.y + b.y) / 2,
      scale: state.scale,
      tx: state.tx,
      ty: state.ty
    });
    panSessions.delete(canvas);
    return;
  }
  panSessions.set(canvas, { x: event.clientX, y: event.clientY });
}

function handlePointerMove(event: PointerEvent, root: HTMLElement): void {
  const canvas = canvasOf(event.target);
  if (!canvas || !root.contains(canvas)) return;
  const points = pointMap(canvas);
  if (!points.has(event.pointerId)) return;
  points.set(event.pointerId, { x: event.clientX, y: event.clientY });
  const pinch = pinchSessions.get(canvas);
  if (pinch && points.size >= 2) {
    // 移动端捏合：两指距离比 = 缩放比，质心位移 = 平移
    const [a, b] = [...points.values()];
    const dist = Math.hypot(a.x - b.x, a.y - b.y) || 1;
    const cx = (a.x + b.x) / 2;
    const cy = (a.y + b.y) / 2;
    const state = viewState(canvas);
    state.scale = Math.min(5, Math.max(0.25, pinch.scale * (dist / pinch.dist)));
    state.tx = pinch.tx + (cx - pinch.cx);
    state.ty = pinch.ty + (cy - pinch.cy);
    applyView(canvas);
    event.preventDefault();
    return;
  }
  const start = panSessions.get(canvas);
  if (!start) return;
  event.preventDefault();
  const state = viewState(canvas);
  state.tx += event.clientX - start.x;
  state.ty += event.clientY - start.y;
  panSessions.set(canvas, { x: event.clientX, y: event.clientY });
  applyView(canvas);
}

function handlePointerUp(event: PointerEvent, root: HTMLElement): void {
  const canvas = canvasOf(event.target);
  if (!canvas || !root.contains(canvas)) return;
  const points = pointMap(canvas);
  points.delete(event.pointerId);
  if (points.size < 2) pinchSessions.delete(canvas);
  if (points.size === 0) {
    panSessions.delete(canvas);
    setDragging(false);
  } else if (points.size === 1) {
    const [left] = [...points.values()];
    panSessions.set(canvas, { x: left.x, y: left.y });
  }
}

const wired = new WeakSet<HTMLElement>();

function wireInteractions(root: HTMLElement): void {
  if (wired.has(root)) return;
  wired.add(root);
  root.addEventListener("click", (event) => handleClick(event, root));
  root.addEventListener("wheel", (event) => handleWheel(event, root), { passive: false });
  root.addEventListener("pointerdown", (event) => handlePointerDown(event, root));
  root.addEventListener("pointermove", (event) => handlePointerMove(event, root));
  root.addEventListener("pointerup", (event) => handlePointerUp(event, root));
  root.addEventListener("pointercancel", (event) => handlePointerUp(event, root));
}

/** Svelte action：渲染 markdown 的容器挂上它，图与代码块的交互就都有了。 */
export function markdownWire(node: HTMLElement): { update: () => void; destroy: () => void } {
  wireInteractions(node);
  renderDiagramsIn(node);
  // {@html} 重渲会换掉 canvas 节点，而无参 action 的 update 不会被调用——
  // 用 MutationObserver 盯住子树，新占位框一出现就填（fillDiagram 有 rendered 守卫）。
  const observer =
    typeof MutationObserver !== "undefined"
      ? new MutationObserver(() => renderDiagramsIn(node))
      : null;
  observer?.observe(node, { childList: true, subtree: true });
  return {
    update() {
      renderDiagramsIn(node);
    },
    destroy() {
      observer?.disconnect();
    }
  };
}
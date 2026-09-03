import { get } from "svelte/store";
import { isMobile } from "./platform";

const HOLD_MS = 500;
const MOVE_TOLERANCE_PX = 10;
const SUPPRESS_MS = 700;

let suppressedUntil = 0;

/**
 * Chromium 在触摸长按后会补发一个原生 contextmenu 事件。长按处理器已经打开了
 * 菜单，contextmenu handler 用这个标志去重，避免同一手势开两次菜单。
 */
export function isLongPressSuppressed(): boolean {
  return Date.now() < suppressedUntil;
}

export type LongPressPosition = { x: number; y: number };

/**
 * 触摸长按 Svelte action：按住 500ms 触发 handler（携带原始 pointerdown 坐标，
 * 菜单以触点为锚）。仅移动端 + pointerType === "touch" 生效，桌面右键路径不受
 * 影响。不 preventDefault touchstart/pointerdown，滚动与点击行为保持原样；
 * 文本选择由 mobile.css 的 user-select 规则抑制。
 */
export function longpress(
  node: HTMLElement,
  handler: (pos: LongPressPosition) => void
): { destroy(): void } {
  let timer: number | null = null;
  let startX = 0;
  let startY = 0;
  let active = false;

  function clearTimer(): void {
    if (timer !== null) {
      window.clearTimeout(timer);
      timer = null;
    }
  }

  function release(): void {
    clearTimer();
    active = false;
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerUp);
  }

  function onPointerMove(event: PointerEvent): void {
    if (!active) return;
    const distance = Math.hypot(event.clientX - startX, event.clientY - startY);
    if (distance > MOVE_TOLERANCE_PX) {
      release();
    }
  }

  function onPointerUp(): void {
    release();
  }

  function onPointerDown(event: PointerEvent): void {
    if (!get(isMobile) || event.pointerType !== "touch") return;
    // 交互控件（勾选框、编辑笔、标签等）不参与长按开菜单。
    const target = event.target as Element | null;
    if (target?.closest("button, input, textarea, a, select")) return;
    release();
    active = true;
    startX = event.clientX;
    startY = event.clientY;
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    timer = window.setTimeout(() => {
      if (!active) return;
      release();
      suppressedUntil = Date.now() + SUPPRESS_MS;
      handler({ x: startX, y: startY });
    }, HOLD_MS);
  }

  node.addEventListener("pointerdown", onPointerDown);

  return {
    destroy() {
      release();
      node.removeEventListener("pointerdown", onPointerDown);
    }
  };
}

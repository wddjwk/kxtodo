import { touchOnly } from "./platform";

/**
 * 移动端「幽灵点击」抑制。
 *
 * 浮层在 pointerdown 阶段就把自己拆掉（点遮罩关闭、点保存键落盘后关闭），浏览器随后
 * 补发的那一个 click 会落在同一坐标上此刻最顶层的元素——通常正是浮层下面的东西：
 * 记账底部抽屉的保存键下方是列表、右下角是「+」浮动按钮，于是「点遮罩却打开了别的
 * 卡片」「保存完编辑器又自己弹出来」都是这一下补发的 click 干的（桌面看不出来：
 * 浮层还在原地时 click 就已经派发给按钮了）。
 *
 * **只吃原来那一下**：记下触发关闭的指针坐标，补发的 click 必然落在同一点附近；
 * 坐标差得远的是用户的下一次操作，绝不能碰（早前按时间窗无条件吞掉一个 click，
 * 结果「保存后紧接着点齿轮」这一下被吃了）。只在纯触屏设备上生效——桌面的 click
 * 就是用户点的那一下。
 */
const WINDOW_MS = 500;
const TOLERANCE_PX = 24;

let timer: ReturnType<typeof setTimeout> | undefined;
let origin: { x: number; y: number } | null = null;

function disarm(): void {
  if (timer !== undefined) {
    clearTimeout(timer);
    timer = undefined;
  }
  origin = null;
  window.removeEventListener("click", swallow, true);
}

function swallow(event: MouseEvent): void {
  if (origin) {
    const dx = Math.abs(event.clientX - origin.x);
    const dy = Math.abs(event.clientY - origin.y);
    if (dx > TOLERANCE_PX || dy > TOLERANCE_PX) return;
  }
  event.preventDefault();
  event.stopPropagation();
  disarm();
}

/** 关掉浮层时调用，`at` 是触发这一下关闭的指针坐标。 */
export function suppressGhostClick(at?: { x: number; y: number }): void {
  if (!touchOnly || typeof window === "undefined") return;
  disarm();
  origin = at ?? null;
  window.addEventListener("click", swallow, true);
  timer = setTimeout(disarm, WINDOW_MS);
}

import { tick } from "svelte";

/**
 * 浮层打开后把自己收进视口。编辑器元数据行的浮层默认 `left:0` 锚在触发器左缘，
 * 触发器靠右（窄屏上天气/标签按钮经常排在行尾）时浮层会整个伸出屏幕右边——
 * 安卓日记编辑器天气选择框越界就是它。打开后量一次，超出哪边就往哪边推回去
 * （app-shell 有 transform 缩放，rect 是视觉像素，位移要除回逻辑像素）。
 */
export async function clampPopoverToViewport(
  container: HTMLElement | undefined,
  scale: number,
  selector = ".editor-meta-pop"
): Promise<void> {
  await tick();
  const pop = container?.querySelector<HTMLElement>(selector);
  if (!pop) return;
  pop.style.left = "";
  const rect = pop.getBoundingClientRect();
  const margin = 8;
  let shift = 0;
  const overflowRight = rect.right - (window.innerWidth - margin);
  if (overflowRight > 0) shift -= overflowRight / scale;
  const overflowLeft = margin - (rect.left + shift * scale);
  if (overflowLeft > 0) shift += overflowLeft / scale;
  if (shift !== 0) pop.style.left = `${Math.round(shift)}px`;
}

/**
 * 给「触发器 + `position: fixed` 浮层」这一对算内联定位样式（年月选择器、分类钻取面板）。
 * 用 fixed 而不是 absolute 是因为触发器常常待在滚动容器里（日历栏、统计顶栏），
 * absolute 浮层会被裁剪还会跟着滚走。app-shell 带 transform 缩放，fixed 就以它为包含块：
 * rect 是视觉像素，写进样式前一律除回 scale。放不下就向上翻转，横向收进视口。
 */
export function anchoredPopoverStyle(
  trigger: HTMLElement | undefined,
  scale: number,
  width: number,
  estHeight: number,
  gap = 6
): string {
  if (!trigger) return "";
  const margin = 8;
  const rect = trigger.getBoundingClientRect();
  const visualHeight = estHeight * scale;
  const openBelow = rect.bottom + gap + visualHeight <= window.innerHeight;
  const top = openBelow ? rect.bottom + gap : Math.max(margin, rect.top - gap - visualHeight);
  const maxLeft = window.innerWidth - margin - width * scale;
  const left = Math.max(margin, Math.min(rect.left, maxLeft));
  return `top: ${Math.round(top / scale)}px; left: ${Math.round(left / scale)}px;`;
}

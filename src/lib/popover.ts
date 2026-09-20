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

// ---------------------------------------------------------------------------
// 点锚定的浮层定位（v0.8.6 需求 4）
// ---------------------------------------------------------------------------

/** 视口边缘保留的逻辑像素边距 */
export const POPOVER_MARGIN_PX = 8;
/** 浮层的最小可用高度：比这还矮就不好用，宁可翻到锚点上方 */
export const POPOVER_MIN_HEIGHT_PX = 120;

export type PopoverSize = { width: number; height: number };
export type PopoverView = { width: number; height: number };
export type PopoverPlacement = {
  /** 逻辑像素（调用方把 rect 除以 uiScale 之后再进、再原样写进样式） */
  left: number;
  top: number;
  /** > 0 时给浮层 `max-height` 并让它内部滚动 */
  maxHeight: number;
};

/**
 * 点锚定的浮层定位（右键/长按菜单、三点菜单、日期浮层共用这一份）。
 *
 * 三条语义：
 * ① **优先向下展开**（盖住唤起它的按钮会让「点按钮」变成「点菜单」）；
 * ② 下方连最小高度都放不下（或上方明显更宽松）才翻到锚点上方，且**下边缘与锚点对齐**
 *    ——右键菜单在屏幕下半部的常规行为；
 * ③ 四边一律钳制在视口内：宁可限高内部滚动，也绝不溢出（此前竖向第三分支用
 *    `max(120, spaceBelow)`，底部空间不足 120px 时菜单就是从这里溢出视口的）。
 *
 * 入参与返回都是**逻辑像素**：`app-shell` 有 `transform: scale(uiScale)`，rect 是
 * 视觉像素，调用方负责除一次 scale（与 `anchoredPopoverStyle` 同一约定）。
 */
export function placePopover(
  anchor: { x: number; y: number },
  size: PopoverSize,
  view: PopoverView,
  options: { xAlign?: "left" | "right"; gap?: number; margin?: number; minHeight?: number } = {}
): PopoverPlacement {
  const margin = options.margin ?? POPOVER_MARGIN_PX;
  const gap = options.gap ?? 0;
  const minHeight = options.minHeight ?? POPOVER_MIN_HEIGHT_PX;
  const xAlign = options.xAlign ?? "left";
  const width = Math.min(size.width, Math.max(0, view.width - margin * 2));
  const left = Math.max(
    margin,
    Math.min(xAlign === "right" ? anchor.x - width : anchor.x, view.width - margin - width)
  );
  const spaceBelow = view.height - margin - anchor.y - gap;
  const spaceAbove = anchor.y - gap - margin;
  if (size.height <= spaceBelow) return { left, top: anchor.y + gap, maxHeight: 0 };
  if (spaceBelow >= minHeight && spaceBelow >= spaceAbove) {
    // 放不下但下方还留得出可用高度、且不比上方差：原地展开、**贴视口下边界**限高滚动
    return { left, top: anchor.y + gap, maxHeight: Math.round(spaceBelow) };
  }
  // 翻到锚点上方：下边缘与锚点对齐，上边缘同样不许越过视口顶
  const available = Math.max(0, Math.round(spaceAbove));
  const used = Math.min(size.height, available);
  return {
    left,
    top: Math.max(margin, anchor.y - gap - used),
    maxHeight: used < size.height ? available : 0
  };
}

// 调试出口（与 `window.__kxtodoRenderStats` / `__kxtodoSearch` 同款）：
// 几何是纯函数，回归脚本直接在页面里对它做四角断言
if (typeof window !== "undefined") {
  (window as Window & { __kxtodoPlacePopover?: typeof placePopover }).__kxtodoPlacePopover = placePopover;
}

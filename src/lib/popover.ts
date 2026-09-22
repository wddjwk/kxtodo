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

export type PopoverSize = { width: number; height: number };
export type PopoverView = { width: number; height: number };
export type PopoverPlacement = {
  /** 逻辑像素（调用方把 rect 除以 uiScale 之后再进、再原样写进样式） */
  left: number;
  top: number;
  /** > 0 时给浮层 `max-height` 并让它内部滚动（只在上、下都放不下时出现） */
  maxHeight: number;
};

/**
 * 点锚定的浮层定位（右键/长按菜单、三点菜单、日期浮层共用这一份）。
 *
 * v0.8.7 需求 1 的终裁语义（两轮改判后定案，**两条分支，没有中间态**）：
 * ① **下方放得下 → 开在下方，左上角顶点贴鼠标**（盖住唤起按钮会让「点按钮」变「点菜单」）；
 * ② **下方放不下 → 整个翻到上方，下边缘贴锚点**；右键/长按菜单再横向镜像成
 *    **右下角顶点贴鼠标**（`mirrorXOnFlip`）——按钮锚定的浮层不开镜像，
 *    否则锚点对不上（`xAlign:"right"` 时镜像与不镜像结构性恒等，开了也无害）；
 * ③ **不做「下方限高 + 内部滚动」**：菜单/面板里出现滚动条被真机打回（原话「这太蠢了」）。
 *    唯一滚动兜底是「上下都放不下」——翻上 + maxHeight 钳制，上下都不越界；
 * ④ 四边一律钳制在视口内。
 *
 * 入参与返回都是**逻辑像素**：`app-shell` 有 `transform: scale(uiScale)`，rect 是
 * 视觉像素，调用方负责除一次 scale（与 `anchoredPopoverStyle` 同一约定）。
 * 前提：**锚点须在视口内**（界外锚点没有夹回语义，也不该有）。
 */
export function placePopover(
  anchor: { x: number; y: number },
  size: PopoverSize,
  view: PopoverView,
  options: { xAlign?: "left" | "right"; gap?: number; margin?: number; mirrorXOnFlip?: boolean } = {}
): PopoverPlacement {
  const margin = options.margin ?? POPOVER_MARGIN_PX;
  const gap = options.gap ?? 0;
  const xAlign = options.xAlign ?? "left";
  const mirrorXOnFlip = options.mirrorXOnFlip ?? false;
  const width = Math.min(size.width, Math.max(0, view.width - margin * 2));
  const clampLeft = (value: number): number =>
    Math.max(margin, Math.min(value, view.width - margin - width));
  const xLeft = clampLeft(xAlign === "right" ? anchor.x - width : anchor.x);
  const spaceBelow = view.height - margin - anchor.y - gap;
  if (size.height <= spaceBelow) return { left: xLeft, top: anchor.y + gap, maxHeight: 0 };
  // 翻上：下边缘贴锚点。`used` 只在「上方也放不下」时才小于内容高（滚动兜底）。
  const available = Math.max(0, anchor.y - gap - margin);
  const used = Math.min(size.height, available);
  return {
    left: mirrorXOnFlip ? clampLeft(anchor.x - width) : xLeft,
    top: Math.max(margin, anchor.y - gap - used),
    maxHeight: used < size.height ? available : 0
  };
}

// 调试出口（与 `window.__kxtodoRenderStats` / `__kxtodoSearch` 同款）：
// 几何是纯函数，回归脚本直接在页面里对它做四角断言
if (typeof window !== "undefined") {
  (window as Window & { __kxtodoPlacePopover?: typeof placePopover }).__kxtodoPlacePopover = placePopover;
}

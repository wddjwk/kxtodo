/**
 * 移动端任务卡的左右滑动手势（v0.9.0 新特性 3）。
 *
 * - **右滑**：渐进露出红色「删除」，过阈值松手即删（阈值前松手弹回）。
 * - **左滑**：同时露出「置顶/取消置顶」与「编辑」两块，随滑动加深**一起**缩放
 *   （不是抽屉式一块接一块地推出来），过阈值松手就停在这个位置等用户点。
 *
 * 与既有手势的分工：
 * - **纵向滚动 / 下拉同步**：方向锁（`swipeLock`）先判意图，纵向占优就整个放弃接管，
 *   一个 `preventDefault` 都不发；`pullrefresh.ts` 那侧同样加了横纵判定，两边不会抢。
 * - **长按开菜单**：`longpress` 自己的 10px 移动容差会先放手，不需要这里配合。
 * - **单击展开 / 双击编辑**：拖动过就置一段抑制窗（`isSwipeSuppressed`），
 *   卡片自己的 click 处理器据此吞掉那一记补发的 click。
 *
 * 几何量写在 CSS 变量上（`--swipe-x` / `--swipe-reveal`），JS 只算数不碰样式表；
 * 拖动期间加 `.dragging` 关掉 transition，松手去掉它由 CSS 弹回——回弹曲线是
 * `cubic-bezier(.2,.8,.2,1)`，与移动端其它浮层同一套手感。
 */

/** 认定横滑所需的最小位移（小于它一律当点击/滚动） */
export const SWIPE_SLOP_PX = 8;
/** 右滑删除的阈值 */
export const SWIPE_DELETE_THRESHOLD_PX = 92;
/** 左滑露出两个动作的阈值（也是松手后的停靠宽度：两块各 76px） */
export const SWIPE_REVEAL_THRESHOLD_PX = 152;
/** 过阈值之后的阻尼：再滑也只走这么多 */
export const SWIPE_DAMPING = 0.4;
/** 右滑最多能拉多远 */
export const SWIPE_DELETE_MAX_PX = 148;
/** 左滑最多能拉多远 */
export const SWIPE_REVEAL_MAX_PX = 196;
/** 拖动结束后吞掉补发 click 的窗口 */
const SUPPRESS_CLICK_MS = 450;

let suppressedUntil = 0;

/** 刚滑过一行：随后的那一记 click 不算点击（与 `isLongPressSuppressed` 同一套路） */
export function isSwipeSuppressed(): boolean {
  return Date.now() < suppressedUntil;
}

export type SwipeDirection = "none" | "horizontal" | "vertical";

/**
 * 方向锁：横向位移要明显压过纵向才算横滑（1.4 倍），否则判纵向、整个手势交还给
 * 列表滚动与下拉同步。都没过 slop 时返回 "none"（还没到能下结论的位移）。
 */
export function swipeLock(dx: number, dy: number): SwipeDirection {
  const ax = Math.abs(dx);
  const ay = Math.abs(dy);
  if (ax < SWIPE_SLOP_PX && ay < SWIPE_SLOP_PX) return "none";
  if (ax >= ay * 1.4 && ax >= SWIPE_SLOP_PX) return "horizontal";
  if (ay > ax && ay >= SWIPE_SLOP_PX) return "vertical";
  return "none";
}

/**
 * 阻尼位移：阈值之前 1:1 跟手，过阈值后按 `damping` 衰减并封顶 `max`。
 * 入参出参都是「绝对值」，符号由调用方决定（左滑右滑共用一条曲线）。
 */
export function dampedSwipeOffset(raw: number, threshold: number, max: number, damping = SWIPE_DAMPING): number {
  const value = Math.abs(raw);
  if (value <= threshold) return value;
  return Math.min(max, threshold + (value - threshold) * damping);
}

/** 露出进度 0..1：动作块的缩放与透明度都吃它（两块同时缩放就靠这一个数） */
export function swipeRevealRatio(offset: number, threshold: number): number {
  if (threshold <= 0) return 0;
  return Math.min(1, Math.max(0, Math.abs(offset) / threshold));
}

export type SwipeActionKind = "delete" | "pin" | "edit";

export type SwipeRowConfig = {
  /** 关掉手势（展开态的卡片、桌面端）：返回 false 时连 touchstart 都不记 */
  enabled: () => boolean;
  /** 外部（模块级 store）决定的露出态：true = 这一行停在露出位置 */
  open: boolean;
  /** 左滑露出后的停靠宽度（**手势像素**，与阈值同一套单位） */
  revealWidth: number;
  /**
   * 壳的缩放比（`appearance.uiScale`，默认 0.75）。手势量的是 clientX = **视觉像素**，
   * 而 `--swipe-x` 要写成**布局像素**（translateX 会被壳再乘一次 scale）——不换算的
   * 症状是卡片只跟到手指的 75%，越滑越脱手（v0.8.5 的 VirtualStack 量高是同一个坑）。
   */
  scale: () => number;
  onAction: (action: SwipeActionKind) => void;
  /** 露出态变化：宿主用它把「同一时刻只有一行露出」收敛进 store */
  onRevealChange: (open: boolean) => void;
};

export function swipeRow(
  node: HTMLElement,
  config: SwipeRowConfig
): { update: (next: SwipeRowConfig) => void; destroy: () => void } {
  let current = config;
  let startX = 0;
  let startY = 0;
  let locked: SwipeDirection = "none";
  let tracking = false;
  let dragging = false;
  /** 当前偏移（**手势像素**，带符号：右滑为正、左滑为负） */
  let offset = 0;

  function scaleValue(): number {
    const value = current.scale();
    return Number.isFinite(value) && value > 0 ? value : 1;
  }

  function paint(value: number): void {
    offset = value;
    const left = value < 0;
    const threshold = left ? current.revealWidth : SWIPE_DELETE_THRESHOLD_PX;
    // CSS 变量是布局像素：视觉位移 ÷ 壳缩放比
    node.style.setProperty("--swipe-x", `${Math.round(value / scaleValue())}px`);
    node.style.setProperty("--swipe-reveal", swipeRevealRatio(value, threshold).toFixed(3));
    node.dataset.swipe = value === 0 ? "" : left ? "left" : "right";
    node.classList.toggle("swipe-ready", left ? -value >= current.revealWidth : value >= SWIPE_DELETE_THRESHOLD_PX);
  }

  function settle(value: number): void {
    node.classList.remove("dragging");
    paint(value);
  }

  function onTouchStart(event: TouchEvent): void {
    if (!current.enabled() || event.touches.length !== 1) return;
    const target = event.target as HTMLElement | null;
    // 交互控件上的手势交还给控件本身（勾选框、日期按钮、露出的动作块）
    if (target?.closest("button, input, textarea, a, select")) return;
    tracking = true;
    dragging = false;
    locked = "none";
    startX = event.touches[0].clientX;
    startY = event.touches[0].clientY;
  }

  function onTouchMove(event: TouchEvent): void {
    if (!tracking) return;
    const touch = event.touches[0];
    if (!touch) return;
    const dx = touch.clientX - startX;
    const dy = touch.clientY - startY;
    if (locked === "none") {
      locked = swipeLock(dx, dy);
      // 纵向：整个手势交还给列表滚动与下拉同步，本次触摸不再接管
      if (locked === "vertical") {
        tracking = false;
        return;
      }
      if (locked !== "horizontal") return;
      dragging = true;
      node.classList.add("dragging");
    }
    const base = current.open ? -current.revealWidth : 0;
    const raw = base + dx;
    const next =
      raw >= 0
        ? dampedSwipeOffset(raw, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)
        : -dampedSwipeOffset(-raw, current.revealWidth, SWIPE_REVEAL_MAX_PX);
    paint(next);
    // 横滑已接管：吞掉默认行为，否则 WebView 会同时纵向滚动这一行
    if (event.cancelable) event.preventDefault();
  }

  function onTouchEnd(): void {
    if (!tracking) return;
    tracking = false;
    if (!dragging) return;
    dragging = false;
    suppressedUntil = Date.now() + SUPPRESS_CLICK_MS;
    if (offset > 0) {
      // 右滑到位即删；没到位弹回
      if (offset >= SWIPE_DELETE_THRESHOLD_PX) {
        settle(0);
        current.onAction("delete");
        return;
      }
      settle(0);
      return;
    }
    if (-offset >= current.revealWidth) {
      settle(-current.revealWidth);
      current.onRevealChange(true);
      return;
    }
    settle(0);
    if (current.open) current.onRevealChange(false);
  }

  function onClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    const action = target?.closest("[data-swipe-action]") as HTMLElement | null;
    if (action) {
      event.preventDefault();
      event.stopPropagation();
      const kind = action.dataset.swipeAction as SwipeActionKind;
      settle(0);
      current.onRevealChange(false);
      current.onAction(kind);
      return;
    }
    // 滑过之后的那一记补发 click：吞掉，别让它变成「展开/折叠」
    if (isSwipeSuppressed()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    // 露出态下点卡片本身 = 收回（业界通行的「点别处关掉」）
    if (current.open) {
      event.preventDefault();
      event.stopPropagation();
      settle(0);
      current.onRevealChange(false);
    }
  }

  node.addEventListener("touchstart", onTouchStart, { passive: true });
  node.addEventListener("touchmove", onTouchMove, { passive: false });
  node.addEventListener("touchend", onTouchEnd);
  node.addEventListener("touchcancel", onTouchEnd);
  node.addEventListener("click", onClick, true);

  paint(current.open ? -current.revealWidth : 0);

  return {
    update(next: SwipeRowConfig): void {
      const wasOpen = current.open;
      current = next;
      if (dragging) return;
      if (next.open !== wasOpen) paint(next.open ? -next.revealWidth : 0);
    },
    destroy(): void {
      node.removeEventListener("touchstart", onTouchStart);
      node.removeEventListener("touchmove", onTouchMove);
      node.removeEventListener("touchend", onTouchEnd);
      node.removeEventListener("touchcancel", onTouchEnd);
      node.removeEventListener("click", onClick, true);
    }
  };
}

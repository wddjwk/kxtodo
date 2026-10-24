// 移动端下拉同步：列表滚到顶再往下拉，超过阈值松手就跑一轮同步。
//
// 只在「滚到顶 + 单指 + 往下拽」时接管手势，其余触摸一律不碰（preventDefault 也只
// 在这个窗口里发），滚动、长按、单击双击都不受影响。
//
// **与任务卡的左右滑互斥**（v0.9.0 新特性 3）：横纵意图用同一把方向锁判定，横向
// 占优就当场放弃这次下拉（`swipeRow.ts::swipeLock` 是同一套判据），否则斜着滑一行
// 会把下拉指示条一起拽出来。

import { swipeLock } from "./swipeRow";

export type PullHandlers = {
  /** 同步功能是否开启（没配对/暂停时不接管手势） */
  enabled: () => boolean;
  /** 上一轮还没跑完时不重复触发 */
  busy: () => boolean;
  /** 下拉进度（逻辑像素）与是否已过阈值 */
  onProgress: (distance: number, ready: boolean) => void;
  /** 松手且已过阈值 */
  onRelease: () => void;
  /** 松手但没到阈值（宿主用它播回收动画，v0.9.0） */
  onCancel?: () => void;
};

/** 触发同步的下拉距离（阻尼后） */
const THRESHOLD = 72;
/** 提示条的最大高度，再拉也不长 */
const MAX_PULL = 140;

export function pullToRefresh(
  node: HTMLElement,
  handlers: PullHandlers
): { update: (next: PullHandlers) => void; destroy: () => void } {
  let current = handlers;
  let startX = 0;
  let startY = 0;
  let pulling = false;
  let ready = false;
  /** 方向锁：横向占优就整轮放弃（与 swipeRow 同一把锁） */
  let locked = false;

  function reset(): void {
    pulling = false;
    ready = false;
    current.onProgress(0, false);
  }

  function onTouchStart(event: TouchEvent): void {
    if (!current.enabled() || current.busy()) return;
    if (event.touches.length !== 1 || node.scrollTop > 0) return;
    startX = event.touches[0].clientX;
    startY = event.touches[0].clientY;
    locked = false;
    pulling = true;
  }

  function onTouchMove(event: TouchEvent): void {
    if (!pulling) return;
    if (node.scrollTop > 0) {
      reset();
      return;
    }
    const dx = event.touches[0].clientX - startX;
    const dy = event.touches[0].clientY - startY;
    if (!locked && swipeLock(dx, dy) === "horizontal") {
      // 这一轮是横滑（任务卡的左右滑）：下拉彻底让位
      reset();
      return;
    }
    locked = true;
    if (dy <= 4) {
      if (ready) {
        ready = false;
        current.onProgress(0, false);
      }
      return;
    }
    const damped = Math.min(MAX_PULL, dy * 0.55);
    ready = damped >= THRESHOLD;
    current.onProgress(damped, ready);
    // 只在真的下拉且列表已在顶时吞掉默认行为（挡掉 WebView 自带的过卷回弹）
    if (event.cancelable) event.preventDefault();
  }

  function onTouchEnd(): void {
    if (!pulling) return;
    const fire = ready;
    reset();
    if (fire) current.onRelease();
    else current.onCancel?.();
  }

  node.addEventListener("touchstart", onTouchStart, { passive: true });
  node.addEventListener("touchmove", onTouchMove, { passive: false });
  node.addEventListener("touchend", onTouchEnd);
  node.addEventListener("touchcancel", onTouchEnd);
  return {
    update(next: PullHandlers): void {
      current = next;
    },
    destroy(): void {
      node.removeEventListener("touchstart", onTouchStart);
      node.removeEventListener("touchmove", onTouchMove);
      node.removeEventListener("touchend", onTouchEnd);
      node.removeEventListener("touchcancel", onTouchEnd);
    }
  };
}

/**
 * 横向滑动 action（移动端日历用它换月）。
 *
 * 只读手势结果，不抢手势：不 preventDefault、不 touch-action 干预，
 * 垂直滚动照旧归浏览器；只有在「一次基本水平的滑动」结束时才回调。
 * 判定：位移 ≥ 45px 且横向分量明显大于纵向（1.4 倍），向左滑 = 下一段、向右滑 = 上一段。
 */

export type SwipeXHandlers = { onPrev: () => void; onNext: () => void };

export function swipeX(node: HTMLElement, handlers: SwipeXHandlers): { update: (next: SwipeXHandlers) => void; destroy: () => void } {
  const THRESHOLD = 45;
  const MINOR = 12;
  let current = handlers;
  let start: { x: number; y: number } | null = null;
  let tracking = false;

  function onStart(event: TouchEvent): void {
    if (event.touches.length !== 1) {
      tracking = false;
      return;
    }
    const touch = event.touches[0];
    start = { x: touch.clientX, y: touch.clientY };
    tracking = true;
  }

  function onMove(event: TouchEvent): void {
    if (!tracking || !start || event.touches.length !== 1) return;
    const touch = event.touches[0];
    const dx = Math.abs(touch.clientX - start.x);
    const dy = Math.abs(touch.clientY - start.y);
    // 竖向意图一旦明确就放弃这次手势：滚页面优先，别在滚动结束时误换月
    if (dy > MINOR && dy > dx) {
      tracking = false;
      start = null;
    }
  }

  function onEnd(event: TouchEvent): void {
    if (!tracking || !start) return;
    const touch = event.changedTouches[0];
    const origin = start;
    tracking = false;
    start = null;
    if (!touch) return;
    const dx = touch.clientX - origin.x;
    const dy = touch.clientY - origin.y;
    if (Math.abs(dx) < THRESHOLD || Math.abs(dx) < Math.abs(dy) * 1.4) return;
    if (dx < 0) current.onNext();
    else current.onPrev();
  }

  node.addEventListener("touchstart", onStart, { passive: true });
  node.addEventListener("touchmove", onMove, { passive: true });
  node.addEventListener("touchend", onEnd, { passive: true });
  node.addEventListener("touchcancel", () => {
    tracking = false;
    start = null;
  });

  return {
    update(next: SwipeXHandlers): void {
      current = next;
    },
    destroy(): void {
      node.removeEventListener("touchstart", onStart);
      node.removeEventListener("touchmove", onMove);
      node.removeEventListener("touchend", onEnd);
    }
  };
}

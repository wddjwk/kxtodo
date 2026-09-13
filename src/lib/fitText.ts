/**
 * 金额自适应：数字长了就一档一档缩小字号，而不是截断出省略号——
 * 钱被截掉一半读不出来，比字小一点糟得多。
 *
 * 用 scrollWidth（内容宽）比 clientWidth（盒子宽）判断，所以目标元素必须
 * `white-space: nowrap` 且不带 overflow 裁剪。基准字号每次都从计算样式重读，
 * 于是设置里改字号之后重新量一遍就是新基准。
 *
 * 无参 action 的 update() 不会被调用，所以调用方必须把当前文本当参数传进来
 * （`use:fitAmount={value}`），换了金额才会重新量。
 */
const FLOOR_RATIO = 0.6;

export function fitAmount(node: HTMLElement, _value?: unknown): { update: () => void; destroy: () => void } {
  let observer: ResizeObserver | undefined;

  function fit(): void {
    node.style.fontSize = "";
    const width = node.clientWidth;
    if (!width) return;
    const base = Number.parseFloat(getComputedStyle(node).fontSize) || 16;
    const floor = base * FLOOR_RATIO;
    let size = base;
    while (size > floor && node.scrollWidth > width + 1) {
      size = Math.max(floor, size - 1);
      node.style.fontSize = `${size.toFixed(1)}px`;
    }
  }

  fit();
  if (typeof ResizeObserver === "function") {
    observer = new ResizeObserver(fit);
    observer.observe(node);
  }

  return {
    update: fit,
    destroy(): void {
      observer?.disconnect();
    }
  };
}

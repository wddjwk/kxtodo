/**
 * 长列表的窗口化（v0.8.4 需求 3/5）：**只动渲染层**。
 *
 * 5000 篇日记全量挂载会同时建 5000 个 DiaryCard 组件实例（每个还带标题/摘要的
 * 溢出测量订阅），这是那个「点开日记界面就卡死、内存吃到 2GB」的根因——数据在
 * store 里全量进内存是无害的，有害的是全部变成 DOM。这里只算「当前该挂哪一段」，
 * 上下留 `overscan` 个缓冲，滑出去的销毁。
 *
 * 高度是**动态**的：每项挂载时量一次、之后由 ResizeObserver 跟踪（展开卡片、
 * 图片加载完都会变高），没量过的先用 estimate 估。纯函数与副作用分开——
 * 这几条公式分错了只会表现为「列表跳一下」，肉眼很难定位，所以单独成模块跑单测。
 */

/** 累计高度前缀和（`prefix[i]` = 第 0..i-1 项的总高；末尾多一位 = 总高）。 */
export function prefixSums(heights: number[]): number[] {
  const out: number[] = new Array(heights.length + 1);
  out[0] = 0;
  for (let index = 0; index < heights.length; index += 1) {
    out[index + 1] = out[index] + Math.max(0, heights[index]);
  }
  return out;
}

/** 偏移量落在哪一项（二分；偏移超出末尾时回最后一项）。 */
export function indexAtOffset(prefix: number[], offset: number): number {
  const count = prefix.length - 1;
  if (count <= 0) return 0;
  let low = 0;
  let high = count;
  while (low < high) {
    const mid = (low + high) >> 1;
    // 第 mid 项占据 [prefix[mid], prefix[mid+1])
    if (prefix[mid + 1] <= offset) low = mid + 1;
    else high = mid;
  }
  return Math.min(count - 1, low);
}

export type WindowRange = {
  /** 渲染区间 [start, end]（含端点）；count 为 0 时两者都是 0 */
  start: number;
  end: number;
  /** 顶部占位高度（= prefix[start]） */
  offsetTop: number;
  /** 底部占位高度（= total - prefix[end + 1]） */
  offsetBottom: number;
};

/**
 * 由滚动位置算渲染区间。
 *
 * `overscan` 是上下各多挂几项（滚动惯性里先有内容再到位）；`maxCount` 是硬上限，
 * 防某种极端（视口极高 / estimate 极小）一次挂太多。
 */
export function windowRange(
  prefix: number[],
  scrollTop: number,
  viewport: number,
  overscan: number,
  maxCount = 400
): WindowRange {
  const count = prefix.length - 1;
  if (count <= 0) return { start: 0, end: 0, offsetTop: 0, offsetBottom: 0 };
  const total = prefix[count];
  const top = Math.max(0, Math.min(scrollTop, Math.max(0, total)));
  const bottom = top + Math.max(0, viewport);
  const first = indexAtOffset(prefix, top);
  const last = indexAtOffset(prefix, bottom);
  let start = Math.max(0, first - overscan);
  let end = Math.min(count - 1, last + overscan);
  if (end - start + 1 > maxCount) end = Math.min(count - 1, start + maxCount - 1);
  return {
    start,
    end,
    offsetTop: prefix[start],
    offsetBottom: Math.max(0, total - prefix[end + 1])
  };
}

/**
 * 首屏窗口（需求 3 的「首屏 30 + 上下各 overscan」）。
 * 一屏能装多少不确定，所以取 `max(首屏下限, 视口能装下的项数) + overscan`。
 */
export const FIRST_SCREEN_ITEMS = 30;

export function initialCount(estimate: number, viewport: number, overscan: number): number {
  const fitting = estimate > 0 ? Math.ceil(viewport / estimate) : FIRST_SCREEN_ITEMS;
  return Math.max(FIRST_SCREEN_ITEMS, fitting) + overscan;
}

/**
 * 头部插入/删除后的滚动补偿：按「锚点项 + 项内偏移」还原滚动位置。
 *
 * 照搬 ScrollAnchoring 的思路但不依赖浏览器实现（安卓 WebView 上它时有时无）：
 * 记下第一个可见项的下标与它内部的偏移，列表变了以后用新高度算回 scrollTop——
 * 否则「写完日记回到列表、滚回顶部又弹回来」。
 */
export type ScrollAnchor = { index: number; offset: number; key?: string };

export function anchorAt(prefix: number[], scrollTop: number, keys: string[]): ScrollAnchor {
  const index = indexAtOffset(prefix, scrollTop);
  return { index, offset: scrollTop - prefix[index], key: keys[index] };
}

export function scrollTopForAnchor(prefix: number[], keys: string[], anchor: ScrollAnchor): number {
  let index = anchor.key ? keys.indexOf(anchor.key) : -1;
  if (index < 0) index = Math.min(Math.max(0, anchor.index), Math.max(0, prefix.length - 2));
  return prefix[index] + anchor.offset;
}

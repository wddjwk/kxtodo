/**
 * 拖动的落点判定（v0.8.6 需求 2，纯逻辑有单测）。
 *
 * 两个必须分清的坑：
 *
 * ① **不能用动画中的实时矩形**。行让位走 `animate:flip`，而 flip 是**纯 transform**——
 *    `getBoundingClientRect()` 拿到的是「飞在半路」的临时值，几行之间互相不自洽；
 *    拿它做落点判定会 hover 振荡、预览反复触发，整棵树跟着闪。布局位置（行最终会停在
 *    哪里，不含 transform）才是稳定口径：把 rect 里的 `translateY` 减掉就是它
 *    （`settledTop`）。**壳上还有 `transform: scale(uiScale)`**，元素自己的 translate
 *    是逻辑像素、要乘缩放比才是视觉像素——两处都按 v0.8.5 的坐标纪律处理。
 * ② **边界要带迟滞**。指针停在行边界上时，抖动一像素就换行、换回来又换回去，看起来
 *    就是「一跳一跳」。判定一律走施密特触发器：越过边界 `band/2` 才换，回带内不换。
 */

export type DragPosition = "before" | "after" | "inside";
export type DragZone = { key: string; top: number; bottom: number };

/** 迟滞带宽（视觉像素）：单次抖动幅度的一半上下就够，别大到手感发钝 */
export const DRAG_BAND_PX = 8;

/** 滚动容器的视觉/布局缩放比（壳上 `transform: scale(uiScale)` 时不为 1）。 */
export function scaleOf(container: HTMLElement): number {
  const layout = container.offsetHeight;
  if (layout <= 0) return 1;
  return container.getBoundingClientRect().height / layout;
}

/**
 * 元素**布局位置**的顶端（视觉像素，已剔除 flip 的 translate）。
 * `scale` 由调用方批量取一次（`scaleOf`），别每行都读一遍。
 */
export function settledTop(element: HTMLElement, scale: number): number {
  const rect = element.getBoundingClientRect();
  const transform = getComputedStyle(element).transform;
  if (!transform || transform === "none") return rect.top;
  const match = /matrix(3d)?\(([^)]+)\)/.exec(transform);
  if (!match) return rect.top;
  const parts = match[2].split(",").map((value) => Number.parseFloat(value.trim()));
  // matrix(a, b, c, d, tx, ty) / matrix3d 的 ty 在第 14 位
  const ty = match[1] ? parts[13] ?? 0 : parts[5] ?? 0;
  return rect.top - ty * scale;
}

/**
 * 施密特触发器：`pointer` 是否算在阈值 `threshold` 之上。
 * `wasAbove` = 上一帧的结论；传 null 表示**首次判定**（不加偏置，等价于普通比较）。
 * 已经在上面时要退到 `threshold - band/2` 才翻下去，在下面时要越过 `threshold + band/2`
 * 才翻上来。
 */
export function schmitt(pointer: number, threshold: number, band: number, wasAbove: boolean | null): boolean {
  if (wasAbove === null) return pointer > threshold;
  const half = band / 2;
  return wasAbove ? pointer > threshold - half : pointer > threshold + half;
}

/**
 * 行内位置判定要不要「保持上一轮」。
 *
 * 场景（v0.8.6 需求 2 实测抓到的抖动）：拖条目到分组头的中部，判成「移入」之后
 * 预览把条目挂到分组下面 → **分组头那一行整体上移**；指针没动，却已经落到新几何的
 * 下缘之外 → 下一帧重新判定就翻成「插到分组后面」，行再让位、再翻回来……来回抖。
 *
 * 判据：**行动了、而指针几乎没动 = 布局在动，不是用户在动**，保持现判
 * （与 v0.8.5「浏览器夹回越界的 scrollTop 不是用户操作」同一条纪律）。
 * 指针自己动了（超过迟滞带的一半）就照常重算——所以「在分组头里往下挪一点改判」
 * 仍然可用，慢慢挪也不会被这条规则卡死（累计位移照样算）。
 */
export function keepsPreviousDecision(input: {
  pointerY: number;
  lastPointerY: number;
  rowTop: number;
  lastRowTop: number;
  band: number;
}): boolean {
  const pointerMoved = Math.abs(input.pointerY - input.lastPointerY) > input.band / 2;
  const rowMoved = Math.abs(input.rowTop - input.lastRowTop) > 1;
  return rowMoved && !pointerMoved;
}

/**
 * 把行界连成**没有缝隙**的一份分区（v0.8.6 需求 2）。
 *
 * 行与行之间有 2px 的间距（`.custom-nav` 与 `.tree-item` 的 gap），严格按「指针落在
 * 哪一行的 span 里」判定时，指针走到缝里会返回 null——调用方把它当「空白区」，
 * 于是拖动经过缝隙时落点被清掉/变成「拖到末尾」，行跟着预览乱跳。
 * 缝隙按**中点**判给相邻的某一行，指针在任何 Y 上都恰好命中一行。
 */
export function contiguousZones(rows: DragZone[]): DragZone[] {
  const sorted = [...rows].sort((a, b) => a.top - b.top);
  return sorted.map((row, index) => {
    const prev = sorted[index - 1];
    const next = sorted[index + 1];
    return {
      key: row.key,
      top: prev ? (prev.bottom + row.top) / 2 : Number.NEGATIVE_INFINITY,
      bottom: next ? (row.bottom + next.top) / 2 : Number.POSITIVE_INFINITY
    };
  });
}

/**
 * 指针落在哪个 zone。返回 null = 一个都没命中（空白区，由调用方决定回退语义）。
 * `lastKey` 是当前决策的 zone：换 zone 时要求指针越过两 zone 的公共边界 `band/2`
 * 以上，否则维持原判（这就是「越过中点 N 像素才换、回带内不换」）。
 */
export function zoneAt(zones: DragZone[], pointer: number, lastKey: string | null, band: number): string | null {
  const index = zones.findIndex((zone) => pointer >= zone.top && pointer < zone.bottom);
  if (index < 0) return null;
  const candidate = zones[index].key;
  if (!lastKey || lastKey === candidate) return candidate;
  const lastIndex = zones.findIndex((zone) => zone.key === lastKey);
  // 上一轮的 zone 已经不在了（悬停展开/折叠改了行数）：直接按新布局判
  if (lastIndex < 0) return candidate;
  // 跨了不止一格（快速拖动）：直接换，别被迟滞拖住
  if (Math.abs(index - lastIndex) > 1) return candidate;
  const boundary = index > lastIndex ? zones[lastIndex].bottom : zones[lastIndex].top;
  if (Math.abs(pointer - boundary) < band / 2) return lastKey;
  return candidate;
}

/**
 * 行内的三段/两段判定（分类行 before/inside/after，普通行 before/after），带迟滞。
 * `last` 为 null 时是首次判定，不加偏置。
 */
export function positionInRow(
  row: DragZone,
  pointer: number,
  last: DragPosition | null,
  band: number,
  isCategory: boolean
): DragPosition {
  const height = row.bottom - row.top;
  if (!isCategory) {
    const above = schmitt(pointer, row.top + height / 2, band, last === null ? null : last === "after");
    return above ? "after" : "before";
  }
  const lastInsideOrAfter = last === null ? null : last === "inside" || last === "after";
  const lastAfter = last === null ? null : last === "after";
  const aboveUpper = schmitt(pointer, row.top + height * 0.25, band, lastInsideOrAfter);
  const aboveLower = schmitt(pointer, row.top + height * 0.75, band, lastAfter);
  if (!aboveUpper) return "before";
  return aboveLower ? "after" : "inside";
}

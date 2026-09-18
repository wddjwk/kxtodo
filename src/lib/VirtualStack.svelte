<script lang="ts">
  /**
   * 长列表的窗口化宿主（v0.8.4 需求 3/5）：只挂视口附近的一段，滑出去的销毁。
   *
   * 用法（legacy 槽位：`let:row` 拿到当前项）：
   *
   *     <section class="diary-scroll" bind:this={scrollEl}>
   *       <VirtualStack items={listRows} keyOf={(r) => r.key} scroller={scrollEl} estimate={96}>
   *         <div slot="item" let:row>…</div>
   *       </VirtualStack>
   *     </section>
   *
   * 为什么不用 virtua：它的 Svelte 包是 **runes 组件**，item 内容走 `children`
   * snippet。legacy 组件给带参数的 snippet 传内容时编译器只产出
   * `children: $.invalid_default_snippet`（Svelte 明确报「Cannot use {@render children(...)}
   * if the parent component uses let: directives」），也就是说在 legacy 语法下**根本传不进
   * 每一项的数据**。自己写这一个组件反而更短、更好测（公式都在 windowing.ts 里）。
   *
   * 高度动态：每项挂载时同步量一次，之后交给共享的 ResizeObserver
   * （measureBus，展开卡片/图片加载都会变高）。
   */
  import { afterUpdate, onMount } from "svelte";
  import { observeResize } from "./measureBus";
  import {
    anchorAt,
    prefixSums,
    scrollTopForAnchor,
    windowRange,
    type ScrollAnchor
  } from "./windowing";

  /** 全部数据（**数据层不动**：store 里永远是全量，这里只决定挂谁） */
  export let items: readonly unknown[] = [];
  export let keyOf: (item: unknown, index: number) => string = (_item, index) => String(index);
  /** 没量过时的估高（px） */
  export let estimate = 96;
  /** 视口上下各多挂几项 */
  export let overscan = 10;
  /** 滚动容器（含视口的那个元素）；给了才启用滚动监听 */
  export let scroller: HTMLElement | null = null;
  /**
   * 项数 ≤ 这个值时**全量直出**（0 = 永远窗口化）。
   * 任务列表用它：100 条以内全量比虚拟化还快（少一层测量与占位），
   * 超过阈值才是「不虚拟化会卡」的场景。
   */
  export let fullBelow = 0;

  let heights: number[] = [];
  let scrollTop = 0;
  let viewport = 0;
  let frame = 0;
  let previousKeys: string[] = [];
  /** 列表变化前记下的滚动锚点（每次滚动都刷新） */
  let anchor: ScrollAnchor | null = null;

  $: keys = items.map((item, index) => keyOf(item, index));
  $: prefix = prefixSums(items.map((_, index) => heights[index] ?? estimate));
  $: fullMode = fullBelow > 0 && items.length <= fullBelow;
  $: range = fullMode
    ? { start: 0, end: Math.max(0, items.length - 1), offsetTop: 0, offsetBottom: 0 }
    : windowRange(prefix, scrollTop, viewport > 0 ? viewport : 800, overscan);
  $: visible = items.slice(range.start, range.end + 1).map((item, offset) => ({
    item,
    index: range.start + offset,
    key: keys[range.start + offset] ?? String(range.start + offset)
  }));

  function measure(index: number, node: HTMLElement): void {
    const height = node.getBoundingClientRect().height;
    if (height <= 0) return;
    const current = heights[index] ?? estimate;
    if (Math.abs(current - height) < 0.5) return;
    const next = [...heights];
    next[index] = height;
    heights = next;
  }

  /** 动作：挂载时同步量一次（首帧就要有正确结论），之后交给共享 RO 跟踪 */
  function track(node: HTMLElement, index: number): { update(next: number): void; destroy(): void } {
    // 全量模式不做高度记账：没有占位要算，量了也白量
    if (fullMode) return { update: () => undefined, destroy: () => undefined };
    let current = index;
    const release = observeResize(node, () => measure(current, node));
    measure(current, node);
    return {
      update(next: number) {
        current = next;
        measure(current, node);
      },
      destroy() {
        release();
      }
    };
  }

  function readScroll(): void {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    viewport = scroller.clientHeight;
    anchor = anchorAt(prefix, scrollTop, keys);
  }

  function scheduleRead(): void {
    if (frame !== 0) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      readScroll();
    });
  }

  /**
   * 滚动容器是父组件 `bind:this` 出来的，**不能只在 onMount 里挂监听**：
   * 子组件的 onMount 可能先于父组件的绑定赋值跑完，那样监听从来没挂上
   * （症状：滚动条能拖、窗口永远停在开头）。跟着 prop 变化挂/摘。
   */
  let attached: HTMLElement | null = null;
  const onScroll = (): void => scheduleRead();
  const onResize = (): void => scheduleRead();

  $: if (scroller !== attached) {
    attached?.removeEventListener("scroll", onScroll);
    attached = scroller;
    attached?.addEventListener("scroll", onScroll, { passive: true });
    readScroll();
  }

  onMount(() => {
    readScroll();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      attached?.removeEventListener("scroll", onScroll);
      attached = null;
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  });

  // 列表变了：先把锚点补回去（写完日记头部多一篇时，正在看的那些不该整体下移）
  afterUpdate(() => {
    const changed = keys.length !== previousKeys.length || keys.some((key, index) => key !== previousKeys[index]);
    previousKeys = keys;
    if (!changed || !scroller || !anchor) return;
    const restored = Math.max(0, scrollTopForAnchor(prefix, keys, anchor));
    if (Math.abs(restored - scroller.scrollTop) < 0.5) return;
    scroller.scrollTop = restored;
    scrollTop = restored;
  });

  /** 供宿主跳转（日历点某天 / 搜索定位）：滚到第 index 项 */
  export function scrollToIndex(index: number): void {
    if (!scroller) return;
    const target = Math.min(Math.max(0, index), Math.max(0, items.length - 1));
    scroller.scrollTop = prefix[target];
    scrollTop = scroller.scrollTop;
    anchor = anchorAt(prefix, scrollTop, keys);
  }

  /** 宿主自己动了滚动位置（比如换视图归零）后同步一次 */
  export function syncScroll(): void {
    readScroll();
  }
</script>

<div class="virtual-stack">
  <div class="virtual-spacer" style={`height: ${range.offsetTop}px`} aria-hidden="true"></div>
  {#each visible as row (row.key)}
    <div class="virtual-item" use:track={row.index}>
      <slot name="item" item={row.item} row={row.item} index={row.index} />
    </div>
  {/each}
  <div class="virtual-spacer" style={`height: ${range.offsetBottom}px`} aria-hidden="true"></div>
</div>

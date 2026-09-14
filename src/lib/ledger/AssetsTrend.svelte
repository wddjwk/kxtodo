<script lang="ts">
  /**
   * 总资产趋势曲线（手写 SVG，与统计曲线同一族）：横纵坐标全自适应——
   * 纵轴按 1/2/5×10ⁿ 取整刻度（结余为负也放得下），横轴按跨度选标签格式、
   * 只画稀疏几档。少刻度、无注释，重点看曲线本身。
   * axes = 画刻度（放大查看用）；interactive = 悬浮/点按读数（放大查看用）；
   * 嵌在资产视图里的小块两个都关，就是一条安静的趋势线，点整卡去放大。
   */
  import { formatCents } from "../ledger";
  import type { AssetTrendPoint } from "../ledger";

  export let points: AssetTrendPoint[] = [];
  export let axes = false;
  export let interactive = false;
  /** 刻度文字大小（SVG 用户单位）：SVG 是整体缩放的，同一份 viewBox 画在不同宽度的
   *  容器里，字号要跟着容器宽度补——卡片里的图窄，字就得按比例放大，否则缩成蚂蚁。 */
  export let axisFont = 12;

  const W = 640;
  const H = 240;
  const PAD_TOP = 12;
  const PAD_RIGHT = 10;

  let boxEl: HTMLElement;
  let hoverIndex: number | null = null;

  $: padLeft = axes ? 50 : 8;
  $: padBottom = axes ? 26 : 10;
  $: count = points.length;

  function niceStep(rough: number): number {
    if (!(rough > 0)) return 100;
    const exp = Math.floor(Math.log10(rough));
    const base = 10 ** exp;
    const frac = rough / base;
    const mult = frac <= 1 ? 1 : frac <= 2 ? 2 : frac <= 5 ? 5 : 10;
    return mult * base;
  }

  $: rawLo = count > 0 ? Math.min(...points.map((point) => point.cents)) : 0;
  $: rawHi = count > 0 ? Math.max(...points.map((point) => point.cents)) : 0;
  /** 纵轴取值：整平的上下界（水平线也要给出呼吸空间，不然除零/贴边） */
  $: lo = (() => {
    if (count === 0) return 0;
    if (rawLo === rawHi) return rawLo - Math.max(10000, Math.abs(rawLo) * 0.1);
    const step = niceStep((rawHi - rawLo) / 3);
    return Math.floor(rawLo / step) * step;
  })();
  $: hi = (() => {
    if (count === 0) return 1;
    if (rawLo === rawHi) return rawHi + Math.max(10000, Math.abs(rawHi) * 0.1);
    const step = niceStep((rawHi - rawLo) / 3);
    return Math.ceil(rawHi / step) * step;
  })();

  $: stepX = count > 1 ? (W - padLeft - PAD_RIGHT) / (count - 1) : 0;
  function pointX(index: number): number {
    return padLeft + index * stepX;
  }
  function pointY(cents: number): number {
    return H - padBottom - ((cents - lo) / (hi - lo || 1)) * (H - PAD_TOP - padBottom);
  }

  $: path = points
    .map((point, index) => `${index === 0 ? "M" : "L"}${pointX(index).toFixed(1)},${pointY(point.cents).toFixed(1)}`)
    .join(" ");
  $: area =
    count > 1
      ? `${path} L${pointX(count - 1).toFixed(1)},${pointY(Math.max(lo, 0)).toFixed(1)} L${pointX(0).toFixed(1)},${pointY(Math.max(lo, 0)).toFixed(1)} Z`
      : "";

  $: yTicks = axes ? [hi, (hi + lo) / 2, lo] : [];
  /** 横轴稀疏刻度：最多 4 档，首尾必含（左右填满、不密） */
  $: xTickIndexes = (() => {
    if (!axes || count < 2) return [];
    const target = Math.min(4, count);
    const indexes = new Set<number>();
    for (let k = 0; k < target; k += 1) indexes.add(Math.round((k * (count - 1)) / (target - 1)));
    return [...indexes].sort((a, b) => a - b);
  })();
  $: spanDays =
    count > 1
      ? Math.round(
          (new Date(`${points[count - 1].date}T00:00:00`).getTime() -
            new Date(`${points[0].date}T00:00:00`).getTime()) /
            86_400_000
        )
      : 0;
  $: crossYears = count > 1 && points[0].date.slice(0, 4) !== points[count - 1].date.slice(0, 4);

  /** 横轴标签格式随跨度自适应：长跨度按月、跨年带年份、同年只给月/日 */
  function xLabel(date: string): string {
    const year = date.slice(0, 4);
    const month = Number(date.slice(5, 7));
    const day = Number(date.slice(8, 10));
    if (spanDays > 400) return `${year}/${month}`;
    if (crossYears) return `${year.slice(2)}/${month}/${day}`;
    return `${month}/${day}`;
  }

  function axisLabel(value: number): string {
    if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}万`;
    return formatCents(value).replace(/\.00$/, "");
  }

  function readAt(clientX: number): void {
    if (!interactive || !boxEl || count < 2) return;
    const rect = boxEl.getBoundingClientRect();
    if (rect.width <= 0) return;
    const viewX = ((clientX - rect.left) / rect.width) * W;
    const index = Math.round((viewX - padLeft) / (stepX || 1));
    hoverIndex = Math.min(count - 1, Math.max(0, index));
  }

  $: hoverPoint = hoverIndex !== null ? points[hoverIndex] : null;
  $: tipLeft = hoverIndex !== null ? Math.min(86, Math.max(14, (pointX(hoverIndex) / W) * 100)) : 0;
</script>

{#if count < 2}
  <div class="ledger-trend-empty">有了账户和流水，这里会画出总资产的走势。</div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="ledger-chart-box"
    bind:this={boxEl}
    on:mousemove={(event) => readAt(event.clientX)}
    on:mouseleave={() => { if (interactive) hoverIndex = null; }}
    on:click={(event) => readAt(event.clientX)}
  >
    <svg class="ledger-line-chart ledger-trend-chart" viewBox="0 0 {W} {H}" role="img" aria-label="总资产趋势">
      {#each yTicks as value (value)}
        <line class="ledger-chart-grid" x1={padLeft} x2={W - PAD_RIGHT} y1={pointY(value)} y2={pointY(value)} />
        <text
          class="ledger-chart-axis"
          style="font-size: {axisFont}px"
          x={padLeft - 8}
          y={pointY(value) + axisFont * 0.36}
          text-anchor="end"
        >{axisLabel(value)}</text>
      {/each}
      {#if lo < 0 && hi > 0}
        <line class="ledger-chart-zero" x1={padLeft} x2={W - PAD_RIGHT} y1={pointY(0)} y2={pointY(0)} />
      {/if}
      <path class="ledger-trend-area" d={area} />
      <path class="ledger-trend-line" d={path} />
      {#if hoverPoint}
        <line class="ledger-chart-marker" x1={pointX(hoverIndex ?? 0)} x2={pointX(hoverIndex ?? 0)} y1={PAD_TOP - 4} y2={H - padBottom} />
        <circle class="ledger-chart-dot ledger-trend-dot" cx={pointX(hoverIndex ?? 0)} cy={pointY(hoverPoint.cents)} r="3.8" />
      {/if}
      {#each xTickIndexes as index (index)}
        <text
          class="ledger-chart-axis"
          style="font-size: {axisFont}px"
          x={pointX(index)}
          y={H - 8}
          text-anchor="middle"
        >{xLabel(points[index].date)}</text>
      {/each}
    </svg>
    {#if interactive && hoverPoint}
      <div class="ledger-chart-tip" style="left: {tipLeft}%">
        <strong>{hoverPoint.date.replaceAll("-", "/")}</strong>
        <span><i style="background: var(--accent)"></i>总资产 {formatCents(hoverPoint.cents)}</span>
      </div>
    {/if}
  </div>
{/if}

<script lang="ts">
  /**
   * 统计视图：汇总卡 + 收支曲线（手写 SVG，描边动画）+ 分类占比环 + 排行进度条。
   * 不引图表库：包体积与风格都不值，曲线/环都是几十行 SVG 的事（server 管理台的活动
   * 曲线就是先例）。月视图逐天、年视图逐月。
   */
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";
  import type { MonthCursor } from "../diary";
  import { statsSeries, categoryStats, formatCents, categoryColor } from "../ledger";
  import { ledgerIcon } from "../ledgerIcons";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;

  let mode: "month" | "year" = "month";
  let side: LedgerSide = "expense";
  let openCategory = "";

  $: series = statsSeries(entries, mode, cursor);
  $: totalIncome = series.reduce((sum, point) => sum + point.income, 0);
  $: totalExpense = series.reduce((sum, point) => sum + point.expense, 0);
  $: stats = categoryStats(book, entries, side).filter((item) => item.cents > 0);
  $: statsTotal = stats.reduce((sum, item) => sum + item.cents, 0);

  // --- 曲线几何 ---
  const W = 640;
  const H = 220;
  const PAD_X = 34;
  const PAD_TOP = 18;
  const PAD_BOTTOM = 26;
  $: peak = Math.max(1, ...series.map((point) => Math.max(point.income, point.expense)));
  $: stepX = series.length > 1 ? (W - PAD_X * 2) / (series.length - 1) : 0;
  function pointX(index: number): number {
    return PAD_X + index * stepX;
  }
  function pointY(cents: number): number {
    return H - PAD_BOTTOM - (cents / peak) * (H - PAD_TOP - PAD_BOTTOM);
  }
  function linePath(key: "income" | "expense"): string {
    return series
      .map((point, index) => `${index === 0 ? "M" : "L"}${pointX(index).toFixed(1)},${pointY(point[key]).toFixed(1)}`)
      .join(" ");
  }
  function areaPath(key: "income" | "expense"): string {
    if (series.length === 0) return "";
    const base = (H - PAD_BOTTOM).toFixed(1);
    return `${linePath(key)} L${pointX(series.length - 1).toFixed(1)},${base} L${pointX(0).toFixed(1)},${base} Z`;
  }
  $: axisLabels = [peak, peak / 2, 0];
  $: tickIndexes = series
    .map((_, index) => index)
    .filter((index) => {
      const every = Math.max(1, Math.ceil(series.length / 8));
      return index % every === 0;
    });

  // --- 环几何 ---
  const R = 62;
  const CIRC = 2 * Math.PI * R;
  $: donutSegments = (() => {
    let offset = 0;
    return stats.slice(0, 12).map((item) => {
      const fraction = statsTotal > 0 ? item.cents / statsTotal : 0;
      const segment = {
        id: item.categoryId || "none",
        fraction,
        offset,
        color: segmentColor(item.categoryId)
      };
      offset += fraction;
      return segment;
    });
  })();
  function segmentColor(categoryId: string): string {
    const category = book.categories.find((item) => item.id === categoryId);
    return categoryColor(book, category);
  }
  function toggleCategory(id: string): void {
    openCategory = openCategory === id ? "" : id;
  }
</script>

<div class="ledger-stats">
  <div class="ledger-stats-mode">
    <button type="button" class:active={mode === "month"} on:click={() => (mode = "month")}>
      月
    </button>
    <button type="button" class:active={mode === "year"} on:click={() => (mode = "year")}>
      年
    </button>
  </div>

  <div class="ledger-stats-summary">
    <div>
      <span>{mode === "month" ? "本月收入" : "本年收入"}</span>
      <strong class="income">{formatCents(totalIncome)}</strong>
    </div>
    <div>
      <span>{mode === "month" ? "本月支出" : "本年支出"}</span>
      <strong class="expense">{formatCents(totalExpense)}</strong>
    </div>
    <div>
      <span>结余</span>
      <strong>{formatCents(totalIncome - totalExpense)}</strong>
    </div>
  </div>

  <section class="ledger-chart-card">
    <header>
      <strong>收支趋势</strong>
      <span class="ledger-chart-legend">
        <i class="income"></i>收入 <i class="expense"></i>支出
      </span>
    </header>
    {#key `${mode}-${cursor.year}-${cursor.month}`}
      <svg class="ledger-line-chart" viewBox="0 0 {W} {H}" role="img" aria-label="收支趋势曲线">
        {#each axisLabels as value}
          <line
            class="ledger-chart-grid"
            x1={PAD_X}
            x2={W - PAD_X}
            y1={pointY(value)}
            y2={pointY(value)}
          />
          <text class="ledger-chart-axis" x={4} y={pointY(value) + 4}>
            {value >= 10000 ? `${Math.round(value / 10000)}万` : formatCents(value)}
          </text>
        {/each}
        <path class="ledger-line-area income" d={areaPath("income")} />
        <path class="ledger-line-area expense" d={areaPath("expense")} />
        <path class="ledger-line income" d={linePath("income")} />
        <path class="ledger-line expense" d={linePath("expense")} />
        {#each tickIndexes as index}
          <text class="ledger-chart-axis" x={pointX(index)} y={H - 8} text-anchor="middle">
            {mode === "month" ? series[index].key.slice(8) : series[index].key.slice(5)}
          </text>
        {/each}
      </svg>
    {/key}
  </section>

  <section class="ledger-chart-card">
    <header>
      <strong>分类占比</strong>
      <span class="ledger-stats-side">
        <button type="button" class:active={side === "expense"} on:click={() => (side = "expense")}>
          支出
        </button>
        <button type="button" class:active={side === "income"} on:click={() => (side = "income")}>
          收入
        </button>
      </span>
    </header>
    {#if stats.length === 0}
      <p class="ledger-day-empty">这个范围还没有{side === "expense" ? "支出" : "收入"}记录</p>
    {:else}
      <div class="ledger-donut-wrap">
        <svg class="ledger-donut" viewBox="0 0 160 160" role="img" aria-label="分类占比环">
          <g transform="rotate(-90 80 80)">
            {#each donutSegments as segment (segment.id)}
              <circle
                cx="80"
                cy="80"
                r={R}
                fill="none"
                stroke={segment.color}
                stroke-width="26"
                stroke-dasharray="{(segment.fraction * CIRC).toFixed(2)} {CIRC.toFixed(2)}"
                stroke-dashoffset={(-segment.offset * CIRC).toFixed(2)}
              />
            {/each}
          </g>
          <text class="ledger-donut-label" x="80" y="74" text-anchor="middle">
            {side === "expense" ? "总支出" : "总收入"}
          </text>
          <text class="ledger-donut-value" x="80" y="94" text-anchor="middle">
            {formatCents(statsTotal)}
          </text>
        </svg>
      </div>
      <ul class="ledger-rank">
        {#each stats as item (item.categoryId || "none")}
          {@const category = book.categories.find((entry) => entry.id === item.categoryId)}
          {@const icon = ledgerIcon(category?.icon, side === "income" ? "Banknote" : "Package")}
          <li>
            <button type="button" class="ledger-rank-head" on:click={() => toggleCategory(item.categoryId)}>
              <span class="ledger-row-icon" style="background:{segmentColor(item.categoryId)}">
                <svelte:component this={icon} size={15} />
              </span>
              <strong>{item.name}</strong>
              <em>{item.count} 笔 · {item.percent}%</em>
              <b>{formatCents(item.cents)}</b>
            </button>
            <span
              class="ledger-rank-bar"
              style="width:{item.percent}%;background:{segmentColor(item.categoryId)}"
            ></span>
            {#if openCategory === item.categoryId}
              <ul class="ledger-rank-children">
                {#each item.children as child (child.categoryId)}
                  <li>
                    <span>{child.name}</span>
                    <em>{child.count} 笔</em>
                    <b>{formatCents(child.cents)}</b>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<script lang="ts">
  /**
   * 统计视图：汇总卡 + 收支曲线 + 分类占比环与排行。
   * 曲线/环都是手写 SVG（描边动画），不引图表库——包体积与风格都不值，
   * server 管理台的活动曲线就是先例。月视图逐天、年视图逐月。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import { categoryColor, categoryStats, compactCents, formatCents, statsEntries, statsSeries } from "../ledger";
  import { ledgerIcon } from "../ledgerIcons";
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";
  import type { MonthCursor } from "../diary";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;

  const dispatch = createEventDispatcher<{ month: MonthCursor }>();

  let mode: "month" | "year" = "month";
  let side: LedgerSide = "expense";
  let openCategory = "";

  $: periodLabel = mode === "month" ? `${cursor.year}年${cursor.month + 1}月` : `${cursor.year}年`;
  // 曲线、占比、排行都吃同一个窗口——早前占比拿全量数据配当期汇总，两个数字对不上
  $: rangeEntries = statsEntries(entries, mode, cursor);
  $: series = statsSeries(entries, mode, cursor);
  $: totalIncome = series.reduce((sum, point) => sum + point.income, 0);
  $: totalExpense = series.reduce((sum, point) => sum + point.expense, 0);
  $: stats = categoryStats(book, rangeEntries, side).filter((item) => item.cents > 0);
  $: statsTotal = stats.reduce((sum, item) => sum + item.cents, 0);
  $: top = stats[0];

  // --- 曲线几何 ---
  // 只画当前侧的一条线：收/支共用一根纵轴时，一笔工资就能把整月的支出压成地板线
  const W = 680;
  const H = 220;
  const PAD_X = 40;
  const PAD_TOP = 18;
  const PAD_BOTTOM = 28;
  $: peak = Math.max(1, ...series.map((point) => point[side]));
  $: stepX = series.length > 1 ? (W - PAD_X * 2) / (series.length - 1) : 0;
  function pointX(index: number): number {
    return PAD_X + index * stepX;
  }
  function pointY(cents: number): number {
    return H - PAD_BOTTOM - (cents / peak) * (H - PAD_TOP - PAD_BOTTOM);
  }
  function linePath(): string {
    return series
      .map((point, index) => `${index === 0 ? "M" : "L"}${pointX(index).toFixed(1)},${pointY(point[side]).toFixed(1)}`)
      .join(" ");
  }
  function areaPath(): string {
    if (series.length === 0) return "";
    const base = (H - PAD_BOTTOM).toFixed(1);
    return `${linePath()} L${pointX(series.length - 1).toFixed(1)},${base} L${pointX(0).toFixed(1)},${base} Z`;
  }
  function axisLabel(value: number): string {
    // 金额单位是分：1 万元 = 1_000_000 分
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}万`;
    return compactCents(value);
  }
  $: axisValues = [peak, peak / 2, 0];
  $: tickIndexes = series
    .map((_, index) => index)
    .filter((index) => index % Math.max(1, Math.ceil(series.length / 8)) === 0);

  // --- 环几何 ---
  const R = 62;
  const CIRC = 2 * Math.PI * R;
  function colorOf(categoryId: string): string {
    return categoryColor(book, book.categories.find((item) => item.id === categoryId));
  }
  $: donutSegments = (() => {
    let offset = 0;
    return stats.slice(0, 12).map((item) => {
      const fraction = statsTotal > 0 ? item.cents / statsTotal : 0;
      const segment = { id: item.categoryId || "none", fraction, offset, color: colorOf(item.categoryId) };
      offset += fraction;
      return segment;
    });
  })();

  function step(delta: number): void {
    const date = new Date(cursor.year, cursor.month + delta, 1);
    dispatch("month", { year: date.getFullYear(), month: date.getMonth() });
  }

  function switchSide(next: LedgerSide): void {
    side = next;
    openCategory = "";
  }
</script>

<div class="ledger-stats">
  <div class="ledger-stats-bar">
    <div class="ledger-segmented" role="tablist" aria-label="统计范围">
      <button type="button" role="tab" class:active={mode === "month"} on:click|stopPropagation={() => (mode = "month")}>月</button>
      <button type="button" role="tab" class:active={mode === "year"} on:click|stopPropagation={() => (mode = "year")}>年</button>
    </div>
    <div class="ledger-segmented" role="tablist" aria-label="收支两侧">
      <button type="button" role="tab" class:active={side === "expense"} on:click|stopPropagation={() => switchSide("expense")}>支出</button>
      <button type="button" role="tab" class:active={side === "income"} on:click|stopPropagation={() => switchSide("income")}>收入</button>
    </div>
    <span class="ledger-stats-period">
      <button type="button" aria-label="上一段" on:click|stopPropagation={() => step(mode === "month" ? -1 : -12)}><ChevronLeft size={17} /></button>
      <strong>{periodLabel}</strong>
      <button type="button" aria-label="下一段" on:click|stopPropagation={() => step(mode === "month" ? 1 : 12)}><ChevronRight size={17} /></button>
    </span>
  </div>

  <section class="ledger-summary">
    <div>
      <span>收入</span>
      <strong class="in">{formatCents(totalIncome)}</strong>
    </div>
    <div>
      <span>支出</span>
      <strong class="out">{formatCents(totalExpense)}</strong>
    </div>
    <div>
      <span>结余</span>
      <strong class:out={totalIncome - totalExpense < 0}>{formatCents(totalIncome - totalExpense)}</strong>
    </div>
  </section>

  <section class="ledger-panel">
    <header class="ledger-panel-head">
      <h2>{side === "expense" ? "支出趋势" : "收入趋势"}</h2>
      <span class="ledger-legend">
        <i class={side === "expense" ? "out" : "in"}></i>{side === "expense" ? "支出" : "收入"}
        <em>{series.filter((point) => point[side] > 0).length} 个{mode === "month" ? "日子" : "月份"}</em>
      </span>
    </header>
    {#key `${mode}-${side}-${cursor.year}-${cursor.month}`}
      <svg class="ledger-line-chart" viewBox="0 0 {W} {H}" role="img" aria-label="收支趋势曲线">
        {#each axisValues as value (value)}
          <line class="ledger-chart-grid" x1={PAD_X} x2={W - PAD_X} y1={pointY(value)} y2={pointY(value)} />
          <text class="ledger-chart-axis" x={PAD_X - 8} y={pointY(value) + 4} text-anchor="end">{axisLabel(value)}</text>
        {/each}
        <path class="ledger-line-area {side === "expense" ? "out" : "in"}" d={areaPath()} />
        <path class="ledger-line {side === "expense" ? "out" : "in"}" d={linePath()} />
        {#each tickIndexes as index (index)}
          <text class="ledger-chart-axis" x={pointX(index)} y={H - 8} text-anchor="middle">
            {mode === "month" ? Number(series[index].key.slice(8)) : `${Number(series[index].key.slice(5))}月`}
          </text>
        {/each}
      </svg>
    {/key}
  </section>

  <section class="ledger-panel">
    <header class="ledger-panel-head">
      <h2>分类占比</h2>
    </header>

    {#if stats.length === 0}
      <div class="ledger-day-empty">{periodLabel}还没有{side === "expense" ? "支出" : "收入"}记录。</div>
    {:else}
      <div class="ledger-proportion">
        <div class="ledger-donut-wrap">
          <svg class="ledger-donut" viewBox="0 0 160 160" role="img" aria-label="分类占比环">
            <circle class="ledger-donut-track" cx="80" cy="80" r={R} fill="none" stroke-width="24" />
            <g transform="rotate(-90 80 80)">
              {#each donutSegments as segment (segment.id)}
                <circle
                  cx="80"
                  cy="80"
                  r={R}
                  fill="none"
                  stroke={segment.color}
                  stroke-width="24"
                  stroke-dasharray="{(segment.fraction * CIRC).toFixed(2)} {CIRC.toFixed(2)}"
                  stroke-dashoffset={(-segment.offset * CIRC).toFixed(2)}
                />
              {/each}
            </g>
            <text class="ledger-donut-label" x="80" y="72" text-anchor="middle">
              {top ? top.name : side === "expense" ? "总支出" : "总收入"}
            </text>
            <text class="ledger-donut-value" x="80" y="94" text-anchor="middle">
              {compactCents(statsTotal)}
            </text>
          </svg>
        </div>

        <ul class="ledger-rank">
          {#each stats as item (item.categoryId || "none")}
            {@const category = book.categories.find((entry) => entry.id === item.categoryId)}
            {@const icon = ledgerIcon(category?.icon, side === "income" ? "Banknote" : "Package")}
            {@const color = colorOf(item.categoryId)}
            <li class:open={openCategory === item.categoryId}>
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <button
                type="button"
                class="ledger-rank-head"
                on:click|stopPropagation={() => (openCategory = openCategory === item.categoryId ? "" : item.categoryId)}
              >
                <span class="ledger-rank-icon" style="--cat: {color}; background: {color}">
                  <svelte:component this={icon} size={15} />
                </span>
                <span class="ledger-rank-text">
                  <strong>{item.name}</strong>
                  <em>{item.count} 笔 · {item.percent}%</em>
                  <span class="ledger-rank-track"><i style="width: {item.percent}%; background: {color}"></i></span>
                </span>
                <b class="ledger-rank-amount">{formatCents(item.cents)}</b>
              </button>
              {#if openCategory === item.categoryId && item.children.length}
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
      </div>
    {/if}
  </section>
</div>

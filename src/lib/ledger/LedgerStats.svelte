<script lang="ts">
  /**
   * 统计视图：汇总卡 + 收支曲线 + 分类占比环与排行。
   * 曲线是手写 SVG（描边动画），环抽成了 LedgerDonut（钻取面板复用同一份）——
   * 不引图表库，包体积与风格都不值，server 管理台的活动曲线就是先例。
   * 月视图逐天、年视图逐月。
   *
   * 排行里点一个大类不再是就地展开子分类，而是派发 drill 让上层弹出钻取面板
   * （移动端下半屏、桌面端锚在这一行的下拉区）：子分类占比环 → 账单明细 → 改这一笔。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import { categoryColor, categoryStats, compactCents, formatCents, statsEntries, statsSeries } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";
  import { ledgerIcon } from "../ledgerIcons";
  import { fitAmount } from "../fitText";
  import LedgerDonut from "./LedgerDonut.svelte";
  import MonthPopover from "../MonthPopover.svelte";
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";
  import type { MonthCursor } from "../diary";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;

  const dispatch = createEventDispatcher<{
    month: MonthCursor;
    drill: { categoryId: string; side: LedgerSide; mode: "month" | "year"; cursor: MonthCursor; anchor: HTMLElement };
  }>();

  let mode: "month" | "year" = "month";
  let side: LedgerSide = "expense";
  let periodOpen = false;
  let periodEl: HTMLElement;

  $: periodLabel = mode === "month" ? `${cursor.year}年${cursor.month + 1}月` : `${cursor.year}年`;
  // 曲线、占比、排行都吃同一个窗口——早前占比拿全量数据配当期汇总，两个数字对不上
  $: rangeEntries = statsEntries(entries, mode, cursor);
  $: series = statsSeries(entries, mode, cursor);
  $: totalIncome = series.reduce((sum, point) => sum + point.income, 0);
  $: totalExpense = series.reduce((sum, point) => sum + point.expense, 0);
  $: stats = categoryStats(book, rangeEntries, side).filter((item) => item.cents > 0);
  $: statsTotal = stats.reduce((sum, item) => sum + item.cents, 0);

  function colorOf(categoryId: string): string {
    return categoryColor(book, book.categories.find((item) => item.id === categoryId));
  }

  $: donutItems = stats.slice(0, 12).map<LedgerDonutItem>((item) => ({
    id: item.categoryId || "none",
    name: item.name,
    cents: item.cents,
    count: item.count,
    color: colorOf(item.categoryId)
  }));

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

  function step(delta: number): void {
    periodOpen = false;
    const date = new Date(cursor.year, cursor.month + delta, 1);
    dispatch("month", { year: date.getFullYear(), month: date.getMonth() });
  }

  function pickPeriod(next: { year: number; month: number }): void {
    dispatch("month", { year: next.year, month: next.month });
  }

  function switchSide(next: LedgerSide): void {
    side = next;
  }

  function drill(item: { categoryId: string }, anchor: HTMLElement): void {
    periodOpen = false;
    dispatch("drill", { categoryId: item.categoryId, side, mode, cursor, anchor });
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
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
      <strong
        bind:this={periodEl}
        class="month-pop-anchor"
        role="button"
        tabindex="0"
        title="点击直接选年月"
        on:click|stopPropagation={() => (periodOpen = !periodOpen)}
      >{periodLabel}</strong>
      <button type="button" aria-label="下一段" on:click|stopPropagation={() => step(mode === "month" ? 1 : 12)}><ChevronRight size={17} /></button>
    </span>
  </div>

  <MonthPopover
    open={periodOpen}
    anchor={periodEl}
    year={cursor.year}
    month={cursor.month}
    mode={mode}
    onSelect={pickPeriod}
    onClose={() => (periodOpen = false)}
  />

  <section class="ledger-summary">
    <div>
      <span>收入</span>
      <strong class="in" use:fitAmount={totalIncome}>{formatCents(totalIncome)}</strong>
    </div>
    <div>
      <span>支出</span>
      <strong class="out" use:fitAmount={totalExpense}>{formatCents(totalExpense)}</strong>
    </div>
    <div>
      <span>结余</span>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <strong
        class:out={totalIncome - totalExpense < 0}
        use:fitAmount={totalIncome - totalExpense}
      >{formatCents(totalIncome - totalExpense)}</strong>
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
      <span class="ledger-legend"><em>点一类看它的明细</em></span>
    </header>

    {#if stats.length === 0}
      <div class="ledger-day-empty">{periodLabel}还没有{side === "expense" ? "支出" : "收入"}记录。</div>
    {:else}
      <div class="ledger-proportion">
        <div class="ledger-donut-wrap">
          <LedgerDonut items={donutItems} total={statsTotal} totalLabel={side === "expense" ? "总支出" : "总收入"} />
        </div>

        <ul class="ledger-rank">
          {#each stats as item (item.categoryId || "none")}
            {@const category = book.categories.find((entry) => entry.id === item.categoryId)}
            {@const icon = ledgerIcon(category?.icon, side === "income" ? "Banknote" : "Package")}
            {@const color = colorOf(item.categoryId)}
            <li>
              <button
                type="button"
                class="ledger-rank-head"
                title="查看这一类的二级分类与账单明细"
                on:click|stopPropagation={(event) => drill(item, event.currentTarget)}
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
                <ChevronRight class="ledger-rank-go" size={16} />
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </section>
</div>

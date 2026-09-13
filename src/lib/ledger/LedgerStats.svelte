<script lang="ts">
  /**
   * 统计视图：顶行两个段控（左 = 周期 周/月/年/总/自定义，右 = 侧 支出/收入/收支），
   * 下面一整块白底圆角卡 = 可点周期标签 + 三标签 + 三数额，再往下是趋势与分类占比。
   * 曲线手写 SVG（描边动画），环抽成了 LedgerDonut（钻取面板复用同一份）——
   * 不引图表库，包体积与风格都不值，server 管理台的活动曲线就是先例。
   *
   * 侧选「收支」时趋势把收/支两条线画在同一张图上（共用纵轴此时才成立：
   * 用户明确要对照两者）；分类占比仍按支出画（占比环的语义是"钱花在哪"）。
   * 排行里点一个大类派发 drill 让上层弹出钻取面板（移动端下半屏、桌面端锚定下拉）。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import {
    categoryColor, categoryStats, compactCents, formatCents, statsBounds, statsEntries,
    statsPeriodLabel, statsSeries, shiftWeek, paletteColor, type StatsMode
  } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";
  import { ledgerIcon } from "../ledgerIcons";
  import { fitAmount } from "../fitText";
  import { todayDate } from "../diary";
  import LedgerDonut from "./LedgerDonut.svelte";
  import MonthPopover from "../MonthPopover.svelte";
  import DatePicker from "../DatePicker.svelte";
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";
  import type { MonthCursor } from "../diary";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;

  const dispatch = createEventDispatcher<{
    month: MonthCursor;
    drill: {
      categoryId: string;
      side: LedgerSide;
      from: string;
      to: string;
      periodLabel: string;
      anchor: HTMLElement;
    };
  }>();

  type Side = LedgerSide | "both";

  const MODES: Array<{ id: StatsMode; label: string }> = [
    { id: "week", label: "周" },
    { id: "month", label: "月" },
    { id: "year", label: "年" },
    { id: "total", label: "总" },
    { id: "custom", label: "自定义" }
  ];

  let mode: StatsMode = "month";
  let side: Side = "expense";
  /** 周周期的锚点日（周一起算那一周）；自定义周期的起止 */
  let weekAnchor = todayDate();
  let customFrom = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}-01`;
  let customTo = todayDate();
  let popOpen: "" | "month" | "week" | "from" | "to" = "";
  let periodEl: HTMLElement;
  let fromEl: HTMLElement;
  let toEl: HTMLElement;

  $: bounds = statsBounds(entries, mode, cursor, weekAnchor, customFrom, customTo);
  $: periodLabel = statsPeriodLabel(mode, bounds, cursor);
  // 曲线、占比、排行都吃同一个窗口——早前占比拿全量数据配当期汇总，两个数字对不上
  $: rangeEntries = statsEntries(entries, bounds);
  $: series = statsSeries(entries, bounds);
  $: totalIncome = series.reduce((sum, point) => sum + point.income, 0);
  $: totalExpense = series.reduce((sum, point) => sum + point.expense, 0);
  /** 侧 = 收支时占比环仍画支出：环的语义是"钱花在哪一类" */
  let catSide: LedgerSide = "expense";
  $: catSide = side === "both" ? "expense" : side;
  $: stats = categoryStats(book, rangeEntries, catSide).filter((item) => item.cents > 0);
  $: statsTotal = stats.reduce((sum, item) => sum + item.cents, 0);
  let bucket: "day" | "month" = "day";
  $: bucket = series.length > 0 && series[0].key.length > 7 ? "month" : "day";

  function colorOf(categoryId: string, index: number): string {
    const category = book.categories.find((item) => item.id === categoryId);
    return category?.color || paletteColor(index);
  }

  $: donutItems = stats.slice(0, 12).map<LedgerDonutItem>((item, index) => ({
    id: item.categoryId || "none",
    name: item.name,
    cents: item.cents,
    count: item.count,
    color: colorOf(item.categoryId, index)
  }));

  // --- 曲线几何 ---
  const W = 680;
  const H = 220;
  const PAD_X = 40;
  const PAD_TOP = 18;
  const PAD_BOTTOM = 28;
  $: peak = Math.max(
    1,
    ...series.map((point) => (side === "income" ? point.income : side === "expense" ? point.expense : Math.max(point.income, point.expense)))
  );
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
    popOpen = "";
    if (mode === "week") {
      weekAnchor = shiftWeek(weekAnchor, delta);
      return;
    }
    if (mode === "month") {
      const date = new Date(cursor.year, cursor.month + delta, 1);
      dispatch("month", { year: date.getFullYear(), month: date.getMonth() });
      return;
    }
    if (mode === "year") {
      dispatch("month", { year: cursor.year + delta, month: cursor.month });
    }
  }

  function pickPeriod(next: { year: number; month: number }): void {
    popOpen = "";
    dispatch("month", { year: next.year, month: next.month });
  }

  function switchMode(next: StatsMode): void {
    mode = next;
    popOpen = "";
  }

  function switchSide(next: Side): void {
    side = next;
  }

  function drill(item: { categoryId: string }, anchor: HTMLElement): void {
    popOpen = "";
    dispatch("drill", { categoryId: item.categoryId, side: catSide, from: bounds.from, to: bounds.to, periodLabel, anchor });
  }

  function slash(date: string): string {
    return date.replaceAll("-", "/");
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    if (popOpen) {
      event.preventDefault();
      event.stopPropagation();
      popOpen = "";
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="ledger-stats">
  <div class="ledger-stats-bar">
    <div class="ledger-segmented" role="tablist" aria-label="统计范围">
      {#each MODES as item (item.id)}
        <button type="button" role="tab" class:active={mode === item.id} on:click|stopPropagation={() => switchMode(item.id)}>
          {item.label}
        </button>
      {/each}
    </div>
    <div class="ledger-segmented ledger-side-switch" role="tablist" aria-label="收支两侧">
      <button type="button" role="tab" class:active={side === "expense"} on:click|stopPropagation={() => switchSide("expense")}>支出</button>
      <button type="button" role="tab" class:active={side === "income"} on:click|stopPropagation={() => switchSide("income")}>收入</button>
      <button type="button" role="tab" class:active={side === "both"} on:click|stopPropagation={() => switchSide("both")}>收支</button>
    </div>
  </div>

  <section class="ledger-panel ledger-summary-card">
    <div class="ledger-stats-period">
      {#if mode === "custom"}
        <span class="ledger-custom-field" class:open={popOpen === "from"}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <strong
            bind:this={fromEl}
            role="button"
            tabindex="0"
            title="起始日期"
            on:click|stopPropagation={() => (popOpen = popOpen === "from" ? "" : "from")}
          >{slash(bounds.from)}</strong>
          {#if popOpen === "from"}
            <div class="ledger-pop date">
              <DatePicker
                value={customFrom}
                on:select={(event) => {
                  customFrom = event.detail;
                  if (customTo < customFrom) customTo = customFrom;
                  popOpen = "";
                }}
              />
            </div>
          {/if}
        </span>
        <span class="ledger-custom-sep">-</span>
        <span class="ledger-custom-field" class:open={popOpen === "to"}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <strong
            bind:this={toEl}
            role="button"
            tabindex="0"
            title="结束日期"
            on:click|stopPropagation={() => (popOpen = popOpen === "to" ? "" : "to")}
          >{slash(bounds.to)}</strong>
          {#if popOpen === "to"}
            <div class="ledger-pop date">
              <DatePicker
                value={customTo}
                on:select={(event) => {
                  customTo = event.detail;
                  if (customFrom > customTo) customFrom = customTo;
                  popOpen = "";
                }}
              />
            </div>
          {/if}
        </span>
      {:else}
        {#if mode !== "total"}
          <button type="button" aria-label="上一段" on:click|stopPropagation={() => step(-1)}><ChevronLeft size={17} /></button>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
        <strong
          bind:this={periodEl}
          class="month-pop-anchor"
          class:pickable={mode === "month" || mode === "year" || mode === "week"}
          role="button"
          tabindex="0"
          title={mode === "total" ? "全部流水的跨度" : "点击直接选周期"}
          on:click|stopPropagation={() => {
            if (mode === "month" || mode === "year") popOpen = popOpen === "month" ? "" : "month";
            else if (mode === "week") popOpen = popOpen === "week" ? "" : "week";
          }}
        >{periodLabel}</strong>
        {#if mode !== "total"}
          <button type="button" aria-label="下一段" on:click|stopPropagation={() => step(1)}><ChevronRight size={17} /></button>
        {/if}
      {/if}
    </div>

    {#if mode === "month" || mode === "year"}
      <MonthPopover
        open={popOpen === "month"}
        anchor={periodEl}
        year={cursor.year}
        month={cursor.month}
        mode={mode === "year" ? "year" : "month"}
        onSelect={pickPeriod}
        onClose={() => (popOpen = "")}
      />
    {:else if mode === "week"}
      {#if popOpen === "week"}
        <div class="ledger-pop date ledger-week-pop">
          <DatePicker
            value={weekAnchor}
            on:select={(event) => {
              weekAnchor = event.detail;
              popOpen = "";
            }}
          />
        </div>
      {/if}
    {/if}

    <div class="ledger-summary-labels">
      <span>支出</span>
      <span>收入</span>
      <span>结余</span>
    </div>
    <div class="ledger-summary">
      <strong class="out" use:fitAmount={totalExpense}>{formatCents(totalExpense)}</strong>
      <strong class="in" use:fitAmount={totalIncome}>{formatCents(totalIncome)}</strong>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <strong class:out={totalIncome - totalExpense < 0} use:fitAmount={totalIncome - totalExpense}>
        {formatCents(totalIncome - totalExpense)}
      </strong>
    </div>
  </section>

  <section class="ledger-panel">
    <header class="ledger-panel-head">
      <h2>{side === "expense" ? "支出趋势" : side === "income" ? "收入趋势" : "收支趋势"}</h2>
      <span class="ledger-legend">
        {#if side !== "income"}<i class="out"></i>支出{/if}
        {#if side !== "expense"}<i class="in"></i>收入{/if}
      </span>
    </header>
    {#key `${mode}-${side}-${bounds.from}-${bounds.to}`}
      <svg class="ledger-line-chart" viewBox="0 0 {W} {H}" role="img" aria-label="收支趋势曲线">
        {#each axisValues as value (value)}
          <line class="ledger-chart-grid" x1={PAD_X} x2={W - PAD_X} y1={pointY(value)} y2={pointY(value)} />
          <text class="ledger-chart-axis" x={PAD_X - 8} y={pointY(value) + 4} text-anchor="end">{axisLabel(value)}</text>
        {/each}
        {#if side !== "both"}
          {@const key = side === "income" ? "income" : "expense"}
          <path class="ledger-line-area {key === "income" ? "in" : "out"}" d={areaPath(key)} />
        {/if}
        {#if side !== "income"}<path class="ledger-line out" d={linePath("expense")} />{/if}
        {#if side !== "expense"}<path class="ledger-line in" d={linePath("income")} />{/if}
        {#each tickIndexes as index (index)}
          <text class="ledger-chart-axis" x={pointX(index)} y={H - 8} text-anchor="middle">
            {bucket === "day" ? Number(series[index].key.slice(8)) : `${Number(series[index].key.slice(5))}月`}
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
      <div class="ledger-day-empty">{periodLabel}还没有{catSide === "expense" ? "支出" : "收入"}记录。</div>
    {:else}
      <div class="ledger-proportion">
        <div class="ledger-donut-wrap">
          <LedgerDonut items={donutItems} total={statsTotal} totalLabel={catSide === "expense" ? "总支出" : "总收入"} />
        </div>

        <ul class="ledger-rank">
          {#each stats as item (item.categoryId || "none")}
            {@const category = book.categories.find((entry) => entry.id === item.categoryId)}
            {@const icon = ledgerIcon(category?.icon, catSide === "income" ? "Banknote" : "Package")}
            {@const color = categoryColor(book, category)}
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

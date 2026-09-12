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

  // --- 环几何（viewBox 460×260，圆心 230/130）---
  const CX = 230;
  const CY = 130;
  const R = 72;
  const RING = 26;
  const CIRC = 2 * Math.PI * R;
  /** 引线只给占比够大的分类画：小切片的名字挤在一起反而读不出来，排行列表里有全量 */
  const LABEL_MIN_FRACTION = 0.045;
  const LABEL_GAP = 17;

  function colorOf(categoryId: string): string {
    return categoryColor(book, book.categories.find((item) => item.id === categoryId));
  }

  $: donutSegments = (() => {
    let offset = 0;
    return stats.slice(0, 12).map((item) => {
      const fraction = statsTotal > 0 ? item.cents / statsTotal : 0;
      // -90° 起算：第一片从正上方开始，顺时针
      const mid = (offset + fraction / 2) * 2 * Math.PI - Math.PI / 2;
      const segment = {
        id: item.categoryId || "none",
        name: item.name,
        cents: item.cents,
        fraction,
        offset,
        color: colorOf(item.categoryId),
        mid,
        // 选中时沿中角挪出去的方向分量（交给 CSS transform，属性 transform 不做过渡）
        dx: 10 * Math.cos(mid),
        dy: 10 * Math.sin(mid)
      };
      offset += fraction;
      return segment;
    });
  })();

  /** 点中的那一片：放大挪出去，环心显示它的名字与金额；再点一下取消。 */
  let focusId = "";
  $: focused = donutSegments.find((segment) => segment.id === focusId) ?? null;
  $: centerLabel = focused ? focused.name : side === "expense" ? "总支出" : "总收入";
  $: centerValue = compactCents(focused ? focused.cents : statsTotal);
  $: centerNote = focused
    ? `${Math.round(focused.fraction * 100)}% · ${stats.find((item) => (item.categoryId || "none") === focused.id)?.count ?? 0} 笔`
    : "";

  /** 引线标签：先按切片中角算拐点，再把同一侧上下挨太近的名字推开（否则叠字）。 */
  $: donutLabels = (() => {
    type Label = {
      id: string; text: string; x1: number; y1: number; x2: number; y2: number;
      x3: number; y3: number; right: boolean;
    };
    const candidates: Label[] = [];
    for (const segment of donutSegments) {
      if (segment.fraction < LABEL_MIN_FRACTION) continue;
      const cos = Math.cos(segment.mid);
      const sin = Math.sin(segment.mid);
      const right = cos >= 0;
      const x2 = CX + (R + RING / 2 + 14) * cos;
      const y2 = CY + (R + RING / 2 + 14) * sin;
      candidates.push({
        id: segment.id,
        text: `${segment.name.length > 8 ? `${segment.name.slice(0, 8)}…` : segment.name} ${Math.round(segment.fraction * 100)}%`,
        x1: CX + (R + RING / 2 + 3) * cos,
        y1: CY + (R + RING / 2 + 3) * sin,
        x2,
        y2,
        x3: right ? CX + R + RING / 2 + 44 : CX - R - RING / 2 - 44,
        y3: y2,
        right
      });
    }
    for (const right of [true, false]) {
      const group = candidates.filter((item) => item.right === right).sort((a, b) => a.y2 - b.y2);
      let previous = -Number.MAX_VALUE;
      for (const item of group) {
        item.y3 = Math.max(item.y2, previous + LABEL_GAP);
        previous = item.y3;
      }
      // 推开后可能超出画布下缘：整组往上收回
      const overflow = previous + 14 - 260;
      if (overflow > 0) {
        for (const item of group) item.y3 = Math.max(16, item.y3 - overflow);
      }
    }
    return candidates;
  })();

  function toggleFocus(id: string): void {
    focusId = focusId === id ? "" : id;
  }

  function step(delta: number): void {
    focusId = "";
    const date = new Date(cursor.year, cursor.month + delta, 1);
    dispatch("month", { year: date.getFullYear(), month: date.getMonth() });
  }

  function switchSide(next: LedgerSide): void {
    side = next;
    openCategory = "";
    focusId = "";
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
          <svg class="ledger-donut" viewBox="0 0 460 260" role="img" aria-label="分类占比环">
            <circle class="ledger-donut-track" cx={CX} cy={CY} r={R} fill="none" stroke-width={RING} />
            {#each donutSegments as segment (segment.id)}
              <g
                class="ledger-donut-slice"
                class:focus={segment.id === focusId}
                style="--dx: {segment.dx.toFixed(2)}px; --dy: {segment.dy.toFixed(2)}px"
              >
                <circle
                  cx={CX}
                  cy={CY}
                  r={R}
                  fill="none"
                  stroke={segment.color}
                  stroke-width={RING}
                  stroke-dasharray="{(segment.fraction * CIRC).toFixed(2)} {CIRC.toFixed(2)}"
                  stroke-dashoffset={(-segment.offset * CIRC).toFixed(2)}
                  transform="rotate(-90 {CX} {CY})"
                  role="button"
                  tabindex="0"
                  aria-label="{segment.name} {compactCents(segment.cents)}，点击在环心显示这一类"
                  on:click|stopPropagation={() => toggleFocus(segment.id)}
                  on:keydown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      toggleFocus(segment.id);
                    }
                  }}
                />
              </g>
            {/each}
            {#each donutLabels as label (label.id)}
              <polyline
                class="ledger-donut-leader"
                points="{label.x1.toFixed(1)},{label.y1.toFixed(1)} {label.x2.toFixed(1)},{label.y2.toFixed(1)} {label.x3.toFixed(1)},{label.y3.toFixed(1)}"
              />
              <text
                class="ledger-donut-tag"
                x={label.right ? label.x3 + 5 : label.x3 - 5}
                y={label.y3 + 4}
                text-anchor={label.right ? "start" : "end"}
              >{label.text}</text>
            {/each}
            <text class="ledger-donut-label" x={CX} y={focused ? CY - 18 : CY - 8} text-anchor="middle">{centerLabel}</text>
            <text class="ledger-donut-value" x={CX} y={focused ? CY + 12 : CY + 18} text-anchor="middle">{centerValue}</text>
            {#if centerNote}
              <text class="ledger-donut-note" x={CX} y={CY + 34} text-anchor="middle">{centerNote}</text>
            {/if}
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

<script lang="ts">
  /**
   * 日历视图：月历热力图（收入绿 / 支出红，金额越大颜色越深）+ 选中当天的明细。
   * 热力等级在 ledger.ts 的 heatCells 里算（按本月单日峰值分四档）。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import type { LedgerBook, LedgerEntry } from "../types";
  import type { MonthCursor } from "../diary";
  import { calendarWeekdayHeaders, fullDayLabel, shiftMonth } from "../diary";
  import { heatCells, formatCents, sortEntries } from "../ledger";
  import LedgerRow from "./LedgerRow.svelte";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;
  export let selectedDate: string;
  export let today: string;

  const dispatch = createEventDispatcher<{
    edit: string;
    context: { id: string; x: number; y: number };
    month: MonthCursor;
    day: string;
  }>();

  $: cells = heatCells(cursor, entries);
  $: dayEntries = sortEntries(entries.filter((entry) => entry.date === selectedDate));
  $: dayIncome = dayEntries
    .filter((entry) => entry.kind === "income")
    .reduce((sum, entry) => sum + entry.amountCents, 0);
  $: dayExpense = dayEntries
    .filter((entry) => entry.kind === "expense")
    .reduce((sum, entry) => sum + entry.amountCents, 0);

  function heatStyle(cell: { level: number; side: string | null }): string {
    if (!cell.side || cell.level === 0) return "";
    const base = cell.side === "income" ? "47, 159, 110" : "217, 83, 79";
    const alpha = 0.1 + cell.level * 0.11;
    return `background: rgba(${base}, ${alpha.toFixed(2)});`;
  }
</script>

<div class="ledger-calendar">
  <div class="ledger-calendar-bar">
    <button type="button" on:click={() => dispatch("month", shiftMonth(cursor, -1))}>
      <ChevronLeft size={17} />
    </button>
    <strong>{cursor.year} 年 {cursor.month + 1} 月</strong>
    <button type="button" on:click={() => dispatch("month", shiftMonth(cursor, 1))}>
      <ChevronRight size={17} />
    </button>
  </div>
  <div class="ledger-heat-grid">
    {#each calendarWeekdayHeaders as head}
      <span class="ledger-heat-head">{head}</span>
    {/each}
    {#each cells as cell (cell.date)}
      <button
        type="button"
        class="ledger-heat-cell"
        class:other-month={cell.otherMonth}
        class:today={cell.date === today}
        class:selected={cell.date === selectedDate}
        style={heatStyle(cell)}
        title="{cell.date} 收 {formatCents(cell.income)} / 支 {formatCents(cell.expense)}"
        on:click={() => dispatch("day", cell.date)}
      >
        <span class="ledger-heat-day">{cell.day}</span>
        {#if cell.income > 0 && cell.expense > 0}
          <span class="ledger-heat-both">±</span>
        {:else if cell.income > 0}
          <span class="ledger-heat-mark income">+{formatCents(cell.income)}</span>
        {:else if cell.expense > 0}
          <span class="ledger-heat-mark expense">-{formatCents(cell.expense)}</span>
        {/if}
      </button>
    {/each}
  </div>
</div>

<div class="ledger-day-head ledger-day-detail-head">
  <strong>{fullDayLabel(selectedDate)}</strong>
  <span>
    {#if dayIncome > 0}<i class="income">收 {formatCents(dayIncome)}</i>{/if}
    {#if dayExpense > 0}<i class="expense">支 {formatCents(dayExpense)}</i>{/if}
  </span>
</div>
{#each dayEntries as entry (entry.id)}
  <LedgerRow
    {entry}
    {book}
    on:edit={(event) => dispatch("edit", event.detail)}
    on:context={(event) => dispatch("context", event.detail)}
  />
{:else}
  <p class="ledger-day-empty">这一天还没有记过账，点右下角 + 记一笔</p>
{/each}

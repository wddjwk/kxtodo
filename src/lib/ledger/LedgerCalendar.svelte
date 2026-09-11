<script lang="ts">
  /**
   * 日历视图：与日记的日历同一条骨架（月份条 + 周头 + 格子 + 选中日明细）。
   * 格子里直接写当天的收/支数额——不做热力着色，数额本身就是最直白的信息。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import { calendarWeekdayHeaders, fullDayLabel, relativeDayLabel, shiftMonth } from "../diary";
  import { compactCents, dayGroup, ledgerCalendarCells, monthTotals } from "../ledger";
  import LedgerDayCard from "./LedgerDayCard.svelte";
  import type { LedgerBook } from "../types";
  import type { MonthCursor } from "../diary";

  export let book: LedgerBook;
  export let cursor: MonthCursor;
  export let selectedDate: string;
  export let today = "";
  export let selectedId = "";

  const dispatch = createEventDispatcher<{
    month: MonthCursor;
    day: string;
    edit: string;
    add: string;
    context: { id: string; x: number; y: number };
  }>();

  $: cells = ledgerCalendarCells(cursor, book.entries);
  $: monthLabel = `${cursor.year}年${cursor.month + 1}月`;
  $: totals = monthTotals(book.entries, cursor);
  $: group = dayGroup(book.entries, selectedDate);
</script>

<div class="ledger-calendar">
  <div class="ledger-calendar-bar">
    <button type="button" aria-label="上个月" on:click|stopPropagation={() => dispatch("month", shiftMonth(cursor, -1))}>
      <ChevronLeft size={18} />
    </button>
    <strong>{monthLabel}</strong>
    <button type="button" aria-label="下个月" on:click|stopPropagation={() => dispatch("month", shiftMonth(cursor, 1))}>
      <ChevronRight size={18} />
    </button>
  </div>

  <div class="ledger-calendar-sums">
    <span class="in">收 {compactCents(totals.income)}</span>
    <span class="out">支 {compactCents(totals.expense)}</span>
    <span class="net">结余 {compactCents(totals.income - totals.expense)}</span>
  </div>

  <div class="ledger-calendar-grid">
    {#each calendarWeekdayHeaders as label (label)}
      <span class="ledger-calendar-head">{label}</span>
    {/each}
    {#each cells as cell (cell.date)}
      <button
        type="button"
        class="ledger-calendar-cell"
        class:other-month={cell.otherMonth}
        class:today={cell.date === today}
        class:selected={cell.date === selectedDate}
        on:click|stopPropagation={() => dispatch("day", cell.date)}
      >
        <span class="ledger-cell-day">{cell.day}</span>
        <span class="ledger-cell-amounts">
          {#if cell.expense > 0}<em class="out">{compactCents(cell.expense)}</em>{/if}
          {#if cell.income > 0}<em class="in">{compactCents(cell.income)}</em>{/if}
        </span>
      </button>
    {/each}
  </div>
</div>

<div class="ledger-day-head">
  <strong>{fullDayLabel(selectedDate)}</strong>
  <span>
    {relativeDayLabel(selectedDate, today)}
    {#if group}
      · {group.entries.length} 笔 · 支 {compactCents(group.expense)} · 收 {compactCents(group.income)}
    {:else}
      · 没有记账
    {/if}
  </span>
</div>

{#if group}
  <LedgerDayCard
    {book}
    {group}
    {today}
    showDate={false}
    {selectedId}
    on:edit={(event) => dispatch("edit", event.detail)}
    on:add={(event) => dispatch("add", event.detail)}
    on:context={(event) => dispatch("context", event.detail)}
  />
{:else}
  <div class="ledger-day-empty">这一天还没有记账，点右下角 + 记一笔。</div>
{/if}

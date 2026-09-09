<script lang="ts">
  /**
   * 列表视图：按天的卡片流，一个月一段（段头 = 年月 + 该月收支合计）。
   * 月份栈由父级维护：滚到底自动接上一个月、滚到顶接回下一个月，
   * 于是「往下翻」就是自然地往过去走，与时光序的手感一致。
   */
  import { createEventDispatcher } from "svelte";
  import type { LedgerBook, LedgerEntry } from "../types";
  import type { MonthCursor } from "../diary";
  import { monthDayGroups, monthTotals, formatCents } from "../ledger";
  import { weekdayOf, monthDayLabel } from "../diary";
  import LedgerRow from "./LedgerRow.svelte";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let months: MonthCursor[];
  export let today: string;

  const dispatch = createEventDispatcher<{
    edit: string;
    context: { id: string; x: number; y: number };
  }>();

  $: sections = months.map((cursor) => {
    const groups = monthDayGroups(entries, cursor);
    const totals = monthTotals(entries, cursor);
    return { cursor, groups, totals };
  });

  function dayLabel(date: string): string {
    if (date === today) return "今天";
    return `${monthDayLabel(date)} ${weekdayOf(date)}`;
  }
</script>

{#each sections as section (`${section.cursor.year}-${section.cursor.month}`)}
  <section class="ledger-month">
    <header class="ledger-month-head">
      <strong>{section.cursor.year} 年 {section.cursor.month + 1} 月</strong>
      <span>
        收 <b class="income">{formatCents(section.totals.income)}</b>
        支 <b class="expense">{formatCents(section.totals.expense)}</b>
      </span>
    </header>
    {#each section.groups as group (group.date)}
      <div class="ledger-day">
        <header class="ledger-day-head">
          <strong>{dayLabel(group.date)}</strong>
          <span>
            {#if group.income > 0}<i class="income">收 {formatCents(group.income)}</i>{/if}
            {#if group.expense > 0}<i class="expense">支 {formatCents(group.expense)}</i>{/if}
          </span>
        </header>
        {#each group.entries as entry (entry.id)}
          <LedgerRow
            {entry}
            {book}
            on:edit={(event) => dispatch("edit", event.detail)}
            on:context={(event) => dispatch("context", event.detail)}
          />
        {/each}
      </div>
    {:else}
      <p class="ledger-empty-month">这个月还没有记过账</p>
    {/each}
  </section>
{/each}

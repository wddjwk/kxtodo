<script lang="ts">
  /**
   * 记账的一天 = 一张卡片。标题行 = 日期 + 周几 + 右端当天收/支与「在这天记一笔」；
   * 每一笔的排版在 LedgerEntryRow 里（列表 / 日历选中日 / 钻取明细 / 搜索结果共用一份）。
   */
  import { createEventDispatcher } from "svelte";
  import { Plus } from "@lucide/svelte";
  import { compactCents } from "../ledger";
  import { relativeDayLabel, weekdayOf } from "../diary";
  import LedgerEntryRow from "./LedgerEntryRow.svelte";
  import type { LedgerBook } from "../types";
  import type { LedgerDayGroup } from "../ledger";

  export let book: LedgerBook;
  export let group: LedgerDayGroup;
  export let today = "";
  export let selectedId = "";

  const dispatch = createEventDispatcher<{
    edit: string;
    add: string;
    image: string;
    context: { id: string; x: number; y: number };
  }>();

  $: dayNumber = Number.parseInt(group.date.slice(8, 10), 10);
  $: dayMonth = Number.parseInt(group.date.slice(5, 7), 10);
  $: weekday = weekdayOf(group.date);
  $: dayLabel = relativeDayLabel(group.date, today);
  $: dayTitle = `${dayMonth}/${dayNumber}`;

  function addHere(): void {
    dispatch("add", group.date);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<article class="ledger-card" on:contextmenu|preventDefault|stopPropagation>
  <header class="ledger-card-head">
    <h3 class="ledger-card-title" title={dayLabel}>{dayTitle}</h3>
    <span class="ledger-date-week">{weekday}</span>
    <span class="ledger-card-sums">
      <em title="当天收入">收 {compactCents(group.income)}</em>
      <em title="当天支出">支 {compactCents(group.expense)}</em>
    </span>
    <button class="ledger-card-add" type="button" title="在这天记一笔" on:click|stopPropagation={addHere}>
      <Plus size={15} />
    </button>
  </header>

  <div class="ledger-entry-list">
    {#each group.entries as entry (entry.id)}
      <LedgerEntryRow
        {book}
        {entry}
        selected={selectedId === entry.id}
        on:edit={(event) => dispatch("edit", event.detail)}
        on:image={(event) => dispatch("image", event.detail)}
        on:context={(event) => dispatch("context", event.detail)}
      />
    {/each}
  </div>
</article>

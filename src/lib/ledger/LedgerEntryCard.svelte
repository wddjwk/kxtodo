<script lang="ts">
  /**
   * 搜索结果里的一条账（单条卡片，不是按天卡片）：标题行 = 月/日 + 周几，
   * 下面就是那一笔（与列表视图同一套 LedgerEntryRow）。
   * 记账页的搜索与全局搜索混排共用这一张卡。
   */
  import { createEventDispatcher } from "svelte";
  import { relativeDayLabel, weekdayOf } from "../diary";
  import { todayDate } from "../diary";
  import LedgerEntryRow from "./LedgerEntryRow.svelte";
  import type { LedgerBook, LedgerEntry } from "../types";

  export let book: LedgerBook;
  export let entry: LedgerEntry;
  export let selected = false;

  const dispatch = createEventDispatcher<{
    edit: string;
    image: string;
    context: { id: string; x: number; y: number };
  }>();

  $: today = todayDate();
  $: dayTitle = `${Number(entry.date.slice(5, 7))}/${Number(entry.date.slice(8, 10))}`;
  $: dayLabel = relativeDayLabel(entry.date, today);
  $: weekday = weekdayOf(entry.date);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<article class="ledger-card ledger-result-card" on:contextmenu|preventDefault|stopPropagation>
  <header class="ledger-card-head">
    <h3 class="ledger-card-title" title={dayLabel}>{dayTitle}</h3>
    <span class="ledger-date-week">{weekday}</span>
  </header>
  <div class="ledger-entry-list">
    <LedgerEntryRow
      {book}
      {entry}
      {selected}
      on:edit={(event) => dispatch("edit", event.detail)}
      on:image={(event) => dispatch("image", event.detail)}
      on:context={(event) => dispatch("context", event.detail)}
    />
  </div>
</article>

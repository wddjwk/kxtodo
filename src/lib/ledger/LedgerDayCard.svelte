<script lang="ts">
  /**
   * 记账的一天 = 一张卡片（与日记卡片同一套组织形式），单列布局：
   * 标题行是日期（主题色的日期号 + 日期 + 周几，右侧当天收/支），下面每一笔一行、左对齐。
   * 不做折叠——一天的笔数本来就该一眼看完，折叠只会把信息藏起来。
   */
  import { createEventDispatcher } from "svelte";
  import { Plus } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "../longpress";
  import { compactCents, formatCents } from "../ledger";
  import { ledgerIcon, softColor, ACCOUNT_KIND_ICON, TRANSFER_ICON } from "../ledgerIcons";
  import { monthDayLabel, relativeDayLabel, weekdayOf } from "../diary";
  import type { LedgerBook, LedgerEntry } from "../types";
  import type { LedgerDayGroup } from "../ledger";

  export let book: LedgerBook;
  export let group: LedgerDayGroup;
  export let today = "";
  /** 日历视图里日期已经写在日头上了，卡片就不再重复一遍 */
  export let showDate = true;
  export let selectedId = "";

  const dispatch = createEventDispatcher<{
    edit: string;
    add: string;
    context: { id: string; x: number; y: number };
  }>();

  $: dayNumber = group.date.slice(8, 10);
  $: weekday = weekdayOf(group.date);
  $: dayLabel = relativeDayLabel(group.date, today);
  /** 标题行的日期：口语标签（今天/昨天/前天）或「9月10日」——完整标签自带周几，会和旁边那格重复 */
  $: dayTitle =
    dayLabel === "今天" || dayLabel === "昨天" || dayLabel === "前天" ? dayLabel : monthDayLabel(group.date);
  $: dayTotal = group.income + group.expense;

  function entryName(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return "转账";
    const category = entry.categoryId
      ? book.categories.find((item) => item.id === entry.categoryId)
      : undefined;
    return category?.name ?? "未分类";
  }

  function entryIcon(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return TRANSFER_ICON;
    const category = entry.categoryId
      ? book.categories.find((item) => item.id === entry.categoryId)
      : undefined;
    return category?.icon || (category ? "Package" : "Ellipsis");
  }

  function entryColor(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return "#7f8c8d";
    const category = entry.categoryId
      ? book.categories.find((item) => item.id === entry.categoryId)
      : undefined;
    if (category?.color) return category.color;
    const parent = category?.parentId
      ? book.categories.find((item) => item.id === category?.parentId)
      : undefined;
    if (parent?.color) return parent.color;
    return entry.kind === "income" ? "#2f9e6e" : "#f0862c";
  }

  function accountLabel(entry: LedgerEntry): string {
    const from = book.accounts.find((item) => item.id === entry.accountId);
    if (entry.kind !== "transfer") return from?.name ?? "";
    const to = book.accounts.find((item) => item.id === entry.toAccountId);
    return `${from?.name ?? "?"} → ${to?.name ?? "?"}`;
  }

  function accountIcon(entry: LedgerEntry): string {
    const account = book.accounts.find((item) => item.id === entry.accountId);
    return account?.icon || ACCOUNT_KIND_ICON[account?.kind ?? "other"];
  }

  function amountText(entry: LedgerEntry): string {
    const value = formatCents(entry.amountCents);
    if (entry.kind === "transfer") return value;
    return `${entry.kind === "income" ? "+" : "-"}${value}`;
  }

  function openEntry(event: MouseEvent, id: string): void {
    if (isLongPressSuppressed()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    dispatch("edit", id);
  }

  function openMenu(event: MouseEvent, id: string): void {
    event.preventDefault();
    event.stopPropagation();
    if (isLongPressSuppressed()) return;
    dispatch("context", { id, x: event.clientX, y: event.clientY });
  }

  /** 长按 = 右键（longpress action 给的是触点坐标，不必伪造 MouseEvent）。 */
  function longPressMenu(id: string): (pos: { x: number; y: number }) => void {
    return (pos) => dispatch("context", { id, x: pos.x, y: pos.y });
  }

  function addHere(): void {
    dispatch("add", group.date);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<article class="ledger-card" on:contextmenu|preventDefault|stopPropagation>
  <header class="ledger-card-head">
    {#if showDate}
      <span class="ledger-date-day" title={dayLabel}>{dayNumber}</span>
      <h3 class="ledger-card-title">{dayTitle}</h3>
      <span class="ledger-date-week">{weekday}</span>
    {/if}
    <span class="ledger-card-sums">
      {#if group.income > 0}
        <em class="in" title="当天收入">收 {compactCents(group.income)}</em>
      {/if}
      {#if group.expense > 0}
        <em class="out" title="当天支出">支 {compactCents(group.expense)}</em>
      {/if}
      {#if dayTotal === 0}
        <em class="flat">转账 {group.entries.length} 笔</em>
      {/if}
    </span>
    <button class="ledger-card-add" type="button" title="在这天记一笔" on:click|stopPropagation={addHere}>
      <Plus size={15} />
    </button>
  </header>

  <div class="ledger-entry-list">
    {#each group.entries as entry (entry.id)}
      {@const color = entryColor(entry)}
      {@const icon = ledgerIcon(entryIcon(entry), "Ellipsis")}
      {@const accountText = accountLabel(entry)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="ledger-entry"
        class:selected={selectedId === entry.id}
        use:longpress={longPressMenu(entry.id)}
        on:click={(event) => openEntry(event, entry.id)}
        on:contextmenu={(event) => openMenu(event, entry.id)}
      >
        <span class="ledger-entry-icon" style="--cat: {color}; background: {softColor(color)}">
          <svelte:component this={icon} size={16} />
        </span>
        <span class="ledger-entry-text">
          <strong>{entryName(entry)}</strong>
          {#if entry.note}<em>{entry.note}</em>{/if}
        </span>
        {#if accountText}
          <span class="ledger-entry-account" title={accountText}>
            {#if entry.kind === "transfer"}
              <svelte:component this={ledgerIcon(accountIcon(entry), "Wallet")} size={12} />
            {/if}
            {accountText}
          </span>
        {/if}
        <span class="ledger-entry-amount" class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>
          {amountText(entry)}
        </span>
      </div>
    {/each}
  </div>
</article>

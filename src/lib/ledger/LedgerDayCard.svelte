<script lang="ts">
  /**
   * 记账的一天 = 一张卡片。标题行 = 日期 + 周几 + 右端当天收/支与「在这天记一笔」；
   * 每一笔占两行：图标跨两行，大字行 = 分类名 + 带符号金额（左右对齐），
   * 小字行 = 备注（有插图时跟一个图片图标，多张带数量角标）+ 右端时刻与账户。
   * **没有备注时**分类名跟图标一样上下居中（金额与时刻/账户仍在右侧叠两行），
   * 否则左半边会空出一块，读起来像缺了一行。
   * 所有展示记账条目的地方（列表 / 日历选中日 / 钻取账单明细）都是这一套排版。
   */
  import { createEventDispatcher } from "svelte";
  import { Image as ImageIcon, Plus } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "../longpress";
  import { compactCents, formatCents } from "../ledger";
  import { ledgerIcon, softColor, TRANSFER_ICON } from "../ledgerIcons";
  import { accountTypeIcon } from "../ledgerAccountTypes";
  import { relativeDayLabel, weekdayOf } from "../diary";
  import { displayClock } from "../clock";
  import type { LedgerBook, LedgerEntry } from "../types";
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
    return account?.icon || accountTypeIcon(account?.kind ?? "other", book.accountTypes);
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
      {@const color = entryColor(entry)}
      {@const icon = ledgerIcon(entryIcon(entry), "Ellipsis")}
      {@const accountText = accountLabel(entry)}
      {@const clock = displayClock(entry.time)}
      {@const images = entry.images ?? []}
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
          <svelte:component this={icon} size={17} />
        </span>
        {#if entry.note}
          <span class="ledger-entry-main">
            <span class="ledger-entry-line">
              <strong>{entryName(entry)}</strong>
              <b class="ledger-entry-amount" class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>
                {amountText(entry)}
              </b>
            </span>
            <span class="ledger-entry-sub">
              <em class="ledger-entry-note">
                {entry.note}
                {#if images.length > 0}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <span
                    class="ledger-entry-image"
                    title="查看这条账的图片"
                    on:click|stopPropagation={() => dispatch("image", entry.id)}
                  >
                    <ImageIcon size={12} />
                    {#if images.length > 1}<i class="ledger-entry-image-count">{images.length}</i>{/if}
                  </span>
                {/if}
              </em>
              <span class="ledger-entry-meta">
                {#if clock}<em class="ledger-entry-time" title="记账时刻">{clock}</em>{/if}
                {#if accountText}
                  <em class="ledger-entry-account" title={accountText}>
                    {#if entry.kind === "transfer"}
                      <svelte:component this={ledgerIcon(accountIcon(entry), "Wallet")} size={11} />
                    {/if}
                    {accountText}
                  </em>
                {/if}
              </span>
            </span>
          </span>
        {:else}
          <!-- 无备注：分类名上下居中（跟图标一致），右侧金额与时刻/账户仍分两行 -->
          <span class="ledger-entry-main solo">
            <span class="ledger-entry-solo-name">
              <strong>{entryName(entry)}</strong>
              {#if images.length > 0}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <span
                  class="ledger-entry-image"
                  title="查看这条账的图片"
                  on:click|stopPropagation={() => dispatch("image", entry.id)}
                >
                  <ImageIcon size={12} />
                  {#if images.length > 1}<i class="ledger-entry-image-count">{images.length}</i>{/if}
                </span>
              {/if}
            </span>
            <span class="ledger-entry-solo-side">
              <b class="ledger-entry-amount" class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>
                {amountText(entry)}
              </b>
              <span class="ledger-entry-meta">
                {#if clock}<em class="ledger-entry-time" title="记账时刻">{clock}</em>{/if}
                {#if accountText}
                  <em class="ledger-entry-account" title={accountText}>
                    {#if entry.kind === "transfer"}
                      <svelte:component this={ledgerIcon(accountIcon(entry), "Wallet")} size={11} />
                    {/if}
                    {accountText}
                  </em>
                {/if}
              </span>
            </span>
          </span>
        {/if}
      </div>
    {/each}
  </div>
</article>

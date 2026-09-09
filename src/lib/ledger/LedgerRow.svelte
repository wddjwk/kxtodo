<script lang="ts">
  /**
   * 一笔账的行：分类图标圆 + 分类名/备注 + 带符号金额与账户名。
   * 列表/日历/统计三处共用；点按进编辑器，右键/长按出菜单（与任务卡片同一套手势）。
   */
  import { createEventDispatcher } from "svelte";
  import type { LedgerBook, LedgerEntry } from "../types";
  import { categoryColor, signedLabel } from "../ledger";
  import { ledgerIcon, TRANSFER_ICON } from "../ledgerIcons";
  import { longpress, isLongPressSuppressed } from "../longpress";

  export let entry: LedgerEntry;
  export let book: LedgerBook;

  const dispatch = createEventDispatcher<{
    edit: string;
    context: { id: string; x: number; y: number };
  }>();

  $: category = entry.categoryId
    ? book.categories.find((item) => item.id === entry.categoryId)
    : undefined;
  $: parent = category?.parentId
    ? book.categories.find((item) => item.id === category?.parentId)
    : undefined;
  $: title = entry.kind === "transfer"
    ? "转账"
    : category?.name ?? parent?.name ?? "未分类";
  $: subtitle = entry.kind === "transfer"
    ? entry.note || `${accountName(entry.accountId)} → ${accountName(entry.toAccountId ?? "")}`
    : entry.note;
  $: color = entry.kind === "transfer" ? "#7f8c8d" : categoryColor(book, category ?? parent);
  $: icon = entry.kind === "transfer"
    ? ledgerIcon(TRANSFER_ICON, TRANSFER_ICON)
    : ledgerIcon(category?.icon || parent?.icon, category?.side === "income" ? "Banknote" : "Package");

  function accountName(id: string): string {
    return book.accounts.find((item) => item.id === id)?.name ?? "已删除账户";
  }

  function handleLongPress(pos: { x: number; y: number }): void {
    dispatch("context", { id: entry.id, x: pos.x, y: pos.y });
  }

  function openMenu(event: MouseEvent): void {
    // 触摸长按后 Chromium 会补发一个原生 contextmenu，去重避免开两次菜单
    if (isLongPressSuppressed()) return;
    event.preventDefault();
    dispatch("context", { id: entry.id, x: event.clientX, y: event.clientY });
  }
</script>

<button
  type="button"
  class="ledger-row"
  on:click={() => dispatch("edit", entry.id)}
  on:contextmenu={openMenu}
  use:longpress={handleLongPress}
>
  <span class="ledger-row-icon" style="background:{color}">
    <svelte:component this={icon} size={17} />
  </span>
  <span class="ledger-row-main">
    <strong>{title}</strong>
    {#if subtitle}<em>{subtitle}</em>{/if}
  </span>
  <span class="ledger-row-side">
    <b class={entry.kind}>{signedLabel(entry)}</b>
    <i>{accountName(entry.accountId)}</i>
  </span>
</button>

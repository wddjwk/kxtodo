<script lang="ts">
  /**
   * 分类钻取的内容层（外壳见 CategoryDrilldown.svelte：移动端下半屏、桌面端锚定下拉）。
   * 两级：① 这个大类下的二级分类占比环 + 明细行；② 点一行看该二级分类的账单明细，
   * 再点某一笔交给上层打开记账面板改它。
   * 大类没有二级分类时跳过第一级，直接给账单明细。
   *
   * 二级分类的环**不按继承色画**：子分类默认继承大类颜色，照抄的话整个环就是一块色——
   * 自己没填颜色的按序号取调色板，行里的图标/进度条跟环同色。
   * 账单明细与列表视图同一套两行排版（图标跨两行 + 大字行 + 小字行）。
   */
  import { ChevronLeft, ChevronRight, Image as ImageIcon, X } from "@lucide/svelte";
  import { categoryStats, formatCents, paletteColor, sortEntries } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";
  import { ledgerIcon, softColor } from "../ledgerIcons";
  import { accountTypeIcon } from "../ledgerAccountTypes";
  import { displayClock } from "../clock";
  import { monthDayLabel } from "../diary";
  import LedgerDonut from "./LedgerDonut.svelte";
  import type { LedgerBook, LedgerCategory, LedgerEntry, LedgerSide } from "../types";

  export let book: LedgerBook;
  /** 已经按统计周期过滤过的流水 */
  export let rangeEntries: LedgerEntry[];
  export let category: LedgerCategory;
  export let side: LedgerSide;
  export let periodLabel: string;
  export let onClose: () => void = () => {};
  export let onEditEntry: (id: string) => void = () => {};
  export let onImageView: (id: string) => void = () => {};

  let openChild = "";

  $: stat = categoryStats(book, rangeEntries, side).find((item) => item.categoryId === category.id);
  $: children = stat?.children ?? [];
  $: totalCents = stat?.cents ?? 0;
  $: totalCount = stat?.count ?? 0;
  $: icon = ledgerIcon(category.icon, side === "income" ? "Banknote" : "Package");
  $: color = categoryColorOf(category);

  function categoryColorOf(item: LedgerCategory | undefined): string {
    if (!item) return "#95a5a6";
    if (item.color) return item.color;
    const parent = item.parentId ? book.categories.find((entry) => entry.id === item.parentId) : undefined;
    return parent?.color || (item.side === "income" ? "#27ae60" : "#f0862c");
  }

  /** 环与行共用的子分类配色：自己的颜色优先，没有就按序号取调色板 */
  function childColor(childId: string, index: number): string {
    const childCategory = book.categories.find((item) => item.id === childId);
    return childCategory?.color || paletteColor(index);
  }

  $: donutItems = children.map<LedgerDonutItem>((child, index) => ({
    id: child.categoryId,
    name: child.name,
    cents: child.cents,
    count: child.count,
    color: childColor(child.categoryId, index)
  }));

  /** 大类自己没有二级分类时，账都记在它身上，直接列出来 */
  $: directEntries = sortEntries(rangeEntries.filter((entry) => entry.categoryId === category.id));
  $: childEntries = openChild
    ? sortEntries(rangeEntries.filter((entry) => entry.categoryId === openChild))
    : [];
  $: openChildName = book.categories.find((item) => item.id === openChild)?.name ?? "";
  $: entries = openChild ? childEntries : directEntries;

  function childPercent(cents: number): number {
    return totalCents > 0 ? Math.round((cents / totalCents) * 10000) / 100 : 0;
  }

  function entryName(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return "转账";
    const item = entry.categoryId ? book.categories.find((cat) => cat.id === entry.categoryId) : undefined;
    return item?.name ?? "未分类";
  }

  function entryIconName(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return "ArrowLeftRight";
    const item = entry.categoryId ? book.categories.find((cat) => cat.id === entry.categoryId) : undefined;
    return item?.icon || (item ? "Package" : "Ellipsis");
  }

  function entryColor(entry: LedgerEntry): string {
    if (entry.kind === "transfer") return "#7f8c8d";
    const item = entry.categoryId ? book.categories.find((cat) => cat.id === entry.categoryId) : undefined;
    return categoryColorOf(item);
  }

  function accountLabel(entry: LedgerEntry): string {
    const from = book.accounts.find((item) => item.id === entry.accountId);
    if (entry.kind !== "transfer") return from?.name ?? "";
    const to = book.accounts.find((item) => item.id === entry.toAccountId);
    return `${from?.name ?? "?"} → ${to?.name ?? "?"}`;
  }

  function accountIcon(entry: LedgerEntry): string {
    const account = book.accounts.find((item) => item.id === entry.accountId);
    return account?.icon || accountTypeIcon(account?.kind ?? "other");
  }

  function amountText(entry: LedgerEntry): string {
    const value = formatCents(entry.amountCents);
    if (entry.kind === "transfer") return value;
    return `${entry.kind === "income" ? "+" : "-"}${value}`;
  }
</script>

<div class="ledger-drill">
  <header class="ledger-drill-head">
    {#if openChild}
      <button type="button" class="ledger-drill-back" title="返回二级分类" on:click={() => (openChild = "")}>
        <ChevronLeft size={17} />
      </button>
    {:else}
      <span class="ledger-drill-icon" style="--cat: {color}; background: {softColor(color)}">
        <svelte:component this={icon} size={17} />
      </span>
    {/if}
    <span class="ledger-drill-title">
      <strong>{openChild ? openChildName : category.name}</strong>
      <em>{periodLabel} · {totalCount} 笔 · {formatCents(totalCents)}</em>
    </span>
    <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
      <X size={18} />
    </button>
  </header>

  <div class="ledger-drill-body">
    {#if openChild || children.length === 0}
      <!-- 账单明细：点一笔就去改它 -->
      {#if entries.length === 0}
        <p class="ledger-drill-empty">{periodLabel}这一类还没有账。</p>
      {:else}
        <ul class="ledger-drill-entries">
          {#each entries as entry (entry.id)}
            {@const accountText = accountLabel(entry)}
            {@const clock = displayClock(entry.time)}
            {@const eColor = entryColor(entry)}
            <li>
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div class="ledger-entry" title="点开修改这一笔" on:click={() => onEditEntry(entry.id)}>
                <span class="ledger-entry-icon" style="--cat: {eColor}; background: {softColor(eColor)}">
                  <svelte:component this={ledgerIcon(entryIconName(entry), "Ellipsis")} size={17} />
                </span>
                <span class="ledger-entry-main">
                  <span class="ledger-entry-line">
                    <strong>{monthDayLabel(entry.date)} · {entryName(entry)}</strong>
                    <b
                      class="ledger-entry-amount"
                      class:in={entry.kind === "income"}
                      class:out={entry.kind === "expense"}
                    >{amountText(entry)}</b>
                  </span>
                  <span class="ledger-entry-sub">
                    <em class="ledger-entry-note">
                      {#if entry.note}{entry.note}{/if}
                      {#if entry.images && entry.images.length > 0}
                        <!-- svelte-ignore a11y_no_static_element_interactions -->
                        <!-- svelte-ignore a11y_click_events_have_key_events -->
                        <span
                          class="ledger-entry-image"
                          title="查看这条账的图片"
                          on:click|stopPropagation={() => onImageView(entry.id)}
                        >
                          <ImageIcon size={12} />
                          {#if entry.images.length > 1}<i class="ledger-entry-image-count">{entry.images.length}</i>{/if}
                        </span>
                      {/if}
                    </em>
                    <span class="ledger-entry-meta">
                      {#if clock}<em class="ledger-entry-time">{clock}</em>{/if}
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
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    {:else}
      {#if children.length > 1}
        <div class="ledger-drill-donut">
          <LedgerDonut items={donutItems} total={totalCents} totalLabel={category.name} />
        </div>
      {/if}
      <ul class="ledger-drill-rows">
        {#each children as child, index (child.categoryId)}
          {@const childCategory = book.categories.find((item) => item.id === child.categoryId)}
          {@const childColor = childCategory?.color || paletteColor(index)}
          <li>
            <button type="button" class="ledger-drill-row" title="看这一类的账单明细" on:click={() => (openChild = child.categoryId)}>
              <span class="ledger-drill-row-icon" style="--cat: {childColor}; background: {softColor(childColor)}">
                <svelte:component this={ledgerIcon(childCategory?.icon, "Package")} size={15} />
              </span>
              <span class="ledger-drill-row-text">
                <strong>{child.name}</strong>
                <em>{child.count} 笔 · {childPercent(child.cents)}%</em>
                <span class="ledger-rank-track"><i style="width: {childPercent(child.cents)}%; background: {childColor}"></i></span>
              </span>
              <b class="ledger-drill-row-amount">{formatCents(child.cents)}</b>
              <ChevronRight class="ledger-rank-go" size={16} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

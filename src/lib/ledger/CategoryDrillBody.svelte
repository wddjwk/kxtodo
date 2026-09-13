<script lang="ts">
  /**
   * 分类钻取的内容层（外壳见 CategoryDrilldown.svelte：移动端下半屏、桌面端锚定下拉）。
   * 两级：① 这个大类下的二级分类占比环 + 明细行；② 点一行看该二级分类的账单明细，
   * 再点某一笔交给上层打开记账面板改它。
   * 大类没有二级分类时跳过第一级，直接给账单明细。
   */
  import { ChevronLeft, ChevronRight, X } from "@lucide/svelte";
  import { categoryColor, categoryStats, formatCents, sortEntries } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";
  import { ledgerIcon, softColor, ACCOUNT_KIND_ICON } from "../ledgerIcons";
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

  let openChild = "";

  $: stat = categoryStats(book, rangeEntries, side).find((item) => item.categoryId === category.id);
  $: children = stat?.children ?? [];
  $: totalCents = stat?.cents ?? 0;
  $: totalCount = stat?.count ?? 0;
  $: icon = ledgerIcon(category.icon, side === "income" ? "Banknote" : "Package");
  $: color = categoryColor(book, category);

  $: donutItems = children.map<LedgerDonutItem>((child) => ({
    id: child.categoryId,
    name: child.name,
    cents: child.cents,
    count: child.count,
    color: categoryColor(book, book.categories.find((item) => item.id === child.categoryId))
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
            <li>
              <button type="button" class="ledger-drill-entry" title="点开修改这一笔" on:click={() => onEditEntry(entry.id)}>
                <span class="ledger-drill-date">
                  <strong>{monthDayLabel(entry.date)}</strong>
                  {#if clock}<em>{clock}</em>{/if}
                </span>
                <span class="ledger-drill-note">
                  {#if entry.note}{entry.note}{:else}<i>{accountText || "—"}</i>{/if}
                </span>
                {#if accountText}
                  <span class="ledger-drill-account">
                    <svelte:component this={ledgerIcon(accountIcon(entry), "Wallet")} size={12} />{accountText}
                  </span>
                {/if}
                <b class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>{amountText(entry)}</b>
              </button>
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
        {#each children as child (child.categoryId)}
          {@const childCategory = book.categories.find((item) => item.id === child.categoryId)}
          {@const childColor = categoryColor(book, childCategory)}
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

<script lang="ts">
  /**
   * 一笔账的一行（列表卡片 / 日历选中日 / 钻取账单明细 / 搜索结果卡片共用同一份）。
   * 图标跨两行居左，大字行 = 分类名 + 带符号金额（左右对齐），小字行 = 备注（有插图时
   * 跟一个图片图标）+ 右端时刻与账户。
   * **图片图标永远在小字行（备注区）**：没有备注时它就是这一行的"备注"，
   * 不跑到分类名旁边去（那样同一个图标在不同条目上位置飘）。
   * 没有备注、也没有图片时才走 solo 布局：分类名跟图标一样上下居中。
   * 账户一律带图标：普通一笔是「图标 + 账户名」，转账是「[转出图标]A → [转入图标]B」。
   * 数量角标只画在记账面板的图片按钮上——列表里露出了会被行高裁掉一半。
   */
  import { createEventDispatcher } from "svelte";
  import { ArrowLeftRight, Image as ImageIcon } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "../longpress";
  import { formatCents, ledgerLookup } from "../ledger";
  import { ledgerIcon, softColor, TRANSFER_ICON } from "../ledgerIcons";
  import { accountTypeIcon } from "../ledgerAccountTypes";
  import { displayClock } from "../clock";
  import type { LedgerBook, LedgerCategory, LedgerEntry } from "../types";

  export let book: LedgerBook;
  export let entry: LedgerEntry;
  export let selected = false;
  /** 钻取面板的明细行在分类名前带上日期（同一个二级分类里的多笔分不出是哪天） */
  export let datePrefix = "";

  const dispatch = createEventDispatcher<{
    edit: string;
    image: string;
    context: { id: string; x: number; y: number };
  }>();

  // 一律走索引查表：早先这一行要做 6 次 `Array.find`（分类查了 3 次同一个 id：名字、图标、
  // 颜色各一次，颜色还要再查父类；账户 2 次）。列表 / 日历选中日 / 钻取明细 / 搜索结果
  // 四处共用这一行，一屏几百行 × 每行 6 次 × 分类表长度，每次记账写入还要重来一遍。
  // 索引按 book 对象身份缓存（ledger.ts::ledgerLookup），所以四处调用点都不用传 prop。
  $: lookup = ledgerLookup(book);
  $: category =
    entry.kind === "transfer" || !entry.categoryId
      ? undefined
      : lookup.categoryById.get(entry.categoryId);
  $: parent = category?.parentId ? lookup.categoryById.get(category.parentId) : undefined;

  $: images = entry.images ?? [];
  $: hasNote = Boolean(entry.note);
  /** 分类名那一格里显示的文字（可带日期前缀） */
  $: title = `${datePrefix}${entryName(entry, category)}`;
  $: color = entryColor(entry, category, parent);
  $: icon = ledgerIcon(entryIcon(entry, category), "Ellipsis");
  $: clock = displayClock(entry.time);
  $: amount = amountText(entry);
  $: showSub = hasNote || images.length > 0;
  $: fromAccount = lookup.accountById.get(entry.accountId);
  $: toAccount = entry.toAccountId ? lookup.accountById.get(entry.toAccountId) : undefined;
  $: fromName = fromAccount?.name ?? "";
  $: toName = toAccount?.name ?? "";
  $: accountText = entry.kind === "transfer" ? `${fromName} → ${toName}` : fromName;

  // 分类/父类都从参数进来，不在函数体里读响应式变量：**Svelte 不跟踪函数调用里的依赖**，
  // 少传一个参数就等于这条 `$:` 永远不会因为它变化而重算。
  function entryName(item: LedgerEntry, cat: LedgerCategory | undefined): string {
    if (item.kind === "transfer") return "转账";
    return cat?.name ?? "未分类";
  }

  function entryIcon(item: LedgerEntry, cat: LedgerCategory | undefined): string {
    if (item.kind === "transfer") return TRANSFER_ICON;
    return cat?.icon || (cat ? "Package" : "Ellipsis");
  }

  function entryColor(
    item: LedgerEntry,
    cat: LedgerCategory | undefined,
    dad: LedgerCategory | undefined
  ): string {
    if (item.kind === "transfer") return "#7f8c8d";
    if (cat?.color) return cat.color;
    if (dad?.color) return dad.color;
    return item.kind === "income" ? "#2f9e6e" : "#f0862c";
  }

  function accountIconOf(account: { icon?: string; kind?: string } | undefined): string {
    return account?.icon || accountTypeIcon(account?.kind ?? "other", book.accountTypes);
  }

  function amountText(item: LedgerEntry): string {
    const value = formatCents(item.amountCents);
    if (item.kind === "transfer") return value;
    return `${item.kind === "income" ? "+" : "-"}${value}`;
  }

  function openEntry(event: MouseEvent): void {
    if (isLongPressSuppressed()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    dispatch("edit", entry.id);
  }

  function openMenu(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    if (isLongPressSuppressed()) return;
    dispatch("context", { id: entry.id, x: event.clientX, y: event.clientY });
  }

  /** 长按 = 右键（longpress action 给的是触点坐标，不必伪造 MouseEvent）。 */
  function longPressMenu(pos: { x: number; y: number }): void {
    dispatch("context", { id: entry.id, x: pos.x, y: pos.y });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="ledger-entry"
  class:selected
  use:longpress={longPressMenu}
  on:click={openEntry}
  on:contextmenu={openMenu}
>
  <span class="ledger-entry-icon" style="--cat: {color}; background: {softColor(color)}">
    <svelte:component this={icon} size={17} />
  </span>
  {#if showSub}
    <span class="ledger-entry-main">
      <span class="ledger-entry-line">
        <strong>{title}</strong>
        <b class="ledger-entry-amount" class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>
          {amount}
        </b>
      </span>
      <span class="ledger-entry-sub">
        <em class="ledger-entry-note">
          {#if hasNote}{entry.note}{/if}
          {#if images.length > 0}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <span
              class="ledger-entry-image"
              title={images.length > 1 ? `查看这条账的 ${images.length} 张图片` : "查看这条账的图片"}
              on:click|stopPropagation={() => dispatch("image", entry.id)}
            >
              <ImageIcon size={14} />
            </span>
          {/if}
        </em>
        <span class="ledger-entry-meta">
          {#if clock}<em class="ledger-entry-time" title="记账时刻">{clock}</em>{/if}
          {#if entry.kind === "transfer"}
            <em class="ledger-entry-account transfer" title={accountText}>
              <svelte:component this={ledgerIcon(accountIconOf(fromAccount), "Wallet")} size={13} />
              {fromName}
              <ArrowLeftRight class="ledger-account-arrow" size={11} />
              <svelte:component this={ledgerIcon(accountIconOf(toAccount), "Wallet")} size={13} />
              {toName}
            </em>
          {:else if fromName}
            <em class="ledger-entry-account" title={accountText}>
              <svelte:component this={ledgerIcon(accountIconOf(fromAccount), "Wallet")} size={13} />
              {fromName}
            </em>
          {/if}
        </span>
      </span>
    </span>
  {:else}
    <!-- 无备注也无图：分类名上下居中（跟图标一致），右侧金额与时刻/账户仍分两行。
         两个包裹层 display:contents（见 ledger.css），四块直接落进两行两列的网格。 -->
    <span class="ledger-entry-main solo">
      <span class="ledger-entry-solo-name">
        <strong>{title}</strong>
      </span>
      <span class="ledger-entry-solo-side">
        <b class="ledger-entry-amount" class:in={entry.kind === "income"} class:out={entry.kind === "expense"}>
          {amount}
        </b>
        <span class="ledger-entry-meta">
          {#if clock}<em class="ledger-entry-time" title="记账时刻">{clock}</em>{/if}
          {#if entry.kind === "transfer"}
            <em class="ledger-entry-account transfer" title={accountText}>
              <svelte:component this={ledgerIcon(accountIconOf(fromAccount), "Wallet")} size={13} />
              {fromName}
              <ArrowLeftRight class="ledger-account-arrow" size={11} />
              <svelte:component this={ledgerIcon(accountIconOf(toAccount), "Wallet")} size={13} />
              {toName}
            </em>
          {:else if fromName}
            <em class="ledger-entry-account" title={accountText}>
              <svelte:component this={ledgerIcon(accountIconOf(fromAccount), "Wallet")} size={13} />
              {fromName}
            </em>
          {/if}
        </span>
      </span>
    </span>
  {/if}
</div>

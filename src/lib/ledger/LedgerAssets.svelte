<script lang="ts">
  /**
   * 资产视图：净资产汇总卡 + 总资产趋势 + 账户列表（点行去改账户）+ 转账/添加入口。
   * 余额一律现场推导（期初 + 流水），不存现值——改历史账目不用回头修余额。
   * 趋势块桌面并排在净资产右侧、移动端纵排夹在净资产与资金账户之间；
   * 点它放大（桌面浮窗悬浮读数、移动端横屏全屏）。
   */
  import { createEventDispatcher } from "svelte";
  import { ArrowLeftRight, Maximize2, Plus } from "@lucide/svelte";
  import { assetsOverview, assetsTrend, formatCents } from "../ledger";
  import {
    ledgerIcon, softColor
  } from "../ledgerIcons";
  import { accountTypeColor, accountTypeIcon, accountTypeLabel } from "../ledgerAccountTypes";
  import AssetsTrend from "./AssetsTrend.svelte";
  import type { LedgerBook } from "../types";

  export let book: LedgerBook;

  const dispatch = createEventDispatcher<{
    editAccount: string;
    addAccount: void;
    transfer: void;
    openTrend: void;
  }>();

  $: assets = assetsOverview(book);
  $: trend = assetsTrend(book);
</script>

<div class="ledger-assets-top">
  <section class="ledger-net-card">
    <span class="ledger-net-label">净资产</span>
    <strong class="ledger-net-value">{formatCents(assets.net)}</strong>
    <div class="ledger-net-split">
      <div>
        <span>总资产</span>
        <b class="in">{formatCents(assets.assets)}</b>
      </div>
      <div>
        <span>总负债</span>
        <b class="out">{formatCents(assets.liabilities)}</b>
      </div>
      <div>
        <span>账户</span>
        <b>{book.accounts.length} 个</b>
      </div>
    </div>
  </section>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <section
    class="ledger-panel ledger-trend-card"
    role="button"
    tabindex="0"
    title="点击放大查看总资产趋势"
    on:click|stopPropagation={() => dispatch("openTrend")}
    on:keydown|stopPropagation={(event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        dispatch("openTrend");
      }
    }}
  >
    <header class="ledger-panel-head">
      <h2>总资产趋势</h2>
      <span class="ledger-panel-actions">
        <Maximize2 class="ledger-trend-zoom" size={15} />
      </span>
    </header>
    <AssetsTrend points={trend} />
  </section>
</div>

<section class="ledger-panel">
  <header class="ledger-panel-head">
    <h2>资金账户</h2>
    <span class="ledger-panel-actions">
      <button type="button" class="ledger-chip-button" on:click|stopPropagation={() => dispatch("transfer")}>
        <ArrowLeftRight size={14} />转账
      </button>
      <button type="button" class="ledger-chip-button" on:click|stopPropagation={() => dispatch("addAccount")}>
        <Plus size={14} />添加
      </button>
    </span>
  </header>

  <div class="ledger-account-list">
    {#each assets.perAccount as item (item.account.id)}
      {@const iconName = item.account.icon || accountTypeIcon(item.account.kind, book.accountTypes)}
      {@const icon = ledgerIcon(iconName, iconName)}
      {@const color = item.account.color || accountTypeColor(item.account.kind, book.accountTypes)}
      <button
        type="button"
        class="ledger-account-row"
        title="编辑这个账户"
        on:click|stopPropagation={() => dispatch("editAccount", item.account.id)}
      >
        <span class="ledger-account-icon" style="--cat: {color}; background: {softColor(color)}">
          <svelte:component this={icon} size={17} />
        </span>
        <span class="ledger-account-text">
          <strong>{item.account.name}</strong>
          <em>{accountTypeLabel(item.account.kind)}{item.account.note ? ` · ${item.account.note}` : ""}</em>
        </span>
        <b class="ledger-account-balance" class:negative={item.balance < 0}>{formatCents(item.balance)}</b>
      </button>
    {:else}
      <div class="ledger-day-empty">还没有账户，点右上角「添加」建一个。</div>
    {/each}
  </div>
</section>

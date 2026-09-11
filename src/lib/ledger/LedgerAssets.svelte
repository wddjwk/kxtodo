<script lang="ts">
  /**
   * 资产视图：净资产汇总卡 + 账户列表（点行去改账户）+ 转账/添加入口。
   * 余额一律现场推导（期初 + 流水），不存现值——改历史账目不用回头修余额。
   */
  import { createEventDispatcher } from "svelte";
  import { ArrowLeftRight, Plus } from "@lucide/svelte";
  import { assetsOverview, formatCents } from "../ledger";
  import {
    ACCOUNT_KIND_COLOR, ACCOUNT_KIND_LABEL, accountIconName, ledgerIcon, softColor
  } from "../ledgerIcons";
  import type { LedgerBook } from "../types";

  export let book: LedgerBook;

  const dispatch = createEventDispatcher<{
    editAccount: string;
    addAccount: void;
    transfer: void;
  }>();

  $: assets = assetsOverview(book);
</script>

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
      {@const iconName = accountIconName(item.account.icon, item.account.kind)}
      {@const icon = ledgerIcon(iconName, iconName)}
      {@const color = item.account.color || ACCOUNT_KIND_COLOR[item.account.kind]}
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
          <em>{ACCOUNT_KIND_LABEL[item.account.kind]}{item.account.note ? ` · ${item.account.note}` : ""}</em>
        </span>
        <b class="ledger-account-balance" class:negative={item.balance < 0}>{formatCents(item.balance)}</b>
      </button>
    {:else}
      <div class="ledger-day-empty">还没有账户，点右上角「添加」建一个。</div>
    {/each}
  </div>
</section>

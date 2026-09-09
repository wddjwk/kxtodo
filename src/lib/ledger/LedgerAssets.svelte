<script lang="ts">
  /**
   * 资产视图：净资产卡（总资产/总负债）+ 账户列表（余额 = 期初 + 流水推导）。
   * 点账户行进编辑；「添加」「转账」两个动作按钮在卡片头部。
   */
  import { createEventDispatcher } from "svelte";
  import { ArrowLeftRight, Plus } from "@lucide/svelte";
  import type { LedgerBook } from "../types";
  import { assetsOverview, formatCents } from "../ledger";
  import { accountIconName, ACCOUNT_KIND_LABEL, ACCOUNT_KIND_COLOR, ledgerIcon } from "../ledgerIcons";

  export let book: LedgerBook;

  const dispatch = createEventDispatcher<{
    editAccount: string;
    addAccount: void;
    transfer: void;
  }>();

  $: overview = assetsOverview(book);
</script>

<div class="ledger-assets">
  <section class="ledger-net-card">
    <span class="ledger-net-label">净资产</span>
    <strong class="ledger-net-value">{formatCents(overview.net)}</strong>
    <span class="ledger-net-split">
      <i>总资产 {formatCents(overview.assets)}</i>
      <i>总负债 {formatCents(overview.liabilities)}</i>
    </span>
  </section>

  <section class="ledger-chart-card">
    <header>
      <strong>资金账户</strong>
      <span class="ledger-asset-actions">
        <button type="button" class="menu-action-button" on:click={() => dispatch("transfer")}>
          <ArrowLeftRight size={14} /> 转账
        </button>
        <button type="button" class="menu-action-button" on:click={() => dispatch("addAccount")}>
          <Plus size={14} /> 添加
        </button>
      </span>
    </header>
    {#each overview.perAccount as item (item.account.id)}
      {@const iconName = accountIconName(item.account.icon, item.account.kind)}
      {@const icon = ledgerIcon(iconName, iconName)}
      {@const color = item.account.color || ACCOUNT_KIND_COLOR[item.account.kind]}
      <button
        type="button"
        class="ledger-account-row"
        on:click={() => dispatch("editAccount", item.account.id)}
      >
        <span class="ledger-row-icon" style="background:{color}">
          <svelte:component this={icon} size={16} />
        </span>
        <span class="ledger-account-name">
          <strong>{item.account.name}</strong>
          <em>{ACCOUNT_KIND_LABEL[item.account.kind]}</em>
        </span>
        <b class:income={item.balance > 0} class:expense={item.balance < 0}>
          {formatCents(item.balance)}
        </b>
      </button>
    {:else}
      <p class="ledger-day-empty">还没有账户，点「添加」建一个</p>
    {/each}
  </section>
</div>

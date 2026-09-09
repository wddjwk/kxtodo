<script lang="ts">
  /**
   * 账户管理：账户列表（带余额）+ 增改删表单 + 转账面板。
   * 名下还有账的账户 core 会拒绝删除（错误走 toast），这里不重复拦。
   */
  import { createEventDispatcher } from "svelte";
  import { ArrowLeftRight, Pencil, Plus, Trash2, X } from "@lucide/svelte";
  import type { LedgerAccountKind, LedgerBook } from "../types";
  import { assetsOverview, formatCents, parseYuanToCents } from "../ledger";
  import {
    ACCOUNT_KIND_COLOR,
    ACCOUNT_KIND_LABEL,
    LEDGER_ICON_CHOICES,
    accountIconName,
    ledgerIcon
  } from "../ledgerIcons";
  import { addLedgerAccount, updateLedgerAccount, deleteLedgerAccount, transferLedger } from "../actions";
  import { todayIso } from "../stores";

  export let book: LedgerBook;
  export let onClose: () => void = () => {};
  /** 打开就直接进转账面板（资产视图的「转账」按钮） */
  export let startWithTransfer = false;
  /** 打开就直接编辑这个账户（资产视图点账户行） */
  export let editId = "";

  const dispatch = createEventDispatcher<{ close: void }>();

  const COLORS = [
    "#e8a33d",
    "#b23a48",
    "#c0392b",
    "#2980b9",
    "#2f8f6b",
    "#9b59b6",
    "#7f8c8d",
    "#1677ff"
  ];
  const KINDS: LedgerAccountKind[] = ["cash", "debit", "credit", "investment", "other"];

  let panel: "list" | "form" | "transfer" = startWithTransfer ? "transfer" : "list";
  let form: {
    id: string | null;
    name: string;
    kind: LedgerAccountKind;
    icon: string;
    color: string;
    initial: string;
    note: string;
  } | null = null;
  let transfer = { from: book.accounts[0]?.id ?? "", to: book.accounts[1]?.id ?? "", amount: "", note: "" };

  $: overview = assetsOverview(book);

  if (editId) openEdit(editId);

  function openAdd(): void {
    form = { id: null, name: "", kind: "cash", icon: "", color: "", initial: "", note: "" };
    panel = "form";
  }

  function openEdit(id: string): void {
    const account = book.accounts.find((item) => item.id === id);
    if (!account) return;
    form = {
      id: account.id,
      name: account.name,
      kind: account.kind,
      icon: account.icon,
      color: account.color,
      initial: (account.initialCents / 100).toString(),
      note: account.note
    };
    panel = "form";
  }

  async function submit(): Promise<void> {
    if (!form || form.name.trim() === "") return;
    const initialCents = form.initial === "" ? 0 : parseYuanToCents(form.initial) ?? 0;
    const payload = {
      name: form.name.trim(),
      kind: form.kind,
      icon: form.icon,
      color: form.color,
      initialCents,
      note: form.note.trim()
    };
    if (form.id) {
      await updateLedgerAccount(form.id, payload);
    } else {
      await addLedgerAccount(payload);
    }
    form = null;
    panel = "list";
  }

  async function remove(id: string): Promise<void> {
    await deleteLedgerAccount(id);
    if (form?.id === id) {
      form = null;
      panel = "list";
    }
  }

  async function submitTransfer(): Promise<void> {
    const cents = parseYuanToCents(transfer.amount);
    if (!cents || cents <= 0 || transfer.from === transfer.to) return;
    const ok = await transferLedger({
      from: transfer.from,
      to: transfer.to,
      amountCents: cents,
      date: todayIso(),
      note: transfer.note.trim()
    });
    if (ok) {
      transfer.amount = "";
      transfer.note = "";
      panel = "list";
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (panel !== "list") panel = "list";
      else onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="ledger-editor-backdrop" on:click={() => onClose()}></div>
<div class="ledger-manager" role="dialog" aria-label="账户管理">
  <header class="ledger-editor-head">
    <strong class="ledger-manager-title">
      {panel === "transfer" ? "转账" : panel === "form" ? "账户" : "资金账户"}
    </strong>
    <button type="button" class="ledger-editor-close" on:click={() => onClose()}>
      <X size={18} />
    </button>
  </header>

  {#if panel === "list"}
    <div class="ledger-manager-list">
      {#each overview.perAccount as item (item.account.id)}
        {@const iconName = accountIconName(item.account.icon, item.account.kind)}
        {@const icon = ledgerIcon(iconName, iconName)}
        <div class="ledger-manager-row">
          <span
            class="ledger-row-icon"
            style="background:{item.account.color || ACCOUNT_KIND_COLOR[item.account.kind]}"
          >
            <svelte:component this={icon} size={16} />
          </span>
          <strong>{item.account.name}</strong>
          <em>{ACCOUNT_KIND_LABEL[item.account.kind]}</em>
          <b>{formatCents(item.balance)}</b>
          <span class="ledger-manager-row-actions">
            <button type="button" on:click={() => openEdit(item.account.id)} title="编辑">
              <Pencil size={14} />
            </button>
            <button type="button" class="danger" on:click={() => remove(item.account.id)} title="删除">
              <Trash2 size={14} />
            </button>
          </span>
        </div>
      {:else}
        <p class="ledger-day-empty">还没有账户</p>
      {/each}
    </div>
    <footer class="ledger-editor-foot">
      <button type="button" class="settings-button" on:click={() => (panel = "transfer")}>
        <ArrowLeftRight size={15} /> 转账
      </button>
      <button type="button" class="settings-button primary" on:click={openAdd}>
        <Plus size={15} /> 添加账户
      </button>
    </footer>
  {:else if panel === "form" && form}
    {@const f = form}
    <div class="ledger-manager-form">
      <input type="text" maxlength="20" placeholder="账户名称" bind:value={f.name} />
      <label class="ledger-manager-field">
        <span>类型</span>
        <select bind:value={f.kind}>
          {#each KINDS as value (value)}
            <option value={value}>{ACCOUNT_KIND_LABEL[value]}</option>
          {/each}
        </select>
      </label>
      <label class="ledger-manager-field">
        <span>期初余额（元）</span>
        <input type="text" inputmode="decimal" placeholder="0.00" bind:value={f.initial} />
      </label>
      <label class="ledger-manager-field">
        <span>备注</span>
        <input type="text" maxlength="40" bind:value={f.note} />
      </label>
      <div class="ledger-manager-swatches">
        {#each COLORS as color (color)}
          <button
            type="button"
            class="ledger-swatch"
            class:active={f.color === color}
            style="background:{color}"
            on:click={() => (f.color = f.color === color ? "" : color)}
            aria-label="颜色 {color}"
          ></button>
        {/each}
      </div>
      <div class="ledger-manager-icons">
        {#each LEDGER_ICON_CHOICES as name (name)}
          {@const icon = ledgerIcon(name, name)}
          <button
            type="button"
            class:active={f.icon === name}
            on:click={() => (f.icon = f.icon === name ? "" : name)}
            title={name}
          >
            <svelte:component this={icon} size={16} />
          </button>
        {/each}
      </div>
      <footer class="ledger-editor-foot">
        <button type="button" class="settings-button" on:click={() => (panel = "list")}>取消</button>
        <button type="button" class="settings-button primary" on:click={submit}>保存</button>
      </footer>
    </div>
  {:else}
    <div class="ledger-manager-form">
      <label class="ledger-manager-field">
        <span>转出</span>
        <select bind:value={transfer.from}>
          {#each book.accounts as account (account.id)}
            <option value={account.id}>{account.name}</option>
          {/each}
        </select>
      </label>
      <label class="ledger-manager-field">
        <span>转入</span>
        <select bind:value={transfer.to}>
          {#each book.accounts as account (account.id)}
            <option value={account.id}>{account.name}</option>
          {/each}
        </select>
      </label>
      <label class="ledger-manager-field">
        <span>金额（元）</span>
        <input type="text" inputmode="decimal" placeholder="0.00" bind:value={transfer.amount} />
      </label>
      <label class="ledger-manager-field">
        <span>备注</span>
        <input type="text" maxlength="40" bind:value={transfer.note} />
      </label>
      <footer class="ledger-editor-foot">
        <button type="button" class="settings-button" on:click={() => (panel = "list")}>取消</button>
        <button type="button" class="settings-button primary" on:click={submitTransfer}>转账</button>
      </footer>
    </div>
  {/if}
</div>

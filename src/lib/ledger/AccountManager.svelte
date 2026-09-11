<script lang="ts">
  /**
   * 账户管理：账户列表（带推导余额）+ 增改删表单 + 转账面板。
   * 三个子层共用记账面板那一套浮层语言（桌面居中对话框 / 移动端底部抽屉），
   * 切换子层时整个 body 让位——不做弹窗套弹窗，也不让对话框里出现滚动条。
   * 名下还有账的账户 core 会拒绝删除（错误走 toast），这里不重复拦。
   *
   * 表单字段一律是平铺的 let 变量（同 CategoryManager 的理由：bind 到 `{@const}`
   * 别名指向的对象属性不会让 Svelte 失效，保存按钮会一直停在 disabled）。
   */
  import { ArrowLeft, ArrowLeftRight, PenLine, Plus, Trash2, X } from "@lucide/svelte";
  import { appSettings, showToast, todayIso } from "../stores";
  import { imeInset } from "../imeInset";
  import { fieldKeydown } from "../shortcuts";
  import { ledgerAccent } from "../styles";
  import { assetsOverview, formatCents, parseYuanToCents } from "../ledger";
  import {
    ACCOUNT_KIND_COLOR, ACCOUNT_KIND_ICON, ACCOUNT_KIND_LABEL,
    LEDGER_ICON_CHOICES, accountIconName, ledgerIcon, softColor
  } from "../ledgerIcons";
  import {
    addLedgerAccount, deleteLedgerAccount, transferLedger, updateLedgerAccount
  } from "../actions";
  import type { LedgerAccountKind, LedgerBook } from "../types";

  export let book: LedgerBook;
  export let onClose: () => void = () => {};
  /** 打开就直接进转账面板（资产视图的「转账」按钮） */
  export let startWithTransfer = false;
  /** 打开就直接进新建表单（资产视图的「添加」按钮） */
  export let startWithAdd = false;
  /** 打开就直接编辑这个账户（资产视图点账户行） */
  export let editId = "";

  const COLORS = [
    "#e8a33d", "#b23a48", "#c0392b", "#2980b9",
    "#2f8f6b", "#9b59b6", "#7f8c8d", "#1677ff",
    "#f0862c", "#34495e", "#7cb342", "#d94f70"
  ];
  const KINDS: LedgerAccountKind[] = ["cash", "debit", "credit", "investment", "other"];

  let panel: "list" | "form" | "transfer" = startWithTransfer ? "transfer" : "list";
  /** null = 不在表单里；空串 = 新建；否则是要改的账户 id */
  let editingId: string | null = null;
  let nameDraft = "";
  let kindDraft: LedgerAccountKind = "debit";
  let iconDraft = "";
  let colorDraft = "";
  let initialDraft = "";
  let noteDraft = "";
  let busy = false;
  let nameInput: HTMLInputElement;

  let fromId = book.accounts[0]?.id ?? "";
  let toId = book.accounts.find((item) => item.id !== fromId)?.id ?? "";
  let transferText = "";
  let transferNote = "";
  let transferDate = todayIso();

  $: assets = assetsOverview(book);
  $: accent = ledgerAccent($appSettings.ledger);
  $: transferCents = parseYuanToCents(transferText) ?? 0;
  /** 表单预览：没手选颜色/图标就退回该类型的默认值 */
  $: previewColor = colorDraft || ACCOUNT_KIND_COLOR[kindDraft];
  $: previewIcon = iconDraft || ACCOUNT_KIND_ICON[kindDraft];

  if (editId) {
    const target = book.accounts.find((item) => item.id === editId);
    if (target) beginEdit(target.id);
  } else if (startWithAdd) {
    beginAdd();
  }

  function focusName(): void {
    void Promise.resolve().then(() => nameInput?.focus());
  }

  function beginAdd(): void {
    editingId = "";
    nameDraft = "";
    kindDraft = "debit";
    iconDraft = "";
    colorDraft = "";
    initialDraft = "";
    noteDraft = "";
    panel = "form";
    focusName();
  }

  function beginEdit(id: string): void {
    const target = book.accounts.find((item) => item.id === id);
    if (!target) return;
    editingId = target.id;
    nameDraft = target.name;
    kindDraft = target.kind;
    iconDraft = target.icon;
    colorDraft = target.color;
    initialDraft = target.initialCents ? (target.initialCents / 100).toString() : "";
    noteDraft = target.note;
    panel = "form";
    focusName();
  }

  function beginTransfer(): void {
    fromId = book.accounts[0]?.id ?? "";
    toId = book.accounts.find((item) => item.id !== fromId)?.id ?? "";
    transferText = "";
    transferNote = "";
    transferDate = todayIso();
    panel = "transfer";
  }

  function backToList(): void {
    editingId = null;
    panel = "list";
  }

  /** 选账户类型时顺手把图标换回该类型的默认（用户已手选过就不动） */
  function pickKind(kind: LedgerAccountKind): void {
    const wasDefault = !iconDraft || iconDraft === ACCOUNT_KIND_ICON[kindDraft];
    kindDraft = kind;
    if (wasDefault) iconDraft = "";
  }

  async function saveForm(): Promise<void> {
    if (busy) return;
    const name = nameDraft.trim();
    if (!name) {
      showToast("账户名称不能为空");
      return;
    }
    const trimmed = initialDraft.trim();
    const initialCents = trimmed === "" ? 0 : parseYuanToCents(trimmed);
    if (initialCents === null) {
      showToast("期初余额要是一个金额（可以是 0）");
      return;
    }
    busy = true;
    const draft = {
      name,
      kind: kindDraft,
      icon: iconDraft,
      color: colorDraft,
      initialCents,
      note: noteDraft.trim()
    };
    const ok = editingId ? await updateLedgerAccount(editingId, draft) : await addLedgerAccount(draft);
    busy = false;
    if (!ok) return;
    backToList();
  }

  async function removeForm(): Promise<void> {
    if (!editingId || busy) return;
    busy = true;
    const ok = await deleteLedgerAccount(editingId);
    busy = false;
    if (!ok) return;
    backToList();
  }

  async function saveTransfer(): Promise<void> {
    if (busy) return;
    if (transferCents <= 0) {
      showToast("转账金额要大于 0");
      return;
    }
    if (!fromId || !toId) {
      showToast("请先选择转出与转入账户");
      return;
    }
    if (fromId === toId) {
      showToast("转出与转入不能是同一个账户");
      return;
    }
    busy = true;
    const ok = await transferLedger({
      from: fromId,
      to: toId,
      amountCents: transferCents,
      date: transferDate,
      note: transferNote.trim()
    });
    busy = false;
    if (!ok) return;
    backToList();
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target === event.currentTarget) onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      event.stopPropagation();
      if (panel !== "list") backToList();
      else onClose();
      return;
    }
    if (event.key === "Enter" && panel === "form" && !event.isComposing && event.keyCode !== 229) {
      const element = event.target as HTMLElement | null;
      if (element?.tagName === "INPUT") {
        event.preventDefault();
        void saveForm();
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="editor-overlay ledger-overlay" use:imeInset on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
  <div
    class="editor-dialog ledger-sheet ledger-manager"
    style={`--accent: ${accent}`}
    role="dialog"
    aria-label="账户与转账"
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="ledger-sheet-head">
      {#if panel !== "list"}
        <button type="button" class="ledger-icon-button" title="返回列表" aria-label="返回列表" on:click={backToList}>
          <ArrowLeft size={18} />
        </button>
      {/if}
      <span class="ledger-sheet-title">
        {panel === "form" ? (editingId ? "编辑账户" : "添加账户") : panel === "transfer" ? "账户转账" : "账户与转账"}
      </span>
      <div class="ledger-sheet-actions">
        <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
          <X size={18} />
        </button>
      </div>
    </header>

    {#if panel === "list"}
      <div class="ledger-sheet-body">
        <div class="ledger-manager-summary">
          <div>
            <span>净资产</span>
            <strong>{formatCents(assets.net)}</strong>
          </div>
          <div>
            <span>总资产</span>
            <b class="in">{formatCents(assets.assets)}</b>
          </div>
          <div>
            <span>总负债</span>
            <b class="out">{formatCents(assets.liabilities)}</b>
          </div>
        </div>

        {#each assets.perAccount as item (item.account.id)}
          {@const iconName = accountIconName(item.account.icon, item.account.kind)}
          {@const color = item.account.color || ACCOUNT_KIND_COLOR[item.account.kind]}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="ledger-account-row" role="button" tabindex="0" on:click={() => beginEdit(item.account.id)}>
            <span class="ledger-account-icon" style="--cat: {color}; background: {softColor(color)}">
              <svelte:component this={ledgerIcon(iconName, iconName)} size={17} />
            </span>
            <span class="ledger-account-text">
              <strong>{item.account.name}</strong>
              <em>{ACCOUNT_KIND_LABEL[item.account.kind]}{item.account.note ? ` · ${item.account.note}` : ""}</em>
            </span>
            <b class="ledger-account-balance" class:negative={item.balance < 0}>{formatCents(item.balance)}</b>
            <span class="ledger-row-actions">
              <button type="button" class="ledger-mini-button" title="编辑账户" on:click|stopPropagation={() => beginEdit(item.account.id)}>
                <PenLine size={14} />
              </button>
            </span>
          </div>
        {:else}
          <div class="ledger-day-empty">还没有账户，点下面的「添加账户」建一个。</div>
        {/each}
      </div>

      <footer class="ledger-sheet-foot">
        <button type="button" class="settings-button" on:click={beginTransfer}>
          <ArrowLeftRight size={15} />转账
        </button>
        <span class="ledger-foot-spacer"></span>
        <button type="button" class="settings-button primary" on:click={beginAdd}>
          <Plus size={15} />添加账户
        </button>
      </footer>

    {:else if panel === "transfer"}
      <div class="ledger-sheet-body ledger-form-body">
        <div class="ledger-field-row ledger-field-column">
          <span>转出账户</span>
          <div class="ledger-choice-row">
            {#each book.accounts as account (account.id)}
              <button
                type="button"
                class="ledger-choice"
                class:active={fromId === account.id}
                on:click={() => {
                  fromId = account.id;
                  if (toId === account.id) toId = book.accounts.find((item) => item.id !== account.id)?.id ?? "";
                }}
              >{account.name}</button>
            {/each}
          </div>
        </div>

        <div class="ledger-field-row ledger-field-column">
          <span>转入账户</span>
          <div class="ledger-choice-row">
            {#each book.accounts as account (account.id)}
              <button type="button" class="ledger-choice" class:active={toId === account.id} on:click={() => (toId = account.id)}>
                {account.name}
              </button>
            {/each}
          </div>
        </div>

        <label class="ledger-field-row">
          <span>金额</span>
          <input bind:value={transferText} type="text" inputmode="decimal" placeholder="0.00" on:keydown={fieldKeydown} />
        </label>

        <label class="ledger-field-row">
          <span>日期</span>
          <input bind:value={transferDate} type="date" on:keydown={fieldKeydown} />
        </label>

        <label class="ledger-field-row">
          <span>备注</span>
          <input bind:value={transferNote} type="text" maxlength="120" placeholder="例如 还信用卡" on:keydown={fieldKeydown} />
        </label>

        <p class="ledger-sheet-hint">转账只改两个账户的余额，不计入收支统计。</p>
      </div>

      <footer class="ledger-sheet-foot">
        <span class="ledger-foot-hint">{transferCents > 0 ? `${formatCents(transferCents)} 元` : ""}</span>
        <button type="button" class="settings-button" on:click={backToList}>取消</button>
        <button type="button" class="settings-button primary" disabled={busy} on:click={() => void saveTransfer()}>确认转账</button>
      </footer>

    {:else}
      <div class="ledger-sheet-body ledger-form-body">
        <label class="ledger-field-row">
          <span>名称</span>
          <input bind:this={nameInput} bind:value={nameDraft} type="text" maxlength="20" placeholder="例如 招商银行" on:keydown={fieldKeydown} />
        </label>

        <div class="ledger-field-row ledger-field-column">
          <span>类型</span>
          <div class="ledger-choice-row">
            {#each KINDS as kind (kind)}
              <button type="button" class="ledger-choice" class:active={kindDraft === kind} on:click={() => pickKind(kind)}>
                <svelte:component this={ledgerIcon(ACCOUNT_KIND_ICON[kind], "Wallet")} size={14} />
                {ACCOUNT_KIND_LABEL[kind]}
              </button>
            {/each}
          </div>
        </div>

        <label class="ledger-field-row">
          <span>期初余额</span>
          <input bind:value={initialDraft} type="text" inputmode="decimal" placeholder="0.00" on:keydown={fieldKeydown} />
        </label>

        <div class="ledger-field-row ledger-field-column">
          <span>图标</span>
          <div class="ledger-icon-grid">
            {#each LEDGER_ICON_CHOICES as name (name)}
              {@const icon = ledgerIcon(name, name)}
              <button
                type="button"
                class="ledger-icon-cell"
                class:active={previewIcon === name}
                title={name}
                on:click={() => (iconDraft = iconDraft === name ? "" : name)}
              >
                <svelte:component this={icon} size={18} />
              </button>
            {/each}
          </div>
        </div>

        <div class="ledger-field-row ledger-field-column">
          <span>颜色</span>
          <div class="ledger-color-row">
            {#each COLORS as color (color)}
              <button
                type="button"
                class="ledger-color-dot"
                class:active={previewColor === color}
                style="background: {color}"
                title={color}
                on:click={() => (colorDraft = colorDraft === color ? "" : color)}
              ></button>
            {/each}
            <label class="ledger-color-custom" title="自定义颜色">
              <input type="color" value={previewColor} on:input={(event) => (colorDraft = event.currentTarget.value)} />
            </label>
            {#if colorDraft}
              <button type="button" class="ledger-choice" on:click={() => (colorDraft = "")}>跟随类型</button>
            {/if}
          </div>
        </div>

        <label class="ledger-field-row">
          <span>备注</span>
          <input bind:value={noteDraft} type="text" maxlength="60" placeholder="选填" on:keydown={fieldKeydown} />
        </label>

        <div class="ledger-form-preview">
          <span class="ledger-tile-icon" style="--cat: {previewColor}; background: {softColor(previewColor)}">
            <svelte:component this={ledgerIcon(previewIcon, "Wallet")} size={20} />
          </span>
          <em>{nameDraft.trim() || "账户名称"}</em>
        </div>
      </div>

      <footer class="ledger-sheet-foot">
        {#if editingId}
          <button type="button" class="settings-button danger" disabled={busy} on:click={() => void removeForm()}>
            <Trash2 size={15} />删除账户
          </button>
        {/if}
        <span class="ledger-foot-spacer"></span>
        <button type="button" class="settings-button" on:click={backToList}>取消</button>
        <button type="button" class="settings-button primary" disabled={busy || !nameDraft.trim()} on:click={() => void saveForm()}>保存</button>
      </footer>
    {/if}
  </div>
</div>

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
  import { onMount } from "svelte";
  import { ArrowLeft, ArrowLeftRight, PenLine, Plus, Trash2, X } from "@lucide/svelte";
  import { addBackInterceptor } from "../platform";
  import { appSettings, showToast, todayIso } from "../stores";
  import { imeInset } from "../imeInset";
  import { suppressGhostClick } from "../ghostClick";
  import { fieldKeydown } from "../shortcuts";
  import { ledgerAccent } from "../styles";
  import { accountBalance, assetsOverview, formatCents, parseYuanToCents } from "../ledger";
  import {
    LEDGER_ACCOUNT_ICON_GROUPS, ledgerIcon, softColor
  } from "../ledgerIcons";
  import {
    ACCOUNT_TYPE_PRESETS, accountTypeColor, accountTypeIcon, accountTypeLabel
  } from "../ledgerAccountTypes";
  import {
    addLedgerAccount, addLedgerAccountType, deleteLedgerAccount, deleteLedgerAccountType,
    transferLedger, updateLedgerAccount, updateLedgerAccountType
  } from "../actions";
  import type { LedgerAccountType, LedgerBook } from "../types";

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
  const ALL_ICON_GROUP = "全部";

  let panel: "list" | "form" | "transfer" = startWithTransfer ? "transfer" : "list";
  /** null = 不在表单里；空串 = 新建；否则是要改的账户 id */
  let editingId: string | null = null;
  let nameDraft = "";
  let kindDraft = "cash";
  let iconDraft = "";
  let colorDraft = "";
  /** 新建 = 期初余额；编辑 = 当前金额（保存后余额直接变成这个数） */
  let amountDraft = "";
  let noteDraft = "";
  let busy = false;
  /** 自定义账户类型的小表单：typeForm.id 空串 = 新建，否则是编辑既有类型 */
  let typeFormOpen = false;
  let typeForm = { id: "", name: "", icon: "", color: "" };
  let iconGroup = ALL_ICON_GROUP;

  let fromId = book.accounts[0]?.id ?? "";
  let toId = book.accounts.find((item) => item.id !== fromId)?.id ?? "";
  let transferText = "";
  let transferNote = "";
  let transferDate = todayIso();

  $: assets = assetsOverview(book);
  $: accent = ledgerAccent($appSettings.ledger);
  $: transferCents = parseYuanToCents(transferText) ?? 0;
  /** 类型候选：预置 + 账本里持久化的自定义类型 + 已有账户用过的 + 当前草稿 */
  $: kindChoices = [...new Set([
    ...ACCOUNT_TYPE_PRESETS.map((item) => item.kind),
    ...book.accountTypes.map((item) => item.name),
    ...book.accounts.map((item) => item.kind),
    kindDraft
  ])];
  $: iconChoices =
    iconGroup === ALL_ICON_GROUP
      ? LEDGER_ACCOUNT_ICON_GROUPS.flatMap((group) => [...group.icons])
      : LEDGER_ACCOUNT_ICON_GROUPS.find((group) => group.name === iconGroup)?.icons ?? [];
  /** 表单预览：没手选颜色/图标就退回该类型（预置或自定义）的默认值 */
  $: previewColor = colorDraft || accountTypeColor(kindDraft, book.accountTypes);
  $: previewIcon = iconDraft || accountTypeIcon(kindDraft, book.accountTypes);

  if (editId) {
    const target = book.accounts.find((item) => item.id === editId);
    if (target) beginEdit(target.id);
  } else if (startWithAdd) {
    beginAdd();
  }
  /** 打开时直接落在表单/转账面板（资产视图的添加/编辑/转账入口）：用户没见过列表面板，
   *  返回就该直接关掉浮层回到他来时的页面，而不是先退到一个「陌生的账户列表」再关。 */
  const startedOutsideList = panel !== "list";

  function beginAdd(): void {
    editingId = "";
    nameDraft = "";
    kindDraft = "cash";
    iconDraft = "";
    colorDraft = "";
    amountDraft = "";
    noteDraft = "";
    typeFormOpen = false;
    panel = "form";
  }

  function beginEdit(id: string): void {
    const target = book.accounts.find((item) => item.id === id);
    if (!target) return;
    editingId = target.id;
    nameDraft = target.name;
    kindDraft = target.kind;
    iconDraft = target.icon;
    colorDraft = target.color;
    // 编辑的是「当前金额」（余额 = 期初 + 流水，现场推导）：对齐用户看到的数，
    // 保存时 core 把差额折回期初——余额永远只有推导这一个来源，不存第二份
    amountDraft = formatCents(accountBalance(book, target.id));
    noteDraft = target.note;
    typeFormOpen = false;
    panel = "form";
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

  onMount(() =>
    // 安卓返回键逐级退：类型小表单 → 账户表单/转账 → 列表 → 关浮层。
    // 早先漏了 typeFormOpen 这一级（从类型表单返回会连账户表单一起跳过），
    // 也没有「直达表单」的语义（从资产页点添加进来的，返回却退到一个用户
    // 从没见过的列表面板——「返回到奇怪的位置」正是这个）。
    addBackInterceptor(() => {
      if (typeFormOpen) {
        typeFormOpen = false;
        return true;
      }
      if (panel !== "list") {
        if (startedOutsideList) onClose();
        else backToList();
        return true;
      }
      onClose();
      return true;
    })
  );

  /** 选账户类型时顺手把图标换回该类型的默认（用户已手选过就不动） */
  function pickKind(kind: string): void {
    const wasDefault = !iconDraft || iconDraft === accountTypeIcon(kindDraft, book.accountTypes);
    kindDraft = kind;
    typeFormOpen = false;
    if (wasDefault) iconDraft = "";
  }

  /** 类型行末的加号 / 自定义 chip 上的铅笔：打开类型小表单（新建或编辑） */
  function openTypeForm(type: LedgerAccountType | null): void {
    typeForm = type
      ? { id: type.id, name: type.name, icon: type.icon, color: type.color }
      : { id: "", name: "", icon: "", color: "" };
    typeFormOpen = true;
  }

  async function saveTypeForm(): Promise<void> {
    if (!typeFormOpen || busy) return;
    const name = typeForm.name.trim();
    if (!name) {
      showToast("类型名称不能为空");
      return;
    }
    busy = true;
    const draft = { name, icon: typeForm.icon, color: typeForm.color };
    const ok = typeForm.id
      ? await updateLedgerAccountType(typeForm.id, draft)
      : await addLedgerAccountType(draft);
    busy = false;
    if (!ok) return;
    typeFormOpen = false;
    // 建好即选中：账户跟着用上这个类型的图标与颜色默认值
    pickKind(name);
  }

  /** 删类型只删这条「建议」：已建账户的 kind 字符串与图标颜色都不受影响 */
  async function removeTypeForm(): Promise<void> {
    if (!typeFormOpen || !typeForm.id || busy) return;
    busy = true;
    const ok = await deleteLedgerAccountType(typeForm.id);
    busy = false;
    if (!ok) return;
    const gone = typeForm.name;
    typeFormOpen = false;
    if (kindDraft === gone) kindDraft = "cash";
  }

  async function saveForm(): Promise<void> {
    if (busy) return;
    const name = nameDraft.trim();
    if (!name) {
      showToast("账户名称不能为空");
      return;
    }
    const trimmed = amountDraft.trim();
    const amountCents = trimmed === "" ? null : parseYuanToCents(trimmed);
    if (trimmed !== "" && amountCents === null) {
      showToast("金额要是一个数字（可以是负数）");
      return;
    }
    busy = true;
    const draft = {
      name,
      kind: kindDraft,
      icon: iconDraft,
      color: colorDraft,
      note: noteDraft.trim()
    };
    // 编辑：余额直设成新值（core 把差额折回期初，流水与统计都不动）；留空 = 不改金额
    // 新建：这就是期初余额
    const ok = editingId
      ? await updateLedgerAccount(editingId, amountCents === null ? draft : { ...draft, balanceCents: amountCents })
      : await addLedgerAccount({ ...draft, initialCents: amountCents ?? 0 });
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
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    onClose();
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
          {@const iconName = item.account.icon || accountTypeIcon(item.account.kind, book.accountTypes)}
          {@const color = item.account.color || accountTypeColor(item.account.kind, book.accountTypes)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="ledger-account-row" role="button" tabindex="0" on:click={() => beginEdit(item.account.id)}>
            <span class="ledger-account-icon" style="--cat: {color}; background: {softColor(color)}">
              <svelte:component this={ledgerIcon(iconName, iconName)} size={17} />
            </span>
            <span class="ledger-account-text">
              <strong>{item.account.name}</strong>
              <em>{accountTypeLabel(item.account.kind)}{item.account.note ? ` · ${item.account.note}` : ""}</em>
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
          <input bind:value={nameDraft} type="text" maxlength="20" placeholder="例如 招商银行" on:keydown={fieldKeydown} />
        </label>

        <label class="ledger-field-row">
          <span>备注</span>
          <input bind:value={noteDraft} type="text" maxlength="60" placeholder="选填" on:keydown={fieldKeydown} />
        </label>

        <div class="ledger-field-row ledger-field-column">
          <span>类型</span>
          <div class="ledger-choice-row">
            {#each kindChoices as kind (kind)}
              {@const customType = book.accountTypes.find((item) => item.name === kind)}
              <button type="button" class="ledger-choice" class:active={kindDraft === kind} on:click={() => pickKind(kind)}>
                <span class="ledger-choice-icon" style="color: {accountTypeColor(kind, book.accountTypes)}">
                  <svelte:component this={ledgerIcon(accountTypeIcon(kind, book.accountTypes), "Wallet")} size={14} />
                </span>
                {accountTypeLabel(kind)}
                {#if customType}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span
                    class="ledger-choice-edit"
                    role="button"
                    tabindex="0"
                    title="编辑或删除这个自定义类型"
                    on:click|stopPropagation={() => openTypeForm(customType)}
                    on:keydown|stopPropagation={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        openTypeForm(customType);
                      }
                    }}
                  ><PenLine size={12} /></span>
                {/if}
              </button>
            {/each}
            <button
              type="button"
              class="ledger-choice"
              title="自定义账户类型（可挑图标与颜色，之后能改能删）"
              on:click={() => openTypeForm(null)}
            ><Plus size={14} />类型</button>
          </div>

          {#if typeFormOpen}
            <!-- 自定义类型的小表单：类型持久化在账本里，之后建账户直接选 -->
            <div class="ledger-type-form">
              <label class="ledger-field-row">
                <span>名称</span>
                <input
                  bind:value={typeForm.name}
                  type="text"
                  maxlength="10"
                  placeholder="例如 校园卡"
                  on:keydown={fieldKeydown}
                />
              </label>
              <div class="ledger-field-row ledger-field-column">
                <span>图标</span>
                <div class="ledger-icon-groups" role="tablist" aria-label="类型图标分组">
                  <button
                    type="button"
                    class="ledger-icon-group"
                    class:active={iconGroup === ALL_ICON_GROUP}
                    on:click={() => (iconGroup = ALL_ICON_GROUP)}
                  >{ALL_ICON_GROUP}</button>
                  {#each LEDGER_ACCOUNT_ICON_GROUPS as group (group.name)}
                    <button
                      type="button"
                      class="ledger-icon-group"
                      class:active={iconGroup === group.name}
                      on:click={() => (iconGroup = group.name)}
                    >{group.name}</button>
                  {/each}
                </div>
                <div class="ledger-icon-grid">
                  {#each iconChoices as name (name)}
                    {@const icon = ledgerIcon(name, name)}
                    <button
                      type="button"
                      class="ledger-icon-cell"
                      class:active={typeForm.icon === name}
                      title={name}
                      on:click={() => (typeForm.icon = typeForm.icon === name ? "" : name)}
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
                      class:active={typeForm.color === color}
                      style="background: {color}"
                      title={color}
                      on:click={() => (typeForm.color = typeForm.color === color ? "" : color)}
                    ></button>
                  {/each}
                  <label class="ledger-color-custom" title="自定义颜色">
                    <input
                      type="color"
                      value={typeForm.color || "#7f8c8d"}
                      on:input={(event) => (typeForm.color = event.currentTarget.value)}
                    />
                  </label>
                </div>
              </div>
              <div class="ledger-type-form-actions">
                {#if typeForm.id}
                  <button type="button" class="settings-button danger" disabled={busy} on:click={() => void removeTypeForm()}>
                    <Trash2 size={14} />删除类型
                  </button>
                {/if}
                <span class="ledger-foot-spacer"></span>
                <button type="button" class="settings-button" on:click={() => (typeFormOpen = false)}>取消</button>
                <button
                  type="button"
                  class="settings-button primary"
                  disabled={busy || !typeForm.name.trim()}
                  on:click={() => void saveTypeForm()}
                >{typeForm.id ? "保存类型" : "添加类型"}</button>
              </div>
            </div>
          {/if}
        </div>

        <label class="ledger-field-row">
          <span>{editingId ? "当前金额" : "期初余额"}</span>
          <input
            bind:value={amountDraft}
            type="text"
            inputmode="decimal"
            placeholder="0.00"
            title={editingId
              ? "保存后账户余额直接变成这个数：差额自动折进期初，流水与统计一概不动（可以为负，信用卡尤其如此）"
              : "开始记账之前账户里已有的钱（可以为负）"}
            on:keydown={fieldKeydown}
          />
        </label>

        <div class="ledger-field-row ledger-field-column">
          <span>图标</span>
          <div class="ledger-icon-groups" role="tablist" aria-label="账户图标分组">
            <button
              type="button"
              class="ledger-icon-group"
              class:active={iconGroup === ALL_ICON_GROUP}
              on:click={() => (iconGroup = ALL_ICON_GROUP)}
            >{ALL_ICON_GROUP}</button>
            {#each LEDGER_ACCOUNT_ICON_GROUPS as group (group.name)}
              <button
                type="button"
                class="ledger-icon-group"
                class:active={iconGroup === group.name}
                on:click={() => (iconGroup = group.name)}
              >{group.name}</button>
            {/each}
          </div>
          <div class="ledger-icon-grid">
            {#each iconChoices as name (name)}
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

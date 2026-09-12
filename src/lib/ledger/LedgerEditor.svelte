<script lang="ts">
  /**
   * 记账面板（记一笔 / 改一笔）：复用应用的 .editor-overlay 浮层——桌面居中对话框、
   * 移动端底部抽屉（主流记账 App 的同一条手感）。
   * 结构自上而下：类型页签 → 大额金额 → 分类（大类 chips + 子分类图标网格）→
   * 日期/账户/备注 → 移动端数字键盘（桌面是「保存再记 / 记一笔」两个按钮）。
   *
   * **只有显式点保存才落盘**：关掉（X / 遮罩 / Esc / 移动端返回）一律丢弃草稿。
   * 卡片编辑器"关掉即保存"是为了防丢正文，账目照抄那条会凭空多出用户没确认过的记录。
   */
  import { onMount } from "svelte";
  import {
    ArrowLeftRight, CalendarDays, Check, ChevronRight, Delete, PenLine, Trash2, Wallet, X
  } from "@lucide/svelte";
  import { appSettings, showToast, todayIso } from "../stores";
  import { isMobile } from "../platform";
  import { imeInset } from "../imeInset";
  import { fieldKeydown } from "../shortcuts";
  import { clampPopoverToViewport } from "../popover";
  import { ledgerAccent, uiScaleValue } from "../styles";
  import { categoryTree, formatCents, parseYuanToCents } from "../ledger";
  import { accountIconName, ledgerIcon, softColor, TRANSFER_ICON } from "../ledgerIcons";
  import { addLedgerEntry, transferLedger, updateLedgerEntry, deleteLedgerEntry } from "../actions";
  import { relativeDayLabel, todayDate } from "../diary";
  import DatePicker from "../DatePicker.svelte";
  import type { LedgerBook, LedgerEditorTarget, LedgerKind, LedgerSide } from "../types";

  export let target: LedgerEditorTarget;
  export let book: LedgerBook;
  export let onClose: () => void = () => {};

  const existing = "id" in target ? book.entries.find((entry) => entry.id === target.id) : undefined;

  let kind: LedgerKind = existing?.kind ?? ("kind" in target ? target.kind : "expense");
  let amountText = existing ? (existing.amountCents / 100).toFixed(2).replace(/\.?0+$/, "") : "";
  let accountId = existing?.accountId ?? book.accounts[0]?.id ?? "";
  let toAccountId = existing?.toAccountId ?? book.accounts.find((item) => item.id !== accountId)?.id ?? "";
  let categoryId = existing?.categoryId ?? "";
  let date = existing?.date ?? ("date" in target ? target.date : todayIso());
  let note = existing?.note ?? "";
  let busy = false;
  let closed = false;
  let openPicker: "" | "date" | "account" | "toAccount" = "";
  let metaRowEl: HTMLDivElement;

  /** 大类 chips 里当前展开的那一个；跟着已选分类走，没有就落在第一个大类 */
  let parentDraft = "";

  $: side = (kind === "income" ? "income" : "expense") as LedgerSide;
  $: tree = categoryTree(book, side);
  $: selectedCategory = book.categories.find((item) => item.id === categoryId);
  $: openParent =
    parentDraft || selectedCategory?.parentId || selectedCategory?.id || tree[0]?.parent.id || "";
  $: openParentCategory = tree.find((item) => item.parent.id === openParent)?.parent ?? null;
  $: openChildren = tree.find((item) => item.parent.id === openParent)?.children ?? [];
  /** 大类没有子分类时，它自己就是可选的那一格 */
  $: tiles = openChildren.length > 0 ? openChildren : openParentCategory ? [openParentCategory] : [];
  $: cents = parseYuanToCents(amountText) ?? 0;
  $: account = book.accounts.find((item) => item.id === accountId);
  $: toAccount = book.accounts.find((item) => item.id === toAccountId);
  $: today = todayDate();
  $: dateLabel = date === today ? "今天" : relativeDayLabel(date, today);
  $: accent = ledgerAccent($appSettings.ledger);
  $: sideColor = kind === "income" ? "#2f9e6e" : kind === "transfer" ? "#6b7fd7" : "#e0654f";

  function switchKind(next: LedgerKind): void {
    if (next === kind) return;
    kind = next;
    parentDraft = "";
    if (next === "transfer") {
      if (toAccountId === accountId) {
        toAccountId = book.accounts.find((item) => item.id !== accountId)?.id ?? "";
      }
      return;
    }
    const nextSide: LedgerSide = next === "income" ? "income" : "expense";
    const current = book.categories.find((item) => item.id === categoryId);
    if (!current || current.side !== nextSide) categoryId = "";
  }

  function pressKey(key: string): void {
    if (key === "back") {
      amountText = amountText.slice(0, -1);
      return;
    }
    if (key === "clear") {
      amountText = "";
      return;
    }
    if (key === ".") {
      if (!amountText.includes(".")) amountText = `${amountText || "0"}.`;
      return;
    }
    const [whole, frac = ""] = amountText.split(".");
    if (frac.length >= 2) return;
    if (whole.length >= 9 && !amountText.includes(".")) return;
    amountText = amountText === "0" ? key : `${amountText}${key}`;
  }

  function pickParent(id: string): void {
    parentDraft = id;
    const parent = tree.find((item) => item.parent.id === id)?.parent;
    // 大类自己没有子分类时，点它就是选它
    if (parent && !tree.find((item) => item.parent.id === id)?.children.length) {
      categoryId = categoryId === id ? "" : id;
    }
  }

  function pickCategory(id: string): void {
    categoryId = categoryId === id ? "" : id;
  }

  function togglePicker(name: typeof openPicker): void {
    openPicker = openPicker === name ? "" : name;
    if (openPicker) {
      void clampPopoverToViewport(metaRowEl, uiScaleValue($appSettings.appearance.uiScale), ".ledger-pop");
    }
  }

  function pickAccount(id: string, which: "account" | "toAccount"): void {
    if (which === "account") {
      accountId = id;
      if (kind === "transfer" && toAccountId === id) {
        toAccountId = book.accounts.find((item) => item.id !== id)?.id ?? "";
      }
    } else {
      toAccountId = id;
    }
    openPicker = "";
  }

  function dismissPopovers(event: Event): void {
    if (!openPicker) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".ledger-meta-field, .ledger-transfer-field")) return;
    openPicker = "";
  }

  // 捕获阶段：对话框对 pointerdown 做了 stopPropagation，冒泡阶段收不到里面的点击
  onMount(() => {
    window.addEventListener("pointerdown", dismissPopovers, true);
    return () => window.removeEventListener("pointerdown", dismissPopovers, true);
  });

  function valid(): boolean {
    if (cents <= 0) return false;
    if (kind === "transfer") return Boolean(accountId) && Boolean(toAccountId) && accountId !== toAccountId;
    return Boolean(accountId);
  }

  /** 真正落盘。keepOpen = 「保存再记」：清掉金额与备注，分类/账户/日期留着连记。 */
  async function commit(keepOpen: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    let ok = false;
    if (kind === "transfer") {
      ok = await transferLedger({ from: accountId, to: toAccountId, amountCents: cents, date, note });
    } else if (existing) {
      ok = await updateLedgerEntry(existing.id, {
        kind,
        amountCents: cents,
        accountId,
        categoryId: categoryId || undefined,
        date,
        note
      });
    } else {
      ok = await addLedgerEntry({
        kind,
        amountCents: cents,
        accountId,
        categoryId: categoryId || undefined,
        date,
        note
      });
    }
    busy = false;
    if (!ok) return;
    if (keepOpen) {
      amountText = "";
      note = "";
      return;
    }
    closed = true;
    onClose();
  }

  /** 显式点保存：不合法要给一句话，不能默默不动。 */
  async function save(keepOpen: boolean): Promise<void> {
    if (cents <= 0) {
      showToast("金额要大于 0");
      return;
    }
    if (kind === "transfer" && accountId === toAccountId) {
      showToast("转出与转入不能是同一个账户");
      return;
    }
    if (!valid()) {
      showToast("请先选择账户");
      return;
    }
    await commit(keepOpen);
  }

  /** 关闭（X / 点遮罩 / Esc / 移动端返回）：一律**不落盘**。
   *  记账与卡片编辑器刻意不同——卡片"关掉即保存"是防丢正文，而金额这里
   *  自动落盘会凭空多出一堆用户没确认过的账，所以只有显式点「记一笔 / 保存」才写。 */
  function closeEditor(): void {
    if (closed) return;
    closed = true;
    onClose();
  }

  async function remove(): Promise<void> {
    if (!existing || busy) return;
    busy = true;
    const ok = await deleteLedgerEntry(existing.id);
    busy = false;
    if (!ok) return;
    closed = true;
    onClose();
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target === event.currentTarget) closeEditor();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      event.stopPropagation();
      if (openPicker) {
        openPicker = "";
        return;
      }
      closeEditor();
      return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void save(false);
      return;
    }
    // 桌面没有键盘：数字键直接喂金额（输入框聚焦时不抢）
    if ($isMobile || busy) return;
    const element = event.target as HTMLElement | null;
    if (element?.closest("input, textarea")) return;
    if (/^[0-9.]$/.test(event.key)) {
      event.preventDefault();
      pressKey(event.key);
    } else if (event.key === "Backspace") {
      event.preventDefault();
      pressKey("back");
    } else if (event.key === "Enter") {
      event.preventDefault();
      void save(false);
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="editor-overlay ledger-overlay"
  style={`--side: ${sideColor}`}
  use:imeInset
  on:pointerdown={handleBackdrop}
  on:contextmenu|preventDefault|stopPropagation
>
  <div
    class="editor-dialog ledger-sheet"
    style={`--accent: ${accent}`}
    role="dialog"
    aria-label={existing ? "修改这一笔" : "记一笔"}
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="ledger-sheet-head">
      <div class="ledger-kind-tabs" role="tablist">
        {#each [["expense", "支出"], ["income", "收入"], ["transfer", "转账"]] as [value, label] (value)}
          <button
            type="button"
            role="tab"
            aria-selected={kind === value}
            class:active={kind === value}
            on:click={() => switchKind(value as LedgerKind)}
          >{label}</button>
        {/each}
      </div>
      <div class="ledger-sheet-actions">
        {#if $isMobile}
          <button type="button" class="ledger-head-text" title="保存并接着记下一笔" on:click={() => void save(true)}>
            保存再记
          </button>
        {/if}
        {#if existing}
          <button type="button" class="ledger-icon-button danger" title="删除这一笔" on:click={() => void remove()}>
            <Trash2 size={17} />
          </button>
        {/if}
        <button type="button" class="ledger-icon-button" title="关闭（不保存这一笔）" aria-label="关闭" on:click={closeEditor}>
          <X size={18} />
        </button>
      </div>
    </header>

    <div class="ledger-amount-row">
      {#if kind === "transfer"}
        <span class="ledger-amount-cat">
          <span class="ledger-amount-icon" style="background: {softColor("#6b7fd7")}">
            <svelte:component this={ledgerIcon(TRANSFER_ICON, TRANSFER_ICON)} size={18} />
          </span>
          <em>{account?.name ?? "转出"} → {toAccount?.name ?? "转入"}</em>
        </span>
      {:else}
        {@const color = selectedCategory?.color || (side === "income" ? "#2f9e6e" : "#f0862c")}
        {@const icon = ledgerIcon(selectedCategory?.icon, side === "income" ? "Banknote" : "Package")}
        <span class="ledger-amount-cat">
          <span class="ledger-amount-icon" style="--cat: {color}; background: {softColor(color)}">
            <svelte:component this={icon} size={18} />
          </span>
          <em class:selected={Boolean(selectedCategory)}>{selectedCategory?.name ?? "选择分类"}</em>
        </span>
      {/if}

      {#if $isMobile}
        <!-- 移动端金额只由键盘驱动：再放一个可聚焦的输入框会和软键盘抢位 -->
        <div class="ledger-amount-value" class:empty={!amountText}>
          <span class="ledger-amount-symbol">¥</span>{amountText || "0.00"}<i class="ledger-amount-caret"></i>
        </div>
      {:else}
        <label class="ledger-amount-field">
          <span class="ledger-amount-symbol">¥</span>
          <input
            type="text"
            inputmode="decimal"
            autocomplete="off"
            placeholder="0.00"
            aria-label="金额（元）"
            style="width: {Math.max(4, amountText.length + 1)}ch"
            bind:value={amountText}
          />
        </label>
      {/if}
    </div>

    <div class="ledger-sheet-body">
      {#if kind === "transfer"}
        <div class="ledger-transfer-row" on:click|stopPropagation>
          <div class="ledger-transfer-field" class:open={openPicker === "account"}>
            <button type="button" on:click={() => togglePicker("account")}>
              <span>转出</span>
              <strong>{account?.name ?? "选择账户"}</strong>
              <ChevronRight size={15} />
            </button>
            {#if openPicker === "account"}
              <div class="ledger-pop">
                {#each book.accounts as item (item.id)}
                  <button type="button" class="ledger-pick-row" class:active={item.id === accountId} on:click={() => pickAccount(item.id, "account")}>
                    <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                    <span>{item.name}</span>
                    {#if item.id === accountId}<Check size={14} />{/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <ArrowLeftRight class="ledger-transfer-arrow" size={17} />
          <div class="ledger-transfer-field" class:open={openPicker === "toAccount"}>
            <button type="button" on:click={() => togglePicker("toAccount")}>
              <span>转入</span>
              <strong>{toAccount?.name ?? "选择账户"}</strong>
              <ChevronRight size={15} />
            </button>
            {#if openPicker === "toAccount"}
              <div class="ledger-pop">
                {#each book.accounts as item (item.id)}
                  <button type="button" class="ledger-pick-row" class:active={item.id === toAccountId} on:click={() => pickAccount(item.id, "toAccount")}>
                    <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                    <span>{item.name}</span>
                    {#if item.id === toAccountId}<Check size={14} />{/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
        <p class="ledger-sheet-hint">转账只改两个账户的余额，不计入收支统计。</p>
      {:else}
        <div class="ledger-parent-row">
          {#each tree as item (item.parent.id)}
            {@const color = item.parent.color || (side === "income" ? "#2f9e6e" : "#f0862c")}
            <button
              type="button"
              class="ledger-parent-chip"
              class:active={openParent === item.parent.id}
              on:click={() => pickParent(item.parent.id)}
            >
              <span class="ledger-chip-icon" style="--cat: {color}; background: {softColor(color)}">
                <svelte:component this={ledgerIcon(item.parent.icon, "Package")} size={14} />
              </span>
              {item.parent.name}
            </button>
          {:else}
            <p class="ledger-sheet-hint">还没有{side === "income" ? "收入" : "支出"}分类，去齿轮 → 分类管理里加一个。</p>
          {/each}
        </div>

        <div class="ledger-tile-grid">
          {#each tiles as tile (tile.id)}
            {@const color = tile.color || (side === "income" ? "#2f9e6e" : "#f0862c")}
            <button
              type="button"
              class="ledger-cat-tile"
              class:active={categoryId === tile.id}
              style="--cat: {color}"
              on:click={() => pickCategory(tile.id)}
            >
              <span class="ledger-tile-icon" style="background: {softColor(color)}">
                <svelte:component this={ledgerIcon(tile.icon, "Package")} size={20} />
              </span>
              <em>{tile.name}</em>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="ledger-meta-row" bind:this={metaRowEl} on:click|stopPropagation>
      <div class="ledger-meta-field" class:open={openPicker === "date"}>
        <button type="button" class="ledger-meta-trigger" title="归属日期" on:click={() => togglePicker("date")}>
          <CalendarDays size={15} />{dateLabel}
        </button>
        {#if openPicker === "date"}
          <div class="ledger-pop date">
            <DatePicker value={date} on:select={(event) => { date = event.detail; openPicker = ""; }} on:clear={() => { date = today; openPicker = ""; }} />
          </div>
        {/if}
      </div>

      {#if kind !== "transfer"}
        <div class="ledger-meta-field" class:open={openPicker === "account"}>
          <button type="button" class="ledger-meta-trigger" title="资金账户" on:click={() => togglePicker("account")}>
            <Wallet size={15} />{account?.name ?? "选择账户"}
          </button>
          {#if openPicker === "account"}
            <div class="ledger-pop">
              {#each book.accounts as item (item.id)}
                <button type="button" class="ledger-pick-row" class:active={item.id === accountId} on:click={() => pickAccount(item.id, "account")}>
                  <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                  <span>{item.name}</span>
                  {#if item.id === accountId}<Check size={14} />{/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <label class="ledger-note-field" title="备注">
        <PenLine size={15} />
        <input type="text" maxlength="120" placeholder="备注" bind:value={note} on:keydown={fieldKeydown} />
      </label>
    </div>

    {#if $isMobile}
      <div class="ledger-keypad">
        {#each ["7", "8", "9"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button type="button" class="ledger-key fn" title="退格" on:pointerdown|preventDefault={() => pressKey("back")}>
          <Delete size={19} />
        </button>
        {#each ["4", "5", "6"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button type="button" class="ledger-key fn" on:pointerdown|preventDefault={() => pressKey("clear")}>清除</button>
        {#each ["1", "2", "3"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button type="button" class="ledger-key save" disabled={busy} on:pointerdown|preventDefault={() => void save(false)}>
          <Check size={18} />{existing ? "保存" : "记一笔"}
        </button>
        <button type="button" class="ledger-key zero" on:pointerdown|preventDefault={() => pressKey("0")}>0</button>
        <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(".")}>.</button>
      </div>
    {:else}
      <footer class="ledger-sheet-foot">
        <span class="ledger-foot-hint">
          {#if cents > 0}{formatCents(cents)} 元{/if}
        </span>
        <span class="ledger-foot-spacer"></span>
        <button type="button" class="settings-button" disabled={busy} on:click={() => void save(true)}>保存再记</button>
        <button type="button" class="settings-button primary" disabled={busy} on:click={() => void save(false)}>
          {existing ? "保存修改" : "记一笔"}
        </button>
      </footer>
    {/if}
  </div>
</div>

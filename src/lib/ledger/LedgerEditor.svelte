<script lang="ts">
  /**
   * 记账面板：支出/收入/转账三页签 + 分类网格 + 金额键盘 + 日期/账户/备注行。
   * 手感对齐时光序：分类是大类横排、点开展子分类网格；金额走键盘（也允许直接输入）；
   * 「保存再记」连记多笔不用重开面板。新建时金额为 0 就关掉 = 不落盘（与日记同规则）。
   */
  import { X } from "@lucide/svelte";
  import type { LedgerBook, LedgerEditorTarget, LedgerKind, LedgerSide } from "../types";
  import { categoryTree, parseYuanToCents, formatCents, categoryColor } from "../ledger";
  import { ledgerIcon, SIDE_LABEL } from "../ledgerIcons";
  import { addLedgerEntry, updateLedgerEntry, transferLedger } from "../actions";
  import { todayIso } from "../stores";

  export let target: LedgerEditorTarget;
  export let book: LedgerBook;
  export let onClose: () => void = () => {};

  const existing = "id" in target ? book.entries.find((entry) => entry.id === target.id) : undefined;

  let kind: LedgerKind = existing?.kind ?? ("kind" in target ? target.kind : "expense");
  let amountText = existing ? (existing.amountCents / 100).toString() : "";
  let accountId = existing?.accountId ?? book.accounts[0]?.id ?? "";
  let toAccountId = existing?.toAccountId ?? book.accounts[1]?.id ?? "";
  let categoryId = existing?.categoryId ?? "";
  let date = existing?.date ?? ("date" in target ? target.date : todayIso());
  let note = existing?.note ?? "";
  let busy = false;

  $: side = (kind === "income" ? "income" : "expense") as LedgerSide;
  $: tree = categoryTree(book, side);
  $: selectedCategory = book.categories.find((item) => item.id === categoryId);
  $: openParent = selectedCategory?.parentId ?? selectedCategory?.id ?? tree[0]?.parent.id ?? "";
  $: openChildren = tree.find((item) => item.parent.id === openParent)?.children ?? [];
  $: cents = parseYuanToCents(amountText) ?? 0;

  function switchKind(next: LedgerKind): void {
    kind = next;
    if (next !== "transfer") {
      const nextSide: LedgerSide = next === "income" ? "income" : "expense";
      const current = book.categories.find((item) => item.id === categoryId);
      if (current && current.side !== nextSide) categoryId = "";
    }
  }

  function pressKey(key: string): void {
    if (key === "back") {
      amountText = amountText.slice(0, -1);
      return;
    }
    if (key === ".") {
      if (!amountText.includes(".")) amountText = `${amountText || "0"}.`;
      return;
    }
    const [, frac = ""] = amountText.split(".");
    if (frac.length >= 2) return;
    if (amountText === "0") amountText = key;
    else amountText += key;
  }

  function pickCategory(id: string): void {
    categoryId = categoryId === id ? "" : id;
  }

  async function save(keepOpen: boolean): Promise<void> {
    if (busy) return;
    if (cents <= 0) {
      onClose();
      return;
    }
    busy = true;
    let ok = false;
    if (kind === "transfer") {
      ok = await transferLedger({
        from: accountId,
        to: toAccountId,
        amountCents: cents,
        date,
        note
      });
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
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="ledger-editor-backdrop" on:click={() => save(false)}></div>
<div class="ledger-editor" role="dialog" aria-label="记账">
  <header class="ledger-editor-head">
    <span class="ledger-editor-tabs">
      {#each ["expense", "income", "transfer"] as value (value)}
        <button
          type="button"
          class:active={kind === value}
          on:click={() => switchKind(value as LedgerKind)}
        >
          {value === "transfer" ? "转账" : SIDE_LABEL[value as LedgerSide]}
        </button>
      {/each}
    </span>
    <button type="button" class="ledger-editor-close" on:click={() => save(false)}>
      <X size={18} />
    </button>
  </header>

  {#if kind === "transfer"}
    <div class="ledger-editor-accounts">
      <label>
        <span>转出</span>
        <select bind:value={accountId}>
          {#each book.accounts as account (account.id)}
            <option value={account.id}>{account.name}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>转入</span>
        <select bind:value={toAccountId}>
          {#each book.accounts as account (account.id)}
            <option value={account.id}>{account.name}</option>
          {/each}
        </select>
      </label>
    </div>
  {:else}
    <div class="ledger-editor-parents">
      {#each tree as item (item.parent.id)}
        {@const icon = ledgerIcon(item.parent.icon, "Package")}
        <button
          type="button"
          class:active={openParent === item.parent.id}
          on:click={() => {
            openParent = item.parent.id;
            pickCategory(item.parent.id);
          }}
        >
          <span
            class="ledger-row-icon"
            style="background:{categoryColor(book, item.parent)}"
          >
            <svelte:component this={icon} size={16} />
          </span>
          <em>{item.parent.name}</em>
        </button>
      {:else}
        <p class="ledger-day-empty">还没有{SIDE_LABEL[side]}分类，去齿轮菜单里加一个</p>
      {/each}
    </div>
    {#if openChildren.length > 0}
      <div class="ledger-editor-children">
        {#each openChildren as child (child.id)}
          {@const icon = ledgerIcon(child.icon, "Package")}
          <button
            type="button"
            class:active={categoryId === child.id}
            on:click={() => pickCategory(child.id)}
          >
            <span class="ledger-row-icon" style="background:{categoryColor(book, child)}">
              <svelte:component this={icon} size={16} />
            </span>
            <em>{child.name}</em>
          </button>
        {/each}
      </div>
    {/if}
  {/if}

  <div class="ledger-editor-amount">
    <input
      type="text"
      inputmode="decimal"
      placeholder="0.00"
      bind:value={amountText}
      aria-label="金额（元）"
    />
    <span class="ledger-editor-amount-hint">{formatCents(cents)}</span>
  </div>
  <div class="ledger-keypad">
    {#each ["1", "2", "3", "4", "5", "6", "7", "8", "9"] as key (key)}
      <button type="button" on:click={() => pressKey(key)}>{key}</button>
    {/each}
    <button type="button" class="ledger-key-back" on:click={() => pressKey("back")}>⌫</button>
    <button type="button" on:click={() => pressKey("0")}>0</button>
    <button type="button" on:click={() => pressKey(".")}>.</button>
    <button type="button" class="ledger-key-clear" on:click={() => (amountText = "")}>C</button>
  </div>

  <div class="ledger-editor-meta">
    <label>
      <span>日期</span>
      <input type="date" bind:value={date} />
    </label>
    {#if kind !== "transfer"}
      <label>
        <span>账户</span>
        <select bind:value={accountId}>
          {#each book.accounts as account (account.id)}
            <option value={account.id}>{account.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    <label class="ledger-editor-note">
      <span>备注</span>
      <input type="text" placeholder="记点什么…" bind:value={note} maxlength="120" />
    </label>
  </div>

  <footer class="ledger-editor-foot">
    <button type="button" class="settings-button" on:click={() => save(true)}>保存再记</button>
    <button type="button" class="settings-button primary" on:click={() => save(false)}>完成</button>
  </footer>
</div>

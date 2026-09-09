<script lang="ts">
  /**
   * 记账整页：四个视图（列表/日历/统计/资产）+ 齿轮面板 + 记账 FAB。
   * 与 DiaryView 同一条骨架：桌面 diary-open 式互斥、移动端历史栈一层；
   * 主题色与背景走 settings.ledger（外观同步、view 本机偏好）。
   */
  import { onMount } from "svelte";
  import {
    ArrowLeft,
    CalendarDays,
    ChartPie,
    List as ListIcon,
    Plus,
    Settings as SettingsIcon,
    Tags,
    Wallet
  } from "@lucide/svelte";
  import { appSettings, ledgerData, ledgerEditor, ledgerOpen } from "./stores";
  import { setConfig } from "./actions";
  import { buildMainStyle, ledgerAccent, ledgerBackground } from "./styles";
  import { isMobile, showMobileList } from "./platform";
  import { imageCache, resolveImageSrc } from "./images";
  import { monthOf, shiftMonth, todayDate, type MonthCursor } from "./diary";
  import { assetsOverview, formatCents, monthTotals } from "./ledger";
  import type { LedgerViewMode } from "./types";
  import MenuItem from "./menu/MenuItem.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import LedgerList from "./ledger/LedgerList.svelte";
  import LedgerCalendar from "./ledger/LedgerCalendar.svelte";
  import LedgerStats from "./ledger/LedgerStats.svelte";
  import LedgerAssets from "./ledger/LedgerAssets.svelte";
  import LedgerEditor from "./ledger/LedgerEditor.svelte";
  import LedgerEntryMenu from "./ledger/LedgerEntryMenu.svelte";
  import CategoryManager from "./ledger/CategoryManager.svelte";
  import AccountManager from "./ledger/AccountManager.svelte";

  const VIEWS: Array<{ mode: LedgerViewMode; label: string; icon: typeof ListIcon }> = [
    { mode: "list", label: "列表视图", icon: ListIcon },
    { mode: "calendar", label: "日历视图", icon: CalendarDays },
    { mode: "stats", label: "统计视图", icon: ChartPie },
    { mode: "assets", label: "资产视图", icon: Wallet }
  ];

  let showGear = false;
  let gearButtonEl: HTMLButtonElement;
  let listMenuAt: { x: number; y: number } | null = null;
  let entryMenu: { id: string; x: number; y: number } | null = null;
  let showCategories = false;
  let showAccounts = false;
  let startWithTransfer = false;
  let accountEditId = "";
  let scrollEl: HTMLElement;
  let paging = false;

  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
  /** 列表视图已加载的月份栈（新→旧）；滚到底接上一个月、滚到顶接回下一个月 */
  let months: MonthCursor[] = [monthOf(todayDate())];
  let dayTick = 0;

  onMount(() => {
    const timer = window.setInterval(() => {
      dayTick += 1;
    }, 60_000);
    return () => window.clearInterval(timer);
  });

  $: today = dayTick >= 0 ? todayDate() : "";
  $: view = $appSettings.ledger.view;
  $: book = $ledgerData;
  $: thisMonth = monthOf(today);
  $: monthTotal = monthTotals(book.entries, cursor);
  $: assets = assetsOverview(book);
  $: ledgerBg = ledgerBackground($appSettings.ledger);
  $: resolvedBgImage = resolveImageSrc(ledgerBg.image, $imageCache);
  $: mainStyle = buildMainStyle(ledgerBg, ledgerAccent($appSettings.ledger), resolvedBgImage);
  $: menuEntry = entryMenu
    ? book.entries.find((entry) => entry.id === entryMenu?.id) ?? null
    : null;
  $: awayFromToday =
    view === "calendar" &&
    (selectedDate !== today || cursor.year !== thisMonth.year || cursor.month !== thisMonth.month);
  $: showTodayButton = view === "calendar" && awayFromToday;
  /** 记账按钮落在哪一天：日历视图跟着选中的日期，其余视图永远是今天 */
  $: focusDate = view === "calendar" ? selectedDate : today;

  export function closeOverlays(): void {
    showGear = false;
    listMenuAt = null;
    entryMenu = null;
  }

  function closeLedger(): void {
    closeOverlays();
    if ($isMobile) showMobileList();
    else ledgerOpen.set(false);
  }

  function switchView(mode: LedgerViewMode): void {
    if (mode === view) return;
    if (mode === "calendar") cursor = monthOf(selectedDate);
    void setConfig("ledger.view", mode);
  }

  function toggleGear(): void {
    showGear = !showGear;
    listMenuAt = null;
  }

  function openListMenuFromGear(): void {
    const rect = gearButtonEl?.getBoundingClientRect();
    showGear = false;
    if (!rect) return;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
  }

  function changeMonth(next: MonthCursor): void {
    cursor = next;
    // 列表视图的月份栈跟着导航走：导航到哪儿就从哪儿开始往下接，
    // 否则段控/箭头指的月份和栈里渲染的月份会对不上
    if (view === "list") {
      months = [next];
    }
  }

  function pickDay(date: string): void {
    selectedDate = date;
    const cursorOfMonth = monthOf(date);
    if (cursorOfMonth.year !== cursor.year || cursorOfMonth.month !== cursor.month) {
      cursor = cursorOfMonth;
    }
  }

  function jumpToToday(): void {
    selectedDate = today;
    cursor = monthOf(today);
    if (view === "list") months = [monthOf(today)];
  }

  function createEntry(): void {
    ledgerEditor.set({ date: focusDate, kind: "expense" });
  }

  function openEditor(id: string): void {
    entryMenu = null;
    ledgerEditor.set({ id });
  }

  function openCategories(): void {
    closeOverlays();
    showCategories = true;
  }

  function openAccounts(transfer: boolean): void {
    closeOverlays();
    startWithTransfer = transfer;
    accountEditId = "";
    showAccounts = true;
  }

  function openAccountEdit(id: string): void {
    closeOverlays();
    startWithTransfer = false;
    accountEditId = id;
    showAccounts = true;
  }

  /** 列表视图的无限月份：到底接上一个月（往过去），到顶接回下一个月（往现在）。 */
  function handleScroll(): void {
    if (view !== "list" || paging || !scrollEl) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollEl;
    if (scrollHeight - scrollTop - clientHeight < 80) {
      const last = months[months.length - 1];
      paging = true;
      months = [...months, shiftMonth(last, -1)];
      window.setTimeout(() => (paging = false), 120);
    } else if (scrollTop < 60) {
      const first = months[0];
      if (first.year === thisMonth.year && first.month === thisMonth.month) return;
      paging = true;
      months = [shiftMonth(first, 1), ...months];
      window.setTimeout(() => (paging = false), 120);
    }
  }
</script>

<main class="ledger-view" style={mainStyle}>
  <section class="list-header">
    {#if $isMobile}
      <button type="button" class="mobile-back" on:click={closeLedger} aria-label="返回">
        <ArrowLeft size={19} />
      </button>
    {/if}
    <span class="header-icon"><Wallet size={19} /></span>
    <h1>记账</h1>
    <div class="header-actions">
      <div class="ledger-view-switch" role="tablist" aria-label="记账视图">
        {#each VIEWS as item (item.mode)}
          <button
            type="button"
            role="tab"
            aria-selected={view === item.mode}
            class:active={view === item.mode}
            title={item.label}
            on:click={() => switchView(item.mode)}
          >
            <svelte:component this={item.icon} size={16} />
          </button>
        {/each}
      </div>
      <button
        type="button"
        class="header-menu-button"
        title="记账菜单"
        bind:this={gearButtonEl}
        on:click|stopPropagation={toggleGear}
      >
        <SettingsIcon size={17} />
      </button>
    </div>
    {#if showGear}
      <div class="header-menu-panel ledger-gear-panel">
        <MenuItem icon={Tags} label="分类管理" onSelect={openCategories} />
        <MenuItem icon={Wallet} label="账户与转账" onSelect={() => openAccounts(false)} />
        <MenuItem icon={SettingsIcon} label="记账菜单" onSelect={openListMenuFromGear} />
      </div>
    {/if}
  </section>

  {#if view === "list" || view === "calendar"}
    <p class="ledger-subtitle">
      {cursor.year} 年 {cursor.month + 1} 月 · 收 {formatCents(monthTotal.income)} · 支
      {formatCents(monthTotal.expense)} · 结余 {formatCents(monthTotal.income - monthTotal.expense)}
    </p>
  {:else if view === "assets"}
    <p class="ledger-subtitle">
      净资产 {formatCents(assets.net)} · {book.accounts.length} 个账户
    </p>
  {:else}
    <p class="ledger-subtitle">共 {book.entries.length} 笔账</p>
  {/if}

  <section class="ledger-scroll" bind:this={scrollEl} on:scroll={handleScroll}>
    {#if view === "list"}
      <div class="ledger-month-nav">
        <button type="button" on:click={() => changeMonth(shiftMonth(cursor, -1))}>‹</button>
        <strong>{cursor.year} 年 {cursor.month + 1} 月</strong>
        <button type="button" on:click={() => changeMonth(shiftMonth(cursor, 1))}>›</button>
      </div>
      <LedgerList
        {book}
        entries={book.entries}
        {months}
        {today}
        on:edit={(event) => openEditor(event.detail)}
        on:context={(event) => (entryMenu = event.detail)}
      />
    {:else if view === "calendar"}
      <LedgerCalendar
        {book}
        entries={book.entries}
        {cursor}
        {selectedDate}
        {today}
        on:month={(event) => changeMonth(event.detail)}
        on:day={(event) => pickDay(event.detail)}
        on:edit={(event) => openEditor(event.detail)}
        on:context={(event) => (entryMenu = event.detail)}
      />
    {:else if view === "stats"}
      <LedgerStats {book} entries={book.entries} {cursor} />
    {:else}
      <LedgerAssets
        {book}
        on:editAccount={(event) => openAccountEdit(event.detail)}
        on:addAccount={() => openAccounts(false)}
        on:transfer={() => openAccounts(true)}
      />
    {/if}
  </section>

  <div class="ledger-fab-row">
    {#if showTodayButton}
      <button type="button" class="ledger-fab-today" on:click={jumpToToday}>今</button>
    {/if}
    <button type="button" class="ledger-fab" title="记一笔" on:click={createEntry}>
      <Plus size={24} />
    </button>
  </div>

  {#if listMenuAt}
    <ListMenu
      x={listMenuAt.x}
      y={listMenuAt.y}
      xAlign="right"
      ledgerMode
      background={ledgerBg}
      accentColor={ledgerAccent($appSettings.ledger)}
      onClose={() => (listMenuAt = null)}
    />
  {/if}

  {#if menuEntry}
    <LedgerEntryMenu
      x={entryMenu?.x ?? 0}
      y={entryMenu?.y ?? 0}
      entry={menuEntry}
      {book}
      on:edit={(event) => openEditor(event.detail)}
      on:close={() => (entryMenu = null)}
    />
  {/if}

  {#if $ledgerEditor}
    <LedgerEditor target={$ledgerEditor} {book} onClose={() => ledgerEditor.set(null)} />
  {/if}

  {#if showCategories}
    <CategoryManager {book} onClose={() => (showCategories = false)} />
  {/if}

  {#if showAccounts}
    <AccountManager
      {book}
      startWithTransfer={startWithTransfer}
      editId={accountEditId}
      onClose={() => (showAccounts = false)}
    />
  {/if}
</main>

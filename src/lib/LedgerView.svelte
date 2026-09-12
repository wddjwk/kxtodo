<script lang="ts">
  /**
   * 记账整页：四个视图（列表 / 日历 / 统计 / 资产）+ 齿轮面板 + 记一笔 FAB。
   * 与 DiaryView 同一条骨架（头部结构、字号、卡片组织、浮层开合规则都对齐日记）：
   * 桌面 ledger-open 式互斥、移动端历史栈一层；外观走 settings.ledger。
   * 列表视图的组织单位是**天**——一天一张卡片，卡片里是当天每一笔（不折叠）。
   */
  import { onMount } from "svelte";
  import {
    CalendarDays, ChartPie, ChevronLeft, ChevronRight,
    List as ListIcon, MoreHorizontal, Plus, Settings as SettingsIcon, Tags, Wallet
  } from "@lucide/svelte";
  import { appSettings, ledgerData, ledgerEditor } from "./stores";
  import { setConfig } from "./actions";
  import { buildMainStyle, ledgerAccent, ledgerBackground } from "./styles";
  import { imageCache, resolveImageSrc } from "./images";
  import { monthOf, shiftMonth, todayDate, type MonthCursor } from "./diary";
  import { compactCents, monthDayGroups, monthTotals } from "./ledger";
  import MenuItem from "./menu/MenuItem.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import LedgerDayCard from "./ledger/LedgerDayCard.svelte";
  import LedgerCalendar from "./ledger/LedgerCalendar.svelte";
  import LedgerStats from "./ledger/LedgerStats.svelte";
  import LedgerAssets from "./ledger/LedgerAssets.svelte";
  import LedgerEntryMenu from "./ledger/LedgerEntryMenu.svelte";
  import CategoryManager from "./ledger/CategoryManager.svelte";
  import AccountManager from "./ledger/AccountManager.svelte";
  import type { LedgerViewMode } from "./types";

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
  let accountStart: "list" | "transfer" | "add" = "list";
  let accountEditId = "";
  let scrollEl: HTMLElement;
  let paging = false;

  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
  /** 列表视图已加载的月份栈（新→旧）：滚到底接上一个月，滚到顶接回下一个月 */
  let months: MonthCursor[] = [monthOf(todayDate())];
  // 分钟级 tick：记账页常常一直开着，跨天后「今天」必须自己跟上（与日记同一套路）
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
  $: monthLabel = `${cursor.year}年${cursor.month + 1}月`;
  $: ledgerBg = ledgerBackground($appSettings.ledger);
  $: resolvedBgImage = resolveImageSrc(ledgerBg.image, $imageCache);
  $: mainStyle = buildMainStyle(ledgerBg, ledgerAccent($appSettings.ledger), resolvedBgImage);
  $: menuEntry = entryMenu ? book.entries.find((entry) => entry.id === entryMenu?.id) ?? null : null;

  /** 列表视图的月份分区：每段带上自己的按天分组与合计（滚进来的旧月份要有自己的段头） */
  $: sections = months.map((month, index) => ({
    key: `${month.year}-${month.month}`,
    month,
    index,
    groups: monthDayGroups(book.entries, month),
    totals: monthTotals(book.entries, month)
  }));

  /** 记账按钮落在哪一天：日历视图跟着选中的日期，其余视图永远是今天 */
  $: focusDate = view === "calendar" ? selectedDate : today;
  $: awayFromToday =
    view === "calendar"
      ? selectedDate !== today || cursor.year !== thisMonth.year || cursor.month !== thisMonth.month
      : months[0].year !== thisMonth.year || months[0].month !== thisMonth.month;
  $: showTodayButton = awayFromToday;

  export function closeOverlays(): void {
    showGear = false;
    listMenuAt = null;
    entryMenu = null;
  }

  function switchView(mode: LedgerViewMode): void {
    closeOverlays();
    if (mode === view) return;
    if (mode === "calendar") cursor = monthOf(selectedDate);
    if (mode === "list") months = [cursor];
    void setConfig("ledger.view", mode);
  }

  /** 齿轮面板与其它头部浮层互斥（与日记/工作区同一套开合规则）。 */
  function toggleGear(): void {
    showGear = !showGear;
    listMenuAt = null;
    entryMenu = null;
  }

  /** 齿轮面板 → 记账菜单：锚在齿轮按钮右下角（视口像素，ContextMenu 内部除以缩放）。 */
  function openListMenuFromGear(): void {
    showGear = false;
    const rect = gearButtonEl?.getBoundingClientRect();
    if (!rect) return;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    entryMenu = null;
  }

  function handlePanelKeydown(event: KeyboardEvent): void {
    if (!showGear) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) showGear = false;
  }

  function changeMonth(next: MonthCursor): void {
    cursor = next;
    selectedDate = `${next.year}-${(next.month + 1).toString().padStart(2, "0")}-01`;
    if (view === "list") months = [next];
  }

  function pickDay(date: string): void {
    selectedDate = date;
    const month = monthOf(date);
    if (month.year !== cursor.year || month.month !== cursor.month) cursor = month;
    entryMenu = null;
  }

  function jumpToToday(): void {
    selectedDate = today;
    cursor = monthOf(today);
    if (view === "list") months = [monthOf(today)];
  }

  function createEntry(date = focusDate): void {
    entryMenu = null;
    ledgerEditor.set({ date, kind: "expense" });
  }

  function openEditor(id: string): void {
    entryMenu = null;
    ledgerEditor.set({ id });
  }

  function openCategories(): void {
    closeOverlays();
    showCategories = true;
  }

  function openAccounts(mode: "list" | "transfer" | "add"): void {
    closeOverlays();
    accountStart = mode;
    accountEditId = "";
    showAccounts = true;
  }

  function openAccountEdit(id: string): void {
    closeOverlays();
    accountStart = "list";
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
      const next = shiftMonth(first, 1);
      months = [next, ...months];
      cursor = next;
      window.setTimeout(() => (paging = false), 120);
    }
  }
</script>

<svelte:window on:keydown={handlePanelKeydown} />

<main class="ledger-view" style={mainStyle}>
  <section class="list-header">
    <div>
      <span class="header-icon"><Wallet size={34} /></span>
      <h1>记账</h1>
    </div>
    <div class="header-actions" on:click|stopPropagation>
      <div class="ledger-view-switch" role="tablist" aria-label="记账视图">
        {#each VIEWS as item (item.mode)}
          <button
            type="button"
            role="tab"
            title={item.label}
            aria-label={item.label}
            aria-selected={view === item.mode}
            class:active={view === item.mode}
            on:click|stopPropagation={() => switchView(item.mode)}
          ><svelte:component this={item.icon} size={18} /></button>
        {/each}
      </div>
      <button
        bind:this={gearButtonEl}
        type="button"
        title="更多操作"
        aria-label="更多操作"
        aria-expanded={showGear}
        on:click|stopPropagation={toggleGear}
      ><SettingsIcon size={21} /></button>

      {#if showGear}
        <div class="header-menu-panel ledger-gear-panel" role="menu" tabindex="-1">
          <MenuItem icon={Tags} label="分类管理" onSelect={openCategories} />
          <MenuItem icon={Wallet} label="账户与转账" onSelect={() => openAccounts("list")} />
          <MenuItem icon={MoreHorizontal} label="记账菜单" onSelect={openListMenuFromGear} />
        </div>
      {/if}
    </div>
  </section>

  {#if view === "list"}
    <div class="ledger-month-bar">
      <span class="ledger-month-step">
        <button type="button" aria-label="上个月" on:click|stopPropagation={() => changeMonth(shiftMonth(cursor, -1))}>
          <ChevronLeft size={18} />
        </button>
        <strong>{monthLabel}</strong>
        <button type="button" aria-label="下个月" on:click|stopPropagation={() => changeMonth(shiftMonth(cursor, 1))}>
          <ChevronRight size={18} />
        </button>
      </span>
      <span class="ledger-month-sums">
        <em class="in">收 {compactCents(monthTotal.income)}</em>
        <em class="out">支 {compactCents(monthTotal.expense)}</em>
        <em class="net">结余 {compactCents(monthTotal.income - monthTotal.expense)}</em>
      </span>
    </div>
  {/if}

  <section class="ledger-scroll" bind:this={scrollEl} on:scroll={handleScroll}>
    {#if view === "list"}
      {#if book.entries.length === 0}
        <div class="empty-state">
          <strong>还没有记账</strong>
          <span>点右下角的 + 记下第一笔。</span>
        </div>
      {:else}
        {#each sections as section (section.key)}
          {#if section.index > 0}
            <div class="ledger-month-divider">
              <span>{section.month.year}年{section.month.month + 1}月</span>
              <em>收 {compactCents(section.totals.income)} · 支 {compactCents(section.totals.expense)}</em>
            </div>
          {/if}
          {#each section.groups as group (group.date)}
            <LedgerDayCard
              {book}
              {group}
              {today}
              selectedId={entryMenu?.id ?? ""}
              on:edit={(event) => openEditor(event.detail)}
              on:add={(event) => createEntry(event.detail)}
              on:context={(event) => {
                entryMenu = event.detail;
                showGear = false;
                listMenuAt = null;
              }}
            />
          {:else}
            {#if section.index === 0}
              <div class="ledger-day-empty">{monthLabel}还没有记账。</div>
            {/if}
          {/each}
        {/each}
      {/if}

    {:else if view === "calendar"}
      <LedgerCalendar
        {book}
        {cursor}
        {selectedDate}
        {today}
        selectedId={entryMenu?.id ?? ""}
        on:month={(event) => changeMonth(event.detail)}
        on:day={(event) => pickDay(event.detail)}
        on:edit={(event) => openEditor(event.detail)}
        on:add={(event) => createEntry(event.detail)}
        on:context={(event) => (entryMenu = event.detail)}
      />

    {:else if view === "stats"}
      <LedgerStats {book} entries={book.entries} {cursor} on:month={(event) => changeMonth(event.detail)} />

    {:else}
      <LedgerAssets
        {book}
        on:editAccount={(event) => openAccountEdit(event.detail)}
        on:addAccount={() => openAccounts("add")}
        on:transfer={() => openAccounts("transfer")}
      />
    {/if}
  </section>

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

  {#if entryMenu && menuEntry}
    <LedgerEntryMenu
      x={entryMenu.x}
      y={entryMenu.y}
      entry={menuEntry}
      {book}
      on:edit={(event) => openEditor(event.detail)}
      on:close={() => (entryMenu = null)}
    />
  {/if}

  {#if showCategories}
    <CategoryManager {book} onClose={() => (showCategories = false)} />
  {/if}

  {#if showAccounts}
    <AccountManager
      {book}
      startWithTransfer={accountStart === "transfer"}
      startWithAdd={accountStart === "add"}
      editId={accountEditId}
      onClose={() => (showAccounts = false)}
    />
  {/if}

  <div class="ledger-fab-row">
    {#if showTodayButton}
      <button class="ledger-fab-today" type="button" title="回到本月" on:click|stopPropagation={jumpToToday}>今</button>
    {/if}
    <button class="ledger-fab" type="button" title="记一笔" aria-label="记一笔" on:click|stopPropagation={() => createEntry()}>
      <Plus size={24} />
    </button>
  </div>
</main>

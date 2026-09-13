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
  import { appSettings, ledgerData, ledgerEditor, ledgerCategoryDraft } from "./stores";
  import { setConfig } from "./actions";
  import { buildMainStyle, ledgerAccent, ledgerBackground } from "./styles";
  import { imageCache, resolveImageSrc } from "./images";
  import { monthOf, shiftMonth, todayDate, type MonthCursor } from "./diary";
  import { compactCents, monthDayGroups, monthTotals, LEDGER_IMAGE_NODE } from "./ledger";
  import { mdImageUrl } from "./backend";
  import MenuItem from "./menu/MenuItem.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import LedgerDayCard from "./ledger/LedgerDayCard.svelte";
  import LedgerCalendar from "./ledger/LedgerCalendar.svelte";
  import LedgerStats from "./ledger/LedgerStats.svelte";
  import LedgerAssets from "./ledger/LedgerAssets.svelte";
  import LedgerEntryMenu from "./ledger/LedgerEntryMenu.svelte";
  import CategoryDrilldown from "./ledger/CategoryDrilldown.svelte";
  import CategoryManager from "./ledger/CategoryManager.svelte";
  import AccountManager from "./ledger/AccountManager.svelte";
  import LedgerImagePreview from "./ledger/LedgerImagePreview.svelte";
  import type { LedgerSide, LedgerViewMode } from "./types";

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
  /** 滚到边缘后是否允许再换月：换完一次先收掉，等滚回中间再武装，
   *  否则停在顶/底时每个 scroll 事件都会再翻一个月（一路翻到尽头）。 */
  let edgeArmed = true;
  /** 统计里点了某个大类：钻取面板（移动端下半屏、桌面端锚在那一行下方） */
  let drill: {
    categoryId: string;
    side: LedgerSide;
    from: string;
    to: string;
    periodLabel: string;
    anchor: HTMLElement;
  } | null = null;
  let monthPopOpen = false;
  let monthLabelEl: HTMLElement;
  /** 从记账面板的加号过来时，分类管理直接停在新增表单上（可带预置大类） */
  let categoryStart: { side: LedgerSide; parentId: string } | null = null;
  /** 条目插图的全屏查看（点卡片小字行里的图片图标） */
  let preview: { src: string; title: string } | null = null;

  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
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

  /** 列表视图只展示 cursor 一个月：滚到底整屏换成上一个月，滚到顶下拉换回下一个月 */
  $: sections = [{ key: `${cursor.year}-${cursor.month}`, groups: monthDayGroups(book.entries, cursor) }];

  /** 记账按钮落在哪一天：日历视图跟着选中的日期，其余视图永远是今天 */
  $: focusDate = view === "calendar" ? selectedDate : today;
  $: awayFromToday =
    view === "calendar"
      ? selectedDate !== today || cursor.year !== thisMonth.year || cursor.month !== thisMonth.month
      : cursor.year !== thisMonth.year || cursor.month !== thisMonth.month;
  $: showTodayButton = awayFromToday;

  export function closeOverlays(): void {
    showGear = false;
    listMenuAt = null;
    entryMenu = null;
    drill = null;
    monthPopOpen = false;
    preview = null;
  }

  /** 记账面板里的加号：面板挂在 App 层，只能靠 store 把「要加分类」递到这一页来。 */
  $: if ($ledgerCategoryDraft) {
    categoryStart = $ledgerCategoryDraft;
    ledgerCategoryDraft.set(null);
    showGear = false;
    listMenuAt = null;
    entryMenu = null;
    drill = null;
    monthPopOpen = false;
    showCategories = true;
  }

  function switchView(mode: LedgerViewMode): void {
    closeOverlays();
    if (mode === view) return;
    if (mode === "calendar") cursor = monthOf(selectedDate);
    if (mode === "list") resetScroll();
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

  function resetScroll(): void {
    edgeArmed = true;
    paging = false;
    if (scrollEl) scrollEl.scrollTop = 0;
  }

  function changeMonth(next: MonthCursor): void {
    cursor = next;
    selectedDate = `${next.year}-${(next.month + 1).toString().padStart(2, "0")}-01`;
    if (view === "list") resetScroll();
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
    resetScroll();
  }

  function createEntry(date = focusDate): void {
    entryMenu = null;
    ledgerEditor.set({ date, kind: "expense" });
  }

  function openEditor(id: string): void {
    entryMenu = null;
    ledgerEditor.set({ id });
  }

  /** 点条目小字行的图片图标：全屏看这条账的插图（图走 markdown 插图的 ledger 伪条目通道）。 */
  async function openEntryImage(id: string): Promise<void> {
    const entry = book.entries.find((item) => item.id === id);
    if (!entry?.image) return;
    try {
      const src = await mdImageUrl(LEDGER_IMAGE_NODE, entry.image);
      preview = { src, title: entry.note || entry.date };
    } catch {
      preview = null;
    }
  }

  function openCategories(): void {
    closeOverlays();
    categoryStart = null;
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

  /** 列表视图的换月：滚到底 = 整屏换成上一个月（时间近的在上面），滚到顶下拉 = 换回下一个月。
   *  向上以当前真实月为顶——再新就是还没发生的月份，翻过去只有空屏。 */
  function handleScroll(): void {
    if (view !== "list" || paging || !scrollEl) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollEl;
    if (!edgeArmed) {
      if (scrollTop > 120 && scrollHeight - scrollTop - clientHeight > 120) edgeArmed = true;
      return;
    }
    if (scrollHeight - scrollTop - clientHeight < 60) {
      paging = true;
      edgeArmed = false;
      cursor = shiftMonth(cursor, -1);
      window.setTimeout(() => {
        paging = false;
        if (scrollEl) scrollEl.scrollTop = 0;
      }, 60);
    } else if (scrollTop < 60) {
      if (cursor.year === thisMonth.year && cursor.month === thisMonth.month) return;
      paging = true;
      edgeArmed = false;
      cursor = shiftMonth(cursor, 1);
      window.setTimeout(() => {
        paging = false;
        if (scrollEl) scrollEl.scrollTop = 0;
      }, 60);
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
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
        <strong
          bind:this={monthLabelEl}
          class="month-pop-anchor"
          role="button"
          tabindex="0"
          title="点击直接选年月"
          on:click|stopPropagation={() => (monthPopOpen = !monthPopOpen)}
        >{monthLabel}</strong>
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

    <MonthPopover
      open={monthPopOpen}
      anchor={monthLabelEl}
      year={cursor.year}
      month={cursor.month}
      onSelect={(next) => changeMonth({ year: next.year, month: next.month })}
      onClose={() => (monthPopOpen = false)}
    />
  {/if}

  <section class="ledger-scroll" bind:this={scrollEl} on:scroll={handleScroll}>
    {#if view === "list"}
      {#if book.entries.length === 0}
        <div class="empty-state">
          <strong>还没有记账</strong>
          <span>点右下角的 + 记下第一笔。</span>
        </div>
      {:else}
        {#each sections[0].groups as group (group.date)}
          <LedgerDayCard
            {book}
            {group}
            {today}
            selectedId={entryMenu?.id ?? ""}
            on:edit={(event) => openEditor(event.detail)}
            on:add={(event) => createEntry(event.detail)}
            on:image={(event) => void openEntryImage(event.detail)}
            on:context={(event) => {
              entryMenu = event.detail;
              showGear = false;
              listMenuAt = null;
            }}
          />
        {:else}
          <div class="ledger-day-empty">{monthLabel}还没有记账。</div>
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
        on:image={(event) => void openEntryImage(event.detail)}
        on:context={(event) => (entryMenu = event.detail)}
      />

    {:else if view === "stats"}
      <LedgerStats
        {book}
        entries={book.entries}
        {cursor}
        on:month={(event) => changeMonth(event.detail)}
        on:drill={(event) => {
          drill = event.detail;
          showGear = false;
          entryMenu = null;
        }}
      />

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
      on:image={(event) => void openEntryImage(event.detail)}
      on:close={() => (entryMenu = null)}
    />
  {/if}

  {#if drill}
    <CategoryDrilldown
      {book}
      entries={book.entries}
      categoryId={drill.categoryId}
      side={drill.side}
      from={drill.from}
      to={drill.to}
      periodLabel={drill.periodLabel}
      anchor={drill.anchor}
      onClose={() => (drill = null)}
      onEditEntry={(id) => {
        drill = null;
        openEditor(id);
      }}
      onImageView={(id) => {
        drill = null;
        void openEntryImage(id);
      }}
    />
  {/if}

  {#if preview}
    <LedgerImagePreview src={preview.src} title={preview.title} onClose={() => (preview = null)} />
  {/if}

  {#if showCategories}
    <CategoryManager
      {book}
      side={categoryStart?.side ?? "expense"}
      startWithAdd={Boolean(categoryStart)}
      startSide={categoryStart?.side ?? ""}
      startParentId={categoryStart?.parentId ?? ""}
      onClose={() => {
        showCategories = false;
        categoryStart = null;
      }}
    />
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

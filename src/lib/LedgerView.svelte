<script lang="ts">
  /**
   * 记账整页：四个视图（列表 / 日历 / 统计 / 资产）+ 齿轮面板 + 记一笔 FAB。
   * 与 DiaryView 同一条骨架（头部结构、字号、卡片组织、浮层开合规则都对齐日记）：
   * 桌面 ledger-open 式互斥、移动端历史栈一层；外观走 settings.ledger。
   * 列表视图的组织单位是**天**——一天一张卡片，卡片里是当天每一笔（不折叠）。
   */
  import { onMount, tick } from "svelte";
  import {
    CalendarDays, ChartPie, ChevronLeft, ChevronRight,
    List as ListIcon, MoreHorizontal, Plus, Search, Settings as SettingsIcon, Tags, Wallet, X
  } from "@lucide/svelte";
  import { appSettings, ledgerData, ledgerEditor, ledgerCategoryDraft } from "./stores";
  import { setConfig } from "./actions";
  import { buildMainStyle, ledgerAccent, ledgerBackground } from "./styles";
  import { imageCache, resolveImageSrc } from "./images";
  import { monthOf, shiftMonth, todayDate, type MonthCursor } from "./diary";
  import {
    compactCents, entriesTotals, filterLedgerEntries, monthDayGroups, monthTotals,
    assetsTrend, LEDGER_IMAGE_NODE
  } from "./ledger";
  import type { AssetTrendPoint } from "./ledger";
  import { mdImageUrl } from "./backend";
  import MenuItem from "./menu/MenuItem.svelte";
  import MobileBack from "./MobileBack.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import LedgerDayCard from "./ledger/LedgerDayCard.svelte";
  import LedgerEntryCard from "./ledger/LedgerEntryCard.svelte";
  import LedgerCalendar from "./ledger/LedgerCalendar.svelte";
  import LedgerStats from "./ledger/LedgerStats.svelte";
  import LedgerAssets from "./ledger/LedgerAssets.svelte";
  import LedgerEntryMenu from "./ledger/LedgerEntryMenu.svelte";
  import CategoryDrilldown from "./ledger/CategoryDrilldown.svelte";
  import CategoryManager from "./ledger/CategoryManager.svelte";
  import AccountManager from "./ledger/AccountManager.svelte";
  import LedgerImagePreview from "./ledger/LedgerImagePreview.svelte";
  import AssetsTrendViewer from "./ledger/AssetsTrendViewer.svelte";
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
  /** 条目插图的全屏查看（点卡片小字行里的图片图标）；一条账可以有多张 */
  let preview: { src: string; title: string }[] | null = null;
  /** 总资产趋势的放大查看：viewer 必须挂在本层——挂在 .ledger-scroll 里的话，
   *  滚动区自己是个 z-index:1 的 stacking context，4600 的全屏层会被 FAB 盖住 */
  let trendView: AssetTrendPoint[] | null = null;

  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
  /** 搜索记账：齿轮面板里的「搜索记账」开关那一条输入框；有查询词时整页让给结果 */
  let searchOpen = false;
  let searchInput: HTMLInputElement;
  let query = "";
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

  /** 搜索态：有查询词时整页换成「单条卡片 + 收支结余汇总」 */
  $: searching = searchOpen && query.trim().length > 0;
  $: searchResults = searching ? filterLedgerEntries(book, query) : [];
  $: searchTotals = searching ? entriesTotals(searchResults) : { income: 0, expense: 0 };

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
    trendView = null;
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

  /** 搜索输入框的开合（齿轮面板里那一条，与日记同一套）；收起时清词回到正常视图 */
  function toggleSearch(): void {
    searchOpen = !searchOpen;
    showGear = false;
    entryMenu = null;
    listMenuAt = null;
    if (!searchOpen) query = "";
    void tick().then(() => searchInput?.focus());
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

  /**
   * 点条目小字行的图片图标：全屏看这条账的插图（图走 markdown 插图的 ledger 伪条目通道）。
   * 逐张独立解析（allSettled）：缺一张图不该把整个查看器弄没——只展示解析得出来的那些。
   */
  async function openEntryImage(id: string): Promise<void> {
    const entry = book.entries.find((item) => item.id === id);
    const names = entry?.images ?? [];
    if (names.length === 0) return;
    const title = entry?.note || entry?.date || "";
    try {
      const settled = await Promise.allSettled(
        names.map((name) => mdImageUrl(LEDGER_IMAGE_NODE, name))
      );
      const resolved = settled.flatMap((result) =>
        result.status === "fulfilled" && result.value ? [{ src: result.value, title }] : []
      );
      preview = resolved.length > 0 ? resolved : null;
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

  /** 列表视图的换月：滚到底 = 整屏换成上一个月（时间近的在上面），滚到顶 = 换回下一个月。
   *  向上以当前真实月为顶——再新就是还没发生的月份，翻过去只有空屏。
   *
   *  **换月有冷却**：一次连续手势只翻一个月（滚轮的惯性事件、触摸拖动在换月后还停在
   *  边缘，不冷却会一路翻下去）。冷却只管**手势路径**——滚到底那条路径有自己的
   *  「先滚回中间再武装」（edgeArmed）节流，再叠一层冷却会把正常的一次前翻也吞掉。 */
  let edgeCooldownUntil = 0;
  const EDGE_COOLDOWN_MS = 420;

  /** `dir`：-1 = 更早的月份，1 = 更新的月份。返回是否真的换了。 */
  function switchMonthBy(dir: -1 | 1): boolean {
    if (paging) return false;
    if (dir === 1 && cursor.year === thisMonth.year && cursor.month === thisMonth.month) return false;
    paging = true;
    edgeArmed = false;
    edgeCooldownUntil = Date.now() + EDGE_COOLDOWN_MS;
    cursor = shiftMonth(cursor, dir);
    window.setTimeout(() => {
      paging = false;
      if (scrollEl) scrollEl.scrollTop = 0;
    }, 60);
    return true;
  }

  function handleScroll(): void {
    if (view !== "list" || searching || paging || !scrollEl) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollEl;
    if (!edgeArmed) {
      if (scrollTop > 120 && scrollHeight - scrollTop - clientHeight > 120) edgeArmed = true;
      return;
    }
    if (scrollHeight - scrollTop - clientHeight < 60) {
      switchMonthBy(-1);
    } else if (scrollTop < 60) {
      switchMonthBy(1);
    }
  }

  /**
   * 边缘手势换月（滚轮 / 触摸拖动）。
   *
   * **光靠 scroll 事件不够**：容器已经停在边缘时再往下/上拉根本不会触发 scroll，
   * 而刚换过月、或这一整个月的内容还没铺满一屏（连一个滚动条都没有）时，用户正
   * 处在「想看上一个/下一个月」的那个边缘上——从前的表现就是「往上滑不换月」。
   * 这里直接读手势意图：贴顶且手势朝上 = 换到更近的月，贴底且手势朝下 = 换到更早的月。
   */
  const EDGE_SWIPE_PX = 42;

  function edgeIntent(dy: number): void {
    if (view !== "list" || searching || !scrollEl) return;
    if (Date.now() < edgeCooldownUntil) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollEl;
    if (dy > EDGE_SWIPE_PX) {
      if (scrollTop <= 2) switchMonthBy(1);
      return;
    }
    if (dy < -EDGE_SWIPE_PX && scrollHeight - scrollTop - clientHeight <= 2) {
      switchMonthBy(-1);
    }
  }

  function handleWheel(event: WheelEvent): void {
    if (event.deltaY === 0) return;
    edgeIntent(-event.deltaY);
  }

  let touchStartY = 0;

  function handleTouchStart(event: TouchEvent): void {
    touchStartY = event.touches[0]?.clientY ?? 0;
  }

  function handleTouchMove(event: TouchEvent): void {
    const touch = event.touches[0];
    if (!touch) return;
    const dy = touch.clientY - touchStartY;
    if (Math.abs(dy) < EDGE_SWIPE_PX) return;
    const before = `${cursor.year}-${cursor.month}`;
    edgeIntent(dy);
    // 真换了月就把手势锚点挪到当前触点：同一次长拖不再被算作第二次意图
    if (`${cursor.year}-${cursor.month}` !== before) touchStartY = touch.clientY;
  }
</script>

<svelte:window on:keydown={handlePanelKeydown} />

<main class="ledger-view" style={mainStyle}>
  <section class="list-header">
    <div>
      <MobileBack />
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
          <MenuItem icon={searchOpen ? X : Search} label={searchOpen ? "关闭搜索" : "搜索记账"} onSelect={toggleSearch} />
          <MenuItem icon={Tags} label="分类管理" onSelect={openCategories} />
          <MenuItem icon={Wallet} label="账户与转账" onSelect={() => openAccounts("list")} />
          <MenuItem icon={MoreHorizontal} label="记账菜单" onSelect={openListMenuFromGear} />
        </div>
      {/if}
    </div>
  </section>

  {#if searchOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <label class="ledger-search" on:click|stopPropagation>
      <Search size={16} />
      <input bind:this={searchInput} bind:value={query} type="text" placeholder="搜索分类、备注或金额" />
    </label>
  {/if}

  {#if searching}
    <!-- 搜索结果的收/支/结余：转账不计入，与统计口径一致 -->
    <div class="ledger-search-sum">
      <span><strong>收</strong>{compactCents(searchTotals.income)}</span>
      <span><strong>支</strong>{compactCents(searchTotals.expense)}</span>
      <span><strong>结余</strong>{compactCents(searchTotals.income - searchTotals.expense)}</span>
      <em>{searchResults.length} 笔</em>
    </div>
  {/if}

  {#if view === "list" && !searching}
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

  <section
    class="ledger-scroll"
    bind:this={scrollEl}
    on:scroll={handleScroll}
    on:wheel={handleWheel}
    on:touchstart={handleTouchStart}
    on:touchmove={handleTouchMove}
  >
    {#if searching}
      <!-- 搜索结果：每一条一张单笔卡片（不按天成卡，也不带当天的收/支） -->
      {#each searchResults as entry (entry.id)}
        <LedgerEntryCard
          {book}
          {entry}
          selected={entryMenu?.id === entry.id}
          on:edit={(event) => openEditor(event.detail)}
          on:image={(event) => void openEntryImage(event.detail)}
          on:context={(event) => {
            entryMenu = event.detail;
            showGear = false;
            listMenuAt = null;
          }}
        />
      {:else}
        <div class="ledger-day-empty">没有匹配「{query.trim()}」的账。</div>
      {/each}
    {:else if view === "list"}
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
        on:openTrend={() => (trendView = assetsTrend(book))}
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
    <LedgerImagePreview items={preview} onClose={() => (preview = null)} />
  {/if}

  {#if trendView}
    <AssetsTrendViewer points={trendView} onClose={() => (trendView = null)} />
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
    <button
      class="ledger-fab"
      type="button"
      title={view === "assets" ? "添加账户" : "记一笔"}
      aria-label={view === "assets" ? "添加账户" : "记一笔"}
      on:click|stopPropagation={() => (view === "assets" ? openAccounts("add") : createEntry())}
    >
      <Plus size={24} />
    </button>
  </div>
</main>

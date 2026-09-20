<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    CalendarDays, ChevronDown, ChevronLeft, ChevronRight, Flame, FolderTree,
    List as ListIcon, NotebookPen, Plus, Search,
    Settings as SettingsIcon, X
  } from "@lucide/svelte";
  import { appSettings, diaryEditor, diaryEntries, weekStart } from "./stores";
  import { createBackGuard } from "./platform";
  import { setConfig, setDiaryUi } from "./actions";
  import { buildMainStyle, diaryAccent, diaryBackground } from "./styles";
  import { accentWithPreview, backgroundWithPreview, colorPreview } from "./colorPreview";
  import { imageCache, resolveImageSrc } from "./images";
  import {
    calendarCells, calendarWeekdayHeaders, diaryByDate, diaryStats, filterDiaries,
    fullDayLabel, monthOf, relativeDayLabel, shiftMonth, todayDate,
    yearGroups, type MonthCursor
  } from "./diary";
  import VirtualStack from "./VirtualStack.svelte";
  import DiaryCard from "./diary/DiaryCard.svelte";
  import DiaryEntryMenu from "./diary/DiaryEntryMenu.svelte";
  import { swipeX } from "./swipe";
  import MobileBack from "./MobileBack.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import type { DiaryEntry, DiaryViewMode } from "./types";

  /** 链接打开走 Workspace 那一条（应用内预览 / 系统浏览器由设置决定），不重复实现。 */
  export let onOpenLink: (url: string, title?: string) => void = () => {};

  const VIEWS: Array<{ mode: DiaryViewMode; label: string; icon: typeof ListIcon }> = [
    { mode: "list", label: "列表视图", icon: ListIcon },
    { mode: "calendar", label: "日历视图", icon: CalendarDays },
    { mode: "group", label: "分组视图", icon: FolderTree }
  ];

  let searchOpen = false;
  /** 列表视图的滚动容器（窗口化要用它当视口） */
  let scrollEl: HTMLElement | null = null;
  /** 列表视图的窗口化宿主（日历跳某天 / 回顶时要拿着它调 scrollToIndex / syncScroll） */
  let stack: VirtualStack | null = null;
  let searchInput: HTMLInputElement;
  let query = "";
  let showGear = false;
  let gearButtonEl: HTMLButtonElement;
  let listMenuAt: { x: number; y: number } | null = null;
  let entryMenu: { id: string; x: number; y: number } | null = null;
  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
  /** 用户是否在日历里显式点过某天（决定切回列表是定位过去还是回顶） */
  let calendarPicked = false;
  let monthPopOpen = false;
  let monthLabelEl: HTMLElement;
  /** 分组视图的年/月折叠状态（本机 UI 状态，不持久化） */
  /** 分组视图：记「展开」而不是「折叠」（v0.8.4 需求 4）——默认全折叠，
   *  点开哪一组才渲染哪一组的条目，5000 篇也不会一次全挂上去。 */
  let expanded: Record<string, boolean> = {};
  // 分钟级 tick：日记页面常常一直开着，跨天后「今天」必须自己跟上，
  // 否则添加按钮会把新的一天写进昨天、连续天数也算错（与 Workspace 同一套路）
  let dayTick = 0;

  onMount(() => {
    const timer = window.setInterval(() => {
      dayTick += 1;
    }, 60_000);
    return () => window.clearInterval(timer);
  });

  $: today = dayTick >= 0 ? todayDate() : "";
  $: view = $appSettings.diary.view;
  $: entries = filterDiaries($diaryEntries, query);
  // 统计永远基于全部日记：搜索时「共几篇/连续几天」不该跟着筛选结果变
  $: stats = diaryStats($diaryEntries, today);
  $: byDate = diaryByDate(entries);
  $: years = yearGroups(entries);
  $: cells = calendarCells(cursor, byDate, $weekStart);
  $: monthLabel = `${cursor.year}年${cursor.month + 1}月`;
  /** 添加按钮落在哪一天：日历视图跟着选中的日期，其余视图永远是今天 */
  $: focusDate = view === "calendar" ? selectedDate : today;
  $: thisMonth = monthOf(today);
  /**
   * 「今」按钮不只看选中的那一天：日历翻到别的月份时选中日可能还是今天，
   * 但画面已经不在今天这一屏了，同样需要一个回家的入口。
   */
  $: awayFromToday =
    view === "calendar" &&
    (selectedDate !== today || cursor.year !== thisMonth.year || cursor.month !== thisMonth.month);
  $: showTodayButton = focusDate !== today || awayFromToday;
  $: dayEntries = byDate.get(selectedDate) ?? [];
  $: menuEntry = entryMenu
    ? $diaryEntries.find((entry) => entry.id === entryMenu?.id) ?? null
    : null;
  // 日记自己的主题色与背景（settings.diary）：背景图与列表背景走同一套解析与缓存
  $: diaryBg = diaryBackground($appSettings.diary);
  $: resolvedBgImage = resolveImageSrc(diaryBg.image, $imageCache);
  // 取色预览（需求 9）：菜单里拖色盘时先把界面染上，点保存才落盘
  $: mainStyle = buildMainStyle(
    backgroundWithPreview($colorPreview, "diary", diaryBg),
    accentWithPreview($colorPreview, "diary", diaryAccent($appSettings.diary)),
    resolvedBgImage
  );

  // 列表视图 = 年份分隔行 + 卡片。直接摊平分组视图算好的年→月→天，两边顺序天然一致。
  type DiaryListRow = {
    kind: "year" | "entry";
    key: string;
    label: string;
    count: number;
    entry: DiaryEntry | null;
  };

  $: listRows = years.flatMap((year): DiaryListRow[] => [
    { kind: "year", key: `year-${year.key}`, label: year.label, count: year.count, entry: null },
    ...year.months.flatMap((month) =>
      month.days.flatMap((day) =>
        day.entries.map((entry): DiaryListRow => ({
          kind: "entry",
          key: entry.id,
          label: "",
          count: 0,
          entry: entry as DiaryEntry | null
        }))
      )
    )
  ]);

  // 月份浮层是这一页的浮层：返回键先收它（v0.8.4 起齿轮直接弹菜单，没有中间面板）
  const backGuard = createBackGuard();
  $: backGuard(monthPopOpen, () => {
    monthPopOpen = false;
  });

  export function closeOverlays(): void {
    entryMenu = null;
    listMenuAt = null;
    monthPopOpen = false;
  }

  /** 切视图一律复位滚动（v0.8.2 铁律）：列表内容整批换掉，留在原偏移只会看见中部 */
  function resetScroll(): void {
    if (scrollEl) scrollEl.scrollTop = 0;
    // 立刻刷新窗口自己的锚点，否则 afterUpdate 的锚点还原会把复位顶回去
    stack?.syncScroll();
  }

  function switchView(mode: DiaryViewMode): void {
    entryMenu = null;
    if (mode === view) return;
    const fromCalendar = view === "calendar";
    if (mode === "calendar") {
      cursor = monthOf(selectedDate);
    }
    void setConfig("diary.view", mode);
    if (mode === "list" && fromCalendar) {
      // 日历上**点过**的那一天：切到列表时定位到它的第一篇（需求 9 的锚点）。
      // 只是进日历看了一眼（没点任何格子）就不跳——那种情况按 ① 回顶。
      if (!calendarPicked) {
        resetScroll();
        void tick().then(resetScroll);
        return;
      }
      calendarPicked = false;
      // 等 tick：列表的 VirtualStack 要等视图换过去才挂载。
      void tick().then(() => {
        if (!stack) return;
        const index = listRows.findIndex((row) => row.entry?.date === selectedDate);
        if (index >= 0) stack.scrollToIndex(index);
      });
      return;
    }
    resetScroll();
    // 换到列表视图时 stack 是这一刻才挂载的：等它出现再同步一次锚点
    void tick().then(resetScroll);
  }

  /** 搜索按钮（v0.8.4 需求 14）：从齿轮面板里挪到头部，一次点击直接开/关搜索框 */
  function toggleSearch(): void {
    searchOpen = !searchOpen;
    listMenuAt = null;
    entryMenu = null;
    if (!searchOpen) query = "";
    void tick().then(() => {
      searchInput?.focus();
      resetScroll();
    });
  }

  /** 搜索条件一变，可见项整批换掉：回顶，让第一条命中在眼前（需求 9） */
  function handleQueryInput(): void {
    resetScroll();
  }

  /**
   * 齿轮按钮直接弹「日记菜单」（需求 14）：省掉中间那层只有两项的面板。
   * v0.8.6 需求 8 两件一起做：**已开着再点 = 关闭**；并且把齿轮按钮作为 `anchor`
   * 传给 ListMenu→ContextMenu——ContextMenu 的 capture 阶段 pointerdown 会先关掉
   * 「点菜单外」的菜单，只有 anchor 豁免；不传的话 pointerdown 先关、click 再开，
   * 看起来就是「点一下闪一下、永远关不掉」。
   */
  function openListMenuFromGear(): void {
    if (listMenuAt) {
      listMenuAt = null;
      return;
    }
    const rect = gearButtonEl?.getBoundingClientRect();
    if (!rect) return;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    entryMenu = null;
  }

  function createEntry(): void {
    entryMenu = null;
    diaryEditor.set({ date: focusDate });
  }

  function openEntry(id: string): void {
    entryMenu = null;
    diaryEditor.set({ id });
  }

  function jumpToToday(): void {
    cursor = monthOf(today);
    selectedDate = today;
  }

  function prevMonth(): void {
    cursor = shiftMonth(cursor, -1);
  }

  function nextMonth(): void {
    cursor = shiftMonth(cursor, 1);
  }

  /** 点日历格：选中那一天；点到前后月的补齐格就顺势翻到那个月。 */
  function pickCell(date: string): void {
    selectedDate = date;
    cursor = monthOf(date);
    entryMenu = null;
    // 用户**显式**点过某天：切回列表时定位到那一组（没点过就按老规矩回顶）
    calendarPicked = true;
  }

  function toggleGroup(key: string): void {
    expanded = { ...expanded, [key]: !expanded[key] };
  }

  function handleExpand(event: CustomEvent<{ id: string; expanded: boolean }>): void {
    void setDiaryUi(event.detail.id, { expanded: event.detail.expanded });
  }

  function handleContext(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    entryMenu = { id: event.detail.id, x: event.detail.x, y: event.detail.y };
    showGear = false;
    listMenuAt = null;
  }

  function handleOpenLink(event: CustomEvent<{ href: string; title: string }>): void {
    onOpenLink(event.detail.href, event.detail.title);
  }

</script>

<main class="diary-view" style={mainStyle}>
  <section class="list-header">
    <div>
      <MobileBack />
      <span class="header-icon"><NotebookPen size={34} /></span>
      <h1>日记</h1>
    </div>
    <div class="header-actions" on:click|stopPropagation>
      <div class="diary-view-switch" role="tablist" aria-label="日记视图">
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
        type="button"
        title={searchOpen ? "关闭搜索" : "搜索日记"}
        aria-label={searchOpen ? "关闭搜索" : "搜索日记"}
        aria-pressed={searchOpen}
        on:click|stopPropagation={toggleSearch}
      >{#if searchOpen}<X size={21} />{:else}<Search size={21} />{/if}</button>
      <button
        bind:this={gearButtonEl}
        type="button"
        title="日记菜单"
        aria-label="日记菜单"
        aria-expanded={listMenuAt !== null}
        on:click|stopPropagation={openListMenuFromGear}
      ><SettingsIcon size={21} /></button>
    </div>
  </section>

  <p class="list-subtitle">
    <span>共 {stats.total} 篇</span>
    <span class="diary-stat-dot"></span>
    <span>本月 {stats.monthCount} 篇</span>
    {#if stats.streak > 1}
      <span class="diary-stat-dot"></span>
      <span class="diary-stat-streak" title="连续记录天数"><Flame size={13} />连续 {stats.streak} 天</span>
    {/if}
  </p>

  {#if searchOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <label class="diary-search" on:click|stopPropagation>
      <Search size={16} />
      <input bind:this={searchInput} bind:value={query} type="text" placeholder="搜索标题、正文或标签" on:input={handleQueryInput} />
    </label>
  {/if}

  <section class="diary-scroll" bind:this={scrollEl}>
    {#if view === "list"}
      <!-- 窗口化（v0.8.4 需求 3）：5000 篇也只在树上挂视口附近的几十张卡 -->
      <VirtualStack
        bind:this={stack}
        items={listRows}
        keyOf={(row) => (row as { key: string }).key}
        scroller={scrollEl}
        estimate={96}
        overscan={10}
      >
        <svelte:fragment slot="item" let:row>
          {@const item = row as DiaryListRow}
          {#if item.kind === "year"}
            <div class="diary-year-divider">
              <span>{item.label}</span>
              <em>{item.count} 篇</em>
            </div>
          {:else if item.entry}
            <DiaryCard
              entry={item.entry}
              {today}
              selected={entryMenu?.id === item.entry.id}
              on:expand={handleExpand}
              on:edit={(event) => openEntry(event.detail)}
              on:context={handleContext}
              on:openLink={handleOpenLink}
            />
          {/if}
        </svelte:fragment>
      </VirtualStack>

    {:else if view === "calendar"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="diary-calendar" use:swipeX={{ onPrev: prevMonth, onNext: nextMonth }}>
        <div class="diary-calendar-bar">
          <button type="button" aria-label="上个月" on:click|stopPropagation={prevMonth}><ChevronLeft size={18} /></button>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <strong
            bind:this={monthLabelEl}
            class="month-pop-anchor"
            role="button"
            tabindex="0"
            title="点击直接选年月"
            on:click|stopPropagation={() => (monthPopOpen = !monthPopOpen)}
          >{monthLabel}</strong>
          <button type="button" aria-label="下个月" on:click|stopPropagation={nextMonth}><ChevronRight size={18} /></button>
        </div>
        <MonthPopover
          open={monthPopOpen}
          anchor={monthLabelEl}
          year={cursor.year}
          month={cursor.month}
          onSelect={(next) => {
            cursor = { year: next.year, month: next.month };
            selectedDate = `${next.year}-${(next.month + 1).toString().padStart(2, "0")}-01`;
            entryMenu = null;
          }}
          onClose={() => (monthPopOpen = false)}
        />
        <div class="diary-calendar-grid">
          {#each calendarWeekdayHeaders($weekStart) as label (label)}
            <span class="diary-calendar-head">{label}</span>
          {/each}
          {#each cells as cell (cell.date)}
            <button
              type="button"
              class="diary-calendar-cell"
              class:other-month={!cell.current}
              class:today={cell.date === today}
              class:selected={cell.date === selectedDate}
              class:has-entry={cell.count > 0}
              on:click|stopPropagation={() => pickCell(cell.date)}
            >
              <span class="diary-cell-day">{cell.day}</span>
              {#if cell.mood}
                <span class="diary-cell-mood">{cell.mood}</span>
              {:else if cell.count}
                <span class="diary-cell-dot"></span>
              {/if}
              {#if cell.count > 1}<span class="diary-cell-count">{cell.count}</span>{/if}
            </button>
          {/each}
        </div>
      </div>

      <div class="diary-day-head">
        <strong>{fullDayLabel(selectedDate)}</strong>
        <span>{relativeDayLabel(selectedDate, today)} · {dayEntries.length} 篇</span>
      </div>
      {#each dayEntries as entry (entry.id)}
        <DiaryCard
          {entry}
          {today}
          showDate={false}
          selected={entryMenu?.id === entry.id}
          on:expand={handleExpand}
          on:edit={(event) => openEntry(event.detail)}
          on:context={handleContext}
          on:openLink={handleOpenLink}
        />
      {:else}
        <div class="diary-day-empty">{query.trim() ? "没有匹配的日记" : "这一天还没有日记"}</div>
      {/each}

    {:else}
      {#each years as year (year.key)}
        <section class="diary-group">
          <button class="diary-group-head" type="button" on:click|stopPropagation={() => toggleGroup(`y-${year.key}`)}>
            <ChevronDown class={expanded[`y-${year.key}`] ? "" : "collapsed"} size={16} />
            <strong>{year.label}</strong>
            <em>{year.count} 篇</em>
          </button>
          {#if expanded[`y-${year.key}`]}
            {#each year.months as month (month.key)}
              <section class="diary-group-month">
                <button class="diary-group-subhead" type="button" on:click|stopPropagation={() => toggleGroup(`m-${month.key}`)}>
                  <ChevronDown class={expanded[`m-${month.key}`] ? "" : "collapsed"} size={14} />
                  <span>{month.month + 1}月</span>
                  <em>{month.count} 篇</em>
                </button>
                {#if expanded[`m-${month.key}`]}
                  {#each month.days.flatMap((day) => day.entries) as entry (entry.id)}
                    <DiaryCard
                      {entry}
                      {today}
                      selected={entryMenu?.id === entry.id}
                      on:expand={handleExpand}
                      on:edit={(event) => openEntry(event.detail)}
                      on:context={handleContext}
                      on:openLink={handleOpenLink}
                    />
                  {/each}
                {/if}
              </section>
            {/each}
          {/if}
        </section>
      {/each}
    {/if}

    {#if entries.length === 0 && view !== "calendar"}
      <div class="empty-state">
        <strong>{query.trim() ? "没有匹配的日记" : "还没有日记"}</strong>
        {#if !query.trim()}
          <span>点右下角的 + 写下第一篇。</span>
        {/if}
      </div>
    {/if}
  </section>

  {#if listMenuAt}
    <ListMenu
      x={listMenuAt.x}
      y={listMenuAt.y}
      xAlign="right"
      anchor={gearButtonEl}
      diaryMode
      background={diaryBg}
      accentColor={diaryAccent($appSettings.diary)}
      onClose={() => (listMenuAt = null)}
    />
  {/if}

  {#if entryMenu && menuEntry}
    <DiaryEntryMenu
      x={entryMenu.x}
      y={entryMenu.y}
      entry={menuEntry}
      {today}
      on:edit={(event) => openEntry(event.detail)}
      on:close={() => (entryMenu = null)}
    />
  {/if}

  <div class="diary-fab-row">
    {#if showTodayButton}
      <button class="diary-fab-today" type="button" title="回到今天" on:click|stopPropagation={jumpToToday}>今</button>
    {/if}
    <button class="diary-fab" type="button" title="写一篇日记" aria-label="写一篇日记" on:click|stopPropagation={createEntry}>
      <Plus size={24} />
    </button>
  </div>
</main>

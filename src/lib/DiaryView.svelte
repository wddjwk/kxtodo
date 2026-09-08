<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    ArrowLeft, CalendarDays, ChevronDown, ChevronLeft, ChevronRight, CloudSun,
    Flame, FolderTree, List as ListIcon, NotebookPen, PenLine, Plus, Search,
    Smile, Trash2, X
  } from "@lucide/svelte";
  import { appSettings, appState, diaryEditor, diaryOpen } from "./stores";
  import { deleteDiaryEntry, setConfig, setDiaryUi, updateDiaryEntry } from "./actions";
  import { buildMainStyle, DIARY_ACCENT } from "./styles";
  import { defaultBackground } from "./defaults";
  import { isMobile, showMobileList } from "./platform";
  import {
    calendarCells, calendarWeekdayHeaders, diaryByDate, diaryStats, filterDiaries,
    fullDayLabel, monthOf, MOOD_PRESETS, relativeDayLabel, shiftMonth, todayDate,
    WEATHER_PRESETS, yearGroups, type MonthCursor
  } from "./diary";
  import DiaryCard from "./diary/DiaryCard.svelte";
  import DatePicker from "./DatePicker.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import type { DiaryEntry, DiaryViewMode } from "./types";

  /** 链接打开走 Workspace 那一条（应用内预览 / 系统浏览器由设置决定），不重复实现。 */
  export let onOpenLink: (url: string, title?: string) => void = () => {};

  const VIEWS: Array<{ mode: DiaryViewMode; label: string; icon: typeof ListIcon }> = [
    { mode: "list", label: "列表视图", icon: ListIcon },
    { mode: "calendar", label: "日历视图", icon: CalendarDays },
    { mode: "group", label: "分组视图", icon: FolderTree }
  ];

  let searchOpen = false;
  let searchInput: HTMLInputElement;
  let query = "";
  let entryMenu: { id: string; x: number; y: number } | null = null;
  let cursor: MonthCursor = monthOf(todayDate());
  let selectedDate = todayDate();
  /** 分组视图的年/月折叠状态（本机 UI 状态，不持久化） */
  let collapsed: Record<string, boolean> = {};
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
  $: entries = filterDiaries($appState.diaries, query);
  // 统计永远基于全部日记：搜索时「共几篇/连续几天」不该跟着筛选结果变
  $: stats = diaryStats($appState.diaries, today);
  $: byDate = diaryByDate(entries);
  $: years = yearGroups(entries);
  $: cells = calendarCells(cursor, byDate);
  $: monthLabel = `${cursor.year}年${cursor.month + 1}月`;
  /** 添加按钮落在哪一天：日历视图跟着选中的日期，其余视图永远是今天 */
  $: focusDate = view === "calendar" ? selectedDate : today;
  $: dayEntries = byDate.get(selectedDate) ?? [];
  $: menuEntry = entryMenu
    ? $appState.diaries.find((entry) => entry.id === entryMenu?.id) ?? null
    : null;

  // 列表视图 = 年份分隔行 + 卡片。直接摊平分组视图算好的年→月→天，两边顺序天然一致。
  $: listRows = years.flatMap((year) => [
    { kind: "year" as const, key: `year-${year.key}`, label: year.label, count: year.count, entry: null as DiaryEntry | null },
    ...year.months.flatMap((month) =>
      month.days.flatMap((day) =>
        day.entries.map((entry) => ({
          kind: "entry" as const,
          key: entry.id,
          label: "",
          count: 0,
          entry: entry as DiaryEntry | null
        }))
      )
    )
  ]);

  export function closeOverlays(): void {
    entryMenu = null;
  }

  function closeDiary(): void {
    if ($isMobile) {
      showMobileList();
      return;
    }
    diaryOpen.set(false);
  }

  function switchView(mode: DiaryViewMode): void {
    entryMenu = null;
    if (mode === "calendar") {
      cursor = monthOf(selectedDate);
    }
    if (mode === view) return;
    void setConfig("diary.view", mode);
  }

  function toggleSearch(): void {
    searchOpen = !searchOpen;
    entryMenu = null;
    if (!searchOpen) query = "";
    void tick().then(() => searchInput?.focus());
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
  }

  function toggleGroup(key: string): void {
    collapsed = { ...collapsed, [key]: !collapsed[key] };
  }

  function handleExpand(event: CustomEvent<{ id: string; expanded: boolean }>): void {
    void setDiaryUi(event.detail.id, { expanded: event.detail.expanded });
  }

  function handleContext(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    entryMenu = { id: event.detail.id, x: event.detail.x, y: event.detail.y };
  }

  function handleOpenLink(event: CustomEvent<{ href: string; title: string }>): void {
    onOpenLink(event.detail.href, event.detail.title);
  }

  function setEntryDate(id: string, date: string): void {
    entryMenu = null;
    void updateDiaryEntry(id, { date });
  }

  function setEntryMood(id: string, emoji: string): void {
    entryMenu = null;
    void updateDiaryEntry(id, { mood: emoji });
  }

  function setEntryWeather(id: string, emoji: string): void {
    entryMenu = null;
    void updateDiaryEntry(id, { weather: emoji });
  }

  function removeEntry(id: string): void {
    entryMenu = null;
    void deleteDiaryEntry(id);
  }
</script>

<main class="diary-view" style={buildMainStyle(defaultBackground, DIARY_ACCENT)}>
  <section class="list-header">
    <div>
      <button class="mobile-back" type="button" aria-label="返回列表" on:click|stopPropagation={closeDiary}>
        <ArrowLeft size={26} />
      </button>
      <span class="header-icon"><NotebookPen size={34} /></span>
      <h1>日记</h1>
    </div>
    <div class="header-actions">
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
      <button type="button" title="搜索日记" aria-label="搜索日记" on:click|stopPropagation={toggleSearch}>
        {#if searchOpen}<X size={21} />{:else}<Search size={21} />{/if}
      </button>
    </div>
  </section>

  <p class="diary-subtitle">
    <span>共 {stats.total} 篇</span>
    <span class="diary-stat-dot"></span>
    <span>本月 {stats.monthCount} 篇</span>
    {#if stats.streak > 1}
      <span class="diary-stat-dot"></span>
      <span class="diary-stat-streak" title="连续记录天数"><Flame size={13} />连续 {stats.streak} 天</span>
    {/if}
  </p>

  {#if searchOpen}
    <label class="diary-search">
      <Search size={16} />
      <input bind:this={searchInput} bind:value={query} type="text" placeholder="搜索标题、正文或标签" />
    </label>
  {/if}

  <section class="diary-scroll">
    {#if view === "list"}
      {#each listRows as row (row.key)}
        {#if row.kind === "year"}
          <div class="diary-year-divider">
            <span>{row.label}</span>
            <em>{row.count} 篇</em>
          </div>
        {:else if row.entry}
          <DiaryCard
            entry={row.entry}
            {today}
            selected={entryMenu?.id === row.entry.id}
            on:expand={handleExpand}
            on:edit={(event) => openEntry(event.detail)}
            on:context={handleContext}
            on:openLink={handleOpenLink}
          />
        {/if}
      {/each}

    {:else if view === "calendar"}
      <div class="diary-calendar">
        <div class="diary-calendar-bar">
          <button type="button" aria-label="上个月" on:click|stopPropagation={prevMonth}><ChevronLeft size={18} /></button>
          <strong>{monthLabel}</strong>
          <button type="button" aria-label="下个月" on:click|stopPropagation={nextMonth}><ChevronRight size={18} /></button>
        </div>
        <div class="diary-calendar-grid">
          {#each calendarWeekdayHeaders as label (label)}
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
            <ChevronDown class={collapsed[`y-${year.key}`] ? "collapsed" : ""} size={16} />
            <strong>{year.label}</strong>
            <em>{year.count} 篇</em>
          </button>
          {#if !collapsed[`y-${year.key}`]}
            {#each year.months as month (month.key)}
              <section class="diary-group-month">
                <button class="diary-group-subhead" type="button" on:click|stopPropagation={() => toggleGroup(`m-${month.key}`)}>
                  <ChevronDown class={collapsed[`m-${month.key}`] ? "collapsed" : ""} size={14} />
                  <span>{month.month + 1}月</span>
                  <em>{month.count} 篇</em>
                </button>
                {#if !collapsed[`m-${month.key}`]}
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

  {#if entryMenu && menuEntry}
    <ContextMenu x={entryMenu.x} y={entryMenu.y} minWidth={216} onClose={() => (entryMenu = null)}>
      <MenuItem icon={PenLine} label="编辑" onSelect={() => openEntry(menuEntry.id)} />
      <MenuItem icon={CalendarDays} label="修改日期">
        <div slot="submenu" class="task-menu-date">
          <DatePicker
            value={menuEntry.date}
            on:select={(event) => setEntryDate(menuEntry.id, event.detail)}
            on:clear={() => setEntryDate(menuEntry.id, today)}
          />
        </div>
      </MenuItem>
      <MenuItem icon={Smile} label="心情">
        <div slot="submenu" class="diary-emoji-menu">
          {#each MOOD_PRESETS as preset (preset.emoji)}
            <button
              class="diary-emoji-cell"
              type="button"
              class:selected={menuEntry.mood === preset.emoji}
              title={preset.label}
              on:click|stopPropagation={() => setEntryMood(menuEntry.id, preset.emoji)}
            >{preset.emoji}</button>
          {/each}
        </div>
      </MenuItem>
      <MenuItem icon={CloudSun} label="天气">
        <div slot="submenu" class="diary-emoji-menu">
          {#each WEATHER_PRESETS as preset (preset.emoji)}
            <button
              class="diary-emoji-cell"
              type="button"
              class:selected={menuEntry.weather === preset.emoji}
              title={preset.label}
              on:click|stopPropagation={() => setEntryWeather(menuEntry.id, preset.emoji)}
            >{preset.emoji}</button>
          {/each}
        </div>
      </MenuItem>
      <MenuSeparator />
      <MenuItem icon={Trash2} danger label="删除" onSelect={() => removeEntry(menuEntry.id)} />
    </ContextMenu>
  {/if}

  <div class="diary-fab-row">
    {#if focusDate !== today}
      <button class="diary-fab-today" type="button" title="回到今天" on:click|stopPropagation={jumpToToday}>今</button>
    {/if}
    <button class="diary-fab" type="button" title="写一篇日记" aria-label="写一篇日记" on:click|stopPropagation={createEntry}>
      <Plus size={24} />
    </button>
  </div>
</main>

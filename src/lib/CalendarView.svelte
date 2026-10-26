<script lang="ts">
  /**
   * 日程整页（v0.9.0；v0.9.1 加分类、重复、缩放与工具栏收口）。
   * 月 / 周 / 三日 / 日 四个视图（默认周视图）+ 右上角段控 +
   * 三点菜单（管理分类 / UI颜色 / 背景颜色）+ 右下角新建 FAB。
   *
   * 与日记/记账同一条骨架：桌面 `calendar-open` 式互斥、移动端历史栈一层；
   * 外观走 settings.calendar（ListMenu 的 calendarMode）；段控/头部/FAB 照抄
   * LedgerView 的结构与类名口径。
   *
   * 重复日程在这一层摊平成「逐次出现」（`expandForRange`），下游两个视图只吃
   * 摊平后的数组——它们不需要知道重复这回事。
   */
  import { onMount } from "svelte";
  import {
    Calendar as CalendarIcon, CalendarClock, CalendarDays, CalendarRange, ChevronLeft, ChevronRight,
    Columns3, MoreHorizontal, Plus
  } from "@lucide/svelte";
  import {
    appSettings, appState, calendarCategories, calendarEditor, calendarEvents, calendarOpen,
    showToast, weekStart
  } from "./stores";
  import { selectNode as selectNodeAction, setConfig, updateCalendarEvent } from "./actions";
  import { buildMainStyle, calendarAccent, calendarBackground, PAGE_HEADER_ICON_SIZE } from "./styles";
  import { accentWithPreview, backgroundWithPreview, colorPreview } from "./colorPreview";
  import { imageCache, resolveImageSrc } from "./images";
  import {
    addDays, calendarToday, expandForRange, formatMinutes, MINUTES_PER_DAY, monthGridDates,
    rangeTitle, seriesChangesFromDrag, shiftRange, threeDayDates, weekDates
  } from "./calendar";
  import { monthOf } from "./diary";
  import { swipeX } from "./swipe";
  import MobileBack from "./MobileBack.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import CalendarTimeGrid from "./calendar/CalendarTimeGrid.svelte";
  import CalendarMonth from "./calendar/CalendarMonth.svelte";
  import CalendarEventMenu from "./calendar/CalendarEventMenu.svelte";
  import CalendarCategoryPanel from "./calendar/CalendarCategoryPanel.svelte";
  import { createBackGuard, isMobile, showMobileContent } from "./platform";
  import type { CalendarEvent, CalendarViewMode } from "./types";

  const VIEWS: Array<{ mode: CalendarViewMode; label: string; icon: typeof CalendarDays }> = [
    { mode: "month", label: "月视图", icon: CalendarDays },
    { mode: "week", label: "周视图", icon: CalendarRange },
    { mode: "threeday", label: "三日视图", icon: Columns3 },
    { mode: "day", label: "日视图", icon: CalendarIcon }
  ];

  let menuButtonEl: HTMLButtonElement;
  let listMenuAt: { x: number; y: number } | null = null;
  let eventMenu: { id: string; x: number; y: number; occurrence: string } | null = null;
  let monthPopOpen = false;
  let monthLabelEl: HTMLElement;
  let grid: CalendarTimeGrid;
  let showCategories = false;

  /** 当前区间的锚点日（月视图取当月、周视图取所在周、三日/日视图取中心日） */
  let anchorDate = calendarToday();
  // 分钟级 tick：「现在」红线与跨天要自己跟上（与日记/记账同一套路）
  let dayTick = 0;

  onMount(() => {
    const timer = window.setInterval(() => {
      dayTick += 1;
    }, 60_000);
    return () => window.clearInterval(timer);
  });

  $: today = dayTick >= 0 ? calendarToday() : "";
  $: nowMinutes = dayTick >= 0
    ? new Date().getHours() * 60 + new Date().getMinutes()
    : 0;
  $: view = $appSettings.calendar.view;
  $: hourZoom = $appSettings.calendar.hourZoom;
  $: events = $calendarEvents;
  $: categories = $calendarCategories;
  $: accent = accentWithPreview($colorPreview, "calendar", calendarAccent($appSettings.calendar));
  $: dates = view === "month"
    ? monthGridDates(`${anchorDate.slice(0, 7)}-01`, $weekStart)
    : view === "week"
      ? weekDates(anchorDate, $weekStart)
      : view === "threeday"
        ? threeDayDates(anchorDate)
        : [anchorDate];
  /** 时间网格只吃「当前区间」的天（月视图不用网格） */
  $: gridDates = view === "week"
    ? weekDates(anchorDate, $weekStart)
    : view === "threeday"
      ? threeDayDates(anchorDate)
      : [anchorDate];
  /** 重复日程按当前区间摊平成逐次出现（区间外的实例根本不生成，量是可控的） */
  $: occurrences = expandForRange(events, dates[0], dates[dates.length - 1]);
  $: title = rangeTitle(dates, view, view === "month" ? `${anchorDate.slice(0, 7)}-01` : anchorDate);
  $: calendarBg = calendarBackground($appSettings.calendar);
  $: resolvedBgImage = resolveImageSrc(calendarBg.image, $imageCache);
  $: mainStyle = buildMainStyle(
    backgroundWithPreview($colorPreview, "calendar", calendarBg),
    accent,
    resolvedBgImage
  );
  $: menuEvent = eventMenu
    ? occurrences.find((event) => event.id === eventMenu?.id) ?? events.find((event) => event.id === eventMenu?.id) ?? null
    : null;

  const backGuard = createBackGuard();
  $: backGuard(monthPopOpen, () => {
    monthPopOpen = false;
  });

  export function closeOverlays(): void {
    listMenuAt = null;
    eventMenu = null;
    monthPopOpen = false;
    grid?.clearDraft();
  }

  function switchView(mode: CalendarViewMode): void {
    closeOverlays();
    if (mode === view) return;
    void setConfig("calendar.view", mode);
  }

  function step(direction: -1 | 1): void {
    closeOverlays();
    anchorDate = shiftRange(view === "month" ? `${anchorDate.slice(0, 7)}-01` : anchorDate, view, direction);
  }

  /**
   * 「现在」（v0.9.1 起工具栏只有它，没有「今天」）：跳回今天并把网格滚到此刻——
   * 一个按钮同时管「回到今天」和「回到此刻」，月视图就是回到本月。
   */
  function jumpToNow(): void {
    closeOverlays();
    anchorDate = today;
    if (view !== "month") grid?.scrollToNow();
  }

  /** 三点菜单：照抄工具页/记账的点锚定四件套（贴按钮正下方、右缘对齐、再点收起） */
  function openListMenu(): void {
    if (listMenuAt) {
      listMenuAt = null;
      return;
    }
    const rect = menuButtonEl?.getBoundingClientRect();
    if (!rect) return;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    eventMenu = null;
  }

  function openCategoryPanel(): void {
    showCategories = true;
  }

  /** 网格拖选/点击空白 → 新建（预置起止时刻） */
  function createAt(detail: { date: string; startMin: number; endMin: number }): void {
    eventMenu = null;
    grid?.clearDraft();
    const startTime = formatMinutes(detail.startMin);
    const endAtMidnight = detail.endMin >= MINUTES_PER_DAY;
    calendarEditor.set({
      startDate: detail.date,
      startTime,
      endDate: endAtMidnight ? addDays(detail.date, 1) : detail.date,
      endTime: endAtMidnight ? "00:00" : formatMinutes(detail.endMin)
    });
  }

  function createAllDayAt(date: string): void {
    eventMenu = null;
    grid?.clearDraft();
    calendarEditor.set({ startDate: date, allDay: true });
  }

  function createNow(): void {
    closeOverlays();
    // FAB 建在「区间里的那一天」：区间含今天就是今天，否则落在锚点日
    const date = dates.includes(today) ? today : (view === "month" ? anchorDate : gridDates[0]);
    const now = new Date();
    const hour = (now.getHours() + 1) % 24;
    const startTime = `${`${hour}`.padStart(2, "0")}:00`;
    const endHour = (hour + 1) % 24;
    const endTime = `${`${endHour}`.padStart(2, "0")}:00`;
    calendarEditor.set({
      startDate: date,
      startTime,
      endDate: endHour <= hour ? addDays(date, 1) : date,
      endTime
    });
  }

  function openEvent(id: string): void {
    eventMenu = null;
    calendarEditor.set({ id });
  }

  function openDay(date: string): void {
    closeOverlays();
    anchorDate = date;
    if (view !== "day") void setConfig("calendar.view", "day");
  }

  function handleContext(event: CustomEvent<{ id: string; x: number; y: number; occurrence: string }>): void {
    eventMenu = { ...event.detail };
    listMenuAt = null;
  }

  /**
   * 网格里拖动/缩放事件块的落地写（core 会再校验一遍区间）。
   * 重复日程拖的是「整个系列」：`seriesChangesFromDrag` 决定平移多少、要不要跟着改
   * 重复的星期/日号（多星期的周规则只改时刻，不换哪几天）。
   */
  function handleRetime(
    event: CustomEvent<{ id: string; occurrence: string; startDate: string; startTime: string; endDate: string; endTime: string }>
  ): void {
    const detail = event.detail;
    const source = events.find((item) => item.id === detail.id);
    const changes = source
      ? seriesChangesFromDrag(source, detail.occurrence, {
          startDate: detail.startDate,
          startTime: detail.startTime,
          endDate: detail.endDate,
          endTime: detail.endTime
        })
      : {
          startDate: detail.startDate,
          startTime: detail.startTime,
          endDate: detail.endDate,
          endTime: detail.endTime
        };
    void updateCalendarEvent(detail.id, changes);
  }

  /** 缩放手势（Ctrl+滚轮 / 双指捏合）落进设置：本机偏好，不进同步 */
  function handleZoom(event: CustomEvent<number>): void {
    void setConfig("calendar.hourZoom", event.detail);
  }

  /** 事件菜单「转到关联条目」：选中条目所在分组并回到工作区（与侧栏 selectNode 同一条互斥） */
  function gotoTask(taskId: string): void {
    eventMenu = null;
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (!task) {
      showToast("关联的条目已不存在");
      return;
    }
    calendarEditor.set(null);
    if ($isMobile) {
      showMobileContent();
    } else {
      calendarOpen.set(false);
    }
    void selectNodeAction(task.nodeId);
  }

  function handleMonthCreate(event: CustomEvent<{ date: string }>): void {
    createAllDayAt(event.detail.date);
  }
</script>

<main class="calendar-view" style={mainStyle}>
  <section class="list-header">
    <div>
      <MobileBack />
      <span class="header-icon"><CalendarClock size={PAGE_HEADER_ICON_SIZE} /></span>
      <h1>日程</h1>
    </div>
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="header-actions" on:click|stopPropagation>
      <div class="calendar-view-switch" role="tablist" aria-label="日程视图">
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
        bind:this={menuButtonEl}
        type="button"
        title="日程菜单"
        aria-label="日程菜单"
        aria-expanded={listMenuAt !== null}
        on:click|stopPropagation={openListMenu}
      ><MoreHorizontal size={21} /></button>
    </div>
  </section>

  <div class="calendar-range-bar">
    <span class="calendar-range-step">
      <button type="button" aria-label="上一段" on:click|stopPropagation={() => step(-1)}><ChevronLeft size={18} /></button>
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
      <strong
        bind:this={monthLabelEl}
        class="month-pop-anchor"
        role="button"
        tabindex="0"
        title={view === "month" ? "点击直接选年月" : undefined}
        on:click|stopPropagation={() => { if (view === "month") monthPopOpen = !monthPopOpen; }}
      >{title}</strong>
      <button type="button" aria-label="下一段" on:click|stopPropagation={() => step(1)}><ChevronRight size={18} /></button>
    </span>
    <button class="calendar-now-btn" type="button" title="回到今天的此刻" on:click|stopPropagation={jumpToNow}>现在</button>
  </div>

  {#if view === "month"}
    <MonthPopover
      open={monthPopOpen}
      anchor={monthLabelEl}
      year={monthOf(anchorDate).year}
      month={monthOf(anchorDate).month}
      onSelect={(next) => {
        anchorDate = `${next.year}-${`${next.month + 1}`.padStart(2, "0")}-01`;
        monthPopOpen = false;
      }}
      onClose={() => (monthPopOpen = false)}
    />
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <section class="calendar-body" use:swipeX={{ onPrev: () => step(1), onNext: () => step(-1) }}>
    {#if view === "month"}
      <CalendarMonth
        anchor={anchorDate}
        events={occurrences}
        {today}
        {categories}
        {accent}
        weekStart={$weekStart}
        on:create={handleMonthCreate}
        on:open={(event) => openEvent(event.detail.id)}
        on:context={handleContext}
        on:openDay={(event) => openDay(event.detail)}
      />
    {:else}
      <CalendarTimeGrid
        bind:this={grid}
        dates={gridDates}
        events={occurrences}
        {today}
        {nowMinutes}
        {categories}
        {accent}
        zoom={hourZoom}
        on:create={(event) => createAt(event.detail)}
        on:createAllDay={(event) => createAllDayAt(event.detail.date)}
        on:open={(event) => openEvent(event.detail.id)}
        on:context={handleContext}
        on:retimed={handleRetime}
        on:openDay={(event) => openDay(event.detail)}
        on:zoom={handleZoom}
      />
    {/if}
  </section>

  {#if listMenuAt}
    <ListMenu
      x={listMenuAt.x}
      y={listMenuAt.y}
      xAlign="right"
      anchor={menuButtonEl}
      calendarMode
      onManageCategories={openCategoryPanel}
      background={calendarBg}
      accentColor={calendarAccent($appSettings.calendar)}
      onClose={() => (listMenuAt = null)}
    />
  {/if}

  {#if eventMenu && menuEvent}
    <CalendarEventMenu
      x={eventMenu.x}
      y={eventMenu.y}
      event={menuEvent as CalendarEvent}
      occurrence={eventMenu.occurrence}
      on:edit={(event) => openEvent(event.detail)}
      on:gotoTask={(event) => gotoTask(event.detail)}
      on:close={() => (eventMenu = null)}
    />
  {/if}

  {#if showCategories}
    <CalendarCategoryPanel onClose={() => (showCategories = false)} />
  {/if}

  <div class="calendar-fab-row">
    <button
      class="calendar-fab"
      type="button"
      title="新建日程"
      aria-label="新建日程"
      on:click|stopPropagation={createNow}
    >
      <Plus size={24} />
    </button>
  </div>
</main>

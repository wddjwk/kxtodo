<script lang="ts">
  /**
   * 日程整页（v0.9.0）：月 / 周 / 三日 / 日 四个视图（默认周视图）+ 右上角段控 +
   * 三点菜单（UI颜色 / 背景颜色）+ 右下角新建 FAB。
   *
   * 与日记/记账同一条骨架：桌面 `calendar-open` 式互斥、移动端历史栈一层；
   * 外观走 settings.calendar（ListMenu 的 calendarMode）；段控/头部/FAB 照抄
   * LedgerView 的结构与类名口径。
   */
  import { onMount } from "svelte";
  import {
    Calendar as CalendarIcon, CalendarClock, CalendarDays, CalendarRange, ChevronLeft, ChevronRight,
    Columns3, MoreHorizontal, Plus
  } from "@lucide/svelte";
  import { appSettings, appState, calendarEditor, calendarEvents, calendarOpen, showToast, weekStart } from "./stores";
  import { selectNode as selectNodeAction, setConfig, updateCalendarEvent } from "./actions";
  import { buildMainStyle, calendarAccent, calendarBackground, PAGE_HEADER_ICON_SIZE } from "./styles";
  import { accentWithPreview, backgroundWithPreview, colorPreview } from "./colorPreview";
  import { imageCache, resolveImageSrc } from "./images";
  import {
    addDays, calendarToday, formatMinutes, MINUTES_PER_DAY, monthGridDates, rangeTitle,
    shiftRange, threeDayDates, weekDates
  } from "./calendar";
  import { monthOf } from "./diary";
  import { swipeX } from "./swipe";
  import MobileBack from "./MobileBack.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import CalendarTimeGrid from "./calendar/CalendarTimeGrid.svelte";
  import CalendarMonth from "./calendar/CalendarMonth.svelte";
  import CalendarEventMenu from "./calendar/CalendarEventMenu.svelte";
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
  let eventMenu: { id: string; x: number; y: number } | null = null;
  let monthPopOpen = false;
  let monthLabelEl: HTMLElement;
  let grid: CalendarTimeGrid;

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
  $: events = $calendarEvents;
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
  $: title = rangeTitle(dates, view, view === "month" ? `${anchorDate.slice(0, 7)}-01` : anchorDate);
  $: calendarBg = calendarBackground($appSettings.calendar);
  $: resolvedBgImage = resolveImageSrc(calendarBg.image, $imageCache);
  $: mainStyle = buildMainStyle(
    backgroundWithPreview($colorPreview, "calendar", calendarBg),
    accentWithPreview($colorPreview, "calendar", calendarAccent($appSettings.calendar)),
    resolvedBgImage
  );
  $: menuEvent = eventMenu
    ? events.find((event) => event.id === eventMenu?.id) ?? null
    : null;
  $: awayFromToday = !dates.includes(today);

  const backGuard = createBackGuard();
  $: backGuard(monthPopOpen, () => {
    monthPopOpen = false;
  });

  export function closeOverlays(): void {
    listMenuAt = null;
    eventMenu = null;
    monthPopOpen = false;
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

  function jumpToToday(): void {
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

  /** 网格拖选/点击空白 → 新建（预置起止时刻） */
  function createAt(detail: { date: string; startMin: number; endMin: number }): void {
    eventMenu = null;
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

  function handleContext(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    eventMenu = { ...event.detail };
    listMenuAt = null;
  }

  /** 网格里拖动/缩放事件块的落地写（core 会再校验一遍区间） */
  function handleRetime(event: CustomEvent<{ id: string; startDate: string; startTime: string; endDate: string; endTime: string }>): void {
    const detail = event.detail;
    void updateCalendarEvent(detail.id, {
      startDate: detail.startDate,
      startTime: detail.startTime,
      endDate: detail.endDate,
      endTime: detail.endTime
    });
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
    <button class="calendar-today-btn" type="button" class:away={awayFromToday} on:click|stopPropagation={jumpToToday}>今天</button>
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
    {#if view !== "month"}
      <button class="calendar-now-btn" type="button" title="回到当前时刻" on:click|stopPropagation={() => grid?.scrollToNow()}>现在</button>
    {/if}
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
        {events}
        {today}
        weekStart={$weekStart}
        on:create={handleMonthCreate}
        on:open={(event) => openEvent(event.detail)}
        on:context={handleContext}
        on:openDay={(event) => openDay(event.detail)}
      />
    {:else}
      <CalendarTimeGrid
        bind:this={grid}
        dates={gridDates}
        {events}
        {today}
        {nowMinutes}
        on:create={(event) => createAt(event.detail)}
        on:createAllDay={(event) => createAllDayAt(event.detail.date)}
        on:open={(event) => openEvent(event.detail)}
        on:context={handleContext}
        on:retimed={handleRetime}
        on:openDay={(event) => openDay(event.detail)}
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
      on:edit={(event) => openEvent(event.detail)}
      on:gotoTask={(event) => gotoTask(event.detail)}
      on:close={() => (eventMenu = null)}
    />
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

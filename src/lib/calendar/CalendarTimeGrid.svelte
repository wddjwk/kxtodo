<script lang="ts">
  /**
   * 日程的时间网格（周 / 三日 / 日视图共用，v0.9.0）。
   *
   * 结构与业界日历一致：顶部星期行 → 全天栏 → 可滚动的时间网格。
   * 冲突占位走 `calendar.ts::layoutDaySegments`（重叠簇内平分列）。
   *
   * 交互分两套（对齐飞书/Google 日历）：
   * - 桌面（鼠标）：空白按下拖动 = 框选时间段建日程；单击空白 = 默认一小时；
   *   事件块上下边缘拖动 = 改时长；事件块中间拖动 = 挪动（跨列 = 跨天）。
   * - 触屏：只保留单击（空白 = 建默认一小时、事件 = 打开编辑），拖动手势让给滚动。
   *
   * **坐标换算铁律**：壳上有 `transform: scale(uiScale)`，rect 是视觉像素——
   * 分钟数一律按 `(clientY - rect.top) / rect.height * 1440` 的**比例**算，
   * 比例对缩放不敏感，不需要再除 scale。
   */
  import { createEventDispatcher, onMount, tick } from "svelte";
  import {
    addDays, allDayEventsFor, diffDays, eventTimeText, formatMinutes, layoutDaySegments,
    minutesOf, MINUTES_PER_DAY, segmentForDay, snapMinutes, timeLabel,
    type DaySegment, type LayoutBlock
  } from "../calendar";
  import { isMobile } from "../platform";
  import type { CalendarEvent } from "../types";

  export let dates: string[];
  export let events: CalendarEvent[];
  export let today = "";
  /** 当前时刻的分钟数（外部按分钟 tick 驱动，画「现在」红线） */
  export let nowMinutes = 0;
  /** 每小时的高度（布局像素） */
  export let hourHeight = 48;

  const dispatch = createEventDispatcher<{
    create: { date: string; startMin: number; endMin: number };
    createAllDay: { date: string };
    open: string;
    context: { id: string; x: number; y: number };
    retimed: { id: string; startDate: string; startTime: string; endDate: string; endTime: string };
    openDay: string;
  }>();

  const SNAP_MIN = 15;

  let scrollEl: HTMLElement;
  let columnEls: HTMLElement[] = [];

  $: totalHeight = hourHeight * 24;
  $: hours = Array.from({ length: 24 }, (_, hour) => hour);
  $: gridEpoch = dates[0] ?? today;

  /** 每一列（天）的段与布局块 */
  type DayColumn = {
    date: string;
    blocks: LayoutBlock[];
    allDay: CalendarEvent[];
  };
  $: columns = dates.map((date): DayColumn => {
    const segments: DaySegment[] = [];
    for (const event of events) {
      const segment = segmentForDay(event, date);
      if (segment) segments.push(segment);
    }
    return { date, blocks: layoutDaySegments(segments), allDay: allDayEventsFor(events, date) };
  });

  /**
   * 渲染用的列（含拖动预览）。必须是 `$:` 派生而不是模板里 `{@const displayBlocks(column)}`：
   * 函数调用会把 blockDrag/events 藏进闭包，键控 each 的块不随它们变化重算——
   * 月视图芯片不更新就是这个坑（CalendarMonth 有同款注释）。
   */
  $: renderedColumns = columns.map((column) => ({
    date: column.date,
    allDay: column.allDay,
    blocks: displayBlocks(column)
  }));

  onMount(() => {
    // 打开时滚到「现在」附近（早上 8 点与当前时刻取早者再往前一小时），
    // 与主流日历一致：既看得到今天剩下的安排，也不从午夜开始。
    void tick().then(scrollToNow);
  });

  export function scrollToNow(): void {
    if (!scrollEl) return;
    const anchor = Math.max(0, Math.min(nowMinutes, 8 * 60) - 60);
    scrollEl.scrollTop = (anchor / MINUTES_PER_DAY) * totalHeight;
  }

  function minutesFromClientY(date: string, clientY: number): number {
    const el = columnEls[dates.indexOf(date)];
    if (!el) return 0;
    const rect = el.getBoundingClientRect();
    if (rect.height <= 0) return 0;
    const ratio = (clientY - rect.top) / rect.height;
    return Math.min(MINUTES_PER_DAY, Math.max(0, ratio * MINUTES_PER_DAY));
  }

  function columnAt(clientX: number): string | null {
    for (let index = 0; index < columnEls.length; index++) {
      const el = columnEls[index];
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (clientX >= rect.left && clientX <= rect.right) return dates[index];
    }
    return null;
  }

  /** 事件的绝对分钟（以 dates[0] 为零点；可见区间之前的多日事件会是负数，算术仍成立） */
  function absRange(ev: CalendarEvent): { start: number; end: number } {
    const start = diffDays(gridEpoch, ev.startDate) * MINUTES_PER_DAY + minutesOf(ev.startTime || "00:00");
    const endMinutes = minutesOf(ev.endTime || "00:00");
    const end =
      diffDays(gridEpoch, ev.endDate) * MINUTES_PER_DAY +
      (ev.endDate === ev.startDate && endMinutes <= minutesOf(ev.startTime || "00:00")
        ? MINUTES_PER_DAY
        : endMinutes || MINUTES_PER_DAY);
    return { start, end };
  }

  // -------------------------------------------------------------------------
  // 空白拖动框选（桌面鼠标）
  // -------------------------------------------------------------------------

  let dragCreate: { date: string; anchorMin: number; currentMin: number } | null = null;
  let suppressColumnClick = 0;

  function handleColumnPointerDown(event: PointerEvent, date: string): void {
    if (event.button !== 0 || event.pointerType !== "mouse" || $isMobile) return;
    const target = event.target as HTMLElement;
    if (target.closest(".cal-event, .cal-now")) return;
    const minutes = snapMinutes(minutesFromClientY(date, event.clientY), SNAP_MIN);
    dragCreate = { date, anchorMin: minutes, currentMin: minutes };
    const onMove = (move: PointerEvent): void => {
      if (!dragCreate) return;
      dragCreate = { ...dragCreate, currentMin: snapMinutes(minutesFromClientY(dragCreate.date, move.clientY), SNAP_MIN) };
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      const drag = dragCreate;
      dragCreate = null;
      if (!drag) return;
      const start = Math.min(drag.anchorMin, drag.currentMin);
      const end = Math.max(drag.anchorMin, drag.currentMin);
      if (end - start >= SNAP_MIN) {
        suppressColumnClick = Date.now() + 350;
        dispatch("create", { date: drag.date, startMin: start, endMin: Math.max(end, start + SNAP_MIN) });
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  /** 单击空白（两端都有）：默认一小时的日程，弹编辑器 */
  function handleColumnClick(event: MouseEvent, date: string): void {
    if (Date.now() < suppressColumnClick) return;
    const target = event.target as HTMLElement;
    if (target.closest(".cal-event")) return;
    const minutes = snapMinutes(minutesFromClientY(date, event.clientY), SNAP_MIN);
    dispatch("create", { date, startMin: minutes, endMin: Math.min(MINUTES_PER_DAY, minutes + 60) });
  }

  // -------------------------------------------------------------------------
  // 事件块的拖动：边缘改时长 / 中间挪动（桌面鼠标）
  // -------------------------------------------------------------------------

  type BlockDrag = {
    kind: "resize-start" | "resize-end" | "move";
    eventId: string;
    /** 拖动预览的绝对分钟区间；null = 还没越过阈值 */
    preview: { start: number; end: number } | null;
  };

  let blockDrag: BlockDrag | null = null;

  function handleBlockPointerDown(
    event: PointerEvent,
    segment: DaySegment,
    date: string,
    kind: "resize-start" | "resize-end" | "move"
  ): void {
    if (event.button !== 0 || event.pointerType !== "mouse" || $isMobile) return;
    event.stopPropagation();
    const ev = segment.event;
    const { start: startAbs, end: endAbs } = absRange(ev);
    const grabMin = snapMinutes(minutesFromClientY(date, event.clientY), SNAP_MIN);
    const grabAbs = dates.indexOf(date) * MINUTES_PER_DAY + grabMin;
    let preview: { start: number; end: number } | null = null;
    blockDrag = { kind, eventId: ev.id, preview: null };
    event.preventDefault();

    const onMove = (move: PointerEvent): void => {
      const over = columnAt(move.clientX);
      if (!over) return;
      const index = dates.indexOf(over);
      const minutes = snapMinutes(minutesFromClientY(over, move.clientY), SNAP_MIN);
      const abs = index * MINUTES_PER_DAY + minutes;
      let nextStart = startAbs;
      let nextEnd = endAbs;
      if (kind === "move") {
        if (Math.abs(abs - grabAbs) < 4) return;
        const delta = abs - grabAbs;
        nextStart = startAbs + delta;
        nextEnd = endAbs + delta;
      } else if (kind === "resize-start") {
        if (Math.abs(abs - grabAbs) < 4 && preview === null) return;
        nextStart = Math.min(abs, endAbs - SNAP_MIN);
      } else {
        if (Math.abs(abs - grabAbs) < 4 && preview === null) return;
        nextEnd = Math.max(abs, startAbs + SNAP_MIN);
      }
      preview = { start: nextStart, end: nextEnd };
      blockDrag = { kind, eventId: ev.id, preview };
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      const current = blockDrag;
      blockDrag = null;
      // 没拖出预览 = 单击：交给随后的 click 事件派发 open（鼠标与触屏同一条路）
      if (!current || current.eventId !== ev.id || !current.preview) return;
      const range = current.preview;
      const startDate = addDays(gridEpoch, Math.floor(range.start / MINUTES_PER_DAY));
      const startTime = formatMinutes(((range.start % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY);
      const endDay = Math.floor(range.end / MINUTES_PER_DAY);
      const endMinOfDay = ((range.end % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
      // 结束恰好落在午夜：归到当天 00:00 会让区间倒挂，折成前一天结束
      const endDate = endMinOfDay === 0 ? addDays(gridEpoch, endDay - 1) : addDays(gridEpoch, endDay);
      const endTime = endMinOfDay === 0 ? "00:00" : formatMinutes(endMinOfDay);
      suppressColumnClick = Date.now() + 350;
      if (startDate === ev.startDate && startTime === (ev.startTime || "") && endDate === ev.endDate && endTime === (ev.endTime || "")) {
        return;
      }
      dispatch("retimed", { id: ev.id, startDate, startTime, endDate, endTime });
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  /**
   * 一列最终渲染的块：拖动预览时，被拖事件在所有列里都换成「预览区间与本列的交集」
   * （跨列挪动的幽灵块就是这么画出来的），其余块重新参与布局。
   */
  function displayBlocks(column: DayColumn): LayoutBlock[] {
    const drag = blockDrag;
    if (!drag || !drag.preview) return column.blocks;
    const rest = column.blocks.filter((block) => block.event.id !== drag.eventId);
    const colIndex = dates.indexOf(column.date);
    const colStart = colIndex * MINUTES_PER_DAY;
    const colEnd = colStart + MINUTES_PER_DAY;
    if (drag.preview.start >= colEnd || drag.preview.end <= colStart) {
      // 预览已经离开这一列：重排剩下的
      return layoutDaySegments(rest.map(toSegment));
    }
    const draggedEvent = column.blocks.find((block) => block.event.id === drag.eventId)?.event
      ?? events.find((event) => event.id === drag.eventId);
    if (!draggedEvent) return column.blocks;
    const ghost: DaySegment = {
      event: draggedEvent,
      startMin: Math.max(0, drag.preview.start - colStart),
      endMin: Math.min(MINUTES_PER_DAY, drag.preview.end - colStart),
      continuesBefore: false,
      continuesAfter: false
    };
    return layoutDaySegments([...rest.map(toSegment), ghost]);
  }

  function toSegment(block: LayoutBlock): DaySegment {
    return {
      event: block.event,
      startMin: block.startMin,
      endMin: block.endMin,
      continuesBefore: block.continuesBefore,
      continuesAfter: block.continuesAfter
    };
  }

  function isDragging(id: string): boolean {
    return blockDrag?.eventId === id && blockDrag?.preview !== null;
  }

  /** 事件块点击（鼠标与触屏同一条路：拖过动的被 suppressColumnClick 挡掉） */
  function handleBlockClick(event: MouseEvent, segment: DaySegment): void {
    event.stopPropagation();
    if (Date.now() < suppressColumnClick) return;
    dispatch("open", segment.event.id);
  }

  function handleBlockContext(event: MouseEvent, segment: DaySegment): void {
    event.stopPropagation();
    event.preventDefault();
    dispatch("context", { id: segment.event.id, x: event.clientX, y: event.clientY });
  }

  /** 块样式：位置与宽度（重叠簇平分列） */
  function blockStyle(block: LayoutBlock): string {
    const top = (block.startMin / MINUTES_PER_DAY) * totalHeight;
    const height = Math.max(18, ((block.endMin - block.startMin) / MINUTES_PER_DAY) * totalHeight);
    const width = 100 / block.columns;
    const left = width * block.column;
    return `top: ${top}px; height: ${height}px; left: calc(${left}% + 2px); width: calc(${width}% - 4px);`;
  }

  function isPast(event: CalendarEvent): boolean {
    if (!today) return false;
    if (event.endDate < today) return true;
    if (event.endDate > today) return false;
    return minutesOf(event.endTime || "00:00") <= nowMinutes;
  }

  function nowLineStyle(): string {
    return `top: ${(nowMinutes / MINUTES_PER_DAY) * totalHeight}px;`;
  }

  $: gutterStyle = `--cal-hour-h: ${hourHeight}px;`;
</script>

<div class="cal-grid">
  <div class="cal-grid-head" style={`--cal-days: ${dates.length}`}>
    <span class="cal-gutter cal-gutter-head"></span>
    {#each dates as date (date)}
      <button
        type="button"
        class="cal-day-head"
        class:today={date === today}
        title="查看这一天"
        on:click|stopPropagation={() => dispatch("openDay", date)}
      >
        <span class="cal-dow">{["日", "一", "二", "三", "四", "五", "六"][new Date(`${date}T00:00:00`).getDay()]}</span>
        <span class="cal-dom">{Number(date.slice(8, 10))}</span>
      </button>
    {/each}
  </div>

  <div class="cal-allday" style={`--cal-days: ${dates.length}`}>
    <span class="cal-gutter cal-gutter-label">全天</span>
    {#each renderedColumns as column (column.date)}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div
        class="cal-allday-cell"
        on:click|stopPropagation={() => dispatch("createAllDay", { date: column.date })}
      >
        {#each column.allDay as item (item.id)}
          <button
            type="button"
            class="cal-chip"
            class:past={isPast(item)}
            title={`${item.title || "(无标题)"} · ${eventTimeText(item)}`}
            on:click|stopPropagation={() => dispatch("open", item.id)}
            on:contextmenu|preventDefault|stopPropagation={(event) =>
              dispatch("context", { id: item.id, x: event.clientX, y: event.clientY })}
          >
            <span class="cal-chip-title">{item.title || "(无标题)"}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>

  <div class="cal-scroll" bind:this={scrollEl}>
    <div class="cal-grid-body" style={`--cal-days: ${dates.length}; ${gutterStyle}`}>
      <div class="cal-gutter cal-time-gutter">
        {#each hours as hour (hour)}
          <span class="cal-hour-label" style={`height: ${hourHeight}px`}>
            {hour > 0 ? `${`${hour}`.padStart(2, "0")}:00` : ""}
          </span>
        {/each}
      </div>
      {#each renderedColumns as column, columnIndex (column.date)}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div
          class="cal-day-col"
          class:today={column.date === today}
          bind:this={columnEls[columnIndex]}
          on:pointerdown={(event) => handleColumnPointerDown(event, column.date)}
          on:click={(event) => handleColumnClick(event, column.date)}
          on:contextmenu|preventDefault
        >
          {#each hours as hour (hour)}
            <span class="cal-hour-line" style={`top: ${hour * hourHeight}px`}></span>
          {/each}

          {#each column.blocks as block (block.event.id + "-" + block.startMin + "-" + block.column)}
            <button
              type="button"
              class="cal-event"
              class:past={isPast(block.event)}
              class:ghost={isDragging(block.event.id)}
              class:short={block.endMin - block.startMin <= 45}
              style={blockStyle(block)}
              title={`${block.event.title || "(无标题)"} · ${eventTimeText(block.event)}`}
              on:click={(event) => handleBlockClick(event, block)}
              on:contextmenu={(event) => handleBlockContext(event, block)}
              on:pointerdown={(event) => handleBlockPointerDown(event, block, column.date, "move")}
            >
              {#if !block.continuesBefore && block.endMin - block.startMin > 30}
                <span class="cal-event-time">
                  {timeLabel(block.event.startTime) || "00:00"}{#if block.event.endDate === block.event.startDate && block.event.endTime} – {timeLabel(block.event.endTime)}{/if}
                </span>
              {/if}
              <span class="cal-event-title">{block.event.title || "(无标题)"}</span>
              {#if !block.continuesAfter && block.event.startDate === block.event.endDate && !$isMobile}
                <span
                  class="cal-resize cal-resize-top"
                  on:pointerdown={(event) => handleBlockPointerDown(event, block, column.date, "resize-start")}
                ></span>
                <span
                  class="cal-resize cal-resize-bottom"
                  on:pointerdown={(event) => handleBlockPointerDown(event, block, column.date, "resize-end")}
                ></span>
              {/if}
            </button>
          {/each}

          {#if dragCreate?.date === column.date}
            {@const start = Math.min(dragCreate.anchorMin, dragCreate.currentMin)}
            {@const end = Math.max(dragCreate.anchorMin, dragCreate.currentMin)}
            <div
              class="cal-drag-preview"
              style={`top: ${(start / MINUTES_PER_DAY) * totalHeight}px; height: ${Math.max(12, ((end - start) / MINUTES_PER_DAY) * totalHeight)}px`}
            >
              {formatMinutes(start)} – {formatMinutes(end)}
            </div>
          {/if}

          {#if column.date === today}
            <div class="cal-now" style={nowLineStyle()}></div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

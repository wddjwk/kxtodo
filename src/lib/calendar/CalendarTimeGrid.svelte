<script lang="ts">
  /**
   * 日程的时间网格（周 / 三日 / 日视图共用，v0.9.0；v0.9.1 大改）。
   *
   * 结构与业界日历一致：顶部星期行 → 全天栏 → 可滚动的时间网格。
   * 冲突占位走 `calendar.ts::layoutDaySegments`（重叠簇内平分列）。
   *
   * 交互分两套（对齐飞书/Google 日历）：
   * - 桌面（鼠标）：空白按下拖动 = 框选时间段建日程；单击空白 = 默认一小时；
   *   事件块上下边缘拖动 = 改时长；事件块中间拖动 = 挪动（跨列 = 跨天）；
   *   **拖动全程实时改块的位置与时间文本**（v0.9.0 只在松手后才更新）。
   * - 触屏：单击空白 = 先落一个 30 分钟的**占位块**（15 分钟吸附），拖它的上下边界
   *   改起止，点它本身才进编辑器；单击已有事件 = 打开编辑。拖动手势让给滚动。
   *
   * 格高：一屏刚好 12 小时（`hourHeight = 可视高 / 12 × zoom`）。桌面 Ctrl+滚轮、
   * 移动端双指捏合改 zoom，落在 `settings.calendar.hourZoom`（本机偏好，不进同步）。
   *
   * **坐标换算铁律**：壳上有 `transform: scale(uiScale)`，rect 是视觉像素——
   * 分钟数一律按 `(clientY - rect.top) / rect.height * 1440` 的**比例**算，
   * 比例对缩放不敏感，不需要再除 scale。
   *
   * **响应式铁律**：模板里不许直接调「读了组件状态的函数」（`isPast(block)`、
   * `blockVars(block)`）——Svelte 只扫表达式里**字面出现**的变量，函数体里的依赖
   * 不算，于是拖动预览、分类改色都不重算。一律在 `$: renderedColumns` 里算好，
   * 并把依赖当实参传进去（v0.9.0 的「拖完才显示」就是这个坑）。
   */
  import { createEventDispatcher, onDestroy, onMount, tick } from "svelte";
  import { Repeat } from "@lucide/svelte";
  import {
    addDays, allDayEventsFor, diffDays, eventColors, formatMinutes, layoutDaySegments,
    minutesOf, MINUTES_PER_DAY, segmentForDay, snapMinutes,
    type DaySegment, type LayoutBlock
  } from "../calendar";
  import { isMobile } from "../platform";
  import type { CalendarCategory, CalendarEvent } from "../types";

  export let dates: string[];
  /** 已经按区间展开过的「逐次出现」（重复日程在 `CalendarView` 里摊平） */
  export let events: CalendarEvent[];
  export let today = "";
  /** 当前时刻的分钟数（外部按分钟 tick 驱动，画「现在」红线） */
  export let nowMinutes = 0;
  export let categories: CalendarCategory[] = [];
  /** 日程主题色（没分类的事件用它） */
  export let accent = "#7c5cbf";
  /** 时间网格缩放倍率（settings.calendar.hourZoom） */
  export let zoom = 1;

  const dispatch = createEventDispatcher<{
    create: { date: string; startMin: number; endMin: number };
    createAllDay: { date: string };
    open: { id: string; occurrence: string };
    context: { id: string; x: number; y: number; occurrence: string };
    retimed: { id: string; occurrence: string; startDate: string; startTime: string; endDate: string; endTime: string };
    openDay: string;
    zoom: number;
  }>();

  const SNAP_MIN = 15;
  /** 一屏放几小时（需求：刚好 12 小时） */
  const HOURS_IN_VIEW = 12;
  const ZOOM_MIN = 0.5;
  const ZOOM_MAX = 3;

  let scrollEl: HTMLElement;
  let columnEls: HTMLElement[] = [];
  let viewportHeight = 0;
  let observer: ResizeObserver | null = null;

  /** 手势进行中的临时倍率（松手才落盘，免得一路写设置） */
  let zoomPreview: number | null = null;
  let zoomCommitTimer: number | undefined;

  $: effectiveZoom = zoomPreview ?? zoom;
  $: baseHourHeight = viewportHeight > 0 ? viewportHeight / HOURS_IN_VIEW : 48;
  $: hourHeight = Math.max(22, Math.round(baseHourHeight * effectiveZoom));
  $: totalHeight = hourHeight * 24;
  $: hours = Array.from({ length: 24 }, (_, hour) => hour);
  $: gridEpoch = dates[0] ?? today;
  $: gutterStyle = `--cal-hour-h: ${hourHeight}px;`;

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

  type RenderBlock = LayoutBlock & {
    key: string;
    style: string;
    vars: string;
    past: boolean;
    ghost: boolean;
    short: boolean;
    timeText: string;
  };

  /**
   * 渲染用的列（含拖动预览、分类配色、过去态）。所有会变的输入都必须**字面出现**
   * 在这条语句里：blockDrag（预览）、categories/accent（配色）、today/nowMinutes（过去态）、
   * hourHeight（几何）。
   */
  $: renderedColumns = columns.map((column) => {
    const blocks = displayBlocks(column, blockDrag).map((block): RenderBlock => {
      const height = Math.max(18, ((block.endMin - block.startMin) / MINUTES_PER_DAY) * totalHeight);
      return {
        ...block,
        // key 里带 occurrence：跨天的重复事件在同一列可能出现两次（昨天的实例还压着今天），
        // 只靠 id + startMin 会撞键，键控 each 撞键是未定义行为
        key: `${block.event.id}-${occurrenceOf(block.event)}-${block.startMin}-${block.column}`,
        style: blockStyle(block, totalHeight),
        vars: blockVars(block.event, categories, accent),
        past: isPast(block.event, today, nowMinutes),
        ghost: blockDrag?.eventId === block.event.id && blockDrag.preview !== null,
        short: height < 34,
        timeText: blockTimeText(block)
      };
    });
    return {
      date: column.date,
      blocks,
      allDay: column.allDay.map((item) => ({
        event: item,
        vars: blockVars(item, categories, accent),
        past: isPast(item, today, nowMinutes),
        occurrence: occurrenceOf(item)
      }))
    };
  });

  // 触屏的占位块（v0.9.1）：点空白先落一个块，拖边界改起止，点块本身才进编辑器
  let draft: { date: string; startMin: number; endMin: number } | null = null;
  $: if (draft && !dates.includes(draft.date)) draft = null;
  $: draftStyle = draft
    ? `top: ${(draft.startMin / MINUTES_PER_DAY) * totalHeight}px; height: ${Math.max(
        18,
        ((draft.endMin - draft.startMin) / MINUTES_PER_DAY) * totalHeight
      )}px; --cal-main: ${accent}; --cal-soft: ${eventColors({ categoryId: "" }, categories, accent).soft};`
    : "";

  onMount(() => {
    // 打开时滚到「现在」附近（早上 8 点与当前时刻取早者再往前一小时），
    // 与主流日历一致：既看得到今天剩下的安排，也不从午夜开始。
    void tick().then(scrollToNow);
    if (scrollEl && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => {
        viewportHeight = scrollEl?.clientHeight ?? 0;
      });
      observer.observe(scrollEl);
      viewportHeight = scrollEl.clientHeight;
    }
    return () => {
      observer?.disconnect();
      observer = null;
      window.clearTimeout(zoomCommitTimer);
    };
  });
  onDestroy(() => window.clearTimeout(zoomCommitTimer));

  export function scrollToNow(): void {
    if (!scrollEl) return;
    const anchor = Math.max(0, Math.min(nowMinutes, 8 * 60) - 60);
    scrollEl.scrollTop = (anchor / MINUTES_PER_DAY) * totalHeight;
  }

  /** 清掉触屏占位块（切视图/开编辑器/点别处都由父组件调） */
  export function clearDraft(): void {
    draft = null;
  }

  function clampZoom(value: number): number {
    return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(value * 100) / 100));
  }

  function commitZoom(value: number): void {
    window.clearTimeout(zoomCommitTimer);
    zoomPreview = null;
    if (Math.abs(value - zoom) < 0.005) return;
    dispatch("zoom", value);
  }

  // -------------------------------------------------------------------------
  // 缩放手势：桌面 Ctrl+滚轮 / 移动端双指捏合
  // -------------------------------------------------------------------------

  function handleWheel(event: WheelEvent): void {
    if (!event.ctrlKey) return;
    event.preventDefault();
    const next = clampZoom(effectiveZoom * (event.deltaY < 0 ? 1.12 : 1 / 1.12));
    zoomPreview = next;
    // 连续滚动只写一次设置（防抖 240ms）
    window.clearTimeout(zoomCommitTimer);
    zoomCommitTimer = window.setTimeout(() => commitZoom(next), 240);
  }

  let pinchStart: { distance: number; zoom: number } | null = null;

  function touchDistance(event: TouchEvent): number {
    const a = event.touches[0];
    const b = event.touches[1];
    return Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
  }

  function handleTouchStart(event: TouchEvent): void {
    if (event.touches.length !== 2) {
      pinchStart = null;
      return;
    }
    pinchStart = { distance: touchDistance(event), zoom: effectiveZoom };
  }

  function handleTouchMove(event: TouchEvent): void {
    if (event.touches.length !== 2 || !pinchStart) return;
    // 两指手势必须自己吃掉：交给浏览器就变成整页捏合缩放，还会 pointercancel
    event.preventDefault();
    const distance = touchDistance(event);
    if (pinchStart.distance <= 0) return;
    zoomPreview = clampZoom(pinchStart.zoom * (distance / pinchStart.distance));
  }

  function handleTouchEnd(event: TouchEvent): void {
    if (event.touches.length >= 2 || !pinchStart) return;
    pinchStart = null;
    if (zoomPreview !== null) commitZoom(zoomPreview);
  }

  // -------------------------------------------------------------------------
  // 几何
  // -------------------------------------------------------------------------

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

  /** 重复日程的「这一次」是哪天：展开层写在 occurrence 上，没展开就是开始日期 */
  function occurrenceOf(ev: CalendarEvent): string {
    const withOccurrence = ev as CalendarEvent & { occurrence?: string };
    return withOccurrence.occurrence || ev.startDate;
  }

  // -------------------------------------------------------------------------
  // 空白拖动框选（桌面鼠标）
  // -------------------------------------------------------------------------

  let dragCreate: { date: string; anchorMin: number; currentMin: number } | null = null;
  let suppressColumnClick = 0;
  /** 触屏的点击 slop：滚动过的手指抬起时浏览器仍可能补一记 click */
  let touchAnchor: { x: number; y: number } | null = null;

  function handleColumnPointerDown(event: PointerEvent, date: string): void {
    if (event.pointerType !== "mouse" || $isMobile) {
      touchAnchor = { x: event.clientX, y: event.clientY };
      return;
    }
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest(".cal-event, .cal-draft, .cal-now")) return;
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

  /**
   * 单击空白：
   * - 触屏 = 落一个 30 分钟占位块（15 分钟吸附），不直接进编辑器；
   * - 桌面 = 默认一小时的日程，直接弹编辑器。
   */
  function handleColumnClick(event: MouseEvent, date: string): void {
    if (Date.now() < suppressColumnClick) return;
    if (touchAnchor) {
      const moved = Math.hypot(event.clientX - touchAnchor.x, event.clientY - touchAnchor.y);
      touchAnchor = null;
      if (moved > 10) return;
    }
    const target = event.target as HTMLElement;
    if (target.closest(".cal-event, .cal-draft")) return;
    const minutes = snapMinutes(minutesFromClientY(date, event.clientY), SNAP_MIN);
    if ($isMobile) {
      // 必须吃掉冒泡：`.app-shell` 上有 `on:click={closeOverlays}`，它会调本页的
      // `closeOverlays()` → `grid.clearDraft()`——不 stopPropagation 的话，落占位块的
      // 这一下自己就把刚落的块收掉了（探针实测：draft 赋值后紧接着一轮 draft=null）。
      event.stopPropagation();
      draft = {
        date,
        startMin: Math.max(0, minutes),
        endMin: Math.min(MINUTES_PER_DAY, Math.max(0, minutes) + 30)
      };
      return;
    }
    dispatch("create", { date, startMin: minutes, endMin: Math.min(MINUTES_PER_DAY, minutes + 60) });
  }

  /** 占位块本身被点：带着当前起止进编辑器 */
  function handleDraftClick(event: MouseEvent): void {
    event.stopPropagation();
    touchAnchor = null;
    const current = draft;
    if (!current) return;
    draft = null;
    suppressColumnClick = Date.now() + 350;
    dispatch("create", { date: current.date, startMin: current.startMin, endMin: current.endMin });
  }

  /** 拖占位块的上下边界（触屏与鼠标同一条路；手柄上 touch-action:none 不吃滚动） */
  function handleDraftResize(event: PointerEvent, kind: "start" | "end"): void {
    event.stopPropagation();
    event.preventDefault();
    const current = draft;
    if (!current) return;
    const onMove = (move: PointerEvent): void => {
      const minutes = Math.min(MINUTES_PER_DAY, Math.max(0, snapMinutes(minutesFromClientY(current.date, move.clientY), SNAP_MIN)));
      if (kind === "start") {
        draft = { ...current, startMin: Math.min(minutes, current.endMin - SNAP_MIN) };
      } else {
        draft = { ...current, endMin: Math.max(minutes, current.startMin + SNAP_MIN) };
      }
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      suppressColumnClick = Date.now() + 350;
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
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
      dispatch("retimed", {
        id: ev.id,
        occurrence: occurrenceOf(ev),
        startDate,
        startTime,
        endDate,
        endTime
      });
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  /**
   * 一列最终渲染的块：拖动预览时，被拖事件在所有列里都换成「预览区间与本列的交集」
   * （跨列挪动的幽灵块就是这么画出来的），其余块重新参与布局。
   * `drag` 必须是**参数**而不是函数体里读的 blockDrag——见文件头的响应式铁律。
   */
  function displayBlocks(column: DayColumn, drag: BlockDrag | null): LayoutBlock[] {
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

  /** 事件块点击（鼠标与触屏同一条路：拖过动的被 suppressColumnClick 挡掉） */
  function handleBlockClick(event: MouseEvent, block: RenderBlock): void {
    event.stopPropagation();
    if (touchAnchor) touchAnchor = null;
    if (Date.now() < suppressColumnClick) return;
    draft = null;
    dispatch("open", { id: block.event.id, occurrence: occurrenceOf(block.event) });
  }

  function handleBlockContext(event: MouseEvent, block: RenderBlock): void {
    event.stopPropagation();
    event.preventDefault();
    dispatch("context", {
      id: block.event.id,
      x: event.clientX,
      y: event.clientY,
      occurrence: occurrenceOf(block.event)
    });
  }

  function handleChipClick(event: MouseEvent, item: CalendarEvent): void {
    event.stopPropagation();
    if (touchAnchor) touchAnchor = null;
    dispatch("open", { id: item.id, occurrence: occurrenceOf(item) });
  }

  function handleChipContext(event: MouseEvent, item: CalendarEvent): void {
    event.stopPropagation();
    event.preventDefault();
    dispatch("context", {
      id: item.id,
      x: event.clientX,
      y: event.clientY,
      occurrence: occurrenceOf(item)
    });
  }

  /** 块样式：位置与宽度（重叠簇平分列） */
  function blockStyle(block: LayoutBlock, height: number): string {
    const top = (block.startMin / MINUTES_PER_DAY) * height;
    const blockHeight = Math.max(18, ((block.endMin - block.startMin) / MINUTES_PER_DAY) * height);
    const width = 100 / block.columns;
    const left = width * block.column;
    return `top: ${top}px; height: ${blockHeight}px; left: calc(${left}% + 2px); width: calc(${width}% - 4px);`;
  }

  /** 分类配色 → 块上的 CSS 变量（左轨主色 + 块内淡色） */
  function blockVars(event: CalendarEvent, list: CalendarCategory[], fallback: string): string {
    const colors = eventColors(event, list, fallback);
    return `--cal-main: ${colors.main}; --cal-soft: ${colors.soft}; --cal-line: ${colors.line}; --cal-on-main: ${colors.onMain};`;
  }

  /** 块内的时间文本：按**当天段**算，跨天与拖动预览都对得上 */
  function blockTimeText(block: LayoutBlock): string {
    const start = formatMinutes(block.startMin);
    const end = block.endMin >= MINUTES_PER_DAY ? "24:00" : formatMinutes(block.endMin);
    return `${start} – ${end}`;
  }

  function isPast(event: CalendarEvent, todayDate: string, now: number): boolean {
    if (!todayDate) return false;
    if (event.endDate < todayDate) return true;
    if (event.endDate > todayDate) return false;
    return minutesOf(event.endTime || "00:00") <= now;
  }

  function nowLineStyle(): string {
    return `top: ${(nowMinutes / MINUTES_PER_DAY) * totalHeight}px;`;
  }

  function draftTimeText(current: { startMin: number; endMin: number }): string {
    return `${formatMinutes(current.startMin)} – ${formatMinutes(current.endMin)}`;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="cal-grid"
  class:zooming={zoomPreview !== null || pinchStart !== null}
  on:wheel={handleWheel}
  on:touchstart={handleTouchStart}
  on:touchmove|nonpassive={handleTouchMove}
  on:touchend={handleTouchEnd}
  on:touchcancel={handleTouchEnd}
>
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
        {#each column.allDay as item (item.event.id + item.occurrence)}
          <button
            type="button"
            class="cal-chip"
            class:past={item.past}
            style={item.vars}
            title={`${item.event.title || "(无标题)"} · 全天${item.event.location ? ` · ${item.event.location}` : ""}`}
            on:click|stopPropagation={(event) => handleChipClick(event, item.event)}
            on:contextmenu|preventDefault|stopPropagation={(event) => handleChipContext(event, item.event)}
          >
            {#if item.event.repeat}<span class="cal-repeat" aria-hidden="true"><Repeat size={9} strokeWidth={2.6} /></span>{/if}
            <span class="cal-chip-title">{item.event.title || "(无标题)"}</span>
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

          {#each column.blocks as block (block.key)}
            <button
              type="button"
              class="cal-event"
              class:past={block.past}
              class:ghost={block.ghost}
              class:short={block.short}
              class:recurring={block.event.repeat !== null}
              style={`${block.style}${block.vars}`}
              title={`${block.event.title || "(无标题)"} · ${block.timeText}${block.event.location ? ` · ${block.event.location}` : ""}`}
              on:click={(event) => handleBlockClick(event, block)}
              on:contextmenu={(event) => handleBlockContext(event, block)}
              on:pointerdown={(event) => handleBlockPointerDown(event, block, column.date, "move")}
            >
              {#if !block.continuesBefore}
                <span class="cal-event-time">{block.timeText}</span>
              {/if}
              <span class="cal-event-title">
                {#if block.event.repeat}<span class="cal-repeat" aria-hidden="true"><Repeat size={10} strokeWidth={2.6} /></span>{/if}
                {block.event.title || "(无标题)"}
              </span>
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
              style={`top: ${(start / MINUTES_PER_DAY) * totalHeight}px; height: ${Math.max(12, ((end - start) / MINUTES_PER_DAY) * totalHeight)}px; --cal-main: ${accent}`}
            >
              {formatMinutes(start)} – {formatMinutes(end)}
            </div>
          {/if}

          {#if draft?.date === column.date}
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div class="cal-draft" style={draftStyle} on:click={handleDraftClick} role="presentation">
              <span class="cal-draft-time">{draftTimeText(draft)}</span>
              <span class="cal-draft-hint">点我编辑</span>
              <span class="cal-resize cal-resize-top" on:pointerdown={(event) => handleDraftResize(event, "start")}></span>
              <span class="cal-resize cal-resize-bottom" on:pointerdown={(event) => handleDraftResize(event, "end")}></span>
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

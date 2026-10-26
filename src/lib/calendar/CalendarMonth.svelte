<script lang="ts">
  /**
   * 日程的月视图（v0.9.0；v0.9.1 加分类配色与重复标记）。
   * 每天一个格子，格子里是当天日程的芯片（全天在前、按开始时刻排），
   * 超过上限折叠成「+N 更多」（点了跳到那天的日视图）。
   *
   * 点击语义：
   * - 点日期号 → 切到那天的日视图；
   * - 点格子空白 → 在那天新建**全天**日程（编辑器里可以改成定时）；
   * - 点芯片 → 打开对应日程的编辑器。
   *
   * **响应式铁律**（与 CalendarTimeGrid 同源）：渲染要用的东西一律在 `$:` 里算好，
   * 并且把 `events` / `categories` / `accent` **字面写进那条语句**——模板里调
   * 「读了组件状态的函数」不会随它们变化重算。
   */
  import { createEventDispatcher } from "svelte";
  import { Repeat } from "@lucide/svelte";
  import { calendarWeekdayHeaders, type WeekStart } from "../diary";
  import { eventColors, eventTimeText, monthCellEvents, monthGridDates, monthOf } from "../calendar";
  import type { CalendarCategory, CalendarEvent } from "../types";

  export let anchor = "";
  /** 已按区间展开的「逐次出现」 */
  export let events: CalendarEvent[] = [];
  export let today = "";
  export let weekStart: WeekStart = 1;
  export let categories: CalendarCategory[] = [];
  export let accent = "#7c5cbf";
  /** 每格最多直出的芯片数（其余折叠进「+N 更多」） */
  export let maxChips = 3;

  const dispatch = createEventDispatcher<{
    create: { date: string };
    open: { id: string; occurrence: string };
    context: { id: string; x: number; y: number; occurrence: string };
    openDay: string;
  }>();

  type Cell = {
    date: string;
    dayNumber: number;
    otherMonth: boolean;
    isToday: boolean;
    visible: Array<{ event: CalendarEvent; vars: string; occurrence: string }>;
    hidden: number;
  };

  $: currentMonth = monthOf(anchor);
  $: cells = monthGridDates(anchor, weekStart).map(
    (date): Cell => {
      const items = monthCellEvents(events, date);
      return {
        date,
        dayNumber: Number(date.slice(8, 10)),
        otherMonth: monthOf(date) !== currentMonth,
        isToday: date === today,
        visible: items.slice(0, maxChips).map((event) => ({
          event,
          vars: chipVars(event, categories, accent),
          occurrence: occurrenceOf(event)
        })),
        hidden: Math.max(0, items.length - maxChips)
      };
    }
  );

  function chipVars(event: CalendarEvent, list: CalendarCategory[], fallback: string): string {
    const colors = eventColors(event, list, fallback);
    return `--cal-main: ${colors.main}; --cal-soft: ${colors.soft}; --cal-on-main: ${colors.onMain};`;
  }

  function occurrenceOf(event: CalendarEvent): string {
    const withOccurrence = event as CalendarEvent & { occurrence?: string };
    return withOccurrence.occurrence || event.startDate;
  }

  function open(event: CalendarEvent, occurrence: string): void {
    dispatch("open", { id: event.id, occurrence });
  }

  function context(mouseEvent: MouseEvent, event: CalendarEvent, occurrence: string): void {
    dispatch("context", {
      id: event.id,
      x: mouseEvent.clientX,
      y: mouseEvent.clientY,
      occurrence
    });
  }
</script>

<div class="cal-month">
  <div class="cal-month-head" style={`--cal-week-start: ${weekStart}`}>
    {#each calendarWeekdayHeaders(weekStart) as label (label)}
      <span class="cal-month-head-cell">{label}</span>
    {/each}
  </div>
  <div class="cal-month-grid">
    {#each cells as cell (cell.date)}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div
        class="cal-month-cell"
        class:other-month={cell.otherMonth}
        class:today={cell.isToday}
        on:click|stopPropagation={() => dispatch("create", { date: cell.date })}
        on:contextmenu|preventDefault
        role="presentation"
      >
        <button
          type="button"
          class="cal-month-daynum"
          title="查看这一天"
          on:click|stopPropagation={() => dispatch("openDay", cell.date)}
        >{cell.dayNumber}</button>
        {#each cell.visible as item (item.event.id + item.occurrence)}
          <button
            type="button"
            class="cal-month-chip"
            class:allday={item.event.allDay}
            class:past={item.event.endDate < today}
            style={item.vars}
            title={`${item.event.title || "(无标题)"} · ${eventTimeText(item.event)}`}
            on:click|stopPropagation={() => open(item.event, item.occurrence)}
            on:contextmenu|preventDefault|stopPropagation={(event) => context(event, item.event, item.occurrence)}
          >
            {#if item.event.repeat}<span class="cal-repeat" aria-hidden="true"><Repeat size={9} strokeWidth={2.6} /></span>{/if}
            <span class="cal-month-chip-title">{item.event.title || "(无标题)"}</span>
          </button>
        {/each}
        {#if cell.hidden > 0}
          <button
            type="button"
            class="cal-month-more"
            on:click|stopPropagation={() => dispatch("openDay", cell.date)}
          >+{cell.hidden} 更多</button>
        {/if}
      </div>
    {/each}
  </div>
</div>

<script lang="ts">
  /**
   * 日程的月视图（v0.9.0）：每天一个格子，格子里是当天日程的芯片
   * （全天在前、按开始时刻排），超过上限折叠成「+N 更多」（点了跳到那天的日视图）。
   *
   * 点击语义（与需求 b 对齐）：
   * - 点日期号 → 切到那天的日视图；
   * - 点格子空白 → 在那天新建**全天**日程（编辑器里可以改成定时）；
   * - 点芯片 → 打开对应日程的编辑器。
   */
  import { createEventDispatcher } from "svelte";
  import { calendarWeekdayHeaders, type WeekStart } from "../diary";
  import { eventTimeText, monthCellEvents, monthGridDates, monthOf, timeLabel } from "../calendar";
  import type { CalendarEvent } from "../types";

  export let anchor = "";
  export let events: CalendarEvent[] = [];
  export let today = "";
  export let weekStart: WeekStart = 1;
  /** 每格最多直出的芯片数（其余折叠进「+N 更多」） */
  export let maxChips = 3;

  const dispatch = createEventDispatcher<{
    create: { date: string };
    open: string;
    context: { id: string; x: number; y: number };
    openDay: string;
  }>();

  $: cells = monthGridDates(anchor, weekStart);
  $: currentMonth = monthOf(anchor);
</script>

<div class="cal-month">
  <div class="cal-month-head" style={`--cal-week-start: ${weekStart}`}>
    {#each calendarWeekdayHeaders(weekStart) as label (label)}
      <span class="cal-month-head-cell">{label}</span>
    {/each}
  </div>
  <div class="cal-month-grid">
    {#each cells as date (date)}
      <!-- 依赖必须显式写在模板表达式里：{@const} 里调包装函数会把 events 藏进闭包，
           键控 each 的块就不随 events 变化重算（月格芯片不更新的根因） -->
      {@const items = monthCellEvents(events, date)}
      {@const visible = items.slice(0, maxChips)}
      {@const hidden = items.length - visible.length}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div
        class="cal-month-cell"
        class:other-month={monthOf(date) !== currentMonth}
        class:today={date === today}
        on:click|stopPropagation={() => dispatch("create", { date })}
        on:contextmenu|preventDefault
        role="presentation"
      >
        <button
          type="button"
          class="cal-month-daynum"
          title="查看这一天"
          on:click|stopPropagation={() => dispatch("openDay", date)}
        >{Number(date.slice(8, 10))}</button>
        {#each visible as item (item.id)}
          <button
            type="button"
            class="cal-month-chip"
            class:allday={item.allDay}
            class:past={item.endDate < today}
            title={`${item.title || "(无标题)"} · ${eventTimeText(item)}`}
            on:click|stopPropagation={() => dispatch("open", item.id)}
            on:contextmenu|preventDefault|stopPropagation={(event) =>
              dispatch("context", { id: item.id, x: event.clientX, y: event.clientY })}
          >
            {#if !item.allDay}<span class="cal-month-chip-time">{timeLabel(item.startTime) || "00:00"}</span>{/if}
            <span class="cal-month-chip-title">{item.title || "(无标题)"}</span>
          </button>
        {/each}
        {#if hidden > 0}
          <button
            type="button"
            class="cal-month-more"
            on:click|stopPropagation={() => dispatch("openDay", date)}
          >+{hidden} 更多</button>
        {/if}
      </div>
    {/each}
  </div>
</div>

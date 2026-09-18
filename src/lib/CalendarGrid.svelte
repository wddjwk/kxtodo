<script lang="ts">
  /**
   * 日历网格：全应用**唯一**的一份日网格实现。
   *
   * `DatePicker`（日记 / 记账 / 两个编辑器）与「日期与提醒」面板都建立在它上面——
   * 两边各写一份的话，周起始口径、翻月手感与返回键的「先退子面板」行为迟早分叉
   * （v0.8.1 才刚把周起始统一成 `stores.weekStart` 一个来源）。
   *
   * 头部的年月也可以点：就地换成月/年网格，选完回到日网格，**不派发 select**
   * （翻月只是看，不是选）。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import MonthPicker from "./MonthPicker.svelte";
  import { calendarWeekdayHeaders } from "./diary";
  import { weekStart } from "./stores";

  /** 选中的日期 YYYY-MM-DD（空 = 没有选中） */
  export let value = "";
  /** 初始展示哪个月（空 = 选中日所在月，再空 = 本月） */
  export let initial = "";

  const dispatch = createEventDispatcher<{ select: string }>();

  // 一周从周几开始跟着设置走（默认周一），与日历视图同一个来源
  $: weekDayLabels = calendarWeekdayHeaders($weekStart);

  function isoOf(year: number, month: number, day: number): string {
    const date = new Date(year, month, day);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  }

  function todayIso(): string {
    const now = new Date();
    return isoOf(now.getFullYear(), now.getMonth(), now.getDate());
  }

  // 只在挂载时定一次初始月份：之后 value 变化（比如面板里选了别的日子）不该把
  // 用户翻到的月份拽回去。
  const seed = new Date(`${value || initial || todayIso()}T00:00:00`);
  let viewYear = Number.isNaN(seed.getTime()) ? new Date().getFullYear() : seed.getFullYear();
  let viewMonth = Number.isNaN(seed.getTime()) ? new Date().getMonth() : seed.getMonth();
  let panel: "days" | "months" = "days";

  $: cells = (() => {
    const first = new Date(viewYear, viewMonth, 1);
    const last = new Date(viewYear, viewMonth + 1, 0);
    const startDow = (first.getDay() - $weekStart + 7) % 7;
    const totalDays = last.getDate();
    const out: Array<{ date: string; day: number; current: boolean }> = [];
    const prevLast = new Date(viewYear, viewMonth, 0);
    for (let index = startDow - 1; index >= 0; index--) {
      const day = prevLast.getDate() - index;
      out.push({ date: isoOf(viewYear, viewMonth - 1, day), day, current: false });
    }
    for (let day = 1; day <= totalDays; day++) {
      out.push({ date: isoOf(viewYear, viewMonth, day), day, current: true });
    }
    const trailing = (7 - (out.length % 7)) % 7;
    for (let day = 1; day <= trailing; day++) {
      out.push({ date: isoOf(viewYear, viewMonth + 1, day), day, current: false });
    }
    return out;
  })();

  function prev(): void {
    if (viewMonth === 0) {
      viewYear--;
      viewMonth = 11;
    } else viewMonth--;
  }

  function next(): void {
    if (viewMonth === 11) {
      viewYear++;
      viewMonth = 0;
    } else viewMonth++;
  }

  function pick(date: string): void {
    dispatch("select", date);
  }

  function pickMonth(event: CustomEvent<{ year: number; month: number }>): void {
    viewYear = event.detail.year;
    viewMonth = event.detail.month;
    panel = "days";
  }

  /**
   * 返回键：月/年网格是这一层里的子面板，先退回日网格。
   * 回 false = 这一层没得退，交给宿主（浮层该关了）。
   */
  export function back(): boolean {
    if (panel !== "days") {
      panel = "days";
      return true;
    }
    return false;
  }

  /** 把网格翻到某个日期所在的月（「今天」按钮用；选中日在别的月份时不翻会看不见）。 */
  export function reveal(date: string): void {
    const target = new Date(`${date}T00:00:00`);
    if (Number.isNaN(target.getTime())) return;
    viewYear = target.getFullYear();
    viewMonth = target.getMonth();
    panel = "days";
  }
</script>

<div class="calendar-grid-panel">
  <div class="date-picker-header">
    <button type="button" on:click={prev} aria-label="上个月"><ChevronLeft size={16} /></button>
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
    <span
      class="dp-title pickable"
      role="button"
      tabindex="0"
      title="点击直接选年月"
      on:click={() => { panel = panel === "days" ? "months" : "days"; }}
    >{viewYear}年{viewMonth + 1}月</span>
    <button type="button" on:click={next} aria-label="下个月"><ChevronRight size={16} /></button>
  </div>

  {#if panel === "months"}
    <MonthPicker year={viewYear} month={viewMonth} on:select={pickMonth} />
  {:else}
    <div class="date-picker-grid">
      {#each weekDayLabels as label}
        <span class="dp-head">{label}</span>
      {/each}
      {#each cells as cell}
        <button
          type="button"
          class="dp-cell"
          class:other-month={!cell.current}
          class:today={cell.date === todayIso()}
          class:selected={value === cell.date}
          on:click={() => pick(cell.date)}
        >{cell.day}</button>
      {/each}
    </div>
  {/if}
</div>

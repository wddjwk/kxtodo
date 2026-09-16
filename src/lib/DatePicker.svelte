<script lang="ts">
  /**
   * 日历选择器：全应用唯一的一个（记账、日记、任务卡、两个编辑器的日期浮层都用它）。
   *
   * `withTime` 打开时在日历与「清除 / 今天」之间插一行「时钟图标 + 18:19」，点它把
   * 日网格换成双列滚轮选到分钟——记账的 `time`、日记 `createdAt` 的时钟部分、任务的
   * `dueTime` 都靠这一行。**换面板而不是往下追加**：浮层高度因此恒定，锚在底部的
   * 记账浮层不会因为展开滚轮而顶出屏幕。
   *
   * 头部的年月也可以点：就地换成月/年网格，选完回到日网格，不派发 select
   * （翻月只是看，不是选）。
   */
  import { createEventDispatcher, onDestroy } from "svelte";
  import { ChevronLeft, ChevronRight, Clock } from "@lucide/svelte";
  import TimePicker from "./TimePicker.svelte";
  import MonthPicker from "./MonthPicker.svelte";
  import { formatClock, nowClock, parseClock } from "./clock";
  import { calendarWeekdayHeaders } from "./diary";
  import { weekStart } from "./stores";
  import { createBackGuard } from "./platform";

  export let value = "";
  /** HH:MM（或 HH:MM:SS，只取前两段）；空 = 没有时刻 */
  export let time = "";
  export let withTime = false;

  const dispatch = createEventDispatcher<{ select: string; selectTime: string; clear: void; close: void }>();

  // 一周从周几开始跟着设置走（默认周一），与日历视图同一个来源
  $: weekDayLabels = calendarWeekdayHeaders($weekStart);

  function todayIso(): string {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }

  function isoOf(y: number, m: number, d: number): string {
    const dt = new Date(y, m, d);
    return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  }

  const initial = value ? new Date(value + "T00:00:00") : new Date();
  let viewYear = initial.getFullYear();
  let viewMonth = initial.getMonth();
  let panel: "days" | "months" | "time" = "days";

  $: cells = (() => {
    const first = new Date(viewYear, viewMonth, 1);
    const last = new Date(viewYear, viewMonth + 1, 0);
    const startDow = (first.getDay() - $weekStart + 7) % 7;
    const totalDays = last.getDate();
    const out: Array<{ date: string; day: number; current: boolean }> = [];
    const prevLast = new Date(viewYear, viewMonth, 0);
    for (let i = startDow - 1; i >= 0; i--) {
      const dd = prevLast.getDate() - i;
      out.push({ date: isoOf(viewYear, viewMonth - 1, dd), day: dd, current: false });
    }
    for (let dd = 1; dd <= totalDays; dd++) {
      out.push({ date: isoOf(viewYear, viewMonth, dd), day: dd, current: true });
    }
    const rem = (7 - (out.length % 7)) % 7;
    for (let dd = 1; dd <= rem; dd++) {
      out.push({ date: isoOf(viewYear, viewMonth + 1, dd), day: dd, current: false });
    }
    return out;
  })();

  // 返回键：**先退面板再退浮层**——月/年网格与时刻滚轮都是这一层里的子面板，
  // 一按就整个收掉会让人以为选错了地方。
  const backGuard = createBackGuard();
  $: backGuard(true, () => {
    if (panel !== "days") panel = "days";
    else dispatch("close");
  });
  onDestroy(() => backGuard.dispose());

  /** 有没有指定时刻（勾选框的状态、时刻按钮能不能点都看它） */
  $: hasTime = parseClock(time) !== null;
  $: clock = parseClock(time);
  /** 没有存过时刻就显示当前时刻（只是显示：core 侧新建时本来就按当前时刻落盘） */
  $: clockLabel = clock ? formatClock(clock.hour, clock.minute) : nowClock();
  // 关掉时刻之后面板不该停在滚轮上（回去也没得拨）
  $: if (!hasTime && panel === "time") {
    panel = "days";
  }

  function prev(): void {
    if (viewMonth === 0) { viewYear--; viewMonth = 11; } else viewMonth--;
  }
  function next(): void {
    if (viewMonth === 11) { viewYear++; viewMonth = 0; } else viewMonth++;
  }
  function pick(date: string): void {
    dispatch("select", date);
  }
  function goToday(): void {
    const t = todayIso();
    const d = new Date(t + "T00:00:00");
    viewYear = d.getFullYear();
    viewMonth = d.getMonth();
    dispatch("select", t);
  }
  function pickMonth(event: CustomEvent<{ year: number; month: number }>): void {
    viewYear = event.detail.year;
    viewMonth = event.detail.month;
    panel = "days";
  }
  function handleTimeChange(event: CustomEvent<{ hour: number; minute: number }>): void {
    dispatch("selectTime", formatClock(event.detail.hour, event.detail.minute));
  }

  /**
   * 「要不要指定时刻」的开关（v0.8.1）。默认状态看 `time` 有没有值：
   * 存过时刻就是开着的，没存过就是关的、整行置灰点不动。
   *
   * 打开时**默认就是此刻**——用户勾上这一下的意思就是「这条要精确到分钟」，
   * 此刻是最省事的初值；之后他拨到的值会一路留在浮层与卡片上，拨到哪就是哪。
   * 关掉就把时刻值清空（宿主会把 `dueTime` 一并清掉）。
   */
  function toggleTimeNext(): void {
    if (hasTime) {
      dispatch("selectTime", "");
      return;
    }
    const now = new Date();
    dispatch("selectTime", formatClock(now.getHours(), now.getMinutes()));
  }
</script>

<div class="date-picker" on:click|stopPropagation on:mousedown|stopPropagation>
  {#if panel === "time"}
    <div class="date-picker-header">
      <button type="button" on:click={() => (panel = "days")} aria-label="返回日历"><ChevronLeft size={16} /></button>
      <span class="dp-title">选择时刻</span>
      <span class="dp-head-spacer" aria-hidden="true"></span>
    </div>
    <div class="dp-time-wheel">
      <TimePicker
        hour={clock?.hour ?? new Date().getHours()}
        minute={clock?.minute ?? new Date().getMinutes()}
        on:change={handleTimeChange}
      />
    </div>
  {:else}
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

    {#if withTime}
      <div class="date-picker-time">
        <label class="dp-time-switch" title="要不要精确到分钟">
          <input type="checkbox" checked={hasTime} on:change={toggleTimeNext} />
        </label>
        <button
          type="button"
          class="dp-time-trigger"
          class:disabled={!hasTime}
          title="选到分钟"
          disabled={!hasTime}
          on:click={() => (panel = "time")}
        >
          <Clock size={15} />
          <strong>{clockLabel}</strong>
          <span class="dp-time-caret" aria-hidden="true"></span>
        </button>
      </div>
    {/if}
  {/if}

  <div class="date-picker-actions">
    <button type="button" class="dp-clear" on:click={() => dispatch("clear")}>清除</button>
    <button type="button" class="dp-today" on:click={goToday}>今天</button>
  </div>
</div>

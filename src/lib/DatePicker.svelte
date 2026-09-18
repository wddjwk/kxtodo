<script lang="ts">
  /**
   * 日历选择器：全应用唯一的一个（记账、日记、两个编辑器的日期浮层都用它）。
   *
   * 三层拼装：`CalendarGrid`（日网格 + 年月翻页）+ `withTime` 时多出来的
   * 「时钟图标 + 18:19」一行 + 「清除 / 今天」。
   *
   * `withTime` 那一行点下去是**换面板而不是往下追加**（日网格换成双列滚轮）：
   * 浮层高度因此恒定，锚在底部的记账浮层不会因为展开滚轮而顶出屏幕。
   *
   * 任务卡片的「日期与提醒」不走这里——那是 `TaskDateReminderPanel`，
   * 但它复用同一个 `CalendarGrid`，所以日历部分的手感与口径完全一致。
   */
  import { createEventDispatcher, onDestroy } from "svelte";
  import { ChevronLeft, Clock } from "@lucide/svelte";
  import CalendarGrid from "./CalendarGrid.svelte";
  import TimePicker from "./TimePicker.svelte";
  import { formatClock, nowClock, parseClock } from "./clock";
  import { createBackGuard } from "./platform";

  export let value = "";
  /** HH:MM（或 HH:MM:SS，只取前两段）；空 = 没有时刻 */
  export let time = "";
  export let withTime = false;

  const dispatch = createEventDispatcher<{ select: string; selectTime: string; clear: void; close: void }>();

  let grid: CalendarGrid;
  let panel: "days" | "time" = "days";

  function todayIso(): string {
    const now = new Date();
    return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  }

  // 返回键：**先退面板再退浮层**——年月网格与时刻滚轮都是这一层里的子面板，
  // 一按就整个收掉会让人以为选错了地方。
  const backGuard = createBackGuard();
  $: backGuard(true, () => {
    if (panel === "time") panel = "days";
    else if (!grid?.back()) dispatch("close");
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

  function goToday(): void {
    const today = todayIso();
    grid?.reveal(today);
    dispatch("select", today);
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
    <CalendarGrid bind:this={grid} {value} on:select={(event) => dispatch("select", event.detail)} />

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

<script lang="ts">
  /**
   * 习惯统计面板（v0.9.3）：累计 / 当前连续 / 最长连续 / 本月 + 近 30 天完成率 +
   * 月历热力图（打卡日实心、计划内未打空心、非计划日淡点、今天描边）。
   * 骨架与分类面板同一套 editor-overlay + calendar-sheet；返回键/Esc/遮罩现成。
   */
  import { CalendarDays, CheckCircle2, ChevronLeft, ChevronRight, Flame, Pencil, Pause, Play, Trophy, Trash2, X } from "@lucide/svelte";
  import { showToast, todayIso, weekStart } from "../../stores";
  import { deleteHabit, updateHabit } from "../../actions";
  import { freqLabel, HABIT_WEEKDAY_LABELS, checkinCount, checkinProgress, isComplete, monthGrid, scheduledOn, habitStats, timeLabel } from "../../habits";
  import { ledgerIcon, softColor } from "../../ledgerIcons";
  import { createBackGuard } from "../../platform";
  import { imeInset } from "../../imeInset";
  import { suppressGhostClick } from "../../ghostClick";
  import type { Habit } from "../../types";

  export let habit: Habit;
  export let onClose: () => void = () => {};
  export let onEdit: () => void = () => {};

  let month = todayIso().slice(0, 7);
  let confirmDelete = false;
  let busy = false;

  $: today = todayIso();
  $: stats = habitStats(habit, today);
  $: cells = monthGrid(month, $weekStart);
  $: weekdayLabels = [...HABIT_WEEKDAY_LABELS.slice($weekStart), ...HABIT_WEEKDAY_LABELS.slice(0, $weekStart)];
  $: main = habit.color || "var(--accent)";
  $: soft = habit.color ? softColor(habit.color, 0.16) : "rgb(0 0 0 / 5%)";
  $: monthTitle = `${Number(month.slice(0, 4))}年${Number(month.slice(5, 7))}月`;

  const backGuard = createBackGuard();
  $: backGuard(true, onClose);

  function stepMonth(delta: number): void {
    const [y, m] = month.split("-").map(Number);
    const base = new Date(y, m - 1 + delta, 1);
    const pad = (n: number): string => String(n).padStart(2, "0");
    month = `${base.getFullYear()}-${pad(base.getMonth() + 1)}`;
  }

  async function toggleStatus(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      const next = habit.status === "active" ? "paused" : "active";
      const ok = await updateHabit(habit.id, { status: next });
      if (ok) showToast(next === "paused" ? `「${habit.name}」已暂停` : `「${habit.name}」已恢复`, 1800);
    } finally {
      busy = false;
    }
  }

  async function remove(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      const ok = await deleteHabit(habit.id);
      if (ok) {
        showToast(`习惯「${habit.name}」已删除`, 1800);
        onClose();
      }
    } finally {
      busy = false;
    }
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="editor-overlay" use:imeInset on:pointerdown={handleBackdropPointerDown}>
  <div
    class="editor-dialog calendar-sheet habit-sheet"
    role="dialog"
    aria-label="习惯统计"
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="calendar-sheet-head habit-stats-head">
      <span class="habit-card-icon" style={`--habit-main: ${main}; --habit-soft: ${soft}`}>
        <svelte:component this={ledgerIcon(habit.icon, "CalendarDays")} size={20} />
      </span>
      <span class="habit-stats-title">
        <strong>{habit.name}</strong>
        <span>{freqLabel(habit.freq)} · {timeLabel(habit.time)}</span>
      </span>
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="editor-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
        <X size={17} />
      </button>
    </header>

    <div class="calendar-sheet-body">
      <div class="habit-stats-grid">
        <div class="habit-stat">
          <strong>{stats.total}<small>次</small></strong>
          <span><CheckCircle2 size={12} />累计打卡</span>
        </div>
        <div class="habit-stat">
          <strong>{stats.current}<small>天</small></strong>
          <span><Flame size={12} />当前连续</span>
        </div>
        <div class="habit-stat">
          <strong>{stats.best}<small>天</small></strong>
          <span><Trophy size={12} />最长连续</span>
        </div>
        <div class="habit-stat">
          <strong>{stats.month}<small>次</small></strong>
          <span><CalendarDays size={12} />本月打卡</span>
        </div>
      </div>

      <div class="habit-rate">
        <span class="habit-rate-label">近 30 天完成率</span>
        <span class="habit-rate-bar"><span class="habit-rate-fill" style={`width: ${Math.round(stats.rate * 100)}%; background: ${main}`}></span></span>
        <strong class="habit-rate-num">{Math.round(stats.rate * 100)}%</strong>
      </div>

      <div class="habit-heat">
        <header class="habit-heat-head">
          <button type="button" class="editor-icon-button" title="上一月" aria-label="上一月" on:click={() => stepMonth(-1)}>
            <ChevronLeft size={15} />
          </button>
          <strong>{monthTitle}</strong>
          <button type="button" class="editor-icon-button" title="下一月" aria-label="下一月" on:click={() => stepMonth(1)}>
            <ChevronRight size={15} />
          </button>
        </header>
        <div class="habit-heat-week">
          {#each weekdayLabels as label (label)}
            <span>{label}</span>
          {/each}
        </div>
        <div class="habit-heat-grid" style={`--habit-main: ${main}; --habit-soft: ${soft}`}>
          {#each cells as date, index (index)}
            {#if date}
              <span
                class="habit-heat-cell"
                class:checked={isComplete(habit, date)}
                class:partial={checkinCount(habit, date) > 0 && !isComplete(habit, date)}
                class:due={scheduledOn(habit, date)}
                class:today={date === today}
                style={`--habit-fill: ${checkinProgress(habit, date).ratio * 360}deg`}
                title={checkinCount(habit, date) > 0 ? `${date} 已打卡 ${checkinCount(habit, date)}/${checkinProgress(habit, date).target}` : scheduledOn(habit, date) ? `${date} 未打卡` : date}
              >{Number(date.slice(8, 10))}</span>
            {:else}
              <span class="habit-heat-cell blank"></span>
            {/if}
          {/each}
        </div>
      </div>
    </div>

    <footer class="calendar-sheet-foot">
      {#if confirmDelete}
        <button type="button" class="settings-button danger" disabled={busy} on:click={() => void remove()}>
          <Trash2 size={15} />确认删除
        </button>
        <button type="button" class="settings-button" on:click={() => (confirmDelete = false)}>取消</button>
      {:else}
        <button type="button" class="settings-button danger" disabled={busy} on:click={() => (confirmDelete = true)}>
          <Trash2 size={15} />删除
        </button>
      {/if}
      <span class="calendar-sheet-spacer"></span>
      {#if habit.status !== "done"}
        <button type="button" class="settings-button" disabled={busy} on:click={() => void toggleStatus()}>
          {#if habit.status === "active"}<Pause size={15} />暂停{:else}<Play size={15} />恢复{/if}
        </button>
      {/if}
      <button type="button" class="settings-button primary" on:click={onEdit}><Pencil size={15} />编辑</button>
    </footer>
  </div>
</div>
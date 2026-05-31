<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";

  export let value = "";

  const dispatch = createEventDispatcher<{ select: string; clear: void; close: void }>();

  const weekDayLabels = ["日", "一", "二", "三", "四", "五", "六"];

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

  $: cells = (() => {
    const first = new Date(viewYear, viewMonth, 1);
    const last = new Date(viewYear, viewMonth + 1, 0);
    const startDow = first.getDay();
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
</script>

<div class="date-picker" on:click|stopPropagation on:mousedown|stopPropagation>
  <div class="date-picker-header">
    <button type="button" on:click={prev} aria-label="上个月"><ChevronLeft size={16} /></button>
    <span>{viewYear}年{viewMonth + 1}月</span>
    <button type="button" on:click={next} aria-label="下个月"><ChevronRight size={16} /></button>
  </div>
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
  <div class="date-picker-actions">
    <button type="button" class="dp-today" on:click={goToday}>今天</button>
    <button type="button" class="dp-clear" on:click={() => dispatch("clear")}>清除</button>
  </div>
</div>

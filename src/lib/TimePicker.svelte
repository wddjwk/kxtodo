<script lang="ts">
  /**
   * 双列滚轮时刻选择器（时 0-23 / 分 0-59）。用原生滚动 + CSS scroll-snap 停位，
   * 滚动停下来后按 scrollTop 反推选中项——不做 transform 模拟惯性，触屏上原生手感已经够。
   * 行高必须与样式里的 `.time-cell` 高度一致，否则反推出来的下标会漂。
   */
  import { createEventDispatcher, onMount, tick } from "svelte";

  export let hour: number;
  export let minute: number;

  const dispatch = createEventDispatcher<{ change: Clock }>();

  const ROW = 34;
  const VISIBLE = 5;
  const PAD = ((VISIBLE - 1) / 2) * ROW;
  const HEIGHT = VISIBLE * ROW;

  const hours = Array.from({ length: 24 }, (_, index) => index);
  const minutes = Array.from({ length: 60 }, (_, index) => index);

  let hourEl: HTMLDivElement;
  let minuteEl: HTMLDivElement;
  let syncing = false;
  let settled: ReturnType<typeof setTimeout> | undefined;

  interface Clock { hour: number; minute: number }

  function emit(): void {
    dispatch("change", { hour, minute });
  }

  /** 把两列滚到当前值；程序化滚动会触发 scroll，用 syncing 挡住别把它当成用户操作。 */
  function place(smooth = false): void {
    syncing = true;
    const behavior: ScrollBehavior = smooth ? "smooth" : "auto";
    hourEl?.scrollTo({ top: hour * ROW, behavior });
    minuteEl?.scrollTo({ top: minute * ROW, behavior });
    window.setTimeout(() => {
      syncing = false;
    }, smooth ? 320 : 60);
  }

  function handleScroll(which: "hour" | "minute"): void {
    if (syncing) return;
    if (settled) clearTimeout(settled);
    settled = window.setTimeout(() => {
      const el = which === "hour" ? hourEl : minuteEl;
      if (!el) return;
      const limit = which === "hour" ? 23 : 59;
      const value = Math.min(limit, Math.max(0, Math.round(el.scrollTop / ROW)));
      if (which === "hour" ? value === hour : value === minute) return;
      if (which === "hour") hour = value;
      else minute = value;
      emit();
    }, 90);
  }

  function jump(which: "hour" | "minute", value: number): void {
    if (which === "hour") hour = value;
    else minute = value;
    place(true);
    emit();
  }

  onMount(() => {
    void tick().then(() => place());
    return () => {
      if (settled) clearTimeout(settled);
    };
  });
</script>

<div class="time-picker" style="--time-rows: {VISIBLE}; --time-pad: {PAD}px; --time-height: {HEIGHT}px">
  <div class="time-band" aria-hidden="true"></div>
  <div class="time-col" bind:this={hourEl} on:scroll={() => handleScroll("hour")} role="listbox" aria-label="时">
    {#each hours as value (value)}
      <button
        type="button"
        class="time-cell"
        class:active={value === hour}
        role="option"
        aria-selected={value === hour}
        on:click={() => jump("hour", value)}
      >{String(value).padStart(2, "0")}</button>
    {/each}
  </div>
  <span class="time-sep" aria-hidden="true">:</span>
  <div class="time-col" bind:this={minuteEl} on:scroll={() => handleScroll("minute")} role="listbox" aria-label="分">
    {#each minutes as value (value)}
      <button
        type="button"
        class="time-cell"
        class:active={value === minute}
        role="option"
        aria-selected={value === minute}
        on:click={() => jump("minute", value)}
      >{String(value).padStart(2, "0")}</button>
    {/each}
  </div>
</div>

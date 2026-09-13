<script lang="ts">
  /**
   * 年月选择器：年份步进 + 一页 12 格（月模式是 12 个月，年模式是 12 个年份）。
   * 各处「‹ 2026年9月 ›」的年月标签点开来就是它——左右箭头只能一个月一个月挪，
   * 翻到去年这时候要点十几下。
   *
   * 只管「选了哪个年月」，不管自己怎么浮出来：调用方要么把它内联进日历
   * （DatePicker 的头部），要么包一层 `position: fixed` 的面板（anchoredPopoverStyle）。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";

  export let year: number;
  /** 0-11 */
  export let month = 0;
  export let mode: "month" | "year" = "month";

  const dispatch = createEventDispatcher<{ select: { year: number; month: number } }>();

  const PAGE = 12;
  const now = new Date();

  let viewYear = year;
  $: pageStart = mode === "year" ? Math.floor(viewYear / PAGE) * PAGE : viewYear;
  $: monthCells = Array.from({ length: PAGE }, (_, index) => index);
  $: yearCells = Array.from({ length: PAGE }, (_, index) => pageStart + index);

  function prevPage(): void {
    viewYear = mode === "year" ? pageStart - PAGE : viewYear - 1;
  }
  function nextPage(): void {
    viewYear = mode === "year" ? pageStart + PAGE : viewYear + 1;
  }
  function pickMonth(index: number): void {
    dispatch("select", { year: viewYear, month: index });
  }
  function pickYear(value: number): void {
    dispatch("select", { year: value, month });
  }
  function pickNow(): void {
    dispatch("select", { year: now.getFullYear(), month: now.getMonth() });
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="month-picker" on:click|stopPropagation on:mousedown|stopPropagation>
  <div class="month-picker-header">
    <button type="button" on:click={prevPage} aria-label="上一年"><ChevronLeft size={16} /></button>
    <span>{mode === "year" ? `${pageStart} - ${pageStart + PAGE - 1}` : `${viewYear}年`}</span>
    <button type="button" on:click={nextPage} aria-label="下一年"><ChevronRight size={16} /></button>
  </div>

  {#if mode === "month"}
    <div class="month-picker-grid">
      {#each monthCells as index (index)}
        <button
          type="button"
          class="mp-cell"
          class:selected={index === month && viewYear === year}
          on:click={() => pickMonth(index)}
        >{index + 1}月</button>
      {/each}
    </div>
  {:else}
    <div class="month-picker-grid years">
      {#each yearCells as value (value)}
        <button
          type="button"
          class="mp-cell"
          class:selected={value === year}
          on:click={() => pickYear(value)}
        >{value}</button>
      {/each}
    </div>
  {/if}

  <div class="month-picker-actions">
    <button type="button" class="mp-now" on:click={pickNow}>{mode === "year" ? "今年" : "本月"}</button>
  </div>
</div>

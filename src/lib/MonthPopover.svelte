<script lang="ts">
  /**
   * 年月选择浮层的壳：定位、点外面关、Esc 关、滚动/缩放时跟着锚点重算。
   * 各处「‹ 2026年9月 ›」的年月标签只管当触发器（保留自己的样式与语义），
   * 浮层这一层统一走这里，省得四个调用点各写一遍。
   */
  import { onMount } from "svelte";
  import { createBackGuard } from "./platform";
  import MonthPicker from "./MonthPicker.svelte";
  import { anchoredPopoverStyle } from "./popover";
  import { appSettings } from "./stores";
  import { uiScaleValue } from "./styles";

  export let open = false;
  export let anchor: HTMLElement | undefined = undefined;
  export let year: number;
  export let month = 0;
  export let mode: "month" | "year" = "month";
  export let onSelect: (next: { year: number; month: number }) => void = () => {};
  export let onClose: () => void = () => {};

  // 月份/年份浮层：返回键先收它（`open` 是 prop，收放都靠这一条）
  const backGuard = createBackGuard();
  $: backGuard(open, onClose);

  const WIDTH = 240;
  const HEIGHT = 258;

  let style = "";

  function place(): void {
    style = anchoredPopoverStyle(anchor, uiScaleValue($appSettings.appearance.uiScale), WIDTH, HEIGHT);
  }

  function handlePointerDown(event: PointerEvent): void {
    if (!open) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".month-pop") || (anchor && target?.closest(".month-pop-anchor"))) return;
    if (anchor && (target === anchor || anchor.contains(target))) return;
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!open || event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  onMount(() => {
    window.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  });

  $: if (open) place();
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="month-pop" style={style} on:click|stopPropagation on:pointerdown|stopPropagation>
    <MonthPicker {year} {month} {mode} on:select={(event) => { onSelect(event.detail); onClose(); }} />
  </div>
{/if}

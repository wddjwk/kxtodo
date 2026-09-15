<script lang="ts">
  /**
   * 总资产趋势的放大查看（入口只有一个：趋势卡片右上角的全屏按钮）。
   * 桌面 = 居中浮窗，浮窗里的按钮再铺满窗口；移动端 = 直接旋转 90° 横屏全屏
   * （竖屏手机看长趋势图最舒服的姿势）——卡片本身已经能悬浮/点按读数，
   * 放大视图不再充当第二层「看内容」的中间态。
   * 关闭走三路：X / Esc / 安卓返回键（addBackInterceptor），遮罩点空白也算。
   */
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { Maximize2, Minimize2, X } from "@lucide/svelte";
  import { addBackInterceptor, isMobile } from "../platform";
  import { suppressGhostClick } from "../ghostClick";
  import { appSettings } from "../stores";
  import { uiScaleValue } from "../styles";
  import AssetsTrend from "./AssetsTrend.svelte";
  import type { AssetTrendPoint } from "../ledger";

  export let points: AssetTrendPoint[] = [];
  export let onClose: () => void = () => {};

  /** 移动端：进来就是旋转 90° 的横屏全屏——卡片上的全屏按钮是唯一入口，
   *  再拉一层半屏浮窗就是多此一举（卡片自己已经能读数了）。
   *  桌面：居中浮窗，浮窗里的按钮再铺满窗口（真正的全屏）。 */
  let full = get(isMobile);
  /** 桌面：false = 居中浮窗，true = 铺满窗口 */
  let expanded = false;
  let fullEl: HTMLDivElement;
  /** 移动端旋转层的尺寸（布局像素）：量遮罩的视觉矩形再除回缩放 */
  let rotStyle = "";

  function close(at?: { x: number; y: number }): void {
    if (at) suppressGhostClick(at);
    onClose();
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    close({ x: event.clientX, y: event.clientY });
  }

  function measure(): void {
    if (!fullEl) return;
    const rect = fullEl.getBoundingClientRect();
    const scale = uiScaleValue($appSettings.appearance.uiScale) || 1;
    // app-shell 带 transform 缩放：rect 是视觉像素，写进样式前除回逻辑像素
    const width = Math.round(rect.height / scale);
    const height = Math.round(rect.width / scale);
    rotStyle = `width: ${width}px; height: ${height}px;`;
  }

  /** Esc / 返回键：两种形态都直接收掉（移动端没有中间态可退） */
  function stepBack(): void {
    close();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    event.stopPropagation();
    stepBack();
  }

  onMount(() => {
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("resize", measure);
    const release = addBackInterceptor(() => {
      stepBack();
      return true;
    });
    return () => {
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("resize", measure);
      release();
    };
  });

  // 进全屏时现量尺寸：浮层可能刚挂上，量早了是 0
  $: if (full && fullEl) {
    measure();
  }
</script>

{#if full}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ledger-trend-full" bind:this={fullEl} on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
    <div
      class="ledger-trend-rot"
      style={rotStyle}
      on:pointerdown|stopPropagation
      on:click|stopPropagation
    >
      <div class="ledger-trend-rot-head">
        <strong>总资产趋势</strong>
        <button type="button" class="ledger-image-tool" title="关闭" aria-label="关闭" on:click={() => close()}>
          <X size={17} />
        </button>
      </div>
      <div class="ledger-trend-rot-body">
        <AssetsTrend {points} axes interactive axisFont={14} />
      </div>
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="editor-overlay ledger-overlay" on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
    <div
      class="editor-dialog ledger-sheet ledger-trend-dialog"
      class:expanded={expanded && !$isMobile}
      role="dialog"
      aria-label="总资产趋势"
      tabindex="-1"
      on:pointerdown|stopPropagation
      on:click|stopPropagation
    >
      <header class="ledger-sheet-head">
        <span class="ledger-sheet-title">总资产趋势</span>
        <div class="ledger-sheet-actions">
          <button
            type="button"
            class="ledger-icon-button"
            title={expanded ? "退出全屏" : "全屏查看"}
            aria-label={expanded ? "退出全屏" : "全屏查看"}
            on:click={() => (expanded = !expanded)}
          >
            {#if expanded}<Minimize2 size={17} />{:else}<Maximize2 size={17} />{/if}
          </button>
          <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={() => close()}>
            <X size={18} />
          </button>
        </div>
      </header>
      <div class="ledger-trend-dialog-body">
        <AssetsTrend {points} axes interactive axisFont={12} />
      </div>
    </div>
  </div>
{/if}

<script lang="ts">
  /**
   * 总资产趋势的放大查看：桌面 = 居中浮窗（悬浮读数），移动端 = 横屏全屏
   * （整个图表旋转 90°，宽高对调铺满屏幕——竖屏手机看长趋势图的唯一舒服姿势）。
   * 关闭走三路：X / Esc / 安卓返回键（addBackInterceptor），遮罩点空白也算。
   */
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import { addBackInterceptor, isMobile } from "../platform";
  import { suppressGhostClick } from "../ghostClick";
  import { appSettings } from "../stores";
  import { uiScaleValue } from "../styles";
  import AssetsTrend from "./AssetsTrend.svelte";
  import type { AssetTrendPoint } from "../ledger";

  export let points: AssetTrendPoint[] = [];
  export let onClose: () => void = () => {};

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

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    event.stopPropagation();
    close();
  }

  onMount(() => {
    window.addEventListener("keydown", handleKeydown, true);
    if ($isMobile) {
      measure();
      window.addEventListener("resize", measure);
    }
    const release = addBackInterceptor(() => {
      close();
      return true;
    });
    return () => {
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("resize", measure);
      release();
    };
  });
</script>

{#if $isMobile}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ledger-trend-full" bind:this={fullEl} on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
    <div class="ledger-trend-rot" style={rotStyle} on:pointerdown|stopPropagation on:click|stopPropagation>
      <div class="ledger-trend-rot-head">
        <strong>总资产趋势</strong>
        <button type="button" class="ledger-image-tool" title="关闭" aria-label="关闭" on:click={() => close()}>
          <X size={17} />
        </button>
      </div>
      <div class="ledger-trend-rot-body">
        <AssetsTrend {points} axes interactive />
      </div>
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="editor-overlay ledger-overlay" on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
    <div
      class="editor-dialog ledger-sheet ledger-trend-dialog"
      role="dialog"
      aria-label="总资产趋势"
      tabindex="-1"
      on:pointerdown|stopPropagation
      on:click|stopPropagation
    >
      <header class="ledger-sheet-head">
        <span class="ledger-sheet-title">总资产趋势</span>
        <div class="ledger-sheet-actions">
          <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={() => close()}>
            <X size={18} />
          </button>
        </div>
      </header>
      <div class="ledger-trend-dialog-body">
        <AssetsTrend {points} axes interactive />
      </div>
    </div>
  </div>
{/if}

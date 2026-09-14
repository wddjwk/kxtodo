<script lang="ts">
  /**
   * 总资产趋势的放大查看：**两段式**——点卡片先给「看得清内容」的放大视图
   * （桌面居中浮窗、移动端竖屏浮层，都带坐标轴与读数），浮窗里的全屏按钮才是
   * 真·全屏（桌面铺满窗口、移动端整层旋转 90° 横屏——竖屏手机看长趋势图最舒服的姿势）。
   * 早前移动端点一下就直接横屏全屏，用户还没看清就被转了屏，所以要分两段。
   * 关闭走三路：X / Esc / 安卓返回键（addBackInterceptor），遮罩点空白也算；
   * 全屏态下 Esc 与返回键先退全屏，再退浮窗（与两层浮层同一套两段式语义）。
   */
  import { onMount } from "svelte";
  import { Maximize2, Minimize2, X } from "@lucide/svelte";
  import { addBackInterceptor, isMobile } from "../platform";
  import { suppressGhostClick } from "../ghostClick";
  import { appSettings } from "../stores";
  import { uiScaleValue } from "../styles";
  import AssetsTrend from "./AssetsTrend.svelte";
  import type { AssetTrendPoint } from "../ledger";

  export let points: AssetTrendPoint[] = [];
  export let onClose: () => void = () => {};

  /** 移动端：false = 竖屏浮层（内容视图），true = 旋转 90° 的横屏全屏 */
  let full = false;
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

  /** Esc / 返回键：全屏态先退全屏，浮层还留着 */
  function stepBack(): void {
    if (full) {
      full = false;
      return;
    }
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
        <button type="button" class="ledger-image-tool" title="退出全屏" aria-label="退出全屏" on:click={() => (full = false)}>
          <Minimize2 size={17} />
        </button>
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
            title={$isMobile ? "全屏查看（横屏）" : expanded ? "退出全屏" : "全屏查看"}
            aria-label={$isMobile ? "全屏查看" : expanded ? "退出全屏" : "全屏查看"}
            on:click={() => ($isMobile ? (full = true) : (expanded = !expanded))}
          >
            {#if expanded && !$isMobile}<Minimize2 size={17} />{:else}<Maximize2 size={17} />{/if}
          </button>
          <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={() => close()}>
            <X size={18} />
          </button>
        </div>
      </header>
      <div class="ledger-trend-dialog-body">
        <AssetsTrend {points} axes interactive axisFont={$isMobile ? 16 : 12} />
      </div>
    </div>
  </div>
{/if}

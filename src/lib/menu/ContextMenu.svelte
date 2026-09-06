<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ChevronLeft } from "@lucide/svelte";
  import { appSettings } from "../stores";
  import { isMobile } from "../platform";
  import { uiScaleValue } from "../styles";
  import { openSubmenus, requestSubmenuClose } from "./submenu";

  /** 触发点坐标（clientX/clientY，屏幕像素）。 */
  export let x = 0;
  export let y = 0;
  /** x 锚点对齐方式：left = 菜单左缘贴 x；right = 菜单右缘贴 x。 */
  export let xAlign: "left" | "right" = "left";
  export let minWidth = 232;
  export let onClose: () => void = () => {};

  let menuEl: HTMLElement;
  let left = -9999;
  let top = -9999;
  let maxHeight = 0;
  let minWidthPx = minWidth;
  let ready = false;
  let lastSubOpen = false;

  /** 有子菜单打开着：移动端据此把菜单变成钻入式（一级隐藏，二级占据菜单位置） */
  $: subOpen = $openSubmenus > 0;
  // 子菜单开合后菜单的尺寸完全变了（移动端一级被藏起来、二级顶上来），
  // 原来收敛好的位置可能已经超出屏幕，必须重新量一次。桌面端子菜单是绝对定位的
  // 浮出面板，不改变根菜单尺寸，不必重排。
  $: if (subOpen !== lastSubOpen) {
    lastSubOpen = subOpen;
    if (isMobile) void layout();
  }

  function goBack(): void {
    requestSubmenuClose();
  }

  /** 视口边缘保留的逻辑像素边距。 */
  const MENU_MARGIN_PX = 8;

  /**
   * 跟手定位：调用方传视口像素（clientX/Y 或长按触点），这里统一除以 uiScale
   * 换算成缩放 shell 内的逻辑坐标（只除一次，调用方不做换算）。
   * 优先落在锚点右下；放不下时贴边收敛，下方溢出则向上翻转；
   * 菜单比可用高度还高时限高并内部滚动。
   */
  async function layout(): Promise<void> {
    await tick();
    if (!menuEl) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const viewWidth = window.innerWidth / scale;
    const viewHeight = window.innerHeight / scale;
    const rect = menuEl.getBoundingClientRect();
    const width = rect.width / scale;
    // 内容高度取 rect 与 scrollHeight 的较大者：菜单自己带着 inline max-height 时，
    // rect 量到的只是被夹住的高度，重新收敛（子菜单开合）时就发现不了溢出。
    const height = Math.max(rect.height / scale, menuEl.scrollHeight / scale);
    const anchorX = x / scale;
    const anchorY = y / scale;
    // 逻辑视口可能比固定 minWidth 还窄（移动端高缩放），先收敛宽度再定位。
    minWidthPx = Math.min(minWidth, Math.max(160, viewWidth - MENU_MARGIN_PX * 2));
    const anchorLeft = xAlign === "right" ? anchorX - width : anchorX;
    left = Math.max(MENU_MARGIN_PX, Math.min(anchorLeft, viewWidth - width - MENU_MARGIN_PX));

    const availableHeight = viewHeight - MENU_MARGIN_PX * 2;
    if (height > availableHeight) {
      maxHeight = Math.max(120, Math.round(availableHeight));
      top = MENU_MARGIN_PX;
    } else {
      maxHeight = 0;
      if (anchorY + height > viewHeight - MENU_MARGIN_PX) {
        const flippedTop = anchorY - height;
        top = flippedTop >= MENU_MARGIN_PX ? flippedTop : viewHeight - height - MENU_MARGIN_PX;
      } else {
        top = anchorY;
      }
    }
    ready = true;
  }

  function isInside(target: EventTarget | null): boolean {
    return target instanceof Node && Boolean(menuEl?.contains(target));
  }

  function handlePointerDown(event: PointerEvent): void {
    if (!isInside(event.target)) onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    // 输入法组合期间（候选键/确认键）不响应菜单键盘导航，避免抢焦点打断输入。
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    const items = focusableItems();
    if (items.length === 0) return;
    const activeIndex = items.findIndex((item) => item === document.activeElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      const next = activeIndex < 0 ? 0 : (activeIndex + delta + items.length) % items.length;
      items[next]?.focus({ preventScroll: true });
    } else if (event.key === "Home") {
      event.preventDefault();
      items[0]?.focus({ preventScroll: true });
    } else if (event.key === "End") {
      event.preventDefault();
      items[items.length - 1]?.focus({ preventScroll: true });
    }
  }

  function focusableItems(): HTMLElement[] {
    if (!menuEl) return [];
    return [...menuEl.querySelectorAll<HTMLElement>("[data-menu-item]:not([disabled])")];
  }

  /** 按钮等元素阻止 mousedown 默认聚焦（防 focus-scroll 滚动祖先误关菜单），
   * click 时再以 preventScroll 补焦点；输入类控件必须保留原生聚焦——子面板
   * 会 stopPropagation 吞掉 click，根级补焦 handler 收不到事件。输入框的
   * focus-scroll 由 inputActivity 时间窗守卫兜底。 */
  function handleMenuMouseDown(event: MouseEvent): void {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, textarea, select")) return;
    event.preventDefault();
  }

  function handleMenuClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    const focusable = target?.closest<HTMLElement>("button, input, [tabindex]:not([tabindex='-1'])");
    focusable?.focus({ preventScroll: true });
  }

  // 输入框 focus / 输入法 compositionend 时浏览器会对输入框自动
  // scrollIntoView，滚动 DOM 祖先并触发 scroll 事件——这是“输入中文菜单就
  // 消失”的根源。记录输入活动时刻，短窗口内的 scroll 视为自动滚动，不关菜单。
  let inputActivityAt = 0;

  function markInputActivity(): void {
    inputActivityAt = performance.now();
  }

  function handleScroll(event: Event): void {
    if (performance.now() - inputActivityAt < 150) return;
    if (!isInside(event.target)) onClose();
  }

  let blurCloseTimer: number | undefined;

  // 输入法候选窗等系统浮层会瞬时甚至持续抢走窗口焦点；菜单内有聚焦元素
  // （正在输入）时绝不因 blur 关闭，其余情况延迟确认窗口真的失焦才关。
  function handleWindowBlur(): void {
    const active = document.activeElement;
    if (active && menuEl?.contains(active)) return;
    window.clearTimeout(blurCloseTimer);
    blurCloseTimer = window.setTimeout(() => {
      if (!document.hasFocus()) onClose();
    }, 180);
  }

  function handleWindowFocus(): void {
    window.clearTimeout(blurCloseTimer);
  }

  onMount(() => {
    void layout();
    window.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("blur", handleWindowBlur);
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener("resize", onClose);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("blur", handleWindowBlur);
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("scroll", handleScroll, true);
      window.clearTimeout(blurCloseTimer);
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={menuEl}
  class="context-menu"
  class:capped={maxHeight > 0}
  class:sub-open={subOpen}
  role="menu"
  tabindex="-1"
  style={`left: ${left}px; top: ${top}px; min-width: ${minWidthPx}px;${maxHeight ? ` max-height: ${maxHeight}px; overflow-y: auto;` : ""} visibility: ${ready ? "visible" : "hidden"};`}
  on:mousedown={handleMenuMouseDown}
  on:click|stopPropagation={handleMenuClick}
  on:focusin={markInputActivity}
  on:compositionend={markInputActivity}
  on:contextmenu|preventDefault|stopPropagation
>
  {#if subOpen && isMobile}
    <button class="menu-item menu-item-button submenu-back" type="button" data-menu-item on:click={goBack}>
      <ChevronLeft size={15} />
      <span class="menu-item-label">返回</span>
    </button>
  {/if}
  <slot />
</div>

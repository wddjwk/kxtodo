<script lang="ts">
  import { onMount, tick } from "svelte";
  import { appSettings } from "../stores";
  import { uiScaleValue } from "../styles";

  /** 触发点坐标（clientX/clientY，屏幕像素）。 */
  export let x = 0;
  export let y = 0;
  export let minWidth = 232;
  export let onClose: () => void = () => {};

  let menuEl: HTMLElement;
  let left = -9999;
  let top = -9999;
  let ready = false;

  /** 跟手定位：先渲染测量，再收敛到视口内（超出右/下边界时自动翻转）。 */
  async function layout(): Promise<void> {
    await tick();
    if (!menuEl) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const viewWidth = window.innerWidth / scale;
    const viewHeight = window.innerHeight / scale;
    const rect = menuEl.getBoundingClientRect();
    const width = rect.width / scale;
    const height = rect.height / scale;
    left = Math.max(8, Math.min(x / scale, viewWidth - width - 8));
    top = Math.max(8, Math.min(y / scale, viewHeight - height - 8));
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
  role="menu"
  tabindex="-1"
  style={`left: ${left}px; top: ${top}px; min-width: ${minWidth}px; visibility: ${ready ? "visible" : "hidden"};`}
  on:mousedown={handleMenuMouseDown}
  on:click|stopPropagation={handleMenuClick}
  on:focusin={markInputActivity}
  on:compositionend={markInputActivity}
  on:contextmenu|preventDefault|stopPropagation
>
  <slot />
</div>

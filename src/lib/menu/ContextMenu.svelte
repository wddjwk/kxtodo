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
      items[next]?.focus();
    } else if (event.key === "Home") {
      event.preventDefault();
      items[0]?.focus();
    } else if (event.key === "End") {
      event.preventDefault();
      items[items.length - 1]?.focus();
    }
  }

  function focusableItems(): HTMLElement[] {
    if (!menuEl) return [];
    return [...menuEl.querySelectorAll<HTMLElement>("[data-menu-item]:not([disabled])")];
  }

  function handleScroll(event: Event): void {
    if (!isInside(event.target)) onClose();
  }

  onMount(() => {
    void layout();
    window.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("scroll", handleScroll, true);
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
  on:click|stopPropagation
  on:contextmenu|preventDefault|stopPropagation
>
  <slot />
</div>

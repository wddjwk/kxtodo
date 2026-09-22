<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { ChevronRight } from "@lucide/svelte";
  import { submenuClosed, submenuOpened, submenuBack } from "./submenu";
  import type { Component } from "svelte";

  export let icon: Component | null = null;
  export let label = "";
  export let danger = false;
  export let disabled = false;
  export let active = false;
  /**
   * 选择类菜单项（勾选住在**最左侧的定宽槽位**里，v0.8.7 追加需求 2）。
   * 槽位对每一项都常驻（未选中只留空格）：勾在行尾时，选中项一换、最宽的那一项跟着换，
   * 菜单宽度就会跳一下；定宽槽位让所有项等宽，勾选怎么换都不动宽度。
   */
  export let checkable = false;
  export let onSelect: () => void = () => {};

  let submenuOpen = false;
  let submenuEl: HTMLElement;
  let itemEl: HTMLElement;
  let flipX = false;
  let flipY = false;
  /** 子菜单离视口边缘至少留这么多（视觉像素） */
  const MENU_EDGE_MARGIN = 8;
  /** 计数幂等：收起会被 onDestroy、点击别处、移动端「返回」多条路径触发，不能重复减 */
  let counted = false;

  $: hasSubmenu = Boolean($$slots.submenu);
  $: submenuClass = `submenu-panel${flipX ? " flip-x" : ""}${flipY ? " flip-y" : ""}`;

  function openSubmenu(): void {
    submenuOpen = true;
    if (!counted) {
      counted = true;
      submenuOpened();
    }
    window.addEventListener("click", handleDocumentClick, true);
    void adjustSubmenu();
  }

  function closeSubmenu(): void {
    submenuOpen = false;
    if (counted) {
      counted = false;
      submenuClosed();
    }
    window.removeEventListener("click", handleDocumentClick, true);
  }

  /** 点击菜单内其它位置（根菜单对 click stopPropagation，故用捕获阶段）时收起本子菜单。 */
  function handleDocumentClick(event: MouseEvent): void {
    const target = event.target as Node | null;
    if (target && itemEl?.contains(target)) return;
    closeSubmenu();
  }

  /**
   * 子菜单贴右缘展开；超出视口右/下缘时翻转。
   * 移动端不翻转——那里是钻入式（一级隐藏，二级占据菜单位置），见 mobile.css。
   *
   * 翻转口径与 `popover.ts` 的四边钳制同一套纪律（v0.8.6 需求 4 顺带收口）：
   * 翻转只能解决「右缘放不下」，窄视口下翻到左侧仍可能越左缘——再用 margin 补回来。
   * 量到的 rect 是视觉像素（壳上有 `transform: scale`），margin 要除回逻辑像素。
   */
  async function adjustSubmenu(): Promise<void> {
    await tick();
    if (!submenuEl) return;
    flipX = false;
    flipY = false;
    submenuEl.style.marginLeft = "";
    await tick();
    const rect = submenuEl.getBoundingClientRect();
    flipX = rect.right > window.innerWidth - 4;
    flipY = rect.bottom > window.innerHeight - 4;
    await tick();
    // 限高菜单里子菜单改行内手风琴（静态定位），没有越界问题，别去动它的 margin
    if (getComputedStyle(submenuEl).position !== "absolute") return;
    const shifted = submenuEl.getBoundingClientRect();
    const overflowLeft = MENU_EDGE_MARGIN - shifted.left;
    if (overflowLeft <= 0) return;
    const logicalWidth = submenuEl.offsetWidth;
    const scale = logicalWidth > 0 ? shifted.width / logicalWidth : 1;
    submenuEl.style.marginLeft = `${Math.round(overflowLeft / scale)}px`;
  }

  function handleClick(): void {
    if (hasSubmenu) {
      if (submenuOpen) {
        closeSubmenu();
      } else {
        openSubmenu();
      }
      return;
    }
    onSelect();
  }

  onMount(() =>
    // 移动端二级面板上的「返回」：收起自己，一级菜单随即重新显示
    submenuBack.subscribe((count) => {
      if (count > 0 && submenuOpen) closeSubmenu();
    })
  );

  onDestroy(closeSubmenu);
</script>

{#if hasSubmenu}
  <div
    bind:this={itemEl}
    class="menu-item has-submenu"
    role="none"
  >
    <button
      type="button"
      class="menu-item-button"
      data-menu-item
      {disabled}
      on:click={handleClick}
    >
      {#if icon}<svelte:component this={icon} size={15} />{/if}
      <span class="menu-item-label">{label}</span>
      <ChevronRight size={14} class="submenu-chevron" />
    </button>
    {#if submenuOpen}
      <div bind:this={submenuEl} class={submenuClass} role="menu">
        <slot name="submenu" />
      </div>
    {/if}
  </div>
{:else}
  <button
    type="button"
    class="menu-item menu-item-button"
    class:danger
    class:active
    data-menu-item
    {disabled}
    on:click={handleClick}
  >
    {#if checkable}<span class="menu-item-check" class:on={active}>✓</span>{/if}
    {#if icon}<svelte:component this={icon} size={15} />{/if}
    <span class="menu-item-label">{label}</span>
  </button>
{/if}

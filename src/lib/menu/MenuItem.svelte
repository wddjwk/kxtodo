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
  export let onSelect: () => void = () => {};

  let submenuOpen = false;
  let submenuEl: HTMLElement;
  let itemEl: HTMLElement;
  let flipX = false;
  let flipY = false;
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

  /** 子菜单贴右缘展开；超出视口右/下缘时翻转。
   * 移动端不翻转——那里是钻入式（一级隐藏，二级占据菜单位置），见 mobile.css。 */
  async function adjustSubmenu(): Promise<void> {
    await tick();
    if (!submenuEl) return;
    flipX = false;
    flipY = false;
    await tick();
    const rect = submenuEl.getBoundingClientRect();
    flipX = rect.right > window.innerWidth - 4;
    flipY = rect.bottom > window.innerHeight - 4;
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
    {#if icon}<svelte:component this={icon} size={15} />{/if}
    <span class="menu-item-label">{label}</span>
    {#if active}<span class="menu-item-check">✓</span>{/if}
  </button>
{/if}

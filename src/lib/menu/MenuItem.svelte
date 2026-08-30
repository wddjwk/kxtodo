<script lang="ts">
  import { tick } from "svelte";
  import { ChevronRight } from "@lucide/svelte";
  import type { Component } from "svelte";

  export let icon: Component | null = null;
  export let label = "";
  export let danger = false;
  export let disabled = false;
  export let active = false;
  export let onSelect: () => void = () => {};

  let submenuOpen = false;
  let submenuEl: HTMLElement;
  let flipX = false;
  let flipY = false;
  let openTimer: number | undefined;
  let closeTimer: number | undefined;

  $: hasSubmenu = Boolean($$slots.submenu);
  $: submenuClass = `submenu-panel${flipX ? " flip-x" : ""}${flipY ? " flip-y" : ""}`;

  function scheduleOpen(): void {
    window.clearTimeout(closeTimer);
    openTimer = window.setTimeout(() => {
      submenuOpen = true;
      void adjustSubmenu();
    }, 200);
  }

  function scheduleClose(): void {
    window.clearTimeout(openTimer);
    closeTimer = window.setTimeout(() => {
      submenuOpen = false;
    }, 160);
  }

  function openNow(): void {
    window.clearTimeout(closeTimer);
    window.clearTimeout(openTimer);
    submenuOpen = true;
    void adjustSubmenu();
  }

  /** 子菜单贴右缘展开；超出视口右/下缘时翻转。 */
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
        submenuOpen = false;
      } else {
        openNow();
      }
      return;
    }
    onSelect();
  }
</script>

{#if hasSubmenu}
  <div
    class="menu-item has-submenu"
    role="none"
    on:pointerenter={scheduleOpen}
    on:pointerleave={scheduleClose}
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

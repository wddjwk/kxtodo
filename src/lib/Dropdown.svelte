<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { Check, ChevronDown } from "@lucide/svelte";

  export let value: string;
  export let options: Array<{ value: string; label: string }> = [];
  export let ariaLabel = "";

  const dispatch = createEventDispatcher<{ change: string }>();

  let open = false;
  let flipUp = false;
  let menuEl: HTMLElement;

  $: current = options.find((o) => o.value === value);

  /** 打开后测量菜单高度，超出窗口下缘则向上展开。 */
  async function adjustPosition(): Promise<void> {
    await tick();
    if (!menuEl) return;
    flipUp = false;
    await tick();
    const rect = menuEl.getBoundingClientRect();
    flipUp = rect.bottom > window.innerHeight - 6 && rect.top > rect.height;
  }

  function toggle(): void {
    open = !open;
    if (open) void adjustPosition();
  }

  function choose(v: string): void {
    open = false;
    if (v !== value) dispatch("change", v);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (open && event.key === "Escape") {
      event.preventDefault();
      open = false;
    }
  }
</script>

<svelte:window on:click={() => (open = false)} on:keydown={handleKeydown} />

<div class="dropdown" class:open>
  <button
    type="button"
    class="dropdown-trigger"
    aria-label={ariaLabel}
    aria-expanded={open}
    on:click|stopPropagation={toggle}
  >
    <span>{current?.label ?? ""}</span>
    <ChevronDown size={15} />
  </button>
  {#if open}
    <div bind:this={menuEl} class="dropdown-menu" class:flip-up={flipUp} role="listbox" on:click|stopPropagation>
      {#each options as option (option.value)}
        <button type="button" role="option" aria-selected={option.value === value} class:selected={option.value === value} on:click={() => choose(option.value)}>
          <span class="dropdown-check">
            {#if option.value === value}<Check size={14} strokeWidth={3} />{/if}
          </span>
          {option.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { Check, ChevronDown } from "@lucide/svelte";

  export let value: string;
  export let options: Array<{ value: string; label: string }> = [];
  export let ariaLabel = "";

  const dispatch = createEventDispatcher<{ change: string }>();

  let open = false;

  $: current = options.find((o) => o.value === value);

  function toggle(): void {
    open = !open;
  }

  function choose(v: string): void {
    open = false;
    if (v !== value) dispatch("change", v);
  }
</script>

<svelte:window on:click={() => (open = false)} />

<div class="dropdown" class:open>
  <button
    type="button"
    class="dropdown-trigger"
    aria-label={ariaLabel}
    on:click|stopPropagation={toggle}
  >
    <span>{current?.label ?? ""}</span>
    <ChevronDown size={15} />
  </button>
  {#if open}
    <div class="dropdown-menu" on:click|stopPropagation>
      {#each options as option (option.value)}
        <button type="button" class:selected={option.value === value} on:click={() => choose(option.value)}>
          <span class="dropdown-check">
            {#if option.value === value}<Check size={14} strokeWidth={3} />{/if}
          </span>
          {option.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

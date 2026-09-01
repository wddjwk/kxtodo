<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { ChevronRight } from "@lucide/svelte";
  import MenuItem from "./MenuItem.svelte";
  import type { AppNode } from "../types";

  export let nodes: AppNode[] = [];
  export let parentId: string | null = null;
  export let currentEntryId: string;
  export let level = 0;

  const dispatch = createEventDispatcher<{ move: string }>();

  let expandedCategories = new Set<string>();

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  // 含有可移动条目的分类才显示；空分类折叠后没有内容，展示出来只会造成困惑。
  $: eligibleCategoryIds = (() => {
    const ids = new Set<string>();
    for (const node of nodes) {
      if (node.kind !== "entry" || node.id === currentEntryId) continue;
      let current: AppNode | undefined = nodes.find((item) => item.id === node.id);
      while (current?.parentId) {
        ids.add(current.parentId);
        current = nodes.find((item) => item.id === current?.parentId);
      }
    }
    return ids;
  })();

  function toggleCategory(id: string): void {
    const next = new Set(expandedCategories);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    expandedCategories = next;
  }
</script>

{#each children as node (node.id)}
  {#if node.kind === "category" && eligibleCategoryIds.has(node.id)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <button
      type="button"
      class="menu-item menu-item-button move-category-row"
      style={`padding-left: ${10 + level * 14}px`}
      on:click|stopPropagation={() => toggleCategory(node.id)}
    >
      <ChevronRight
        size={14}
        class={`move-chevron${expandedCategories.has(node.id) ? " open" : ""}`}
      />
      <span class="menu-item-label">{node.name}</span>
    </button>
    {#if expandedCategories.has(node.id)}
      <svelte:self
        {nodes}
        parentId={node.id}
        {currentEntryId}
        level={level + 1}
        on:move={(event) => dispatch("move", event.detail)}
      />
    {/if}
  {:else if node.kind === "entry" && node.id !== currentEntryId}
    <div style={`padding-left: ${level * 14}px`}>
      <MenuItem label={node.name} onSelect={() => dispatch("move", node.id)} />
    </div>
  {/if}
{/each}

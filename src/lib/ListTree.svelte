<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { ChevronDown, Folder } from "@lucide/svelte";
  import type { AppNode } from "./types";
  import IconGlyph from "./IconGlyph.svelte";

  export let nodes: AppNode[] = [];
  export let parentId: string | null = null;
  export let selectedNodeId = "";
  export let counts: Record<string, number> = {};
  export let level = 0;
  export let renamingId: string | null = null;
  export let renameDraft = "";

  const dispatch = createEventDispatcher<{
    selectEntry: string;
    toggleCategory: string;
    renameInput: string;
    renameCommit: string;
    openMenu: { id: string; x: number; y: number };
    dragStart: string;
    dropNode: { id: string; targetId: string };
  }>();

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  function rowStyle(levelValue: number): string {
    return `padding-left: ${levelValue * 18 + 10}px;`;
  }

  function handleClick(node: AppNode): void {
    if (node.kind === "category") {
      dispatch("toggleCategory", node.id);
    } else {
      dispatch("selectEntry", node.id);
    }
  }

  function openMenu(event: MouseEvent, node: AppNode): void {
    event.preventDefault();
    event.stopPropagation();
    dispatch("openMenu", { id: node.id, x: event.clientX, y: event.clientY });
  }

  function handleDrop(event: DragEvent, target: AppNode): void {
    event.preventDefault();
    event.stopPropagation();
    if (target.kind !== "category") {
      return;
    }
    const id = event.dataTransfer?.getData("text/plain") || "";
    if (!id || id === target.id) {
      return;
    }
    dispatch("dropNode", { id, targetId: target.id });
  }
</script>

{#each children as node (node.id)}
  <div
    class:selected={node.id === selectedNodeId}
    class:category={node.kind === "category"}
    class="tree-row"
    style={rowStyle(level)}
    draggable={node.kind !== "system"}
    on:click={() => handleClick(node)}
    on:contextmenu={(event) => openMenu(event, node)}
    on:dragstart={(event) => {
      event.dataTransfer?.setData("text/plain", node.id);
      dispatch("dragStart", node.id);
    }}
    on:dragend={() => dispatch("dragStart", "")}
    on:dragover={(event) => node.kind === "category" && event.preventDefault()}
    on:drop={(event) => handleDrop(event, node)}
  >
    <span class="tree-icon">
      {#if node.kind === "category"}
        <Folder size={18} />
      {:else}
        <IconGlyph icon={node.icon || "notebook"} size={18} />
      {/if}
    </span>

    {#if renamingId === node.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="rename-input"
        value={renameDraft}
        autofocus
        on:click|stopPropagation
        on:input={(event) => dispatch("renameInput", event.currentTarget.value)}
        on:blur={() => dispatch("renameCommit", node.id)}
        on:keydown={(event) => event.key === "Enter" && dispatch("renameCommit", node.id)}
      />
    {:else}
      <span class="list-name">{node.name}</span>
    {/if}

    {#if counts[node.id]}
      <span class="count-pill">{counts[node.id]}</span>
    {/if}
    {#if node.kind === "category"}
      <button class="collapse-button" type="button" aria-label="折叠分类" on:click|stopPropagation={() => dispatch("toggleCategory", node.id)}>
        <ChevronDown class={node.collapsed ? "collapsed" : ""} size={19} />
      </button>
    {/if}
  </div>
  {#if node.kind === "category" && !node.collapsed}
    <svelte:self
      {nodes}
      parentId={node.id}
      {selectedNodeId}
      {counts}
      level={level + 1}
      {renamingId}
      {renameDraft}
      on:selectEntry
      on:toggleCategory
      on:renameInput
      on:renameCommit
      on:openMenu
      on:dragStart
      on:dropNode
    />
  {/if}
{/each}

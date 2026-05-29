<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { ChevronDown } from "@lucide/svelte";
  import type { AppNode } from "./types";
  import IconGlyph from "./IconGlyph.svelte";

  type DropPosition = "before" | "after" | "inside";

  export let nodes: AppNode[] = [];
  export let parentId: string | null = null;
  export let selectedNodeId = "";
  export let counts: Record<string, number> = {};
  export let level = 0;
  export let renamingId: string | null = null;
  export let renameDraft = "";
  export let draggingId: string | null = null;
  export let requestIconPicker: (id: string) => void = () => {};

  const dispatch = createEventDispatcher<{
    selectEntry: string;
    toggleCategory: string;
    renameInput: string;
    renameCommit: string;
    openMenu: { id: string; x: number; y: number };
    pickIcon: string;
    dragStart: string;
    dropNode: { id: string; targetId: string; position: DropPosition };
  }>();

  let dropTargetId: string | null = null;
  let dropPosition: DropPosition | null = null;
  let suppressNextClick = false;

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  function rowStyle(levelValue: number): string {
    return `--depth: ${levelValue}; padding-left: ${levelValue * 18 + 10}px;`;
  }

  function isIconZone(event: MouseEvent | PointerEvent, node: AppNode): boolean {
    const row = event.currentTarget;
    if (!(row instanceof HTMLElement) || node.kind === "system") {
      return false;
    }
    const localX = event.clientX - row.getBoundingClientRect().left;
    return localX <= level * 20 + 92;
  }

  function handlePointerDown(event: PointerEvent, node: AppNode): void {
    if (!isIconZone(event, node)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    suppressNextClick = true;
    openIconPicker(node.id);
  }

  function handleClick(event: MouseEvent, node: AppNode): void {
    if (suppressNextClick || isIconZone(event, node)) {
      event.preventDefault();
      event.stopPropagation();
      suppressNextClick = false;
      openIconPicker(node.id);
      return;
    }
    suppressNextClick = false;
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

  function openIconPicker(id: string): void {
    requestIconPicker(id);
    dispatch("pickIcon", id);
  }

  function focusRename(node: HTMLInputElement): { destroy(): void } {
    const frame = requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
    return {
      destroy() {
        cancelAnimationFrame(frame);
      }
    };
  }

  function positionFromPointer(event: DragEvent, target: AppNode): DropPosition {
    const row = event.currentTarget;
    if (!(row instanceof HTMLElement)) {
      return target.kind === "category" ? "inside" : "after";
    }
    const rect = row.getBoundingClientRect();
    const ratio = (event.clientY - rect.top) / Math.max(1, rect.height);
    if (target.kind === "category") {
      if (ratio < 0.25) {
        return "before";
      }
      if (ratio > 0.75) {
        return "after";
      }
      return "inside";
    }
    return ratio < 0.5 ? "before" : "after";
  }

  function handleDragOver(event: DragEvent, target: AppNode): void {
    const id = event.dataTransfer?.getData("text/plain") || draggingId || "";
    if (!id || id === target.id) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "move";
    }
    dropTargetId = target.id;
    dropPosition = positionFromPointer(event, target);
  }

  function clearDropTarget(): void {
    dropTargetId = null;
    dropPosition = null;
  }

  function handleDrop(event: DragEvent, target: AppNode): void {
    event.preventDefault();
    event.stopPropagation();
    const id = event.dataTransfer?.getData("text/plain") || "";
    if (!id || id === target.id) {
      clearDropTarget();
      return;
    }
    dispatch("dropNode", { id, targetId: target.id, position: dropPosition ?? positionFromPointer(event, target) });
    clearDropTarget();
  }
</script>

{#each children as node (node.id)}
  <div
    class:selected={node.id === selectedNodeId}
    class:category={node.kind === "category"}
    class:dragging={draggingId === node.id}
    class:drop-before={dropTargetId === node.id && dropPosition === "before"}
    class:drop-after={dropTargetId === node.id && dropPosition === "after"}
    class:drop-inside={dropTargetId === node.id && dropPosition === "inside"}
    class="tree-row"
    data-node-id={node.id}
    data-level={level}
    style={rowStyle(level)}
    draggable={node.kind !== "system"}
    on:pointerdown={(event) => handlePointerDown(event, node)}
    on:click={(event) => handleClick(event, node)}
    on:contextmenu={(event) => openMenu(event, node)}
    on:dragstart={(event) => {
      event.dataTransfer?.setData("text/plain", node.id);
      dispatch("dragStart", node.id);
    }}
    on:dragend={() => {
      clearDropTarget();
      dispatch("dragStart", "");
    }}
    on:dragover={(event) => handleDragOver(event, node)}
    on:dragleave={(event) => {
      if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) {
        clearDropTarget();
      }
    }}
    on:drop={(event) => handleDrop(event, node)}
  >
    <button
      class="tree-icon"
      type="button"
      aria-label="选择图标"
      on:mousedown|preventDefault|stopPropagation={() => openIconPicker(node.id)}
      on:click|preventDefault|stopPropagation={() => openIconPicker(node.id)}
    >
      <IconGlyph icon={node.kind === "category" ? node.icon || "folder" : node.icon || "notebook"} size={18} />
    </button>

    {#if renamingId === node.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        use:focusRename
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
      {draggingId}
      {requestIconPicker}
      on:selectEntry={(event) => dispatch("selectEntry", event.detail)}
      on:toggleCategory={(event) => dispatch("toggleCategory", event.detail)}
      on:renameInput={(event) => dispatch("renameInput", event.detail)}
      on:renameCommit={(event) => dispatch("renameCommit", event.detail)}
      on:openMenu={(event) => dispatch("openMenu", event.detail)}
      on:pickIcon={(event) => dispatch("pickIcon", event.detail)}
      on:dragStart={(event) => dispatch("dragStart", event.detail)}
      on:dropNode={(event) => dispatch("dropNode", event.detail)}
    />
  {/if}
{/each}

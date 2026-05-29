<script lang="ts">
  import { ChevronDown, Folder } from "@lucide/svelte";
  import IconGlyph from "./IconGlyph.svelte";
  import type { TodoList } from "./types";

  export let lists: TodoList[] = [];
  export let selectedId = "";
  export let counts: Record<string, number> = {};
  export let editingId: string | null = null;
  export let renameDraft = "";
  export let draggingId: string | null = null;
  export let parentId: string | null = null;
  export let depth = 0;
  export let onSelectEntry: (id: string) => void;
  export let onToggleCategory: (id: string) => void;
  export let onOpenMenu: (id: string, x: number, y: number) => void;
  export let onRenameDraft: (value: string) => void;
  export let onCommitRename: () => void;
  export let onDragStartNode: (nodeId: string | null) => void;
  export let onDropNode: (targetId: string) => void;

  $: children = lists
    .filter((list) => list.kind === "custom" && list.parentId === parentId)
    .sort((a, b) => a.order - b.order || a.name.localeCompare(b.name));

  function childrenOf(id: string): TodoList[] {
    return lists.filter((list) => list.kind === "custom" && list.parentId === id);
  }

  function rowClick(node: TodoList): void {
    if (node.nodeType === "category") {
      onToggleCategory(node.id);
      return;
    }
    onSelectEntry(node.id);
  }

  function openMenu(event: MouseEvent, node: TodoList): void {
    event.preventDefault();
    event.stopPropagation();
    onOpenMenu(node.id, event.clientX, event.clientY);
  }

  function dragStart(event: DragEvent, node: TodoList): void {
    event.dataTransfer?.setData("text/plain", node.id);
    onDragStartNode(node.id);
  }

  function dragOver(event: DragEvent, node: TodoList): void {
    if (node.nodeType === "category") {
      event.preventDefault();
    }
  }

  function drop(event: DragEvent, node: TodoList): void {
    if (node.nodeType !== "category") {
      return;
    }
    event.preventDefault();
    onDropNode(node.id);
    onDragStartNode(null);
  }
</script>

{#each children as node (node.id)}
  {@const isCategory = node.nodeType === "category"}
  {@const hasChildren = childrenOf(node.id).length > 0}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class:category={isCategory}
    class:dragging={draggingId === node.id}
    class:selected={selectedId === node.id}
    class="tree-row"
    draggable="true"
    style={`--depth: ${depth}`}
    on:click={() => rowClick(node)}
    on:contextmenu={(event) => openMenu(event, node)}
    on:dragstart={(event) => dragStart(event, node)}
    on:dragend={() => onDragStartNode(null)}
    on:dragover={(event) => dragOver(event, node)}
    on:drop={(event) => drop(event, node)}
  >
    <span class="tree-icon">
      {#if isCategory}
        <Folder size={19} />
      {:else}
        <IconGlyph icon={node.icon} size={18} />
      {/if}
    </span>

    {#if editingId === node.id}
      <input
        class="rename-input"
        value={renameDraft}
        autofocus
        on:click|stopPropagation
        on:input={(event) => event.currentTarget instanceof HTMLInputElement && onRenameDraft(event.currentTarget.value)}
        on:blur={onCommitRename}
        on:keydown={(event) => {
          if (event.key === "Enter" || event.key === "Escape") onCommitRename();
        }}
      />
    {:else}
      <span class="list-name">{node.name}</span>
    {/if}

    {#if counts[node.id] > 0}
      <span class="count-pill">{counts[node.id]}</span>
    {/if}

    {#if isCategory}
      <button class="collapse-button" type="button" aria-label="折叠/展开" on:click|stopPropagation={() => onToggleCategory(node.id)}>
        {#if node.collapsed}
          <span class="chevron-right">›</span>
        {:else}
          <ChevronDown size={18} />
        {/if}
      </button>
    {/if}
  </div>

  {#if isCategory && !node.collapsed && hasChildren}
    <svelte:self
      {lists}
      {counts}
      {editingId}
      {renameDraft}
      {draggingId}
      parentId={node.id}
      depth={depth + 1}
      {selectedId}
      {onSelectEntry}
      {onToggleCategory}
      {onOpenMenu}
      {onRenameDraft}
      {onCommitRename}
      {onDragStartNode}
      {onDropNode}
    />
  {/if}
{/each}

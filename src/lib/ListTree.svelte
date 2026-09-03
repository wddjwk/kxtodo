<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { ChevronDown } from "@lucide/svelte";
  import { isMobile } from "./platform";
  import type { AppNode } from "./types";
  import IconGlyph from "./IconGlyph.svelte";

  type DropPosition = "before" | "after" | "inside";

  export let nodes: AppNode[] = [];
  export let parentId: string | null = null;
  export let selectedNodeId = "";
  export let counts: Record<string, number> = {};
  export let showCategoryCounts = true;
  export let level = 0;
  export let renamingId: string | null = null;
  export let renameDraft = "";
  export let draggingId: string | null = null;

  const dispatch = createEventDispatcher<{
    selectEntry: string;
    toggleCategory: string;
    renameInput: string;
    renameCommit: string;
    openMenu: { id: string; x: number; y: number };
    pickIcon: string;
    dragStart: string;
    dropNode: { id: string; targetId: string; position: DropPosition };
    dropRootEnd: string;
    dragEnd: void;
  }>();

  const DRAG_THRESHOLD_PX = 6;
  const AUTO_SCROLL_ZONE_PX = 30;
  const AUTO_SCROLL_SPEED_PX = 14;
  const HOVER_EXPAND_MS = 600;

  let dropTargetId: string | null = null;
  let dropPosition: DropPosition | null = null;
  let dropRootEnd = false;
  let suppressNextClick = false;
  let pointerDrag: { id: string; startX: number; startY: number; active: boolean } | null = null;
  let longPressTimer: number | null = null;
  let longPressFired = false;
  let scrollContainer: HTMLElement | null = null;
  let hoverExpandTimer: number | null = null;
  let hoverExpandTarget = "";

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  function rowStyle(levelValue: number): string {
    return `--depth: ${levelValue}; padding-left: ${levelValue * 18 + 10}px;`;
  }

  function handlePointerDown(event: PointerEvent, node: AppNode): void {
    const target = event.target;
    if (event.button !== 0 || node.kind === "system" || (target instanceof Element && target.closest("button, input"))) {
      return;
    }
    pointerDrag = { id: node.id, startX: event.clientX, startY: event.clientY, active: false };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: true });
    window.addEventListener("keydown", handleDragKeydown, true);
  }

  function handleClick(event: MouseEvent, node: AppNode): void {
    if (suppressNextClick || longPressFired) {
      event.preventDefault();
      event.stopPropagation();
      suppressNextClick = false;
      longPressFired = false;
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

  function handleTouchStart(event: TouchEvent, node: AppNode): void {
    if (!$isMobile) return;
    longPressFired = false;
    const touch = event.touches[0];
    if (!touch) return;
    const x = touch.clientX;
    const y = touch.clientY;
    longPressTimer = window.setTimeout(() => {
      longPressFired = true;
      suppressNextClick = true;
      dispatch("openMenu", { id: node.id, x, y });
    }, 500);
  }

  function handleTouchEnd(): void {
    if (longPressTimer !== null) {
      window.clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function handleTouchMove(): void {
    handleTouchEnd();
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

  function positionFromClientY(clientY: number, row: HTMLElement, target: AppNode): DropPosition {
    const rect = row.getBoundingClientRect();
    const ratio = (clientY - rect.top) / Math.max(1, rect.height);
    if (target.kind === "category") {
      if (ratio < 0.25) return "before";
      if (ratio > 0.75) return "after";
      return "inside";
    }
    return ratio < 0.5 ? "before" : "after";
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!pointerDrag) return;
    const distance = Math.hypot(event.clientX - pointerDrag.startX, event.clientY - pointerDrag.startY);
    if (!pointerDrag.active) {
      if (distance < DRAG_THRESHOLD_PX) return;
      pointerDrag.active = true;
      suppressNextClick = true;
      scrollContainer = (event.target as Element | null)?.closest?.(".custom-nav") ?? document.querySelector(".custom-nav");
      dispatch("dragStart", pointerDrag.id);
    }

    event.preventDefault();
    autoScroll(event.clientY);

    const targetElement = document.elementFromPoint(event.clientX, event.clientY);
    const row = targetElement?.closest<HTMLElement>(".tree-row[data-node-id]");
    const targetId = row?.dataset.nodeId ?? "";
    const target = nodes.find((node) => node.id === targetId);
    if (!row || !target || target.kind === "system" || target.id === pointerDrag.id) {
      // 落在空白区域：拖到列表末尾（root）
      if (targetElement?.closest(".custom-nav") && !row) {
        setRootEndTarget();
      } else {
        clearDropTarget();
      }
      return;
    }
    dropRootEnd = false;
    dropTargetId = target.id;
    dropPosition = positionFromClientY(event.clientY, row, target);
    scheduleHoverExpand(target);
  }

  function setRootEndTarget(): void {
    dropRootEnd = true;
    dropTargetId = null;
    dropPosition = null;
    clearHoverExpand();
  }

  function scheduleHoverExpand(target: AppNode): void {
    if (target.kind !== "category" || !target.collapsed || target.id === pointerDrag?.id) {
      clearHoverExpand();
      return;
    }
    if (hoverExpandTarget === target.id) return;
    clearHoverExpand();
    hoverExpandTarget = target.id;
    hoverExpandTimer = window.setTimeout(() => {
      dispatch("toggleCategory", target.id);
      clearHoverExpand();
    }, HOVER_EXPAND_MS);
  }

  function clearHoverExpand(): void {
    if (hoverExpandTimer !== null) {
      window.clearTimeout(hoverExpandTimer);
      hoverExpandTimer = null;
    }
    hoverExpandTarget = "";
  }

  function autoScroll(clientY: number): void {
    if (!scrollContainer) return;
    const rect = scrollContainer.getBoundingClientRect();
    if (clientY < rect.top + AUTO_SCROLL_ZONE_PX) {
      scrollContainer.scrollTop -= AUTO_SCROLL_SPEED_PX;
    } else if (clientY > rect.bottom - AUTO_SCROLL_ZONE_PX) {
      scrollContainer.scrollTop += AUTO_SCROLL_SPEED_PX;
    }
  }

  function handlePointerUp(): void {
    if (pointerDrag?.active) {
      if (dropRootEnd) {
        dispatch("dropRootEnd", pointerDrag.id);
      } else if (dropTargetId && dropPosition && dropTargetId !== pointerDrag.id) {
        dispatch("dropNode", { id: pointerDrag.id, targetId: dropTargetId, position: dropPosition });
      }
    }
    cleanupPointerDrag();
  }

  function handleDragKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && pointerDrag) {
      event.preventDefault();
      event.stopPropagation();
      cleanupPointerDrag();
    }
  }

  function cleanupPointerDrag(): void {
    window.removeEventListener("pointermove", handlePointerMove);
    window.removeEventListener("pointerup", handlePointerUp);
    window.removeEventListener("keydown", handleDragKeydown, true);
    if (pointerDrag?.active) {
      dispatch("dragEnd");
    }
    pointerDrag = null;
    scrollContainer = null;
    clearDropTarget();
    clearHoverExpand();
  }

  function clearDropTarget(): void {
    dropTargetId = null;
    dropPosition = null;
    dropRootEnd = false;
  }

  onDestroy(() => {
    cleanupPointerDrag();
    handleTouchEnd();
  });
</script>

{#each children as node (node.id)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
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
    on:pointerdown={(event) => handlePointerDown(event, node)}
    on:click={(event) => handleClick(event, node)}
    on:contextmenu={(event) => openMenu(event, node)}
    on:touchstart={(event) => handleTouchStart(event, node)}
    on:touchend={handleTouchEnd}
    on:touchmove={handleTouchMove}
  >
    <button
      class="tree-icon"
      type="button"
      aria-label="选择图标"
      on:mousedown|preventDefault|stopPropagation
      on:pointerdown|stopPropagation
      on:click|preventDefault|stopPropagation={() => dispatch("pickIcon", node.id)}
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
        on:pointerdown|stopPropagation
        on:input={(event) => dispatch("renameInput", event.currentTarget.value)}
        on:blur={() => dispatch("renameCommit", node.id)}
        on:keydown={(event) => {
          if (event.isComposing || event.keyCode === 229) return;
          if (event.key === "Enter") dispatch("renameCommit", node.id);
        }}
      />
    {:else}
      <span class="list-name">{node.name}</span>
    {/if}

    {#if node.kind === "category"}
      {#if showCategoryCounts && node.collapsed && counts[node.id]}
        <span class="count-pill">{counts[node.id]}</span>
      {/if}
      <button class="collapse-button" type="button" aria-label="折叠分类" on:click|stopPropagation={() => dispatch("toggleCategory", node.id)}>
        <ChevronDown class={node.collapsed ? "collapsed" : ""} size={19} />
      </button>
    {:else if counts[node.id]}
      <span class="count-pill">{counts[node.id]}</span>
    {/if}
  </div>
  {#if node.kind === "category" && !node.collapsed}
    <svelte:self
      {nodes}
      parentId={node.id}
      {selectedNodeId}
      {counts}
      {showCategoryCounts}
      level={level + 1}
      {renamingId}
      {renameDraft}
      {draggingId}
      on:selectEntry={(event) => dispatch("selectEntry", event.detail)}
      on:toggleCategory={(event) => dispatch("toggleCategory", event.detail)}
      on:renameInput={(event) => dispatch("renameInput", event.detail)}
      on:renameCommit={(event) => dispatch("renameCommit", event.detail)}
      on:openMenu={(event) => dispatch("openMenu", event.detail)}
      on:pickIcon={(event) => dispatch("pickIcon", event.detail)}
      on:dragStart={(event) => dispatch("dragStart", event.detail)}
      on:dropNode={(event) => dispatch("dropNode", event.detail)}
      on:dropRootEnd={(event) => dispatch("dropRootEnd", event.detail)}
      on:dragEnd={() => dispatch("dragEnd")}
    />
  {/if}
{/each}

{#if level === 0 && dropRootEnd}
  <div class="tree-root-drop-line" aria-hidden="true"></div>
{/if}

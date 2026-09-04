<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { ChevronDown } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "./longpress";
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
    closeMenu: void;
    pickIcon: string;
    dragStart: string;
    dropNode: { id: string; targetId: string; position: DropPosition };
    dropRootEnd: string;
    dragEnd: void;
  }>();

  const DRAG_THRESHOLD_PX = 6;
  /** 长按开菜单后继续按住移动超过该距离 → 关菜单转入拖拽（Android 启动器式）。 */
  const TOUCH_DRAG_THRESHOLD_PX = 12;
  const AUTO_SCROLL_ZONE_PX = 30;
  const AUTO_SCROLL_SPEED_PX = 14;
  const HOVER_EXPAND_MS = 600;

  let dropTargetId: string | null = null;
  let dropPosition: DropPosition | null = null;
  let dropRootEnd = false;
  let suppressNextClick = false;
  let pointerDrag: { id: string; startX: number; startY: number; active: boolean } | null = null;
  let touchDragArmed: { id: string; startX: number; startY: number } | null = null;
  let longPressFired = false;
  let scrollContainer: HTMLElement | null = null;
  let hoverExpandTimer: number | null = null;
  let hoverExpandTarget = "";

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  function rowStyle(levelValue: number): string {
    return `--depth: ${levelValue}; padding-left: ${levelValue * 18 + 10}px;`;
  }

  function handlePointerDown(event: PointerEvent, node: AppNode): void {
    // 拖拽排序仅支持鼠标；触摸端用长按开菜单，避免与滚动/长按冲突
    if (event.pointerType !== "mouse") return;
    const target = event.target;
    if (event.button !== 0 || node.kind === "system" || (target instanceof Element && target.closest("button, input"))) {
      return;
    }
    pointerDrag = { id: node.id, startX: event.clientX, startY: event.clientY, active: false };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: true });
    window.addEventListener("pointercancel", cleanupPointerDrag);
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
    // 触摸长按已开过菜单时，Chromium 补发的原生 contextmenu 直接吞掉
    if (isLongPressSuppressed()) return;
    dispatch("openMenu", { id: node.id, x: event.clientX, y: event.clientY });
  }

  /** 移动端触摸长按行：以原始触点为锚打开树菜单，并吞掉长按后的那次 click。
   * 长按后手指继续按住移动（Android 启动器式）→ 关菜单转入行拖拽。 */
  function handleRowLongPress(node: AppNode) {
    return (pos: { x: number; y: number }): void => {
      longPressFired = true;
      suppressNextClick = true;
      dispatch("openMenu", { id: node.id, x: pos.x, y: pos.y });
      armTouchDrag(node.id, pos);
    };
  }

  function armTouchDrag(nodeId: string, pos: { x: number; y: number }): void {
    disarmTouchDrag();
    touchDragArmed = { id: nodeId, startX: pos.x, startY: pos.y };
    window.addEventListener("pointermove", handleTouchDragMove);
    window.addEventListener("pointerup", disarmTouchDrag, { once: true });
    window.addEventListener("pointercancel", disarmTouchDrag);
  }

  function disarmTouchDrag(): void {
    touchDragArmed = null;
    window.removeEventListener("pointermove", handleTouchDragMove);
    window.removeEventListener("pointerup", disarmTouchDrag);
    window.removeEventListener("pointercancel", disarmTouchDrag);
  }

  /** 长按保持 + 移动超阈值：关菜单，用长按原始触点合成启动既有 pointerDrag 机制。 */
  function handleTouchDragMove(event: PointerEvent): void {
    const armed = touchDragArmed;
    if (!armed) return;
    if (Math.hypot(event.clientX - armed.startX, event.clientY - armed.startY) < TOUCH_DRAG_THRESHOLD_PX) return;
    disarmTouchDrag();
    dispatch("closeMenu");
    suppressNextClick = true;
    longPressFired = true;
    pointerDrag = { id: armed.id, startX: armed.startX, startY: armed.startY, active: false };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: true });
    window.addEventListener("pointercancel", cleanupPointerDrag);
    window.addEventListener("keydown", handleDragKeydown, true);
    // 当前这次移动立即生效，拖拽无感衔接
    handlePointerMove(event);
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
    // 触摸拖拽结束（位移超过 tap slop）时浏览器可能不再补发 click，抑制标志会
    // 残留并吞掉用户下一次正常点按；宏任务里兜底复位（真正的 click 在同一轮
    // 事件派发中先于定时器执行，抑制不受影响）。
    window.setTimeout(() => {
      suppressNextClick = false;
      longPressFired = false;
    }, 0);
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
    window.removeEventListener("pointercancel", cleanupPointerDrag);
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
    disarmTouchDrag();
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
    use:longpress={handleRowLongPress(node)}
    on:pointerdown={(event) => handlePointerDown(event, node)}
    on:click={(event) => handleClick(event, node)}
    on:contextmenu={(event) => openMenu(event, node)}
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
      on:closeMenu={() => dispatch("closeMenu")}
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

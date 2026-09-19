<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { get } from "svelte/store";
  import { ChevronDown } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import type { AppNode } from "./types";
  import type { TreeDropPosition, TreeHover } from "./nodes";
  import { clearTreeDropTarget, setTreeDropRootEnd, setTreeDropTarget, treeDropState } from "./listTreeDrag";
  import IconGlyph from "./IconGlyph.svelte";

  type DropPosition = TreeDropPosition;

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
    /** 拖动中的悬停落点（宿主拿它算「行实时让位」的预览） */
    dragHover: TreeHover;
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

  /**
   * 落点状态提到共享 store（v0.8.5 需求 29）：每层 ListTree 是独立实例，而目标行
   * 常常属于另一个实例——状态留在实例里时，跨层的「移入虚框 / 插入位」永远画不出来。
   * `pointerDrag` 仍然住在实例里：只有按下指针的那一层在跑拖动逻辑，这是对的。
   */
  let suppressNextClick = false;
  let pointerDrag: { id: string; startX: number; startY: number; active: boolean } | null = null;
  let touchDragArmed: { id: string; startX: number; startY: number } | null = null;
  let longPressFired = false;
  let scrollContainer: HTMLElement | null = null;
  let hoverExpandTimer: number | null = null;
  let hoverExpandTarget = "";
  /** 拖动中最近一次指针位置（自动滚动时行在指针下走，得拿它重算落点） */
  let lastPointer: { x: number; y: number } | null = null;
  /** 最近一次派发出去的悬停状态：同样的话不重复派发 */
  let lastHoverKey = "";

  $: children = nodes.filter((node) => node.parentId === parentId && node.kind !== "system");

  function reportHover(hover: TreeHover): void {
    const key = hover.over === "node" ? `node:${hover.targetId}:${hover.position}` : hover.over;
    if (key === lastHoverKey) return;
    lastHoverKey = key;
    dispatch("dragHover", hover);
  }

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
      scrollContainer?.addEventListener("scroll", handleDragScroll, { passive: true });
      dispatch("dragStart", pointerDrag.id);
    }

    event.preventDefault();
    lastPointer = { x: event.clientX, y: event.clientY };
    autoScroll(event.clientY);
    updateHover(event.clientX, event.clientY);
  }

  /** 自动滚动会把行从指针底下挪走：滚动一次就按最后指针位置重算落点 */
  function handleDragScroll(): void {
    if (!pointerDrag?.active || !lastPointer) return;
    updateHover(lastPointer.x, lastPointer.y);
  }

  function updateHover(clientX: number, clientY: number): void {
    if (!pointerDrag) return;
    const targetElement = document.elementFromPoint(clientX, clientY);
    const row = targetElement?.closest<HTMLElement>(".tree-row[data-node-id]");
    const targetId = row?.dataset.nodeId ?? "";
    const target = nodes.find((node) => node.id === targetId);
    if (row && target && target.id === pointerDrag.id) {
      // 指针正落在被拖的那一行身上（行实时让位之后它就在指针下方）：**保持现落点**。
      // 若在这里按「拖到自己」清空，行会弹回原位、下一帧又命中，来回抖。
      return;
    }
    if (!row || !target || target.kind === "system") {
      // 落在空白区域：拖到列表末尾（root）
      if (targetElement?.closest(".custom-nav") && !row) {
        setRootEndTarget();
      } else {
        clearDropTarget();
      }
      return;
    }
    const position = positionFromClientY(clientY, row, target);
    setTreeDropTarget(target.id, position);
    reportHover({ over: "node", targetId: target.id, position });
    scheduleHoverExpand(target);
  }

  function setRootEndTarget(): void {
    setTreeDropRootEnd();
    reportHover({ over: "rootEnd" });
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
      const drop = get(treeDropState);
      if (drop.rootEnd) {
        dispatch("dropRootEnd", pointerDrag.id);
      } else if (drop.targetId && drop.position && drop.targetId !== pointerDrag.id) {
        dispatch("dropNode", { id: pointerDrag.id, targetId: drop.targetId, position: drop.position });
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
    scrollContainer?.removeEventListener("scroll", handleDragScroll);
    const wasDragging = pointerDrag?.active === true;
    if (wasDragging) {
      dispatch("dragEnd");
      // 落点状态是全局共享的：**只有真正在拖的这一层**才清，
      // 别的实例（拖拽中因展开/折叠而重建的那些）销毁时不能顺手把它清掉
      clearDropTarget();
    }
    pointerDrag = null;
    scrollContainer = null;
    lastPointer = null;
    clearHoverExpand();
  }

  function clearDropTarget(): void {
    clearTreeDropTarget();
    reportHover({ over: "none" });
  }

  onDestroy(() => {
    cleanupPointerDrag();
    disarmTouchDrag();
  });
</script>

{#each children as node (node.id)}
  <!-- 每一项包一层：`animate:flip` 要求 animate 指令所在元素是 each 块的唯一子元素，
       而这一项里行与子树是并列的两块——包一层才能让整棵子树跟着行一起平移
       （v0.8.5 需求 29 的「行实时让位」）。间距由 .tree-item 自己补同款 2px。 -->
  <div class="tree-item" animate:flip={{ duration: 150 }}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class:selected={node.id === selectedNodeId}
      class:category={node.kind === "category"}
      class:dragging={draggingId === node.id}
      class:drop-before={$treeDropState.targetId === node.id && $treeDropState.position === "before"}
      class:drop-after={$treeDropState.targetId === node.id && $treeDropState.position === "after"}
      class:drop-inside={$treeDropState.targetId === node.id && $treeDropState.position === "inside"}
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
        on:dragHover={(event) => dispatch("dragHover", event.detail)}
        on:dropNode={(event) => dispatch("dropNode", event.detail)}
        on:dropRootEnd={(event) => dispatch("dropRootEnd", event.detail)}
        on:dragEnd={() => dispatch("dragEnd")}
      />
    {/if}
  </div>
{/each}

{#if level === 0 && $treeDropState.rootEnd}
  <div class="tree-root-drop-line" aria-hidden="true"></div>
{/if}

<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { get } from "svelte/store";
  import { ChevronDown } from "@lucide/svelte";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import type { AppNode } from "./types";
  import type { TreeDropPosition, TreeHover } from "./nodes";
  import {
    contiguousZones, DRAG_BAND_PX, keepsPreviousDecision, positionInRow, scaleOf, settledTop, zoneAt,
    type DragPosition, type DragZone
  } from "./dragHit";
  import {
    clearTreeDropTarget, registerTreeRow, setTreeDropRootEnd, setTreeDropTarget, treeDropState, treeRowElements
  } from "./listTreeDrag";
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
  /** 落点判定的 rAF 句柄（一帧最多判一次） */
  let hoverFrame = 0;
  /** 当前决策（迟滞判定的「上一轮」）：行 id 与该行之内的位置 */
  let hoverTargetId: string | null = null;
  let hoverPosition: DragPosition | null = null;
  /** 上一次「按几何重算」时用的那一行几何与指针位置：用来分辨「布局在动」与「用户在动」 */
  let hoverZone: { key: string; top: number; bottom: number } | null = null;
  let hoverPointerY = 0;
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

  /**
   * 行元素登记（落点判定要跨实例取「布局位置」，见 listTreeDrag 的说明）。
   * action 的 `update` 在 id 变化时重新登记——each 块 keyed by id，理论上不会变，
   * 但别留一个「元素换了 id 却还挂在旧 id 下」的隐患。
   */
  function trackRow(element: HTMLElement, id: string): { update(next: string): void; destroy(): void } {
    let current = id;
    let release = registerTreeRow(current, element);
    return {
      update(next: string) {
        if (next === current) return;
        release();
        current = next;
        release = registerTreeRow(current, element);
      },
      destroy() {
        release();
      }
    };
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
    scheduleHover();
  }

  /** 自动滚动会把行从指针底下挪走：滚动一次就按最后指针位置重算落点 */
  function handleDragScroll(): void {
    if (!pointerDrag?.active || !lastPointer) return;
    scheduleHover();
  }

  /**
   * 一帧最多判定一次（v0.8.6 需求 2「预览排序 rAF 合并」）：pointermove 在部分设备上
   * 一帧能来好几条，而每次判定都会读一圈 rect（强制布局）并可能触发预览重排。
   */
  function scheduleHover(): void {
    if (hoverFrame !== 0 || !lastPointer) return;
    hoverFrame = requestAnimationFrame(() => {
      hoverFrame = 0;
      if (!pointerDrag?.active || !lastPointer) return;
      updateHover(lastPointer.x, lastPointer.y);
    });
  }

  function cancelHoverFrame(): void {
    if (hoverFrame === 0) return;
    cancelAnimationFrame(hoverFrame);
    hoverFrame = 0;
  }
  /**
   * 落点判定（v0.8.6 需求 2）：
   * - 位置一律取**布局位置**（`settledTop` 把 flip 的半路 transform 减掉）——实时矩形
   *   在行让位动画期间互相不自洽，用它判定会 hover 振荡、整棵树反复 flip；
   * - 与上一轮决策之间走迟滞带（`zoneAt` / `positionInRow`）：越过边界 `band/2` 才换；
   * - 整个判定合并到 rAF：指针事件与自动滚动都只记录位置，一帧最多判一次、派发一次。
   */
  function buildZones(): { zones: DragZone[]; byId: Map<string, AppNode>; scale: number } {
    const scale = scaleOf(scrollContainer ?? document.documentElement);
    const zones: DragZone[] = [];
    const byId = new Map<string, AppNode>();
    for (const [id, element] of treeRowElements()) {
      // 收起态子树是**挂载但被裁到 0 高**的（高度动画的代价）：它们的行有布局位置、
      // 会和后面的行重叠，落点判定必须把它们排除，否则会命中看不见的行。
      if (element.closest(".tree-children:not(.open)")) continue;
      const target = nodes.find((node) => node.id === id);
      if (!target || target.kind === "system") continue;
      const top = settledTop(element, scale);
      // **被拖的那一行要留在 zones 里**（v0.8.5 需求 29 的纪律，别改成排除）：
      // 行实时让位之后它就在指针下方，排除掉就出现一个「没有 zone 的空档」，
      // 指针落在空档里会被当成空白区清掉落点——行弹回原位、下一帧又命中，来回抖。
      // 命中自己时保持现落点，在 updateHover 里判。
      zones.push({ key: id, top, bottom: top + element.offsetHeight * scale });
      byId.set(id, target);
    }
    zones.sort((a, b) => a.top - b.top);
    return { zones, byId, scale };
  }

  function updateHover(clientX: number, clientY: number): void {
    if (!pointerDrag) return;
    const { zones, byId } = buildZones();
    const lastKey = hoverTargetId;
    // 落点用**连成一片**的分区：行与行之间有 2px 缝隙，严格按 span 判定会掉进缝里
    // 被当成「空白区」（落点被清掉 / 变成拖到末尾），拖动经过缝隙时整棵树乱跳。
    const key = zoneAt(contiguousZones(zones), clientY, lastKey, DRAG_BAND_PX);
    if (key === pointerDrag.id) {
      // 指针正落在被拖的那一行身上（行实时让位之后它就在指针下方）：**保持现落点**
      return;
    }
    if (!key) {
      // 落在空白区域：拖到列表末尾（root）；不在列表里则什么都不做
      const element = document.elementFromPoint(clientX, clientY);
      if (element?.closest(".custom-nav")) {
        hoverTargetId = null;
        hoverPosition = null;
        hoverZone = null;
        setRootEndTarget();
      } else {
        hoverTargetId = null;
        hoverPosition = null;
        hoverZone = null;
        clearDropTarget();
      }
      return;
    }
    // 行内位置判定用**行自己的真实边界**（连成一片的边界已经把缝隙算给了邻居）
    const zone = zones.find((item) => item.key === key)!;
    const target = byId.get(key)!;
    // 行动了、指针没动 = 是我们自己的预览让位在动它（不是用户在动）：保持现判。
    // 少了这条，瞄准分组头中部会在让位之后被判成「插到分组后面」，行来回闪。
    if (
      hoverZone?.key === key &&
      hoverPosition !== null &&
      keepsPreviousDecision({
        pointerY: clientY,
        lastPointerY: hoverPointerY,
        rowTop: zone.top,
        lastRowTop: hoverZone.top,
        band: DRAG_BAND_PX
      })
    ) {
      setTreeDropTarget(key, hoverPosition);
      reportHover({ over: "node", targetId: key, position: hoverPosition });
      scheduleHoverExpand(target);
      return;
    }
    const last = hoverTargetId === key ? hoverPosition : null;
    const position = positionInRow(zone, clientY, last, DRAG_BAND_PX, target.kind === "category");
    hoverTargetId = key;
    hoverPosition = position;
    hoverZone = { key, top: zone.top, bottom: zone.bottom };
    hoverPointerY = clientY;
    setTreeDropTarget(key, position);
    reportHover({ over: "node", targetId: key, position });
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
      // 悬停自动展开改了行数与位置：上一轮的行内位置不再有意义，按新布局重判
      hoverPosition = null;
      lastPointer && scheduleHover();
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
    hoverTargetId = null;
    hoverPosition = null;
    hoverZone = null;
    cancelHoverFrame();
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
      use:trackRow={node.id}
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
    {#if node.kind === "category"}
      <!-- 子树容器做**高度动画**（v0.8.6 需求 2）：早先 `{#if !collapsed}` 直接把子树
           挂上去，新内容瞬间占满高度、兄弟行再慢慢 flip 下去——「先盖住、再挪走」。
           现在收起态也挂载（被裁到 0 高：不占位、不可命中），展开/收起走
           `grid-template-rows: 0fr → 1fr`，时长与缓动跟兄弟行的 flip 对齐。 -->
      <div class="tree-children" class:open={!node.collapsed}>
        <div class="tree-children-inner">
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
        </div>
      </div>
    {/if}
  </div>
{/each}

{#if level === 0 && $treeDropState.rootEnd}
  <div class="tree-root-drop-line" aria-hidden="true"></div>
{/if}

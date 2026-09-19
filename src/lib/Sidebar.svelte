<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Component } from "svelte";
  import { flip } from "svelte/animate";
  import {
    ChevronsDownUp, ChevronsUpDown, FilePlus2, FolderInput, FolderPlus, NotebookPen, Pencil, PinOff, Search, Shapes, Toolbox, Trash2, Upload, Wallet
  } from "@lucide/svelte";
  import {
    appState, appSettings, showToast, showSettings,
    searchQuery, listCounts, isSearching,
    now, safeFileName, appVersion, diaryOpen, diaryEditor, ledgerOpen, ledgerEditor, toolboxOpen
  } from "./stores";
  import {
    selectNode as selectNodeAction, toggleCategory as toggleCategoryAction,
    addNode as addNodeAction, renameNode as renameNodeAction,
    deleteNodeCascade as deleteNodeCascadeAction, applyTreeOrder as applyTreeOrderAction,
    setNodeIcon as setNodeIconAction,
    setToolPinned as setToolPinnedAction, reorderNavItems as reorderNavItemsAction
  } from "./actions";
  import { nodeAndDescendantIds, planTreeMove, planTreeRootEnd, moveTargetOptions, exportStateForNode } from "./nodes";
  import type { TreeHover } from "./nodes";
  import type { AppNode } from "./types";
  import { uiScaleValue, avatarStyle, avatarInitial } from "./styles";
  import { avatarCache, resolveAvatarSrc, isLocalImageRef, localImageFilename } from "./images";
  import { deleteBackgroundImage, deleteNodeImages, exportData } from "./backend";
  import IconGlyph from "./IconGlyph.svelte";
  import ListTree from "./ListTree.svelte";
  import SearchResults from "./SearchResults.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import { isMobile, mobileView, showMobileContent, showMobileDiary, showMobileLedger, showMobileToolbox, createBackGuard } from "./platform";
  import { caps } from "./capabilities";
  import type { NavItemId } from "./nav";
  import { navIdToolId } from "./nav";
  import { toolRoute, openToolboxTool, resetToolRoute } from "./tools/navigation";
  import { toolById } from "./tools/registry";
  import { transferState } from "./transferStore";
  import type { ToolId } from "./tools/catalog";
  import { longpress, isLongPressSuppressed } from "./longpress";

  const dispatch = createEventDispatcher<{ suppressClose: void }>();

  let searchInput: HTMLInputElement;
  let searchResultsRef: SearchResults;
  let sidebarWidth = 320;
  let renamingId: string | null = null;
  let renameDraft = "";
  let iconPickerListId: string | null = null;
  let treeMenu: { id: string; x: number; y: number } | null = null;
  let emptyAreaMenu: { x: number; y: number } | null = null;
  let draggingId: string | null = null;
  /**
   * 拖动中的树预览（v0.8.5 需求 29）：用与落盘**同一个 planner** 算出来的节点顺序渲染，
   * 行跟着指针实时让位（animate:flip 补平移动画）；松手落盘后清空，回到权威数据。
   */
  let treePreview: AppNode[] | null = null;
  let ignoreOverlayCloseOnce = false;

  $: treeMenuNode = treeMenu ? $appState.nodes.find((n) => n.id === treeMenu?.id) : null;
  $: treeMoveTargets = treeMenuNode ? moveTargetOptions(treeMenuNode.id, $appState.nodes) : [];
  $: selectedIconPickerList = iconPickerListId ? $appState.nodes.find((n) => n.id === iconPickerListId) : null;
  // 图标选择器是懒加载的：chunk 在途的窗口里组件还没挂载、它自己的 guard 也没注册，
  // 返回键这一下会把底下的页面弹掉。按本地标志先守一层（组件挂载后它的 guard
  // 注册得更晚、先被问到，两层不冲突）。
  const iconPickerGuard = createBackGuard();
  $: iconPickerGuard(iconPickerListId !== null, () => (iconPickerListId = null));
  $: resolvedAvatar = resolveAvatarSrc($appSettings.profile.avatar, $avatarCache);
  $: avStyle = avatarStyle(resolvedAvatar);
  $: avInitial = avatarInitial($appSettings.profile.displayName);
  // 日记不是节点：高亮跟着「谁占着主区域」走（移动端 mobileView，桌面 diaryOpen）
  $: diaryActive = $isMobile ? $mobileView === "diary" : $diaryOpen;
  // 记账同日记：不是节点，高亮跟着「谁占着主区域」走
  $: ledgerActive = $isMobile ? $mobileView === "ledger" : $ledgerOpen;
  // 工具箱 v0.7.5 起两端都有，高亮同一条口径
  $: toolboxActive = $isMobile ? $mobileView === "toolbox" : $toolboxOpen;

  /**
   * 固定导航的每一行：四个系统节点（我的一天/计划内/收藏/定时任务）与三条不是节点的
   * 行（日记/记账/工具箱）合成一份数据，于是「显示哪些、什么顺序、怎么排」全由
   * appearance.navItems / navLayout 决定，模板里不再散落 hardcoded 的行。
   */
  type NavRow = {
    id: NavItemId;
    label: string;
    glyph?: string;
    // 与工具注册表同一个口径：图标组件（lucide 或自绘）都满足 svelte 的 Component
    component?: Component;
    selected: boolean;
    count: number;
    onSelect: () => void;
  };

  /** 拖动中的临时顺序（v0.8.4 需求 8）：拖动时行跟着让位，松手才落盘。 */
  let navDragOrder: NavItemId[] | null = null;

  $: navRows = (navDragOrder ?? $appSettings.appearance.navItems).flatMap((id): NavRow[] => {
    if (id === "diary") {
      return [{ id, label: "日记", component: NotebookPen, selected: diaryActive, count: 0, onSelect: openDiary }];
    }
    if (id === "ledger") {
      return [{ id, label: "记账", component: Wallet, selected: ledgerActive, count: 0, onSelect: openLedger }];
    }
    if (id === "toolbox") {
      // 工具箱两端都有（v0.7.5）；caps 过滤保留着——将来某端不放工具箱时只动 capabilities
      // 角标 = 传输助手待确认的接收请求（v0.8.5）：人不在工具页时系统通知之外的第二道提示，
      // 点进来就能看到确认卡，别让 60 秒超时变成「静默拒绝」
      return caps.toolbox
        ? [{ id, label: "工具箱", component: Toolbox, selected: toolboxActive, count: $transferState.requests.length, onSelect: openToolbox }]
        : [];
    }
    // 钉住的工具行（v0.8.3「固定此工具」）：直达该工具的子视图，图标用工具自己的
    const toolId = navIdToolId(id);
    if (toolId !== null) {
      const tool = toolById(toolId);
      if (!tool) return [];
      return [
        {
          id,
          label: tool.name,
          component: tool.icon,
          selected: toolboxActive && $toolRoute === toolId,
          count: 0,
          onSelect: () => openPinnedTool(toolId)
        }
      ];
    }
    // 移动端没有调度引擎：定时任务这一行不给
    if (id === "scheduled" && !caps.scheduler) return [];
    const node = $appState.nodes.find((item) => item.id === id && item.kind === "system");
    if (!node) return [];
    return [
      {
        id,
        label: node.name,
        glyph: node.icon,
        selected:
          !diaryActive && !ledgerActive && !toolboxActive && $appState.selectedNodeId === node.id && !$isSearching,
        count: $listCounts[node.id] ?? 0,
        onSelect: () => selectNode(node.id)
      }
    ];
  });
  $: navLayout = $appSettings.appearance.navLayout;

  export function closeOverlays(): void {
    if (ignoreOverlayCloseOnce) {
      ignoreOverlayCloseOnce = false;
      return;
    }
    treeMenu = null;
    emptyAreaMenu = null;
    navMenu = null;
    iconPickerListId = null;
    searchResultsRef?.closeOverlays();
  }

  export function shouldSuppressClose(): boolean {
    if (ignoreOverlayCloseOnce) {
      ignoreOverlayCloseOnce = false;
      return true;
    }
    return false;
  }

  export function focusSearch(): void {
    searchInput?.focus();
  }

  function selectNode(id: string): void {
    searchQuery.set("");
    diaryEditor.set(null);
    diaryOpen.set(false);
    ledgerEditor.set(null);
    ledgerOpen.set(false);
    toolboxOpen.set(false);
    void selectNodeAction(id);
    treeMenu = null;
    emptyAreaMenu = null;
    iconPickerListId = null;
    showMobileContent();
  }

  function openDiary(): void {
    searchQuery.set("");
    treeMenu = null;
    emptyAreaMenu = null;
    iconPickerListId = null;
    ledgerEditor.set(null);
    ledgerOpen.set(false);
    toolboxOpen.set(false);
    if ($isMobile) {
      showMobileDiary();
      return;
    }
    diaryOpen.set(true);
  }

  function openLedger(): void {
    searchQuery.set("");
    treeMenu = null;
    emptyAreaMenu = null;
    iconPickerListId = null;
    diaryEditor.set(null);
    diaryOpen.set(false);
    toolboxOpen.set(false);
    if ($isMobile) {
      showMobileLedger();
      return;
    }
    ledgerOpen.set(true);
  }

  /** 工具箱与日记/记账同一条互斥：谁占主区域，另外两个收起 */
  function showToolboxPage(): void {
    searchQuery.set("");
    treeMenu = null;
    emptyAreaMenu = null;
    navMenu = null;
    iconPickerListId = null;
    diaryEditor.set(null);
    diaryOpen.set(false);
    ledgerEditor.set(null);
    ledgerOpen.set(false);
    if ($isMobile) {
      showMobileToolbox();
      return;
    }
    toolboxOpen.set(true);
  }

  /** 从工具箱这一行进的一定是看列表：清掉可能残留的直达路由 */
  function openToolbox(): void {
    resetToolRoute();
    showToolboxPage();
  }

  /** 钉住的工具行：直达子视图。路由必须在开页**之后**设：
   *  openToolbox / showToolboxPage 一带的收尾会把可能残留的路由先清掉。 */
  function openPinnedTool(toolId: ToolId): void {
    showToolboxPage();
    openToolboxTool(toolId);
  }

  // ---- 固定区：钉住的工具的右键 / 长按菜单（取消固定） ----
  let navMenu: { id: NavItemId; x: number; y: number } | null = null;

  function openNavMenuAt(x: number, y: number, id: NavItemId): void {
    navMenu = { id, x, y };
  }

  function handleNavContext(event: MouseEvent, id: NavItemId): void {
    // 触摸长按后 Chromium 会补发一个 contextmenu：长按已经开过菜单，这里去重
    if (isLongPressSuppressed()) return;
    event.preventDefault();
    event.stopPropagation();
    openNavMenuAt(event.clientX, event.clientY, id);
  }

  function unpinNavRow(): void {
    const id = navMenu?.id;
    navMenu = null;
    const toolId = id ? navIdToolId(id) : null;
    if (toolId) void setToolPinnedAction(toolId, false);
  }

  // ---- 固定区拖动排序（鼠标；触屏长按是菜单，与全应用手势一致） ----
  let navEl: HTMLElement;
  let navDrag: { id: NavItemId } | null = null;
  /** 拖完松手会补一个 click：不压掉的话「排完序顺手把那一行打开了」 */
  let navDragSuppressClick = false;

  /**
   * 落点下标：**按布局两种算法**。
   *
   * - 单列：一行一项，按 Y 在行内的上半 / 下半决定插在它前还是后；
   * - 双列 / 只图标：**先按 Y 找所在的「视觉行」**（同一竖带），再在该带内按 X 与
   *   各行中心的距离取最近的一项，按左/右决定插前还是插后。图标模式整块只有一排、
   *   所有行共用一个竖带，早前只比 Y 的写法永远在第一行命中、返回 0/1——
   *   拖谁都只能落到前两位。
   */
  function navDropIndex(clientX: number, clientY: number): number {
    const rows = [...(navEl?.querySelectorAll(".nav-row") ?? [])] as HTMLElement[];
    const boxes = rows
      .map((row, index) => {
        const rect = row.getBoundingClientRect();
        return { index, top: rect.top, bottom: rect.bottom, left: rect.left, width: rect.width, height: rect.height };
      })
      .filter((box) => box.height > 0);
    if (boxes.length === 0) return 0;
    const midY = (box: { top: number; bottom: number }) => box.top + (box.bottom - box.top) / 2;
    if (navLayout === "list") {
      for (const box of boxes) {
        if (clientY < box.top) return box.index;
        if (clientY <= box.bottom) return clientY < midY(box) ? box.index : box.index + 1;
      }
      return boxes[boxes.length - 1].index + 1;
    }
    const band = boxes.filter((box) => clientY >= box.top && clientY <= box.bottom);
    if (band.length === 0) {
      // 落在行间的缝隙或整块之外：按最近一行的中心决定插前还是插后
      const nearest = boxes.reduce((a, b) =>
        Math.abs(clientY - midY(a)) <= Math.abs(clientY - midY(b)) ? a : b
      );
      return clientY < midY(nearest) ? nearest.index : nearest.index + 1;
    }
    const midX = (box: { left: number; width: number }) => box.left + box.width / 2;
    const nearest = band.reduce((a, b) =>
      Math.abs(clientX - midX(a)) <= Math.abs(clientX - midX(b)) ? a : b
    );
    return clientX < midX(nearest) ? nearest.index : nearest.index + 1;
  }

  /** 拖动中把行实时挪到落点（松手才落盘）：拖动看着像「行跟着让位」而不是只画一根线 */
  function previewNavOrder(id: NavItemId, clientX: number, clientY: number): void {
    const ids = navDragOrder ?? [...$appSettings.appearance.navItems];
    const from = ids.indexOf(id);
    if (from < 0) return;
    let to = navDropIndex(clientX, clientY);
    const moving = [...ids];
    moving.splice(from, 1);
    if (to > from) to -= 1;
    moving.splice(Math.max(0, Math.min(moving.length, to)), 0, id);
    if (moving.every((item, index) => item === ids[index])) return;
    navDragOrder = moving;
  }

  /** 松手：把拖动中的顺序落盘（没动过就不写） */
  function commitNavDrag(): void {
    const order = navDragOrder;
    navDragOrder = null;
    if (!order) return;
    void reorderNavItemsAction(order);
  }

  function navPointerDown(event: PointerEvent, id: NavItemId): void {
    if (event.pointerType !== "mouse" || event.button !== 0) return;
    const startX = event.clientX;
    const startY = event.clientY;
    let armed = false;
    const move = (ev: PointerEvent): void => {
      if (!armed && Math.hypot(ev.clientX - startX, ev.clientY - startY) > 5) {
        armed = true;
        navDrag = { id };
      }
      if (!armed || !navDrag) return;
      ev.preventDefault();
      previewNavOrder(id, ev.clientX, ev.clientY);
    };
    const up = (): void => {
      window.removeEventListener("pointermove", move, true);
      window.removeEventListener("pointerup", up, true);
      if (armed) commitNavDrag();
      else navDragOrder = null;
      navDrag = null;
      if (armed) {
        navDragSuppressClick = true;
        window.setTimeout(() => (navDragSuppressClick = false), 0);
      }
    };
    window.addEventListener("pointermove", move, true);
    window.addEventListener("pointerup", up, true);
  }

  function toggleCategory(id: string): void {
    const node = $appState.nodes.find((n) => n.id === id);
    if (node) {
      void toggleCategoryAction(id, !node.collapsed);
    }
    treeMenu = null;
    iconPickerListId = null;
  }

  function currentCategoryId(): string | null {
    const sel = $appState.nodes.find((n) => n.id === $appState.selectedNodeId);
    if (sel?.kind === "entry") return sel.parentId;
    if (sel?.kind === "category") return sel.id;
    return null;
  }

  function addNode(parentId: string | null, kind: "category" | "entry"): void {
    const name = kind === "category" ? "未命名分类" : "未命名条目";
    if (parentId) {
      void toggleCategoryAction(parentId, false);
    }
    void addNodeAction(kind, name, parentId).then((node) => {
      if (node) {
        renamingId = node.id;
        renameDraft = node.name;
      }
    });
    treeMenu = null;
    emptyAreaMenu = null;
  }

  function startRename(id: string): void {
    const node = $appState.nodes.find((n) => n.id === id);
    if (!node || node.kind === "system") return;
    renamingId = id;
    renameDraft = node.name;
    treeMenu = null;
  }

  function commitRename(id: string): void {
    if (renamingId !== id) return;
    const name = renameDraft.trim();
    if (!name) {
      showToast("名称不能为空");
      return;
    }
    void renameNodeAction(id, name);
    renamingId = null;
    renameDraft = "";
  }

  function deleteNode(id: string): void {
    const node = $appState.nodes.find((n) => n.id === id);
    if (!node || node.kind === "system") {
      showToast("内置列表不能删除");
      return;
    }
    const ids = nodeAndDescendantIds(id, $appState.nodes);
    for (const delId of ids) {
      const bg = $appState.backgrounds[delId];
      if (bg?.image && isLocalImageRef(bg.image)) {
        void deleteBackgroundImage(localImageFilename(bg.image));
      }
      void deleteNodeImages(delId);
    }
    void deleteNodeCascadeAction(id);
    treeMenu = null;
  }

  async function exportNode(id: string): Promise<void> {
    const node = $appState.nodes.find((n) => n.id === id);
    if (!node) return;
    const payload = {
      version: $appVersion || "0.0.0",
      exportedAt: now(),
      scope: "node",
      nodeId: node.id,
      state: exportStateForNode(node, $appState)
    };
    await exportData(payload, `${safeFileName(node.name)}-${$appVersion || "dev"}.json`);
    treeMenu = null;
    showToast("导出完成");
  }

  function moveNode(id: string, targetId: string, position: "before" | "after" | "inside"): void {
    treePreview = null;
    draggingId = null;
    const nodes = $appState.nodes;
    const plan = planTreeMove(nodes, id, targetId, position);
    if (!plan) {
      const source = nodes.find((n) => n.id === id);
      if (source && nodeAndDescendantIds(source.id, nodes).has(targetId)) {
        showToast("不能移动到自身或自己的子分类中");
      }
      return;
    }
    void applyTreeOrderAction(plan.ordered, { [id]: plan.parentId });
  }

  /** 拖到空白区：移动为根级最后一项。 */
  function moveNodeToRootEnd(id: string): void {
    treePreview = null;
    draggingId = null;
    const plan = planTreeRootEnd($appState.nodes, id);
    if (!plan) return;
    void applyTreeOrderAction(plan.ordered, { [id]: plan.parentId });
  }

  /** 拖动中的悬停：用与落盘同一个 planner 算预览（需求 29 的「行实时让位」） */
  function handleTreeDragHover(event: CustomEvent<TreeHover>): void {
    if (!draggingId) return;
    const hover = event.detail;
    if (hover.over === "none") {
      treePreview = null;
      return;
    }
    const plan =
      hover.over === "rootEnd"
        ? planTreeRootEnd($appState.nodes, draggingId)
        : planTreeMove($appState.nodes, draggingId, hover.targetId, hover.position);
    treePreview = plan ? plan.ordered : null;
  }

  function moveNodeToGroup(id: string, parentId: string | null): void {
    const source = $appState.nodes.find((n) => n.id === id);
    const nextParentId = parentId || null;
    if (!source || source.kind === "system") return;
    if (source.parentId === nextParentId) {
      treeMenu = null;
      return;
    }
    const targetParent = nextParentId ? $appState.nodes.find((n) => n.id === nextParentId && n.kind === "category") : null;
    if (nextParentId && !targetParent) {
      showToast("目标分组不存在");
      return;
    }
    if (source.kind === "category" && nextParentId && nodeAndDescendantIds(source.id, $appState.nodes).has(nextParentId)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }
    const withoutSource = $appState.nodes.filter((n) => n.id !== id);
    const sourceWithParent = { ...source, parentId: nextParentId };
    let insertIndex = withoutSource.length;
    if (nextParentId) {
      const siblingIndexes = withoutSource.map((n, i) => ({ n, i })).filter((item) => item.n.parentId === nextParentId).map((item) => item.i);
      const parentIndex = withoutSource.findIndex((n) => n.id === nextParentId);
      insertIndex = siblingIndexes.length ? Math.max(...siblingIndexes) + 1 : parentIndex >= 0 ? parentIndex + 1 : withoutSource.length;
    }
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, sourceWithParent);
    const ordered = nodes.map((n) => (nextParentId && n.id === nextParentId ? { ...n, collapsed: false } : n));
    void applyTreeOrderAction(ordered, { [id]: nextParentId });
    treeMenu = null;
    draggingId = null;
  }

  function openIconPicker(id: string): void {
    iconPickerListId = id;
    treeMenu = null;
    emptyAreaMenu = null;
    showSettings.set(false);
    ignoreOverlayCloseOnce = true;
    dispatch("suppressClose");
    window.setTimeout(() => {
      ignoreOverlayCloseOnce = false;
    }, 250);
  }

  function pickIcon(icon: string): void {
    if (!selectedIconPickerList) return;
    void setNodeIconAction(selectedIconPickerList.id, icon);
    iconPickerListId = null;
  }

  /** 空白区右键：新建入口（事件来自 ListTree 之外的容器区域）。 */
  function openEmptyAreaMenu(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    if (target?.closest(".tree-row")) return;
    event.preventDefault();
    // 触摸长按已开过菜单时，Chromium 补发的原生 contextmenu 直接吞掉
    if (isLongPressSuppressed()) return;
    treeMenu = null;
    emptyAreaMenu = { x: event.clientX, y: event.clientY };
  }

  /** 移动端空白区长按：与右键同菜单。落在树行上时不处理（行有自己的长按）。 */
  function handleEmptyAreaLongPress(pos: { x: number; y: number }): void {
    // 行长按与空白区长按会被同一次手势先后触发：行菜单已开（或抑制窗内）时跳过，
    // 否则 elementFromPoint 会命中刚渲染的菜单浮层，把行菜单替换成空白区菜单。
    if (isLongPressSuppressed() || treeMenu) return;
    if (document.elementFromPoint(pos.x, pos.y)?.closest(".tree-row")) return;
    treeMenu = null;
    emptyAreaMenu = { x: pos.x, y: pos.y };
  }

  function startSidebarResize(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const startX = event.clientX / scale;
    const startWidth = sidebarWidth;
    const onMove = (moveEvent: MouseEvent): void => {
      sidebarWidth = Math.min(520, Math.max(250, startWidth + moveEvent.clientX / scale - startX));
    };
    const onUp = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<aside class="sidebar" class:searching={$isMobile && $isSearching} style={`width: ${sidebarWidth}px; min-width: ${sidebarWidth}px;`} on:click|stopPropagation>
  <button class="profile-card" type="button" on:click|stopPropagation={() => { showSettings.update((v) => !v); }}>
    <span class="avatar" style={avStyle}>{$appSettings.profile.avatar ? "" : avInitial}</span>
    <span class="profile-text">
      <strong>{$appSettings.profile.displayName}</strong>
      <span>{$appSettings.profile.email}</span>
    </span>
  </button>

  <label class="search-box">
    <Search size={19} />
    <input bind:this={searchInput} bind:value={$searchQuery} placeholder="搜索" />
  </label>

  {#if $isMobile && $isSearching}
    <SearchResults bind:this={searchResultsRef} />
  {/if}

  <nav
    bind:this={navEl}
    class="system-nav"
    class:nav-grid={navLayout === "grid"}
    class:nav-icons={navLayout === "icons"}
    class:nav-dragging={navDrag !== null}
  >
    {#each navRows as row, index (row.id)}
      {@const toolRow = navIdToolId(row.id) !== null}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <button
        class="nav-row"
        class:selected={row.selected}
        class:nav-drag-source={navDrag?.id === row.id}
        type="button"
        title={row.label}
        on:click={() => { if (!navDragSuppressClick) row.onSelect(); }}
        on:pointerdown={(event) => navPointerDown(event, row.id)}
        on:contextmenu={(event) => { if (toolRow) handleNavContext(event, row.id); }}
        use:longpress={(pos) => { if (toolRow) openNavMenuAt(pos.x, pos.y, row.id); }}
        animate:flip={{ duration: 150 }}
      >
        <span class="active-rail"></span>
        <span class="system-icon">
          {#if row.component}
            <svelte:component this={row.component} size={19} />
          {:else}
            <IconGlyph icon={row.glyph ?? "notebook"} size={19} />
          {/if}
        </span>
        {#if navLayout !== "icons"}
          <span class="list-name">{row.label}</span>
        {/if}
        {#if row.count}
          <span class="count-pill">{row.count}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="nav-divider"></div>

  <nav class="custom-nav" class:root-drop-active={draggingId !== null} use:longpress={handleEmptyAreaLongPress} on:contextmenu={openEmptyAreaMenu} on:click|stopPropagation>
    <ListTree
      nodes={treePreview ?? $appState.nodes}
      selectedNodeId={diaryActive || ledgerActive ? "" : $appState.selectedNodeId}
      counts={$listCounts}
      showCategoryCounts={$appSettings.features.showCategoryBadges}
      {renamingId}
      {renameDraft}
      {draggingId}
      on:selectEntry={(e) => selectNode(e.detail)}
      on:toggleCategory={(e) => toggleCategory(e.detail)}
      on:renameInput={(e) => (renameDraft = e.detail)}
      on:renameCommit={(e) => commitRename(e.detail)}
      on:openMenu={(e) => { treeMenu = e.detail; emptyAreaMenu = null; }}
      on:closeMenu={() => { treeMenu = null; emptyAreaMenu = null; }}
      on:pickIcon={(e) => openIconPicker(e.detail)}
      on:dragStart={(e) => (draggingId = e.detail || null)}
      on:dragHover={handleTreeDragHover}
      on:dropNode={(e) => moveNode(e.detail.id, e.detail.targetId, e.detail.position)}
      on:dropRootEnd={(e) => moveNodeToRootEnd(e.detail)}
      on:dragEnd={() => { draggingId = null; treePreview = null; }}
    />
  </nav>

  {#if navMenu}
    <ContextMenu x={navMenu.x} y={navMenu.y} minWidth={168} onClose={() => (navMenu = null)}>
      <MenuItem icon={PinOff} label="取消固定" onSelect={unpinNavRow} />
    </ContextMenu>
  {/if}

  {#if treeMenu && treeMenuNode}
    <ContextMenu x={treeMenu.x} y={treeMenu.y} minWidth={208} onClose={() => (treeMenu = null)}>
      {#if treeMenuNode.kind === "category"}
        <MenuItem icon={FilePlus2} label="新建条目" onSelect={() => addNode(treeMenuNode.id, "entry")} />
        <MenuItem icon={FolderPlus} label="新建子分类" onSelect={() => addNode(treeMenuNode.id, "category")} />
        <MenuSeparator />
      {/if}
      {#if treeMenuNode.kind !== "system"}
        <MenuItem icon={Pencil} label="重命名" onSelect={() => startRename(treeMenuNode.id)} />
        <MenuItem icon={Shapes} label="选择图标" onSelect={() => openIconPicker(treeMenuNode.id)} />
        {#if treeMenuNode.kind === "category"}
          <MenuItem
            icon={treeMenuNode.collapsed ? ChevronsUpDown : ChevronsDownUp}
            label={treeMenuNode.collapsed ? "展开" : "收起"}
            onSelect={() => toggleCategory(treeMenuNode.id)}
          />
        {/if}
        <MenuItem icon={FolderInput} label="移动到分组">
          <div slot="submenu" class="submenu-list">
            {#each treeMoveTargets as target (target.id)}
              <MenuItem
                label={target.name}
                active={(treeMenuNode.parentId ?? "") === target.id}
                onSelect={() => moveNodeToGroup(treeMenuNode.id, target.id || null)}
              />
            {:else}
              <div class="menu-empty">没有可移动的目标</div>
            {/each}
          </div>
        </MenuItem>
        <MenuItem icon={Upload} label="导出" onSelect={() => void exportNode(treeMenuNode.id)} />
        <MenuSeparator />
        <MenuItem icon={Trash2} danger label="删除" onSelect={() => deleteNode(treeMenuNode.id)} />
      {/if}
    </ContextMenu>
  {/if}

  {#if emptyAreaMenu}
    <ContextMenu x={emptyAreaMenu.x} y={emptyAreaMenu.y} minWidth={208} onClose={() => (emptyAreaMenu = null)}>
      <MenuItem icon={FilePlus2} label="新建条目" onSelect={() => addNode(currentCategoryId(), "entry")} />
      <MenuItem icon={FolderPlus} label="新建分类" onSelect={() => addNode(null, "category")} />
    </ContextMenu>
  {/if}

  {#if selectedIconPickerList}
    <!-- 懒加载：IconPicker 会带进 emoji-picker-element（自带整份 emoji 数据库）。
         Sidebar 在首屏链上，静态引入就等于让每个用户一启动就付这份体积。
         App.svelte 与 MarkdownEditorModal 里那两处也是同一套写法。 -->
    {#await import("./IconPicker.svelte") then module}
      <svelte:component
        this={module.default}
        mode="icon"
        selected={selectedIconPickerList.icon}
        onPick={pickIcon}
        onClose={() => (iconPickerListId = null)}
      />
    {:catch error}
      <!-- 拉不到 chunk（网络/缓存出问题）时**必须说一声**：只写 then 的话
           点开图标选择器会是「什么都没有」，用户不知道发生了什么也退不出去 -->
      <div class="lazy-fallback" role="alert">
        打开图标选择器失败：{String(error)}
        <button class="settings-button" type="button" on:click={() => (iconPickerListId = null)}>关闭</button>
      </div>
    {/await}
  {/if}

  <div class="sidebar-footer" on:click|stopPropagation>
    <button type="button" on:click={() => addNode(currentCategoryId(), "entry")}>
      <FilePlus2 size={23} />
      新建条目
    </button>
    <button type="button" title="新建分类" on:click={() => addNode(null, "category")}>
      <FolderPlus size={22} />
    </button>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle" on:mousedown={startSidebarResize}></div>
</aside>

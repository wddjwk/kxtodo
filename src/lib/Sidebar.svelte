<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import {
    ChevronsDownUp, ChevronsUpDown, FilePlus2, FolderInput, FolderPlus, Pencil, Search, Shapes, Trash2, Upload
  } from "@lucide/svelte";
  import {
    appState, appSettings, showToast, showSettings,
    searchQuery, listCounts, isSearching,
    now, safeFileName, appVersion
  } from "./stores";
  import {
    selectNode as selectNodeAction, toggleCategory as toggleCategoryAction,
    addNode as addNodeAction, renameNode as renameNodeAction,
    deleteNodeCascade as deleteNodeCascadeAction, applyTreeOrder as applyTreeOrderAction,
    setNodeIcon as setNodeIconAction
  } from "./actions";
  import { nodeAndDescendantIds, moveTargetOptions, exportStateForNode } from "./nodes";
  import { uiScaleValue, avatarStyle, avatarInitial } from "./styles";
  import { avatarCache, resolveAvatarSrc, isLocalImageRef, localImageFilename } from "./images";
  import { deleteBackgroundImage, deleteNodeImages, exportData } from "./backend";
  import IconGlyph from "./IconGlyph.svelte";
  import IconPicker from "./IconPicker.svelte";
  import ListTree from "./ListTree.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import { showMobileContent } from "./platform";

  const dispatch = createEventDispatcher<{ suppressClose: void }>();

  let searchInput: HTMLInputElement;
  let sidebarWidth = 320;
  let renamingId: string | null = null;
  let renameDraft = "";
  let iconPickerListId: string | null = null;
  let treeMenu: { id: string; x: number; y: number } | null = null;
  let emptyAreaMenu: { x: number; y: number } | null = null;
  let draggingId: string | null = null;
  let ignoreOverlayCloseOnce = false;

  $: treeMenuNode = treeMenu ? $appState.nodes.find((n) => n.id === treeMenu?.id) : null;
  $: treeMoveTargets = treeMenuNode ? moveTargetOptions(treeMenuNode.id, $appState.nodes) : [];
  $: selectedIconPickerList = iconPickerListId ? $appState.nodes.find((n) => n.id === iconPickerListId) : null;
  $: resolvedAvatar = resolveAvatarSrc($appSettings.profile.avatar, $avatarCache);
  $: avStyle = avatarStyle(resolvedAvatar);
  $: avInitial = avatarInitial($appSettings.profile.displayName);

  export function closeOverlays(): void {
    if (ignoreOverlayCloseOnce) {
      ignoreOverlayCloseOnce = false;
      return;
    }
    treeMenu = null;
    emptyAreaMenu = null;
    iconPickerListId = null;
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
    void selectNodeAction(id);
    treeMenu = null;
    emptyAreaMenu = null;
    iconPickerListId = null;
    showMobileContent();
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
    const source = $appState.nodes.find((n) => n.id === id);
    const target = $appState.nodes.find((n) => n.id === targetId);
    if (!source || !target || source.kind === "system" || target.kind === "system") return;
    if (source.id === target.id || nodeAndDescendantIds(source.id, $appState.nodes).has(target.id)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }
    if (position === "inside" && target.kind !== "category") return;
    const nextParentId = position === "inside" ? target.id : target.parentId;
    const sourceWithParent = { ...source, parentId: nextParentId };
    const withoutSource = $appState.nodes.filter((n) => n.id !== id);
    const targetIndex = withoutSource.findIndex((n) => n.id === target.id);
    let insertIndex = withoutSource.length;
    if (position === "before") {
      insertIndex = targetIndex >= 0 ? targetIndex : withoutSource.length;
    } else if (position === "after") {
      insertIndex = targetIndex >= 0 ? targetIndex + 1 : withoutSource.length;
    } else {
      const childIndexes = withoutSource
        .map((n, i) => ({ n, i }))
        .filter((item) => item.n.parentId === target.id)
        .map((item) => item.i);
      insertIndex = childIndexes.length ? Math.max(...childIndexes) + 1 : targetIndex >= 0 ? targetIndex + 1 : withoutSource.length;
    }
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, sourceWithParent);
    const ordered = nodes.map((n) => (position === "inside" && n.id === target.id ? { ...n, collapsed: false } : n));
    void applyTreeOrderAction(ordered, { [id]: nextParentId });
    draggingId = null;
  }

  /** 拖到空白区：移动为根级最后一项。 */
  function moveNodeToRootEnd(id: string): void {
    const source = $appState.nodes.find((n) => n.id === id);
    if (!source || source.kind === "system") return;
    const withoutSource = $appState.nodes.filter((n) => n.id !== id);
    const rootIndexes = withoutSource
      .map((n, i) => ({ n, i }))
      .filter((item) => !item.n.parentId && item.n.kind !== "system")
      .map((item) => item.i);
    const insertIndex = rootIndexes.length ? Math.max(...rootIndexes) + 1 : withoutSource.length;
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, { ...source, parentId: null });
    void applyTreeOrderAction(nodes, { [id]: null });
    draggingId = null;
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
    treeMenu = null;
    emptyAreaMenu = { x: event.clientX, y: event.clientY };
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
<aside class="sidebar" style={`width: ${sidebarWidth}px; min-width: ${sidebarWidth}px;`} on:click|stopPropagation>
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

  <nav class="system-nav">
    {#each $appState.nodes.filter((n) => n.kind === "system") as node (node.id)}
      <button class:selected={$appState.selectedNodeId === node.id && !$isSearching} class="nav-row" type="button" on:click={() => selectNode(node.id)}>
        <span class="active-rail"></span>
        <span class="system-icon"><IconGlyph icon={node.icon} size={19} /></span>
        <span class="list-name">{node.name}</span>
        {#if $listCounts[node.id]}
          <span class="count-pill">{$listCounts[node.id]}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="nav-divider"></div>

  <nav class="custom-nav" class:root-drop-active={draggingId !== null} on:contextmenu={openEmptyAreaMenu} on:click|stopPropagation>
    <ListTree
      nodes={$appState.nodes}
      selectedNodeId={$appState.selectedNodeId}
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
      on:pickIcon={(e) => openIconPicker(e.detail)}
      on:dragStart={(e) => (draggingId = e.detail || null)}
      on:dropNode={(e) => moveNode(e.detail.id, e.detail.targetId, e.detail.position)}
      on:dropRootEnd={(e) => moveNodeToRootEnd(e.detail)}
      on:dragEnd={() => (draggingId = null)}
    />
  </nav>

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
    <IconPicker mode="icon" selected={selectedIconPickerList.icon} onPick={pickIcon} onClose={() => (iconPickerListId = null)} />
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

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import {
    FilePlus2, FolderInput, FolderPlus, Pencil, Search, Star, Trash2
  } from "@lucide/svelte";
  import {
    appState, appSettings, commit, showToast, showSettings,
    searchQuery, listCounts, isSearching
  } from "./stores";
  import {
    nodeAndDescendantIds, moveTargetOptions
  } from "./nodes";
  import {
    createCategoryNode, createEntryNode, defaultBackground
  } from "./defaults";
  import { uiScaleValue, buildMenuStyle, avatarStyle, avatarInitial } from "./styles";
  import { avatarCache, resolveAvatarSrc, isLocalImageRef, localImageFilename } from "./images";
  import { deleteBackgroundImage, deleteNodeImages } from "./backend";
  import IconGlyph from "./IconGlyph.svelte";
  import IconPicker from "./IconPicker.svelte";
  import ListTree from "./ListTree.svelte";
  import { showMobileContent } from "./platform";
  import type { AppNode } from "./types";

  const dispatch = createEventDispatcher<{ suppressClose: void }>();

  let searchInput: HTMLInputElement;
  let sidebarWidth = 320;
  let renamingId: string | null = null;
  let renameDraft = "";
  let iconPickerListId: string | null = null;
  let treeMenu: { id: string; x: number; y: number } | null = null;
  let showTreeMove = false;
  let draggingId: string | null = null;
  let ignoreOverlayCloseOnce = false;

  $: treeMenuNode = treeMenu ? $appState.nodes.find((n) => n.id === treeMenu?.id) : null;
  $: treeMoveTargets = treeMenuNode ? moveTargetOptions(treeMenuNode.id, $appState.nodes) : [];
  $: treeMenuStyle = treeMenu ? buildMenuStyle(treeMenu.x, treeMenu.y, 248, treeMenuNode?.kind === "category" ? 300 : 252, uiScaleValue($appSettings.appearance.uiScale)) : "";
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
    commit({ ...$appState, selectedNodeId: id });
    treeMenu = null;
    iconPickerListId = null;
    showMobileContent();
  }

  function toggleCategory(id: string): void {
    commit({
      ...$appState,
      nodes: $appState.nodes.map((n) => (n.id === id ? { ...n, collapsed: !n.collapsed } : n))
    });
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
    const node = kind === "category" ? createCategoryNode("未命名分类", parentId) : createEntryNode("未命名条目", parentId);
    const backgrounds = kind === "entry" ? { ...$appState.backgrounds, [node.id]: { ...defaultBackground } } : $appState.backgrounds;
    const nodes = $appState.nodes.map((item) => (item.id === parentId ? { ...item, collapsed: false } : item));
    commit({
      ...$appState,
      nodes: [...nodes, node],
      selectedNodeId: kind === "entry" ? node.id : $appState.selectedNodeId,
      backgrounds
    });
    renamingId = node.id;
    renameDraft = node.name;
    treeMenu = null;
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
    commit({
      ...$appState,
      nodes: $appState.nodes.map((n) => (n.id === id ? { ...n, name } : n))
    });
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
    let nodes = $appState.nodes.filter((n) => !ids.has(n.id));
    let backgrounds = Object.fromEntries(Object.entries($appState.backgrounds).filter(([key]) => !ids.has(key)));
    if (!nodes.some((n) => n.kind === "entry")) {
      const inbox = createEntryNode("收集箱", null, "inbox");
      nodes = [...nodes, inbox];
      backgrounds = { ...backgrounds, [inbox.id]: { ...defaultBackground } };
    }
    const validNodeIds = new Set(nodes.map((n) => n.id));
    const fallbackId = validNodeIds.has($appState.selectedNodeId) ? $appState.selectedNodeId : nodes.find((n) => n.kind === "entry")?.id ?? "my-day";
    commit({
      ...$appState,
      nodes,
      tasks: $appState.tasks.filter((t) => validNodeIds.has(t.nodeId)),
      selectedNodeId: fallbackId,
      backgrounds
    });
    treeMenu = null;
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
    commit({
      ...$appState,
      nodes: nodes.map((n) => (position === "inside" && n.id === target.id ? { ...n, collapsed: false } : n))
    });
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
    commit({
      ...$appState,
      nodes: nodes.map((n) => (nextParentId && n.id === nextParentId ? { ...n, collapsed: false } : n))
    });
    treeMenu = null;
    draggingId = null;
  }

  function openIconPicker(id: string): void {
    iconPickerListId = id;
    treeMenu = null;
    showSettings.set(false);
    ignoreOverlayCloseOnce = true;
    dispatch("suppressClose");
    window.setTimeout(() => {
      ignoreOverlayCloseOnce = false;
    }, 250);
  }

  function pickIcon(icon: string): void {
    if (!selectedIconPickerList) return;
    commit({
      ...$appState,
      nodes: $appState.nodes.map((n) => (n.id === selectedIconPickerList!.id ? { ...n, icon } : n))
    });
    iconPickerListId = null;
  }

  function handleTreePointerDownCapture(event: PointerEvent): void {
    openIconPickerFromTreeEvent(event);
  }

  function handleTreeClickCapture(event: MouseEvent): void {
    openIconPickerFromTreeEvent(event);
  }

  function openIconPickerFromTreeEvent(event: MouseEvent | PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (!target.closest(".tree-icon")) return;
    const row = target.closest<HTMLElement>(".tree-row[data-node-id]");
    if (!row) return;
    const id = row.dataset.nodeId;
    const node = $appState.nodes.find((item) => item.id === id);
    if (!node || node.kind === "system") return;
    event.preventDefault();
    event.stopPropagation();
    openIconPicker(node.id);
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

  <nav class="custom-nav" on:pointerdown|capture={handleTreePointerDownCapture} on:click|capture={handleTreeClickCapture} on:click|stopPropagation>
    <ListTree
      nodes={$appState.nodes}
      selectedNodeId={$appState.selectedNodeId}
      counts={$listCounts}
      {renamingId}
      {renameDraft}
      on:selectEntry={(e) => selectNode(e.detail)}
      on:toggleCategory={(e) => toggleCategory(e.detail)}
      on:renameInput={(e) => (renameDraft = e.detail)}
      on:renameCommit={(e) => commitRename(e.detail)}
      on:openMenu={(e) => { treeMenu = e.detail; showTreeMove = false; }}
      requestIconPicker={openIconPicker}
      on:pickIcon={(e) => openIconPicker(e.detail)}
      on:dragStart={(e) => (draggingId = e.detail || null)}
      on:dropNode={(e) => moveNode(e.detail.id, e.detail.targetId, e.detail.position)}
      {draggingId}
    />
  </nav>

  {#if treeMenu && treeMenuNode}
    <section class="tree-context-menu" style={treeMenuStyle} on:click|stopPropagation>
      {#if treeMenuNode.kind === "category"}
        <button type="button" on:click={() => addNode(treeMenuNode.id, "entry")}><FilePlus2 size={15} /> 创建条目</button>
        <button type="button" on:click={() => addNode(treeMenuNode.id, "category")}><FolderPlus size={15} /> 创建子分类</button>
      {/if}
      <button type="button" disabled={treeMenuNode.kind === "system"} on:click={() => startRename(treeMenuNode.id)}><Pencil size={15} /> 重命名</button>
      <button type="button" disabled={treeMenuNode.kind === "system"} on:click={() => openIconPicker(treeMenuNode.id)}><Star size={15} /> 选择图标</button>
      {#if treeMenuNode.kind !== "system"}
        <button type="button" class="has-submenu" on:click={() => (showTreeMove = !showTreeMove)}>
          <FolderInput size={15} /> 移动到分组
        </button>
        {#if showTreeMove}
          <div class="menu-submenu">
            {#each treeMoveTargets as target}
              <button
                type="button"
                class:active={(treeMenuNode.parentId ?? "") === target.id}
                on:click={() => moveNodeToGroup(treeMenuNode.id, target.id || null)}
              >{target.name}</button>
            {/each}
          </div>
        {/if}
      {/if}
      <button class="danger" type="button" disabled={treeMenuNode.kind === "system"} on:click={() => deleteNode(treeMenuNode.id)}><Trash2 size={15} /> 删除</button>
    </section>
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

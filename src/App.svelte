<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    Check,
    ChevronDown,
    Download,
    Eraser,
    FilePlus2,
    FolderPlus,
    Image,
    Minus,
    MoreHorizontal,
    Pencil,
    Plus,
    Search,
    Settings,
    Square,
    Trash2,
    Upload,
    X
  } from "@lucide/svelte";
  import { exportData, loadSettings, loadState, registerGlobalShortcut, saveSettings, saveState } from "./lib/backend";
  import { createId, defaultSettings, defaultState, normalizeState } from "./lib/defaults";
  import IconGlyph from "./lib/IconGlyph.svelte";
  import IconPicker from "./lib/IconPicker.svelte";
  import ListTree from "./lib/ListTree.svelte";
  import TaskCard from "./lib/TaskCard.svelte";
  import { renderMarkdown } from "./lib/markdown";
  import { matchesShortcut, shortcutLabel } from "./lib/shortcuts";
  import { syncRoadmap } from "./lib/sync";
  import type {
    AppSettings,
    AppState,
    ExportPayload,
    ListNodeType,
    ShortcutBinding,
    TodoList,
    TodoTask,
    TodoTaskPatch
  } from "./lib/types";

  const defaultAccent = "#b64a30";
  const themePresets = [
    { name: "桃杏", accent: "#b64a30", background: "#fae9df" },
    { name: "To Do 蓝", accent: "#2564cf", background: "#edf5ff" },
    { name: "薄荷", accent: "#1f7a4d", background: "#edf7f0" },
    { name: "丁香", accent: "#6d5bd0", background: "#f1efff" },
    { name: "砂岩", accent: "#8a5a44", background: "#f6ede6" },
    { name: "玫瑰", accent: "#c2416b", background: "#fff1f4" },
    { name: "琥珀", accent: "#b7791f", background: "#fff8e6" },
    { name: "青绿", accent: "#0f766e", background: "#e8f7f4" },
    { name: "海盐", accent: "#34748f", background: "#eaf7fb" },
    { name: "葡萄", accent: "#7c3aed", background: "#f4efff" },
    { name: "石墨", accent: "#475569", background: "#eef1f4" },
    { name: "夜灰", accent: "#d7dce2", background: "#202329" }
  ];

  let state: AppState = defaultState();
  let settings: AppSettings = defaultSettings();
  let hydrated = false;
  let searchQuery = "";
  let newTaskDraft = "";
  let selectedTaskId: string | null = null;
  let showSettings = false;
  let showListMenu = false;
  let showCompleted = true;
  let editingListId: string | null = null;
  let renameDraft = "";
  let iconPickerListId: string | null = null;
  let treeMenu: { id: string; x: number; y: number } | null = null;
  let draggingId: string | null = null;
  let backgroundLinkDraft = "";
  let toast = "";
  let saveTimer: number | undefined;
  let searchInput: HTMLInputElement;
  let taskInput: HTMLTextAreaElement;
  let importInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;
  let avatarFileInput: HTMLInputElement;

  $: selectedList = state.lists.find((list) => list.id === state.selectedListId) ?? state.lists[0];
  $: selectedTheme = selectedList?.theme ?? settings.defaultListTheme;
  $: customLists = state.lists.filter((list) => list.kind === "custom");
  $: entryLists = customLists.filter((list) => list.nodeType === "entry");
  $: systemLists = state.lists.filter((list) => list.kind === "system").sort((a, b) => a.order - b.order);
  $: listCounts = buildListCounts(state);
  $: selectedIds = selectedList ? collectListIds(state.lists, selectedList.id) : new Set<string>();
  $: isSearching = searchQuery.trim().length > 0;
  $: visibleTasks = filterTasks(state.tasks, selectedList, selectedIds, searchQuery);
  $: incompleteTasks = visibleTasks.filter((task) => !task.completed);
  $: completedTasks = visibleTasks.filter((task) => task.completed);
  $: completedCount = completedTasks.length;
  $: mainStyle = buildMainStyle(selectedTheme.background, selectedTheme.image, selectedTheme.imageOpacity);
  $: selectedIconPickerList = iconPickerListId ? state.lists.find((list) => list.id === iconPickerListId) : null;
  $: treeMenuNode = treeMenu ? state.lists.find((list) => list.id === treeMenu?.id) : null;

  onMount(() => {
    void hydrate();
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  });

  async function hydrate(): Promise<void> {
    let shortcutToRegister = settings.globalShortcut;
    try {
      const [loadedState, loadedSettings] = await Promise.all([loadState(), loadSettings()]);
      state = loadedState;
      settings = loadedSettings;
      shortcutToRegister = loadedSettings.globalShortcut;
    } catch (error) {
      showToast(`加载本地数据失败，已使用默认数据：${String(error)}`);
    } finally {
      hydrated = true;
      await tickResizeComposer();
    }

    try {
      await registerGlobalShortcut(shortcutToRegister);
    } catch (error) {
      showToast(`全局快捷键注册失败：${String(error)}`);
    }
  }

  function closeOverlays(): void {
    showListMenu = false;
    treeMenu = null;
    iconPickerListId = null;
    showSettings = false;
  }

  function now(): string {
    return new Date().toISOString();
  }

  function todayIso(): string {
    const date = new Date();
    const offset = date.getTimezoneOffset();
    const local = new Date(date.getTime() - offset * 60_000);
    return local.toISOString().slice(0, 10);
  }

  function queueStateSave(): void {
    if (!hydrated) {
      return;
    }

    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      saveState(state).catch((error) => showToast(`保存失败：${String(error)}`));
    }, 180);
  }

  function commit(next: AppState): void {
    state = { ...next, updatedAt: now() };
    queueStateSave();
  }

  function commitSettings(next: AppSettings): void {
    settings = next;
    if (hydrated) {
      saveSettings(settings).catch((error) => showToast(`保存设置失败：${String(error)}`));
    }
  }

  function buildMainStyle(background: string, image?: string, imageOpacity = 0.28): string {
    const safeImage = image?.trim().replace(/"/g, "%22");
    return `--accent: ${selectedTheme.accent}; --bg-image: ${
      safeImage ? `url("${safeImage}")` : "none"
    }; --bg-opacity: ${safeImage ? imageOpacity : 0}; background-color: ${background};`;
  }

  function collectListIds(lists: TodoList[], rootId: string): Set<string> {
    const ids = new Set([rootId]);
    let changed = true;

    while (changed) {
      changed = false;
      for (const list of lists) {
        if (list.parentId && ids.has(list.parentId) && !ids.has(list.id)) {
          ids.add(list.id);
          changed = true;
        }
      }
    }

    return ids;
  }

  function buildListCounts(source: AppState): Record<string, number> {
    const counts: Record<string, number> = {};
    for (const list of source.lists) {
      const ids = collectListIds(source.lists, list.id);
      counts[list.id] = source.tasks.filter((task) => ids.has(task.listId) && !task.completed).length;
    }
    counts.favorite = source.tasks.filter((task) => task.important && !task.completed).length;
    counts.planned = source.tasks.filter((task) => Boolean(task.dueDate) && !task.completed).length;
    counts["my-day"] = source.tasks.filter((task) => isMyDayTask(task) && !task.completed).length;
    return counts;
  }

  function filterTasks(tasks: TodoTask[], list: TodoList | undefined, ids: Set<string>, query: string): TodoTask[] {
    const normalized = query.trim().toLowerCase();
    return tasks.filter((task) => {
      if (normalized) {
        return `${task.markdown}\n${task.tags.join(" ")}`.toLowerCase().includes(normalized);
      }

      return (
        !list ||
        (list.id === "favorite" && task.important) ||
        (list.id === "planned" && Boolean(task.dueDate)) ||
        (list.id === "my-day" && isMyDayTask(task)) ||
        (list.kind === "custom" && ids.has(task.listId))
      );
    });
  }

  function isMyDayTask(task: TodoTask): boolean {
    return task.dueDate === "今天" || task.dueDate === todayIso();
  }

  function selectEntry(id: string): void {
    const list = state.lists.find((item) => item.id === id);
    if (!list || list.nodeType !== "entry") {
      return;
    }
    selectedTaskId = null;
    showListMenu = false;
    treeMenu = null;
    commit({ ...state, selectedListId: id });
  }

  function toggleCategory(id: string): void {
    treeMenu = null;
    commit({
      ...state,
      lists: state.lists.map((list) => (list.id === id ? { ...list, collapsed: !list.collapsed } : list))
    });
  }

  function currentCategoryId(): string | null {
    if (selectedList?.kind === "custom" && selectedList.nodeType === "category") {
      return selectedList.id;
    }
    if (selectedList?.kind === "custom") {
      return selectedList.parentId;
    }
    return null;
  }

  function addNode(parentId: string | null, nodeType: ListNodeType): void {
    const id = createId(nodeType);
    const parent = parentId ? state.lists.find((list) => list.id === parentId) : null;
    const order = state.lists.filter((list) => list.parentId === parentId).length + 10;
    const list: TodoList = {
      id,
      parentId,
      kind: "custom",
      nodeType,
      name: nodeType === "category" ? "新建分类" : "新建条目",
      icon: nodeType === "category" ? "folder" : "notebook",
      collapsed: false,
      shared: false,
      order,
      theme: parent?.theme ?? settings.defaultListTheme
    };

    editingListId = id;
    renameDraft = list.name;
    treeMenu = null;
    commit({
      ...state,
      selectedListId: nodeType === "entry" ? id : state.selectedListId,
      lists: state.lists.map((item) => (item.id === parentId ? { ...item, collapsed: false } : item)).concat(list)
    });
  }

  function startRename(id: string): void {
    const list = state.lists.find((item) => item.id === id);
    if (!list || list.kind === "system") {
      return;
    }
    editingListId = id;
    renameDraft = list.name;
    showListMenu = false;
    treeMenu = null;
  }

  function commitRename(): void {
    if (!editingListId) {
      return;
    }

    const name = renameDraft.trim() || "未命名";
    const id = editingListId;
    editingListId = null;
    commit({
      ...state,
      lists: state.lists.map((list) => (list.id === id ? { ...list, name } : list))
    });
  }

  function deleteNode(id: string): void {
    const list = state.lists.find((item) => item.id === id);
    if (!list || list.kind === "system") {
      return;
    }

    if (!window.confirm(`删除“${list.name}”及其子内容？`)) {
      return;
    }

    const ids = collectListIds(state.lists, id);
    const remainingLists = state.lists.filter((item) => !ids.has(item.id));
    const fallback = remainingLists.find((item) => item.kind === "custom" && item.nodeType === "entry")?.id ?? "my-day";
    treeMenu = null;
    commit({
      ...state,
      selectedListId: ids.has(state.selectedListId) ? fallback : state.selectedListId,
      lists: remainingLists,
      tasks: state.tasks.filter((task) => !ids.has(task.listId))
    });
  }

  function openTreeMenu(id: string, x: number, y: number): void {
    showListMenu = false;
    treeMenu = { id, x, y };
  }

  function canMoveNode(sourceId: string, targetId: string): boolean {
    if (!sourceId || sourceId === targetId) {
      return false;
    }
    const target = state.lists.find((list) => list.id === targetId);
    if (!target || target.nodeType !== "category") {
      return false;
    }
    return !collectListIds(state.lists, sourceId).has(targetId);
  }

  function dropNode(targetId: string): void {
    if (!draggingId || !canMoveNode(draggingId, targetId)) {
      draggingId = null;
      return;
    }

    const order = state.lists.filter((list) => list.parentId === targetId).length + 10;
    const sourceId = draggingId;
    draggingId = null;
    commit({
      ...state,
      lists: state.lists.map((list) => {
        if (list.id === targetId) {
          return { ...list, collapsed: false };
        }
        if (list.id === sourceId) {
          return { ...list, parentId: targetId, order };
        }
        return list;
      })
    });
  }

  function pickIcon(icon: string): void {
    if (!iconPickerListId) {
      return;
    }
    const id = iconPickerListId;
    iconPickerListId = null;
    commit({
      ...state,
      lists: state.lists.map((list) => (list.id === id ? { ...list, icon } : list))
    });
  }

  function createTask(): void {
    const markdown = newTaskDraft.trim();
    if (!markdown) {
      return;
    }

    const fallbackList = entryLists[0]?.id ?? "quick-notes";
    const targetListId = selectedList?.kind === "custom" && selectedList.nodeType === "entry" ? selectedList.id : fallbackList;
    const task: TodoTask = {
      id: createId("task"),
      listId: targetListId,
      markdown,
      completed: false,
      important: selectedList?.id === "favorite",
      expanded: false,
      steps: [],
      notes: "",
      dueDate: selectedList?.id === "my-day" ? todayIso() : null,
      reminder: null,
      repeat: null,
      tags: [],
      createdAt: now(),
      updatedAt: now()
    };

    selectedTaskId = task.id;
    newTaskDraft = "";
    commit({ ...state, selectedListId: targetListId, tasks: [task, ...state.tasks] });
    void tickResizeComposer();
  }

  function updateTask(id: string, patch: TodoTaskPatch): void {
    commit({
      ...state,
      tasks: state.tasks.map((task) => (task.id === id ? { ...task, ...patch, updatedAt: now() } : task))
    });
  }

  function selectTask(id: string): void {
    selectedTaskId = id;
    updateTask(id, { expanded: !state.tasks.find((task) => task.id === id)?.expanded });
  }

  function applyTheme(accent: string, background: string): void {
    if (!selectedList) {
      return;
    }

    commit({
      ...state,
      lists: state.lists.map((list) =>
        list.id === selectedList.id ? { ...list, theme: { ...list.theme, accent, background } } : list
      )
    });
  }

  function updateBackgroundImage(image?: string): void {
    if (!selectedList) {
      return;
    }

    commit({
      ...state,
      lists: state.lists.map((list) =>
        list.id === selectedList.id ? { ...list, theme: { ...list.theme, image } } : list
      )
    });
  }

  function updateBackgroundOpacity(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !selectedList) {
      return;
    }
    const imageOpacity = Number(target.value) / 100;
    commit({
      ...state,
      lists: state.lists.map((list) =>
        list.id === selectedList.id ? { ...list, theme: { ...list.theme, imageOpacity } } : list
      )
    });
  }

  function updateBackgroundLink(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    backgroundLinkDraft = target.value;
    updateBackgroundImage(backgroundLinkDraft.trim() || undefined);
  }

  async function uploadBackgroundImage(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }
    updateBackgroundImage(await fileToDataUrl(target.files[0]));
    target.value = "";
  }

  async function uploadAvatar(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }
    updateProfile("avatar", await fileToDataUrl(target.files[0]));
    target.value = "";
  }

  async function fileToDataUrl(file: File): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  function openListMenu(): void {
    treeMenu = null;
    showListMenu = !showListMenu;
    backgroundLinkDraft = selectedTheme.image ?? "";
  }

  async function exportCurrentList(): Promise<void> {
    const path = await exportData({ scope: "list", listId: selectedList?.id, state });
    if (path !== "cancelled") {
      showToast(`已导出当前条目/分类：${path}`);
    }
  }

  async function exportAll(): Promise<void> {
    const path = await exportData({ scope: "all", state });
    if (path !== "cancelled") {
      showToast(`已导出全部数据：${path}`);
    }
  }

  async function importFromFile(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }

    const text = await target.files[0].text();
    const payload = JSON.parse(text) as ExportPayload;

    if (payload.state) {
      commit(normalizeState(payload.state));
      showToast("已导入完整数据");
    } else if (payload.lists && payload.tasks) {
      const existingListIds = new Set(state.lists.map((list) => list.id));
      const existingTaskIds = new Set(state.tasks.map((task) => task.id));
      commit({
        ...state,
        selectedListId: payload.rootListId ?? state.selectedListId,
        lists: [...state.lists, ...payload.lists.filter((list) => !existingListIds.has(list.id))],
        tasks: [...state.tasks, ...payload.tasks.filter((task) => !existingTaskIds.has(task.id))]
      });
      showToast("已导入分类/条目数据");
    } else {
      showToast("无法识别导入文件");
    }

    target.value = "";
  }

  function updateShortcut(binding: ShortcutBinding, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }

    commitSettings({
      ...settings,
      shortcuts: settings.shortcuts.map((shortcut) =>
        shortcut.id === binding.id ? { ...shortcut, combo: target.value.trim() } : shortcut
      )
    });
  }

  function updateProfile(field: "avatar" | "name" | "email", value: string): void {
    commitSettings({
      ...settings,
      profile: { ...settings.profile, [field]: value }
    });
  }

  async function updateGlobalShortcut(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }

    const globalShortcut = target.value.trim() || defaultSettings().globalShortcut;
    commitSettings({ ...settings, globalShortcut });
    try {
      await registerGlobalShortcut(globalShortcut);
      showToast(`全局唤起快捷键已注册：${globalShortcut}`);
    } catch (error) {
      showToast(`全局快捷键注册失败：${String(error)}`);
    }
  }

  function setDensity(taskDensity: AppSettings["taskDensity"]): void {
    commitSettings({ ...settings, taskDensity });
  }

  function setSidebarWidth(width: number): void {
    commitSettings({ ...settings, sidebarWidth: Math.min(520, Math.max(240, Math.round(width))) });
  }

  function startSidebarResize(event: MouseEvent): void {
    const startX = event.clientX;
    const startWidth = settings.sidebarWidth;

    function onMove(moveEvent: MouseEvent): void {
      setSidebarWidth(startWidth + moveEvent.clientX - startX);
    }

    function onUp(): void {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function resizeComposer(): void {
    if (!taskInput) {
      return;
    }
    taskInput.style.height = "auto";
    taskInput.style.height = `${Math.min(taskInput.scrollHeight, 180)}px`;
  }

  async function tickResizeComposer(): Promise<void> {
    await Promise.resolve();
    resizeComposer();
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      createTask();
    }
  }

  function handleShortcut(event: KeyboardEvent): void {
    const target = event.target;
    const isTyping =
      target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;

    for (const shortcut of settings.shortcuts) {
      if (!matchesShortcut(event, shortcut)) {
        continue;
      }

      if (isTyping && shortcut.id !== "search") {
        return;
      }

      event.preventDefault();
      if (shortcut.id === "new-task") {
        taskInput?.focus();
      } else if (shortcut.id === "new-category") {
        addNode(null, "category");
      } else if (shortcut.id === "new-child-category") {
        addNode(currentCategoryId(), "category");
      } else if (shortcut.id === "search") {
        searchInput?.focus();
      } else if (shortcut.id === "toggle-settings") {
        showSettings = !showSettings;
      } else if (shortcut.id === "export-list") {
        void exportCurrentList();
      } else if (shortcut.id === "export-all") {
        void exportAll();
      }
      return;
    }
  }

  function avatarStyle(): string {
    const avatar = settings.profile.avatar;
    return avatar.startsWith("data:image") || avatar.startsWith("http") ? `background-image: url("${avatar}")` : "";
  }

  function avatarText(): string {
    const avatar = settings.profile.avatar;
    return avatar.startsWith("data:image") || avatar.startsWith("http") ? "" : avatar || "示";
  }

  function showToast(message: string): void {
    toast = message;
    window.setTimeout(() => {
      if (toast === message) {
        toast = "";
      }
    }, 2800);
  }

  function minimize(): void {
    void getCurrentWindow().minimize();
  }

  function maximize(): void {
    void getCurrentWindow().toggleMaximize();
  }

  function closeWindow(): void {
    void getCurrentWindow().close();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="app-shell" on:click={closeOverlays}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="window-title" data-tauri-drag-region>
      <span class="app-glyph"><Check size={15} strokeWidth={3.2} /></span>
      <span>Todo Note</span>
    </div>
    <div class="window-controls">
      <button type="button" aria-label="最小化" on:click={minimize}><Minus size={15} /></button>
      <button type="button" aria-label="最大化" on:click={maximize}><Square size={13} /></button>
      <button type="button" aria-label="关闭" on:click={closeWindow}><X size={16} /></button>
    </div>
  </header>

  <div class="layout">
    <aside class="sidebar" style={`width: ${settings.sidebarWidth}px; min-width: ${settings.sidebarWidth}px`}>
      <button class="profile-card" type="button" on:click|stopPropagation={() => (showSettings = !showSettings)}>
        <span class="avatar" style={avatarStyle()}>{avatarText()}</span>
        <span class="profile-text">
          <strong>{settings.profile.name}</strong>
          <span>{settings.profile.email}⌄</span>
        </span>
      </button>

      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <label class="search-box" on:click|stopPropagation>
        <input bind:this={searchInput} bind:value={searchQuery} placeholder="搜索" />
        <Search size={18} />
      </label>

      <nav class="system-nav" aria-label="系统列表">
        {#each systemLists as list (list.id)}
          <button class:selected={state.selectedListId === list.id && !isSearching} class="nav-row system-row" type="button" on:click|stopPropagation={() => commit({ ...state, selectedListId: list.id })}>
            <span class="active-rail"></span>
            <span class="system-icon"><IconGlyph icon={list.icon} size={22} /></span>
            <span class="list-name">{list.name}</span>
            {#if listCounts[list.id] > 0}
              <span class="count-pill">{listCounts[list.id]}</span>
            {/if}
          </button>
        {/each}
      </nav>

      <div class="nav-divider"></div>

      <nav class="custom-nav" aria-label="自定义分类">
        <ListTree
          lists={state.lists}
          selectedId={isSearching ? "" : state.selectedListId}
          counts={listCounts}
          editingId={editingListId}
          {renameDraft}
          {draggingId}
          onSelectEntry={selectEntry}
          onToggleCategory={toggleCategory}
          onOpenMenu={openTreeMenu}
          onRenameDraft={(value) => (renameDraft = value)}
          onCommitRename={commitRename}
          onDragStartNode={(id) => (draggingId = id || null)}
          onDropNode={dropNode}
        />
      </nav>

      {#if treeMenuNode && treeMenu}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <section class="tree-context-menu" style={`left: ${treeMenu.x}px; top: ${treeMenu.y}px`} on:click|stopPropagation>
          {#if treeMenuNode.nodeType === "category"}
            <button type="button" on:click={() => addNode(treeMenuNode.id, "entry")}><FilePlus2 size={15} /> 创建条目</button>
            <button type="button" on:click={() => addNode(treeMenuNode.id, "category")}><FolderPlus size={15} /> 创建子分类</button>
          {/if}
          <button type="button" on:click={() => startRename(treeMenuNode.id)}><Pencil size={15} /> 重命名</button>
          {#if treeMenuNode.nodeType === "entry"}
            <button type="button" on:click={() => (iconPickerListId = treeMenuNode.id)}><Settings size={15} /> 选择图标</button>
          {/if}
          <button class="danger" type="button" on:click={() => deleteNode(treeMenuNode.id)}><Trash2 size={15} /> 删除</button>
        </section>
      {/if}

      {#if selectedIconPickerList}
        <IconPicker selected={selectedIconPickerList.icon} onPick={pickIcon} onClose={() => (iconPickerListId = null)} />
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

    <main class="workspace" style={mainStyle}>
      <section class="list-header">
        <div>
          <span class="header-icon">
            {#if isSearching}
              <Search size={34} />
            {:else}
              <IconGlyph icon={selectedList?.nodeType === "category" ? "folder" : selectedList?.icon ?? "notebook"} size={34} />
            {/if}
          </span>
          <h1>{isSearching ? `搜索结果：${searchQuery}` : selectedList?.name ?? "随手记"}</h1>
        </div>
        <div class="header-actions" on:click|stopPropagation>
          <button type="button" title="分类/条目菜单" on:click={openListMenu}><MoreHorizontal size={23} /></button>
          {#if showListMenu}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <section class="list-menu">
              <button type="button" disabled={selectedList?.kind === "system"} on:click={() => selectedList && startRename(selectedList.id)}>
                <Pencil size={15} /> 重命名
              </button>
              <button type="button" on:click={exportCurrentList}><Upload size={15} /> 导出当前</button>
              <button type="button" on:click={exportAll}><Upload size={15} /> 一键全部导出</button>
              <button type="button" on:click={() => importInput.click()}><Download size={15} /> 导入 JSON</button>
              <div class="menu-section-title">背景颜色</div>
              <div class="color-grid">
                {#each themePresets as preset}
                  <button
                    type="button"
                    title={preset.name}
                    style={`--swatch: ${preset.background}; --accent-color: ${preset.accent}`}
                    on:click={() => applyTheme(preset.accent, preset.background)}
                  ></button>
                {/each}
              </div>
              <label class="background-link">
                背景图片链接
                <input value={backgroundLinkDraft} placeholder="https://..." on:input={updateBackgroundLink} />
              </label>
              <label class="opacity-row">
                图片透明度
                <input type="range" min="0" max="80" value={Math.round((selectedTheme.imageOpacity ?? 0.28) * 100)} on:input={updateBackgroundOpacity} />
              </label>
              <div class="menu-inline two">
                <button type="button" on:click={() => backgroundFileInput.click()}><Image size={15} /> 上传图片</button>
                <button type="button" on:click={() => updateBackgroundImage(undefined)}><Eraser size={15} /> 清除背景</button>
              </div>
            </section>
          {/if}
        </div>
      </section>

      <input bind:this={importInput} class="hidden-file" type="file" accept="application/json,.json" on:change={importFromFile} />
      <input bind:this={backgroundFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadBackgroundImage} />

      <section class="task-list" class:compact={settings.taskDensity === "compact"}>
        <section class="completed-section">
          <button class="completed-toggle" type="button" on:click|stopPropagation={() => (showCompleted = !showCompleted)}>
            {#if showCompleted}
              <ChevronDown size={17} />
            {:else}
              <span class="chevron-placeholder">›</span>
            {/if}
            已完成 {completedCount}
          </button>
          {#if showCompleted}
            {#each completedTasks as task (task.id)}
              <TaskCard
                {task}
                selected={selectedTaskId === task.id}
                accent={selectedTheme.accent ?? defaultAccent}
                density={settings.taskDensity}
                onSelect={selectTask}
                onUpdate={updateTask}
              />
            {/each}
          {/if}
        </section>

        {#each incompleteTasks as task (task.id)}
          <TaskCard
            {task}
            selected={selectedTaskId === task.id}
            accent={selectedTheme.accent ?? defaultAccent}
            density={settings.taskDensity}
            onSelect={selectTask}
            onUpdate={updateTask}
          />
        {/each}

        {#if visibleTasks.length === 0}
          <div class="empty-state">
            <strong>{isSearching ? "没有搜索结果" : "这个条目还没有内容"}</strong>
            <span>在下方输入 Markdown，按 Enter 添加；Shift + Enter 换行。</span>
          </div>
        {/if}
      </section>

      <section class="add-task-bar" on:click|stopPropagation>
        <Plus size={24} />
        <div class="composer-main">
          <textarea
            bind:this={taskInput}
            bind:value={newTaskDraft}
            placeholder="添加 Markdown 内容"
            spellcheck="false"
            rows="1"
            on:input={resizeComposer}
            on:keydown={handleComposerKeydown}
          ></textarea>
          {#if newTaskDraft.trim() && newTaskDraft.includes("\n")}
            <div class="markdown-body composer-preview">
              {@html renderMarkdown(newTaskDraft)}
            </div>
          {/if}
        </div>
      </section>
    </main>

    {#if showSettings}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <aside class="settings-drawer" on:click|stopPropagation>
        <div class="drawer-header">
          <h2>设置</h2>
          <button type="button" on:click={() => (showSettings = false)}>×</button>
        </div>

        <section>
          <h3>个人资料</h3>
          <div class="avatar-setting">
            <span class="avatar large" style={avatarStyle()}>{avatarText()}</span>
            <button type="button" on:click={() => avatarFileInput.click()}>上传头像</button>
          </div>
          <input bind:this={avatarFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadAvatar} />
          <label class="settings-row">头像文字 <input value={settings.profile.avatar.startsWith("data:image") ? "" : settings.profile.avatar} maxlength="2" on:input={(event) => event.currentTarget instanceof HTMLInputElement && updateProfile("avatar", event.currentTarget.value)} /></label>
          <label class="settings-row">名称 <input value={settings.profile.name} on:input={(event) => event.currentTarget instanceof HTMLInputElement && updateProfile("name", event.currentTarget.value)} /></label>
          <label class="settings-row">邮箱 <input value={settings.profile.email} on:input={(event) => event.currentTarget instanceof HTMLInputElement && updateProfile("email", event.currentTarget.value)} /></label>
        </section>

        <section>
          <h3>全局唤起</h3>
          <label class="settings-row">Toggle 快捷键 <input value={settings.globalShortcut} on:change={updateGlobalShortcut} /></label>
          <p class="muted">默认 Ctrl + Shift + Space。注册成功后可在系统内全局显示/隐藏窗口。</p>
        </section>

        <section>
          <h3>快捷键</h3>
          {#each settings.shortcuts as shortcut (shortcut.id)}
            <label class="shortcut-row">
              <span>{shortcut.label}</span>
              <input value={shortcut.combo} on:change={(event) => updateShortcut(shortcut, event)} />
              <small>{shortcutLabel(shortcut.combo)}</small>
            </label>
          {/each}
        </section>

        <section>
          <h3>显示</h3>
          <div class="segmented">
            <button class:active={settings.taskDensity === "comfortable"} type="button" on:click={() => setDensity("comfortable")}>舒适</button>
            <button class:active={settings.taskDensity === "compact"} type="button" on:click={() => setDensity("compact")}>紧凑</button>
          </div>
        </section>

        <section>
          <h3>云同步预留</h3>
          <p class="muted">本次不启用云同步，但数据模型和设置入口已预留。</p>
          <div class="sync-card">
            <strong>当前状态：未配置</strong>
            {#each syncRoadmap as item}
              <span>• {item}</span>
            {/each}
          </div>
        </section>
      </aside>
    {/if}
  </div>

  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
</div>

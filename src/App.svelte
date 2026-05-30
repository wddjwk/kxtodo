<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    CalendarDays,
    Check,
    ChevronDown,
    Download,
    Eraser,
    FilePlus2,
    FolderInput,
    FolderPlus,
    Image,
    Minus,
    MoreHorizontal,
    Pencil,
    Plus,
    Search,
    Square,
    Star,
    Sun,
    Trash2,
    Upload,
    X
  } from "@lucide/svelte";
  import {
    exportData,
    loadSettings,
    loadState,
    openExternalUrl,
    registerGlobalShortcut,
    saveSettings,
    saveState,
    setAutostart,
    setCloseToTray,
    setWebviewZoom,
    isTauriRuntime
  } from "./lib/backend";
  import {
    createCategoryNode,
    createEntryNode,
    defaultBackground,
    defaultSettings,
    emptyState,
    normalizeSettings,
    normalizeState,
    themePresets
  } from "./lib/defaults";
  import IconGlyph from "./lib/IconGlyph.svelte";
  import IconPicker from "./lib/IconPicker.svelte";
  import ListTree from "./lib/ListTree.svelte";
  import TaskCard from "./lib/TaskCard.svelte";
  import { renderMarkdown } from "./lib/markdown";
  import { matchesShortcut } from "./lib/shortcuts";
  import type { AppNode, AppState, ListBackground, Settings, Task } from "./lib/types";

  const appVersion = "5.0.0";
  const defaultAccent = "#2564cf";

  let state: AppState = emptyState();
  let settings: Settings = clone(defaultSettings);
  let hydrated = false;
  let searchQuery = "";
  let newTaskDraft = "";
  let selectedTaskId: string | null = null;
  let showSettings = false;
  let showListMenu = false;
  let showCompleted = true;
  let renamingId: string | null = null;
  let renameDraft = "";
  let iconPickerListId: string | null = null;
  let treeMenu: { id: string; x: number; y: number } | null = null;
  let taskMenu: { taskId: string; x: number; y: number; showDate: boolean } | null = null;
  let draggingId: string | null = null;
  let backgroundLinkDraft = "";
  let backgroundDraftNodeId = "";
  let sidebarWidth = 320;
  let toast = "";
  let stateSaveTimer: number | undefined;
  let settingsSaveTimer: number | undefined;
  let toastTimer: number | undefined;
  let ignoreOverlayCloseOnce = false;
  let searchInput: HTMLInputElement;
  let taskInput: HTMLTextAreaElement;
  let importInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;
  let avatarFileInput: HTMLInputElement;
  let settingsDrawerStyle = "";

  $: systemNodes = state.nodes.filter((node) => node.kind === "system");
  $: firstEntry = state.nodes.find((node) => node.kind === "entry");
  $: selectedNode = state.nodes.find((node) => node.id === state.selectedNodeId) ?? firstEntry ?? systemNodes[0];
  $: selectedBackground = getBackground(selectedNode?.id, state.backgrounds);
  $: accent = accentForNode(selectedNode);
  $: mainStyle = buildMainStyle(selectedBackground, accent);
  $: appShellStyle = buildAppShellStyle(settings.appearance);
  $: windowControlsStyle = buildWindowControlsStyle(settings.appearance);
  $: settingsDrawerStyle = buildSettingsDrawerStyle(settings.appearance);
  $: listCounts = buildListCounts(state);
  $: visibleTasks = buildVisibleTasks(state, selectedNode, searchQuery);
  $: incompleteTasks = visibleTasks.filter((task) => !task.completed);
  $: completedTasks = visibleTasks.filter((task) => task.completed);
  $: isSearching = searchQuery.trim().length > 0;
  $: selectedIconPickerList = iconPickerListId ? state.nodes.find((node) => node.id === iconPickerListId) : null;
  $: treeMenuNode = treeMenu ? state.nodes.find((node) => node.id === treeMenu?.id) : null;
  $: treeMoveTargets = treeMenuNode ? moveTargetOptions(treeMenuNode.id) : [];
  $: selectedMoveTargets = selectedNode ? moveTargetOptions(selectedNode.id) : [];
  $: treeMenuStyle = treeMenu ? buildMenuStyle(treeMenu.x, treeMenu.y, 248, treeMenuNode?.kind === "category" ? 300 : 252) : "";
  $: taskMenuStyle = taskMenu ? buildMenuStyle(taskMenu.x, taskMenu.y, 230, taskMenu.showDate ? 230 : 188) : "";
  $: taskMenuTask = taskMenu ? state.tasks.find((task) => task.id === taskMenu?.taskId) : null;
  $: avatarStyle = settings.profile.avatar ? `background-image: url("${escapeCssUrl(settings.profile.avatar)}");` : "";
  $: avatarInitial = (settings.profile.displayName.trim().charAt(0) || "E").toUpperCase();
  $: if ((selectedNode?.id ?? "") !== backgroundDraftNodeId) {
    backgroundDraftNodeId = selectedNode?.id ?? "";
    backgroundLinkDraft = selectedBackground.image ?? "";
  }

  onMount(() => {
    void hydrate();
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  });

  function clone<T>(value: T): T {
    return JSON.parse(JSON.stringify(value)) as T;
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

  function createTaskId(): string {
    if (crypto.randomUUID) {
      return `task-${crypto.randomUUID().slice(0, 8)}`;
    }
    return `task-${Math.random().toString(36).slice(2, 10)}`;
  }

  async function hydrate(): Promise<void> {
    let loadedSettings = settings;
    try {
      const [storedState, storedSettings] = await Promise.all([loadState(), loadSettings()]);
      state = normalizeState(storedState);
      settings = normalizeSettings(storedSettings);
      loadedSettings = settings;
    } catch (error) {
      showToast(`加载本地数据失败，已使用默认数据：${String(error)}`);
    } finally {
      hydrated = true;
      await tick();
      resizeComposer();
    }

    await syncNativeAppearance(loadedSettings);

    try {
      await registerGlobalShortcut(loadedSettings.shortcuts.toggleWindow);
    } catch (error) {
      showToast(`全局快捷键注册失败：${String(error)}`);
    }

    await syncNativeLifecycle(loadedSettings);
  }

  function queueStateSave(): void {
    if (!hydrated) {
      return;
    }
    window.clearTimeout(stateSaveTimer);
    stateSaveTimer = window.setTimeout(() => {
      saveState(state).catch((error) => showToast(`保存失败：${String(error)}`));
    }, 180);
  }

  function queueSettingsSave(): void {
    if (!hydrated) {
      return;
    }
    window.clearTimeout(settingsSaveTimer);
    settingsSaveTimer = window.setTimeout(() => {
      saveSettings(settings).catch((error) => showToast(`保存设置失败：${String(error)}`));
    }, 180);
  }

  function commit(next: AppState): void {
    state = next;
    queueStateSave();
  }

  function commitSettings(next: Settings): void {
    const previousScale = uiScaleValue(settings.appearance.uiScale);
    const nextScale = uiScaleValue(next.appearance.uiScale);
    settings = next;
    queueSettingsSave();
    if (hydrated && previousScale !== nextScale) {
      void syncNativeAppearance(next);
    }
  }

  async function syncNativeLifecycle(nextSettings: Settings): Promise<void> {
    try {
      await setCloseToTray(nextSettings.lifecycle.closeToTray);
      await setAutostart(nextSettings.lifecycle.launchAtStartup);
    } catch (error) {
      showToast(`系统设置同步失败：${String(error)}`);
    }
  }

  async function syncNativeAppearance(nextSettings: Settings): Promise<void> {
    if (!isTauriRuntime) {
      return;
    }
    try {
      await setWebviewZoom(uiScaleValue(nextSettings.appearance.uiScale));
    } catch (error) {
      showToast(`界面缩放同步失败：${String(error)}`);
    }
  }

  function showToast(message: string): void {
    toast = message;
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => {
      toast = "";
    }, 3200);
  }

  function closeOverlays(): void {
    if (ignoreOverlayCloseOnce) {
      ignoreOverlayCloseOnce = false;
      return;
    }
    showListMenu = false;
    treeMenu = null;
    taskMenu = null;
    iconPickerListId = null;
    showSettings = false;
  }

  function openIconPicker(id: string): void {
    iconPickerListId = id;
    treeMenu = null;
    taskMenu = null;
    showListMenu = false;
    showSettings = false;
    ignoreOverlayCloseOnce = true;
    window.setTimeout(() => {
      ignoreOverlayCloseOnce = false;
    }, 250);
  }

  function getBackground(nodeId?: string, backgrounds: Record<string, ListBackground> = state.backgrounds): ListBackground {
    return nodeId ? (backgrounds[nodeId] ?? defaultBackground) : defaultBackground;
  }

  function accentForNode(node?: AppNode): string {
    if (!node) {
      return defaultAccent;
    }
    if (node.id === "planned") {
      return "#2564cf";
    }
    if (node.id === "important") {
      return "#9f5f00";
    }
    if (node.id === "my-day") {
      return "#b64a30";
    }
    return defaultAccent;
  }

  function escapeCssUrl(value: string): string {
    return value.replace(/\\/g, "\\\\").replace(/"/g, "%22").replace(/\n/g, "");
  }

  function buildMainStyle(background: ListBackground, color: string): string {
    const image = background.image ? `url("${escapeCssUrl(background.image)}")` : "none";
    const opacity = background.image ? background.imageOpacity ?? defaultBackground.imageOpacity ?? 0.28 : 0;
    return `--accent: ${color}; --bg-image: ${image}; --bg-opacity: ${opacity}; background: ${background.color};`;
  }

  function fontSizeValue(value: number, fallback: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, Math.round(value || fallback)));
  }

  function buildAppShellStyle(appearance: Settings["appearance"]): string {
    const scale = uiScaleValue(appearance.uiScale);
    const uiFontSize = fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22);
    const markdownFontSize = fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26);
    const editorFontSize = fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26);
    return [
      `--ui-scale: ${scale}`,
      `--ui-font-size: ${uiFontSize}px`,
      `--markdown-font-size: ${markdownFontSize}px`,
      `--editor-font-size: ${editorFontSize}px`,
      `--app-width: ${100 / scale}vw`,
      `--app-height: ${100 / scale}vh`,
      `--font-title: ${uiFontSize + 18}px`,
      `--font-list: ${uiFontSize + 1}px`,
      `--font-control: ${uiFontSize}px`,
      `--font-task: ${markdownFontSize}px`,
      `--font-composer: ${markdownFontSize}px`,
      `--font-drawer-title: ${uiFontSize + 6}px`
    ].join("; ");
  }

  function buildWindowControlsStyle(appearance: Settings["appearance"]): string {
    return "";
  }

  function buildSettingsDrawerStyle(appearance: Settings["appearance"]): string {
    const scale = uiScaleValue(appearance.uiScale);
    const viewportWidth = typeof window === "undefined" ? 1280 : window.innerWidth;
    const viewportHeight = typeof window === "undefined" ? 820 : window.innerHeight;
    const drawerWidth = 380;
    const titlebarHeight = 52;
    return [
      `left: ${(viewportWidth - drawerWidth) / scale}px`,
      `top: ${titlebarHeight / scale}px`,
      `width: ${drawerWidth / scale}px`,
      `height: ${(viewportHeight - titlebarHeight) / scale}px`,
      `--ui-scale: ${scale}`,
      `--ui-font-size: ${fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22)}px`,
      `--markdown-font-size: ${fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26)}px`,
      `--editor-font-size: ${fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26)}px`
    ].join("; ");
  }

  function uiScaleValue(scaleValue = settings.appearance.uiScale): number {
    const staleScale = scaleValue === 0.62 || scaleValue === 0.72 || scaleValue === 0.86 || scaleValue === 0.92;
    const normalizedScale = staleScale ? defaultSettings.appearance.uiScale : scaleValue;
    return Math.min(1.5, Math.max(0.5, normalizedScale || defaultSettings.appearance.uiScale));
  }

  function scalePercentValue(scaleValue = settings.appearance.uiScale): number {
    return Math.round(uiScaleValue(scaleValue) * 100);
  }

  function clampNumber(value: number, fallback: number, min: number, max: number): number {
    if (!Number.isFinite(value)) {
      return fallback;
    }
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function isNumberInRange(value: number, min: number, max: number): boolean {
    return Number.isFinite(value) && value >= min && value <= max;
  }

  function buildMenuStyle(clientX: number, clientY: number, width: number, height: number): string {
    const scale = uiScaleValue();
    const viewportWidth = typeof window === "undefined" ? 1200 : window.innerWidth / scale;
    const viewportHeight = typeof window === "undefined" ? 800 : window.innerHeight / scale;
    const left = Math.max(8, Math.min(clientX / scale, viewportWidth - width - 10));
    const top = Math.max(8, Math.min(clientY / scale, viewportHeight - height - 10));
    return `left: ${left}px; top: ${top}px;`;
  }

  function buildVisibleTasks(source: AppState, node: AppNode | undefined, queryValue: string): Task[] {
    const query = queryValue.trim().toLowerCase();
    if (query) {
      return source.tasks.filter((task) => {
        const taskNode = source.nodes.find((item) => item.id === task.nodeId);
        return task.markdown.toLowerCase().includes(query) || taskNode?.name.toLowerCase().includes(query);
      });
    }
    if (!node) {
      return [];
    }
    return tasksForNode(node, source.tasks, source.nodes);
  }

  function tasksForNode(node: AppNode, tasks: Task[], nodes: AppNode[] = state.nodes): Task[] {
    if (node.id === "my-day") {
      return tasks.filter((task) => task.myDay);
    }
    if (node.id === "planned") {
      return tasks.filter((task) => Boolean(task.dueDate || task.plannedDate));
    }
    if (node.id === "important") {
      return tasks.filter((task) => task.important);
    }
    if (node.kind === "entry") {
      return tasks.filter((task) => task.nodeId === node.id);
    }
    if (node.kind === "category") {
      const ids = descendantEntryIds(node.id, nodes);
      return tasks.filter((task) => ids.has(task.nodeId));
    }
    return [];
  }

  function descendantEntryIds(rootId: string, nodes: AppNode[] = state.nodes): Set<string> {
    const ids = new Set<string>();
    const visit = (parentId: string): void => {
      for (const node of nodes.filter((item) => item.parentId === parentId)) {
        if (node.kind === "entry") {
          ids.add(node.id);
        }
        if (node.kind === "category") {
          visit(node.id);
        }
      }
    };
    const root = nodes.find((node) => node.id === rootId);
    if (root?.kind === "entry") {
      ids.add(root.id);
    } else {
      visit(rootId);
    }
    return ids;
  }

  function nodeAndDescendantIds(rootId: string): Set<string> {
    const ids = new Set<string>([rootId]);
    const visit = (parentId: string): void => {
      for (const node of state.nodes.filter((item) => item.parentId === parentId)) {
        ids.add(node.id);
        if (node.kind === "category") {
          visit(node.id);
        }
      }
    };
    visit(rootId);
    return ids;
  }

  function ancestorIds(nodeId: string): Set<string> {
    const ids = new Set<string>();
    let current = state.nodes.find((node) => node.id === nodeId);
    while (current?.parentId) {
      ids.add(current.parentId);
      current = state.nodes.find((node) => node.id === current?.parentId);
    }
    return ids;
  }

  function buildListCounts(source: AppState): Record<string, number> {
    const counts: Record<string, number> = {};
    for (const node of source.nodes) {
      if (node.id === "my-day") {
        counts[node.id] = source.tasks.filter((task) => !task.completed && task.myDay).length;
      } else if (node.id === "planned") {
        counts[node.id] = source.tasks.filter((task) => !task.completed && (task.dueDate || task.plannedDate)).length;
      } else if (node.id === "important") {
        counts[node.id] = source.tasks.filter((task) => !task.completed && task.important).length;
      } else if (node.kind === "entry") {
        counts[node.id] = source.tasks.filter((task) => !task.completed && task.nodeId === node.id).length;
      } else if (node.kind === "category") {
        const ids = descendantEntryIds(node.id, source.nodes);
        counts[node.id] = source.tasks.filter((task) => !task.completed && ids.has(task.nodeId)).length;
      }
    }
    return counts;
  }

  function selectNode(id: string): void {
    searchQuery = "";
    commit({ ...state, selectedNodeId: id });
    treeMenu = null;
    taskMenu = null;
    iconPickerListId = null;
  }

  function toggleCategory(id: string): void {
    commit({
      ...state,
      nodes: state.nodes.map((node) => (node.id === id ? { ...node, collapsed: !node.collapsed } : node))
    });
    treeMenu = null;
    iconPickerListId = null;
  }

  function toggleListMenu(): void {
    showListMenu = !showListMenu;
    taskMenu = null;
  }

  function handleListMenuKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" && event.key !== " ") {
      return;
    }
    event.preventDefault();
    toggleListMenu();
  }

  function currentCategoryId(): string | null {
    if (selectedNode?.kind === "entry") {
      return selectedNode.parentId;
    }
    if (selectedNode?.kind === "category") {
      return selectedNode.id;
    }
    return null;
  }

  function addNode(parentId: string | null, kind: "category" | "entry"): void {
    const node = kind === "category" ? createCategoryNode("未命名分类", parentId) : createEntryNode("未命名条目", parentId);
    const backgrounds = kind === "entry" ? { ...state.backgrounds, [node.id]: { ...defaultBackground } } : state.backgrounds;
    const nodes = state.nodes.map((item) => (item.id === parentId ? { ...item, collapsed: false } : item));
    commit({
      ...state,
      nodes: [...nodes, node],
      selectedNodeId: kind === "entry" ? node.id : state.selectedNodeId,
      backgrounds
    });
    renamingId = node.id;
    renameDraft = node.name;
    treeMenu = null;
  }

  function startRename(id: string): void {
    const node = state.nodes.find((item) => item.id === id);
    if (!node || node.kind === "system") {
      return;
    }
    renamingId = id;
    renameDraft = node.name;
    treeMenu = null;
    showListMenu = false;
  }

  function commitRename(id: string): void {
    if (renamingId !== id) {
      return;
    }
    const name = renameDraft.trim();
    if (!name) {
      showToast("名称不能为空");
      return;
    }
    commit({
      ...state,
      nodes: state.nodes.map((node) => (node.id === id ? { ...node, name } : node))
    });
    renamingId = null;
    renameDraft = "";
  }

  function deleteNode(id: string): void {
    const node = state.nodes.find((item) => item.id === id);
    if (!node || node.kind === "system") {
      showToast("内置列表不能删除");
      return;
    }
    const ids = nodeAndDescendantIds(id);
    let nodes = state.nodes.filter((item) => !ids.has(item.id));
    let backgrounds = Object.fromEntries(Object.entries(state.backgrounds).filter(([key]) => !ids.has(key)));
    if (!nodes.some((item) => item.kind === "entry")) {
      const inbox = createEntryNode("收集箱", null, "inbox");
      nodes = [...nodes, inbox];
      backgrounds = { ...backgrounds, [inbox.id]: { ...defaultBackground } };
    }
    const validNodeIds = new Set(nodes.map((item) => item.id));
    const fallbackId = validNodeIds.has(state.selectedNodeId) ? state.selectedNodeId : nodes.find((item) => item.kind === "entry")?.id ?? "my-day";
    commit({
      ...state,
      nodes,
      tasks: state.tasks.filter((task) => validNodeIds.has(task.nodeId)),
      selectedNodeId: fallbackId,
      backgrounds
    });
    treeMenu = null;
  }

  function moveNode(id: string, targetId: string, position: "before" | "after" | "inside"): void {
    const source = state.nodes.find((node) => node.id === id);
    const target = state.nodes.find((node) => node.id === targetId);
    if (!source || !target || source.kind === "system" || target.kind === "system") {
      return;
    }
    if (source.id === target.id || nodeAndDescendantIds(source.id).has(target.id)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }
    if (position === "inside" && target.kind !== "category") {
      return;
    }
    const nextParentId = position === "inside" ? target.id : target.parentId;
    const sourceWithParent = { ...source, parentId: nextParentId };
    const withoutSource = state.nodes.filter((node) => node.id !== id);
    const targetIndex = withoutSource.findIndex((node) => node.id === target.id);
    let insertIndex = withoutSource.length;
    if (position === "before") {
      insertIndex = targetIndex >= 0 ? targetIndex : withoutSource.length;
    } else if (position === "after") {
      insertIndex = targetIndex >= 0 ? targetIndex + 1 : withoutSource.length;
    } else {
      const childIndexes = withoutSource
        .map((node, index) => ({ node, index }))
        .filter((item) => item.node.parentId === target.id)
        .map((item) => item.index);
      insertIndex = childIndexes.length ? Math.max(...childIndexes) + 1 : targetIndex >= 0 ? targetIndex + 1 : withoutSource.length;
    }
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, sourceWithParent);
    commit({
      ...state,
      nodes: nodes.map((node) => (position === "inside" && node.id === target.id ? { ...node, collapsed: false } : node))
    });
    draggingId = null;
  }

  function moveNodeToGroup(id: string, parentId: string | null): void {
    const source = state.nodes.find((node) => node.id === id);
    const nextParentId = parentId || null;
    if (!source || source.kind === "system") {
      return;
    }
    if (source.parentId === nextParentId) {
      treeMenu = null;
      showListMenu = false;
      return;
    }
    const targetParent = nextParentId ? state.nodes.find((node) => node.id === nextParentId && node.kind === "category") : null;
    if (nextParentId && !targetParent) {
      showToast("目标分组不存在");
      return;
    }
    if (source.kind === "category" && nextParentId && nodeAndDescendantIds(source.id).has(nextParentId)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }

    const withoutSource = state.nodes.filter((node) => node.id !== id);
    const sourceWithParent = { ...source, parentId: nextParentId };
    let insertIndex = withoutSource.length;
    if (nextParentId) {
      const siblingIndexes = withoutSource
        .map((node, index) => ({ node, index }))
        .filter((item) => item.node.parentId === nextParentId)
        .map((item) => item.index);
      const parentIndex = withoutSource.findIndex((node) => node.id === nextParentId);
      insertIndex = siblingIndexes.length ? Math.max(...siblingIndexes) + 1 : parentIndex >= 0 ? parentIndex + 1 : withoutSource.length;
    }
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, sourceWithParent);
    commit({
      ...state,
      nodes: nodes.map((node) => (nextParentId && node.id === nextParentId ? { ...node, collapsed: false } : node))
    });
    treeMenu = null;
    showListMenu = false;
    draggingId = null;
  }

  function moveTargetOptions(sourceId: string): Array<{ id: string; name: string }> {
    const source = state.nodes.find((node) => node.id === sourceId);
    if (!source || source.kind === "system") {
      return [];
    }
    const excluded = source.kind === "category" ? nodeAndDescendantIds(source.id) : new Set<string>([source.id]);
    return [
      { id: "", name: "顶层" },
      ...state.nodes
        .filter((node) => node.kind === "category" && !excluded.has(node.id))
        .map((node) => ({
          id: node.id,
          name: `${"　".repeat(ancestorIds(node.id).size)}${node.name}`
        }))
    ];
  }

  function handleMoveTargetChange(nodeId: string, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) {
      return;
    }
    moveNodeToGroup(nodeId, target.value || null);
  }

  function pickIcon(icon: string): void {
    if (!selectedIconPickerList) {
      return;
    }
    commit({
      ...state,
      nodes: state.nodes.map((node) => (node.id === selectedIconPickerList.id ? { ...node, icon } : node))
    });
    iconPickerListId = null;
  }

  function updateTask(taskId: string, updater: (task: Task) => Task): void {
    commit({
      ...state,
      tasks: state.tasks.map((task) => (task.id === taskId ? { ...updater(task), updatedAt: now() } : task))
    });
  }

  function addTaskFromDraft(): void {
    const markdown = newTaskDraft.trim();
    if (!markdown) {
      return;
    }
    const targetNode = selectedNode?.kind === "entry" ? selectedNode : firstEntry;
    if (!targetNode) {
      showToast("请先创建一个条目");
      return;
    }
    const timestamp = now();
    const task: Task = {
      id: createTaskId(),
      nodeId: targetNode.id,
      markdown,
      completed: false,
      important: selectedNode?.id === "important",
      myDay: selectedNode?.id === "my-day",
      plannedDate: selectedNode?.id === "planned" ? todayIso() : undefined,
      dueDate: selectedNode?.id === "planned" ? todayIso() : undefined,
      expanded: false,
      editing: false,
      createdAt: timestamp,
      updatedAt: timestamp
    };
    commit({ ...state, tasks: [...state.tasks, task] });
    newTaskDraft = "";
    void tick().then(resizeComposer);
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      addTaskFromDraft();
    }
  }

  function resizeComposer(): void {
    if (!taskInput) {
      return;
    }
    taskInput.style.height = "auto";
    taskInput.style.height = `${Math.min(taskInput.scrollHeight, 180)}px`;
  }

  function openTaskMenu(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    taskMenu = { taskId: event.detail.id, x: event.detail.x, y: event.detail.y, showDate: false };
    showListMenu = false;
  }

  function setTaskDate(taskId: string, date: string): void {
    updateTask(taskId, (task) => ({ ...task, dueDate: date || undefined, plannedDate: date || undefined }));
    taskMenu = null;
  }

  function setBackground(patch: Partial<ListBackground>): void {
    if (!selectedNode) {
      return;
    }
    commit({
      ...state,
      backgrounds: {
        ...state.backgrounds,
        [selectedNode.id]: {
          ...getBackground(selectedNode.id),
          ...patch
        }
      }
    });
  }

  function updateBackgroundLink(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    backgroundLinkDraft = target.value;
    setBackground({ image: target.value.trim() || undefined });
  }

  function updateBackgroundOpacity(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      setBackground({ imageOpacity: Number(target.value) / 100 });
    }
  }

  function applyTheme(color: string): void {
    setBackground({ color });
  }

  async function fileToDataUrl(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
      reader.readAsDataURL(file);
    });
  }

  async function uploadBackgroundImage(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }
    try {
      const dataUrl = await fileToDataUrl(target.files[0]);
      backgroundLinkDraft = dataUrl;
      setBackground({ image: dataUrl });
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    } finally {
      target.value = "";
    }
  }

  async function uploadAvatar(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }
    try {
      const avatar = await fileToDataUrl(target.files[0]);
      commitSettings({ ...settings, profile: { ...settings.profile, avatar } });
    } catch (error) {
      showToast(`头像读取失败：${String(error)}`);
    } finally {
      target.value = "";
    }
  }

  function exportStateForNode(node: AppNode): AppState {
    const tasks = tasksForNode(node, state.tasks);
    const nodeIds = new Set<string>();
    if (node.kind === "category") {
      for (const id of nodeAndDescendantIds(node.id)) {
        nodeIds.add(id);
      }
    } else if (node.kind === "entry") {
      nodeIds.add(node.id);
      for (const id of ancestorIds(node.id)) {
        nodeIds.add(id);
      }
    } else {
      nodeIds.add(node.id);
      for (const task of tasks) {
        nodeIds.add(task.nodeId);
        for (const id of ancestorIds(task.nodeId)) {
          nodeIds.add(id);
        }
      }
    }
    const exportedNodes = state.nodes.filter((item) => item.kind === "system" || nodeIds.has(item.id));
    return {
      schemaVersion: state.schemaVersion,
      nodes: exportedNodes,
      tasks,
      selectedNodeId: node.id,
      backgrounds: Object.fromEntries(exportedNodes.map((item) => [item.id, getBackground(item.id)]))
    };
  }

  function safeFileName(name: string): string {
    return name.replace(/[<>:"/\\|?*\u0000-\u001F]/g, "-").slice(0, 80) || "todo-note";
  }

  async function exportCurrentList(): Promise<void> {
    if (!selectedNode) {
      return;
    }
    const payload = {
      version: appVersion,
      exportedAt: now(),
      scope: "node",
      nodeId: selectedNode.id,
      state: exportStateForNode(selectedNode)
    };
    await exportData(payload, `${safeFileName(selectedNode.name)}-${appVersion}.json`);
    showListMenu = false;
    showToast("导出完成");
  }

  async function exportAll(): Promise<void> {
    const payload = {
      version: appVersion,
      exportedAt: now(),
      scope: "all",
      state,
      settings
    };
    await exportData(payload, `todo-note-${appVersion}-all.json`);
    showListMenu = false;
    showToast("全部数据已导出");
  }

  async function importFromFile(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) {
      return;
    }
    try {
      const payload = JSON.parse(await target.files[0].text()) as { state?: unknown; settings?: unknown };
      commit(normalizeState(payload.state ?? payload));
      if (payload.settings) {
        commitSettings(normalizeSettings(payload.settings));
      }
      showToast("导入完成");
    } catch (error) {
      showToast(`导入失败：${String(error)}`);
    } finally {
      target.value = "";
      showListMenu = false;
    }
  }

  function updateProfile(field: keyof Settings["profile"], value: string): void {
    commitSettings({
      ...settings,
      profile: {
        ...settings.profile,
        [field]: value
      }
    });
  }

  function updateAppearance<K extends keyof Settings["appearance"]>(field: K, value: Settings["appearance"][K]): void {
    commitSettings({
      ...settings,
      appearance: {
        ...settings.appearance,
        [field]: value
      }
    });
  }

  function updateScalePercent(value: number): void {
    if (isNumberInRange(value, 50, 150)) {
      updateAppearance("uiScale", Math.round(value) / 100);
    }
  }

  function commitScalePercent(value: number): void {
    const nextPercent = clampNumber(value, scalePercentValue(), 50, 150);
    updateAppearance("uiScale", nextPercent / 100);
  }

  function updateAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize", value: number): void {
    const max = field === "uiFontSize" ? 22 : 26;
    if (isNumberInRange(value, 14, max)) {
      updateAppearance(field, Math.round(value));
    }
  }

  function commitAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize", value: number): void {
    const fallback = defaultSettings.appearance[field];
    const nextSize = clampNumber(value, fallback, 14, field === "uiFontSize" ? 22 : 26);
    updateAppearance(field, nextSize);
  }

  async function updateLifecycle<K extends keyof Settings["lifecycle"]>(field: K, value: Settings["lifecycle"][K]): Promise<void> {
    try {
      if (field === "closeToTray") {
        await setCloseToTray(Boolean(value));
      }
      if (field === "launchAtStartup") {
        await setAutostart(Boolean(value));
      }
      commitSettings({
        ...settings,
        lifecycle: {
          ...settings.lifecycle,
          [field]: value
        }
      });
    } catch (error) {
      showToast(`系统设置更新失败：${String(error)}`);
    }
  }

  function updateShortcut(field: keyof Settings["shortcuts"], value: string): void {
    const next = {
      ...settings,
      shortcuts: {
        ...settings.shortcuts,
        [field]: value
      }
    };
    commitSettings(next);
    if (field === "toggleWindow") {
      registerGlobalShortcut(value).catch((error) => showToast(`全局快捷键注册失败：${String(error)}`));
    }
  }

  function updateCloudEndpoint(value: string): void {
    commitSettings({ ...settings, cloud: { ...settings.cloud, endpoint: value } });
  }

  function updateCloudProvider(value: Settings["cloud"]["provider"]): void {
    commitSettings({ ...settings, cloud: { ...settings.cloud, provider: value } });
  }

  function handleShortcut(event: KeyboardEvent): void {
    if (matchesShortcut(event, settings.shortcuts.focusSearch)) {
      event.preventDefault();
      searchInput?.focus();
    } else if (matchesShortcut(event, settings.shortcuts.newTask)) {
      event.preventDefault();
      taskInput?.focus();
    } else if (matchesShortcut(event, settings.shortcuts.openSettings)) {
      event.preventDefault();
      showSettings = !showSettings;
      showListMenu = false;
    }
  }

  function handleTreePointerDownCapture(event: PointerEvent): void {
    openIconPickerFromTreeEvent(event);
  }

  function handleTreeClickCapture(event: MouseEvent): void {
    openIconPickerFromTreeEvent(event);
  }

  function openIconPickerFromTreeEvent(event: MouseEvent | PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Element)) {
      return;
    }
    if (!target.closest(".tree-icon")) {
      return;
    }
    const row = target.closest<HTMLElement>(".tree-row[data-node-id]");
    if (!row) {
      return;
    }
    const id = row.dataset.nodeId;
    const node = state.nodes.find((item) => item.id === id);
    if (!node || node.kind === "system") {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    openIconPicker(node.id);
  }

  async function openTaskLink(event: CustomEvent<string>): Promise<void> {
    try {
      await openExternalUrl(event.detail);
    } catch (error) {
      showToast(`打开链接失败：${String(error)}`);
    }
  }

  function startSidebarResize(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const scale = uiScaleValue();
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

  async function minimizeWindow(): Promise<void> {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      showToast(`最小化失败：${String(error)}`);
    }
  }

  async function toggleMaximizeWindow(): Promise<void> {
    try {
      await getCurrentWindow().toggleMaximize();
    } catch (error) {
      showToast(`切换最大化失败：${String(error)}`);
    }
  }

  async function closeWindow(): Promise<void> {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      showToast(`关闭窗口失败：${String(error)}`);
    }
  }
</script>

<div class="app-shell" style={appShellStyle} on:click={closeOverlays}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="window-title" data-tauri-drag-region>
      <span class="app-glyph"><Check size={15} /></span>
      <span>Todo Note</span>
    </div>
  </header>

  <div class="layout">
    <aside class="sidebar" style={`width: ${sidebarWidth}px; min-width: ${sidebarWidth}px;`} on:click|stopPropagation>
      <button class="profile-card" type="button" on:click|stopPropagation={() => { showSettings = !showSettings; showListMenu = false; }}>
        <span class="avatar" style={avatarStyle}>{settings.profile.avatar ? "" : avatarInitial}</span>
        <span class="profile-text">
          <strong>{settings.profile.displayName}</strong>
          <span>{settings.profile.email}</span>
        </span>
      </button>

      <label class="search-box">
        <Search size={19} />
        <input bind:this={searchInput} bind:value={searchQuery} placeholder="搜索" />
      </label>

      <nav class="system-nav">
        {#each systemNodes as node (node.id)}
          <button class:selected={state.selectedNodeId === node.id && !isSearching} class="nav-row" type="button" on:click={() => selectNode(node.id)}>
            <span class="active-rail"></span>
            <span class="system-icon"><IconGlyph icon={node.icon} size={19} /></span>
            <span class="list-name">{node.name}</span>
            {#if listCounts[node.id]}
              <span class="count-pill">{listCounts[node.id]}</span>
            {/if}
          </button>
        {/each}
      </nav>

      <div class="nav-divider"></div>

      <nav class="custom-nav" on:pointerdown|capture={handleTreePointerDownCapture} on:click|capture={handleTreeClickCapture} on:click|stopPropagation>
        <ListTree
          nodes={state.nodes}
          selectedNodeId={state.selectedNodeId}
          counts={listCounts}
          renamingId={renamingId}
          {renameDraft}
          on:selectEntry={(event) => selectNode(event.detail)}
          on:toggleCategory={(event) => toggleCategory(event.detail)}
          on:renameInput={(event) => (renameDraft = event.detail)}
          on:renameCommit={(event) => commitRename(event.detail)}
          on:openMenu={(event) => { treeMenu = event.detail; taskMenu = null; showListMenu = false; }}
          requestIconPicker={openIconPicker}
          on:pickIcon={(event) => openIconPicker(event.detail)}
          on:dragStart={(event) => (draggingId = event.detail || null)}
          on:dropNode={(event) => moveNode(event.detail.id, event.detail.targetId, event.detail.position)}
          {draggingId}
        />
      </nav>

      {#if treeMenu && treeMenuNode}
        <section
          class="tree-context-menu"
          style={treeMenuStyle}
          on:click|stopPropagation
        >
          {#if treeMenuNode.kind === "category"}
            <button type="button" on:click={() => addNode(treeMenuNode.id, "entry")}><FilePlus2 size={15} /> 创建条目</button>
            <button type="button" on:click={() => addNode(treeMenuNode.id, "category")}><FolderPlus size={15} /> 创建子分类</button>
          {/if}
          <button type="button" disabled={treeMenuNode.kind === "system"} on:click={() => startRename(treeMenuNode.id)}><Pencil size={15} /> 重命名</button>
          <button type="button" disabled={treeMenuNode.kind === "system"} on:click={() => openIconPicker(treeMenuNode.id)}><Star size={15} /> 选择图标</button>
          {#if treeMenuNode.kind !== "system"}
            <label class="move-group-row">
              <span><FolderInput size={15} /> 移动到分组</span>
              <select value={treeMenuNode.parentId ?? ""} on:change={(event) => handleMoveTargetChange(treeMenuNode.id, event)}>
                {#each treeMoveTargets as target}
                  <option value={target.id}>{target.name}</option>
                {/each}
              </select>
            </label>
          {/if}
          <button class="danger" type="button" disabled={treeMenuNode.kind === "system"} on:click={() => deleteNode(treeMenuNode.id)}><Trash2 size={15} /> 删除</button>
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
              <IconGlyph icon={selectedNode?.icon ?? "notebook"} size={34} />
            {/if}
          </span>
          <h1>{isSearching ? `搜索结果：${searchQuery}` : selectedNode?.name ?? "随手记"}</h1>
        </div>
        <div class="header-actions" on:click|stopPropagation>
          <button
            type="button"
            title="分类/条目菜单"
            on:mousedown|preventDefault|stopPropagation={toggleListMenu}
            on:click|stopPropagation
            on:keydown={handleListMenuKeydown}
          ><MoreHorizontal size={23} /></button>
          {#if showListMenu}
            <section class="list-menu">
              <button type="button" disabled={!selectedNode || selectedNode.kind === "system"} on:click={() => selectedNode && startRename(selectedNode.id)}>
                <Pencil size={15} /> 重命名
              </button>
              {#if selectedNode && selectedNode.kind !== "system"}
                <label class="move-group-row">
                  <span><FolderInput size={15} /> 移动到分组</span>
                  <select value={selectedNode.parentId ?? ""} on:change={(event) => handleMoveTargetChange(selectedNode.id, event)}>
                    {#each selectedMoveTargets as target}
                      <option value={target.id}>{target.name}</option>
                    {/each}
                  </select>
                </label>
              {/if}
              <button type="button" on:click={exportCurrentList}><Upload size={15} /> 导出当前</button>
              <button type="button" on:click={exportAll}><Upload size={15} /> 一键全部导出</button>
              <button type="button" on:click={() => importInput.click()}><Download size={15} /> 导入 JSON</button>
              <div class="menu-section-title">背景颜色</div>
              <div class="color-grid">
                {#each themePresets as preset}
                  <button
                    type="button"
                    title={preset.name}
                    style={`--swatch: ${preset.color}; --accent-color: ${preset.color}`}
                    on:click={() => applyTheme(preset.color)}
                  ></button>
                {/each}
              </div>
              <label class="background-link">
                背景图片链接
                <input value={backgroundLinkDraft} placeholder="https://..." on:input={updateBackgroundLink} />
              </label>
              <label class="opacity-row">
                图片透明度
                <input type="range" min="0" max="80" value={Math.round((selectedBackground.imageOpacity ?? 0.28) * 100)} on:input={updateBackgroundOpacity} />
              </label>
              <div class="menu-inline two">
                <button type="button" on:click={() => backgroundFileInput.click()}><Image size={15} /> 上传图片</button>
                <button type="button" on:click={() => { backgroundLinkDraft = ""; setBackground({ image: undefined }); }}><Eraser size={15} /> 清除背景</button>
              </div>
            </section>
          {/if}
        </div>

      </section>

      <input bind:this={importInput} class="hidden-file" type="file" accept="application/json,.json" on:change={importFromFile} />
      <input bind:this={backgroundFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadBackgroundImage} />

      <section class="task-list">
        {#each incompleteTasks as task (task.id)}
          <TaskCard
            {task}
            selected={taskMenu?.taskId === task.id || selectedTaskId === task.id}
            linkOpenMode={settings.appearance.linkOpenMode}
            on:toggle={(event) => updateTask(event.detail, (item) => ({ ...item, completed: !item.completed }))}
            on:expand={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, expanded: !item.expanded })); }}
            on:edit={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, editing: true, expanded: true })); }}
            on:commit={(event) => updateTask(event.detail.id, (item) => ({ ...item, markdown: event.detail.markdown, editing: false, expanded: true }))}
            on:context={openTaskMenu}
            on:openLink={openTaskLink}
          />
        {/each}

        {#if completedTasks.length}
          <section class="completed-section">
            <button class="completed-toggle" type="button" on:click|stopPropagation={() => (showCompleted = !showCompleted)}>
              <ChevronDown class={!showCompleted ? "collapsed" : ""} size={17} />
              已完成 {completedTasks.length}
            </button>
            {#if showCompleted}
              {#each completedTasks as task (task.id)}
                <TaskCard
                  {task}
                  selected={taskMenu?.taskId === task.id || selectedTaskId === task.id}
                  linkOpenMode={settings.appearance.linkOpenMode}
                  on:toggle={(event) => updateTask(event.detail, (item) => ({ ...item, completed: !item.completed }))}
                  on:expand={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, expanded: !item.expanded })); }}
                  on:edit={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, editing: true, expanded: true })); }}
                  on:commit={(event) => updateTask(event.detail.id, (item) => ({ ...item, markdown: event.detail.markdown, editing: false, expanded: true }))}
                  on:context={openTaskMenu}
                  on:openLink={openTaskLink}
                />
              {/each}
            {/if}
          </section>
        {/if}

        {#if visibleTasks.length === 0}
          <div class="empty-state">
            <strong>{isSearching ? "没有搜索结果" : "这个条目还没有内容"}</strong>
            <span>在下方输入 Markdown，按 Enter 添加；Shift + Enter 换行。</span>
          </div>
        {/if}
      </section>

      {#if taskMenu && taskMenuTask}
        <div
          class="task-context-menu"
          style={taskMenuStyle}
          on:click|stopPropagation
        >
          <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, myDay: !task.myDay })); taskMenu = null; }}>
            <Sun size={16} /> {taskMenuTask.myDay ? "从我的一天中移除" : "添加到我的一天"}
          </button>
          <button type="button" on:click={() => taskMenu && (taskMenu = { ...taskMenu, showDate: !taskMenu.showDate })}>
            <CalendarDays size={16} /> 添加日期
          </button>
          {#if taskMenu.showDate}
            <input type="date" value={taskMenuTask.dueDate ?? ""} on:change={(event) => setTaskDate(taskMenuTask.id, event.currentTarget.value)} />
          {/if}
          <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, important: !task.important })); taskMenu = null; }}>
            <Star size={16} /> {taskMenuTask.important ? "取消收藏" : "收藏"}
          </button>
          <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, editing: true, expanded: true })); taskMenu = null; }}>
            <Pencil size={16} /> 编辑
          </button>
        </div>
      {/if}

      <section class="add-task-bar" on:click|stopPropagation>
        <Plus size={24} />
        <div class="composer-main">
          <textarea
            bind:this={taskInput}
            bind:value={newTaskDraft}
            placeholder="添加事项"
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
      <aside class="settings-drawer" style={settingsDrawerStyle} on:click|stopPropagation>
        <div class="drawer-header">
          <h2>设置</h2>
          <button type="button" on:click={() => (showSettings = false)}>×</button>
        </div>

        <section>
          <h3>个人资料</h3>
          <div class="avatar-setting">
            <span class="avatar large" style={avatarStyle}>{settings.profile.avatar ? "" : avatarInitial}</span>
            <button type="button" on:click={() => avatarFileInput.click()}>上传头像</button>
            <input bind:this={avatarFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadAvatar} />
          </div>
          <label class="settings-row">
            名字
            <input value={settings.profile.displayName} on:input={(event) => updateProfile("displayName", event.currentTarget.value)} />
          </label>
          <label class="settings-row">
            邮箱
            <input value={settings.profile.email} on:input={(event) => updateProfile("email", event.currentTarget.value)} />
          </label>
        </section>

        <section>
          <h3>显示与链接</h3>
          <div class="settings-row number-row">
            <span>界面缩放</span>
            <span class="number-control">
              <input
                aria-label="界面缩放"
                type="number"
                min="50"
                max="150"
                step="1"
                value={scalePercentValue(settings.appearance.uiScale)}
                on:input={(event) => updateScalePercent(event.currentTarget.valueAsNumber)}
                on:change={(event) => commitScalePercent(event.currentTarget.valueAsNumber)}
              />
              <span>%</span>
            </span>
          </div>
          <div class="settings-row number-row">
            <span>UI 字号</span>
            <span class="number-control">
              <input
                aria-label="UI 字号"
                type="number"
                min="14"
                max="22"
                step="1"
                value={settings.appearance.uiFontSize}
                on:input={(event) => updateAppearanceFont("uiFontSize", event.currentTarget.valueAsNumber)}
                on:change={(event) => commitAppearanceFont("uiFontSize", event.currentTarget.valueAsNumber)}
              />
              <span>px</span>
            </span>
          </div>
          <div class="settings-row number-row">
            <span>Markdown 字号</span>
            <span class="number-control">
              <input
                aria-label="Markdown 字号"
                type="number"
                min="14"
                max="26"
                step="1"
                value={settings.appearance.markdownFontSize}
                on:input={(event) => updateAppearanceFont("markdownFontSize", event.currentTarget.valueAsNumber)}
                on:change={(event) => commitAppearanceFont("markdownFontSize", event.currentTarget.valueAsNumber)}
              />
              <span>px</span>
            </span>
          </div>
          <div class="settings-row number-row">
            <span>编辑器字号</span>
            <span class="number-control">
              <input
                aria-label="编辑器字号"
                type="number"
                min="14"
                max="26"
                step="1"
                value={settings.appearance.editorFontSize}
                on:input={(event) => updateAppearanceFont("editorFontSize", event.currentTarget.valueAsNumber)}
                on:change={(event) => commitAppearanceFont("editorFontSize", event.currentTarget.valueAsNumber)}
              />
              <span>px</span>
            </span>
          </div>
          <div class="settings-row">
            <span>链接打开</span>
            <select
              aria-label="链接打开"
              value={settings.appearance.linkOpenMode}
              on:change={(event) => updateAppearance("linkOpenMode", event.currentTarget.value as Settings["appearance"]["linkOpenMode"])}
            >
              <option value="app">应用内打开</option>
              <option value="system">系统浏览器</option>
            </select>
          </div>
        </section>

        <section>
          <h3>窗口与系统</h3>
          <label class="settings-row">
            关闭按钮
            <select
              value={settings.lifecycle.closeToTray ? "tray" : "exit"}
              on:change={(event) => void updateLifecycle("closeToTray", event.currentTarget.value === "tray")}
            >
              <option value="tray">退到系统托盘</option>
              <option value="exit">直接退出应用</option>
            </select>
          </label>
          <label class="toggle-row">
            <span>开机自启</span>
            <input
              type="checkbox"
              checked={settings.lifecycle.launchAtStartup}
              on:change={(event) => void updateLifecycle("launchAtStartup", event.currentTarget.checked)}
            />
          </label>
          <p class="muted">托盘图标右键菜单可打开窗口或退出应用；再次启动程序会聚焦已运行窗口。</p>
        </section>

        <section>
          <h3>快捷键</h3>
          <label class="shortcut-row">
            新建内容
            <input value={settings.shortcuts.newTask} on:change={(event) => updateShortcut("newTask", event.currentTarget.value)} />
            <small>聚焦下方输入框</small>
          </label>
          <label class="shortcut-row">
            搜索
            <input value={settings.shortcuts.focusSearch} on:change={(event) => updateShortcut("focusSearch", event.currentTarget.value)} />
            <small>聚焦搜索框</small>
          </label>
          <label class="shortcut-row">
            全局唤起
            <input value={settings.shortcuts.toggleWindow} on:change={(event) => updateShortcut("toggleWindow", event.currentTarget.value)} />
            <small>系统级显示/隐藏</small>
          </label>
          <label class="shortcut-row">
            设置
            <input value={settings.shortcuts.openSettings} on:change={(event) => updateShortcut("openSettings", event.currentTarget.value)} />
            <small>打开或关闭设置</small>
          </label>
        </section>

        <section>
          <h3>云同步预留</h3>
          <div class="sync-card">
            <label class="settings-row">
              提供方
              <select value={settings.cloud.provider} on:change={(event) => updateCloudProvider(event.currentTarget.value as Settings["cloud"]["provider"])}>
                <option value="none">未启用</option>
                <option value="webdav">WebDAV</option>
                <option value="s3">S3</option>
                <option value="custom">自定义 HTTP</option>
              </select>
            </label>
            <label class="settings-row">
              地址
              <input value={settings.cloud.endpoint} placeholder="后续实现时使用" on:input={(event) => updateCloudEndpoint(event.currentTarget.value)} />
            </label>
            <p class="muted">当前版本只保留配置结构，不执行任何网络同步。</p>
          </div>
        </section>
      </aside>
    {/if}
  </div>

  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
</div>

<div class="window-controls" style={windowControlsStyle} on:click|stopPropagation>
  <button type="button" aria-label="最小化" on:click={minimizeWindow}><Minus size={16} /></button>
  <button type="button" aria-label="最大化" on:click={toggleMaximizeWindow}><Square size={15} /></button>
  <button type="button" aria-label="关闭" on:click={closeWindow}><X size={17} /></button>
</div>

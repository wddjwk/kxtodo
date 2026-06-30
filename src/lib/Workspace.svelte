<script lang="ts">
  import { tick } from "svelte";
  import {
    ArrowLeft, ArrowUpDown, Calendar, CalendarDays, ChevronDown, ChevronLeft, ChevronRight,
    ChevronsDown, ChevronsUp, Download, Eraser, FolderInput, Image,
    Lightbulb, MoreHorizontal, Palette, PenLine, Plus, Search, Star, Sun, Trash2, Upload
  } from "@lucide/svelte";
  import {
    appState, appSettings, commit, commitSettings, showToast,
    searchQuery, selectedNode, visibleTasks, selectedBackground,
    accent, isSearching, now, todayIso, yesterdayIso, dateOnly,
    createTaskId, safeFileName, fileToDataUrl, APP_VERSION
  } from "./stores";
  import { moveTargetOptions, nodeAndDescendantIds, exportStateForNode, getBackground } from "./nodes";
  import { buildMainStyle, buildMenuStyle, uiScaleValue } from "./styles";
  import { normalizeState, normalizeSettings, defaultBackground, themePresets, createEntryNode } from "./defaults";
  import { exportData, openExternalUrl, isTauriRuntime, deleteBackgroundImage, pickImageFile, importBackgroundImage, backgroundImageUrl, deleteNodeImages, saveMdImageFromDataUrl, mdImageUrl } from "./backend";
  import {
    imageCache, resolveImageSrc, isLocalImageRef, localImageRef, localImageFilename, primeImageCache,
    mdImageCache, resolveMarkdownImages, primeMdImageCache
  } from "./images";
  import IconGlyph from "./IconGlyph.svelte";
  import TaskCard from "./TaskCard.svelte";
  import DatePicker from "./DatePicker.svelte";
  import { showMobileList, isMobile } from "./platform";
  import type { AppNode, AppState, ListBackground, Settings, Task } from "./types";

  type SortMode = "created-desc" | "created-asc" | "alpha-asc" | "alpha-desc" | "due-asc" | "due-desc" | "importance";

  const sortLabels: Record<SortMode, string> = {
    "created-desc": "创建时间 ↓ 最新",
    "created-asc": "创建时间 ↑ 最早",
    "alpha-asc": "字母顺序 A → Z",
    "alpha-desc": "字母顺序 Z → A",
    "due-asc": "截止时间 ↑ 最近",
    "due-desc": "截止时间 ↓ 最远",
    "importance": "重要性优先"
  };

  let newTaskDraft = "";
  let selectedTaskId: string | null = null;
  let showCompleted = true;
  let showListMenu = false;
  let showSortOptions = false;
  let showMoveOptions = false;
  let showSuggestions = false;
  let showCalendar = false;
  let sortMode: SortMode = "created-desc";
  let allExpanded = false;
  let taskMenu: { taskId: string; x: number; y: number; showDate: boolean } | null = null;
  let taskMenuHeight = 0;
  let taskMenuWidth = 0;
  let backgroundLinkDraft = "";
  let backgroundDraftNodeId = "";
  let taskInput: HTMLTextAreaElement;
  let importInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;
  let colorPickerInput: HTMLInputElement;
  let showMobileHeaderActions = false;

  // Calendar state
  let calViewMode: "month" | "week" = "month";
  let calYear = new Date().getFullYear();
  let calMonth = new Date().getMonth();
  // The date whose completed tasks are mirrored in the main area (history view).
  // Always resets to today when leaving / re-entering My Day.
  let myDayViewDate = todayIso();

  $: resolvedBgImage = resolveImageSrc($selectedBackground.image, $imageCache);
  $: mainStyle = buildMainStyle($selectedBackground, $accent, resolvedBgImage);
  $: isMyDay = $selectedNode?.id === "my-day";
  $: if (!isMyDay && myDayViewDate !== todayIso()) myDayViewDate = todayIso();
  $: isMyDayHistory = isMyDay && myDayViewDate !== todayIso();
  $: sortedTasks = sortTasks($visibleTasks, sortMode);
  $: incompleteTasks = isMyDayHistory ? [] : sortedTasks.filter((task) => !task.completed);
  $: completedTasks = isMyDay
    ? (isMyDayHistory
        ? sortTasks(completedByDate[myDayViewDate] ?? [], sortMode)
        : sortedTasks.filter((task) => task.completed && dateOnly(task.completedAt) === todayIso()))
    : sortedTasks.filter((task) => task.completed);
  $: selectedMoveTargets = $selectedNode ? moveTargetOptions($selectedNode.id, $appState.nodes) : [];
  $: taskMenuStyle = taskMenu ? buildMenuStyle(taskMenu.x, taskMenu.y, taskMenuWidth || 264, taskMenuHeight || (taskMenu.showDate ? 520 : 188), uiScaleValue($appSettings.appearance.uiScale)) : "";
  $: taskMenuTask = taskMenu ? $appState.tasks.find((task) => task.id === taskMenu?.taskId) : null;
  $: if (($selectedNode?.id ?? "") !== backgroundDraftNodeId) {
    backgroundDraftNodeId = $selectedNode?.id ?? "";
    backgroundLinkDraft = isLocalImageRef($selectedBackground.image) ? "" : ($selectedBackground.image ?? "");
  }

  // My Day suggestions
  $: suggestedTasks = (() => {
    if (!isMyDay) return [];
    const today = todayIso();
    const yesterday = yesterdayIso();
    const candidates: Task[] = [];
    const seen = new Set<string>();
    for (const task of $appState.tasks) {
      if (task.completed || task.myDay || seen.has(task.id)) continue;
      const created = dateOnly(task.createdAt);
      const due = dateOnly(task.dueDate);
      if (created === today || created === yesterday || due === today) {
        candidates.push(task);
        seen.add(task.id);
      }
    }
    if (candidates.length > 0) return candidates;
    return $appState.tasks
      .filter((t) => !t.completed && !t.myDay)
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
      .slice(0, 5);
  })();

  // Calendar: completed tasks by date
  $: completedByDate = (() => {
    const map: Record<string, Task[]> = {};
    for (const task of $appState.tasks) {
      if (!task.completed || !task.completedAt) continue;
      const d = task.completedAt.slice(0, 10);
      (map[d] ??= []).push(task);
    }
    return map;
  })();

  $: calSelectedTasks = completedByDate[myDayViewDate] ?? [];

  // Week view: a summary of the whole week's completed tasks grouped by day.
  $: weekSummary = (() => {
    const d = new Date(myDayViewDate + "T00:00:00");
    const start = new Date(d);
    start.setDate(start.getDate() - d.getDay());
    const days: Array<{ date: string; label: string; tasks: Task[] }> = [];
    for (let i = 0; i < 7; i++) {
      const cur = new Date(start);
      cur.setDate(cur.getDate() + i);
      const ds = fmtDateStr(cur.getFullYear(), cur.getMonth(), cur.getDate());
      days.push({
        date: ds,
        label: `${cur.getMonth() + 1}月${cur.getDate()}日 周${weekDayLabels[cur.getDay()]}`,
        tasks: completedByDate[ds] ?? []
      });
    }
    return days;
  })();

  $: weekSummaryTotal = weekSummary.reduce((sum, day) => sum + day.tasks.length, 0);

  const weekDayLabels = ["日", "一", "二", "三", "四", "五", "六"];

  function formatMyDayDate(dateStr: string): string {
    const d = new Date(dateStr + "T00:00:00");
    const weekDays = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
    return `${d.getMonth() + 1}月${d.getDate()}日, ${weekDays[d.getDay()]}`;
  }

  function calMonthDays(): Array<{ date: string; day: number; current: boolean; hasTask: boolean }> {
    const first = new Date(calYear, calMonth, 1);
    const last = new Date(calYear, calMonth + 1, 0);
    const startDow = first.getDay();
    const totalDays = last.getDate();
    const cells: Array<{ date: string; day: number; current: boolean; hasTask: boolean }> = [];
    const prevLast = new Date(calYear, calMonth, 0);
    for (let i = startDow - 1; i >= 0; i--) {
      const dd = prevLast.getDate() - i;
      const ds = fmtDateStr(calYear, calMonth - 1, dd);
      cells.push({ date: ds, day: dd, current: false, hasTask: !!completedByDate[ds]?.length });
    }
    for (let dd = 1; dd <= totalDays; dd++) {
      const ds = fmtDateStr(calYear, calMonth, dd);
      cells.push({ date: ds, day: dd, current: true, hasTask: !!completedByDate[ds]?.length });
    }
    const rem = (7 - (cells.length % 7)) % 7;
    for (let dd = 1; dd <= rem; dd++) {
      const ds = fmtDateStr(calYear, calMonth + 1, dd);
      cells.push({ date: ds, day: dd, current: false, hasTask: !!completedByDate[ds]?.length });
    }
    return cells;
  }

  function calWeekRange(): string {
    const d = new Date(myDayViewDate + "T00:00:00");
    const start = new Date(d);
    start.setDate(start.getDate() - d.getDay());
    const end = new Date(start);
    end.setDate(end.getDate() + 6);
    return `${start.getMonth() + 1}月${start.getDate()}日 – ${end.getMonth() + 1}月${end.getDate()}日`;
  }

  function fmtDateStr(y: number, m: number, d: number): string {
    const dt = new Date(y, m, d);
    return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  }

  function calPrev(): void { if (calMonth === 0) { calYear--; calMonth = 11; } else calMonth--; }
  function calNext(): void { if (calMonth === 11) { calYear++; calMonth = 0; } else calMonth++; }

  function collapsedLine(md: string): string {
    const firstLine = md.split("\n")[0] ?? "";
    return firstLine.replace(/^#+\s*/, "");
  }

  export function closeOverlays(): void {
    showListMenu = false;
    showSortOptions = false;
    showMoveOptions = false;
    showSuggestions = false;
    showCalendar = false;
    showMobileHeaderActions = false;
    taskMenu = null;
  }

  export function focusComposer(): void {
    taskInput?.focus();
  }

  function toggleCompletion(taskId: string): void {
    updateTask(taskId, (task) => ({
      ...task,
      completed: !task.completed,
      completedAt: !task.completed ? now() : undefined
    }));
  }

  function addToMyDay(taskId: string): void {
    updateTask(taskId, (task) => ({ ...task, myDay: true }));
  }

  function handleTaskSetDate(event: CustomEvent<{ id: string; date: string }>): void {
    const dateVal = event.detail.date ? event.detail.date.slice(0, 10) : undefined;
    const addMyDay = dateVal === todayIso();
    updateTask(event.detail.id, (task) => ({
      ...task,
      dueDate: dateVal || undefined,
      plannedDate: dateVal || undefined,
      myDay: addMyDay ? true : task.myDay
    }));
  }

  function toggleSuggestions(): void {
    showSuggestions = !showSuggestions;
    showCalendar = false;
    showListMenu = false;
    taskMenu = null;
  }

  function toggleCalendar(): void {
    showCalendar = !showCalendar;
    showSuggestions = false;
    showListMenu = false;
    taskMenu = null;
    if (showCalendar) {
      const d = new Date();
      calYear = d.getFullYear();
      calMonth = d.getMonth();
    }
  }

  function sortTasks(tasks: Task[], mode: SortMode): Task[] {
    return [...tasks].sort((a, b) => {
      switch (mode) {
        case "created-desc": return b.createdAt.localeCompare(a.createdAt);
        case "created-asc": return a.createdAt.localeCompare(b.createdAt);
        case "alpha-asc": return a.markdown.localeCompare(b.markdown, "zh");
        case "alpha-desc": return b.markdown.localeCompare(a.markdown, "zh");
        case "due-asc": {
          if (!a.dueDate && !b.dueDate) return 0;
          if (!a.dueDate) return 1;
          if (!b.dueDate) return -1;
          return a.dueDate.localeCompare(b.dueDate);
        }
        case "due-desc": {
          if (!a.dueDate && !b.dueDate) return 0;
          if (!a.dueDate) return 1;
          if (!b.dueDate) return -1;
          return b.dueDate.localeCompare(a.dueDate);
        }
        case "importance": return (b.important ? 1 : 0) - (a.important ? 1 : 0);
        default: return 0;
      }
    });
  }

  function setSortMode(mode: SortMode): void {
    sortMode = mode;
    showSortOptions = false;
  }

  function toggleExpandAll(): void {
    allExpanded = !allExpanded;
    const expandTarget = allExpanded;
    const visibleIds = new Set($visibleTasks.map((t) => t.id));
    commit({
      ...$appState,
      tasks: $appState.tasks.map((task) => {
        if (!visibleIds.has(task.id)) return task;
        // Skip single-line tasks when expanding
        if (expandTarget && !task.markdown.includes("\n")) return task;
        return { ...task, expanded: expandTarget, editing: false };
      })
    });
  }

  function updateTask(taskId: string, updater: (task: Task) => Task): void {
    commit({
      ...$appState,
      tasks: $appState.tasks.map((task) => (task.id === taskId ? { ...updater(task), updatedAt: now() } : task))
    });
  }

  function deleteTask(taskId: string): void {
    commit({
      ...$appState,
      tasks: $appState.tasks.filter((task) => task.id !== taskId)
    });
    taskMenu = null;
  }

  function taskTargetNode(): AppNode | undefined {
    return $selectedNode?.kind === "entry" ? $selectedNode : $appState.nodes.find((n) => n.kind === "entry");
  }

  function addTaskFromDraft(): void {
    const markdown = newTaskDraft.trim();
    if (!markdown) return;
    const targetNode = taskTargetNode();
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
      important: $selectedNode?.id === "important",
      myDay: $selectedNode?.id === "my-day",
      expanded: false,
      editing: false,
      createdAt: timestamp,
      updatedAt: timestamp
    };
    commit({ ...$appState, tasks: [...$appState.tasks, task] });
    newTaskDraft = "";
    void tick().then(resizeComposer);
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      addTaskFromDraft();
    }
  }

  async function handleComposerPaste(event: ClipboardEvent): Promise<void> {
    if (!isTauriRuntime) return;
    const items = event.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (!item.type.startsWith("image/")) continue;
      event.preventDefault();
      const targetNode = taskTargetNode();
      if (!targetNode) {
        showToast("请先创建一个条目");
        return;
      }
      const file = item.getAsFile();
      if (!file) return;
      try {
        const dataUrl = await fileToDataUrl(file);
        const filename = await saveMdImageFromDataUrl(dataUrl, targetNode.id);
        const url = await mdImageUrl(targetNode.id, filename);
        primeMdImageCache(targetNode.id, filename, url);
        const cursorStart = taskInput?.selectionStart ?? newTaskDraft.length;
        const cursorEnd = taskInput?.selectionEnd ?? cursorStart;
        const before = newTaskDraft.slice(0, cursorStart);
        const after = newTaskDraft.slice(cursorEnd);
        newTaskDraft = `${before}\n![](${filename})\n${after}`;
        await tick();
        resizeComposer();
      } catch (error) {
        showToast(`图片粘贴失败：${String(error)}`);
      }
      return;
    }
  }

  function resizeComposer(): void {
    if (!taskInput) return;
    taskInput.style.height = "auto";
    taskInput.style.height = `${Math.min(taskInput.scrollHeight, 180)}px`;
  }

  function openTaskMenu(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    taskMenu = { taskId: event.detail.id, x: event.detail.x, y: event.detail.y, showDate: false };
    showListMenu = false;
  }

  function setTaskDate(taskId: string, date: string): void {
    const dateVal = date ? date.slice(0, 10) : undefined;
    const addToMyDay = dateVal === todayIso();
    updateTask(taskId, (task) => ({
      ...task,
      dueDate: dateVal || undefined,
      plannedDate: dateVal || undefined,
      myDay: addToMyDay ? true : task.myDay
    }));
    taskMenu = null;
  }

  function setBackground(patch: Partial<ListBackground>): void {
    if (!$selectedNode) return;
    commit({
      ...$appState,
      backgrounds: {
        ...$appState.backgrounds,
        [$selectedNode.id]: {
          ...getBackground($selectedNode.id, $appState.backgrounds),
          ...patch
        }
      }
    });
  }

  function updateBackgroundLink(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) return;
    const previous = $selectedBackground.image;
    backgroundLinkDraft = target.value;
    const next = target.value.trim() || undefined;
    setBackground({ image: next });
    if (isLocalImageRef(previous) && previous !== next) void deleteBackgroundImage(localImageFilename(previous));
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

  function handleColorPick(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      applyTheme(target.value);
    }
  }

  function openColorPicker(): void {
    colorPickerInput?.click();
  }

  async function pickColorFromScreen(): Promise<void> {
    try {
      const EyeDropperCtor = (window as unknown as Record<string, unknown>).EyeDropper as { new(): { open(): Promise<{ sRGBHex: string }> } } | undefined;
      if (!EyeDropperCtor) {
        showToast("当前环境不支持取色器");
        return;
      }
      const dropper = new EyeDropperCtor();
      const result = await dropper.open();
      applyTheme(result.sRGBHex);
    } catch {
      // User cancelled
    }
  }

  async function pickBackgroundImage(): Promise<void> {
    if (!isTauriRuntime) {
      backgroundFileInput.click();
      return;
    }
    try {
      const path = await pickImageFile();
      if (!path) return;
      const previous = $selectedBackground.image;
      const filename = await importBackgroundImage(path);
      const url = await backgroundImageUrl(filename);
      primeImageCache(filename, url);
      setBackground({ image: localImageRef(filename) });
      backgroundLinkDraft = "";
      if (isLocalImageRef(previous)) void deleteBackgroundImage(localImageFilename(previous));
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    }
  }

  async function uploadBackgroundImage(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      const dataUrl = await fileToDataUrl(target.files[0]);
      setBackground({ image: dataUrl });
      backgroundLinkDraft = "";
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    } finally {
      target.value = "";
    }
  }

  function clearBackground(): void {
    const previous = $selectedBackground.image;
    backgroundLinkDraft = "";
    setBackground({ image: undefined });
    if (isLocalImageRef(previous)) void deleteBackgroundImage(localImageFilename(previous));
  }

  function deleteCurrentNode(): void {
    if (!$selectedNode || $selectedNode.kind === "system") {
      showToast("内置列表不能删除");
      return;
    }
    const id = $selectedNode.id;
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
    const fallbackId = nodes.find((n) => n.kind === "entry")?.id ?? "my-day";
    commit({
      ...$appState,
      nodes,
      tasks: $appState.tasks.filter((t) => validNodeIds.has(t.nodeId)),
      selectedNodeId: fallbackId,
      backgrounds
    });
    showListMenu = false;
    showMobileList();
  }

  async function exportCurrentList(): Promise<void> {
    if (!$selectedNode) return;
    const payload = {
      version: APP_VERSION,
      exportedAt: now(),
      scope: "node",
      nodeId: $selectedNode.id,
      state: exportStateForNode($selectedNode, $appState)
    };
    await exportData(payload, `${safeFileName($selectedNode.name)}-${APP_VERSION}.json`);
    showListMenu = false;
    showToast("导出完成");
  }

  async function exportAll(): Promise<void> {
    const payload = {
      version: APP_VERSION,
      exportedAt: now(),
      scope: "all",
      state: $appState,
      settings: $appSettings
    };
    await exportData(payload, `kxtodo-${APP_VERSION}-all.json`);
    showListMenu = false;
    showToast("全部数据已导出");
  }

  async function importFromFile(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
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

  function toggleListMenu(): void {
    showListMenu = !showListMenu;
    showSortOptions = false;
    showMoveOptions = false;
    showSuggestions = false;
    showCalendar = false;
    taskMenu = null;
  }

  function handleListMenuKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    toggleListMenu();
  }

  function startRename(id: string): void {
    showListMenu = false;
  }

  function moveNodeToGroup(nodeId: string, parentId: string | null): void {
    const source = $appState.nodes.find((n) => n.id === nodeId);
    if (!source || source.kind === "system" || source.parentId === parentId) {
      showListMenu = false;
      return;
    }
    const nextParentId = parentId;
    const targetParent = nextParentId ? $appState.nodes.find((n) => n.id === nextParentId && n.kind === "category") : null;
    if (nextParentId && !targetParent) {
      showToast("目标分组不存在");
      return;
    }
    if (source.kind === "category" && nextParentId && nodeAndDescendantIds(source.id, $appState.nodes).has(nextParentId)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }
    const withoutSource = $appState.nodes.filter((n) => n.id !== nodeId);
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
    showListMenu = false;
  }

  async function openTaskLink(event: CustomEvent<string>): Promise<void> {
    try {
      await openExternalUrl(event.detail);
    } catch (error) {
      showToast(`打开链接失败：${String(error)}`);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<main class="workspace" style={mainStyle}>
  <section class="list-header">
    <div>
      <button class="mobile-back" type="button" aria-label="返回列表" on:click|stopPropagation={showMobileList}>
        <ArrowLeft size={26} />
      </button>
      <span class="header-icon">
        {#if $isSearching}
          <Search size={34} />
        {:else}
          <IconGlyph icon={$selectedNode?.icon ?? "notebook"} size={34} />
        {/if}
      </span>
      <h1
        class:mobile-title-tap={$isMobile}
        on:click|stopPropagation={() => { if ($isMobile) showMobileHeaderActions = !showMobileHeaderActions; }}
      >{$isSearching ? `搜索结果：${$searchQuery}` : $selectedNode?.name ?? "KXToDo"}</h1>
    </div>
    <div class="header-actions" class:mobile-hidden={$isMobile && !showMobileHeaderActions} on:click|stopPropagation>
      {#if isMyDay}
        <button type="button" title="完成日历" on:click|stopPropagation={toggleCalendar}>
          <Calendar size={21} />
        </button>
        <button type="button" title="建议添加" on:click|stopPropagation={toggleSuggestions}>
          <Lightbulb size={21} />
        </button>
      {/if}
      <button
        type="button"
        title={allExpanded ? "收起全部" : "展开全部"}
        on:click|stopPropagation={toggleExpandAll}
      >{#if allExpanded}<ChevronsUp size={21} />{:else}<ChevronsDown size={21} />{/if}</button>
      <button
        type="button"
        title="分类/条目菜单"
        on:mousedown|preventDefault|stopPropagation={toggleListMenu}
        on:click|stopPropagation
        on:keydown={handleListMenuKeydown}
      ><MoreHorizontal size={23} /></button>

      {#if showSuggestions}
        <section class="suggestion-panel" on:click|stopPropagation>
          <div class="suggestion-panel-title">建议添加到我的一天</div>
          {#if suggestedTasks.length === 0}
            <div class="suggestion-empty">暂无建议</div>
          {:else}
            {#each suggestedTasks as task (task.id)}
              <div class="suggestion-item">
                <span class="suggestion-text">{collapsedLine(task.markdown)}</span>
                <button class="suggestion-add" type="button" title="添加到我的一天" on:click|stopPropagation={() => addToMyDay(task.id)}>
                  <Plus size={16} />
                </button>
              </div>
            {/each}
          {/if}
        </section>
      {/if}

      {#if showCalendar}
        <section class="calendar-panel" on:click|stopPropagation>
          <div class="calendar-header">
            <button type="button" on:click={calPrev}><ChevronLeft size={16} /></button>
            <span>{calYear}年{calMonth + 1}月</span>
            <button type="button" on:click={calNext}><ChevronRight size={16} /></button>
          </div>
          <div class="calendar-view-toggle">
            <button type="button" class:active={calViewMode === "month"} on:click={() => (calViewMode = "month")}>月</button>
            <button type="button" class:active={calViewMode === "week"} on:click={() => (calViewMode = "week")}>周</button>
          </div>
          {#if calViewMode === "month"}
            <div class="calendar-grid">
              {#each weekDayLabels as label}
                <span class="day-header">{label}</span>
              {/each}
              {#each calMonthDays() as cell}
                <button
                  type="button"
                  class="day-cell"
                  class:other-month={!cell.current}
                  class:today={cell.date === todayIso()}
                  class:selected={cell.date === myDayViewDate}
                  class:has-tasks={cell.hasTask}
                  on:click={() => (myDayViewDate = cell.date)}
                >{cell.day}</button>
              {/each}
            </div>
            <div class="calendar-tasks-title">
              {(() => { const p = myDayViewDate.split("-").map(Number); return `${p[1]}月${p[2]}日 完成的任务`; })()}
            </div>
            {#if calSelectedTasks.length === 0}
              <div class="calendar-no-tasks">无完成任务</div>
            {:else}
              {#each calSelectedTasks as task (task.id)}
                <div class="calendar-task-item">{collapsedLine(task.markdown)}</div>
              {/each}
            {/if}
          {:else}
            <div class="calendar-tasks-title">
              {calWeekRange()} · 共 {weekSummaryTotal} 项
            </div>
            <div class="week-summary">
              {#each weekSummary as day (day.date)}
                <div class="week-summary-day" class:active={day.date === myDayViewDate}>
                  <button type="button" class="week-summary-head" class:is-today={day.date === todayIso()} on:click={() => (myDayViewDate = day.date)}>
                    <span>{day.label}</span>
                    <span class="week-summary-count">{day.tasks.length}</span>
                  </button>
                  {#each day.tasks as task (task.id)}
                    <div class="calendar-task-item">{collapsedLine(task.markdown)}</div>
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        </section>
      {/if}
      {#if showListMenu}
        <section class="list-menu">
          <button type="button" disabled={!$selectedNode || $selectedNode.kind === "system"} on:click={() => $selectedNode && startRename($selectedNode.id)}>
            <PenLine size={15} /> 重命名
          </button>
          {#if $selectedNode && $selectedNode.kind !== "system"}
            <button type="button" class="has-submenu" on:click={() => showMoveOptions = !showMoveOptions}>
              <FolderInput size={15} /> 移动到分组
            </button>
            {#if showMoveOptions}
              <div class="menu-submenu">
                {#each selectedMoveTargets as target}
                  <button
                    type="button"
                    class:active={($selectedNode.parentId ?? "") === target.id}
                    on:click={() => { moveNodeToGroup($selectedNode.id, target.id || null); showMoveOptions = false; }}
                  >{target.name}</button>
                {/each}
              </div>
            {/if}
          {/if}
          <button type="button" on:click={() => showSortOptions = !showSortOptions}>
            <ArrowUpDown size={15} /> 排序方式
          </button>
          {#if showSortOptions}
            <div class="sort-submenu">
              {#each Object.entries(sortLabels) as [mode, label]}
                <button type="button" class:active={sortMode === mode} on:click={() => setSortMode(mode as SortMode)}>{label}</button>
              {/each}
            </div>
          {/if}
          {#if $selectedNode && $selectedNode.kind !== "system"}
            <button class="danger" type="button" on:click={deleteCurrentNode}>
              <Trash2 size={15} /> 删除当前条目
            </button>
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
            <button type="button" class="palette-button" title="自定义颜色" on:click={openColorPicker}></button>
          </div>
          <input bind:this={colorPickerInput} class="hidden-file" type="color" value={$selectedBackground.color} on:input={handleColorPick} />
          <label class="background-link">
            背景图片链接
            <input value={backgroundLinkDraft} placeholder="https://..." on:input={updateBackgroundLink} />
          </label>
          <label class="opacity-row">
            图片透明度
            <input type="range" min="0" max="80" value={Math.round(($selectedBackground.imageOpacity ?? 0.28) * 100)} on:input={updateBackgroundOpacity} />
          </label>
          <div class="menu-inline two">
            <button type="button" on:click={pickBackgroundImage}><Image size={15} /> 上传图片</button>
            <button type="button" on:click={clearBackground}><Eraser size={15} /> 清除背景</button>
          </div>
        </section>
      {/if}
    </div>
  </section>

  {#if isMyDay}
    <p class="my-day-subtitle">
      {formatMyDayDate(myDayViewDate)}
      {#if isMyDayHistory}
        <button type="button" class="my-day-back" on:click={() => (myDayViewDate = todayIso())}>返回今天</button>
      {/if}
    </p>
  {/if}

  <input bind:this={importInput} class="hidden-file" type="file" accept="application/json,.json" on:change={importFromFile} />
  <input bind:this={backgroundFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadBackgroundImage} />

  <section class="task-list">
    {#each incompleteTasks as task (task.id)}
      <TaskCard
        {task}
        nodeId={task.nodeId}
        selected={taskMenu?.taskId === task.id || selectedTaskId === task.id}
        linkOpenMode={$appSettings.appearance.linkOpenMode}
        on:toggle={(event) => toggleCompletion(event.detail)}
        on:expand={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, expanded: !item.expanded })); }}
        on:edit={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, editing: true, expanded: true })); }}
        on:commit={(event) => updateTask(event.detail.id, (item) => ({ ...item, markdown: event.detail.markdown, editing: false, expanded: true }))}
        on:context={openTaskMenu}
        on:openLink={openTaskLink}
        on:setDate={handleTaskSetDate}
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
              nodeId={task.nodeId}
              selected={taskMenu?.taskId === task.id || selectedTaskId === task.id}
              linkOpenMode={$appSettings.appearance.linkOpenMode}
              on:toggle={(event) => toggleCompletion(event.detail)}
              on:expand={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, expanded: !item.expanded })); }}
              on:edit={(event) => { selectedTaskId = event.detail; taskMenu = null; updateTask(event.detail, (item) => ({ ...item, editing: true, expanded: true })); }}
              on:commit={(event) => updateTask(event.detail.id, (item) => ({ ...item, markdown: event.detail.markdown, editing: false, expanded: true }))}
              on:context={openTaskMenu}
              on:openLink={openTaskLink}
              on:setDate={handleTaskSetDate}
            />
          {/each}
        {/if}
      </section>
    {/if}

    {#if incompleteTasks.length === 0 && completedTasks.length === 0}
      <div class="empty-state">
        <strong>{$isSearching ? "没有搜索结果" : (isMyDayHistory ? "这一天没有完成任何事项" : "这个条目还没有内容")}</strong>
        {#if !isMyDayHistory}
          <span>在下方输入 Markdown，按 Enter 添加；Shift + Enter 换行。</span>
        {/if}
      </div>
    {/if}
  </section>

  {#if taskMenu && taskMenuTask}
    <div
      class="task-context-menu"
      style={taskMenuStyle}
      bind:clientHeight={taskMenuHeight}
      bind:clientWidth={taskMenuWidth}
      on:click|stopPropagation
    >
      <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, myDay: !task.myDay })); taskMenu = null; }}>
        <Sun size={16} /> {taskMenuTask.myDay ? "从我的一天中移除" : "添加到我的一天"}
      </button>
      <button type="button" on:click={() => taskMenu && (taskMenu = { ...taskMenu, showDate: !taskMenu.showDate })}>
        <CalendarDays size={16} /> 添加日期
      </button>
      {#if taskMenu.showDate}
        <div class="task-menu-date">
          <DatePicker
            value={taskMenuTask.dueDate?.slice(0, 10) ?? ""}
            on:select={(event) => setTaskDate(taskMenuTask.id, event.detail)}
            on:clear={() => setTaskDate(taskMenuTask.id, "")}
          />
        </div>
      {/if}
      <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, important: !task.important })); taskMenu = null; }}>
        <Star size={16} /> {taskMenuTask.important ? "取消收藏" : "收藏"}
      </button>
      <button type="button" on:click={() => { updateTask(taskMenuTask.id, (task) => ({ ...task, editing: true, expanded: true })); taskMenu = null; }}>
        <PenLine size={16} /> 编辑
      </button>
      <button class="danger" type="button" on:click={() => deleteTask(taskMenuTask.id)}>
        <Trash2 size={16} /> 删除
      </button>
    </div>
  {/if}

  {#if !isMyDayHistory}
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
          on:paste={handleComposerPaste}
        ></textarea>
      </div>
    </section>
  {/if}
</main>

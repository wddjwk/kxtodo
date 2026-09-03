<script lang="ts">
  import { tick } from "svelte";
  import {
    ArrowLeft, Calendar, CalendarDays, ChevronDown, ChevronLeft, ChevronRight,
    ChevronsDown, ChevronsUp, FolderInput,
    Lightbulb, MoreHorizontal, PenLine, Plus, Search, Settings as SettingsIcon, SmilePlus, Star, Sun, Tag, Trash2, X
  } from "@lucide/svelte";
  import {
    appState, appSettings, showToast,
    searchQuery, selectedNode, visibleTasks, selectedBackground,
    accent, isSearching, todayIso, yesterdayIso, dateOnly,
    taskEmojiPicker, editorTaskId, fileToDataUrl
  } from "./stores";
  import {
    updateTask as updateTaskAction, deleteTask as deleteTaskAction,
    addTask as addTaskAction, setItemUi as setItemUiAction,
    setItemsUi as setItemsUiAction, replaceTaskTags as replaceTaskTagsAction,
    replaceTaskEmojis as replaceTaskEmojisAction,
    renameNode as renameNodeAction
  } from "./actions";
  import { taskMoveTargets } from "./nodes";
  import { buildMainStyle } from "./styles";
  import { hasMultipleMarkdownLines } from "./markdown";
  import { openExternalUrl, isTauriRuntime, saveMdImageFromDataUrl, mdImageUrl } from "./backend";
  import { imageCache, resolveImageSrc, mdImageCache, primeMdImageCache } from "./images";
  import IconGlyph from "./IconGlyph.svelte";
  import TaskCard from "./TaskCard.svelte";
  import ScheduledTasksView from "./ScheduledTasksView.svelte";
  import DatePicker from "./DatePicker.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import MoveTargetTree from "./menu/MoveTargetTree.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import { sortTasks, type SortMode } from "./sort";
  import { showMobileList, isMobile } from "./platform";
  import type { AppNode, TagColor, Task } from "./types";

  let newTaskDraft = "";
  let showCompleted = true;
  let showSuggestions = false;
  let showCalendar = false;
  let sortMode: SortMode = "created-desc";
  let taskMenu: { taskId: string; x: number; y: number } | null = null;
  let listMenuAt: { x: number; y: number } | null = null;
  let tagInputText = "";
  let selectedTagColor: TagColor = "yellow";
  let editingTagIdInMenu = "";
  let editingTagTextInMenu = "";
  let headerRenaming = false;
  let headerRenameDraft = "";
  let headerRenameInput: HTMLInputElement;
  let taskInput: HTMLTextAreaElement;
  let schedulerViewRef: ScheduledTasksView;
  let showMobileHeaderActions = false;
  let linkPreviewUrl = "";

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
  $: isScheduled = !$isSearching && $selectedNode?.id === "scheduled";
  $: if (!isMyDay && myDayViewDate !== todayIso()) myDayViewDate = todayIso();
  $: isMyDayHistory = isMyDay && myDayViewDate !== todayIso();
  $: sortedTasks = sortTasks($visibleTasks, sortMode);
  $: incompleteTasks = isMyDayHistory ? [] : sortedTasks.filter((task) => !task.completed);
  $: completedTasks = isMyDay
    ? (isMyDayHistory
        ? sortTasks(completedByDate[myDayViewDate] ?? [], sortMode)
        : sortedTasks.filter((task) => task.completed && dateOnly(task.completedAt) === todayIso()))
    : sortedTasks.filter((task) => task.completed);
  $: taskMenuTask = taskMenu ? $appState.tasks.find((task) => task.id === taskMenu?.taskId) : null;
  $: hasTaskMoveTargets = taskMenu ? taskMoveTargets($appState.nodes, taskMenuTask?.nodeId ?? "").length > 0 : false;
  $: expandableTasks = $visibleTasks.filter((task) => hasMultipleMarkdownLines(task.markdown));
  $: allExpanded = expandableTasks.length > 0 && expandableTasks.every((task) => task.expanded);
  $: allCollapsed = expandableTasks.every((task) => !task.expanded);
  $: if (!taskMenu) { tagInputText = ""; selectedTagColor = "yellow"; editingTagIdInMenu = ""; editingTagTextInMenu = ""; }

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
    showSuggestions = false;
    showCalendar = false;
    showMobileHeaderActions = false;
    schedulerViewRef?.closeOverlays();
    taskMenu = null;
    listMenuAt = null;
    linkPreviewUrl = "";
    taskEmojiPicker.set(null);
  }

  export function focusComposer(): void {
    taskInput?.focus();
  }

  function toggleCompletion(taskId: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void updateTaskAction(taskId, { completed: !task.completed });
    }
  }

  function addToMyDay(taskId: string): void {
    void updateTaskAction(taskId, { myDay: true });
  }

  function handleTaskSetDate(event: CustomEvent<{ id: string; date: string }>): void {
    setTaskDate(event.detail.id, event.detail.date);
  }

  function toggleSuggestions(): void {
    showSuggestions = !showSuggestions;
    showCalendar = false;
    listMenuAt = null;
    taskMenu = null;
  }

  function toggleCalendar(): void {
    showCalendar = !showCalendar;
    showSuggestions = false;
    listMenuAt = null;
    taskMenu = null;
    if (showCalendar) {
      const d = new Date();
      calYear = d.getFullYear();
      calMonth = d.getMonth();
    }
  }

  /** 展开全部 / 收起全部（两个独立动作，非 toggle）。 */
  function expandAll(expanded: boolean): void {
    const ids = $visibleTasks
      .filter((task) => !expanded || hasMultipleMarkdownLines(task.markdown))
      .map((task) => task.id);
    if (ids.length > 0) {
      void setItemsUiAction(ids, expanded);
    }
  }

  function toggleTaskExpansion(taskId: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (!task) return;
    void setItemUiAction(taskId, {
      expanded: hasMultipleMarkdownLines(task.markdown) ? !task.expanded : false
    });
  }

  function openTaskEditor(taskId: string): void {
    taskMenu = null;
    editorTaskId.set(taskId);
  }

  function deleteTask(taskId: string): void {
    void deleteTaskAction(taskId);
    taskMenu = null;
  }

  function addTagToTask(taskId: string, color: TagColor, text?: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (!task) return;
    void replaceTaskTagsAction(taskId, [
      ...task.tags,
      { id: `tag-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`, color, text: text?.trim() || undefined }
    ]);
  }

  function submitTagInput(): void {
    if (!taskMenu || !taskMenuTask) return;
    const text = tagInputText.trim();
    addTagToTask(taskMenuTask.id, selectedTagColor, text);
    tagInputText = "";
  }

  function submitTagEditInMenu(): void {
    if (!taskMenu || !taskMenuTask || !editingTagIdInMenu) return;
    editTagAtTask(taskMenuTask.id, editingTagIdInMenu, editingTagTextInMenu);
    editingTagIdInMenu = "";
  }

  function clearTagsFromTask(taskId: string): void {
    void replaceTaskTagsAction(taskId, []);
  }

  function removeTagFromTask(taskId: string, tagId: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskTagsAction(taskId, task.tags.filter((t) => t.id !== tagId));
    }
  }

  function editTagAtTask(taskId: string, tagId: string, text: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskTagsAction(taskId, task.tags.map((t) => (t.id === tagId ? { ...t, text: text || undefined } : t)));
    }
  }

  function moveTaskToNode(taskId: string, targetNodeId: string): void {
    void updateTaskAction(taskId, { entryId: targetNodeId });
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
    void addTaskAction(targetNode.id, {
      markdown,
      important: $selectedNode?.id === "important",
      myDay: $selectedNode?.id === "my-day"
    });
    newTaskDraft = "";
    void tick().then(resizeComposer);
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
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
    taskMenu = { taskId: event.detail.id, x: event.detail.x, y: event.detail.y };
    listMenuAt = null;
  }

  function setTaskDate(taskId: string, date: string): void {
    const dateVal = date ? date.slice(0, 10) : null;
    const task = $appState.tasks.find((item) => item.id === taskId);
    void updateTaskAction(taskId, {
      dueDate: dateVal,
      plannedDate: dateVal,
      myDay: dateVal === todayIso() ? true : task?.myDay
    });
    taskMenu = null;
  }

  function openListMenu(event: MouseEvent): void {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    showSuggestions = false;
    showCalendar = false;
    taskMenu = null;
  }

  function beginHeaderRename(): void {
    if (!$selectedNode || $selectedNode.kind === "system") return;
    listMenuAt = null;
    headerRenaming = true;
    headerRenameDraft = $selectedNode.name;
    void tick().then(() => {
      headerRenameInput?.focus();
      headerRenameInput?.select();
    });
  }

  function commitHeaderRename(): void {
    if (!headerRenaming) return;
    const name = headerRenameDraft.trim();
    headerRenaming = false;
    if (!name || !$selectedNode) return;
    void renameNodeAction($selectedNode.id, name);
  }

  function handleHeaderRenameKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commitHeaderRename();
    } else if (event.key === "Escape") {
      headerRenaming = false;
    }
  }

  /** 供 App（编辑器浮窗）与 TaskCard 复用的链接打开入口。 */
  export function openLinkUrl(url: string): void {
    if ($appSettings.appearance.linkOpenMode === "system") {
      void openExternalUrl(url).catch((error) => showToast(`打开链接失败：${String(error)}`));
    } else {
      linkPreviewUrl = url;
    }
  }

  function openTaskLink(event: CustomEvent<string>): void {
    openLinkUrl(event.detail);
  }

  function closeLinkPreview(): void {
    linkPreviewUrl = "";
  }

  function openEmojiPickerForTask(taskId: string): void {
    taskEmojiPicker.set({ taskId, index: -1 });
    taskMenu = null;
  }

  function openEmojiPickerAt(taskId: string, index: number): void {
    taskEmojiPicker.set({ taskId, index });
  }

  function removeEmojiFromTask(taskId: string, index: number): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskEmojisAction(taskId, task.emojis.filter((_, i) => i !== index));
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
      {#if headerRenaming}
        <input
          bind:this={headerRenameInput}
          bind:value={headerRenameDraft}
          class="header-rename-input"
          maxlength="60"
          on:blur={commitHeaderRename}
          on:keydown={handleHeaderRenameKeydown}
          on:click|stopPropagation
        />
      {:else}
        <h1
          class:mobile-title-tap={$isMobile}
          on:click|stopPropagation={() => { if ($isMobile) showMobileHeaderActions = !showMobileHeaderActions; }}
        >{$isSearching ? `搜索结果：${$searchQuery}` : $selectedNode?.name ?? "KXToDo"}</h1>
      {/if}
    </div>
    <div class="header-actions" class:mobile-hidden={$isMobile && !showMobileHeaderActions} on:click|stopPropagation>
      {#if isScheduled}
        <button type="button" title="执行器路径" on:click|stopPropagation={() => schedulerViewRef?.toggleRuntimeSettings()}>
          <SettingsIcon size={21} />
        </button>
      {/if}
      {#if !isScheduled}
        {#if isMyDay}
          <button type="button" title="完成日历" on:click|stopPropagation={toggleCalendar}>
            <Calendar size={21} />
          </button>
          <button type="button" title="建议添加" on:click|stopPropagation={toggleSuggestions}>
            <Lightbulb size={21} />
          </button>
        {/if}
        {#if !allCollapsed}
          <button
            type="button"
            title="收起全部"
            on:click|stopPropagation={() => expandAll(false)}
          ><ChevronsUp size={21} /></button>
        {/if}
        {#if !allExpanded}
          <button
            type="button"
            title="展开全部"
            on:click|stopPropagation={() => expandAll(true)}
          ><ChevronsDown size={21} /></button>
        {/if}

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
      {/if}

      <button
        type="button"
        title="列表菜单"
        on:mousedown|preventDefault|stopPropagation={openListMenu}
        on:click|stopPropagation
      ><MoreHorizontal size={23} /></button>
    </div>
  </section>

  {#if listMenuAt}
    <ListMenu
      x={listMenuAt.x}
      y={listMenuAt.y}
      xAlign="right"
      node={$selectedNode}
      {isScheduled}
      {sortMode}
      onSortMode={(mode) => (sortMode = mode)}
      onRenameRequest={beginHeaderRename}
      onClose={() => (listMenuAt = null)}
    />
  {/if}

  {#if isScheduled}
    <ScheduledTasksView bind:this={schedulerViewRef} />
  {:else}
  {#if isMyDay}
    <p class="my-day-subtitle">
      {formatMyDayDate(myDayViewDate)}
      {#if isMyDayHistory}
        <button type="button" class="my-day-back" on:click={() => (myDayViewDate = todayIso())}>返回今天</button>
      {/if}
    </p>
  {/if}

  <section class="task-list">
    {#each incompleteTasks as task (task.id)}
      <TaskCard
        {task}
        nodeId={task.nodeId}
        selected={taskMenu?.taskId === task.id}
        on:toggle={(event) => toggleCompletion(event.detail)}
        on:expand={(event) => toggleTaskExpansion(event.detail)}
        on:edit={(event) => openTaskEditor(event.detail)}
        on:context={openTaskMenu}
        on:openLink={openTaskLink}
        on:setDate={handleTaskSetDate}
        on:removeTag={(e) => removeTagFromTask(e.detail.id, e.detail.tagId)}
        on:editTag={(e) => editTagAtTask(e.detail.id, e.detail.tagId, e.detail.text)}
        on:removeEmoji={(e) => removeEmojiFromTask(e.detail.id, e.detail.index)}
        on:pickEmoji={(e) => openEmojiPickerAt(e.detail.id, e.detail.index)}
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
              selected={taskMenu?.taskId === task.id}
                    on:toggle={(event) => toggleCompletion(event.detail)}
              on:expand={(event) => toggleTaskExpansion(event.detail)}
              on:edit={(event) => openTaskEditor(event.detail)}
              on:context={openTaskMenu}
              on:openLink={openTaskLink}
              on:setDate={handleTaskSetDate}
              on:removeTag={(e) => removeTagFromTask(e.detail.id, e.detail.tagId)}
              on:editTag={(e) => editTagAtTask(e.detail.id, e.detail.tagId, e.detail.text)}
              on:removeEmoji={(e) => removeEmojiFromTask(e.detail.id, e.detail.index)}
              on:pickEmoji={(e) => openEmojiPickerAt(e.detail.id, e.detail.index)}
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
    <ContextMenu x={taskMenu.x} y={taskMenu.y} minWidth={236} onClose={() => (taskMenu = null)}>
      <MenuItem
        icon={Sun}
        label={taskMenuTask.myDay ? "从我的一天中移除" : "添加到我的一天"}
        onSelect={() => { void updateTaskAction(taskMenuTask.id, { myDay: !taskMenuTask.myDay }); taskMenu = null; }}
      />
      <MenuItem icon={CalendarDays} label="添加日期">
        <div slot="submenu" class="task-menu-date">
          <DatePicker
            value={taskMenuTask.dueDate?.slice(0, 10) ?? ""}
            on:select={(event) => setTaskDate(taskMenuTask.id, event.detail)}
            on:clear={() => setTaskDate(taskMenuTask.id, "")}
          />
        </div>
      </MenuItem>
      <MenuItem
        icon={Star}
        label={taskMenuTask.important ? "取消收藏" : "收藏"}
        onSelect={() => { void updateTaskAction(taskMenuTask.id, { important: !taskMenuTask.important }); taskMenu = null; }}
      />
      <MenuItem icon={Tag} label="标签">
        <div slot="submenu" class="tag-editor-panel" on:click|stopPropagation={() => { editingTagIdInMenu = ""; }}>
          {#if taskMenuTask.tags.length > 0}
            {#each taskMenuTask.tags as tag (tag.id)}
              {#if editingTagIdInMenu === tag.id}
                <div class="tag-editor-input-row" on:click|stopPropagation>
                  <input
                    type="text"
                    maxlength="20"
                    value={editingTagTextInMenu}
                    on:input={(e) => editingTagTextInMenu = e.currentTarget.value}
                    on:keydown|stopPropagation={(e) => { if (e.key === "Enter" && !e.isComposing && e.keyCode !== 229) submitTagEditInMenu(); }}
                    on:blur={submitTagEditInMenu}
                  />
                  <button class="tag-add-btn" type="button" on:click|stopPropagation={submitTagEditInMenu}>
                    <Plus size={15} />
                  </button>
                </div>
              {:else}
                <div
                  class={`tag-list-item bg-${tag.color}`}
                  on:click|stopPropagation={() => { editingTagIdInMenu = tag.id; editingTagTextInMenu = tag.text || ""; }}
                >
                  <span class="tag-list-text">{tag.text || "(无文字)"}</span>
                  <button class="tag-list-delete" type="button" title="删除此标签" on:click|stopPropagation={() => removeTagFromTask(taskMenuTask.id, tag.id)}>
                    <Trash2 size={14} />
                  </button>
                </div>
              {/if}
            {/each}
          {/if}
          <div class="tag-editor-input-row">
            <input
              type="text"
              placeholder="输入标签文字..."
              maxlength="20"
              value={tagInputText}
              on:input={(e) => tagInputText = e.currentTarget.value}
              on:keydown|stopPropagation={(e) => { if (e.key === "Enter" && !e.isComposing && e.keyCode !== 229) submitTagInput(); }}
            />
            <button class="tag-add-btn" type="button" title="添加标签" on:click|stopPropagation={submitTagInput}>
              <Plus size={15} />
            </button>
          </div>
          <div class="tag-editor-colors">
            {#each [["red", "红色"], ["yellow", "黄色"], ["blue", "蓝色"], ["green", "绿色"], ["gray", "灰色"]] as [color, label]}
              <button
                class={`color-circle ${color}`}
                class:selected={selectedTagColor === color}
                title={label}
                on:click|stopPropagation={() => selectedTagColor = color as TagColor}
              ></button>
            {/each}
          </div>
          {#if taskMenuTask.tags.length > 0}
            <button class="menu-item menu-item-button danger tag-clear-all" on:click|stopPropagation={() => clearTagsFromTask(taskMenuTask.id)}>
              <Trash2 size={14} /> 清除所有标签
            </button>
          {/if}
        </div>
      </MenuItem>
      <MenuItem icon={SmilePlus} label="添加表情" onSelect={() => openEmojiPickerForTask(taskMenuTask.id)} />
      <MenuItem icon={FolderInput} label="移动到">
        <div slot="submenu" class="submenu-list">
          <MoveTargetTree
            nodes={$appState.nodes}
            currentEntryId={taskMenuTask.nodeId}
            on:move={(event) => moveTaskToNode(taskMenuTask.id, event.detail)}
          />
          {#if !hasTaskMoveTargets}
            <div class="menu-empty">没有可移动的目标</div>
          {/if}
        </div>
      </MenuItem>
      <MenuSeparator />
      <MenuItem icon={PenLine} label="编辑" onSelect={() => openTaskEditor(taskMenuTask.id)} />
      <MenuItem icon={Trash2} danger label="删除" onSelect={() => deleteTask(taskMenuTask.id)} />
    </ContextMenu>
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
  {/if}

  {#if linkPreviewUrl}
    <div class="link-preview-overlay">
      <div class="link-preview-close-zone">
        <button class="link-preview-close" type="button" title="关闭预览" on:click={closeLinkPreview}>
          <X size={22} strokeWidth={2.5} />
        </button>
      </div>
      <iframe class="link-preview-frame" src={linkPreviewUrl} title="链接预览" sandbox="allow-scripts allow-same-origin allow-forms allow-popups"></iframe>
    </div>
  {/if}

</main>

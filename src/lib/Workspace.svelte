<script lang="ts">
  import { tick } from "svelte";
  import {
    CalendarDays, ChevronDown, Download, Eraser, FolderInput, Image,
    MoreHorizontal, Pencil, Plus, Search, Star, Sun, Trash2, Upload
  } from "@lucide/svelte";
  import {
    appState, appSettings, commit, commitSettings, showToast,
    searchQuery, selectedNode, visibleTasks, selectedBackground,
    accent, isSearching, now, todayIso, createTaskId, safeFileName,
    fileToDataUrl, APP_VERSION
  } from "./stores";
  import { moveTargetOptions, nodeAndDescendantIds, exportStateForNode, getBackground } from "./nodes";
  import { buildMainStyle, buildMenuStyle, uiScaleValue } from "./styles";
  import { normalizeState, normalizeSettings, defaultBackground, themePresets } from "./defaults";
  import { exportData, openExternalUrl } from "./backend";
  import IconGlyph from "./IconGlyph.svelte";
  import TaskCard from "./TaskCard.svelte";
  import type { AppNode, AppState, ListBackground, Settings, Task } from "./types";

  let newTaskDraft = "";
  let selectedTaskId: string | null = null;
  let showCompleted = true;
  let showListMenu = false;
  let taskMenu: { taskId: string; x: number; y: number; showDate: boolean } | null = null;
  let backgroundLinkDraft = "";
  let backgroundDraftNodeId = "";
  let taskInput: HTMLTextAreaElement;
  let importInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;

  $: mainStyle = buildMainStyle($selectedBackground, $accent);
  $: incompleteTasks = $visibleTasks.filter((task) => !task.completed);
  $: completedTasks = $visibleTasks.filter((task) => task.completed);
  $: selectedMoveTargets = $selectedNode ? moveTargetOptions($selectedNode.id, $appState.nodes) : [];
  $: taskMenuStyle = taskMenu ? buildMenuStyle(taskMenu.x, taskMenu.y, 230, taskMenu.showDate ? 230 : 188, uiScaleValue($appSettings.appearance.uiScale)) : "";
  $: taskMenuTask = taskMenu ? $appState.tasks.find((task) => task.id === taskMenu?.taskId) : null;
  $: if (($selectedNode?.id ?? "") !== backgroundDraftNodeId) {
    backgroundDraftNodeId = $selectedNode?.id ?? "";
    backgroundLinkDraft = $selectedBackground.image ?? "";
  }

  export function closeOverlays(): void {
    showListMenu = false;
    taskMenu = null;
  }

  export function focusComposer(): void {
    taskInput?.focus();
  }

  function updateTask(taskId: string, updater: (task: Task) => Task): void {
    commit({
      ...$appState,
      tasks: $appState.tasks.map((task) => (task.id === taskId ? { ...updater(task), updatedAt: now() } : task))
    });
  }

  function addTaskFromDraft(): void {
    const markdown = newTaskDraft.trim();
    if (!markdown) return;
    const targetNode = $selectedNode?.kind === "entry" ? $selectedNode : $appState.nodes.find((n) => n.kind === "entry");
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
      plannedDate: $selectedNode?.id === "planned" ? todayIso() : undefined,
      dueDate: $selectedNode?.id === "planned" ? todayIso() : undefined,
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
    updateTask(taskId, (task) => ({ ...task, dueDate: date || undefined, plannedDate: date || undefined }));
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

  async function uploadBackgroundImage(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
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
    await exportData(payload, `todo-note-${APP_VERSION}-all.json`);
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
    taskMenu = null;
  }

  function handleListMenuKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    toggleListMenu();
  }

  function startRename(id: string): void {
    // Delegate to sidebar for rename — not implemented here as rename is sidebar-owned
    showListMenu = false;
  }

  function handleMoveTargetChange(nodeId: string, event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    const parentId = target.value || null;
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
      <span class="header-icon">
        {#if $isSearching}
          <Search size={34} />
        {:else}
          <IconGlyph icon={$selectedNode?.icon ?? "notebook"} size={34} />
        {/if}
      </span>
      <h1>{$isSearching ? `搜索结果：${$searchQuery}` : $selectedNode?.name ?? "随手记"}</h1>
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
          <button type="button" disabled={!$selectedNode || $selectedNode.kind === "system"} on:click={() => $selectedNode && startRename($selectedNode.id)}>
            <Pencil size={15} /> 重命名
          </button>
          {#if $selectedNode && $selectedNode.kind !== "system"}
            <label class="move-group-row">
              <span><FolderInput size={15} /> 移动到分组</span>
              <select value={$selectedNode.parentId ?? ""} on:change={(event) => handleMoveTargetChange($selectedNode.id, event)}>
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
            <input type="range" min="0" max="80" value={Math.round(($selectedBackground.imageOpacity ?? 0.28) * 100)} on:input={updateBackgroundOpacity} />
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
        linkOpenMode={$appSettings.appearance.linkOpenMode}
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
              linkOpenMode={$appSettings.appearance.linkOpenMode}
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

    {#if $visibleTasks.length === 0}
      <div class="empty-state">
        <strong>{$isSearching ? "没有搜索结果" : "这个条目还没有内容"}</strong>
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
    </div>
  </section>
</main>

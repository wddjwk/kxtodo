// ---------------------------------------------------------------------------
// actions：全部 GUI 写操作的业务命令层（§4.3）。
// 桌面：coreDispatch → Host Domain Core（权威），事件回刷。
// mobile/浏览器：legacy commit 路径（本地全量保存），行为与 v8 一致。
// ---------------------------------------------------------------------------

import { get } from "svelte/store";
import type { AppNode, AppState, ScheduledTask, SchedulerState, Settings, SyncMode, Tag, TagColor, Task } from "./types";
import {
  appState, appSettings, commit, commitScheduler, commitSettings,
  coreMode, createTaskId, editBaseUpdatedAt, markEditStart, clearEditBase, rebaseEditBase,
  refreshFromCore, scheduleEntries, syncConnection, showToast
} from "./stores";
import { coreDispatch, CoreCommandError } from "./backend";
import {
  createCategoryNode, createEntryNode, defaultBackground, createScheduledTask
} from "./defaults";
import { nodeAndDescendantIds } from "./nodes";
import { uiToPatch, uiToSpec, type ScheduleEntryV9 } from "./scheduleAdapter";

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function state(): AppState {
  return get(appState);
}

function settings(): Settings {
  return get(appSettings);
}

async function report(error: unknown, fallback: string): Promise<null> {
  if (error instanceof CoreCommandError) {
    showToast(`${fallback}：${error.message}${error.hint ? `（${error.hint}）` : ""}`);
  } else {
    showToast(`${fallback}：${String(error)}`);
  }
  return null;
}

function findTask(id: string): Task | undefined {
  return state().tasks.find((task) => task.id === id);
}

function findNode(id: string): AppNode | undefined {
  return state().nodes.find((node) => node.id === id);
}

// ---------------------------------------------------------------------------
// 节点选择 / 树状态
// ---------------------------------------------------------------------------

export async function selectNode(nodeId: string): Promise<void> {
  if (coreMode) {
    appState.update((s) => ({ ...s, selectedNodeId: nodeId }));
    try {
      await coreDispatch("gui.select-node", { nodeId });
    } catch {
      // 选择失败可忽略，下次刷新回正
    }
    return;
  }
  commit({ ...state(), selectedNodeId: nodeId });
}

export async function toggleCategory(nodeId: string, collapsed: boolean): Promise<void> {
  if (coreMode) {
    appState.update((s) => ({
      ...s,
      nodes: s.nodes.map((node) => (node.id === nodeId ? { ...node, collapsed } : node))
    }));
    try {
      await coreDispatch("gui.set-collapsed", { id: nodeId, collapsed });
    } catch (error) {
      await report(error, "折叠状态保存失败");
    }
    return;
  }
  commit({
    ...state(),
    nodes: state().nodes.map((node) => (node.id === nodeId ? { ...node, collapsed } : node))
  });
}

export async function setNodeIcon(nodeId: string, icon: string): Promise<void> {
  if (coreMode) {
    appState.update((s) => ({
      ...s,
      nodes: s.nodes.map((node) => (node.id === nodeId ? { ...node, icon } : node))
    }));
    try {
      await coreDispatch("task.modify", { type: findNode(nodeId)?.kind ?? "entry", id: nodeId, icon });
    } catch (error) {
      await report(error, "图标保存失败");
    }
    return;
  }
  commit({
    ...state(),
    nodes: state().nodes.map((node) => (node.id === nodeId ? { ...node, icon } : node))
  });
}

// ---------------------------------------------------------------------------
// 节点增删改移
// ---------------------------------------------------------------------------

export async function addNode(kind: "category" | "entry", name: string, parentId: string | null, icon?: string): Promise<AppNode | null> {
  if (coreMode) {
    try {
      const response = await coreDispatch<{ id: string }>("task.add", {
        type: kind,
        name,
        parentId: parentId ?? "root",
        icon
      });
      const node: AppNode = {
        id: response.data.id,
        kind,
        name,
        icon: icon ?? (kind === "category" ? "folder" : "notebook"),
        parentId,
        collapsed: false,
        createdAt: new Date().toISOString()
      };
      const next = { ...state(), nodes: [...state().nodes, node] };
      if (kind === "entry") {
        next.backgrounds = { ...next.backgrounds, [node.id]: { ...defaultBackground } };
        next.selectedNodeId = node.id;
        void coreDispatch("gui.select-node", { nodeId: node.id }).catch(() => undefined);
      }
      appState.set(next);
      return node;
    } catch (error) {
      return report(error, "新建失败");
    }
  }
  const node = kind === "category" ? createCategoryNode(name, parentId) : createEntryNode(name, parentId, icon ?? "notebook");
  const next = { ...state(), nodes: [...state().nodes, node] };
  if (kind === "entry") {
    next.backgrounds = { ...next.backgrounds, [node.id]: { ...defaultBackground } };
    next.selectedNodeId = node.id;
  }
  commit(next);
  return node;
}

export async function renameNode(nodeId: string, name: string): Promise<void> {
  const trimmed = name.trim();
  if (!trimmed) return;
  const node = findNode(nodeId);
  if (!node || node.kind === "system") return;
  if (coreMode) {
    appState.update((s) => ({
      ...s,
      nodes: s.nodes.map((item) => (item.id === nodeId ? { ...item, name: trimmed } : item))
    }));
    try {
      await coreDispatch("task.modify", { type: node.kind, id: nodeId, name: trimmed });
    } catch (error) {
      await report(error, "重命名失败");
    }
    return;
  }
  commit({
    ...state(),
    nodes: state().nodes.map((item) => (item.id === nodeId ? { ...item, name: trimmed } : item))
  });
}

export async function deleteNodeCascade(nodeId: string): Promise<void> {
  const node = findNode(nodeId);
  if (!node || node.kind === "system") return;
  if (coreMode) {
    try {
      await coreDispatch("task.remove", { type: node.kind, id: nodeId, cascade: true });
    } catch (error) {
      await report(error, "删除失败");
      return;
    }
    const removedIds = nodeAndDescendantIds(nodeId, state().nodes);
    const remainingNodes = state().nodes.filter((item) => !removedIds.has(item.id));
    const fallback = remainingNodes.find((item) => item.kind === "entry") ?? remainingNodes[0];
    appState.set({
      ...state(),
      nodes: remainingNodes,
      tasks: state().tasks.filter((task) => !removedIds.has(task.nodeId)),
      selectedNodeId: removedIds.has(state().selectedNodeId)
        ? fallback?.id ?? state().selectedNodeId
        : state().selectedNodeId
    });
    return;
  }
  const removedIds = nodeAndDescendantIds(nodeId, state().nodes);
  let remainingNodes = state().nodes.filter((item) => !removedIds.has(item.id));
  if (!remainingNodes.some((item) => item.kind === "entry")) {
    remainingNodes = [...remainingNodes, createEntryNode("收集箱")];
  }
  const backgrounds = { ...state().backgrounds };
  for (const id of removedIds) {
    delete backgrounds[id];
  }
  const fallback = remainingNodes.find((item) => item.kind === "entry") ?? remainingNodes[0];
  commit({
    ...state(),
    nodes: remainingNodes,
    tasks: state().tasks.filter((task) => !removedIds.has(task.nodeId)),
    backgrounds,
    selectedNodeId: removedIds.has(state().selectedNodeId)
      ? fallback?.id ?? state().selectedNodeId
      : state().selectedNodeId
  });
}

/** 拖拽/移动分组：nodes 已是目标顺序，parentChanges 记录 parentId 变化。 */
export async function applyTreeOrder(orderedNodes: AppNode[], parentChanges: Record<string, string | null>): Promise<void> {
  if (coreMode) {
    appState.update((s) => ({ ...s, nodes: orderedNodes }));
    try {
      await coreDispatch("gui.apply-tree-order", {
        orderedIds: orderedNodes.map((node) => node.id),
        parentChanges
      });
    } catch (error) {
      await report(error, "移动失败");
    }
    return;
  }
  commit({ ...state(), nodes: orderedNodes });
}

// ---------------------------------------------------------------------------
// 任务（item）
// ---------------------------------------------------------------------------

export type TaskDraft = {
  markdown: string;
  completed?: boolean;
  important?: boolean;
  myDay?: boolean;
  plannedDate?: string;
  dueDate?: string;
  tags?: Tag[];
  emojis?: string[];
};

export async function addTask(entryId: string, draft: TaskDraft): Promise<Task | null> {
  if (coreMode) {
    try {
      const response = await coreDispatch<{ id: string; createdAt: string }>("task.add", {
        type: "item",
        entryId,
        markdown: draft.markdown,
        completed: draft.completed ?? false,
        important: draft.important ?? false,
        myDay: draft.myDay ?? false,
        plannedDate: draft.plannedDate,
        dueDate: draft.dueDate,
        tags: (draft.tags ?? []).map((tag) => `${tag.color}:${tag.text ?? ""}`),
        emojis: draft.emojis ?? []
      });
      const task: Task = {
        id: response.data.id,
        nodeId: entryId,
        markdown: draft.markdown,
        completed: draft.completed ?? false,
        important: draft.important ?? false,
        myDay: draft.myDay ?? false,
        plannedDate: draft.plannedDate,
        dueDate: draft.dueDate,
        completedAt: draft.completed ? new Date().toISOString() : undefined,
        tags: draft.tags ?? [],
        emojis: draft.emojis ?? [],
        expanded: false,
        createdAt: response.data.createdAt ?? new Date().toISOString(),
        updatedAt: response.data.createdAt ?? new Date().toISOString()
      };
      appState.update((s) => ({ ...s, tasks: [...s.tasks, task] }));
      return task;
    } catch (error) {
      return report(error, "新建任务失败");
    }
  }
  const task: Task = {
    id: createTaskId(),
    nodeId: entryId,
    markdown: draft.markdown,
    completed: draft.completed ?? false,
    important: draft.important ?? false,
    myDay: draft.myDay ?? false,
    plannedDate: draft.plannedDate,
    dueDate: draft.dueDate,
    completedAt: draft.completed ? new Date().toISOString() : undefined,
    tags: draft.tags ?? [],
    emojis: draft.emojis ?? [],
    expanded: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  };
  commit({ ...state(), tasks: [...state().tasks, task] });
  return task;
}

export type TaskChanges = {
  completed?: boolean;
  important?: boolean;
  myDay?: boolean;
  markdown?: string;
  entryId?: string;
  plannedDate?: string | null;
  dueDate?: string | null;
};

function legacyUpdateTask(id: string, updater: (task: Task) => Task): void {
  commit({
    ...state(),
    tasks: state().tasks.map((task) => (task.id === id ? updater(task) : task))
  });
}

export async function updateTask(id: string, changes: TaskChanges): Promise<void> {
  const previous = findTask(id);
  if (!previous) return;
  if (coreMode) {
    const params: Record<string, unknown> = { type: "item", id };
    if (changes.completed !== undefined) params.completed = changes.completed;
    if (changes.important !== undefined) params.important = changes.important;
    if (changes.myDay !== undefined) params.myDay = changes.myDay;
    if (changes.markdown !== undefined) params.markdown = changes.markdown;
    if (changes.entryId !== undefined) params.entryId = changes.entryId;
    if (changes.plannedDate !== undefined) {
      if (changes.plannedDate === null) params.clearPlannedDate = true;
      else params.plannedDate = changes.plannedDate;
    }
    if (changes.dueDate !== undefined) {
      if (changes.dueDate === null) params.clearDueDate = true;
      else params.dueDate = changes.dueDate;
    }
    try {
      await coreDispatch("task.modify", params);
    } catch (error) {
      await report(error, "保存失败");
      return;
    }
    legacyLocalTaskPatch(id, changes);
    return;
  }
  legacyUpdateTask(id, (task) => {
    const next = { ...task, updatedAt: new Date().toISOString() };
    if (changes.completed !== undefined) {
      next.completed = changes.completed;
      next.completedAt = changes.completed ? new Date().toISOString() : undefined;
    }
    if (changes.important !== undefined) next.important = changes.important;
    if (changes.myDay !== undefined) next.myDay = changes.myDay;
    if (changes.markdown !== undefined) next.markdown = changes.markdown;
    if (changes.entryId !== undefined) next.nodeId = changes.entryId;
    if (changes.plannedDate !== undefined) next.plannedDate = changes.plannedDate ?? undefined;
    if (changes.dueDate !== undefined) next.dueDate = changes.dueDate ?? undefined;
    return next;
  });
}

/** core 路径下的本地即时补丁（不等事件回刷，保持交互跟手）。 */
function legacyLocalTaskPatch(id: string, changes: TaskChanges): void {
  appState.update((s) => ({
    ...s,
    tasks: s.tasks.map((task) => {
      if (task.id !== id) return task;
      const next = { ...task, updatedAt: new Date().toISOString() };
      if (changes.completed !== undefined) {
        next.completed = changes.completed;
        next.completedAt = changes.completed ? new Date().toISOString() : undefined;
      }
      if (changes.important !== undefined) next.important = changes.important;
      if (changes.myDay !== undefined) next.myDay = changes.myDay;
      if (changes.markdown !== undefined) next.markdown = changes.markdown;
      if (changes.entryId !== undefined) next.nodeId = changes.entryId;
      if (changes.plannedDate !== undefined) next.plannedDate = changes.plannedDate ?? undefined;
      if (changes.dueDate !== undefined) next.dueDate = changes.dueDate ?? undefined;
      return next;
    })
  }));
}

export async function deleteTask(id: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("task.remove", { type: "item", id });
    } catch (error) {
      await report(error, "删除失败");
      return;
    }
    appState.update((s) => ({ ...s, tasks: s.tasks.filter((task) => task.id !== id) }));
    return;
  }
  commit({ ...state(), tasks: state().tasks.filter((task) => task.id !== id) });
}

export async function replaceTaskTags(id: string, tags: Tag[]): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("task.modify", {
        type: "item",
        id,
        replaceTags: tags.map((tag) => `${tag.color}:${tag.text ?? ""}`)
      });
    } catch (error) {
      await report(error, "标签保存失败");
      return;
    }
    appState.update((s) => ({
      ...s,
      tasks: s.tasks.map((task) => (task.id === id ? { ...task, tags } : task))
    }));
    return;
  }
  legacyUpdateTask(id, (task) => ({ ...task, tags, updatedAt: new Date().toISOString() }));
}

export async function replaceTaskEmojis(id: string, emojis: string[]): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("task.modify", { type: "item", id, replaceEmojis: emojis });
    } catch (error) {
      await report(error, "表情保存失败");
      return;
    }
    appState.update((s) => ({
      ...s,
      tasks: s.tasks.map((task) => (task.id === id ? { ...task, emojis } : task))
    }));
    return;
  }
  legacyUpdateTask(id, (task) => ({ ...task, emojis, updatedAt: new Date().toISOString() }));
}

export async function setItemUi(id: string, ui: { expanded?: boolean }): Promise<void> {
  appState.update((s) => ({
    ...s,
    tasks: s.tasks.map((task) => (task.id === id ? { ...task, ...ui } : task))
  }));
  if (coreMode) {
    try {
      await coreDispatch("gui.set-item-ui", { id, ...ui });
    } catch {
      // UI 态写入失败可忽略
    }
    return;
  }
  commit(state());
}

export async function setItemsUi(ids: string[], expanded: boolean): Promise<void> {
  appState.update((s) => ({
    ...s,
    tasks: s.tasks.map((task) => (ids.includes(task.id) ? { ...task, expanded } : task))
  }));
  if (coreMode) {
    try {
      await coreDispatch("gui.set-items-ui", { ids, expanded });
    } catch {
      // 忽略
    }
    return;
  }
  commit(state());
}

/** 编辑保存：携带编辑基准 updatedAt；冲突时重置基准，再次保存将覆盖外部更改（§4.3）。 */
export async function saveTaskMarkdown(id: string, markdown: string, expanded: boolean): Promise<boolean> {
  if (coreMode) {
    try {
      await coreDispatch("task.modify", {
        type: "item",
        id,
        markdown,
        expectedUpdatedAt: editBaseUpdatedAt(id)
      });
    } catch (error) {
      if (error instanceof CoreCommandError && error.code === "ITEM_CONFLICT") {
        rebaseEditBase(id, findTask(id)?.updatedAt);
        showToast("内容已被外部修改，再次保存将覆盖外部更改", 4200);
      } else {
        await report(error, "保存失败");
      }
      return false;
    }
    clearEditBase(id);
    appState.update((s) => ({
      ...s,
      tasks: s.tasks.map((task) =>
        task.id === id
          ? { ...task, markdown, expanded, updatedAt: new Date().toISOString() }
          : task
      )
    }));
    try {
      await coreDispatch("gui.set-item-ui", { id, expanded });
    } catch {
      // 忽略
    }
    return true;
  }
  legacyUpdateTask(id, (task) => ({
    ...task,
    markdown,
    expanded,
    updatedAt: new Date().toISOString()
  }));
  return true;
}

// ---------------------------------------------------------------------------
// 背景与主题
// ---------------------------------------------------------------------------

export async function setBackground(
  nodeId: string,
  background: { color?: string; image?: string | null; imageOpacity?: number }
): Promise<void> {
  appState.update((s) => ({
    ...s,
    backgrounds: {
      ...s.backgrounds,
      [nodeId]: {
        color: background.color ?? s.backgrounds[nodeId]?.color ?? defaultBackground.color,
        image: background.image === null ? undefined : background.image ?? s.backgrounds[nodeId]?.image,
        imageOpacity: background.imageOpacity ?? s.backgrounds[nodeId]?.imageOpacity ?? defaultBackground.imageOpacity
      }
    }
  }));
  if (coreMode) {
    try {
      await coreDispatch("gui.set-background", { nodeId, ...background });
    } catch (error) {
      await report(error, "背景保存失败");
    }
    return;
  }
  commit(state());
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

export async function setConfig(path: string, value: unknown): Promise<boolean> {
  if (coreMode) {
    try {
      await coreDispatch("config.set", { path, value });
    } catch (error) {
      await report(error, "设置保存失败");
      return false;
    }
    // 本地即时生效（事件会兜底一致性）
    appSettings.update((current) => {
      const next = clone(current);
      applySettingsPath(next, path, value);
      return next;
    });
    return true;
  }
  const next = clone(settings());
  applySettingsPath(next, path, value);
  commitSettings(next);
  return true;
}

export async function unsetUiColor(nodeId: string): Promise<boolean> {
  if (coreMode) {
    try {
      await coreDispatch("config.unset", { path: "appearance.uiColors", mapKey: nodeId });
    } catch (error) {
      await report(error, "恢复默认色失败");
      return false;
    }
    appSettings.update((current) => {
      const next = clone(current);
      delete next.appearance.uiColors[nodeId];
      return next;
    });
    return true;
  }
  const next = clone(settings());
  delete next.appearance.uiColors[nodeId];
  commitSettings(next);
  return true;
}

function applySettingsPath(target: Settings, path: string, value: unknown): void {
  const segments = path.split(".");
  let cursor: Record<string, unknown> = target as unknown as Record<string, unknown>;
  for (let index = 0; index < segments.length - 1; index++) {
    const next = cursor[segments[index]];
    if (typeof next !== "object" || next === null) {
      return;
    }
    cursor = next as Record<string, unknown>;
  }
  const leaf = segments[segments.length - 1];
  if (leaf === "uiColors" && typeof value === "object" && value !== null && "__key" in (value as Record<string, unknown>)) {
    const payload = value as { __key: string; __value: unknown };
    (cursor[leaf] as Record<string, unknown>)[payload.__key] = payload.__value;
    return;
  }
  cursor[leaf] = value;
}

export async function setUiColor(nodeId: string, color: string): Promise<boolean> {
  if (coreMode) {
    try {
      await coreDispatch("config.set", { path: "appearance.uiColors", value: color, mapKey: nodeId });
    } catch (error) {
      await report(error, "自定义颜色保存失败");
      return false;
    }
    appSettings.update((current) => {
      const next = clone(current);
      next.appearance.uiColors[nodeId] = color;
      return next;
    });
    return true;
  }
  const next = clone(settings());
  next.appearance.uiColors[nodeId] = color;
  commitSettings(next);
  return true;
}

// ---------------------------------------------------------------------------
// 定时任务
// ---------------------------------------------------------------------------

function scheduleState(): SchedulerState {
  return state().scheduler;
}

function legacyCommitSchedulerTasks(tasks: ScheduledTask[]): void {
  commitScheduler({ ...scheduleState(), tasks });
}

export async function addSchedule(): Promise<ScheduledTask | null> {
  const ui = createScheduledTask(`定时任务 ${scheduleState().tasks.length + 1}`);
  if (coreMode) {
    try {
      const response = await coreDispatch<ScheduleEntryV9>("schedule.add", {
        spec: uiToSpec(ui)
      });
      const entry = response.data;
      scheduleEntries.update((map) => new Map(map).set(entry.id, entry));
      // 新建的定时任务停留在配置界面（持久化 UI 态，快照刷新不会冲掉）
      try {
        await coreDispatch("gui.set-schedule-ui", { id: entry.id, editing: true, expanded: true });
      } catch {
        // 忽略
      }
      const created: ScheduledTask = { ...ui, id: entry.id, editing: true, expanded: true };
      appState.update((s) => ({
        ...s,
        scheduler: { ...s.scheduler, tasks: [...s.scheduler.tasks, created] }
      }));
      // schedule.add 的 domain-changed 回刷可能先于本地追加落地，显式再刷一次保证收敛到磁盘权威快照
      await refreshFromCore(["schedule"]);
      return created;
    } catch (error) {
      return report(error, "新建定时任务失败");
    }
  }
  legacyCommitSchedulerTasks([...scheduleState().tasks, ui]);
  return ui;
}

export async function modifySchedule(id: string, ui: ScheduledTask): Promise<void> {
  if (coreMode) {
    const entry = get(scheduleEntries).get(id);
    if (!entry) {
      showToast("保存失败：定时任务数据未同步，请刷新");
      return;
    }
    try {
      const response = await coreDispatch<ScheduleEntryV9>("schedule.modify", {
        id,
        patch: uiToPatch(ui, entry)
      });
      scheduleEntries.update((map) => new Map(map).set(id, response.data));
    } catch (error) {
      await report(error, "保存失败");
      return;
    }
    appState.update((s) => ({
      ...s,
      scheduler: {
        ...s.scheduler,
        tasks: s.scheduler.tasks.map((task) => (task.id === id ? { ...ui, updatedAt: new Date().toISOString() } : task))
      }
    }));
    return;
  }
  legacyCommitSchedulerTasks(
    scheduleState().tasks.map((task) => (task.id === id ? { ...ui, updatedAt: new Date().toISOString() } : task))
  );
}

export async function removeSchedule(id: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("schedule.remove", { id });
    } catch (error) {
      await report(error, "删除失败");
      return;
    }
    scheduleEntries.update((map) => {
      const next = new Map(map);
      next.delete(id);
      return next;
    });
    appState.update((s) => ({
      ...s,
      scheduler: { ...s.scheduler, tasks: s.scheduler.tasks.filter((task) => task.id !== id) }
    }));
    return;
  }
  legacyCommitSchedulerTasks(scheduleState().tasks.filter((task) => task.id !== id));
}

export async function setScheduleEnabled(id: string, enabled: boolean): Promise<void> {
  if (coreMode) {
    try {
      const response = await coreDispatch<ScheduleEntryV9>(enabled ? "schedule.enable" : "schedule.disable", { id });
      scheduleEntries.update((map) => new Map(map).set(id, response.data));
    } catch (error) {
      await report(error, enabled ? "启用失败" : "禁用失败");
      return;
    }
    appState.update((s) => ({
      ...s,
      scheduler: {
        ...s.scheduler,
        tasks: s.scheduler.tasks.map((task) => (task.id === id ? { ...task, enabled } : task))
      }
    }));
    return;
  }
  legacyCommitSchedulerTasks(
    scheduleState().tasks.map((task) => (task.id === id ? { ...task, enabled } : task))
  );
}

export async function stopSchedule(id: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("schedule.stop", { id });
    } catch (error) {
      await report(error, "停止失败");
    }
    return;
  }
}

export async function setScheduleUi(id: string, ui: { expanded?: boolean; editing?: boolean }): Promise<void> {
  appState.update((s) => ({
    ...s,
    scheduler: {
      ...s.scheduler,
      tasks: s.scheduler.tasks.map((task) => (task.id === id ? { ...task, ...ui } : task))
    }
  }));
  if (coreMode) {
    try {
      await coreDispatch("gui.set-schedule-ui", { id, ...ui });
    } catch {
      // 忽略
    }
    return;
  }
  commit(state());
}

export async function setRuntime(name: string, path: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("schedule.runtime.set", { name, path });
    } catch (error) {
      await report(error, "运行时保存失败");
      return;
    }
    appState.update((s) => ({
      ...s,
      scheduler: { ...s.scheduler, runtimes: { ...s.scheduler.runtimes, [name]: path } }
    }));
    return;
  }
  commitScheduler({ ...scheduleState(), runtimes: { ...scheduleState().runtimes, [name]: path } });
}

export async function clearScheduleOutput(id: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("gui.clear-schedule-output", { id });
    } catch (error) {
      await report(error, "清空输出失败");
    }
    return;
  }
  legacyCommitSchedulerTasks(
    scheduleState().tasks.map((task) =>
      task.id === id ? { ...task, lastExitCode: undefined, lastStdout: "", lastStderr: "" } : task
    )
  );
}

export async function detectRuntimes(): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("schedule.runtime.detect");
    } catch (error) {
      await report(error, "运行时探测失败");
    }
    return;
  }
  const { resolveExecutorPaths } = await import("./backend");
  const detected = await resolveExecutorPaths().catch(() => null);
  if (!detected) return;
  const runtimes = { ...scheduleState().runtimes };
  for (const key of Object.keys(runtimes) as Array<keyof typeof runtimes>) {
    if (!runtimes[key] && detected[key]) {
      runtimes[key] = detected[key];
    }
  }
  commitScheduler({ ...scheduleState(), runtimes });
}

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

export async function importState(
  imported: AppState,
  importedSettings: Settings | null
): Promise<boolean> {
  if (coreMode) {
    try {
      await coreDispatch("gui.import-state", {
        state: {
          nodes: imported.nodes,
          tasks: imported.tasks,
          backgrounds: imported.backgrounds,
          selectedNodeId: imported.selectedNodeId
        }
      });
      // 恢复导出的定时任务（v8 导出形状 → v9 spec）
      for (const task of imported.scheduler?.tasks ?? []) {
        try {
          await coreDispatch("schedule.add", { spec: uiToSpec(task) });
        } catch {
          // 单个任务无效不阻断整体导入
        }
      }
      if (importedSettings) {
        const flat = flattenSettings(importedSettings);
        for (const [path, value] of flat) {
          await coreDispatch("config.set", { path, value });
        }
      }
    } catch (error) {
      await report(error, "导入失败");
      return false;
    }
    const { refreshFromCore } = await import("./stores");
    await refreshFromCore();
    showToast("导入完成");
    return true;
  }
  commit(imported);
  commitScheduler(imported.scheduler);
  if (importedSettings) {
    commitSettings(importedSettings);
  }
  showToast("导入完成");
  return true;
}

// ---------------------------------------------------------------------------
// 数据同步（v0.4.0，端到端加密）
// ---------------------------------------------------------------------------

export type SyncPairInput = {
  /** 通信方式；不传就沿用本机已配置的方式 */
  mode?: SyncMode;
  /** 自建服务：服务器地址 */
  serverUrl?: string;
  /** 局域网：选定的主机名（本机作为主机时留空） */
  lanPeer?: string;
  username: string;
  secret: string;
  syncData?: boolean;
  syncSettings?: boolean;
  syncSchedules?: boolean;
};

/**
 * 统一「开始同步」：账户不存在就当场注册，存在就登录——用户不必自己区分。
 * 三种通信方式共用这一个入口，差别只在 mode 与地址字段。
 * 密码不符 → AUTH_FAILED；用户名撞车只在并发注册时出现 → ACCOUNT_EXISTS。
 */
export async function syncPair(input: SyncPairInput): Promise<boolean> {
  if (!coreMode) {
    showToast("浏览器预览不支持同步");
    return false;
  }
  let registered = false;
  try {
    const envelope = await coreDispatch<{ registered?: boolean }>("sync.pair", {
      mode: input.mode,
      serverUrl: input.serverUrl,
      lanPeer: input.lanPeer,
      username: input.username,
      secret: input.secret,
      syncData: input.syncData,
      syncSettings: input.syncSettings,
      syncSchedules: input.syncSchedules
    });
    registered = envelope.data?.registered === true;
  } catch (error) {
    await report(error, "开始同步失败");
    return false;
  }
  const { refreshFromCore } = await import("./stores");
  await refreshFromCore();
  showToast(registered ? "已创建账户并完成首次同步" : "已配对并完成首次同步");
  return true;
}

/** 本机内置服务器（「本机作为服务器」）的运行状况，来自 runtime/sync-host.json。 */
export type SyncHostStatus = {
  /** 设置里勾了「本机作为服务器」 */
  wanted: boolean;
  running: boolean;
  /** 实际端口（配置端口被占用时会自动上移，所以可能不等于 lanPort） */
  port: number;
  name: string;
  instanceId?: string;
  adminUrl?: string;
  adminUser?: string;
  /** 只有 --show-secrets / showSecrets 才带出来 */
  adminPassword?: string;
  startedAt?: string | null;
  lastError?: string | null;
};

export type SyncStatus = {
  paired: boolean;
  enabled: boolean;
  /** 已配对但被用户暂停（配置保留） */
  paused?: boolean;
  mode: SyncMode;
  modeLabel?: string;
  serverUrl: string;
  lanHost: boolean;
  lanPort: number;
  lanName: string;
  lanPeer: string;
  /** P2P 高级覆盖（空 = n0 免费公共服务） */
  p2pRelay?: string;
  p2pDirectory?: string;
  /** P2P 概览（纯本地读：目录只取进程内缓存） */
  p2p?: {
    running: boolean;
    selfId?: string;
    serving?: boolean;
    /** null = 目录缓存还没有，角色未知 */
    hubIsSelf?: boolean | null;
    hubId?: string;
    onlinePeers?: number;
    peers?: Array<{ id: string; name: string }>;
  };
  /** 本机内置服务器状态 */
  host?: SyncHostStatus;
  /** 上一轮真正连到的主机库身份 */
  serverInstanceId?: string;
  /** 局域网主机的地址缓存（身份是名字，ip:port 只是缓存） */
  lanEndpoint?: { name: string; host: string; port: number; instanceId?: string } | null;
  username: string;
  scopes: { data: boolean; settings: boolean; schedules: boolean };
  intervalSeconds: number;
  reconnectSeconds?: number;
  deviceId: string;
  lastSyncAt?: string;
  lastResult?: {
    pulled: number;
    applied: number;
    pushed: number;
    conflicts: number;
    imagesPulled?: number;
    imagesPushed?: number;
  } | null;
  /** null = 还没探测过 */
  online?: boolean | null;
  lastSeenAt?: string | null;
  lastError?: string | null;
};

export type SyncReport = {
  pulled: number;
  applied: number;
  pushed: number;
  conflicts: number;
  imagesPulled: number;
  imagesPushed: number;
  warnings?: string[];
};

export type DiscoveredServer = {
  /** 展示名 = 这台主机在局域网里的身份（选定主机认的是它，不是 ip） */
  name: string;
  host: string;
  port: number;
  url: string;
  verified: boolean;
  version?: string | null;
  /** 对方数据库身份：换了它就意味着要重新对账（core 自动处理） */
  instanceId?: string | null;
};

/**
 * sync.status 是纯本地读；连接状态由它带出来（后台循环/探测负责刷新缓存）。
 * `showSecrets` 才会带出内置主机的管理台密码（默认不打印任何凭据）。
 */
export async function syncStatus(showSecrets = false): Promise<SyncStatus | null> {
  if (!coreMode) return null;
  try {
    const envelope = await coreDispatch<SyncStatus>(
      "sync.status",
      showSecrets ? { showSecrets: true } : {}
    );
    return envelope.data;
  } catch {
    return null;
  }
}

/** 把状态里的连接结论同步到 store（面板的 🟢/🔴 只订阅这个 store）。 */
export async function refreshSyncConnection(): Promise<SyncStatus | null> {
  const status = await syncStatus();
  if (status) {
    syncConnection.set({
      online: status.online ?? null,
      lastSeenAt: status.lastSeenAt ?? null,
      lastError: status.lastError ?? null
    });
  }
  return status;
}

/** 后台短超时探测（/healthz + /me），刷新连接状态缓存；面板打开时调用，不阻塞 UI。 */
export async function syncProbe(): Promise<SyncStatus | null> {
  if (!coreMode) return null;
  syncConnection.update((state) => ({ ...state, checking: true }));
  try {
    await coreDispatch("sync.probe", {});
  } catch {
    // 未配对/网络失败都已在状态缓存里留痕，这里不打扰用户
  }
  const status = await refreshSyncConnection();
  syncConnection.update((state) => ({ ...state, checking: false }));
  return status;
}

/** 局域网自动发现：返回可点选的服务器列表（Name + ip:port）；null = 当前环境不支持。 */
export async function syncDiscover(timeoutMs = 2500): Promise<DiscoveredServer[] | null> {
  if (!coreMode) {
    showToast("浏览器预览不支持局域网发现");
    return null;
  }
  try {
    const envelope = await coreDispatch<{ servers: DiscoveredServer[] }>("sync.discover", {
      timeoutMs
    });
    return envelope.data.servers ?? [];
  } catch (error) {
    await report(error, "局域网发现失败");
    return [];
  }
}

/** P2P 设备列表与枢纽角色（目录解析，60s 缓存；非 P2P 模式 core 会报 SYNC_MODE_MISMATCH）。 */
export type SyncPeers = {
  selfId: string;
  hubId: string;
  hubIsSelf: boolean;
  serving: boolean;
  peers: Array<{
    id: string;
    name: string;
    publishedAt?: number;
    lastOk?: boolean | null;
    lastSeenAt?: string | null;
    lastError?: string | null;
  }>;
  count: number;
};

export async function syncPeers(): Promise<SyncPeers | null> {
  if (!coreMode) return null;
  try {
    const envelope = await coreDispatch<SyncPeers>("sync.peers", {});
    return envelope.data;
  } catch {
    return null;
  }
}

// 自动同步与手动同步共用一条命令；撞上「另一个同步正在进行」时短等待重试。
async function dispatchSyncNow(): Promise<SyncReport> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      const envelope = await coreDispatch<SyncReport>("sync.now", {});
      return envelope.data;
    } catch (error) {
      const busy = error instanceof CoreCommandError && error.code === "SYNC_IN_PROGRESS";
      if (!busy || attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 700));
    }
  }
}

/**
 * 执行一次同步。`silent` = 自动同步：不发通知（周期性同步一直弹通知很烦人），
 * 结果只在设置页「最近同步」里体现。返回是否成功（暂停也算「没跑」，返回 false）。
 */
export async function syncNow(options: { silent?: boolean } = {}): Promise<boolean> {
  if (!coreMode) return false;
  const silent = options.silent === true;
  let result: SyncReport;
  try {
    result = await dispatchSyncNow();
  } catch (error) {
    // 暂停不是故障：不动连接状态缓存（🟢/🔴 保持上次探测结论）
    if (error instanceof CoreCommandError && error.code === "SYNC_PAUSED") {
      if (!silent) showToast("同步已暂停，点「恢复同步」继续");
      return false;
    }
    if (!silent) await report(error, "同步失败");
    await refreshSyncConnection();
    return false;
  }
  if (!silent) {
    const { pulled, applied, pushed, conflicts, imagesPulled, imagesPushed } = result;
    const images = imagesPulled || imagesPushed ? `，图片 ↓${imagesPulled} ↑${imagesPushed}` : "";
    showToast(
      conflicts > 0
        ? `同步完成：拉取 ${pulled}，推送 ${pushed}，冲突 ${conflicts}（下次同步重试）${images}`
        : `同步完成：拉取 ${pulled}，应用 ${applied}，推送 ${pushed}${images}`
    );
  }
  // 确有变化才回刷快照（Host 也会发 domain-changed 事件，这里兜底）
  if (result.applied > 0 || result.imagesPulled > 0) {
    await refreshFromCore();
  }
  await refreshSyncConnection();
  return true;
}

export async function syncUnpair(): Promise<boolean> {
  if (!coreMode) return false;
  try {
    await coreDispatch("sync.unpair", {});
  } catch (error) {
    await report(error, "解除配对失败");
    return false;
  }
  await refreshFromCore();
  syncConnection.set({ online: null });
  showToast("已解除本机配对（服务器数据保留）");
  return true;
}

export async function setSyncScopes(scopes: {
  syncData?: boolean;
  syncSettings?: boolean;
  syncSchedules?: boolean;
  enabled?: boolean;
  intervalSeconds?: number;
  reconnectSeconds?: number;
  mode?: SyncMode;
  lanHost?: boolean;
  lanName?: string;
  lanPort?: number;
  lanPeer?: string;
}): Promise<boolean> {
  if (!coreMode) return false;
  try {
    await coreDispatch("sync.configure", scopes);
  } catch (error) {
    await report(error, "同步设置失败");
    return false;
  }
  await refreshFromCore();
  return true;
}

/** 切换通信方式（局域网 / 自建服务 / P2P）。已配好的地址与主机名都保留。 */
export async function setSyncMode(mode: SyncMode): Promise<boolean> {
  const ok = await setSyncScopes({ mode });
  if (ok && mode === "p2p") {
    showToast("P2P 同步会在后续版本提供，当前先用局域网或自建服务");
  }
  return ok;
}

/**
 * 勾选/取消「本机作为服务器」。
 *
 * 这里只写设置：真正的启停由常驻进程做（core 的 reconcile 看到 lanHost 变了，
 * 就让宿主在自己进程内起/停 lib 化的 kxtodo-server）。名字是这台主机在局域网里的
 * 身份，重名会被 core 拒绝并给出错误。
 */
export async function setSyncLanHost(enabled: boolean, name?: string): Promise<boolean> {
  if (!coreMode) return false;
  const trimmed = name?.trim();
  const ok = await setSyncScopes({
    lanHost: enabled,
    ...(trimmed ? { lanName: trimmed } : {})
  });
  if (!ok) return false;
  showToast(
    enabled
      ? "已启用「本机作为服务器」：局域网内其它设备现在可以发现它"
      : "已停止本机服务器（自己仍可作为客户端连别的主机）"
  );
  return true;
}

/** 改本机作为服务器时的展示名（= 局域网身份，要求唯一）。 */
export async function setSyncLanName(name: string): Promise<boolean> {
  const trimmed = name.trim();
  if (!trimmed) {
    showToast("主机名字不能为空");
    return false;
  }
  return setSyncScopes({ lanName: trimmed });
}

/** 选定一台发现到的主机（认名字，不认 ip:port——路由器换了 IP 也照样连得上）。 */
export async function setSyncLanPeer(name: string): Promise<boolean> {
  const ok = await setSyncScopes({ lanPeer: name.trim() });
  if (ok) showToast(`已选定主机「${name.trim()}」`);
  return ok;
}

/** 暂停/恢复同步：只切开关，服务器地址与账户凭据全部保留。 */
export async function setSyncEnabled(enabled: boolean): Promise<boolean> {
  const ok = await setSyncScopes({ enabled });
  if (ok) showToast(enabled ? "已恢复同步" : "已暂停同步（配置保留）");
  return ok;
}

export type SyncHistoryEntry = {
  /** 通信方式；空 = v0.5.1 写下的旧记录，那会儿只有自建服务 */
  mode?: string;
  serverUrl: string;
  /** 局域网模式下选定的主机名 */
  lanPeer?: string;
  username: string;
  secret: string;
  usedAt?: string;
};

/** 这条历史指向哪儿（浮层里的主标题）：自建服务显示地址，局域网显示主机名。 */
export function syncHistoryLabel(entry: SyncHistoryEntry): string {
  if (entry.mode === "lan") {
    return entry.lanPeer?.trim() ? `局域网主机「${entry.lanPeer.trim()}」` : "本机（局域网主机）";
  }
  if (entry.mode === "p2p") return "P2P";
  return entry.serverUrl || "自建服务";
}

/** 本机配对历史（服务器地址 + 用户名 + 密码），设置页「历史」一键回填。 */
export async function syncHistory(): Promise<SyncHistoryEntry[] | null> {
  if (!coreMode) return null;
  try {
    const envelope = await coreDispatch<{ entries: SyncHistoryEntry[] }>("sync.history", {});
    return envelope.data.entries ?? [];
  } catch (error) {
    await report(error, "读取配对历史失败");
    return [];
  }
}

export async function syncHistoryRemove(index: number): Promise<SyncHistoryEntry[] | null> {
  if (!coreMode) return null;
  try {
    const envelope = await coreDispatch<{ entries: SyncHistoryEntry[] }>("sync.historyRemove", {
      index
    });
    return envelope.data.entries ?? [];
  } catch (error) {
    await report(error, "删除配对历史失败");
    return [];
  }
}

function flattenSettings(source: Settings): Array<[string, unknown]> {
  const out: Array<[string, unknown]> = [];
  const walk = (prefix: string, value: unknown) => {
    if (Array.isArray(value) || typeof value !== "object" || value === null) {
      out.push([prefix, value]);
      return;
    }
    for (const [key, child] of Object.entries(value)) {
      walk(prefix ? `${prefix}.${key}` : key, child);
    }
  };
  walk("", source);
  return out.filter(([path]) => !path.startsWith("appearance.uiColors."));
}

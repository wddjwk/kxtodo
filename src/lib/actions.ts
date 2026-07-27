// ---------------------------------------------------------------------------
// actions：全部 GUI 写操作的业务命令层（§4.3）。
// 桌面：coreDispatch → Host Domain Core（权威），事件回刷。
// mobile/浏览器：legacy commit 路径（本地全量保存），行为与 v8 一致。
// ---------------------------------------------------------------------------

import { get } from "svelte/store";
import type { AppNode, AppState, ScheduledTask, SchedulerState, Settings, Tag, TagColor, Task } from "./types";
import {
  appState, appSettings, commit, commitScheduler, commitSettings,
  coreMode, createTaskId, editBaseUpdatedAt, markEditStart, clearEditBase,
  scheduleEntries, showToast
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
      await coreDispatch("task.remove", { type: node.kind, id: nodeId, cascade: true, yes: true });
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
        editing: false,
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
    editing: false,
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
      await coreDispatch("task.remove", { type: "item", id, yes: true });
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

export async function setItemUi(id: string, ui: { expanded?: boolean; editing?: boolean }): Promise<void> {
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
    tasks: s.tasks.map((task) => (ids.includes(task.id) ? { ...task, expanded, editing: false } : task))
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

/** 编辑保存：携带编辑基准 updatedAt，冲突时保持草稿并提示（§4.3）。 */
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
        showToast("外部版本已变化，本次未保存", 4200);
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
          ? { ...task, markdown, editing: false, expanded, updatedAt: new Date().toISOString() }
          : task
      )
    }));
    try {
      await coreDispatch("gui.set-item-ui", { id, editing: false, expanded });
    } catch {
      // 忽略
    }
    return true;
  }
  legacyUpdateTask(id, (task) => ({
    ...task,
    markdown,
    editing: false,
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
      const created: ScheduledTask = { ...ui, id: entry.id };
      appState.update((s) => ({
        ...s,
        scheduler: { ...s.scheduler, tasks: [...s.scheduler.tasks, created] }
      }));
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
      await coreDispatch("schedule.remove", { id, yes: true });
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
      const response = await coreDispatch<ScheduleEntryV9>(enabled ? "schedule.enable" : "schedule.disable", { id, yes: true });
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

export async function runSchedule(id: string): Promise<void> {
  if (coreMode) {
    try {
      await coreDispatch("schedule.run", { id, yes: true });
      showToast("已加入执行队列");
    } catch (error) {
      await report(error, "执行失败");
    }
    return;
  }
  showToast("当前环境不支持执行");
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

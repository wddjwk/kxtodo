import type { AppNode, AppState, ListBackground, Task } from "./types";
import { defaultBackground, emptySchedulerState } from "./defaults";

export function descendantEntryIds(rootId: string, nodes: AppNode[]): Set<string> {
  const ids = new Set<string>();
  const visit = (parentId: string): void => {
    for (const node of nodes.filter((item) => item.parentId === parentId)) {
      if (node.kind === "entry") ids.add(node.id);
      if (node.kind === "category") visit(node.id);
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

export function nodeAndDescendantIds(rootId: string, nodes: AppNode[]): Set<string> {
  const ids = new Set<string>([rootId]);
  const visit = (parentId: string): void => {
    for (const node of nodes.filter((item) => item.parentId === parentId)) {
      ids.add(node.id);
      if (node.kind === "category") visit(node.id);
    }
  };
  visit(rootId);
  return ids;
}

export function ancestorIds(nodeId: string, nodes: AppNode[]): Set<string> {
  const ids = new Set<string>();
  let current = nodes.find((node) => node.id === nodeId);
  while (current?.parentId) {
    ids.add(current.parentId);
    current = nodes.find((node) => node.id === current?.parentId);
  }
  return ids;
}

export function tasksForNode(node: AppNode, tasks: Task[], nodes: AppNode[]): Task[] {
  if (node.id === "my-day") return tasks.filter((task) => task.myDay);
  if (node.id === "planned") return tasks.filter((task) => Boolean(task.dueDate || task.plannedDate));
  if (node.id === "important") return tasks.filter((task) => task.important);
  if (node.id === "scheduled") return [];
  if (node.kind === "entry") return tasks.filter((task) => task.nodeId === node.id);
  if (node.kind === "category") {
    const ids = descendantEntryIds(node.id, nodes);
    return tasks.filter((task) => ids.has(task.nodeId));
  }
  return [];
}

export function buildListCounts(state: AppState): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const node of state.nodes) {
    if (node.id === "my-day") {
      counts[node.id] = state.tasks.filter((task) => !task.completed && task.myDay).length;
    } else if (node.id === "planned") {
      counts[node.id] = state.tasks.filter((task) => !task.completed && (task.dueDate || task.plannedDate)).length;
    } else if (node.id === "important") {
      counts[node.id] = state.tasks.filter((task) => !task.completed && task.important).length;
    } else if (node.id === "scheduled") {
      counts[node.id] = state.scheduler.tasks.length;
    } else if (node.kind === "entry") {
      counts[node.id] = state.tasks.filter((task) => !task.completed && task.nodeId === node.id).length;
    } else if (node.kind === "category") {
      const ids = descendantEntryIds(node.id, state.nodes);
      counts[node.id] = state.tasks.filter((task) => !task.completed && ids.has(task.nodeId)).length;
    }
  }
  return counts;
}

export function buildVisibleTasks(state: AppState, node: AppNode | undefined, queryValue: string): Task[] {
  const query = queryValue.trim().toLowerCase();
  if (query) {
    return state.tasks.filter((task) => {
      const taskNode = state.nodes.find((item) => item.id === task.nodeId);
      return task.markdown.toLowerCase().includes(query) || taskNode?.name.toLowerCase().includes(query);
    });
  }
  if (!node) return [];
  return tasksForNode(node, state.tasks, state.nodes);
}

export function moveTargetOptions(sourceId: string, nodes: AppNode[]): Array<{ id: string; name: string }> {
  const source = nodes.find((node) => node.id === sourceId);
  if (!source || source.kind === "system") return [];
  const excluded = source.kind === "category" ? nodeAndDescendantIds(source.id, nodes) : new Set<string>([source.id]);
  return [
    { id: "", name: "顶层" },
    ...nodes
      .filter((node) => node.kind === "category" && !excluded.has(node.id))
      .map((node) => ({
        id: node.id,
        name: `${"　".repeat(ancestorIds(node.id, nodes).size)}${node.name}`
      }))
  ];
}

export function getBackground(nodeId: string | undefined, backgrounds: Record<string, ListBackground>): ListBackground {
  return nodeId ? (backgrounds[nodeId] ?? defaultBackground) : defaultBackground;
}

export function exportStateForNode(node: AppNode, state: AppState): AppState {
  const tasks = tasksForNode(node, state.tasks, state.nodes);
  const nodeIds = new Set<string>();
  if (node.kind === "category") {
    for (const id of nodeAndDescendantIds(node.id, state.nodes)) nodeIds.add(id);
  } else if (node.kind === "entry") {
    nodeIds.add(node.id);
    for (const id of ancestorIds(node.id, state.nodes)) nodeIds.add(id);
  } else {
    nodeIds.add(node.id);
    for (const task of tasks) {
      nodeIds.add(task.nodeId);
      for (const id of ancestorIds(task.nodeId, state.nodes)) nodeIds.add(id);
    }
  }
  const exportedNodes = state.nodes.filter((item) => item.kind === "system" || nodeIds.has(item.id));
  return {
    schemaVersion: state.schemaVersion,
    nodes: exportedNodes,
    tasks,
    selectedNodeId: node.id,
    backgrounds: Object.fromEntries(exportedNodes.map((item) => [item.id, getBackground(item.id, state.backgrounds)])),
    scheduler: node.id === "scheduled" ? state.scheduler : emptySchedulerState()
  };
}

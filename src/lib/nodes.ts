import type { AppNode, AppState, CardStyle, DiaryEntry, LedgerBook, ListBackground, SearchHit, Task } from "./types";
import { defaultBackground, emptySchedulerState } from "./defaults";
import { filterDiaries } from "./diary";
import { filterLedgerEntries } from "./ledger";

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
  // 一般卡片（cardStyle === "card"）不是待办清单：没有"未完成"语义，角标不该统计它，
  // 也不该往上滚进它所属的分组。四个系统视图照旧统计全部任务。
  const plainEntryIds = new Set(
    state.nodes.filter((node) => node.kind === "entry" && node.cardStyle === "card").map((node) => node.id)
  );
  // **一次遍历任务**把各口径的计数都攒出来。早先是「每个节点各扫一遍全部任务」
  // （O(节点 × 任务)），category 分支里的 descendantEntryIds 还要递归 filter 节点表；
  // 这条 derived 每次 appState 变化都重算，任务上千时是实打实的浪费。
  let myDay = 0;
  let planned = 0;
  let important = 0;
  const byNode = new Map<string, number>();
  for (const task of state.tasks) {
    if (task.completed) continue;
    if (task.myDay) myDay += 1;
    if (task.dueDate || task.plannedDate) planned += 1;
    if (task.important) important += 1;
    if (plainEntryIds.has(task.nodeId)) continue;
    byNode.set(task.nodeId, (byNode.get(task.nodeId) ?? 0) + 1);
  }
  const childrenOf = new Map<string, AppNode[]>();
  for (const node of state.nodes) {
    const key = node.parentId ?? "";
    const bucket = childrenOf.get(key);
    if (bucket) bucket.push(node);
    else childrenOf.set(key, [node]);
  }
  // 分组的角标 = 子树里所有条目的未完成数（一般卡片已经在 byNode 那一步排除掉了）
  const subtree = new Map<string, number>();
  const countSubtree = (node: AppNode): number => {
    const cached = subtree.get(node.id);
    if (cached !== undefined) return cached;
    let total = node.kind === "entry" && !plainEntryIds.has(node.id) ? byNode.get(node.id) ?? 0 : 0;
    for (const child of childrenOf.get(node.id) ?? []) total += countSubtree(child);
    subtree.set(node.id, total);
    return total;
  };
  for (const node of state.nodes) {
    if (node.id === "my-day") {
      counts[node.id] = myDay;
    } else if (node.id === "planned") {
      counts[node.id] = planned;
    } else if (node.id === "important") {
      counts[node.id] = important;
    } else if (node.id === "scheduled") {
      counts[node.id] = state.scheduler.tasks.length;
    } else if (node.kind === "entry") {
      counts[node.id] = plainEntryIds.has(node.id) ? 0 : byNode.get(node.id) ?? 0;
    } else if (node.kind === "category") {
      counts[node.id] = countSubtree(node);
    }
  }
  return counts;
}

export function buildVisibleTasks(state: AppState, node: AppNode | undefined, queryValue: string): Task[] {
  const query = queryValue.trim().toLowerCase();
  if (query) {
    // 搜索时每个任务都要回查它所属条目的名字：先建一次索引，别在 filter 里线性 find
    const nodeById = new Map(state.nodes.map((item) => [item.id, item]));
    return state.tasks.filter((task) => {
      const taskNode = nodeById.get(task.nodeId);
      return task.markdown.toLowerCase().includes(query) || taskNode?.name.toLowerCase().includes(query);
    });
  }
  if (!node) return [];
  return tasksForNode(node, state.tasks, state.nodes);
}

/**
 * 全局搜索的混排结果：任务（含已完成）、日记与记账按「最近改动」排在一条列表里。
 * 匹配规则复用各自那条（任务的 `buildVisibleTasks`、日记的 `filterDiaries`、
 * 记账的 `filterLedgerEntries`），不另写一份。
 */
export function buildSearchHits(state: AppState, diaries: DiaryEntry[], ledger: LedgerBook, query: string): SearchHit[] {
  if (!query.trim()) return [];
  const cardStyleByNode = new Map<string, CardStyle>(
    state.nodes.filter((node) => node.cardStyle === "card").map((node) => [node.id, "card"])
  );
  const hits: SearchHit[] = [
    ...buildVisibleTasks(state, undefined, query).map((task) => ({
      kind: "task" as const,
      key: `task-${task.id}`,
      task,
      cardStyle: cardStyleByNode.get(task.nodeId) ?? ("todo" as CardStyle)
    })),
    ...filterDiaries(diaries, query).map((entry) => ({
      kind: "diary" as const,
      key: `diary-${entry.id}`,
      entry
    })),
    ...filterLedgerEntries(ledger, query).map((entry) => ({
      kind: "ledger" as const,
      key: `ledger-${entry.id}`,
      entry
    }))
  ];
  const touched = (item: { updatedAt?: string; createdAt: string }): string => item.updatedAt || item.createdAt;
  const stampOf = (hit: SearchHit): string => (hit.kind === "task" ? touched(hit.task) : touched(hit.entry));
  return hits.sort((a, b) => stampOf(b).localeCompare(stampOf(a)));
}

/** 拖动落点的三种语义：插到目标前 / 插到目标后 / 移入目标（目标须是分组） */
export type TreeDropPosition = "before" | "after" | "inside";

/** 拖动中向宿主汇报的悬停状态（宿主拿它算实时预览） */
export type TreeHover =
  | { over: "node"; targetId: string; position: TreeDropPosition }
  | { over: "rootEnd" }
  | { over: "none" };

export type TreeMovePlan = {
  /** 移动后的完整节点数组（渲染顺序即数组顺序） */
  ordered: AppNode[];
  /** 移动后的父节点（null = 根级） */
  parentId: string | null;
};

/**
 * 拖动落点 → 新的节点顺序。**预览与提交共用这一份**（v0.8.5 需求 29）：
 * 拖动中行实时让位用的是它，松手落盘也用它是同一个结果，两边不会各算各的。
 * 非法落点（自身 / 自己的后代 / 系统项；inside 落到非分组）返回 null。
 */
export function planTreeMove(
  nodes: AppNode[],
  id: string,
  targetId: string,
  position: TreeDropPosition
): TreeMovePlan | null {
  const source = nodes.find((n) => n.id === id);
  const target = nodes.find((n) => n.id === targetId);
  if (!source || !target || source.kind === "system" || target.kind === "system") return null;
  if (source.id === target.id || nodeAndDescendantIds(source.id, nodes).has(target.id)) return null;
  if (position === "inside" && target.kind !== "category") return null;
  const parentId: string | null = position === "inside" ? target.id : target.parentId ?? null;
  const sourceWithParent = { ...source, parentId };
  const withoutSource = nodes.filter((n) => n.id !== id);
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
    insertIndex = childIndexes.length
      ? Math.max(...childIndexes) + 1
      : targetIndex >= 0
        ? targetIndex + 1
        : withoutSource.length;
  }
  const ordered = [...withoutSource];
  ordered.splice(insertIndex, 0, sourceWithParent);
  return {
    // 移入折叠的分组时顺手展开，落点看得见
    ordered: ordered.map((n) => (position === "inside" && n.id === target.id ? { ...n, collapsed: false } : n)),
    parentId
  };
}

/** 拖到空白区：移动为根级最后一项。 */
export function planTreeRootEnd(nodes: AppNode[], id: string): TreeMovePlan | null {
  const source = nodes.find((n) => n.id === id);
  if (!source || source.kind === "system") return null;
  const withoutSource = nodes.filter((n) => n.id !== id);
  const rootIndexes = withoutSource
    .map((n, i) => ({ n, i }))
    .filter((item) => !item.n.parentId && item.n.kind !== "system")
    .map((item) => item.i);
  const insertIndex = rootIndexes.length ? Math.max(...rootIndexes) + 1 : withoutSource.length;
  const ordered = [...withoutSource];
  ordered.splice(insertIndex, 0, { ...source, parentId: null });
  return { ordered, parentId: null };
}

export function moveTargetOptions(sourceId: string, nodes: AppNode[]): Array<{ id: string; name: string }> {  const source = nodes.find((node) => node.id === sourceId);
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

export function taskMoveTargets(nodes: AppNode[], currentNodeId: string): Array<{ id: string; name: string }> {
  return nodes
    .filter((node) => node.kind === "entry" && node.id !== currentNodeId)
    .map((node) => ({
      id: node.id,
      name: `${"　".repeat(ancestorIds(node.id, nodes).size)}${node.name}`
    }));
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
    // 草稿纸不属于任何条目（单例），导某一条时原样带着走，免得导出再导入把它清掉
    scratchpad: state.scratchpad,
    scheduler: node.id === "scheduled" ? state.scheduler : emptySchedulerState()
  };
}

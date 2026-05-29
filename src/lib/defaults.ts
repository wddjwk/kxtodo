import type { AppSettings, AppState, TodoList, TodoTask } from "./types";

const now = "2026-05-29T00:29:30.913+08:00";

const peachTheme = {
  accent: "#b64a30",
  background: "#fae9df",
  imageOpacity: 0.28
};

export function createId(prefix: string): string {
  const random = Math.random().toString(36).slice(2, 9);
  return `${prefix}-${Date.now().toString(36)}-${random}`;
}

export function cloneState(state: AppState): AppState {
  return JSON.parse(JSON.stringify(state)) as AppState;
}

export function cloneSettings(settings: AppSettings): AppSettings {
  return JSON.parse(JSON.stringify(settings)) as AppSettings;
}

const systemLists: TodoList[] = [
  {
    id: "my-day",
    parentId: null,
    kind: "system",
    nodeType: "entry",
    name: "我的一天",
    icon: "sun",
    collapsed: false,
    shared: false,
    order: 0,
    theme: { accent: "#2564cf", background: "#f4f8ff", imageOpacity: 0.28 }
  },
  {
    id: "planned",
    parentId: null,
    kind: "system",
    nodeType: "entry",
    name: "计划内",
    icon: "calendar",
    collapsed: false,
    shared: false,
    order: 1,
    theme: { accent: "#5b6f82", background: "#f8fafc", imageOpacity: 0.28 }
  },
  {
    id: "favorite",
    parentId: null,
    kind: "system",
    nodeType: "entry",
    name: "收藏",
    icon: "star",
    collapsed: false,
    shared: false,
    order: 2,
    theme: { accent: "#b64a30", background: "#fff7ef", imageOpacity: 0.28 }
  },
  {
    id: "quick-notes",
    parentId: null,
    kind: "custom",
    nodeType: "entry",
    name: "随手记",
    icon: "notebook",
    collapsed: false,
    shared: false,
    order: 10,
    theme: peachTheme
  }
];

export function defaultState(): AppState {
  return {
    schemaVersion: 3,
    selectedListId: "quick-notes",
    lists: cloneState({
      schemaVersion: 3,
      selectedListId: "quick-notes",
      lists: systemLists,
      tasks: [],
      updatedAt: now
    }).lists,
    tasks: [],
    updatedAt: now
  };
}

export function normalizeState(input: unknown): AppState {
  const defaults = defaultState();
  const value = (input && typeof input === "object" ? input : {}) as Partial<AppState>;
  const incomingLists = Array.isArray(value.lists) ? value.lists : [];

  const customLists = incomingLists
    .filter((list) => list.kind === "custom")
    .map((list, index, allCustom) => {
      const hasChildren = allCustom.some((candidate) => candidate.parentId === list.id);
      const nodeType = list.nodeType ?? (hasChildren ? "category" : "entry");
      return {
        ...list,
        kind: "custom" as const,
        nodeType,
        icon: nodeType === "category" ? "folder" : list.icon || "notebook",
        parentId: list.parentId ?? null,
        collapsed: Boolean(list.collapsed),
        shared: Boolean(list.shared),
        order: Number.isFinite(list.order) ? list.order : index + 10,
        theme: { ...peachTheme, ...(list.theme ?? {}), imageOpacity: list.theme?.imageOpacity ?? peachTheme.imageOpacity }
      };
    });

  const lists = [...cloneState({ ...defaults, lists: systemLists }).lists.filter((list) => list.kind === "system"), ...customLists];
  const selectedListId = lists.some((list) => list.id === value.selectedListId)
    ? String(value.selectedListId)
    : customLists.find((list) => list.nodeType === "entry")?.id ?? "quick-notes";

  const tasks = (Array.isArray(value.tasks) ? value.tasks : [])
    .map((task) => {
      const candidate = task as Partial<TodoTask> & { content?: string };
      const markdown = typeof candidate.markdown === "string" ? candidate.markdown : candidate.content;
      if (!markdown || typeof candidate.listId !== "string") {
        return null;
      }
      return {
        ...candidate,
        id: candidate.id || createId("task"),
        listId: candidate.listId,
        markdown,
        completed: Boolean(candidate.completed),
        important: Boolean(candidate.important),
        expanded: Boolean(candidate.expanded),
        steps: Array.isArray(candidate.steps) ? candidate.steps : [],
        notes: candidate.notes ?? "",
        dueDate: candidate.dueDate ?? null,
        reminder: candidate.reminder ?? null,
        repeat: candidate.repeat ?? null,
        tags: Array.isArray(candidate.tags) ? candidate.tags : [],
        createdAt: candidate.createdAt ?? new Date().toISOString(),
        updatedAt: candidate.updatedAt ?? new Date().toISOString()
      } satisfies TodoTask;
    })
    .filter((task): task is TodoTask => Boolean(task));

  return {
    schemaVersion: 3,
    selectedListId,
    lists,
    tasks,
    updatedAt: value.updatedAt ?? new Date().toISOString()
  };
}

export function defaultSettings(): AppSettings {
  return {
    schemaVersion: 3,
    profile: {
      avatar: "示",
      name: "Example User",
      email: "example@example.com"
    },
    taskDensity: "comfortable",
    sidebarWidth: 300,
    globalShortcut: "Ctrl+Shift+Space",
    defaultListTheme: peachTheme,
    cloudSync: {
      enabled: false,
      provider: "none",
      endpoint: "",
      status: "not_configured"
    },
    shortcuts: [
      { id: "new-task", label: "新建 Markdown 内容", combo: "Ctrl+N" },
      { id: "new-category", label: "新建顶级分类", combo: "Ctrl+Shift+N" },
      { id: "new-child-category", label: "新建当前分类下子分类", combo: "Ctrl+Alt+N" },
      { id: "search", label: "搜索", combo: "Ctrl+F" },
      { id: "toggle-settings", label: "打开/关闭设置", combo: "Ctrl+," },
      { id: "export-list", label: "导出当前条目/分类", combo: "Ctrl+E" },
      { id: "export-all", label: "导出全部", combo: "Ctrl+Shift+E" }
    ]
  };
}

export function normalizeSettings(input: unknown): AppSettings {
  const defaults = defaultSettings();
  const value = (input && typeof input === "object" ? input : {}) as Partial<AppSettings>;
  const existing = new Map((Array.isArray(value.shortcuts) ? value.shortcuts : []).map((shortcut) => [shortcut.id, shortcut]));

  return {
    ...defaults,
    ...value,
    schemaVersion: 3,
    profile: { ...defaults.profile, ...(value.profile ?? {}) },
    sidebarWidth: Math.min(520, Math.max(240, Number(value.sidebarWidth ?? defaults.sidebarWidth))),
    globalShortcut: value.globalShortcut || defaults.globalShortcut,
    shortcuts: defaults.shortcuts.map((shortcut) => existing.get(shortcut.id) ?? shortcut),
    cloudSync: { ...defaults.cloudSync, ...(value.cloudSync ?? {}) },
    defaultListTheme: { ...defaults.defaultListTheme, ...(value.defaultListTheme ?? {}) }
  };
}

import type { AppNode, AppState, ListBackground, Settings, Task } from "./types";

const now = () => new Date().toISOString();

export const schemaVersion = 3;

export const systemNodes: AppNode[] = [
  { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now() },
  { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now() },
  { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now() }
];

export const defaultBackground: ListBackground = {
  color: "#f7efe8",
  imageOpacity: 0.28
};

export const themePresets = [
  { name: "暖杏", color: "#f7efe8" },
  { name: "To Do 蓝", color: "#e8f1ff" },
  { name: "薄荷", color: "#eaf7ef" },
  { name: "丁香", color: "#f0edff" },
  { name: "玫瑰", color: "#fff0f4" },
  { name: "石墨", color: "#edf1f6" },
  { name: "沙丘", color: "#f7eadc" },
  { name: "海盐", color: "#e8f7fb" },
  { name: "青柠", color: "#eef9dc" },
  { name: "莓果", color: "#f9e4f2" },
  { name: "深夜", color: "#242936" },
  { name: "纸张", color: "#faf7f1" }
];

export const defaultSettings: Settings = {
  profile: {
    displayName: "Example User",
    email: "example@example.com",
    avatar: ""
  },
  shortcuts: {
    newTask: "Ctrl+N",
    focusSearch: "Ctrl+F",
    toggleWindow: "Ctrl+Shift+Space",
    openSettings: "Ctrl+,"
  },
  cloud: {
    provider: "none",
    endpoint: "",
    enabled: false
  }
};

function createId(prefix: string): string {
  const random = globalThis.crypto?.randomUUID?.().slice(0, 8) ?? Math.random().toString(36).slice(2, 10);
  return `${prefix}-${random}`;
}

export function createEntryNode(name = "未命名条目", parentId: string | null = null, icon = "notebook"): AppNode {
  return {
    id: createId("entry"),
    kind: "entry",
    name,
    icon,
    parentId,
    createdAt: now()
  };
}

export function createCategoryNode(name = "未命名分类", parentId: string | null = null): AppNode {
  return {
    id: createId("category"),
    kind: "category",
    name,
    icon: "folder",
    parentId,
    collapsed: false,
    createdAt: now()
  };
}

export function emptyState(): AppState {
  const inbox = createEntryNode("收集箱", null, "inbox");
  return {
    schemaVersion,
    nodes: [...systemNodes, inbox],
    tasks: [],
    selectedNodeId: inbox.id,
    backgrounds: {
      [inbox.id]: { ...defaultBackground }
    }
  };
}

function normalizeNode(raw: unknown): AppNode | null {
  const source = raw as Partial<AppNode> & {
    kind?: string;
    nodeType?: string;
    parent?: string | null;
    label?: string;
    order?: number;
  };
  if (!source || typeof source !== "object") {
    return null;
  }
  const kind =
    source.kind === "category" || source.kind === "entry" || source.kind === "system"
      ? source.kind
      : source.nodeType === "category"
        ? "category"
        : source.kind === "custom" || source.nodeType === "entry"
          ? "entry"
          : null;
  if (!kind) {
    return null;
  }
  const id = typeof source.id === "string" && source.id ? source.id : createId(kind);
  return {
    id,
    kind,
    name:
      typeof source.name === "string"
        ? source.name
        : typeof source.label === "string"
          ? source.label
          : kind === "category"
            ? "未命名分类"
            : "未命名条目",
    icon: typeof source.icon === "string" ? source.icon : kind === "category" ? "folder" : "notebook",
    parentId: typeof source.parentId === "string" || source.parentId === null ? source.parentId : typeof source.parent === "string" ? source.parent : null,
    collapsed: Boolean(source.collapsed),
    createdAt: typeof source.createdAt === "string" ? source.createdAt : now()
  };
}

function normalizeTask(raw: unknown, fallbackNodeId: string): Task | null {
  const source = raw as Partial<Task> & { listId?: string; content?: string; dueDate?: string | null; plannedDate?: string | null };
  if (!source || typeof source !== "object") {
    return null;
  }
  const markdown = typeof source.markdown === "string" ? source.markdown : typeof source.content === "string" ? source.content : "";
  if (!markdown.trim()) {
    return null;
  }
  return {
    id: typeof source.id === "string" && source.id ? source.id : createId("task"),
    nodeId: typeof source.nodeId === "string" ? source.nodeId : typeof source.listId === "string" ? source.listId : fallbackNodeId,
    markdown,
    completed: Boolean(source.completed),
    important: Boolean(source.important),
    myDay: Boolean(source.myDay),
    plannedDate: typeof source.plannedDate === "string" ? source.plannedDate : undefined,
    dueDate: typeof source.dueDate === "string" ? source.dueDate : undefined,
    expanded: Boolean(source.expanded),
    editing: Boolean(source.editing),
    createdAt: typeof source.createdAt === "string" ? source.createdAt : now(),
    updatedAt: typeof source.updatedAt === "string" ? source.updatedAt : now()
  };
}

export function normalizeState(raw: unknown): AppState {
  const fallback = emptyState();
  const source = raw as Partial<AppState> & { lists?: unknown[]; selectedListId?: string };
  const rawNodes = Array.isArray(source?.nodes) ? source.nodes : Array.isArray(source?.lists) ? source.lists : [];
  const nodes = rawNodes.map(normalizeNode).filter((node): node is AppNode => Boolean(node));
  const mergedNodes = [...systemNodes, ...nodes.filter((node) => node.kind !== "system")];
  const fallbackEntry = mergedNodes.find((node) => node.kind === "entry") ?? createEntryNode("收集箱");
  if (!mergedNodes.some((node) => node.id === fallbackEntry.id)) {
    mergedNodes.push(fallbackEntry);
  }

  const validNodeIds = new Set(mergedNodes.map((node) => node.id));
  const tasks = Array.isArray(source?.tasks)
    ? source.tasks.map((item) => normalizeTask(item, fallbackEntry.id)).filter((task): task is Task => task !== null && validNodeIds.has(task.nodeId))
    : [];

  const backgrounds: Record<string, ListBackground> = {};
  const rawBackgrounds = source?.backgrounds as Record<string, Partial<ListBackground>> | undefined;
  const legacyThemes = (source as { lists?: Array<{ id?: string; theme?: Partial<{ background: string; image: string; imageOpacity: number }> }> }).lists;
  for (const node of mergedNodes) {
    const rawBackground = rawBackgrounds?.[node.id];
    const legacyTheme = legacyThemes?.find((list) => list.id === node.id)?.theme;
    backgrounds[node.id] = {
      color: typeof rawBackground?.color === "string" ? rawBackground.color : typeof legacyTheme?.background === "string" ? legacyTheme.background : defaultBackground.color,
      image: typeof rawBackground?.image === "string" ? rawBackground.image : typeof legacyTheme?.image === "string" ? legacyTheme.image : undefined,
      imageOpacity:
        typeof rawBackground?.imageOpacity === "number"
          ? rawBackground.imageOpacity
          : typeof legacyTheme?.imageOpacity === "number"
            ? legacyTheme.imageOpacity
            : defaultBackground.imageOpacity
    };
  }

  const selectedNodeId = validNodeIds.has(source?.selectedNodeId ?? "")
    ? (source?.selectedNodeId as string)
    : validNodeIds.has(source?.selectedListId ?? "")
      ? (source?.selectedListId as string)
      : fallbackEntry.id;

  return {
    schemaVersion,
    nodes: mergedNodes,
    tasks,
    selectedNodeId,
    backgrounds
  };
}

export function normalizeSettings(raw: unknown): Settings {
  const source = raw as Partial<Settings> & {
    profile?: Partial<Settings["profile"]> & { name?: string };
    globalShortcut?: string;
    shortcuts?: Partial<Settings["shortcuts"]> | Array<{ id: string; combo: string }>;
    cloudSync?: Partial<Settings["cloud"]>;
  };
  const legacyShortcuts = Array.isArray(source?.shortcuts) ? source.shortcuts : [];
  const shortcutValue = (key: keyof Settings["shortcuts"], fallback: string) => {
    if (!Array.isArray(source?.shortcuts) && typeof source?.shortcuts?.[key] === "string") {
      return source.shortcuts[key] as string;
    }
    if (key === "openSettings") {
      return legacyShortcuts.find((shortcut) => shortcut.id === "openSettings" || shortcut.id === "toggleSettings")?.combo ?? fallback;
    }
    return legacyShortcuts.find((shortcut) => shortcut.id === key)?.combo ?? fallback;
  };
  return {
    profile: {
      displayName:
        typeof source?.profile?.displayName === "string"
          ? source.profile.displayName
          : typeof source?.profile?.name === "string"
            ? source.profile.name
            : defaultSettings.profile.displayName,
      email: typeof source?.profile?.email === "string" ? source.profile.email : defaultSettings.profile.email,
      avatar: typeof source?.profile?.avatar === "string" ? source.profile.avatar : defaultSettings.profile.avatar
    },
    shortcuts: {
      newTask: shortcutValue("newTask", defaultSettings.shortcuts.newTask),
      focusSearch: shortcutValue("focusSearch", defaultSettings.shortcuts.focusSearch),
      toggleWindow:
        typeof source?.globalShortcut === "string"
          ? source.globalShortcut
          : shortcutValue("toggleWindow", defaultSettings.shortcuts.toggleWindow),
      openSettings: shortcutValue("openSettings", defaultSettings.shortcuts.openSettings)
    },
    cloud: {
      provider:
        source?.cloud?.provider === "webdav" || source?.cloud?.provider === "s3" || source?.cloud?.provider === "custom"
          ? source.cloud.provider
          : source?.cloudSync?.provider === "webdav" || source?.cloudSync?.provider === "s3" || source?.cloudSync?.provider === "custom"
            ? source.cloudSync.provider
            : "none",
      endpoint: typeof source?.cloud?.endpoint === "string" ? source.cloud.endpoint : typeof source?.cloudSync?.endpoint === "string" ? source.cloudSync.endpoint : "",
      enabled: Boolean(source?.cloud?.enabled ?? source?.cloudSync?.enabled)
    }
  };
}

import type { AppNode, AppState, ListBackground, Settings, Task } from "./types";

const now = () => new Date().toISOString();

export const schemaVersion = 3;

export const systemNodes: AppNode[] = [
  { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now() },
  { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now() },
  { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now() }
];

export const defaultBackground: ListBackground = {
  color: "#fafaf8",
  imageOpacity: 0.28
};

export const themePresets = [
  { name: "白瓷",   color: "#fafaf8" },
  { name: "日出",   color: "#f5e6d8" },
  { name: "睡莲",   color: "#e4ede6" },
  { name: "晨雾",   color: "#dfe8ef" },
  { name: "干草垛", color: "#f0e4cc" },
  { name: "教堂",   color: "#e8e2ed" },
  { name: "花园",   color: "#f2e2e5" },
  { name: "拱桥",   color: "#daeee8" },
  { name: "鸢尾",   color: "#ddd8ea" },
  { name: "麦田",   color: "#ece8d4" },
  { name: "夜色",   color: "#2a2d38" }
];

export const defaultSettings: Settings = {
  profile: {
    displayName: "Example User",
    email: "example@example.com",
    avatar: ""
  },
  appearance: {
    linkOpenMode: "app",
    uiScale: 0.75,
    uiFontSize: 18,
    markdownFontSize: 20,
    editorFontSize: 20
  },
  lifecycle: {
    closeToTray: true,
    launchAtStartup: false
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
    completedAt: typeof source.completedAt === "string" ? source.completedAt : source.completed ? (typeof source.updatedAt === "string" ? source.updatedAt : now()) : undefined,
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
    appearance?: Partial<Settings["appearance"]>;
    lifecycle?: Partial<Settings["lifecycle"]>;
    behavior?: Partial<{ linkOpenMode: Settings["appearance"]["linkOpenMode"] }>;
    display?: Partial<{ uiScale: number; closeToTray: boolean; launchAtStartup: boolean }>;
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
  const normalizeUiScale = (value: unknown): number | null => {
    if (typeof value !== "number") {
      return null;
    }
    if (Math.abs(value - 0.62) < 0.001 || Math.abs(value - 0.72) < 0.001 || Math.abs(value - 0.86) < 0.001 || Math.abs(value - 0.92) < 0.001) {
      return defaultSettings.appearance.uiScale;
    }
    return Math.min(1.5, Math.max(0.5, value));
  };
  const normalizeFontSize = (value: unknown, fallback: number, min = 14, max = 24): number =>
    typeof value === "number" ? Math.min(max, Math.max(min, Math.round(value))) : fallback;
  const storedUiScale = normalizeUiScale(source?.appearance?.uiScale) ?? normalizeUiScale(source?.display?.uiScale);
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
    appearance: {
      linkOpenMode:
        source?.appearance?.linkOpenMode === "system" || source?.behavior?.linkOpenMode === "system"
          ? "system"
          : defaultSettings.appearance.linkOpenMode,
      uiScale:
        storedUiScale ?? defaultSettings.appearance.uiScale,
      uiFontSize: normalizeFontSize(source?.appearance?.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22),
      markdownFontSize: normalizeFontSize(source?.appearance?.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26),
      editorFontSize: normalizeFontSize(source?.appearance?.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26)
    },
    lifecycle: {
      closeToTray:
        typeof source?.lifecycle?.closeToTray === "boolean"
          ? source.lifecycle.closeToTray
          : typeof source?.display?.closeToTray === "boolean"
            ? source.display.closeToTray
            : defaultSettings.lifecycle.closeToTray,
      launchAtStartup:
        typeof source?.lifecycle?.launchAtStartup === "boolean"
          ? source.lifecycle.launchAtStartup
          : typeof source?.display?.launchAtStartup === "boolean"
            ? source.display.launchAtStartup
            : defaultSettings.lifecycle.launchAtStartup
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

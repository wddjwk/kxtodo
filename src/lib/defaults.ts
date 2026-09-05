import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import type {
  AppNode,
  AppState,
  ListBackground,
  AppNotification,
  NotificationPosition,
  NotificationTone,
  ScheduledTask,
  ScheduledTaskAction,
  ScheduledTaskTrigger,
  SchedulerCondition,
  SchedulerRuntimeKey,
  SchedulerRuntimePaths,
  SchedulerState,
  Settings,
  Tag,
  TagColor,
  Task,
  ThemePreset
} from "./types";

const now = () => new Date().toISOString();

export const schemaVersion = 4;

export const systemNodes: AppNode[] = [
  { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now() },
  { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now() },
  { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now() },
  { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now() }
];

export const defaultBackground: ListBackground = {
  color: "#f4f1ea",
  imageOpacity: 0.28
};

export const themePresets: ThemePreset[] = [
  { name: "雾瓷",     color: "#f4f1ea" },
  { name: "睡莲灰绿", color: "#dfe8df" },
  { name: "晨雾蓝",   color: "#dbe4e6" },
  { name: "粉霞",     color: "#ead9d5" },
  { name: "鸢尾雾紫", color: "#ded8e6" },
  { name: "亚麻麦秆", color: "#ece2ca" },
  { name: "石英灰",   color: "#e3e0d8" },
  { name: "鼠尾草",   color: "#d8dfd2" },
  { name: "贵族蓝灰", color: "#cfd9df" },
  { name: "陶土玫瑰", color: "#e5d4cb" }
];

/**
 * 不从 platform.ts 导入 hostOs：defaults 被 stores 顶层引用，而 platform→stores
 * 已存在，再加 defaults→platform 边会构成新环，模块求值顺序一变即 TDZ 白屏
 * （见 AGENTS.md 模块循环坑位）。故此处内联同款检测（仅判 Linux）：官方 os
 * 插件为准，UA 只是浏览器 dev / 未注册插件端的回退。
 * Linux 桌面托盘常不可见（WSLg/GNOME），关闭按钮默认退出而非隐藏到托盘。
 */
function isLinuxHost(): boolean {
  try {
    return tauriPlatform() === "linux";
  } catch {
    if (typeof navigator === "undefined") {
      return false;
    }
    const ua = navigator.userAgent || "";
    return !/Android|iPhone|iPad|iPod/i.test(ua) && /Linux|X11/i.test(ua);
  }
}

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
    editorFontSize: 20,
    tagFontSize: 14,
    themePresets: themePresets.map((preset) => ({ ...preset })),
    uiColors: {}
  },
  lifecycle: {
    closeToTray: !isLinuxHost(),
    launchAtStartup: false
  },
  notifications: {
    durationMs: 3000,
    position: "bottom-right",
    width: 400,
    height: 68,
    titleFontSize: 14,
    bodyFontSize: 12
  },
  shortcuts: {
    newTask: "Ctrl+N",
    focusSearch: "Ctrl+F",
    toggleWindow: "Ctrl+Shift+Space",
    openSettings: "Ctrl+,"
  },
  sync: {
    enabled: false,
    serverUrl: "",
    username: "",
    email: "",
    secret: "",
    syncData: true,
    syncSettings: false,
    syncSchedules: false,
    intervalMinutes: 5
  },
  updates: {
    autoCheck: true
  },
  features: {
    showCategoryBadges: true
  }
};

export const schedulerRuntimeKeys: SchedulerRuntimeKey[] = ["python", "node", "pwsh", "bash", "make"];

export const defaultSchedulerRuntimes: SchedulerRuntimePaths = {
  python: "",
  node: "",
  pwsh: "",
  bash: "",
  make: ""
};

export function defaultSchedulerCondition(enabled = false): SchedulerCondition {
  return {
    enabled,
    mode: "contains",
    pattern: ""
  };
}

export function defaultAppNotification(message = "定时任务已触发", tone: NotificationTone = "info"): AppNotification {
  return {
    title: "KXToDo",
    message,
    durationMs: defaultSettings.notifications.durationMs,
    tone
  };
}

export function defaultScheduledTaskAction(language: ScheduledTaskAction["language"] = "python"): ScheduledTaskAction {
  return {
    type: "script",
    scriptMode: "inline",
    language,
    interpreter: "",
    filePath: "",
    code: language === "python" ? "print(\"hello from KXToDo\")" : "",
    executablePath: "",
    arguments: "",
    workingDirectory: "",
    notification: defaultAppNotification("定时任务已触发", "info"),
    notifyOnComplete: false,
    completionNotification: defaultAppNotification("任务 {taskName} 执行完成\n{stdout}", "success"),
    stdoutNotification: {
      enabled: false,
      condition: defaultSchedulerCondition(false),
      notification: defaultAppNotification("stdout 匹配成功：\n{stdout}", "info")
    }
  };
}

export function defaultScheduledTaskTrigger(type: ScheduledTaskTrigger["type"] = "once"): ScheduledTaskTrigger {
  const runAt = new Date(Date.now() + 5 * 60_000).toISOString().slice(0, 16);
  return {
    type,
    runAt,
    everySeconds: type === "condition" ? 60 : 300,
    repeatCount: type === "interval" ? 0 : 1,
    cron: "0 9 * * *",
    stopCondition: defaultSchedulerCondition(false),
    probeAction: defaultScheduledTaskAction("python"),
    probeCondition: defaultSchedulerCondition(true)
  };
}

export function emptySchedulerState(): SchedulerState {
  return {
    runtimes: { ...defaultSchedulerRuntimes },
    tasks: []
  };
}

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

export function createScheduledTask(name = "新的定时任务"): ScheduledTask {
  const timestamp = now();
  return {
    id: createId("schedule"),
    name,
    enabled: false,
    expanded: true,
    editing: true,
    trigger: defaultScheduledTaskTrigger("once"),
    action: defaultScheduledTaskAction("python"),
    runCount: 0,
    lastStatus: "idle",
    createdAt: timestamp,
    updatedAt: timestamp
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
    },
    scheduler: emptySchedulerState()
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

const TAG_COLORS: TagColor[] = ["red", "yellow", "blue", "green", "gray"];

function normalizeTags(raw: unknown): Tag[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((item): Tag | null => {
      if (!item || typeof item !== "object") return null;
      const tag = item as Partial<Tag>;
      const color = TAG_COLORS.includes(tag.color as TagColor) ? tag.color as TagColor : "gray";
      const id = typeof tag.id === "string" && tag.id ? tag.id : createId("tag");
      const text = typeof tag.text === "string" ? tag.text.trim().slice(0, 20) : undefined;
      return { id, color, text: text || undefined };
    })
    .filter((tag): tag is Tag => tag !== null);
}

function normalizeEmojis(raw: unknown, legacyEmoji: unknown): string[] {
  const list: string[] = [];
  if (Array.isArray(raw)) {
    for (const item of raw) {
      if (typeof item === "string" && item.trim()) list.push(item);
    }
  } else if (typeof legacyEmoji === "string" && legacyEmoji.trim()) {
    list.push(legacyEmoji);
  }
  return list;
}

function normalizeTask(raw: unknown, fallbackNodeId: string): Task | null {
  const source = raw as Partial<Task> & { listId?: string; content?: string; dueDate?: string | null; plannedDate?: string | null; emoji?: string };
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
    tags: normalizeTags(source.tags),
    emojis: normalizeEmojis(source.emojis, source.emoji),
    expanded: Boolean(source.expanded),
    createdAt: typeof source.createdAt === "string" ? source.createdAt : now(),
    updatedAt: typeof source.updatedAt === "string" ? source.updatedAt : now()
  };
}

function normalizeSchedulerCondition(raw: unknown, fallbackEnabled = false): SchedulerCondition {
  const source = raw as Partial<SchedulerCondition> | undefined;
  return {
    enabled: typeof source?.enabled === "boolean" ? source.enabled : fallbackEnabled,
    mode: source?.mode === "regex" ? "regex" : "contains",
    pattern: typeof source?.pattern === "string" ? source.pattern : ""
  };
}

function normalizeNotificationTone(raw: unknown, fallback: NotificationTone): NotificationTone {
  return raw === "success" || raw === "warning" || raw === "error" || raw === "info" ? raw : fallback;
}

function normalizeNotificationPosition(raw: unknown, fallback: NotificationPosition): NotificationPosition {
  return raw === "top-right" || raw === "bottom-left" || raw === "top-left" || raw === "bottom-right" ? raw : fallback;
}

function normalizeNotificationDuration(raw: unknown, fallback: number): number {
  return typeof raw === "number" && Number.isFinite(raw)
    ? Math.min(60_000, Math.max(1_200, Math.round(raw)))
    : fallback;
}

function normalizeAppNotification(raw: unknown, fallback: AppNotification): AppNotification {
  const source = raw as Partial<AppNotification> | undefined;
  return {
    title: typeof source?.title === "string" && source.title.trim() ? source.title.trim().slice(0, 80) : fallback.title,
    message: typeof source?.message === "string" && source.message.trim() ? source.message : fallback.message,
    durationMs: normalizeNotificationDuration(source?.durationMs, fallback.durationMs),
    tone: normalizeNotificationTone(source?.tone, fallback.tone),
    position: source?.position ? normalizeNotificationPosition(source.position, defaultSettings.notifications.position) : undefined
  };
}

function normalizeScheduledAction(raw: unknown, fallbackLanguage: ScheduledTaskAction["language"] = "python"): ScheduledTaskAction {
  const source = raw as Partial<ScheduledTaskAction> | undefined;
  const language =
    source?.language === "javascript" ||
    source?.language === "powershell" ||
    source?.language === "bash" ||
    source?.language === "makefile" ||
    source?.language === "custom" ||
    source?.language === "python"
      ? source.language
      : fallbackLanguage;
  const defaultAction = defaultScheduledTaskAction(language);
  return {
    type: source?.type === "executable" || source?.type === "notification" ? source.type : "script",
    scriptMode: source?.scriptMode === "path" ? "path" : "inline",
    language,
    interpreter: typeof source?.interpreter === "string" ? source.interpreter : "",
    filePath: typeof source?.filePath === "string" ? source.filePath : "",
    code: typeof source?.code === "string" ? source.code : (language === "python" ? "print(\"hello from KXToDo\")" : ""),
    executablePath: typeof source?.executablePath === "string" ? source.executablePath : "",
    arguments: typeof source?.arguments === "string" ? source.arguments : "",
    workingDirectory: typeof source?.workingDirectory === "string" ? source.workingDirectory : "",
    notification: normalizeAppNotification(source?.notification, defaultAction.notification),
    notifyOnComplete: Boolean(source?.notifyOnComplete),
    completionNotification: normalizeAppNotification(source?.completionNotification, defaultAction.completionNotification),
    stdoutNotification: {
      enabled: Boolean(source?.stdoutNotification?.enabled),
      condition: normalizeSchedulerCondition(source?.stdoutNotification?.condition, false),
      notification: normalizeAppNotification(source?.stdoutNotification?.notification, defaultAction.stdoutNotification.notification)
    }
  };
}

function normalizePositiveInteger(value: unknown, fallback: number, min: number, max: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, Math.round(value)))
    : fallback;
}

function normalizeNonNegativeInteger(value: unknown, fallback: number, max: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(0, Math.round(value)))
    : fallback;
}

function normalizeScheduledTrigger(raw: unknown): ScheduledTaskTrigger {
  const source = raw as Partial<ScheduledTaskTrigger> | undefined;
  const type =
    source?.type === "interval" || source?.type === "calendar" || source?.type === "condition" || source?.type === "once"
      ? source.type
      : "once";
  const fallback = defaultScheduledTaskTrigger(type);
  return {
    type,
    runAt: typeof source?.runAt === "string" && source.runAt ? source.runAt : fallback.runAt,
    everySeconds: normalizePositiveInteger(source?.everySeconds, fallback.everySeconds, 1, 31_536_000),
    repeatCount: normalizeNonNegativeInteger(source?.repeatCount, fallback.repeatCount, 1_000_000),
    cron: typeof source?.cron === "string" && source.cron.trim() ? source.cron.trim() : fallback.cron,
    stopCondition: normalizeSchedulerCondition(source?.stopCondition, false),
    probeAction: normalizeScheduledAction(source?.probeAction, "python"),
    probeCondition: normalizeSchedulerCondition(source?.probeCondition, true)
  };
}

function normalizeScheduledTask(raw: unknown): ScheduledTask | null {
  const source = raw as Partial<ScheduledTask> | undefined;
  if (!source || typeof source !== "object") {
    return null;
  }
  const name = typeof source.name === "string" && source.name.trim() ? source.name.trim() : "未命名定时任务";
  return {
    id: typeof source.id === "string" && source.id ? source.id : createId("schedule"),
    name,
    enabled: Boolean(source.enabled),
    expanded: Boolean(source.expanded),
    editing: Boolean(source.editing),
    trigger: normalizeScheduledTrigger(source.trigger),
    action: normalizeScheduledAction(source.action, "python"),
    runCount: normalizeNonNegativeInteger(source.runCount, 0, 1_000_000),
    lastRunAt: typeof source.lastRunAt === "string" ? source.lastRunAt : undefined,
    nextRunAt: typeof source.nextRunAt === "string" ? source.nextRunAt : undefined,
    lastStatus:
      source.lastStatus === "running" || source.lastStatus === "success" || source.lastStatus === "failed" || source.lastStatus === "stopped"
        ? source.lastStatus
        : "idle",
    lastExitCode: typeof source.lastExitCode === "number" || source.lastExitCode === null ? source.lastExitCode : undefined,
    lastStdout: typeof source.lastStdout === "string" ? source.lastStdout : "",
    lastStderr: typeof source.lastStderr === "string" ? source.lastStderr : "",
    createdAt: typeof source.createdAt === "string" ? source.createdAt : now(),
    updatedAt: typeof source.updatedAt === "string" ? source.updatedAt : now()
  };
}

export function normalizeSchedulerState(raw: unknown): SchedulerState {
  const source = raw as Partial<SchedulerState> | undefined;
  const rawRuntimes = source?.runtimes as Partial<SchedulerRuntimePaths> | undefined;
  const runtimes = schedulerRuntimeKeys.reduce((acc, key) => {
    const stored = rawRuntimes?.[key];
    acc[key] = typeof stored === "string" ? stored : "";
    return acc;
  }, { ...defaultSchedulerRuntimes });
  return {
    runtimes,
    tasks: Array.isArray(source?.tasks)
      ? source.tasks.map(normalizeScheduledTask).filter((task): task is ScheduledTask => task !== null)
      : []
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
    backgrounds,
    scheduler: normalizeSchedulerState(source?.scheduler)
  };
}

export function normalizeSettings(raw: unknown): Settings {
  const source = raw as Partial<Settings> & {
    profile?: Partial<Settings["profile"]> & { name?: string };
    appearance?: Partial<Settings["appearance"]>;
    lifecycle?: Partial<Settings["lifecycle"]>;
    notifications?: Partial<Settings["notifications"]>;
    behavior?: Partial<{ linkOpenMode: Settings["appearance"]["linkOpenMode"] }>;
    display?: Partial<{ uiScale: number; closeToTray: boolean; launchAtStartup: boolean; notificationDurationMs: number }>;
    globalShortcut?: string;
    shortcuts?: Partial<Settings["shortcuts"]> | Array<{ id: string; combo: string }>;
    sync?: Partial<Settings["sync"]>;
    features?: Partial<Settings["features"]>;
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
  const normalizeUiScale = (value: unknown): number | null =>
    typeof value === "number" && Number.isFinite(value) ? Math.min(1.5, Math.max(0.5, value)) : null;
  const normalizeFontSize = (value: unknown, fallback: number, min = 14, max = 24): number =>
    typeof value === "number" ? Math.min(max, Math.max(min, Math.round(value))) : fallback;
  const normalizeHexColor = (value: unknown, fallback: string): string =>
    typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value.trim()) ? value.trim() : fallback;
  const normalizeThemePresets = (value: unknown): ThemePreset[] => {
    const presets = Array.isArray(value) ? value : [];
    return themePresets.map((fallback, index) => {
      const preset = presets[index] as Partial<ThemePreset> | undefined;
      return {
        name: typeof preset?.name === "string" && preset.name.trim() ? preset.name.trim().slice(0, 24) : fallback.name,
        color: normalizeHexColor(preset?.color, fallback.color)
      };
    });
  };
  const normalizeUiColors = (value: unknown): Record<string, string> => {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return {};
    }
    const colors: Record<string, string> = {};
    for (const [nodeId, color] of Object.entries(value)) {
      if (typeof nodeId === "string" && nodeId && typeof color === "string" && /^#[0-9a-f]{6}$/i.test(color.trim())) {
        colors[nodeId] = color.trim();
      }
    }
    return colors;
  };
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
      editorFontSize: normalizeFontSize(source?.appearance?.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26),
      tagFontSize: normalizeFontSize(source?.appearance?.tagFontSize, defaultSettings.appearance.tagFontSize, 11, 30),
      themePresets: normalizeThemePresets(source?.appearance?.themePresets),
      uiColors: normalizeUiColors(source?.appearance?.uiColors)
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
    notifications: {
      durationMs: normalizeNotificationDuration(
        source?.notifications?.durationMs ?? source?.display?.notificationDurationMs,
        defaultSettings.notifications.durationMs
      ),
      position: normalizeNotificationPosition(source?.notifications?.position, defaultSettings.notifications.position),
      width: typeof source?.notifications?.width === "number" && Number.isFinite(source.notifications.width)
        ? Math.min(600, Math.max(280, Math.round(source.notifications.width))) : defaultSettings.notifications.width,
      height: typeof source?.notifications?.height === "number" && Number.isFinite(source.notifications.height)
        ? Math.min(200, Math.max(50, Math.round(source.notifications.height))) : defaultSettings.notifications.height,
      titleFontSize: typeof source?.notifications?.titleFontSize === "number" && Number.isFinite(source.notifications.titleFontSize)
        ? Math.min(24, Math.max(10, Math.round(source.notifications.titleFontSize))) : defaultSettings.notifications.titleFontSize,
      bodyFontSize: typeof source?.notifications?.bodyFontSize === "number" && Number.isFinite(source.notifications.bodyFontSize)
        ? Math.min(20, Math.max(8, Math.round(source.notifications.bodyFontSize))) : defaultSettings.notifications.bodyFontSize
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
    sync: {
      enabled: Boolean(source?.sync?.enabled),
      serverUrl: typeof source?.sync?.serverUrl === "string" ? source.sync.serverUrl : "",
      username: typeof source?.sync?.username === "string" ? source.sync.username : "",
      email: typeof source?.sync?.email === "string" ? source.sync.email : "",
      secret: typeof source?.sync?.secret === "string" ? source.sync.secret : "",
      syncData: typeof source?.sync?.syncData === "boolean" ? source.sync.syncData : true,
      syncSettings: Boolean(source?.sync?.syncSettings),
      syncSchedules: Boolean(source?.sync?.syncSchedules),
      intervalMinutes:
        typeof source?.sync?.intervalMinutes === "number" && source.sync.intervalMinutes >= 1
          ? Math.min(1440, Math.round(source.sync.intervalMinutes))
          : 5
    },
    updates: {
      autoCheck: typeof source?.updates?.autoCheck === "boolean" ? source.updates.autoCheck : true
    },
    features: {
      showCategoryBadges:
        typeof source?.features?.showCategoryBadges === "boolean"
          ? source.features.showCategoryBadges
          : defaultSettings.features.showCategoryBadges
    }
  };
}

import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import type {
  AppNode,
  AppState,
  DiaryEntry,
  DiaryViewMode,
  LedgerAccount,
  LedgerAccountKind,
  LedgerBook,
  LedgerCategory,
  LedgerEntry,
  LedgerKind,
  LedgerSide,
  LedgerViewMode,
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
  SyncMode,
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
    editorWidthPercent: 72,
    editorHeightPercent: 86,
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
    openSettings: "Ctrl+,",
    syncNow: "F5"
  },
  sync: {
    enabled: false,
    mode: "lan",
    serverUrl: "",
    lanHost: false,
    lanPort: 52177,
    lanName: "",
    lanPeer: "",
    p2pRelay: "",
    p2pDirectory: "",
    username: "",
    secret: "",
    syncData: true,
    syncSettings: true,
    syncSchedules: false,
    syncDiary: true,
    syncLedger: true,
    intervalSeconds: 30,
    reconnectSeconds: 300
  },
  updates: {
    autoCheck: true
  },
  features: {
    showCategoryBadges: true,
    sync: true,
    editorToolbar: true
  },
  diary: {
    view: "list",
    accent: "",
    backgroundColor: "#f4f1ea",
    backgroundImage: "",
    backgroundOpacity: 0.28
  },
  ledger: {
    view: "list",
    accent: "",
    backgroundColor: "#eef3ee",
    backgroundImage: "",
    backgroundOpacity: 0.28
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

/** 新建条目的默认图标（core 的 node.create 用的是同一个值） */
export const DEFAULT_ENTRY_ICON = "check-square";

export function createEntryNode(
  name = "未命名条目",
  parentId: string | null = null,
  icon = DEFAULT_ENTRY_ICON
): AppNode {
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
    cardStyle: source.cardStyle === "card" ? "card" : undefined,
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

const DIARY_DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

/** ISO 时间戳 → 本地日历日 YYYY-MM-DD（解析不出返回空串）。 */
function localDateOf(iso: string): string {
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return "";
  return `${parsed.getFullYear()}-${String(parsed.getMonth() + 1).padStart(2, "0")}-${String(parsed.getDate()).padStart(2, "0")}`;
}

function normalizeDiaryEntry(raw: unknown): DiaryEntry | null {
  const source = raw as Partial<DiaryEntry> | undefined;
  if (!source || typeof source !== "object") return null;
  const title = typeof source.title === "string" ? source.title.trim().slice(0, 120) : "";
  const markdown = typeof source.markdown === "string" ? source.markdown : "";
  // 标题与正文都空的是误操作留下的空壳，不留
  if (!title && !markdown.trim()) return null;
  const createdAt = typeof source.createdAt === "string" ? source.createdAt : now();
  // 日期是日记的骨架：缺失或非法时退回创建那天，再退回今天
  const date = DIARY_DATE_RE.test(source.date ?? "")
    ? (source.date as string)
    : localDateOf(createdAt) || localDateOf(now());
  return {
    id: typeof source.id === "string" && source.id ? source.id : createId("diary"),
    date,
    title,
    markdown,
    mood: typeof source.mood === "string" ? source.mood.trim() : "",
    weather: typeof source.weather === "string" ? source.weather.trim() : "",
    tags: normalizeTags(source.tags),
    expanded: source.expanded === true,
    createdAt,
    updatedAt: typeof source.updatedAt === "string" ? source.updatedAt : undefined
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

/**
 * 通信方式：显式选过就用它，没选过按已有配置推断（填过服务器地址 = 自建服务）。
 * 与 core 的 `SyncSettings::effective_mode` 保持同一口径。
 */
function normalizeSyncMode(raw: unknown, serverUrl: unknown): SyncMode {
  if (raw === "lan" || raw === "server" || raw === "p2p") {
    return raw;
  }
  return typeof serverUrl === "string" && serverUrl.trim() ? "server" : "lan";
}

function normalizeDiaryView(raw: unknown): DiaryViewMode {
  if (raw === "list" || raw === "calendar" || raw === "group") {
    return raw;
  }
  return defaultSettings.diary.view;
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

/**
 * 规范化 diary.json（独立的第四个领域文件）。
 * 日记不挂在任何条目下，所以没有「节点必须存在」那层过滤。
 */
export function normalizeDiaryEntries(raw: unknown): DiaryEntry[] {
  const source = raw as { entries?: unknown } | undefined;
  const list = Array.isArray(source?.entries) ? source?.entries : Array.isArray(raw) ? raw : [];
  return list.map(normalizeDiaryEntry).filter((entry): entry is DiaryEntry => entry !== null);
}

function normalizeLedgerView(raw: unknown): LedgerViewMode {
  if (raw === "list" || raw === "calendar" || raw === "stats" || raw === "assets") {
    return raw;
  }
  return "list";
}

function normalizeLedgerKind(raw: unknown): LedgerKind {
  if (raw === "income" || raw === "transfer") return raw;
  return "expense";
}

function normalizeLedgerSide(raw: unknown): LedgerSide {
  return raw === "income" ? "income" : "expense";
}

function normalizeAccountKind(raw: unknown): LedgerAccountKind {
  if (raw === "debit" || raw === "credit" || raw === "investment" || raw === "other") return raw;
  return "cash";
}

function toCents(raw: unknown): number {
  if (typeof raw === "number" && Number.isFinite(raw)) return Math.round(raw);
  if (typeof raw === "string" && raw.trim() !== "") {
    const parsed = Number.parseFloat(raw);
    if (Number.isFinite(parsed)) return Math.round(parsed);
  }
  return 0;
}

function normalizeLedgerAccount(raw: unknown): LedgerAccount | null {
  const item = raw as Record<string, unknown> | undefined;
  if (!item || typeof item.id !== "string" || item.id === "") return null;
  const name = typeof item.name === "string" ? item.name.trim() : "";
  if (name === "") return null;
  return {
    id: item.id,
    name,
    icon: typeof item.icon === "string" ? item.icon : "",
    color: typeof item.color === "string" ? item.color : "",
    kind: normalizeAccountKind(item.kind),
    initialCents: toCents(item.initialCents),
    note: typeof item.note === "string" ? item.note : "",
    order: typeof item.order === "number" && Number.isFinite(item.order) ? item.order : 0,
    createdAt: typeof item.createdAt === "string" ? item.createdAt : now(),
    updatedAt: typeof item.updatedAt === "string" ? item.updatedAt : undefined
  };
}

function normalizeLedgerCategory(raw: unknown): LedgerCategory | null {
  const item = raw as Record<string, unknown> | undefined;
  if (!item || typeof item.id !== "string" || item.id === "") return null;
  const name = typeof item.name === "string" ? item.name.trim() : "";
  if (name === "") return null;
  return {
    id: item.id,
    name,
    side: normalizeLedgerSide(item.side),
    parentId: typeof item.parentId === "string" && item.parentId !== "" ? item.parentId : undefined,
    icon: typeof item.icon === "string" ? item.icon : "",
    color: typeof item.color === "string" ? item.color : "",
    order: typeof item.order === "number" && Number.isFinite(item.order) ? item.order : 0,
    createdAt: typeof item.createdAt === "string" ? item.createdAt : now(),
    updatedAt: typeof item.updatedAt === "string" ? item.updatedAt : undefined
  };
}

function normalizeLedgerEntry(raw: unknown): LedgerEntry | null {
  const item = raw as Record<string, unknown> | undefined;
  if (!item || typeof item.id !== "string" || item.id === "") return null;
  const amountCents = toCents(item.amountCents);
  if (amountCents <= 0) return null;
  const accountId = typeof item.accountId === "string" ? item.accountId : "";
  if (accountId === "") return null;
  const kind = normalizeLedgerKind(item.kind);
  const date =
    typeof item.date === "string" && /^\d{4}-\d{2}-\d{2}$/.test(item.date)
      ? item.date
      : localDateOf(typeof item.createdAt === "string" ? item.createdAt : "");
  return {
    id: item.id,
    kind,
    amountCents,
    accountId,
    toAccountId:
      kind === "transfer" && typeof item.toAccountId === "string" && item.toAccountId !== ""
        ? item.toAccountId
        : undefined,
    categoryId: typeof item.categoryId === "string" && item.categoryId !== "" ? item.categoryId : undefined,
    date,
    time: typeof item.time === "string" ? item.time : "",
    note: typeof item.note === "string" ? item.note : "",
    createdAt: typeof item.createdAt === "string" ? item.createdAt : now(),
    updatedAt: typeof item.updatedAt === "string" ? item.updatedAt : undefined
  };
}

/**
 * 规范化 ledger.json（独立的第五个领域文件）。
 * 返回整本账：账户 / 分类 / 流水三张表一起给 store，视图层不再各自解析。
 */
export function normalizeLedger(raw: unknown): {
  accounts: LedgerAccount[];
  categories: LedgerCategory[];
  entries: LedgerEntry[];
} {
  const source = raw as Record<string, unknown> | undefined;
  const accounts = Array.isArray(source?.accounts) ? source?.accounts : [];
  const categories = Array.isArray(source?.categories) ? source?.categories : [];
  const entries = Array.isArray(source?.entries) ? source?.entries : [];
  return {
    accounts: accounts
      .map(normalizeLedgerAccount)
      .filter((item): item is LedgerAccount => item !== null),
    categories: categories
      .map(normalizeLedgerCategory)
      .filter((item): item is LedgerCategory => item !== null),
    entries: entries.map(normalizeLedgerEntry).filter((item): item is LedgerEntry => item !== null)
  };
}

/**
 * 浏览器预览首跑的种子账本：与 core 的 `LedgerFile::seed_defaults` 同一套
 * 账户/两级分类（名字、图标、颜色都对齐），只是 id 用前端生成。
 */
export function seedLedgerBook(): LedgerBook {
  const nowIso = now();
  // 种子 id 与 core 的 LedgerFile::seed_defaults 完全一致（确定性，不能随机）：
  // 浏览器预览与桌面 core 模式切换时，同一本账的 id 必须对得上。
  const accounts: LedgerAccount[] = [
    ["lacc-01", "现金", "cash", "Wallet", "#e8a33d"],
    ["lacc-02", "微信", "other", "MessageCircle", "#2aae67"],
    ["lacc-03", "支付宝", "other", "Smartphone", "#1677ff"],
    ["lacc-04", "储蓄卡", "debit", "Landmark", "#b23a48"]
  ].map(([id, name, kind, icon, color], index) => ({
    id,
    name,
    icon,
    color,
    kind: kind as LedgerAccountKind,
    initialCents: 0,
    note: "",
    order: index + 1,
    createdAt: nowIso
  }));

  const categories: LedgerCategory[] = [];
  const group = (
    side: LedgerSide,
    sideTag: string,
    index: number,
    name: string,
    icon: string,
    color: string,
    kids: [string, string][]
  ) => {
    const parentId = `lcat-${sideTag}-${index.toString().padStart(2, "0")}`;
    categories.push({
      id: parentId,
      name,
      side,
      icon,
      color,
      order: index,
      createdAt: nowIso
    });
    kids.forEach(([kid, kidIcon], kidIndex) => {
      categories.push({
        id: `${parentId}-${(kidIndex + 1).toString().padStart(2, "0")}`,
        name: kid,
        side,
        parentId,
        icon: kidIcon,
        color: "",
        order: kidIndex + 1,
        createdAt: nowIso
      });
    });
  };
  group("expense", "exp", 1, "餐饮", "Utensils", "#f0862c", [
    ["早餐", "Coffee"],
    ["午餐", "Utensils"],
    ["晚餐", "UtensilsCrossed"],
    ["零食", "Candy"],
    ["饮料", "CupSoda"],
    ["水果", "Apple"],
    ["买菜", "Carrot"]
  ]);
  group("expense", "exp", 2, "交通", "Bus", "#4a90d9", [
    ["公交地铁", "TrainFront"],
    ["打车", "CarTaxiFront"],
    ["火车飞机", "Plane"],
    ["油费", "Fuel"],
    ["停车", "SquareParking"],
    ["单车", "Bike"]
  ]);
  group("expense", "exp", 3, "居住", "House", "#7f8fa6", [
    ["房租", "KeyRound"],
    ["水电", "Zap"],
    ["燃气", "Flame"],
    ["网费", "Wifi"],
    ["物业维修", "Wrench"]
  ]);
  group("expense", "exp", 4, "购物", "ShoppingBag", "#e67e9c", [
    ["日用百货", "ShoppingCart"],
    ["服饰鞋包", "Shirt"],
    ["数码电器", "Smartphone"],
    ["美妆护肤", "Sparkles"]
  ]);
  group("expense", "exp", 5, "娱乐", "Gamepad2", "#9b59b6", [
    ["游戏", "Gamepad2"],
    ["电影演出", "Clapperboard"],
    ["音乐会员", "Music"],
    ["运动健身", "Dumbbell"]
  ]);
  group("expense", "exp", 6, "医疗", "HeartPulse", "#e74c3c", [
    ["药品", "Pill"],
    ["门诊诊疗", "Stethoscope"]
  ]);
  group("expense", "exp", 7, "学习", "BookOpen", "#16a085", [
    ["书籍课程", "BookOpen"],
    ["学习办公", "PenLine"]
  ]);
  group("expense", "exp", 8, "人情", "Gift", "#d35400", [
    ["红包礼金", "Gift"],
    ["请客吃饭", "PartyPopper"],
    ["孝敬长辈", "HeartHandshake"]
  ]);
  group("expense", "exp", 9, "宠物", "Dog", "#8e6e53", [
    ["宠物食品", "Bone"],
    ["宠物用品", "PawPrint"]
  ]);
  group("expense", "exp", 10, "其他", "Ellipsis", "#95a5a6", [["杂项", "Package"]]);
  group("income", "inc", 1, "工资", "Banknote", "#27ae60", [
    ["工资薪金", "Banknote"],
    ["奖金", "Medal"],
    ["补贴", "Coins"]
  ]);
  group("income", "inc", 2, "理财", "TrendingUp", "#2980b9", [
    ["利息", "Percent"],
    ["基金股票", "ChartLine"]
  ]);
  group("income", "inc", 3, "兼职", "Briefcase", "#8e44ad", [
    ["外快", "Briefcase"],
    ["稿费", "FileText"]
  ]);
  group("income", "inc", 4, "红包", "Gift", "#c0392b", [["红包礼金", "Gift"]]);
  group("income", "inc", 5, "退款", "RotateCcw", "#7f8c8d", [["退款报销", "ReceiptText"]]);
  group("income", "inc", 6, "其他", "Ellipsis", "#95a5a6", [["杂项", "CircleDot"]]);

  return { accounts, categories, entries: [] };
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
    diary?: Partial<Settings["diary"]>;
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
      editorWidthPercent: normalizeFontSize(
        source?.appearance?.editorWidthPercent,
        defaultSettings.appearance.editorWidthPercent,
        30,
        100
      ),
      editorHeightPercent: normalizeFontSize(
        source?.appearance?.editorHeightPercent,
        defaultSettings.appearance.editorHeightPercent,
        30,
        100
      ),
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
      openSettings: shortcutValue("openSettings", defaultSettings.shortcuts.openSettings),
      syncNow: shortcutValue("syncNow", defaultSettings.shortcuts.syncNow)
    },
    sync: {
      enabled: Boolean(source?.sync?.enabled),
      // 用户还没显式选过通信方式时按已有配置推断：填过服务器地址就是「自建服务」，
      // 否则「局域网」。与 core 的 SyncSettings::effective_mode 同一口径，
      // 于是从 v0.5.1 升上来的配置直接可用，不会掉进「还没选主机」的空状态。
      mode: normalizeSyncMode(source?.sync?.mode, source?.sync?.serverUrl),
      serverUrl: typeof source?.sync?.serverUrl === "string" ? source.sync.serverUrl : "",
      lanHost: Boolean(source?.sync?.lanHost),
      lanPort: normalizePositiveInteger(source?.sync?.lanPort, 52177, 1, 65535),
      lanName: typeof source?.sync?.lanName === "string" ? source.sync.lanName : "",
      lanPeer: typeof source?.sync?.lanPeer === "string" ? source.sync.lanPeer : "",
      p2pRelay: typeof source?.sync?.p2pRelay === "string" ? source.sync.p2pRelay : "",
      p2pDirectory: typeof source?.sync?.p2pDirectory === "string" ? source.sync.p2pDirectory : "",
      username: typeof source?.sync?.username === "string" ? source.sync.username : "",
      secret: typeof source?.sync?.secret === "string" ? source.sync.secret : "",
      syncData: typeof source?.sync?.syncData === "boolean" ? source.sync.syncData : true,
      syncSettings: typeof source?.sync?.syncSettings === "boolean" ? source.sync.syncSettings : true,
      syncSchedules: Boolean(source?.sync?.syncSchedules),
      syncDiary: typeof source?.sync?.syncDiary === "boolean" ? source.sync.syncDiary : true,
      syncLedger: typeof source?.sync?.syncLedger === "boolean" ? source.sync.syncLedger : true,
      // 低于下限的间隔按下限生效（用户要的是「至少 5 秒」）
      intervalSeconds:
        typeof source?.sync?.intervalSeconds === "number" && Number.isFinite(source.sync.intervalSeconds)
          ? Math.min(86400, Math.max(5, Math.round(source.sync.intervalSeconds)))
          : 30,
      reconnectSeconds:
        typeof source?.sync?.reconnectSeconds === "number" && Number.isFinite(source.sync.reconnectSeconds)
          ? Math.min(86400, Math.max(5, Math.round(source.sync.reconnectSeconds)))
          : 300
    },
    updates: {
      autoCheck: typeof source?.updates?.autoCheck === "boolean" ? source.updates.autoCheck : true
    },
    features: {
      showCategoryBadges:
        typeof source?.features?.showCategoryBadges === "boolean"
          ? source.features.showCategoryBadges
          : defaultSettings.features.showCategoryBadges,
      sync: typeof source?.features?.sync === "boolean" ? source.features.sync : defaultSettings.features.sync,
      editorToolbar:
        typeof source?.features?.editorToolbar === "boolean"
          ? source.features.editorToolbar
          : defaultSettings.features.editorToolbar
    },
    diary: {
      view: normalizeDiaryView(source?.diary?.view),
      accent: normalizeHexColor(source?.diary?.accent ?? "", ""),
      backgroundColor: normalizeHexColor(source?.diary?.backgroundColor, defaultSettings.diary.backgroundColor),
      backgroundImage: typeof source?.diary?.backgroundImage === "string" ? source.diary.backgroundImage : "",
      backgroundOpacity:
        typeof source?.diary?.backgroundOpacity === "number" && Number.isFinite(source.diary.backgroundOpacity)
          ? Math.min(1, Math.max(0, source.diary.backgroundOpacity))
          : defaultSettings.diary.backgroundOpacity
    },
    ledger: {
      view: normalizeLedgerView(source?.ledger?.view),
      accent: normalizeHexColor(source?.ledger?.accent ?? "", ""),
      backgroundColor: normalizeHexColor(source?.ledger?.backgroundColor, defaultSettings.ledger.backgroundColor),
      backgroundImage: typeof source?.ledger?.backgroundImage === "string" ? source.ledger.backgroundImage : "",
      backgroundOpacity:
        typeof source?.ledger?.backgroundOpacity === "number" && Number.isFinite(source.ledger.backgroundOpacity)
          ? Math.min(1, Math.max(0, source.ledger.backgroundOpacity))
          : defaultSettings.ledger.backgroundOpacity
    }
  };
}

// ---------------------------------------------------------------------------
// 外观缓存（首帧不跳变）
// ---------------------------------------------------------------------------

const APPEARANCE_CACHE_KEY = "kxtodo-appearance-cache";
const CACHED_APPEARANCE_KEYS = ["uiScale", "uiFontSize", "markdownFontSize", "editorFontSize", "tagFontSize"] as const;

/**
 * 水合是异步的（安卓要等 core 把设置读出来），第一帧只能用默认外观渲染，设置到了再跳
 * 到用户定制值——表现成「先渲染一次再缩放，卡卡的」。上一次退出时的外观缓存在
 * localStorage 里，初始 store 直接用它，首帧即到位。
 */
export function cachedAppearance(): Partial<Settings["appearance"]> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(APPEARANCE_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const out: Record<string, number> = {};
    for (const key of CACHED_APPEARANCE_KEYS) {
      const value = parsed[key];
      if (typeof value === "number" && Number.isFinite(value)) out[key] = value;
    }
    return out as Partial<Settings["appearance"]>;
  } catch {
    return {};
  }
}

export function writeAppearanceCache(appearance: Settings["appearance"]): void {
  if (typeof localStorage === "undefined") return;
  try {
    const payload: Record<string, number> = {};
    for (const key of CACHED_APPEARANCE_KEYS) payload[key] = appearance[key];
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify(payload));
  } catch {
    // 写不进（隐私模式等）就算了，最坏退回首帧跳变
  }
}

// ---------------------------------------------------------------------------
// 资料缓存（侧栏首帧不闪 Example User）
// ---------------------------------------------------------------------------

const PROFILE_CACHE_KEY = "kxtodo-profile-cache";
const CACHED_PROFILE_KEYS = ["displayName", "email", "avatar"] as const;

/** 水合前侧栏只能按默认资料渲染一帧（Example User + 默认头像），设置到了再跳——
 * 与外观缓存同一条思路：上一次退出时的资料存 localStorage，初始 store 直接用它。 */
export function cachedProfile(): Partial<Settings["profile"]> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(PROFILE_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const out: Record<string, string> = {};
    for (const key of CACHED_PROFILE_KEYS) {
      const value = parsed[key];
      if (typeof value === "string") out[key] = value;
    }
    return out as Partial<Settings["profile"]>;
  } catch {
    return {};
  }
}

export function writeProfileCache(profile: Settings["profile"]): void {
  if (typeof localStorage === "undefined") return;
  try {
    // 头像是 dataURL 时可能很大：超过上限就不缓存它（最坏退回首帧闪一下头像），
    // 名字邮箱照缓存
    const avatar = profile.avatar.length > 1_500_000 ? "" : profile.avatar;
    localStorage.setItem(
      PROFILE_CACHE_KEY,
      JSON.stringify({ displayName: profile.displayName, email: profile.email, avatar })
    );
  } catch {
    // 写不进就算了
  }
}

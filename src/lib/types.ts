export type NodeKind = "system" | "category" | "entry";

/** 条目渲染类型：todo = 可勾选的待办卡片（默认）；card = 一般卡片（隐藏勾选框，展示型） */
export type CardStyle = "todo" | "card";

export type AppNode = {
  id: string;
  kind: NodeKind;
  name: string;
  icon: string;
  parentId: string | null;
  order?: number;
  collapsed?: boolean;
  cardStyle?: CardStyle;
  createdAt: string;
};

export type TagColor = "red" | "yellow" | "blue" | "green" | "gray";

export type Tag = {
  id: string;
  color: TagColor;
  text?: string;
};

export type Task = {
  id: string;
  nodeId: string;
  order?: number;
  markdown: string;
  completed: boolean;
  important: boolean;
  myDay: boolean;
  plannedDate?: string;
  dueDate?: string;
  completedAt?: string;
  tags: Tag[];
  emojis: string[];
  expanded?: boolean;
  createdAt: string;
  updatedAt?: string;
};

export type EmojiPickerTarget = {
  taskId: string;
  index: number;
};

/** 日记视图：列表（时间轴）/ 日历（月历 + 当日卡片）/ 分组（按年月折叠） */
export type DiaryViewMode = "list" | "calendar" | "group";

export type DiaryEntry = {
  id: string;
  /** 归属日期 YYYY-MM-DD（可以后补写别的日子，不等于 createdAt 的日期） */
  date: string;
  /** 标题；空 = 卡片直接展示正文首行 */
  title: string;
  markdown: string;
  /** 心情 emoji；空 = 没记 */
  mood: string;
  /** 天气 emoji；空 = 没记 */
  weather: string;
  tags: Tag[];
  /** 本机 UI 状态，不参与同步 */
  expanded?: boolean;
  createdAt: string;
  updatedAt?: string;
};

/** 日记编辑器的目标：改已有的一篇（id），或新建一篇归到某天（date）。 */
export type DiaryEditorTarget = { id: string } | { date: string };

/**
 * 全局搜索的一条命中：任务卡片（todo / 一般卡片按所属条目的 cardStyle）或日记卡片。
 * 结果界面按这个 union 混排，两种卡片各走各的组件。
 */
export type SearchHit =
  | { kind: "task"; key: string; task: Task; cardStyle: CardStyle }
  | { kind: "diary"; key: string; entry: DiaryEntry };

export type ProfileSettings = {
  displayName: string;
  email: string;
  avatar: string;
};

export type ThemePreset = {
  name: string;
  color: string;
};

export type SyncMode = "lan" | "server" | "p2p";

export type Settings = {
  profile: ProfileSettings;
  appearance: {
    linkOpenMode: "app" | "system";
    uiScale: number;
    uiFontSize: number;
    markdownFontSize: number;
    editorFontSize: number;
    editorWidthPercent: number;
    editorHeightPercent: number;
    tagFontSize: number;
    themePresets: ThemePreset[];
    uiColors: Record<string, string>;
  };
  lifecycle: {
    closeToTray: boolean;
    launchAtStartup: boolean;
  };
  notifications: {
    durationMs: number;
    position: NotificationPosition;
    width: number;
    height: number;
    titleFontSize: number;
    bodyFontSize: number;
  };
  shortcuts: {
    newTask: string;
    focusSearch: string;
    toggleWindow: string;
    openSettings: string;
    syncNow: string;
  };
  sync: {
    enabled: boolean;
    /** 通信方式：局域网 / 自建服务 / P2P（后续版本）。三种方式共用同一套同步内核 */
    mode: SyncMode;
    /** 自建服务方式的服务器地址 */
    serverUrl: string;
    /** 局域网：本机作为服务器（内置 server 随应用启停；与 lanPeer 二选一） */
    lanHost: boolean;
    /** 局域网：本机作为服务器时的监听端口（被占用会自动向上找） */
    lanPort: number;
    /** 局域网：本机作为服务器时的展示名 = 它在局域网内的身份（要求唯一） */
    lanName: string;
    /** 局域网：选定的远端主机名（从发现列表里点选） */
    lanPeer: string;
    /** P2P 高级覆盖：自部署 iroh relay 地址（空 = n0 免费公共服务；disabled = 不用 relay） */
    p2pRelay: string;
    /** P2P 高级覆盖：自部署 pkarr 目录地址（空 = n0 免费公共服务） */
    p2pDirectory: string;
    username: string;
    secret: string;
    syncData: boolean;
    syncSettings: boolean;
    syncSchedules: boolean;
    intervalSeconds: number;
    reconnectSeconds: number;
  };
  syncUpdatedAt?: string;
  updates: {
    autoCheck: boolean;
  };
  features: {
    showCategoryBadges: boolean;
  };
  /** 日记偏好。view 是本机状态；主题色与背景跟着设置同步走（外观该多端一致）。 */
  diary: {
    view: DiaryViewMode;
    /** 主题色 #rrggbb；空 = 用默认日记色 */
    accent: string;
    backgroundColor: string;
    /** `img:<文件名>` 或 http(s)/data URL；空 = 无图 */
    backgroundImage: string;
    backgroundOpacity: number;
  };
};

export type ListBackground = {
  color: string;
  image?: string;
  imageOpacity?: number;
};

export type SchedulerRuntimeKey = "python" | "node" | "pwsh" | "bash" | "make";

export type SchedulerRuntimePaths = Record<SchedulerRuntimeKey, string>;

export type SchedulerCondition = {
  enabled: boolean;
  mode: "contains" | "regex";
  pattern: string;
};

export type SchedulerScriptLanguage = "python" | "javascript" | "powershell" | "bash" | "makefile" | "custom";

export type NotificationTone = "info" | "success" | "warning" | "error";
export type NotificationPosition = "bottom-right" | "top-right" | "bottom-left" | "top-left";

export type AppNotification = {
  title: string;
  message: string;
  durationMs: number;
  tone: NotificationTone;
  position?: NotificationPosition;
};

export type SchedulerStdoutNotification = {
  enabled: boolean;
  condition: SchedulerCondition;
  notification: AppNotification;
};

export type ScheduledTaskAction = {
  type: "script" | "executable" | "notification";
  scriptMode: "path" | "inline";
  language: SchedulerScriptLanguage;
  interpreter: string;
  filePath: string;
  code: string;
  executablePath: string;
  arguments: string;
  workingDirectory: string;
  notification: AppNotification;
  notifyOnComplete: boolean;
  completionNotification: AppNotification;
  stdoutNotification: SchedulerStdoutNotification;
};

export type ScheduledTaskTrigger = {
  type: "once" | "interval" | "calendar" | "condition";
  runAt: string;
  everySeconds: number;
  repeatCount: number;
  cron: string;
  stopCondition: SchedulerCondition;
  probeAction: ScheduledTaskAction;
  probeCondition: SchedulerCondition;
};

export type ScheduledTaskStatus = "idle" | "running" | "success" | "failed" | "stopped";

export type ScheduledTask = {
  id: string;
  name: string;
  enabled: boolean;
  expanded?: boolean;
  editing?: boolean;
  trigger: ScheduledTaskTrigger;
  action: ScheduledTaskAction;
  runCount: number;
  lastRunAt?: string;
  nextRunAt?: string;
  lastStatus: ScheduledTaskStatus;
  lastExitCode?: number | null;
  lastStdout?: string;
  lastStderr?: string;
  createdAt: string;
  updatedAt?: string;
};

export type SchedulerState = {
  runtimes: SchedulerRuntimePaths;
  tasks: ScheduledTask[];
};

export type AppState = {
  schemaVersion: number;
  nodes: AppNode[];
  tasks: Task[];
  selectedNodeId: string;
  backgrounds: Record<string, ListBackground>;
  scheduler: SchedulerState;
};

export type NodeKind = "system" | "category" | "entry";

export type AppNode = {
  id: string;
  kind: NodeKind;
  name: string;
  icon: string;
  parentId: string | null;
  collapsed?: boolean;
  createdAt: string;
};

export type Task = {
  id: string;
  nodeId: string;
  markdown: string;
  completed: boolean;
  important: boolean;
  myDay: boolean;
  plannedDate?: string;
  dueDate?: string;
  completedAt?: string;
  expanded?: boolean;
  editing?: boolean;
  createdAt: string;
  updatedAt?: string;
};

export type ProfileSettings = {
  displayName: string;
  email: string;
  avatar: string;
};

export type ThemePreset = {
  name: string;
  color: string;
};

export type Settings = {
  profile: ProfileSettings;
  appearance: {
    linkOpenMode: "app" | "system";
    uiScale: number;
    uiFontSize: number;
    markdownFontSize: number;
    editorFontSize: number;
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
  };
  shortcuts: {
    newTask: string;
    focusSearch: string;
    toggleWindow: string;
    openSettings: string;
  };
  cloud: {
    provider: "none" | "webdav" | "s3" | "custom";
    endpoint: string;
    enabled: boolean;
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

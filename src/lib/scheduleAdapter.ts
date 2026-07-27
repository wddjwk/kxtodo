// ---------------------------------------------------------------------------
// v9 ScheduleEntry（spec/state/ui）↔ v8 UI 编辑模型 适配层。
// GUI 编辑器继续工作在熟悉的扁平模型上；保存时生成 SchedulePatch，
// 未映射字段（timeout、missedPolicy、timezone 等 CLI 专属）通过 partial patch 保留。
// ---------------------------------------------------------------------------

import type {
  AppNotification,
  ScheduledTask,
  ScheduledTaskAction,
  ScheduledTaskTrigger,
  SchedulerCondition,
} from "./types";
import { defaultScheduledTaskAction, defaultScheduledTaskTrigger } from "./defaults";

export type ScheduleEntryV9 = {
  id: string;
  spec: any;
  state: {
    runCount: number;
    running?: boolean;
    lastRunAt?: string;
    nextRunAt?: string;
    lastStatus: ScheduledTask["lastStatus"];
    lastExitCode?: number | null;
    lastStdout?: string;
    lastStderr?: string;
  };
  ui: { expanded?: boolean; editing?: boolean };
  createdAt: string;
  updatedAt: string;
};

// ----------------------------- duration ---------------------------------

export function parseDurationSeconds(raw: unknown, fallback = 300): number {
  if (typeof raw !== "string") return fallback;
  const match = raw.trim().match(/^(\d+)(ms|s|m|h|d)$/);
  if (!match) return fallback;
  const amount = Number(match[1]);
  const factor = { ms: 0.001, s: 1, m: 60, h: 3600, d: 86400 }[match[2] as "ms" | "s" | "m" | "h" | "d"];
  return Math.max(1, Math.round(amount * factor));
}

export function secondsToDuration(seconds: number): string {
  const safe = Math.max(1, Math.round(seconds));
  if (safe % 86400 === 0) return `${safe / 86400}d`;
  if (safe % 3600 === 0) return `${safe / 3600}h`;
  if (safe % 60 === 0) return `${safe / 60}m`;
  return `${safe}s`;
}

export function durationToMs(raw: unknown, fallback = 3000): number {
  if (typeof raw !== "string") return fallback;
  const match = raw.trim().match(/^(\d+)(ms|s|m|h|d)$/);
  if (!match) return fallback;
  const amount = Number(match[1]);
  const factor = { ms: 1, s: 1000, m: 60000, h: 3600000, d: 86400000 }[match[2] as "ms" | "s" | "m" | "h" | "d"];
  return amount * factor;
}

// ----------------------------- arguments ---------------------------------

export function splitArguments(raw: string): string[] {
  const args: string[] = [];
  let current = "";
  let inSingle = false;
  let inDouble = false;
  let escaped = false;
  for (const ch of raw) {
    if (escaped) {
      current += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\" && !inSingle) {
      escaped = true;
    } else if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
    } else if (/\s/.test(ch) && !inSingle && !inDouble) {
      if (current) {
        args.push(current);
        current = "";
      }
    } else {
      current += ch;
    }
  }
  if (escaped) current += "\\";
  if (current) args.push(current);
  return args;
}

export function joinArguments(args: unknown): string {
  if (!Array.isArray(args)) return "";
  return args
    .map((arg) => String(arg))
    .map((arg) => (/\s/.test(arg) ? `"${arg.replaceAll('"', '\\"')}"` : arg))
    .join(" ");
}

// ----------------------------- time ---------------------------------

/** "2026-07-31T17:30"（本地墙钟）→ 带时区 ISO。 */
export function localInputToIso(raw: string): string {
  if (!raw) return new Date().toISOString();
  const withSeconds = raw.length === 16 ? `${raw}:00` : raw;
  const date = new Date(withSeconds);
  if (Number.isNaN(date.getTime())) return new Date().toISOString();
  return date.toISOString();
}

/** 带时区 ISO → datetime-local 输入格式。 */
export function isoToLocalInput(iso: unknown): string {
  if (typeof iso !== "string" || !iso) {
    return new Date(Date.now() + 5 * 60_000).toISOString().slice(0, 16);
  }
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso.slice(0, 16);
  const offset = date.getTimezoneOffset();
  return new Date(date.getTime() - offset * 60_000).toISOString().slice(0, 16);
}

// ----------------------------- notifications ---------------------------------

function notificationToUi(raw: any, fallbackMessage: string): AppNotification {
  return {
    title: typeof raw?.title === "string" && raw.title.trim() ? raw.title : "KXToDo",
    message: typeof raw?.message === "string" && raw.message ? raw.message : fallbackMessage,
    durationMs: durationToMs(raw?.duration, 3000),
    tone: raw?.tone ?? "info",
    position: raw?.position,
  };
}

function notificationToV9(ui: AppNotification): Record<string, unknown> {
  const out: Record<string, unknown> = {
    title: ui.title,
    message: ui.message,
    tone: ui.tone,
    duration: `${Math.max(1200, Math.min(60000, Math.round(ui.durationMs)))}ms`,
  };
  if (ui.position) {
    out.position = ui.position;
  }
  return out;
}

function conditionToMatch(condition: SchedulerCondition): Record<string, unknown> | null {
  if (!condition.enabled || !condition.pattern.trim()) {
    return null;
  }
  return { stream: "stdout", mode: condition.mode, pattern: condition.pattern };
}

function matchToCondition(match: any): SchedulerCondition {
  return {
    enabled: Boolean(match && match.pattern),
    mode: match?.mode === "regex" ? "regex" : "contains",
    pattern: typeof match?.pattern === "string" ? match.pattern : "",
  };
}

// ----------------------------- entry → ui ---------------------------------

export function entryToUi(entry: ScheduleEntryV9): ScheduledTask {
  const spec = entry.spec ?? {};
  const trigger = spec.trigger ?? { type: "once" };
  const action = spec.action ?? { type: "script" };
  const uiTrigger = defaultScheduledTaskTrigger(trigger.type);
  switch (trigger.type) {
    case "once":
      uiTrigger.runAt = isoToLocalInput(trigger.at);
      break;
    case "interval":
      uiTrigger.everySeconds = parseDurationSeconds(trigger.every, 300);
      uiTrigger.repeatCount = typeof trigger.maxRuns === "number" ? trigger.maxRuns : 0;
      uiTrigger.stopCondition = matchToCondition(trigger.stopWhen);
      break;
    case "calendar":
      uiTrigger.cron = trigger.cron ?? "0 9 * * *";
      break;
    case "condition":
      uiTrigger.everySeconds = parseDurationSeconds(trigger.every, 60);
      uiTrigger.probeCondition = matchToCondition(trigger.when);
      uiTrigger.probeAction = probeToUi(trigger.probe);
      break;
  }
  return {
    id: entry.id,
    name: spec.name ?? "未命名定时任务",
    enabled: Boolean(spec.enabled),
    expanded: entry.ui?.expanded ?? false,
    editing: entry.ui?.editing ?? false,
    trigger: uiTrigger,
    action: actionToUi(action),
    runCount: entry.state?.runCount ?? 0,
    lastRunAt: entry.state?.lastRunAt,
    nextRunAt: entry.state?.nextRunAt,
    lastStatus: entry.state?.lastStatus ?? "idle",
    lastExitCode: entry.state?.lastExitCode ?? undefined,
    lastStdout: entry.state?.lastStdout ?? "",
    lastStderr: entry.state?.lastStderr ?? "",
    createdAt: entry.createdAt,
    updatedAt: entry.updatedAt,
  };
}

function probeToUi(probe: any): ScheduledTaskAction {
  const ui = defaultScheduledTaskAction("python");
  if (!probe) return ui;
  if (probe.type === "script") {
    ui.type = "script";
    ui.language = probe.language ?? "python";
    if (probe.source?.type === "file") {
      ui.scriptMode = "path";
      ui.filePath = probe.source.path ?? "";
      ui.code = "";
    } else {
      ui.scriptMode = "inline";
      ui.code = probe.source?.code ?? "";
      ui.filePath = "";
    }
    ui.interpreter = probe.interpreter ?? "";
    ui.arguments = joinArguments(probe.args);
    ui.workingDirectory = probe.workingDirectory ?? "";
  } else if (probe.type === "executable") {
    ui.type = "executable";
    ui.executablePath = probe.program ?? "";
    ui.arguments = joinArguments(probe.args);
    ui.workingDirectory = probe.workingDirectory ?? "";
  }
  ui.notifyOnComplete = false;
  ui.stdoutNotification.enabled = false;
  return ui;
}

function actionToUi(action: any): ScheduledTaskAction {
  const ui = defaultScheduledTaskAction(action?.language ?? "python");
  if (!action) return ui;
  if (action.type === "notification") {
    ui.type = "notification";
    ui.notification = notificationToUi(action.notification, "定时任务已触发");
  } else if (action.type === "executable") {
    ui.type = "executable";
    ui.executablePath = action.program ?? "";
    ui.arguments = joinArguments(action.args);
    ui.workingDirectory = action.workingDirectory ?? "";
  } else {
    ui.type = "script";
    ui.language = action.language ?? "python";
    if (action.source?.type === "file") {
      ui.scriptMode = "path";
      ui.filePath = action.source.path ?? "";
      ui.code = "";
    } else {
      ui.scriptMode = "inline";
      ui.code = action.source?.code ?? "";
      ui.filePath = "";
    }
    ui.interpreter = action.interpreter ?? "";
    ui.arguments = joinArguments(action.args);
    ui.workingDirectory = action.workingDirectory ?? "";
  }
  const notifications = action.notifications;
  ui.notifyOnComplete = Boolean(notifications?.onComplete);
  if (notifications?.onComplete) {
    ui.completionNotification = notificationToUi(notifications.onComplete, "任务 {taskName} 执行完成\n{stdout}");
  }
  if (notifications?.onOutput) {
    ui.stdoutNotification = {
      enabled: true,
      condition: matchToCondition(notifications.onOutput.when),
      notification: notificationToUi(notifications.onOutput.notification, "stdout 匹配成功：\n{stdout}"),
    };
  } else {
    ui.stdoutNotification.enabled = false;
  }
  return ui;
}

// ----------------------------- ui → spec/patch ---------------------------------

function buildSource(ui: ScheduledTaskAction): Record<string, unknown> {
  if (ui.scriptMode === "path") {
    return { type: "file", path: ui.filePath };
  }
  return { type: "inline", code: ui.code };
}

function buildActionNotifications(ui: ScheduledTaskAction): Record<string, unknown> | undefined {
  const notifications: Record<string, unknown> = {};
  if (ui.notifyOnComplete) {
    notifications.onComplete = notificationToV9(ui.completionNotification);
  }
  if (ui.stdoutNotification.enabled) {
    const when = conditionToMatch(ui.stdoutNotification.condition);
    if (when) {
      notifications.onOutput = { when, notification: notificationToV9(ui.stdoutNotification.notification) };
    }
  }
  return Object.keys(notifications).length > 0 ? notifications : undefined;
}

function buildActionSpec(ui: ScheduledTaskAction): Record<string, unknown> {
  if (ui.type === "notification") {
    return { type: "notification", notification: notificationToV9(ui.notification) };
  }
  if (ui.type === "executable") {
    const action: Record<string, unknown> = {
      type: "executable",
      program: ui.executablePath,
    };
    const args = splitArguments(ui.arguments);
    if (args.length > 0) action.args = args;
    if (ui.workingDirectory.trim()) action.workingDirectory = ui.workingDirectory;
    const notifications = buildActionNotifications(ui);
    if (notifications) action.notifications = notifications;
    return action;
  }
  const action: Record<string, unknown> = {
    type: "script",
    language: ui.language === "custom" ? "python" : ui.language,
    source: buildSource(ui),
  };
  const args = splitArguments(ui.arguments);
  if (args.length > 0) action.args = args;
  if (ui.interpreter.trim()) action.interpreter = ui.interpreter;
  if (ui.workingDirectory.trim()) action.workingDirectory = ui.workingDirectory;
  const notifications = buildActionNotifications(ui);
  if (notifications) action.notifications = notifications;
  return action;
}

function buildProbeSpec(ui: ScheduledTaskAction): Record<string, unknown> {
  const action = buildActionSpec(ui);
  delete action.notifications;
  return action;
}

function buildTriggerSpec(ui: ScheduledTaskTrigger): Record<string, unknown> {
  switch (ui.type) {
    case "once":
      return { type: "once", at: localInputToIso(ui.runAt) };
    case "interval": {
      const trigger: Record<string, unknown> = {
        type: "interval",
        every: secondsToDuration(ui.everySeconds),
      };
      if (ui.repeatCount > 0) trigger.maxRuns = ui.repeatCount;
      const stopWhen = conditionToMatch(ui.stopCondition);
      if (stopWhen) trigger.stopWhen = stopWhen;
      return trigger;
    }
    case "calendar":
      return {
        type: "calendar",
        cron: ui.cron,
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
      };
    case "condition": {
      const when = conditionToMatch({ ...ui.probeCondition, enabled: true });
      return {
        type: "condition",
        every: secondsToDuration(ui.everySeconds),
        probe: buildProbeSpec(ui.probeAction),
        when: when ?? { stream: "stdout", mode: ui.probeCondition.mode, pattern: ui.probeCondition.pattern || "READY" },
      };
    }
  }
}

/** 编辑保存：以 entry.spec 当前类型为准生成 partial patch（保留未映射字段）。 */
export function uiToPatch(ui: ScheduledTask, entry: ScheduleEntryV9): Record<string, unknown> {
  const patch: Record<string, unknown> = {};
  const spec = entry.spec ?? {};
  if (ui.name !== spec.name) {
    patch.name = ui.name;
  }
  if (ui.trigger.type !== spec.trigger?.type) {
    patch.trigger = buildTriggerSpec(ui.trigger);
  } else {
    const trigger: Record<string, unknown> = {};
    switch (ui.trigger.type) {
      case "once":
        trigger.at = localInputToIso(ui.trigger.runAt);
        break;
      case "interval": {
        trigger.every = secondsToDuration(ui.trigger.everySeconds);
        trigger.maxRuns = ui.trigger.repeatCount > 0 ? ui.trigger.repeatCount : null;
        trigger.stopWhen = conditionToMatch(ui.trigger.stopCondition);
        break;
      }
      case "calendar":
        trigger.cron = ui.trigger.cron;
        break;
      case "condition": {
        trigger.every = secondsToDuration(ui.trigger.everySeconds);
        trigger.probe = buildProbeSpec(ui.trigger.probeAction);
        const when = conditionToMatch({ ...ui.trigger.probeCondition, enabled: true });
        trigger.when = when ?? { stream: "stdout", mode: ui.trigger.probeCondition.mode, pattern: ui.trigger.probeCondition.pattern || "READY" };
        break;
      }
    }
    if (Object.keys(trigger).length > 0) {
      patch.trigger = trigger;
    }
  }
  if (ui.action.type !== spec.action?.type) {
    patch.action = buildActionSpec(ui.action);
  } else {
    const action: Record<string, unknown> = {};
    if (ui.action.type === "notification") {
      action.notification = notificationToV9(ui.action.notification);
    } else if (ui.action.type === "executable") {
      action.program = ui.action.executablePath;
      action.args = splitArguments(ui.action.arguments);
      action.workingDirectory = ui.action.workingDirectory.trim() || null;
      action.notifications = buildActionNotifications(ui.action) ?? null;
    } else {
      action.language = ui.action.language === "custom" ? "python" : ui.action.language;
      action.source = buildSource(ui.action);
      action.args = splitArguments(ui.action.arguments);
      action.interpreter = ui.action.interpreter.trim() || null;
      action.workingDirectory = ui.action.workingDirectory.trim() || null;
      action.notifications = buildActionNotifications(ui.action) ?? null;
    }
    patch.action = action;
  }
  return patch;
}

/** 新建：由 UI 模型生成完整 spec。 */
export function uiToSpec(ui: ScheduledTask): Record<string, unknown> {
  return {
    name: ui.name,
    enabled: ui.enabled,
    trigger: buildTriggerSpec(ui.trigger),
    action: buildActionSpec(ui.action),
  };
}

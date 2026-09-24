// ScheduleEntry (spec/state/ui) ↔ editor. Diff the two UI projections, never
// rewrite untouched source fields (timezone, subsecond durations, ISO precision, etc.).
import type { AppNotification, ScheduledTask, ScheduledTaskAction, ScheduledTaskTrigger, SchedulerCondition } from "./types";
import { defaultScheduledTaskAction, defaultScheduledTaskTrigger } from "./defaults";

export type ScheduleEntryV9 = {
  id: string;
  spec: any;
  state: {
    runCount: number; running?: boolean; lastRunAt?: string; nextRunAt?: string;
    lastStatus: ScheduledTask["lastStatus"]; lastExitCode?: number | null;
    lastStdout?: string; lastStderr?: string;
  };
  ui: { expanded?: boolean; editing?: boolean };
  createdAt: string;
  updatedAt: string;
};

export function durationToMs(raw: unknown, fallback = 3000): number {
  if (typeof raw !== "string") return fallback;
  const match = raw.trim().match(/^(\d+)(ms|s|m|h|d)$/);
  if (!match) return fallback;
  const factors: Record<string, number> = { ms: 1, s: 1000, m: 60000, h: 3600000, d: 86400000 };
  const value = Number(match[1]) * factors[match[2]];
  return Number.isSafeInteger(value) && value > 0 ? value : fallback;
}
export function parseDurationSeconds(raw: unknown, fallback = 300): number {
  return durationToMs(raw, fallback * 1000) / 1000;
}
export function secondsToDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0 || !Number.isSafeInteger(seconds * 1000)) throw new Error("间隔必须为有效正数");
  for (const [factor, unit] of [[86400, "d"], [3600, "h"], [60, "m"], [1, "s"]] as const) {
    if (seconds % factor === 0) return `${seconds / factor}${unit}`;
  }
  return `${seconds * 1000}ms`;
}

/** Shell-style quoted arguments, preserving empty arguments and Windows paths. */
export function splitArguments(raw: string): string[] {
  const args: string[] = [];
  let current = "", quote = "", started = false;
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "\\" && quote !== "'" && i + 1 < raw.length && /[\\"\s]/.test(raw[i + 1])) {
      current += raw[++i]; started = true;
    } else if ((ch === '"' || ch === "'") && (!quote || quote === ch)) {
      quote = quote ? "" : ch; started = true;
    } else if (/\s/.test(ch) && !quote) {
      if (started) args.push(current);
      current = ""; started = false;
    } else { current += ch; started = true; }
  }
  if (quote) throw new Error("参数的引号尚未闭合");
  if (started) args.push(current);
  return args;
}
export function joinArguments(args: unknown): string {
  if (!Array.isArray(args)) return "";
  return args.map(String).map((arg) => !arg || /[\s\\"']/.test(arg)
    ? `"${arg.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"` : arg).join(" ");
}
export function localInputToIso(raw: string): string {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d{1,3})?)?$/.test(raw)) throw new Error("请选择有效的触发时间");
  const date = new Date(raw);
  if (!Number.isFinite(date.getTime()) || isoToLocalInput(date.toISOString()) !== raw.slice(0, 16)) throw new Error("触发时间不存在或无效");
  return date.toISOString();
}
export function isoToLocalInput(iso: unknown): string {
  const date = typeof iso === "string" && iso ? new Date(iso) : new Date(Date.now() + 300000);
  if (!Number.isFinite(date.getTime())) return typeof iso === "string" ? iso : "";
  return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
}

function notificationToUi(raw: any, fallback: string): AppNotification {
  return { title: raw?.title ?? "KXToDo", message: raw?.message ?? fallback,
    durationMs: durationToMs(raw?.duration, 3000), tone: raw?.tone ?? "info", position: raw?.position };
}
function notificationToSpec(ui: AppNotification): Record<string, unknown> {
  return { title: ui.title, message: ui.message, tone: ui.tone, duration: `${ui.durationMs}ms`,
    ...(ui.position ? { position: ui.position } : {}) };
}
function conditionToMatch(ui: SchedulerCondition): Record<string, unknown> | undefined {
  return ui.enabled ? { stream: ui.stream ?? "stdout", mode: ui.mode, pattern: ui.pattern } : undefined;
}
function matchToCondition(raw: any): SchedulerCondition {
  return { enabled: Boolean(raw), mode: raw?.mode === "regex" ? "regex" : "contains",
    pattern: raw?.pattern ?? "", stream: raw?.stream === "stderr" ? "stderr" : "stdout" };
}
function actionToUi(raw: any, probe = false): ScheduledTaskAction {
  const ui = defaultScheduledTaskAction(raw?.language ?? "python");
  if (!raw) return ui;
  ui.type = raw.type;
  ui.timeout = raw.timeout;
  ui.arguments = joinArguments(raw.args);
  ui.workingDirectory = raw.workingDirectory ?? "";
  ui.interpreter = raw.interpreter ?? "";
  ui.executablePath = raw.program ?? "";
  if (raw.type === "notification") ui.notification = notificationToUi(raw.notification, "定时任务已触发");
  if (raw.source?.type === "file") { ui.scriptMode = "path"; ui.filePath = raw.source.path; ui.code = ""; }
  else if (raw.source) { ui.scriptMode = "inline"; ui.code = raw.source.code; ui.filePath = ""; }
  ui.notifyOnComplete = !probe && Boolean(raw.notifications?.onComplete);
  if (ui.notifyOnComplete) ui.completionNotification = notificationToUi(raw.notifications.onComplete, "任务执行完成");
  if (!probe && raw.notifications?.onOutput) ui.stdoutNotification = {
    enabled: true, condition: matchToCondition(raw.notifications.onOutput.when),
    notification: notificationToUi(raw.notifications.onOutput.notification, "输出匹配成功")
  };
  return ui;
}
export function entryToUi(entry: ScheduleEntryV9): ScheduledTask {
  const spec = entry.spec ?? {};
  const trigger = spec.trigger ?? { type: "once" };
  const ui = defaultScheduledTaskTrigger(trigger.type);
  ui.missedPolicy = trigger.missedPolicy;
  if (trigger.type === "once") ui.runAt = isoToLocalInput(trigger.at);
  if (trigger.type === "interval" || trigger.type === "condition") ui.everySeconds = parseDurationSeconds(trigger.every);
  if (trigger.type === "interval") { ui.repeatCount = trigger.maxRuns ?? 0; ui.stopCondition = matchToCondition(trigger.stopWhen); }
  if (trigger.type === "calendar") { ui.cron = trigger.cron; ui.timezone = trigger.timezone; }
  if (trigger.type === "condition") {
    ui.probeCondition = matchToCondition(trigger.when); ui.probeAction = actionToUi(trigger.probe, true); ui.cooldown = trigger.cooldown;
  }
  return {
    id: entry.id, name: spec.name ?? "未命名定时任务", enabled: Boolean(spec.enabled),
    expanded: entry.ui?.expanded ?? false, editing: entry.ui?.editing ?? false,
    trigger: ui, action: actionToUi(spec.action), until: spec.until,
    gate: spec.gate ? {
      windows: (spec.gate.windows ?? []).map((w: any) => ({ ...w, ...(w.weekdays ? { weekdays: [...w.weekdays] } : {}) })),
      probeAction: spec.gate.probe ? actionToUi(spec.gate.probe, true) : undefined,
      condition: spec.gate.when ? matchToCondition(spec.gate.when) : undefined
    } : undefined,
    runCount: entry.state?.runCount ?? 0, lastRunAt: entry.state?.lastRunAt, nextRunAt: entry.state?.nextRunAt,
    lastStatus: entry.state?.lastStatus ?? "idle", lastExitCode: entry.state?.lastExitCode,
    lastStdout: entry.state?.lastStdout ?? "", lastStderr: entry.state?.lastStderr ?? "",
    createdAt: entry.createdAt, updatedAt: entry.updatedAt
  };
}
function buildAction(ui: ScheduledTaskAction, probe = false): Record<string, unknown> {
  if (ui.type === "notification") return { type: "notification", notification: notificationToSpec(ui.notification) };
  const action: Record<string, unknown> = ui.type === "executable"
    ? { type: "executable", program: ui.executablePath }
    : { type: "script", language: ui.language === "custom" ? "python" : ui.language,
        source: ui.scriptMode === "path" ? { type: "file", path: ui.filePath } : { type: "inline", code: ui.code },
        ...(ui.interpreter.trim() ? { interpreter: ui.interpreter } : {}) };
  const args = splitArguments(ui.arguments);
  if (args.length) action.args = args;
  if (ui.workingDirectory.trim()) action.workingDirectory = ui.workingDirectory;
  if (ui.timeout?.trim()) action.timeout = ui.timeout.trim();
  const notifications: Record<string, unknown> = {};
  if (!probe && ui.notifyOnComplete) notifications.onComplete = notificationToSpec(ui.completionNotification);
  if (!probe && ui.stdoutNotification.enabled) notifications.onOutput = {
    when: conditionToMatch({ ...ui.stdoutNotification.condition, enabled: true }),
    notification: notificationToSpec(ui.stdoutNotification.notification)
  };
  if (Object.keys(notifications).length) action.notifications = notifications;
  return action;
}
function buildTrigger(ui: ScheduledTaskTrigger): Record<string, unknown> {
  const trigger: Record<string, unknown> = { type: ui.type };
  if (ui.missedPolicy) trigger.missedPolicy = ui.missedPolicy;
  switch (ui.type) {
    case "once": trigger.at = localInputToIso(ui.runAt); break;
    case "interval":
      trigger.every = secondsToDuration(ui.everySeconds);
      if (ui.repeatCount > 0) trigger.maxRuns = ui.repeatCount;
      if (ui.stopCondition.enabled) trigger.stopWhen = conditionToMatch(ui.stopCondition);
      break;
    case "calendar":
      trigger.cron = ui.cron;
      trigger.timezone = ui.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone ?? "UTC";
      break;
    case "condition":
      trigger.every = secondsToDuration(ui.everySeconds);
      trigger.probe = buildAction(ui.probeAction, true);
      trigger.when = conditionToMatch({ ...ui.probeCondition, enabled: true });
      if (ui.cooldown !== undefined) trigger.cooldown = ui.cooldown;
      break;
  }
  return trigger;
}
export function uiToSpec(ui: ScheduledTask): Record<string, unknown> {
  return {
    name: ui.name, enabled: ui.enabled, trigger: buildTrigger(ui.trigger), action: buildAction(ui.action),
    ...(ui.until ? { until: ui.until } : {}),
    ...(ui.gate ? { gate: {
      windows: ui.gate.windows,
      ...(ui.gate.probeAction ? { probe: buildAction(ui.gate.probeAction, true),
        when: ui.gate.condition ? conditionToMatch({ ...ui.gate.condition, enabled: true }) : undefined } : {})
    } } : {})
  };
}
function diffSpec(before: any, after: any): any {
  if (JSON.stringify(before) === JSON.stringify(after)) return undefined;
  if (after === undefined) return null;
  if (!before || !after || typeof before !== "object" || typeof after !== "object"
    || Array.isArray(before) || Array.isArray(after) || before.type !== after.type) return after;
  const patch: Record<string, unknown> = {};
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    const value = diffSpec(before[key], after[key]);
    if (value !== undefined) patch[key] = value;
  }
  return patch;
}
export function uiToPatch(ui: ScheduledTask, entry: ScheduleEntryV9): Record<string, unknown> {
  return diffSpec(uiToSpec(entryToUi(entry)), uiToSpec(ui)) ?? {};
}

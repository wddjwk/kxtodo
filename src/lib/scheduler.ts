import { get } from "svelte/store";
import { isTauriRuntime, runScheduledAction, stopScheduledAction, type ScheduledActionOutput } from "./backend";
import { appState, commitScheduler, now, showNotification, showToast } from "./stores";
import type { AppNotification, ScheduledTask, ScheduledTaskAction, SchedulerCondition, SchedulerState } from "./types";

const MIN_DELAY_MS = 1_000;
const MAX_DELAY_MS = 60_000;
const MAX_CRON_SCAN_MINUTES = 366 * 24 * 60;

const running = new Set<string>();
const cancellationRequested = new Set<string>();
const lastProbeAt = new Map<string, number>();
let timer: number | undefined;
let unsubscribeState: (() => void) | undefined;
let rescheduleQueued = false;

export function startSchedulerRuntime(): () => void {
  stopSchedulerRuntime();
  if (!isTauriRuntime) {
    return stopSchedulerRuntime;
  }
  unsubscribeState = appState.subscribe(() => queueReschedule());
  queueReschedule();
  return stopSchedulerRuntime;
}

export function stopSchedulerRuntime(): void {
  if (timer !== undefined) {
    window.clearTimeout(timer);
    timer = undefined;
  }
  unsubscribeState?.();
  unsubscribeState = undefined;
  rescheduleQueued = false;
}

export function stopScheduledTask(taskId: string, message = "用户已终止任务"): void {
  cancellationRequested.add(taskId);
  void stopScheduledAction(taskId).catch((error) => {
    showToast(`终止定时任务失败：${String(error)}`);
  });
  stopTask(taskId, message);
  queueReschedule();
}

function queueReschedule(): void {
  if (rescheduleQueued) {
    return;
  }
  rescheduleQueued = true;
  queueMicrotask(() => {
    rescheduleQueued = false;
    scheduleNext();
  });
}

function scheduleNext(): void {
  if (timer !== undefined) {
    window.clearTimeout(timer);
    timer = undefined;
  }
  const delay = nextSchedulerDelay();
  if (delay === null) {
    return;
  }
  timer = window.setTimeout(() => {
    timer = undefined;
    void schedulerTick().finally(queueReschedule);
  }, delay);
}

function nextSchedulerDelay(): number | null {
  const state = get(appState);
  const current = new Date();
  const nowMs = current.getTime();
  let nextAt = Number.POSITIVE_INFINITY;

  for (const task of state.scheduler.tasks) {
    if (!task.enabled || running.has(task.id)) {
      continue;
    }
    const dueAt = nextDueTime(task, current);
    if (dueAt !== null) {
      nextAt = Math.min(nextAt, dueAt);
    }
  }

  if (!Number.isFinite(nextAt)) {
    return null;
  }
  const delay = Math.max(0, nextAt - nowMs);
  return Math.min(MAX_DELAY_MS, Math.max(delay, delay === 0 ? 0 : MIN_DELAY_MS));
}

function nextDueTime(task: ScheduledTask, current: Date): number | null {
  switch (task.trigger.type) {
    case "once": {
      if (task.runCount > 0) {
        return null;
      }
      return parseDateTime(task.trigger.runAt)?.getTime() ?? null;
    }
    case "interval": {
      if (task.trigger.repeatCount > 0 && task.runCount >= task.trigger.repeatCount) {
        return current.getTime();
      }
      const base = parseDateTime(task.lastRunAt ?? task.updatedAt ?? task.createdAt) ?? current;
      return base.getTime() + task.trigger.everySeconds * 1000;
    }
    case "calendar": {
      if (isTaskDue(task, current)) {
        return current.getTime();
      }
      return nextCronTime(task.trigger.cron, current);
    }
    case "condition": {
      const previous = lastProbeAt.get(task.id);
      return previous ? previous + task.trigger.everySeconds * 1000 : current.getTime();
    }
    default:
      return null;
  }
}

async function schedulerTick(): Promise<void> {
  const state = get(appState);
  const current = new Date();
  for (const task of state.scheduler.tasks) {
    if (!task.enabled || running.has(task.id)) {
      continue;
    }
    if (task.trigger.type === "condition") {
      if (isConditionProbeDue(task, current)) {
        void probeAndMaybeRun(task);
      }
      continue;
    }
    if (isTaskDue(task, current)) {
      void runMainAction(task);
    }
  }
}

function isTaskDue(task: ScheduledTask, current: Date): boolean {
  switch (task.trigger.type) {
    case "once": {
      const runAt = parseDateTime(task.trigger.runAt);
      return task.runCount === 0 && runAt !== null && runAt.getTime() <= current.getTime();
    }
    case "interval": {
      if (task.trigger.repeatCount > 0 && task.runCount >= task.trigger.repeatCount) {
        stopTask(task.id, "已达到重复次数");
        return false;
      }
      const base = parseDateTime(task.lastRunAt ?? task.updatedAt ?? task.createdAt) ?? current;
      return current.getTime() - base.getTime() >= task.trigger.everySeconds * 1000;
    }
    case "calendar":
      return cronMatches(task.trigger.cron, current) && minuteKey(task.lastRunAt) !== minuteKey(current);
    default:
      return false;
  }
}

function isConditionProbeDue(task: ScheduledTask, current: Date): boolean {
  const previous = lastProbeAt.get(task.id);
  if (!previous) {
    return true;
  }
  return current.getTime() - previous >= task.trigger.everySeconds * 1000;
}

async function runMainAction(task: ScheduledTask): Promise<void> {
  running.add(task.id);
  cancellationRequested.delete(task.id);
  markTaskRunning(task.id);
  try {
    const output = await executeTaskAction(task, task.action);
    if (isTaskCancellationCurrent(task.id)) {
      return;
    }
    const timestamp = now();
    const shouldStopByStdout = task.trigger.type === "interval" && conditionMatches(task.trigger.stopCondition, output.stdout);
    const nextRunCount = task.runCount + 1;
    const reachedRepeatCount = task.trigger.type === "interval" && task.trigger.repeatCount > 0 && nextRunCount >= task.trigger.repeatCount;
    const shouldDisable = task.trigger.type === "once" || shouldStopByStdout || reachedRepeatCount;
    updateScheduler((scheduler) => ({
      ...scheduler,
      tasks: scheduler.tasks.map((item) => item.id === task.id
        ? {
            ...item,
            enabled: shouldDisable ? false : item.enabled,
            runCount: nextRunCount,
            lastRunAt: timestamp,
            nextRunAt: shouldDisable ? undefined : nextRunAt(item, timestamp),
            lastStatus: shouldStopByStdout || reachedRepeatCount ? "stopped" : output.exitCode === 0 ? "success" : "failed",
            lastExitCode: output.exitCode,
            lastStdout: output.stdout,
            lastStderr: output.stderr,
            updatedAt: timestamp
          }
        : item)
    }));
  } catch (error) {
    if (isTaskCancellationCurrent(task.id)) {
      return;
    }
    const timestamp = now();
    updateTask(task.id, {
      lastRunAt: timestamp,
      lastStatus: "failed",
      lastStderr: String(error),
      updatedAt: timestamp
    });
    showToast(`定时任务执行失败：${task.name}`);
  } finally {
    running.delete(task.id);
    cancellationRequested.delete(task.id);
    queueReschedule();
  }
}

async function probeAndMaybeRun(task: ScheduledTask): Promise<void> {
  running.add(task.id);
  cancellationRequested.delete(task.id);
  markTaskRunning(task.id);
  lastProbeAt.set(task.id, Date.now());
  try {
    const probe = await runScheduledAction(task.trigger.probeAction, get(appState).scheduler.runtimes, task.id);
    if (isTaskCancellationCurrent(task.id)) {
      return;
    }
    if (!conditionMatches(task.trigger.probeCondition, probe.stdout)) {
      updateTask(task.id, {
        lastStatus: "idle",
        lastExitCode: probe.exitCode,
        lastStdout: probe.stdout,
        lastStderr: probe.stderr,
        updatedAt: now()
      });
      return;
    }

    const output = await executeTaskAction(task, task.action);
    if (isTaskCancellationCurrent(task.id)) {
      return;
    }
    const timestamp = now();
    updateTask(task.id, {
      enabled: false,
      runCount: task.runCount + 1,
      lastRunAt: timestamp,
      nextRunAt: undefined,
      lastStatus: output.exitCode === 0 ? "success" : "failed",
      lastExitCode: output.exitCode,
      lastStdout: output.stdout,
      lastStderr: output.stderr,
      updatedAt: timestamp
    });
  } catch (error) {
    if (isTaskCancellationCurrent(task.id)) {
      return;
    }
    updateTask(task.id, {
      lastStatus: "failed",
      lastStderr: String(error),
      updatedAt: now()
    });
    showToast(`条件定时任务执行失败：${task.name}`);
  } finally {
    running.delete(task.id);
    cancellationRequested.delete(task.id);
    queueReschedule();
  }
}

async function executeTaskAction(task: ScheduledTask, action: ScheduledTaskAction): Promise<ScheduledActionOutput> {
  if (action.type === "notification") {
    await dispatchNotification(task, action.notification, {
      stdout: "",
      stderr: "",
      exitCode: ""
    });
    return { exitCode: 0, stdout: "", stderr: "" };
  }

  const output = await runScheduledAction(action, get(appState).scheduler.runtimes, task.id);
  await dispatchActionNotifications(task, action, output);
  return output;
}

function isTaskCancellationCurrent(taskId: string): boolean {
  if (cancellationRequested.has(taskId)) {
    return true;
  }
  const task = get(appState).scheduler.tasks.find((item) => item.id === taskId);
  return task?.lastStatus === "stopped" && !task.enabled;
}

async function dispatchActionNotifications(task: ScheduledTask, action: ScheduledTaskAction, output: ScheduledActionOutput): Promise<void> {
  const replacements = {
    stdout: output.stdout,
    stderr: output.stderr,
    exitCode: output.exitCode === null ? "" : String(output.exitCode)
  };
  if (action.notifyOnComplete) {
    await dispatchNotification(task, action.completionNotification, replacements);
  }
  if (action.stdoutNotification.enabled && conditionMatches(action.stdoutNotification.condition, output.stdout)) {
    await dispatchNotification(task, action.stdoutNotification.notification, replacements);
  }
}

async function dispatchNotification(
  task: ScheduledTask,
  notification: AppNotification,
  replacements: { stdout: string; stderr: string; exitCode: string }
): Promise<void> {
  await showNotification(renderNotificationText(notification.message, task, replacements), {
    title: renderNotificationText(notification.title, task, replacements),
    tone: notification.tone,
    durationMs: notification.durationMs
  });
}

function renderNotificationText(
  template: string,
  task: ScheduledTask,
  replacements: { stdout: string; stderr: string; exitCode: string }
): string {
  return template
    .replaceAll("{stdout}", replacements.stdout)
    .replaceAll("{stderr}", replacements.stderr)
    .replaceAll("{exitCode}", replacements.exitCode)
    .replaceAll("{taskName}", task.name);
}

function updateTask(taskId: string, patch: Partial<ScheduledTask>): void {
  updateScheduler((scheduler) => ({
    ...scheduler,
    tasks: scheduler.tasks.map((task) => task.id === taskId ? { ...task, ...patch } : task)
  }));
}

function updateScheduler(updater: (scheduler: SchedulerState) => SchedulerState): void {
  commitScheduler(updater(get(appState).scheduler));
}

function markTaskRunning(taskId: string): void {
  updateTask(taskId, { lastStatus: "running", updatedAt: now() });
}

function stopTask(taskId: string, message: string): void {
  updateTask(taskId, {
    enabled: false,
    lastStatus: "stopped",
    lastStderr: message,
    updatedAt: now()
  });
}

function conditionMatches(condition: SchedulerCondition, stdout: string): boolean {
  if (!condition.enabled || !condition.pattern.trim()) {
    return false;
  }
  if (condition.mode === "regex") {
    try {
      return new RegExp(condition.pattern, "m").test(stdout);
    } catch {
      return false;
    }
  }
  return stdout.includes(condition.pattern);
}

function parseDateTime(value?: string): Date | null {
  if (!value) {
    return null;
  }
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function nextRunAt(task: ScheduledTask, fromIso: string): string | undefined {
  if (task.trigger.type === "interval") {
    const base = parseDateTime(fromIso);
    return base ? new Date(base.getTime() + task.trigger.everySeconds * 1000).toISOString() : undefined;
  }
  return undefined;
}

function nextCronTime(expression: string, current: Date): number | null {
  const candidate = new Date(current);
  candidate.setSeconds(0, 0);
  candidate.setMinutes(candidate.getMinutes() + 1);
  for (let i = 0; i < MAX_CRON_SCAN_MINUTES; i++) {
    if (cronMatches(expression, candidate)) {
      return candidate.getTime();
    }
    candidate.setMinutes(candidate.getMinutes() + 1);
  }
  return null;
}

function cronMatches(expression: string, date: Date): boolean {
  const fields = expression.trim().split(/\s+/);
  if (fields.length !== 5) {
    return false;
  }
  const values = [
    date.getMinutes(),
    date.getHours(),
    date.getDate(),
    date.getMonth() + 1,
    date.getDay()
  ];
  return fields.every((field, index) => fieldMatches(field, values[index], index === 4));
}

function fieldMatches(field: string, value: number, isDayOfWeek: boolean): boolean {
  return field.split(",").some((part) => partMatches(part.trim(), value, isDayOfWeek));
}

function partMatches(part: string, value: number, isDayOfWeek: boolean): boolean {
  if (!part) {
    return false;
  }
  const [rangePart, stepPart] = part.split("/");
  const step = stepPart ? Number(stepPart) : 1;
  if (!Number.isInteger(step) || step <= 0) {
    return false;
  }
  if (rangePart === "*") {
    return value % step === 0;
  }
  const [startRaw, endRaw] = rangePart.split("-");
  const start = normalizeCronNumber(Number(startRaw), isDayOfWeek);
  const end = endRaw === undefined ? start : normalizeCronNumber(Number(endRaw), isDayOfWeek);
  if (!Number.isInteger(start) || !Number.isInteger(end)) {
    return false;
  }
  if (value < start || value > end) {
    return false;
  }
  return (value - start) % step === 0;
}

function normalizeCronNumber(value: number, isDayOfWeek: boolean): number {
  return isDayOfWeek && value === 7 ? 0 : value;
}

function minuteKey(value?: string | Date): string {
  const date = value instanceof Date ? value : parseDateTime(value);
  if (!date) {
    return "";
  }
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
    String(date.getHours()).padStart(2, "0"),
    String(date.getMinutes()).padStart(2, "0")
  ].join("-");
}

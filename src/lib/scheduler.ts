import { get } from "svelte/store";
import { isTauriRuntime, runScheduledAction } from "./backend";
import { appState, commitScheduler, now, showToast } from "./stores";
import type { ScheduledTask, SchedulerCondition, SchedulerState } from "./types";

const TICK_MS = 15_000;
const running = new Set<string>();
const lastProbeAt = new Map<string, number>();
let timer: number | undefined;

export function startSchedulerRuntime(): () => void {
  stopSchedulerRuntime();
  if (!isTauriRuntime) {
    return stopSchedulerRuntime;
  }
  void schedulerTick();
  timer = window.setInterval(() => void schedulerTick(), TICK_MS);
  return stopSchedulerRuntime;
}

export function stopSchedulerRuntime(): void {
  if (timer !== undefined) {
    window.clearInterval(timer);
    timer = undefined;
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
      return cronMatches(task.trigger.cron, current) && task.lastRunAt?.slice(0, 16) !== current.toISOString().slice(0, 16);
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
  markTaskRunning(task.id);
  try {
    const output = await runScheduledAction(task.action, get(appState).scheduler.runtimes);
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
  }
}

async function probeAndMaybeRun(task: ScheduledTask): Promise<void> {
  running.add(task.id);
  markTaskRunning(task.id);
  lastProbeAt.set(task.id, Date.now());
  try {
    const probe = await runScheduledAction(task.trigger.probeAction, get(appState).scheduler.runtimes);
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

    const output = await runScheduledAction(task.action, get(appState).scheduler.runtimes);
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
    updateTask(task.id, {
      lastStatus: "failed",
      lastStderr: String(error),
      updatedAt: now()
    });
    showToast(`条件定时任务执行失败：${task.name}`);
  } finally {
    running.delete(task.id);
  }
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

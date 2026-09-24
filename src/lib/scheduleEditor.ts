import type { ScheduledTask, ScheduledTaskAction, SchedulerCondition, ScheduleHistoryRun } from "./types";
import { durationToMs, localInputToIso, splitArguments, uiToSpec } from "./scheduleAdapter";

export type CalendarRule = { mode: "daily" | "weekly" | "monthly" | "advanced"; time: string; weekdays: number[]; day: number };
export const scheduleWeekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
const cronDays = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const cronMonths = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
function cronField(raw: string, min: number, max: number, names: string[] = [], nameBase = min): number[] {
  const values = new Set<number>();
  const number = (text: string): number => {
    const named = names.indexOf(text.toUpperCase());
    const n = named >= 0 ? named + nameBase : /^\d+$/.test(text) ? Number(text) : NaN;
    if (!Number.isInteger(n) || n < min || n > max) throw new Error("Cron 字段超出范围");
    return n;
  };
  for (const item of raw.split(",")) {
    const parts = item.split("/");
    if (parts.length > 2) throw new Error("Cron 步长无效");
    const step = parts.length === 2 && /^\d+$/.test(parts[1]) ? Number(parts[1]) : parts.length === 1 ? 1 : NaN;
    if (!Number.isSafeInteger(step) || step < 1) throw new Error("Cron 步长无效");
    const range = parts[0].split("-");
    let start: number, end: number;
    if (parts[0] === "*" || parts[0] === "?") { start = min; end = max; }
    else if (range.length === 2) { start = number(range[0]); end = number(range[1]); }
    else if (range.length === 1) { start = number(range[0]); end = parts.length === 2 ? max : start; }
    else throw new Error("Cron 范围无效");
    if (start > end) throw new Error("Cron 范围起点不能大于终点");
    for (let n = start; n <= end; n += step) values.add(n);
  }
  return [...values].sort((a, b) => a - b);
}
function cronFields(raw: string) {
  const fields = raw.trim().split(/\s+/), standard = fields.length === 5;
  if (![5, 6, 7].includes(fields.length)) throw new Error("Cron 应为 5、6 或 7 段");
  if (standard) fields.unshift("0");
  return {
    seconds: cronField(fields[0], 0, 59), minutes: cronField(fields[1], 0, 59), hours: cronField(fields[2], 0, 23),
    days: cronField(fields[3], 1, 31), months: cronField(fields[4], 1, 12, cronMonths),
    weekdays: cronField(fields[5], standard ? 0 : 1, 7, cronDays, standard ? 0 : 1).map((n) => standard ? n % 7 : n - 1),
    years: fields[6] ? cronField(fields[6], 1970, 2100) : undefined
  };
}
export function readCalendar(cron: string): CalendarRule {
  const fallback: CalendarRule = { mode: "advanced", time: "09:00", weekdays: [1], day: 1 };
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return fallback;
  const [m, h, day, month, week] = parts;
  if (!/^\d+$/.test(m) || !/^\d+$/.test(h) || +m > 59 || +h > 23 || month !== "*") return fallback;
  const time = `${h.padStart(2, "0")}:${m.padStart(2, "0")}`;
  if (day === "*" && week === "*") return { ...fallback, mode: "daily", time };
  if (/^\d+$/.test(day) && +day >= 1 && +day <= 31 && week === "*") return { ...fallback, mode: "monthly", time, day: +day };
  const weekdays = week.split(",").map((d) => /^\d$/.test(d) && +d <= 7 ? +d % 7 : cronDays.indexOf(d.toUpperCase()));
  if (day === "*" && weekdays.every((d) => d >= 0) && new Set(weekdays).size === weekdays.length) return { ...fallback, mode: "weekly", time, weekdays };
  return fallback;
}
export function calendarCron(rule: CalendarRule): string {
  const [h, m] = rule.time.split(":").map(Number);
  if (!validClock(rule.time)) throw new Error("请选择有效时刻");
  if (rule.mode === "weekly" && !rule.weekdays.length) throw new Error("至少选择一个星期");
  if (rule.mode === "monthly" && (!Number.isInteger(rule.day) || rule.day < 1 || rule.day > 31)) throw new Error("每月日期应为 1–31");
  return `${m} ${h} ${rule.mode === "monthly" ? rule.day : "*"} * ${rule.mode === "weekly" ? rule.weekdays.map((d) => cronDays[d]).join(",") : "*"}`;
}
export function validClock(raw: string): boolean { return /^([01]\d|2[0-3]):[0-5]\d$/.test(raw); }
export function validScheduleDate(raw: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(raw)) return false;
  const date = new Date(`${raw}T12:00:00Z`);
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === raw;
}
function conditionError(condition: SchedulerCondition): string | null {
  if (!condition.pattern.trim()) return "请输入匹配文本或正则";
  if (condition.mode === "regex") {
    try { new RegExp(condition.pattern); } catch { return "正则表达式无效"; }
  }
  return null;
}
function actionError(action: ScheduledTaskAction, desktop: boolean, probe = false): string | null {
  if ((!desktop || probe) && action.type === "notification" && probe) return "探针必须是脚本或程序";
  if (!desktop && action.type !== "notification") return "移动端只支持通知动作";
  const notificationError = (n: ScheduledTaskAction["notification"]): string | null => {
    if (!n.message.trim()) return "请输入通知消息";
    if (!Number.isInteger(n.durationMs) || n.durationMs < 1200 || n.durationMs > 60000) return "通知时长应为 1200–60000 毫秒";
    for (const match of `${n.title} ${n.message}`.matchAll(/\{([a-zA-Z][a-zA-Z0-9]*)\}/g)) {
      if (!["taskName", "stdout", "stderr", "exitCode"].includes(match[1])) return `未知通知变量：${match[0]}`;
    }
    return null;
  };
  if (action.type === "notification") return notificationError(action.notification);
  if (action.type === "executable" && !action.executablePath.trim()) return "请选择可执行程序";
  if (action.type === "script" && !(action.scriptMode === "path" ? action.filePath : action.code).trim()) return "请输入脚本内容或路径";
  if (action.language === "custom" && !action.interpreter.trim()) return "请指定自定义解释器";
  if (action.timeout !== undefined && action.timeout !== "" && !Number.isFinite(durationToMs(action.timeout, NaN))) return "超时应为正整数时长，如 30s";
  try { splitArguments(action.arguments); } catch (error) { return String((error as Error).message); }
  if (!probe && action.notifyOnComplete) { const error = notificationError(action.completionNotification); if (error) return error; }
  if (!probe && action.stdoutNotification.enabled) return conditionError(action.stdoutNotification.condition) || notificationError(action.stdoutNotification.notification);
  return null;
}
export function validateScheduleDraft(task: ScheduledTask, desktop: boolean): string | null {
  if (!task.name.trim()) return "请输入任务名称";
  if (task.until && !validScheduleDate(task.until)) return "截止日期无效";
  const trigger = task.trigger;
  if (trigger.type === "once") { try { localInputToIso(trigger.runAt); } catch (e) { return (e as Error).message; } }
  if ((trigger.type === "interval" || trigger.type === "condition") && (!Number.isFinite(trigger.everySeconds) || trigger.everySeconds <= 0 || !Number.isSafeInteger(trigger.everySeconds * 1000))) return "间隔必须为有效正数";
  if (trigger.type === "interval") {
    if (!Number.isSafeInteger(trigger.repeatCount) || trigger.repeatCount < 0) return "重复次数必须为非负整数";
    if (trigger.stopCondition.enabled) { const error = conditionError(trigger.stopCondition); if (error) return error; }
  }
  if (trigger.type === "calendar") {
    try { cronFields(trigger.cron); } catch (error) { return (error as Error).message; }
    try { new Intl.DateTimeFormat("en", { timeZone: trigger.timezone || Intl.DateTimeFormat().resolvedOptions().timeZone }); } catch { return "请输入有效 IANA 时区"; }
  }
  if (trigger.type === "condition") {
    if (!desktop) return "移动端不支持条件探针";
    if (task.gate) return "条件触发不能叠加执行门控";
    if (trigger.cooldown !== undefined && !Number.isFinite(durationToMs(trigger.cooldown, NaN))) return "冷却时长应为正整数时长，如 5m";
    const error = conditionError(trigger.probeCondition) || actionError(trigger.probeAction, desktop, true);
    if (error) return error;
  }
  if (task.gate) {
    const gate = task.gate;
    if (!gate.windows.length && !gate.probeAction) return "请添加时间窗口或探针";
    if (gate.windows.length > 64) return "时间窗口最多 64 个";
    for (const window of gate.windows) {
      if (!validClock(window.start) || !validClock(window.end) || window.start === window.end) return "窗口起止须为不同的有效时刻";
      if (window.weekdays && (!window.weekdays.length || new Set(window.weekdays).size !== window.weekdays.length || window.weekdays.some((d) => !Number.isInteger(d) || d < 0 || d > 6))) return "窗口至少选择一个星期";
    }
    if (Boolean(gate.probeAction) !== Boolean(gate.condition)) return "探针和匹配规则须同时设置";
    if (gate.probeAction) {
      if (!desktop) return "移动端不支持探针门控";
      const error = actionError(gate.probeAction, desktop, true) || conditionError(gate.condition!);
      if (error) return error;
    }
  }
  return actionError(task.action, desktop);
}
export function scheduleDirty(task: ScheduledTask, saved: ScheduledTask): boolean {
  try { return JSON.stringify(uiToSpec(task)) !== JSON.stringify(uiToSpec(saved)); } catch { return true; }
}
export function visibleScheduleHistory(runs: ScheduleHistoryRun[]): ScheduleHistoryRun[] {
  return runs.filter((run) => !(run.kind === "probe" && run.exitCode === 0 && run.stopReason === "probe 未命中"))
    .sort((a, b) => Date.parse(b.startedAt) - Date.parse(a.startedAt));
}
export function whenSentence(task: ScheduledTask): string {
  const t = task.trigger;
  if (t.type === "once") return `在 ${t.runAt.replace("T", " ") || "指定时间"}`;
  if (t.type === "interval") return `每隔 ${t.everySeconds} 秒`;
  if (t.type === "condition") return `每 ${t.everySeconds} 秒检查条件`;
  const rule = readCalendar(t.cron);
  if (rule.mode === "daily") return `每天 ${rule.time}`;
  if (rule.mode === "weekly") return `每${rule.weekdays.map((d) => scheduleWeekdays[d]).join("、")} ${rule.time}`;
  if (rule.mode === "monthly") return `每月 ${rule.day} 日 ${rule.time}`;
  return `按 Cron ${t.cron}`;
}
export function actionSentence(task: ScheduledTask): string {
  const a = task.action;
  return a.type === "notification" ? `通知「${a.notification.title || "KXToDo"}」` : a.type === "executable" ? "运行程序" : `执行 ${a.language} 脚本`;
}
export function stopSentence(task: ScheduledTask): string {
  const t = task.trigger;
  const parts = [t.type === "once" ? "执行一次后结束" : t.type === "condition" ? (t.cooldown !== undefined ? `命中后冷却 ${t.cooldown} 再检查` : "命中后结束") : t.type === "interval" && t.repeatCount ? `执行 ${t.repeatCount} 次后结束` : "持续运行"];
  if (t.type === "interval" && t.stopCondition.enabled) parts.push("输出匹配时停止");
  if (task.until) parts.push(`最晚至 ${task.until}`);
  return parts.join("，");
}
const zoneFormatters = new Map<string, Intl.DateTimeFormat>();
function zonedParts(date: Date, timezone: string): Record<string, string> {
  let formatter = zoneFormatters.get(timezone);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-CA", { timeZone: timezone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });
    if (zoneFormatters.size > 16) zoneFormatters.clear();
    zoneFormatters.set(timezone, formatter);
  }
  return Object.fromEntries(formatter.formatToParts(date).map((p) => [p.type, p.value]));
}
function zonedDay(date: Date, timezone: string): string {
  const p = zonedParts(date, timezone); return `${p.year}-${p.month}-${p.day}`;
}
/** Calendar preview mirrors numeric/list/range/step cron, with timezone/DST checks.
 * Search by days and selected wall times, not by seconds through an entire year. */
export function nextCalendarInstant(cron: string, timezone: string, now: Date): Date | null {
  const fields = cronFields(cron);
  const today = zonedDay(now, timezone);
  let start = new Date(`${today}T00:00:00Z`);
  if (fields.years) {
    const year = fields.years.find((y) => y >= start.getUTCFullYear());
    if (year === undefined) return null;
    if (year > start.getUTCFullYear()) start = new Date(`${year}-01-01T00:00:00Z`);
  }
  // Eight years covers leap-day/weekday combinations; explicit years jump ahead.
  for (let i = 0; i < 366 * 8; i++) {
    const date = new Date(start.getTime() + i * 86400000);
    if (fields.years && !fields.years.includes(date.getUTCFullYear())) continue;
    if (!fields.months.includes(date.getUTCMonth() + 1) || !fields.days.includes(date.getUTCDate()) || !fields.weekdays.includes(date.getUTCDay())) continue;
    const day = date.toISOString().slice(0, 10), offsets = new Set<number>();
    // Both sides of a clock change: ambiguous wall times get two candidates;
    // nonexistent times get none. All candidates are checked against Intl.
    for (const hours of [-24, 12, 36]) {
      const instant = date.getTime() + hours * 3600000, p = zonedParts(new Date(instant), timezone);
      const wall = Date.parse(`${p.year}-${p.month}-${p.day}T${p.hour}:${p.minute}:${p.second}Z`);
      offsets.add(wall - instant);
    }
    let best = Infinity;
    for (const offset of offsets) {
      const base = date.getTime() - offset;
      candidate: for (const h of fields.hours) for (const m of fields.minutes) {
        const s = fields.seconds.find((s) => base + (h * 3600 + m * 60 + s) * 1000 > now.getTime());
        if (s === undefined) continue;
        const value = base + (h * 3600 + m * 60 + s) * 1000;
        const p = zonedParts(new Date(value), timezone);
        if (`${p.year}-${p.month}-${p.day}` === day && +p.hour === h && +p.minute === m && +p.second === s) {
          best = Math.min(best, value); break candidate;
        }
      }
    }
    if (Number.isFinite(best)) return new Date(best);
  }
  return null;
}
export function nextRunPreview(task: ScheduledTask, now = new Date()): string {
  try {
    const t = task.trigger;
    const timezone = t.type === "calendar" ? t.timezone || Intl.DateTimeFormat().resolvedOptions().timeZone : Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (task.until && zonedDay(now, timezone) > task.until) return "已过截止日期";
    let next: Date | null = null;
    if (t.type === "once") next = new Date(localInputToIso(t.runAt));
    else if (t.type === "calendar") {
      next = nextCalendarInstant(t.cron, timezone, now);
      if (!next) return "预览范围内无触发时间，请检查 Cron";
    } else {
      if (t.type === "interval" && t.repeatCount > 0 && task.runCount >= t.repeatCount) return "已达到执行次数";
      if (task.nextRunAt && Date.parse(task.nextRunAt) > now.getTime()) next = new Date(task.nextRunAt);
      else next = new Date(now.getTime() + t.everySeconds * 1000);
    }
    if (!next || !Number.isFinite(next.getTime())) return "请完善触发规则";
    if (task.until && zonedDay(next, timezone) > task.until) return "截止日期内无下次触发";
    const label = next.toLocaleString([], { timeZone: timezone, month: "long", day: "numeric", hour: "2-digit", minute: "2-digit" });
    return `${label}${t.type === "calendar" ? ` · ${timezone}` : ""}${task.gate || t.type === "condition" ? " · 满足条件才执行" : ""}`;
  } catch { return "请完善触发规则"; }
}

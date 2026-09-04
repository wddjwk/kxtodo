import type { Task } from "./types";

/**
 * 计划内视图的日期分组（纯逻辑，无 store 依赖）。
 * 日期字段与 planned 列表口径一致：dueDate 优先，其次 plannedDate
 * （nodes.ts tasksForNode 用 Boolean(dueDate || plannedDate) 收录任务）。
 * 分组是"过滤器"而非"分区"，允许交叠（今天 ⊂ 近三天 ⊂ 本周 等）。
 */

export type PlannedGroupKey = "today" | "tomorrow" | "threeDays" | "week" | "later" | "all";

const WEEKDAY_LABELS = ["日", "一", "二", "三", "四", "五", "六"];

function fmtIso(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function parseIso(iso: string): Date {
  return new Date(`${iso}T00:00:00`);
}

function addDays(iso: string, days: number): string {
  const date = parseIso(iso);
  date.setDate(date.getDate() + days);
  return fmtIso(date);
}

/** 本周起点：周一（周日归上一周末尾）。 */
function weekStartIso(todayIso: string): string {
  const date = parseIso(todayIso);
  const dow = date.getDay();
  date.setDate(date.getDate() + (dow === 0 ? -6 : 1 - dow));
  return fmtIso(date);
}

function plannedDateOf(task: Task): string | undefined {
  return task.dueDate?.slice(0, 10) || task.plannedDate?.slice(0, 10) || undefined;
}

/** 同月区间渲染 M月D-D日；跨月渲染 M月D日-M月D日。 */
function rangeLabel(startIsoValue: string, endIsoValue: string): string {
  const start = parseIso(startIsoValue);
  const end = parseIso(endIsoValue);
  if (start.getMonth() === end.getMonth()) {
    return `${start.getMonth() + 1}月${start.getDate()}-${end.getDate()}日`;
  }
  return `${start.getMonth() + 1}月${start.getDate()}日-${end.getMonth() + 1}月${end.getDate()}日`;
}

export function plannedGroupOptions(todayIsoValue: string): Array<{ key: PlannedGroupKey; label: string }> {
  const weekStart = weekStartIso(todayIsoValue);
  const weekEnd = addDays(weekStart, 6);
  return [
    { key: "today", label: `今天（周${WEEKDAY_LABELS[parseIso(todayIsoValue).getDay()]}）` },
    { key: "tomorrow", label: `明天（周${WEEKDAY_LABELS[parseIso(addDays(todayIsoValue, 1)).getDay()]}）` },
    { key: "threeDays", label: `近三天（${rangeLabel(todayIsoValue, addDays(todayIsoValue, 2))}）` },
    { key: "week", label: `本周（${rangeLabel(weekStart, weekEnd)}）` },
    { key: "later", label: "稍后" },
    { key: "all", label: "全部" }
  ];
}

export function filterPlannedTasks(tasks: Task[], key: PlannedGroupKey, todayIsoValue: string): Task[] {
  if (key === "all") return [...tasks];
  const weekStart = weekStartIso(todayIsoValue);
  const weekEnd = addDays(weekStart, 6);
  const tomorrow = addDays(todayIsoValue, 1);
  const threeDaysEnd = addDays(todayIsoValue, 2);
  return tasks.filter((task) => {
    const date = plannedDateOf(task);
    if (!date) return false;
    switch (key) {
      case "today":
        return date === todayIsoValue;
      case "tomorrow":
        return date === tomorrow;
      case "threeDays":
        return date >= todayIsoValue && date <= threeDaysEnd;
      case "week":
        // 标签展示完整周一~周日区间，但逾期（今天之前）的任务只归"全部"。
        return date >= todayIsoValue && date <= weekEnd;
      case "later":
        return date > weekEnd;
      default:
        return true;
    }
  });
}

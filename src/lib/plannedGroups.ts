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

/** 「全部」视图的互斥分区：已逾期 / 近三天 / 本周剩余 / 稍后，空分区不返回。
 *
 * 下拉里的分组是**过滤器**（允许交叠），分区必须是互斥的，否则一条任务会出现在
 * 两个标题下。标签复用 [`plannedGroupOptions`] 的文案，叫法保持一致。 */
export function plannedSections(
  tasks: Task[],
  todayIsoValue: string
): Array<{ key: string; label: string; tasks: Task[] }> {
  const weekStart = weekStartIso(todayIsoValue);
  const weekEnd = addDays(weekStart, 6);
  const tomorrow = addDays(todayIsoValue, 1);
  const threeDaysEnd = addDays(todayIsoValue, 2);
  const labels = new Map(plannedGroupOptions(todayIsoValue).map((option) => [option.key, option.label]));
  const buckets: Array<{ key: string; label: string; tasks: Task[] }> = [
    { key: "overdue", label: "已逾期", tasks: [] },
    { key: "today", label: labels.get("today") ?? "今天", tasks: [] },
    // 「今天」单独成桶后，近三天桶从明天起算，标签区间也跟着从明天写，避免含义重叠
    { key: "threeDays", label: `近三天（${rangeLabel(tomorrow, threeDaysEnd)}）`, tasks: [] },
    { key: "week", label: labels.get("week") ?? "本周", tasks: [] },
    { key: "later", label: labels.get("later") ?? "稍后", tasks: [] }
  ];
  for (const task of tasks) {
    const date = plannedDateOf(task);
    const bucket = !date
      ? buckets[4]
      : date < todayIsoValue
        ? buckets[0]
        : date === todayIsoValue
          ? buckets[1]
          : date <= threeDaysEnd
            ? buckets[2]
            : date <= weekEnd
              ? buckets[3]
              : buckets[4];
    bucket?.tasks.push(task);
  }
  return buckets.filter((bucket) => bucket.tasks.length > 0);
}

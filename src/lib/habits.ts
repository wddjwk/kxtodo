/**
 * 习惯打卡的纯逻辑（v0.9.3）：计划判定 / 频率与时刻文案 / 每日视图分组 / 统计。
 * 不 import 任何 store 与组件，node 单测可直接跑（断言一律时区无关：日期字符串运算）。
 *
 * 统计口径（与业界打卡应用一致）：
 * - 累计打卡 = checkins 条数；
 * - 当前连续 = 从「今天或昨天」往回数**计划内且已打卡**的连续天数（今天还没打不算断）；
 * - 最长连续 = 历史上最长的连续计划内打卡段；
 * - 完成率 = 近 30 天里计划内日期的打卡占比。
 */
import type { Habit, HabitFreq } from "./types";

export const HABIT_WEEKDAY_LABELS = ["日", "一", "二", "三", "四", "五", "六"];

/** YYYY-MM-DD 加减天数（本地墙上日期，不做时区换算）。 */
export function shiftDate(date: string, days: number): string {
  const [y, m, d] = date.split("-").map(Number);
  const base = new Date(y, (m ?? 1) - 1, d ?? 1);
  base.setDate(base.getDate() + days);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${base.getFullYear()}-${pad(base.getMonth() + 1)}-${pad(base.getDate())}`;
}

export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, (m ?? 1) - 1, d ?? 1).getDay();
}

/** 这一天是否在习惯的计划内（与 core `Habit::scheduled_on` 同口径）。 */
export function scheduledOn(habit: Habit, date: string): boolean {
  if (habit.freq.kind === "daily") return true;
  return habit.freq.weekdays.includes(weekdayOf(date));
}

/** 频率展示串："每天" / "每周 一、三、五" / "每周日"。 */
export function freqLabel(freq: HabitFreq): string {
  if (freq.kind === "daily") return "每天";
  const days = [...freq.weekdays].sort((a, b) => a - b);
  if (days.length === 0) return "每周";
  if (days.length === 7) return "每天";
  if (days.length === 5 && [1, 2, 3, 4, 5].every((day) => days.includes(day))) return "每周工作日";
  return `每周${days.map((day) => HABIT_WEEKDAY_LABELS[day]).join("、")}`;
}

/** 每日视图的时段分组：全天 / 上午 / 下午 / 晚上（与参考应用同口径）。 */
export type HabitSlot = "allday" | "morning" | "afternoon" | "evening";

export const HABIT_SLOTS: Array<{ id: HabitSlot; label: string }> = [
  { id: "allday", label: "全天" },
  { id: "morning", label: "上午" },
  { id: "afternoon", label: "下午" },
  { id: "evening", label: "晚上" }
];

export function slotOf(time: string): HabitSlot {
  if (!time) return "allday";
  const hour = Number(time.slice(0, 2));
  if (hour < 12) return "morning";
  if (hour < 18) return "afternoon";
  return "evening";
}

/** 计划时刻展示串："全天" / "08:00"。 */
export function timeLabel(time: string): string {
  return time || "全天";
}

/** 每日视图：某天计划内且 active 的习惯，按时段分组（组内按 order）。 */
export function dailyGroups(
  habits: Habit[],
  date: string
): Array<{ slot: HabitSlot; label: string; habits: Habit[] }> {
  const due = habits
    .filter((habit) => habit.status === "active" && scheduledOn(habit, date))
    .sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  return HABIT_SLOTS.map(({ id, label }) => ({
    slot: id,
    label,
    habits: due.filter((habit) => slotOf(habit.time) === id)
  })).filter((group) => group.habits.length > 0);
}

export function isChecked(habit: Habit, date: string): boolean {
  return habit.checkins.includes(date);
}

/** 上一个计划内日期（weekly 跳过没选的日子；daily 就是前一天）。 */
export function previousScheduled(habit: Habit, date: string): string {
  let day = shiftDate(date, -1);
  for (let guard = 0; guard < 8; guard += 1) {
    if (scheduledOn(habit, day)) return day;
    day = shiftDate(day, -1);
  }
  return "";
}

/** 下一个计划内日期（bestStreak 用：weekly 的「连续」是相邻两次计划出现）。 */
export function nextScheduled(habit: Habit, date: string): string {
  let day = shiftDate(date, 1);
  for (let guard = 0; guard < 8; guard += 1) {
    if (scheduledOn(habit, day)) return day;
    day = shiftDate(day, 1);
  }
  return "";
}

/** 当前连续：今天已打从今天往回数，今天没打（或今天不在计划内）从上一个计划日往回数。 */
export function currentStreak(habit: Habit, today: string): number {
  let day = scheduledOn(habit, today) && isChecked(habit, today) ? today : previousScheduled(habit, today);
  let streak = 0;
  // 保险丝：连续段不可能比打卡总数长，再多就是脏数据
  while (day && streak <= habit.checkins.length && isChecked(habit, day)) {
    streak += 1;
    day = previousScheduled(habit, day);
  }
  return streak;
}

/** 最长连续：把 checkins 按「计划内相邻出现」切段取最长。 */
export function bestStreak(habit: Habit): number {
  let best = 0;
  let run = 0;
  let previous = "";
  for (const date of habit.checkins) {
    if (!scheduledOn(habit, date)) continue;
    if (previous && nextScheduled(habit, previous) === date) {
      run += 1;
    } else {
      run = 1;
    }
    best = Math.max(best, run);
    previous = date;
  }
  return best;
}

/** 某月（YYYY-MM）的打卡天数。 */
export function monthCount(habit: Habit, month: string): number {
  return habit.checkins.filter((date) => date.slice(0, 7) === month).length;
}

/** 近 30 天（含今天）计划内日期的打卡占比，0-1；没有计划日返回 0。 */
export function completionRate(habit: Habit, today: string): number {
  let due = 0;
  let done = 0;
  for (let offset = 0; offset < 30; offset += 1) {
    const date = shiftDate(today, -offset);
    if (!scheduledOn(habit, date)) continue;
    due += 1;
    if (isChecked(habit, date)) done += 1;
  }
  return due === 0 ? 0 : done / due;
}

export type HabitStats = {
  total: number;
  current: number;
  best: number;
  month: number;
  rate: number;
};

export function habitStats(habit: Habit, today: string): HabitStats {
  return {
    total: habit.checkins.length,
    current: currentStreak(habit, today),
    best: bestStreak(habit),
    month: monthCount(habit, today.slice(0, 7)),
    rate: completionRate(habit, today)
  };
}

/** 月历格子（含前后补空，凑整周）：周起始跟全应用 weekStart 偏好同一来源；
 *  null = 补空。 */
export function monthGrid(month: string, weekStart: number): Array<string | null> {
  const [y, m] = month.split("-").map(Number);
  const first = new Date(y, (m ?? 1) - 1, 1);
  const daysInMonth = new Date(y, m ?? 1, 0).getDate();
  const lead = (first.getDay() - weekStart + 7) % 7;
  const cells: Array<string | null> = [];
  for (let i = 0; i < lead; i += 1) cells.push(null);
  const pad = (n: number): string => String(n).padStart(2, "0");
  for (let day = 1; day <= daysInMonth; day += 1) cells.push(`${month}-${pad(day)}`);
  while (cells.length % 7 !== 0) cells.push(null);
  return cells;
}

/** 打卡切换后的新 checkins（升序去重）；纯函数，写路径交给 actions。 */
export function toggledCheckins(habit: Habit, date: string, checked: boolean): string[] {
  const set = new Set(habit.checkins);
  if (checked) set.add(date);
  else set.delete(date);
  return [...set].sort();
}
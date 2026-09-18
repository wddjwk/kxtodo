import { dueMoment } from "./dueHighlight";
import type { Task } from "./types";

export type SortMode = "created-desc" | "created-asc" | "alpha-asc" | "alpha-desc" | "due-asc" | "due-desc" | "importance";

export const sortLabels: Record<SortMode, string> = {
  "created-desc": "创建时间 ↓ 最新",
  "created-asc": "创建时间 ↑ 最早",
  "alpha-asc": "字母顺序 A → Z",
  "alpha-desc": "字母顺序 Z → A",
  "due-asc": "截止时间 ↑ 最近",
  "due-desc": "截止时间 ↓ 最远",
  "importance": "重要性优先"
};

/**
 * 截止排序比的是**完整时刻**（日期 + 时刻），不是日期字符串。
 *
 * 只比 `dueDate` 的话，同一天里「09:00 到期」与「只精确到天」永远平手，顺序交给
 * Array.sort 的稳定性 = 交给插入顺序——表现就是「精确到分钟的始终排在没精确分钟的
 * 上面，升序降序都一样」（v0.8.3 修的 bug）。没有时刻的按当天 23:59:59 算
 * （与临期高亮的 `dueMoment` 同一口径：「今天到期」= 今天结束前），于是升序里带时刻的
 * 在前、降序里在后，方向终于有了意义。
 */
function compareDue(a: Task, b: Task, direction: 1 | -1): number {
  const left = dueMoment(a);
  const right = dueMoment(b);
  // 没日期的沉底（两个方向都沉底：它们是「没排期」，不是「最远」）
  if (left === null && right === null) return 0;
  if (left === null) return 1;
  if (right === null) return -1;
  return (left - right) * direction;
}

export function sortTasks(tasks: Task[], mode: SortMode): Task[] {
  return [...tasks].sort((a, b) => {
    switch (mode) {
      case "created-desc": return b.createdAt.localeCompare(a.createdAt);
      case "created-asc": return a.createdAt.localeCompare(b.createdAt);
      case "alpha-asc": return a.markdown.localeCompare(b.markdown, "zh");
      case "alpha-desc": return b.markdown.localeCompare(a.markdown, "zh");
      case "due-asc": return compareDue(a, b, 1);
      case "due-desc": return compareDue(a, b, -1);
      case "importance": return (b.important ? 1 : 0) - (a.important ? 1 : 0);
      default: return 0;
    }
  });
}

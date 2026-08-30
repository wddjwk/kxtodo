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

export function sortTasks(tasks: Task[], mode: SortMode): Task[] {
  return [...tasks].sort((a, b) => {
    switch (mode) {
      case "created-desc": return b.createdAt.localeCompare(a.createdAt);
      case "created-asc": return a.createdAt.localeCompare(b.createdAt);
      case "alpha-asc": return a.markdown.localeCompare(b.markdown, "zh");
      case "alpha-desc": return b.markdown.localeCompare(a.markdown, "zh");
      case "due-asc": {
        if (!a.dueDate && !b.dueDate) return 0;
        if (!a.dueDate) return 1;
        if (!b.dueDate) return -1;
        return a.dueDate.localeCompare(b.dueDate);
      }
      case "due-desc": {
        if (!a.dueDate && !b.dueDate) return 0;
        if (!a.dueDate) return 1;
        if (!b.dueDate) return -1;
        return b.dueDate.localeCompare(a.dueDate);
      }
      case "importance": return (b.important ? 1 : 0) - (a.important ? 1 : 0);
      default: return 0;
    }
  });
}

import { describe, expect, it } from "vitest";

import { sortTasks } from "../sort";
import type { Task } from "../types";

/** 本地今天的 YYYY-MM-DD（断言时区无关：只比较相对顺序，不比较绝对值） */
function localDate(offsetDays: number): string {
  const date = new Date();
  date.setDate(date.getDate() + offsetDays);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function task(id: string, dueDate?: string, dueTime?: string): Task {
  return {
    id,
    nodeId: "entry-a",
    markdown: id,
    completed: false,
    important: false,
    myDay: false,
    dueDate,
    dueTime,
    tags: [],
    emojis: [],
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z"
  };
}

const names = (tasks: Task[]): string[] => tasks.map((item) => item.id);

describe("截止排序（需求 7：时刻要参与比较）", () => {
  const today = localDate(0);
  const timed = task("带时刻", today, "09:00");
  const allday = task("只到天", today);
  const later = task("同天更晚", today, "18:30");

  it("升序：同一天里带时刻的按时刻排，只到天的按当天末尾排最后", () => {
    expect(names(sortTasks([allday, later, timed], "due-asc"))).toEqual(["带时刻", "同天更晚", "只到天"]);
  });

  it("降序：方向真的反过来了（只到天的最先）", () => {
    expect(names(sortTasks([timed, allday, later], "due-desc"))).toEqual(["只到天", "同天更晚", "带时刻"]);
  });

  it("跨天仍按天排，时刻不越级", () => {
    const tomorrowEarly = task("明天早", localDate(1), "06:00");
    const todayLate = task("今天晚", today, "23:00");
    expect(names(sortTasks([tomorrowEarly, todayLate], "due-asc"))).toEqual(["今天晚", "明天早"]);
    expect(names(sortTasks([tomorrowEarly, todayLate], "due-desc"))).toEqual(["明天早", "今天晚"]);
  });

  it("没日期的两个方向都沉底", () => {
    const undated = task("没日期");
    expect(names(sortTasks([undated, timed], "due-asc"))).toEqual(["带时刻", "没日期"]);
    expect(names(sortTasks([undated, timed], "due-desc"))).toEqual(["带时刻", "没日期"]);
  });

  it("完全同刻平手（交给稳定性，不瞎排）", () => {
    const a = task("甲", today, "09:00");
    const b = task("乙", today, "09:00");
    expect(names(sortTasks([a, b], "due-asc"))).toEqual(["甲", "乙"]);
  });
});

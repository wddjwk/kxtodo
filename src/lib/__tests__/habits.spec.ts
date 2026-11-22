import { describe, expect, it } from "vitest";
import {
  bestStreak,
  checkinCount,
  checkinProgress,
  completionRate,
  currentStreak,
  dailyGroups,
  freqLabel,
  habitStats,
  habitTarget,
  isComplete,
  monthCount,
  monthGrid,
  nextScheduled,
  previousScheduled,
  scheduledOn,
  shiftDate,
  slotOf,
  tappedCheckins,
  weekdayOf
} from "../habits";
import { normalizeHabits } from "../defaults";
import type { Habit } from "../types";

function habit(partial: Partial<Habit> = {}): Habit {
  return {
    id: "habit-test",
    name: "测试习惯",
    color: "",
    icon: "",
    freq: { kind: "daily", weekdays: [] },
    time: "",
    reminders: [],
    status: "active",
    order: 1,
    targetCount: 1,
    checkins: [],
    createdAt: "2026-01-01T00:00:00Z",
    ...partial
  };
}

describe("日期工具", () => {
  it("shiftDate 跨月跨年", () => {
    expect(shiftDate("2026-01-31", 1)).toBe("2026-02-01");
    expect(shiftDate("2026-03-01", -1)).toBe("2026-02-28");
    expect(shiftDate("2024-03-01", -1)).toBe("2024-02-29");
    expect(shiftDate("2026-12-31", 1)).toBe("2027-01-01");
  });

  it("weekdayOf 与 JS getDay 同口径", () => {
    expect(weekdayOf("2026-11-19")).toBe(4); // 周四
    expect(weekdayOf("2026-11-22")).toBe(0); // 周日
  });
});

describe("计划判定与文案", () => {
  it("daily 全中、weekly 看星期", () => {
    expect(scheduledOn(habit(), "2026-11-19")).toBe(true);
    const weekly = habit({ freq: { kind: "weekly", weekdays: [1, 4] } });
    expect(scheduledOn(weekly, "2026-11-19")).toBe(true); // 周四
    expect(scheduledOn(weekly, "2026-11-20")).toBe(false); // 周五
    expect(scheduledOn(weekly, "2026-11-23")).toBe(true); // 周一
  });

  it("freqLabel 覆盖每天/工作日/单选/多选", () => {
    expect(freqLabel({ kind: "daily", weekdays: [] })).toBe("每天");
    expect(freqLabel({ kind: "weekly", weekdays: [1, 2, 3, 4, 5] })).toBe("每周工作日");
    expect(freqLabel({ kind: "weekly", weekdays: [0] })).toBe("每周日");
    expect(freqLabel({ kind: "weekly", weekdays: [4, 1] })).toBe("每周一、四");
    expect(freqLabel({ kind: "weekly", weekdays: [0, 1, 2, 3, 4, 5, 6] })).toBe("每天");
  });

  it("slotOf 边界：12 点进下午、18 点进晚上", () => {
    expect(slotOf("")).toBe("allday");
    expect(slotOf("06:00")).toBe("morning");
    expect(slotOf("11:59")).toBe("morning");
    expect(slotOf("12:00")).toBe("afternoon");
    expect(slotOf("17:59")).toBe("afternoon");
    expect(slotOf("18:00")).toBe("evening");
    expect(slotOf("23:30")).toBe("evening");
  });
});

describe("每日视图分组", () => {
  it("只留 active 且当天计划内，按时段分组、组内按 order", () => {
    const list = [
      habit({ id: "c", name: "晚饭散步", time: "19:00", order: 3 }),
      habit({ id: "a", name: "刷牙", time: "08:00", order: 1 }),
      habit({ id: "b", name: "阅读", time: "", order: 2 }),
      habit({ id: "p", name: "暂停的", time: "08:30", status: "paused" }),
      habit({ id: "w", name: "周报", freq: { kind: "weekly", weekdays: [0] }, time: "17:00" })
    ];
    const groups = dailyGroups(list, "2026-11-19"); // 周四：周报不在计划
    expect(groups.map((group) => group.slot)).toEqual(["allday", "morning", "evening"]);
    expect(groups[0].habits.map((item) => item.id)).toEqual(["b"]);
    expect(groups[1].habits.map((item) => item.id)).toEqual(["a"]);
    expect(groups[2].habits.map((item) => item.id)).toEqual(["c"]);
  });

  it("没有计划内习惯时返回空数组", () => {
    expect(dailyGroups([habit({ freq: { kind: "weekly", weekdays: [0] } })], "2026-11-19")).toEqual([]);
  });
});

describe("连续与统计", () => {
  it("daily：今天已打从今天数；今天没打从昨天数（不算断）", () => {
    const checked = habit({ checkins: ["2026-11-17", "2026-11-18", "2026-11-19"] });
    expect(currentStreak(checked, "2026-11-19")).toBe(3);
    const notYet = habit({ checkins: ["2026-11-17", "2026-11-18"] });
    expect(currentStreak(notYet, "2026-11-19")).toBe(2);
    const broken = habit({ checkins: ["2026-11-16", "2026-11-18"] });
    expect(currentStreak(broken, "2026-11-19")).toBe(1);
    expect(currentStreak(habit(), "2026-11-19")).toBe(0);
  });

  it("weekly：连续按相邻计划出现算，跳过没选的日子", () => {
    // 每周一、四：11-16(一)、11-19(四)、11-23(一) 是相邻出现
    const weekly = habit({
      freq: { kind: "weekly", weekdays: [1, 4] },
      checkins: ["2026-11-16", "2026-11-19", "2026-11-23"]
    });
    expect(currentStreak(weekly, "2026-11-23")).toBe(3);
    expect(currentStreak(weekly, "2026-11-24")).toBe(3); // 今天不在计划内，从上次出现数
    expect(bestStreak(weekly)).toBe(3);
    const gap = habit({
      freq: { kind: "weekly", weekdays: [1, 4] },
      checkins: ["2026-11-16", "2026-11-23"]
    });
    expect(bestStreak(gap)).toBe(1); // 漏了 11-19 → 断
    expect(currentStreak(gap, "2026-11-23")).toBe(1);
  });

  it("bestStreak 取历史最长段", () => {
    const h = habit({
      checkins: ["2026-11-01", "2026-11-02", "2026-11-03", "2026-11-07", "2026-11-08"]
    });
    expect(bestStreak(h)).toBe(3);
  });

  it("monthCount / completionRate / habitStats", () => {
    const h = habit({ checkins: ["2026-11-17", "2026-11-18", "2026-11-19", "2026-10-31"] });
    expect(monthCount(h, "2026-11")).toBe(3);
    expect(monthCount(h, "2026-10")).toBe(1);
    // 近 30 天（10-21…11-19）daily 全计划：4 次打卡都落在窗口内
    expect(completionRate(h, "2026-11-19")).toBeCloseTo(4 / 30, 5);
    const stats = habitStats(h, "2026-11-19");
    expect(stats.total).toBe(4);
    expect(stats.current).toBe(3);
    expect(stats.best).toBe(3);
    expect(stats.month).toBe(3);
  });

  it("previousScheduled / nextScheduled 跳过非计划日", () => {
    const weekly = habit({ freq: { kind: "weekly", weekdays: [1, 4] } });
    expect(previousScheduled(weekly, "2026-11-20")).toBe("2026-11-19");
    expect(nextScheduled(weekly, "2026-11-19")).toBe("2026-11-23");
    expect(previousScheduled(habit(), "2026-11-19")).toBe("2026-11-18");
  });
});

describe("月历与打卡切换", () => {
  it("monthGrid 补空凑整周、周起始跟偏好", () => {
    const sunday = monthGrid("2026-11", 0);
    expect(sunday.length % 7).toBe(0);
    expect(sunday[0]).toBe("2026-11-01"); // 2026-11-01 是周日
    const monday = monthGrid("2026-11", 1);
    expect(monday[0]).toBeNull();
    expect(monday[6]).toBe("2026-11-01"); // 周日开头补 6 格
    expect(monday.filter((cell) => cell !== null)).toHaveLength(30);
  });

  it("tappedCheckins：没打满 +1（升序允许多重），打满清空当天", () => {
    const h = habit({ checkins: ["2026-11-18"] });
    expect(tappedCheckins(h, "2026-11-19")).toEqual(["2026-11-18", "2026-11-19"]);
    expect(tappedCheckins(h, "2026-11-18")).toEqual([]); // target=1 已打满 → 清空
    expect(isComplete(h, "2026-11-18")).toBe(true);
    const multi = habit({ targetCount: 3, checkins: ["2026-11-18", "2026-11-18"] });
    expect(tappedCheckins(multi, "2026-11-18")).toEqual(["2026-11-18", "2026-11-18", "2026-11-18"]);
    expect(isComplete(multi, "2026-11-18")).toBe(false);
    const full = habit({ targetCount: 3, checkins: ["2026-11-18", "2026-11-18", "2026-11-18"] });
    expect(tappedCheckins(full, "2026-11-18")).toEqual([]);
    expect(isComplete(full, "2026-11-18")).toBe(true);
  });
});

describe("每日多次打卡（v0.9.4）", () => {
  it("checkinCount / checkinProgress / habitTarget", () => {
    const h = habit({ targetCount: 4, checkins: ["2026-11-19", "2026-11-19", "2026-11-18"] });
    expect(checkinCount(h, "2026-11-19")).toBe(2);
    expect(checkinCount(h, "2026-11-18")).toBe(1);
    expect(checkinCount(h, "2026-11-17")).toBe(0);
    expect(checkinProgress(h, "2026-11-19")).toEqual({ count: 2, target: 4, ratio: 0.5 });
    expect(checkinProgress(h, "2026-11-17").ratio).toBe(0);
    expect(habitTarget(h)).toBe(4);
    expect(habitTarget(habit({ targetCount: 0 }))).toBe(1); // 脏数据兜底
  });

  it("部分完成不算连续、不算完成率；打满才算", () => {
    const h = habit({
      targetCount: 2,
      checkins: ["2026-11-17", "2026-11-17", "2026-11-18", "2026-11-19", "2026-11-19"]
    });
    // 11-18 只打了一次（部分）→ 连续断在那里
    expect(currentStreak(h, "2026-11-19")).toBe(1);
    expect(bestStreak(h)).toBe(1);
    expect(completionRate(h, "2026-11-19")).toBeCloseTo(2 / 30, 5);
    expect(monthCount(h, "2026-11")).toBe(5); // 累计按次数
    expect(habitStats(h, "2026-11-19").total).toBe(5);
    const full = habit({
      targetCount: 2,
      checkins: ["2026-11-17", "2026-11-17", "2026-11-18", "2026-11-18", "2026-11-19", "2026-11-19"]
    });
    expect(currentStreak(full, "2026-11-19")).toBe(3);
    expect(bestStreak(full)).toBe(3);
  });
});

describe("normalizeHabits 白名单", () => {
  it("脏形状回落默认、checkins 排序保留多重、weekly 空集合回落 daily", () => {
    const list = normalizeHabits({
      habits: [
        { id: "h1", name: " 刷牙 ", checkins: ["2026-11-19", "2026-11-18", "2026-11-19", "bad"], freq: { kind: "weekly", weekdays: [] }, time: "25:00", status: "weird", order: 2, targetCount: 3 },
        { id: "h2", name: "阅读", order: 1 },
        { id: "", name: "无 id" },
        { id: "h3", name: "   " },
        { id: "h4", name: "越界目标", targetCount: 500 },
        { id: "h5", name: "零目标", targetCount: 0 }
      ]
    });
    expect(list.map((item) => item.id)).toEqual(["h4", "h5", "h2", "h1"]); // order 排序（缺省 0 的排前，同 order 按 id）
    expect(list[3].name).toBe("刷牙");
    // 多重日期保留（每日多次打卡）：出现次数 = 当天已打次数
    expect(list[3].checkins).toEqual(["2026-11-18", "2026-11-19", "2026-11-19"]);
    expect(list[3].targetCount).toBe(3);
    expect(list[3].freq).toEqual({ kind: "daily", weekdays: [] });
    expect(list[3].time).toBe("");
    expect(list[3].status).toBe("active");
    expect(list[2].targetCount).toBe(1); // 缺省
    expect(list[0].targetCount).toBe(99); // 夹取上限
    expect(list[1].targetCount).toBe(1); // 夹取下限
  });

  it("数组直给也认（快照/本地两种形状）", () => {
    expect(normalizeHabits([{ id: "h1", name: "x" }])).toHaveLength(1);
    expect(normalizeHabits(undefined)).toEqual([]);
  });
});
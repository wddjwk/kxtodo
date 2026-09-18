import { describe, expect, it } from "vitest";
import {
  REMINDER_PRESETS,
  absoluteReminder,
  absoluteReminderFromLocal,
  canUseBeforeDue,
  dueInstant,
  formatReminderMoment,
  normalizeReminders,
  presetReminder,
  reminderLabel,
  reminderMoment,
  reminderParam,
  remindersParam,
  sortReminders,
  toLocalRfc3339,
  withReminder
} from "../reminders";
import type { ReminderRule } from "../types";

/**
 * 断言一律**时区无关**（CI 的 ubuntu 是 UTC，开发机是 UTC+8）：
 * 需要具体日期时按本地构造，需要「今天」时用相对偏移算出来。
 */
function localDate(offsetDays = 0): string {
  const date = new Date();
  date.setDate(date.getDate() + offsetDays);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

const beforeDue = (minutes: number): ReminderRule => ({ kind: "beforeDue", minutes });

describe("normalizeReminders", () => {
  it("只留下形状合法的规则", () => {
    expect(
      normalizeReminders([
        { kind: "beforeDue", minutes: 60 },
        { kind: "absolute", at: toLocalRfc3339(new Date()) },
        { kind: "beforeDue", minutes: -5 },
        { kind: "beforeDue", minutes: 1.5 },
        { kind: "absolute", at: "不是时间" },
        { kind: "unknown" },
        null,
        "due-60"
      ])
    ).toHaveLength(2);
  });

  it("非数组一律当没有提醒", () => {
    expect(normalizeReminders(undefined)).toEqual([]);
    expect(normalizeReminders({ kind: "beforeDue", minutes: 5 })).toEqual([]);
  });
});

describe("线上规格", () => {
  it("截止前 N 分钟编码成 due-N", () => {
    expect(reminderParam(beforeDue(60))).toBe("due-60");
    expect(reminderParam(beforeDue(0))).toBe("due-0");
  });

  it("绝对时刻原样带上（core 侧再规范化）", () => {
    const at = toLocalRfc3339(new Date());
    expect(reminderParam({ kind: "absolute", at })).toBe(at);
    expect(remindersParam([beforeDue(5), { kind: "absolute", at }])).toEqual(["due-5", at]);
  });
});

describe("reminderMoment / dueInstant", () => {
  it("截止前按本地时区的截止时刻往前推", () => {
    const date = localDate(1);
    const due = dueInstant(date, "18:30");
    expect(due).not.toBeNull();
    expect(reminderMoment(beforeDue(60), date, "18:30")).toBe(due! - 3_600_000);
    expect(reminderMoment(beforeDue(0), date, "18:30")).toBe(due!);
  });

  it("没有分钟时刻就算不出「截止前」（core 同口径拒绝）", () => {
    const date = localDate(1);
    expect(dueInstant(date, "")).toBeNull();
    expect(reminderMoment(beforeDue(60), date, "")).toBeNull();
    expect(reminderMoment(beforeDue(60), undefined, "18:30")).toBeNull();
    expect(canUseBeforeDue(date, "18:30")).toBe(true);
    expect(canUseBeforeDue(date, "")).toBe(false);
  });

  it("绝对时刻与截止日期无关", () => {
    const rule = absoluteReminderFromLocal(localDate(3), "09:05")!;
    expect(reminderMoment(rule)).toBe(reminderMoment(rule, localDate(9), "23:59"));
  });

  it("非法日期回 null 而不是 NaN", () => {
    expect(dueInstant("abc-def-ghi", "10:00")).toBeNull();
    expect(reminderMoment({ kind: "absolute", at: "nope" })).toBeNull();
  });
});

describe("reminderLabel", () => {
  it("截止前用中文跨度，0 分钟是「到点时」", () => {
    expect(reminderLabel(beforeDue(0))).toBe("到点时");
    expect(reminderLabel(beforeDue(5))).toBe("截止前5分钟");
    expect(reminderLabel(beforeDue(60))).toBe("截止前1小时");
    expect(reminderLabel(beforeDue(120))).toBe("截止前2小时");
    expect(reminderLabel(beforeDue(1440))).toBe("截止前1天");
    expect(reminderLabel(beforeDue(90))).toBe("截止前90分钟");
  });

  it("绝对时刻显示月日与时分，跨年补年份", () => {
    const now = new Date();
    const sameYear = new Date(now.getFullYear(), 8, 16, 17, 23, 0, 0);
    expect(formatReminderMoment(sameYear, now)).toBe("9/16 17:23");
    const otherYear = new Date(now.getFullYear() + 1, 0, 2, 9, 5, 0, 0);
    expect(formatReminderMoment(otherYear, now)).toBe(`${now.getFullYear() + 1}/1/2 09:05`);
  });

  it("解析不了的绝对时刻退化成「提醒」而不是 Invalid Date", () => {
    expect(reminderLabel({ kind: "absolute", at: "垃圾" })).toBe("提醒");
  });
});

describe("构造与去重", () => {
  it("本地日期 + 时刻 → 绝对规则，能原样解析回来", () => {
    const date = localDate(2);
    const rule = absoluteReminderFromLocal(date, "07:08")!;
    expect(rule.kind).toBe("absolute");
    const expected = new Date(`${date}T07:08:00`).getTime();
    expect(reminderMoment(rule)).toBe(expected);
  });

  it("非法输入回 null", () => {
    expect(absoluteReminderFromLocal(localDate(), "25:00")).toBeNull();
    expect(absoluteReminderFromLocal("abc-def-ghi", "10:00")).toBeNull();
  });

  it("同一瞬时不重复添加（哪怕规则写法不同）", () => {
    const date = localDate(1);
    const clock = "18:30";
    const viaBeforeDue = beforeDue(60);
    const viaAbsolute = absoluteReminder(reminderMoment(viaBeforeDue, date, clock)!);
    const once = withReminder([], viaBeforeDue, date, clock);
    expect(once).toHaveLength(1);
    expect(withReminder(once, viaAbsolute, date, clock)).toBe(once);
  });

  it("不同瞬时正常追加", () => {
    const date = localDate(1);
    const rules = withReminder([beforeDue(60)], beforeDue(5), date, "18:30");
    expect(rules).toHaveLength(2);
  });
});

describe("sortReminders", () => {
  it("按触发时刻先后排", () => {
    const date = localDate(1);
    const sorted = sortReminders([beforeDue(5), beforeDue(60)], date, "18:30");
    expect(sorted.map((rule) => reminderLabel(rule, date, "18:30"))).toEqual([
      "截止前1小时",
      "截止前5分钟"
    ]);
  });

  it("算不出瞬时的排最后", () => {
    const absolute = absoluteReminderFromLocal(localDate(1), "08:00")!;
    const sorted = sortReminders([beforeDue(60), absolute], "", "");
    expect(sorted).toEqual([absolute, beforeDue(60)]);
  });

  it("不改原数组", () => {
    const source = [beforeDue(5), beforeDue(60)];
    sortReminders(source, localDate(1), "18:30");
    expect(source[0]).toEqual(beforeDue(5));
  });
});

describe("预设菜单", () => {
  it("五个固定选项，「截止前」两项需要有分钟时刻", () => {
    expect(REMINDER_PRESETS.map((preset) => preset.id)).toEqual([
      "in1h",
      "in2h",
      "beforeDue60",
      "beforeDue5",
      "custom"
    ]);
    expect(REMINDER_PRESETS.filter((preset) => preset.needsDueTime).map((preset) => preset.id)).toEqual([
      "beforeDue60",
      "beforeDue5"
    ]);
  });

  it("一小时后 / 两小时后是绝对时刻，相差一小时", () => {
    // 编码到 RFC3339 只精确到秒（提醒本来就是分钟级），比较前先抹掉毫秒
    const now = new Date(Math.floor(Date.now() / 1000) * 1000);
    const in1h = presetReminder("in1h", now)!;
    const in2h = presetReminder("in2h", now)!;
    expect(reminderMoment(in1h)).toBe(now.getTime() + 3_600_000);
    expect(reminderMoment(in2h)! - reminderMoment(in1h)!).toBe(3_600_000);
  });

  it("截止前两项落成 beforeDue 规则；自定义交给面板", () => {
    expect(presetReminder("beforeDue60")).toEqual(beforeDue(60));
    expect(presetReminder("beforeDue5")).toEqual(beforeDue(5));
    expect(presetReminder("custom")).toBeNull();
  });
});

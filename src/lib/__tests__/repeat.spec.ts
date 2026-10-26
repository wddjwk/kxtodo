import { describe, expect, it } from "vitest";

import {
  buildRepeatPreset,
  eventColors,
  expandForRange,
  matchRepeatPreset,
  occurrenceDates,
  repeatLabel,
  seriesChangesFromDrag
} from "../calendar";
import type { CalendarCategory, CalendarEvent, CalendarRepeat } from "../types";

/**
 * 重复日程（v0.9.1）。这张用例表与 core `crates/core/src/recurrence.rs` 的单元测试
 * **逐条对应**：前端渲染视图、core 算提醒与 CLI 展开，两边跑出来的日期必须一致。
 * 改任何一边都要改另一边（口径写在两处文件的文件头注释里）。
 */

function repeat(partial: Partial<CalendarRepeat>): CalendarRepeat {
  return {
    freq: "day",
    interval: 1,
    byWeekday: [],
    byMonthDay: [],
    end: "never",
    count: 0,
    until: "",
    ...partial
  };
}

function event(partial: Partial<CalendarEvent> & { id: string }): CalendarEvent {
  return {
    title: "",
    details: "",
    startDate: "2026-10-25",
    startTime: "",
    endDate: "2026-10-25",
    endTime: "",
    allDay: true,
    reminders: [],
    location: "",
    categoryId: "",
    repeat: null,
    exceptDates: [],
    taskId: "",
    createdAt: "2026-10-01T00:00:00.000Z",
    ...partial
  };
}

describe("重复日程的展开（与 core recurrence.rs 同一张表）", () => {
  it("每天：区间内逐日", () => {
    const ev = event({ id: "r", startDate: "2026-10-01", repeat: repeat({ freq: "day" }) });
    expect(occurrenceDates(ev, "2026-10-05", "2026-10-08")).toEqual([
      "2026-10-05",
      "2026-10-06",
      "2026-10-07",
      "2026-10-08"
    ]);
  });

  it("每 3 天：跳过区间开头之后仍对齐系列锚点", () => {
    const ev = event({ id: "r", startDate: "2026-10-01", repeat: repeat({ freq: "day", interval: 3 }) });
    expect(occurrenceDates(ev, "2026-10-05", "2026-10-12")).toEqual(["2026-10-07", "2026-10-10"]);
  });

  it("工作日：周次锚在周一，与「一周从周几开始」的显示偏好无关", () => {
    // 2026-10-07 是周三
    const ev = event({
      id: "r",
      startDate: "2026-10-07",
      repeat: repeat({ freq: "week", byWeekday: [1, 2, 3, 4, 5] })
    });
    expect(occurrenceDates(ev, "2026-10-05", "2026-10-18")).toEqual([
      "2026-10-07",
      "2026-10-08",
      "2026-10-09",
      "2026-10-12",
      "2026-10-13",
      "2026-10-14",
      "2026-10-15",
      "2026-10-16"
    ]);
  });

  it("每周只选周一：开始日之前那次不产出，也不占次数名额", () => {
    const ev = event({ id: "r", startDate: "2026-10-07", repeat: repeat({ freq: "week", byWeekday: [1] }) });
    expect(occurrenceDates(ev, "2026-10-01", "2026-10-31")).toEqual(["2026-10-12", "2026-10-19", "2026-10-26"]);
  });

  it("每两周：奇偶周跟着锚点走", () => {
    const ev = event({
      id: "r",
      startDate: "2026-10-07",
      repeat: repeat({ freq: "week", interval: 2, byWeekday: [1, 4] })
    });
    expect(occurrenceDates(ev, "2026-10-05", "2026-11-08")).toEqual([
      "2026-10-08",
      "2026-10-19",
      "2026-10-22",
      "2026-11-02",
      "2026-11-05"
    ]);
  });

  it("每月 31 日：短月跳过，不折算到月末", () => {
    const ev = event({ id: "r", startDate: "2026-01-31", repeat: repeat({ freq: "month", byMonthDay: [31] }) });
    expect(occurrenceDates(ev, "2026-01-01", "2026-08-31")).toEqual([
      "2026-01-31",
      "2026-03-31",
      "2026-05-31",
      "2026-07-31",
      "2026-08-31"
    ]);
  });

  it("每月：没给日号就按开始日期的日号", () => {
    const ev = event({ id: "r", startDate: "2026-10-07", repeat: repeat({ freq: "month" }) });
    expect(occurrenceDates(ev, "2026-10-01", "2027-01-31")).toEqual([
      "2026-10-07",
      "2026-11-07",
      "2026-12-07",
      "2027-01-07"
    ]);
  });

  it("每年 2 月 29 日：平年跳过", () => {
    const ev = event({ id: "r", startDate: "2024-02-29", repeat: repeat({ freq: "year" }) });
    expect(occurrenceDates(ev, "2024-01-01", "2032-12-31")).toEqual(["2024-02-29", "2028-02-29", "2032-02-29"]);
  });

  it("按次数结束：名额是全局的，区间之前的也算", () => {
    const ev = event({ id: "r", startDate: "2026-10-01", repeat: repeat({ freq: "day", end: "count", count: 3 }) });
    expect(occurrenceDates(ev, "2026-10-02", "2026-10-31")).toEqual(["2026-10-02", "2026-10-03"]);
    expect(occurrenceDates(ev, "2026-11-01", "2026-11-30")).toEqual([]);
  });

  it("例外日期被跳过，但同样占次数名额", () => {
    const ev = event({
      id: "r",
      startDate: "2026-10-01",
      exceptDates: ["2026-10-02"],
      repeat: repeat({ freq: "day", end: "count", count: 3 })
    });
    expect(occurrenceDates(ev, "2026-10-01", "2026-10-31")).toEqual(["2026-10-01", "2026-10-03"]);
  });

  it("按日期结束：截止日含当天", () => {
    const ev = event({
      id: "r",
      startDate: "2026-10-01",
      repeat: repeat({ freq: "day", end: "until", until: "2026-10-05" })
    });
    expect(occurrenceDates(ev, "2026-10-01", "2026-10-31")).toEqual([
      "2026-10-01",
      "2026-10-02",
      "2026-10-03",
      "2026-10-04",
      "2026-10-05"
    ]);
  });

  it("不重复的事件就是一次", () => {
    const ev = event({ id: "r", startDate: "2026-10-07" });
    expect(occurrenceDates(ev, "2026-10-01", "2026-10-31")).toEqual(["2026-10-07"]);
    expect(occurrenceDates(ev, "2026-11-01", "2026-11-30")).toEqual([]);
  });

  it("坏规则不炸：until 早于开始 = 一次都没有", () => {
    const ev = event({
      id: "r",
      startDate: "2026-10-01",
      repeat: repeat({ freq: "day", end: "until", until: "2026-09-01" })
    });
    expect(occurrenceDates(ev, "2026-08-01", "2026-12-31")).toEqual([]);
  });

  it("展开时跨天事件整体平移，时长不变", () => {
    const ev = event({
      id: "r",
      startDate: "2026-10-01",
      startTime: "22:00",
      endDate: "2026-10-02",
      endTime: "01:00",
      allDay: false,
      repeat: repeat({ freq: "week" })
    });
    const out = expandForRange([ev], "2026-10-08", "2026-10-09");
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({
      startDate: "2026-10-08",
      startTime: "22:00",
      endDate: "2026-10-09",
      endTime: "01:00",
      occurrence: "2026-10-08",
      recurring: true
    });
  });

  it("跨天事件开始日在区间之前、身体还在区间里的也要出现", () => {
    const ev = event({ id: "r", startDate: "2026-10-01", endDate: "2026-10-05", repeat: repeat({ freq: "month" }) });
    const out = expandForRange([ev], "2026-11-03", "2026-11-30");
    expect(out.map((item) => item.occurrence)).toEqual(["2026-11-01"]);
    expect(out[0].endDate).toBe("2026-11-05");
  });

  it("非重复事件原样带上（按天过滤交给视图）", () => {
    const ev = event({ id: "plain", startDate: "2026-01-01" });
    const out = expandForRange([ev], "2026-10-01", "2026-10-31");
    expect(out).toHaveLength(1);
    expect(out[0].occurrence).toBe("2026-01-01");
    expect(out[0].recurring).toBe(false);
  });
});

describe("重复预设与展示串", () => {
  it("预设造出来的规则能被认回去", () => {
    for (const id of ["daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"] as const) {
      expect(matchRepeatPreset(buildRepeatPreset(id, "2026-10-07"), "2026-10-07"), id).toBe(id);
    }
    expect(buildRepeatPreset("none", "2026-10-07")).toBeNull();
    expect(matchRepeatPreset(null, "2026-10-07")).toBe("none");
  });

  it("「每周」刻意不写死星期（跟开始日期走，与 core preset 同口径）", () => {
    expect(buildRepeatPreset("weekly", "2026-10-07")?.byWeekday).toEqual([]);
    expect(buildRepeatPreset("biweekly", "2026-10-07")?.byWeekday).toEqual([]);
    expect(buildRepeatPreset("weekdays", "2026-10-07")?.byWeekday).toEqual([1, 2, 3, 4, 5]);
    expect(buildRepeatPreset("monthly", "2026-10-07")?.byMonthDay).toEqual([7]);
    expect(repeatLabel(buildRepeatPreset("weekly", "2026-10-07"))).toBe("每周");
  });

  it("对不上任何预设就是自定义", () => {
    expect(matchRepeatPreset(repeat({ freq: "week", interval: 3, byWeekday: [2, 5] }), "2026-10-07")).toBe("custom");
  });

  it("展示串与 core describe 同口径", () => {
    expect(repeatLabel(null)).toBe("");
    expect(repeatLabel(repeat({ freq: "day" }))).toBe("每天");
    expect(repeatLabel(repeat({ freq: "day", interval: 3 }))).toBe("每 3 天");
    expect(repeatLabel(repeat({ freq: "week", interval: 2, byWeekday: [1, 3, 5] }))).toBe("每 2 周的周一、三、五");
    expect(repeatLabel(repeat({ freq: "month", interval: 2 }))).toBe("每 2 个月");
    expect(repeatLabel(repeat({ freq: "month", byMonthDay: [1, 15] }))).toBe("每个月的 1 日、15 日");
    expect(repeatLabel(repeat({ freq: "year" }))).toBe("每年");
    expect(repeatLabel(repeat({ freq: "day", end: "count", count: 10 }))).toBe("每天（共 10 次）");
    expect(repeatLabel(repeat({ freq: "day", end: "until", until: "2026-12-31" }))).toBe("每天（至 12-31）");
  });
});

describe("拖动重复日程 = 改整个系列", () => {
  const next = { startDate: "2026-10-09", startTime: "14:00", endDate: "2026-10-09", endTime: "15:30" };

  it("不重复的事件原样落地", () => {
    const ev = event({
      id: "p",
      startDate: "2026-10-08",
      startTime: "09:00",
      endDate: "2026-10-08",
      endTime: "10:00",
      allDay: false
    });
    expect(seriesChangesFromDrag(ev, "2026-10-08", next)).toEqual(next);
  });

  it("空星期集合的「每周」：拖动只改锚点日期，星期自己跟着走", () => {
    const ev = event({
      id: "w0",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      repeat: repeat({ freq: "week" })
    });
    const changes = seriesChangesFromDrag(ev, "2026-10-08", next);
    expect(changes.startDate).toBe("2026-10-02");
    expect(changes.repeat).toBeUndefined();
  });

  it("显式单星期的周规则：日期平移且星期跟着改", () => {
    const ev = event({
      id: "w",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      repeat: repeat({ freq: "week", byWeekday: [4] })
    });
    const changes = seriesChangesFromDrag(ev, "2026-10-08", next);
    expect(changes.startDate).toBe("2026-10-02");
    expect(changes.startTime).toBe("14:00");
    expect(changes.endTime).toBe("15:30");
    expect(changes.repeat?.byWeekday).toEqual([5]);
  });

  it("多星期的周规则（工作日）：只改时刻与时长，不换哪几天", () => {
    const ev = event({
      id: "w",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      repeat: repeat({ freq: "week", byWeekday: [1, 2, 3, 4, 5] })
    });
    const changes = seriesChangesFromDrag(ev, "2026-10-08", next);
    expect(changes.startDate).toBe("2026-10-01");
    expect(changes.startTime).toBe("14:00");
    expect(changes.endDate).toBe("2026-10-01");
    expect(changes.repeat).toBeUndefined();
  });

  it("按月某一天：日期平移且日号跟着改", () => {
    const ev = event({
      id: "m",
      startDate: "2026-09-15",
      startTime: "09:00",
      endDate: "2026-09-15",
      endTime: "10:00",
      allDay: false,
      repeat: repeat({ freq: "month", byMonthDay: [15] })
    });
    const changes = seriesChangesFromDrag(ev, "2026-10-15", next);
    // 系列锚点跟着平移同样的天数（09-15 − 6 天），日号集合换成新的那一天
    expect(changes.startDate).toBe("2026-09-09");
    expect(changes.repeat?.byMonthDay).toEqual([9]);
  });

  it("系列平移时例外日期跟着平移（否则删掉的那一次会错位）", () => {
    const ev = event({
      id: "x",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      exceptDates: ["2026-10-08", "2026-10-15"],
      repeat: repeat({ freq: "day" })
    });
    // 把 10-08 那一次拖到 10-09（delta = +1）：系列锚点与两个例外日期一起后移一天
    const changes = seriesChangesFromDrag(ev, "2026-10-08", next);
    expect(changes.startDate).toBe("2026-10-02");
    expect(changes.exceptDates).toEqual(["2026-10-09", "2026-10-16"]);
  });

  it("多星期周规则不平移，例外日期也就不动", () => {
    const ev = event({
      id: "y",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      exceptDates: ["2026-10-08"],
      repeat: repeat({ freq: "week", byWeekday: [1, 3, 5] })
    });
    expect(seriesChangesFromDrag(ev, "2026-10-08", next).exceptDates).toBeUndefined();
  });

  it("拖到次日结束（跨午夜）时时长跟着走", () => {
    const ev = event({
      id: "n",
      startDate: "2026-10-01",
      startTime: "09:00",
      endDate: "2026-10-01",
      endTime: "10:00",
      allDay: false,
      repeat: repeat({ freq: "day" })
    });
    const changes = seriesChangesFromDrag(ev, "2026-10-08", {
      startDate: "2026-10-09",
      startTime: "23:00",
      endDate: "2026-10-10",
      endTime: "01:00"
    });
    expect(changes.startDate).toBe("2026-10-02");
    expect(changes.endDate).toBe("2026-10-03");
    expect(changes.endTime).toBe("01:00");
  });
});

describe("分类配色（v0.9.1）", () => {
  const categories: CalendarCategory[] = [
    { id: "c1", name: "工作", color: "#1f6feb", isDefault: false, order: 1, createdAt: "2026-10-01T00:00:00.000Z" },
    { id: "c2", name: "无色", color: "", isDefault: false, order: 2, createdAt: "2026-10-01T00:00:00.000Z" }
  ];

  const luminance = (hex: string): number => {
    const int = Number.parseInt(hex.slice(1), 16);
    return 0.2126 * ((int >> 16) & 255) + 0.7152 * ((int >> 8) & 255) + 0.0722 * (int & 255);
  };

  it("有分类跟分类色，块内淡色明显比主色亮（不影响阅读）", () => {
    const colors = eventColors({ categoryId: "c1" }, categories, "#7c5cbf");
    expect(colors.main).toBe("#1f6feb");
    expect(colors.soft).not.toBe(colors.main);
    expect(luminance(colors.soft) - luminance(colors.main)).toBeGreaterThan(90);
    expect(colors.onMain).toBe("#ffffff");
    // 边框色介于两者之间
    expect(luminance(colors.line)).toBeGreaterThan(luminance(colors.main));
    expect(luminance(colors.line)).toBeLessThan(luminance(colors.soft));
  });

  it("分类没配色 / 没有分类 / 分类已删都回落到主题色", () => {
    expect(eventColors({ categoryId: "c2" }, categories, "#7c5cbf").main).toBe("#7c5cbf");
    expect(eventColors({ categoryId: "" }, categories, "#7c5cbf").main).toBe("#7c5cbf");
    expect(eventColors({ categoryId: "没了" }, categories, "#7c5cbf").main).toBe("#7c5cbf");
  });

  it("亮色底上自动换深字（全天芯片是实心底）", () => {
    expect(eventColors({ categoryId: "" }, [], "#f5f0a0").onMain).toBe("#23272f");
  });
});

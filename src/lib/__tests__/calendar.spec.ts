import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "../types";
import {
  addDays,
  allDayEventsFor,
  calendarToday,
  diffDays,
  layoutDaySegments,
  minutesOf,
  formatMinutes,
  monthCellEvents,
  monthGridDates,
  normalizeCalendarTimes,
  rangeTitle,
  segmentForDay,
  shiftRange,
  snapMinutes,
  startOfWeek,
  threeDayDates,
  weekDates,
  type DaySegment
} from "../calendar";

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
    taskId: "",
    createdAt: "2026-10-01T00:00:00.000Z",
    ...partial
  } as CalendarEvent;
}

describe("日期算术（时区无关）", () => {
  it("addDays 跨月跨年", () => {
    expect(addDays("2026-10-31", 1)).toBe("2026-11-01");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
    expect(addDays("2028-03-01", -1), "闰年").toBe("2028-02-29");
  });

  it("diffDays 与 addDays 互逆", () => {
    expect(diffDays("2026-10-20", "2026-10-25")).toBe(5);
    expect(diffDays("2026-10-25", "2026-10-20")).toBe(-5);
    expect(diffDays("2026-12-30", "2027-01-02")).toBe(3);
  });

  it("calendarToday 返回 YYYY-MM-DD", () => {
    expect(calendarToday()).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe("视图区间", () => {
  it("周一起始的周视图", () => {
    // 2026-10-25 是周日
    expect(weekDates("2026-10-25", 1)).toEqual([
      "2026-10-19", "2026-10-20", "2026-10-21", "2026-10-22", "2026-10-23", "2026-10-24", "2026-10-25"
    ]);
    expect(weekDates("2026-10-25", 0)[0]).toBe("2026-10-25");
  });

  it("startOfWeek 落在周首", () => {
    expect(startOfWeek("2026-10-21", 1)).toBe("2026-10-19");
    expect(startOfWeek("2026-10-19", 1), "本来就是周一").toBe("2026-10-19");
  });

  it("三日视图以锚点为中心", () => {
    expect(threeDayDates("2026-10-25")).toEqual(["2026-10-24", "2026-10-25", "2026-10-26"]);
  });

  it("月网格 42 格、首格是 1 号所在周的第一天", () => {
    const cells = monthGridDates("2026-10-15", 1);
    expect(cells).toHaveLength(42);
    expect(cells[0], "2026-10-01 是周四，周一起始往前补 3 天").toBe("2026-09-28");
    expect(cells).toContain("2026-10-01");
    expect(cells[41]).toBe("2026-11-08");
  });

  it("翻页：月按整月、周按七天、三日按三天", () => {
    expect(shiftRange("2026-10-25", "month", 1)).toBe("2026-11-01");
    expect(shiftRange("2026-10-25", "month", -1)).toBe("2026-09-01");
    expect(shiftRange("2026-10-31", "month", 1), "月末不跳月").toBe("2026-11-01");
    expect(shiftRange("2026-10-25", "week", 1)).toBe("2026-11-01");
    expect(shiftRange("2026-10-25", "threeday", -1)).toBe("2026-10-22");
    expect(shiftRange("2026-10-25", "day", 1)).toBe("2026-10-26");
  });

  it("区间标题：同月收敛、跨月/跨年展开", () => {
    expect(rangeTitle(weekDates("2026-10-22", 1), "week", "2026-10-22")).toBe("2026年10月19日 – 25日");
    expect(rangeTitle(weekDates("2026-12-30", 1), "week", "2026-12-30")).toBe("2026年12月28日 – 2027年1月3日");
    expect(rangeTitle(monthGridDates("2026-10-15", 1), "month", "2026-10-15")).toBe("2026年10月");
    expect(rangeTitle(["2026-10-25"], "day", "2026-10-25")).toBe("2026年10月25日");
  });
});

describe("时刻", () => {
  it("minutesOf / formatMinutes 往返", () => {
    expect(minutesOf("09:30")).toBe(570);
    expect(formatMinutes(570)).toBe("09:30");
    expect(formatMinutes(1440)).toBe("00:00");
    expect(minutesOf("bad")).toBe(0);
  });

  it("snapMinutes 按 15 分钟吸附", () => {
    expect(snapMinutes(0, 15)).toBe(0);
    expect(snapMinutes(7, 15)).toBe(0);
    expect(snapMinutes(8, 15)).toBe(15);
    expect(snapMinutes(50, 30)).toBe(60);
  });
});

describe("事件切段", () => {
  it("当天的普通事件", () => {
    const segment = segmentForDay(
      event({ id: "e1", allDay: false, startTime: "09:00", endTime: "10:30" }),
      "2026-10-25"
    );
    expect(segment).toMatchObject({ startMin: 540, endMin: 630, continuesBefore: false, continuesAfter: false });
  });

  it("全天事件不进时间网格（住全天栏）", () => {
    expect(segmentForDay(event({ id: "e1", allDay: true }), "2026-10-25")).toBeNull();
    expect(allDayEventsFor([event({ id: "e1", allDay: true })], "2026-10-25")).toHaveLength(1);
  });

  it("跨天事件逐日切段", () => {
    const trip = event({
      id: "e1",
      allDay: false,
      startDate: "2026-10-24",
      startTime: "20:00",
      endDate: "2026-10-26",
      endTime: "09:00"
    });
    expect(segmentForDay(trip, "2026-10-24")).toMatchObject({ startMin: 1200, endMin: 1440, continuesAfter: true });
    expect(segmentForDay(trip, "2026-10-25")).toMatchObject({ startMin: 0, endMin: 1440, continuesBefore: true, continuesAfter: true });
    expect(segmentForDay(trip, "2026-10-26")).toMatchObject({ startMin: 0, endMin: 540, continuesBefore: true });
    expect(segmentForDay(trip, "2026-10-27")).toBeNull();
  });

  it("结束是次日 00:00 时画满当天", () => {
    const night = event({
      id: "e1",
      allDay: false,
      startDate: "2026-10-25",
      startTime: "23:00",
      endDate: "2026-10-25",
      endTime: "00:00"
    });
    expect(segmentForDay(night, "2026-10-25")).toMatchObject({ startMin: 1380, endMin: 1440 });
  });

  it("没写结束时刻按一小时", () => {
    const segment = segmentForDay(
      event({ id: "e1", allDay: false, startTime: "14:00", endTime: "" }),
      "2026-10-25"
    );
    expect(segment).toMatchObject({ startMin: 840, endMin: 900 });
  });
});

describe("冲突占位布局", () => {
  function seg(id: string, startMin: number, endMin: number): DaySegment {
    return {
      event: event({ id, allDay: false }),
      startMin,
      endMin,
      continuesBefore: false,
      continuesAfter: false
    };
  }

  it("不重叠的事件各占整宽", () => {
    const blocks = layoutDaySegments([seg("a", 540, 600), seg("b", 600, 660)]);
    expect(blocks).toHaveLength(2);
    expect(blocks.every((block) => block.columns === 1 && block.column === 0)).toBe(true);
  });

  it("两个重叠事件平分两列", () => {
    const blocks = layoutDaySegments([seg("a", 540, 660), seg("b", 600, 720)]);
    const a = blocks.find((block) => block.event.id === "a")!;
    const b = blocks.find((block) => block.event.id === "b")!;
    expect(a.columns).toBe(2);
    expect(b.columns).toBe(2);
    expect(a.column).not.toBe(b.column);
  });

  it("链式重叠归同一簇：a-b 重叠、b-c 重叠、a-c 不重叠则复用列（两列）", () => {
    const blocks = layoutDaySegments([seg("a", 540, 620), seg("b", 600, 700), seg("c", 680, 760)]);
    expect(blocks.every((block) => block.columns === 2)).toBe(true);
    const a = blocks.find((block) => block.event.id === "a")!;
    const b = blocks.find((block) => block.event.id === "b")!;
    const c = blocks.find((block) => block.event.id === "c")!;
    expect(a.column).toBe(c.column);
    expect(b.column).not.toBe(a.column);
  });

  it("三个互相重叠的事件分三列", () => {
    const blocks = layoutDaySegments([seg("a", 540, 700), seg("b", 570, 720), seg("c", 600, 680)]);
    expect(blocks.every((block) => block.columns === 3)).toBe(true);
    const columns = blocks.map((block) => block.column).sort();
    expect(columns).toEqual([0, 1, 2]);
  });

  it("簇之间互不影响", () => {
    const blocks = layoutDaySegments([
      seg("a", 540, 600),
      seg("b", 570, 640),
      seg("c", 900, 960)
    ]);
    expect(blocks.find((block) => block.event.id === "a")!.columns).toBe(2);
    expect(blocks.find((block) => block.event.id === "c")!.columns).toBe(1);
  });

  it("输入顺序无关", () => {
    const forward = layoutDaySegments([seg("a", 540, 660), seg("b", 600, 720)]);
    const backward = layoutDaySegments([seg("b", 600, 720), seg("a", 540, 660)]);
    expect(backward.map((block) => block.event.id)).toEqual(forward.map((block) => block.event.id));
  });
});

describe("月视图芯片排序", () => {
  it("全天在前，其后按开始时刻", () => {
    const events = [
      event({ id: "t2", allDay: false, startTime: "14:00", endTime: "15:00" }),
      event({ id: "ad", allDay: true }),
      event({ id: "t1", allDay: false, startTime: "09:00", endTime: "10:00" })
    ];
    const ids = monthCellEvents(events, "2026-10-25").map((item) => item.id);
    expect(ids).toEqual(["ad", "t1", "t2"]);
  });
});

describe("新建事件的缺省时间（core normalize_times 的镜像）", () => {
  it("只给日期 = 全天", () => {
    expect(normalizeCalendarTimes({ startDate: "2026-10-25" })).toEqual({
      startDate: "2026-10-25",
      startTime: "",
      endDate: "2026-10-25",
      endTime: "",
      allDay: true
    });
  });

  it("定时事件缺省结束 = 开始 + 1 小时，跨午夜进位", () => {
    expect(normalizeCalendarTimes({ startDate: "2026-10-25", startTime: "09:00" })).toMatchObject({
      endTime: "10:00",
      allDay: false
    });
    expect(normalizeCalendarTimes({ startDate: "2026-10-25", startTime: "23:30" })).toMatchObject({
      endDate: "2026-10-26",
      endTime: "00:30"
    });
  });

  it("显式 allDay 清空时刻", () => {
    expect(
      normalizeCalendarTimes({ startDate: "2026-10-25", startTime: "09:00", allDay: true })
    ).toMatchObject({ allDay: true, startTime: "", endTime: "" });
  });
});

/**
 * 时刻（HH:MM）这一小块的单元测试。
 *
 * 权威口径是 core 的 `crates/core/src/time.rs::parse_clock`（三段纯数字、时 <24、
 * 分 <60、秒 <60、秒舍掉）与 `crates/core/src/diary_archive.rs::{compose_timestamp,time_of}`。
 * 标着「不一致（现状）」的用例是前端与 core 对不上的地方——**只记录，不改代码**，
 * 汇总在给用户的报告里。
 */
import { describe, expect, it } from "vitest";

import { clockOf, composeTimestamp, displayClock, formatClock, nowClock, parseClock } from "../clock";

describe("parseClock（与 core time.rs::parse_clock 同口径）", () => {
  it("正常路径：H:M / HH:MM / HH:MM:SS，秒被舍掉", () => {
    expect(parseClock("9:05")).toEqual({ hour: 9, minute: 5 });
    expect(parseClock("09:05")).toEqual({ hour: 9, minute: 5 });
    expect(parseClock("09:05:30")).toEqual({ hour: 9, minute: 5 }); // 秒舍掉
    expect(parseClock("1:2")).toEqual({ hour: 1, minute: 2 }); // core 收，前端曾经拒
    expect(parseClock("0:0")).toEqual({ hour: 0, minute: 0 });
    expect(parseClock("23:59")).toEqual({ hour: 23, minute: 59 });
    expect(parseClock("23:59:59")).toEqual({ hour: 23, minute: 59 });
    expect(parseClock("00:00:00")).toEqual({ hour: 0, minute: 0 });
  });

  it("边界：前后空白 trim", () => {
    expect(parseClock("  09:05  ")).toEqual({ hour: 9, minute: 5 });
    expect(parseClock("\t09:05\n")).toEqual({ hour: 9, minute: 5 });
    expect(parseClock("  ")).toBeNull();
  });

  it("回归：必须有尾锚 —— `9:05xyz` 是 null（旧实现会当成 09:05）", () => {
    expect(parseClock("9:05xyz")).toBeNull();
    expect(parseClock("09:05x")).toBeNull();
    expect(parseClock("xx09:05")).toBeNull();
    expect(parseClock("09:05:30extra")).toBeNull();
  });

  it("边界：越界值一律 null", () => {
    for (const raw of ["24:00", "09:60", "09:05:60", "09:05:99", "99:99", "24:00:00", "-1:00", "09:-5"]) {
      expect(parseClock(raw), `parseClock(${JSON.stringify(raw)})`).toBeNull();
    }
  });

  it("边界：段数不对 / 空段 / 非数字一律 null", () => {
    for (const raw of ["09", "9:", ":05", ":", "", "09:05:30:40", "1:2:3:4", "09:05:", "ab:cd", "09:0a", "１２:３４", "123:45", "09 05"]) {
      expect(parseClock(raw), `parseClock(${JSON.stringify(raw)})`).toBeNull();
    }
  });

  it("边界：非字符串一律 null（不抛）", () => {
    expect(parseClock()).toBeNull();
    expect(parseClock(undefined)).toBeNull();
    expect(parseClock(null)).toBeNull();
    expect(parseClock(905 as unknown as string)).toBeNull();
    expect(parseClock({ hour: 9 } as unknown as string)).toBeNull();
    expect(parseClock(["09:05"] as unknown as string)).toBeNull();
  });

  it("不一致（现状）：core 的 `+9:+5` 能过（Rust u32::from_str 收前导 +），前端拒", () => {
    // 前端 \d{1,2} 不认 "+"；core 是 part.parse::<u32>()，Rust 允许前导 '+'。
    // 现实中没人这么输，但两侧结论相反，记一笔。详见报告。
    expect(parseClock("+9:+5")).toBeNull();
    expect(parseClock("+9:05")).toBeNull();
  });
});

describe("formatClock（补零）", () => {
  it("正常路径：两位补零", () => {
    expect(formatClock(9, 5)).toBe("09:05");
    expect(formatClock(0, 0)).toBe("00:00");
    expect(formatClock(23, 59)).toBe("23:59");
    expect(formatClock(12, 30)).toBe("12:30");
  });

  it("边界（现状）：不做范围校验，给什么拼什么", () => {
    expect(formatClock(99, 5)).toBe("99:05");
    expect(formatClock(9, 60)).toBe("09:60");
    // 负数不会被补零成两位（String(-1).padStart(2,"0") === "-1"）
    expect(formatClock(9, -1)).toBe("09:-1");
  });
});

describe("nowClock", () => {
  it("正常路径：等于给定 Date 的 HH:MM", () => {
    const date = new Date(2026, 8, 8, 7, 3, 0); // 本地 07:03
    expect(nowClock(date)).toBe("07:03");
    expect(nowClock(new Date(2026, 0, 1, 23, 59))).toBe("23:59");
    expect(nowClock()).toMatch(/^\d{2}:\d{2}$/);
  });
});

describe("displayClock（裸时刻字段归一）", () => {
  it("正常路径：HH:MM 与 HH:MM:SS 都归一成 HH:MM", () => {
    expect(displayClock("18:19:00")).toBe("18:19"); // v0.7.3 的记账卡片就是这一条
    expect(displayClock("18:19")).toBe("18:19");
    expect(displayClock("9:5")).toBe("09:05");
    expect(displayClock("0:0")).toBe("00:00");
  });

  it("边界：解析不出来给空串（不给一个错的时间）", () => {
    expect(displayClock("")).toBe("");
    expect(displayClock(undefined)).toBe("");
    expect(displayClock(null)).toBe("");
    expect(displayClock("nope")).toBe("");
    expect(displayClock("24:00")).toBe("");
    expect(displayClock("18:19:00+08:00")).toBe(""); // 带时区的时刻字段不归一
  });
});

describe("clockOf（从 ISO 时间戳取**本地** HH:MM）", () => {
  /** 独立算出某个瞬时的本地钟点：只用 getUTC*，不借被测实现的那条路径。 */
  const localClock = (instant: Date): string => {
    const shifted = new Date(instant.getTime() - instant.getTimezoneOffset() * 60000);
    return formatClock(shifted.getUTCHours(), shifted.getUTCMinutes());
  };

  it("正常路径：无时区偏移的时间戳按本地读（JS 规范如此，各时区同结论）", () => {
    expect(clockOf("2026-09-08T18:19:00")).toBe("18:19");
    expect(clockOf("2026-09-08T18:19")).toBe("18:19");
    expect(clockOf("2026-09-08T09:05:00")).toBe("09:05");
  });

  it("回归（v0.7.3 的坑）：喂裸时刻 / 裸日期一律空串，别拿它归一时刻字段", () => {
    expect(clockOf("18:19")).toBe(""); // 长度 5 < 11
    expect(clockOf("18:19:00")).toBe(""); // 长度 8 < 11 → ""（这条正是 displayClock 存在的理由）
    expect(clockOf("2026-09-08")).toBe(""); // 只有日期（Date 会按 UTC 解析，放进来就错了）
    expect(clockOf("")).toBe("");
    expect(clockOf(undefined)).toBe("");
    expect(clockOf(null)).toBe("");
    expect(clockOf(12345 as unknown as string)).toBe("");
    expect(clockOf("不是时间戳的十一个字符")).toBe("");
  });

  it("回归（v0.8.0）：真实落盘的两种 createdAt 都要能读出时刻", () => {
    // 这两种才是本仓库真实落盘的格式（diary.json 里两种都有）。早先 slice(11) 之后
    // parseClock 的尾锚拒掉 `.693Z` / `+08:00` 尾巴 → 一律空串，日记的写作时刻在
    // 编辑器与菜单里从来不显示（本机 7 篇日记没有一篇能解析）。
    const zulu = "2026-09-12T04:20:41.693Z"; // new Date().toISOString() / core now_iso()
    const offset = "2026-09-13T12:20:00+08:00"; // core diary_archive::compose_timestamp
    expect(clockOf(zulu)).not.toBe("");
    expect(clockOf(offset)).not.toBe("");
    expect(clockOf(zulu)).toBe(localClock(new Date(zulu)));
    expect(clockOf(offset)).toBe(localClock(new Date(offset)));
  });

  it("回归（v0.8.0）：同一瞬时的不同写法给出同一个本地钟点（时区换算与 core 的 time_of 同口径）", () => {
    // 早先按字面切片：Z 时间戳显示的是 UTC 钟点，与 core 差一个时区。
    expect(clockOf("2026-09-12T04:20:41.693Z")).toBe(clockOf("2026-09-12T12:20:41.693+08:00"));
    expect(clockOf("2026-09-12T04:20:00Z")).toBe(clockOf("2026-09-11T23:20:00-05:00"));
  });
});

describe("composeTimestamp（日期 + 时刻 → 时间戳）", () => {
  it("正常路径：秒固定 00，可显式给秒", () => {
    expect(composeTimestamp("2026-09-08", "18:19")).toBe("2026-09-08T18:19:00");
    expect(composeTimestamp("2026-09-08", "9:5")).toBe("2026-09-08T09:05:00");
    expect(composeTimestamp("2026-09-08", "18:19", "30")).toBe("2026-09-08T18:19:30");
    // 时刻里带的秒被丢掉，用第三个参数
    expect(composeTimestamp("2026-09-08", "18:19:45")).toBe("2026-09-08T18:19:00");
  });

  it("边界：时刻解析不出来就原样返回日期（不拼半个时间戳）", () => {
    expect(composeTimestamp("2026-09-08", "")).toBe("2026-09-08");
    expect(composeTimestamp("2026-09-08", "bad")).toBe("2026-09-08");
    expect(composeTimestamp("2026-09-08", "24:00")).toBe("2026-09-08");
  });

  it("正常路径：与 clockOf 自洽往返（前端这一套内部是闭环的）", () => {
    expect(clockOf(composeTimestamp("2026-09-08", "18:19"))).toBe("18:19");
  });

  it("形状：输出没有时区偏移，core 的 compose_timestamp 输出 RFC3339（带 +08:00）", () => {
    // 两侧写出的 createdAt 形状不同，但**碰不到一起**：GUI 的 diary.modify 只发 date/time，
    // createdAt 一律由 core 自己合成；无偏移的这种只出现在浏览器 dev 预览的 localStorage 里，
    // 永远不会落进 diary.json。clockOf 两种都读得懂（见上），所以这里只钉形状、不算 bug。
    expect(composeTimestamp("2026-09-08", "18:19")).not.toMatch(/[+-]\d{2}:\d{2}$/);
    expect(composeTimestamp("2026-09-08", "18:19")).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/);
  });
});

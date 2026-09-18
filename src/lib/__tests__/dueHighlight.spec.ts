import { describe, expect, it } from "vitest";

import {
  DEFAULT_DUE_COLORS,
  darkenHex,
  dueBucketOf,
  dueColorsOf,
  dueHighlightOf,
  dueMoment,
  mixHex
} from "../dueHighlight";

const NOW = new Date(2026, 8, 16, 10, 0, 0); // 2026-09-16（周三）10:00 本地时间

describe("dueBucketOf（已过期/今天/明天/后天）", () => {
  it("没有到期日 → none", () => {
    expect(dueBucketOf({}, NOW)).toBe("none");
    expect(dueBucketOf({ dueDate: "" }, NOW)).toBe("none");
  });

  it("当天算今天，明天/后天各归各档", () => {
    expect(dueBucketOf({ dueDate: "2026-09-16" }, NOW)).toBe("today");
    expect(dueBucketOf({ dueDate: "2026-09-17" }, NOW)).toBe("tomorrow");
    expect(dueBucketOf({ dueDate: "2026-09-18" }, NOW)).toBe("after");
    expect(dueBucketOf({ dueDate: "2026-09-19" }, NOW)).toBe("none");
  });

  it("已过的时刻算「已过期」（v0.8.3 第四档）；只到今天的日期要跨过午夜才变灰", () => {
    expect(dueBucketOf({ dueDate: "2026-09-16", dueTime: "09:00" }, NOW)).toBe("overdue");
    expect(dueBucketOf({ dueDate: "2026-09-15" }, NOW)).toBe("overdue");
    // 只有日期：按当天 23:59:59 算，10:00 的现在还没过
    expect(dueBucketOf({ dueDate: "2026-09-16" }, NOW)).toBe("today");
    // 恰好此刻：不算过期（<  判定）
    expect(dueBucketOf({ dueDate: "2026-09-16", dueTime: "10:00" }, NOW)).toBe("today");
  });

  it("dueMoment：有时刻用时刻，没时刻落在当天最后一刻", () => {
    expect(dueMoment({ dueDate: "2026-09-16", dueTime: "18:30" })).toBe(
      new Date(2026, 8, 16, 18, 30, 0, 0).getTime()
    );
    expect(dueMoment({ dueDate: "2026-09-16" })).toBe(
      new Date(2026, 8, 16, 23, 59, 59, 0).getTime()
    );
    expect(dueMoment({})).toBeNull();
  });
});

describe("dueHighlightOf（四档配色）", () => {
  it("默认四色：灰（过期）/ 红（今天）/ 黄（明天）/ 蓝（后天）", () => {
    expect(DEFAULT_DUE_COLORS).toEqual(["#808080", "#d93025", "#eab308", "#3b82f6"]);
  });

  it("关闭时一律不高亮", () => {
    expect(dueHighlightOf({ dueDate: "2026-09-16" }, "off", undefined, NOW)).toBeNull();
  });

  it("整色模式：过期/今天/明天/后天各取一色；更远不高亮", () => {
    expect(dueHighlightOf({ dueDate: "2026-09-15" }, "solid", undefined, NOW)?.color).toBe(
      DEFAULT_DUE_COLORS[0]
    );
    expect(dueHighlightOf({ dueDate: "2026-09-16" }, "solid", undefined, NOW)?.color).toBe(
      DEFAULT_DUE_COLORS[1]
    );
    expect(dueHighlightOf({ dueDate: "2026-09-17" }, "solid", undefined, NOW)?.color).toBe(
      DEFAULT_DUE_COLORS[2]
    );
    expect(dueHighlightOf({ dueDate: "2026-09-18" }, "solid", undefined, NOW)?.color).toBe(
      DEFAULT_DUE_COLORS[3]
    );
    expect(dueHighlightOf({ dueDate: "2026-09-20" }, "solid", undefined, NOW)).toBeNull();
  });

  it("已过期不加重、不插值：灰就是它的全部语义", () => {
    const overdue = dueHighlightOf({ dueDate: "2026-09-16", dueTime: "09:00" }, "solid", undefined, NOW);
    expect(overdue?.bucket).toBe("overdue");
    expect(overdue?.strong).toBe(false);
    expect(overdue?.color).toBe(DEFAULT_DUE_COLORS[0]);
    expect(
      dueHighlightOf({ dueDate: "2026-09-16", dueTime: "09:00" }, "gradient", undefined, NOW)?.color
    ).toBe(DEFAULT_DUE_COLORS[0]);
  });

  it("5 小时内的加重档：只有设了具体时刻才算", () => {
    const soon = dueHighlightOf({ dueDate: "2026-09-16", dueTime: "13:00" }, "solid", undefined, NOW);
    expect(soon?.strong).toBe(true);
    expect(soon?.color).toBe(darkenHex(DEFAULT_DUE_COLORS[1], 0.16));
    // 只有日期（落在 23:59:59）不按「几小时」算
    const dayOnly = dueHighlightOf({ dueDate: "2026-09-16" }, "solid", undefined, NOW);
    expect(dayOnly?.strong).toBe(false);
    expect(dayOnly?.color).toBe(DEFAULT_DUE_COLORS[1]);
  });

  it("渐变模式：取消 5 小时压深（插值自己就在表达远近）", () => {
    const soon = dueHighlightOf({ dueDate: "2026-09-16", dueTime: "13:00" }, "gradient", undefined, NOW);
    expect(soon?.strong).toBe(true);
    // 加重档只影响样式浓淡，不改颜色本身
    expect(soon?.color).toBe(mixHex(DEFAULT_DUE_COLORS[1], DEFAULT_DUE_COLORS[2], 13 / 24));
  });

  it("渐变模式：在今天色与明天色之间按实际时间插值", () => {
    // 明天 0 点 → 明天色
    expect(dueHighlightOf({ dueDate: "2026-09-17", dueTime: "00:00" }, "gradient", undefined, NOW)?.color)
      .toBe(DEFAULT_DUE_COLORS[2]);
    // 后天 0 点 → 后天色
    expect(dueHighlightOf({ dueDate: "2026-09-18", dueTime: "00:00" }, "gradient", undefined, NOW)?.color)
      .toBe(DEFAULT_DUE_COLORS[3]);
    // 今天 12:00（今天与明天两个锚点的正中）→ 两色的中值
    const middle = dueHighlightOf({ dueDate: "2026-09-16", dueTime: "12:00" }, "gradient", undefined, NOW);
    expect(middle?.color).toBe(mixHex(DEFAULT_DUE_COLORS[1], DEFAULT_DUE_COLORS[2], 0.5));
    // 明天 12:00 → 明天色与后天色的中值
    const middle2 = dueHighlightOf({ dueDate: "2026-09-17", dueTime: "12:00" }, "gradient", undefined, NOW);
    expect(middle2?.color).toBe(mixHex(DEFAULT_DUE_COLORS[2], DEFAULT_DUE_COLORS[3], 0.5));
  });

  it("渐变模式：只有日期没有时刻的不插值，直接按档位取色", () => {
    // 按 23:59:59 插值会把「今天到期」推到几乎纯明天色——没有时刻就没有可插值的时间
    expect(dueHighlightOf({ dueDate: "2026-09-16" }, "gradient", undefined, NOW)?.color)
      .toBe(DEFAULT_DUE_COLORS[1]);
    expect(dueHighlightOf({ dueDate: "2026-09-17" }, "gradient", undefined, NOW)?.color)
      .toBe(DEFAULT_DUE_COLORS[2]);
    expect(dueHighlightOf({ dueDate: "2026-09-18" }, "gradient", undefined, NOW)?.color)
      .toBe(DEFAULT_DUE_COLORS[3]);
  });

  it("自定义配色：用这一页自己的四色；非法项逐条回退默认", () => {
    const custom = ["#111111", "#222222", "#333333", "#444444"];
    expect(dueHighlightOf({ dueDate: "2026-09-17" }, "solid", custom, NOW)?.color).toBe("#333333");
    expect(dueColorsOf(["#112233", "oops"])).toEqual([
      "#112233",
      DEFAULT_DUE_COLORS[1],
      DEFAULT_DUE_COLORS[2],
      DEFAULT_DUE_COLORS[3]
    ]);
    expect(dueColorsOf(undefined)).toEqual(DEFAULT_DUE_COLORS);
  });
});

describe("颜色工具", () => {
  it("mixHex 端点与中点", () => {
    expect(mixHex("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mixHex("#000000", "#ffffff", 1)).toBe("#ffffff");
    expect(mixHex("#000000", "#ffffff", 0.5)).toBe("#808080");
  });

  it("darkenHex / 非法输入原样返回", () => {
    expect(darkenHex("#ffffff", 0.5)).toBe("#808080");
    expect(darkenHex("nope")).toBe("nope");
    expect(mixHex("nope", "#ffffff", 0.5)).toBe("nope");
  });
});

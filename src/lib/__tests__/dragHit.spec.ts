import { describe, expect, it } from "vitest";
import { contiguousZones, DRAG_BAND_PX, keepsPreviousDecision, positionInRow, schmitt, zoneAt, type DragZone } from "../dragHit";

describe("schmitt（施密特触发器）", () => {
  const band = 8;
  it("首次判定不加偏置（等价于普通比较）", () => {
    expect(schmitt(51, 50, band, null)).toBe(true);
    expect(schmitt(49, 50, band, null)).toBe(false);
  });
  it("已经在阈值之上：要退到 threshold - band/2 才翻下去", () => {
    expect(schmitt(47, 50, band, true)).toBe(true);
    expect(schmitt(45, 50, band, true)).toBe(false);
  });
  it("还在阈值之下：要越过 threshold + band/2 才翻上来", () => {
    expect(schmitt(53, 50, band, false)).toBe(false);
    expect(schmitt(55, 50, band, false)).toBe(true);
  });
});

describe("zoneAt（落点 zone + 迟滞带）", () => {
  const zones: DragZone[] = [
    { key: "a", top: 0, bottom: 44 },
    { key: "b", top: 44, bottom: 88 },
    { key: "c", top: 88, bottom: 132 }
  ];

  it("命中所在 zone；空白区返回 null", () => {
    expect(zoneAt(zones, 20, null, DRAG_BAND_PX)).toBe("a");
    expect(zoneAt(zones, 60, null, DRAG_BAND_PX)).toBe("b");
    expect(zoneAt(zones, 200, null, DRAG_BAND_PX)).toBe(null);
  });

  it("刚越过边界一点点不换（回带内不换）", () => {
    // 边界在 44：指针 46 只越了 2px，维持上一轮的 a
    expect(zoneAt(zones, 46, "a", DRAG_BAND_PX)).toBe("a");
    // 越过 band/2 = 4px 才换
    expect(zoneAt(zones, 49, "a", DRAG_BAND_PX)).toBe("b");
    // 反向：从 b 回到 a 同样要退够
    expect(zoneAt(zones, 42, "b", DRAG_BAND_PX)).toBe("b");
    expect(zoneAt(zones, 39, "b", DRAG_BAND_PX)).toBe("a");
  });

  it("快速拖动跨多格直接换（迟滞不挡大位移）", () => {
    expect(zoneAt(zones, 120, "a", DRAG_BAND_PX)).toBe("c");
  });

  it("上一轮的 zone 没了（悬停展开改了行数）→ 按新布局直接判", () => {
    const afterExpand: DragZone[] = [
      { key: "x", top: 0, bottom: 44 },
      { key: "y", top: 44, bottom: 88 }
    ];
    expect(zoneAt(afterExpand, 60, "a", DRAG_BAND_PX)).toBe("y");
  });
});

describe("contiguousZones（缝隙归最近的行）", () => {
  it("行与行之间的 2px 缝隙按中点分给两边", () => {
    const rows: DragZone[] = [
      { key: "a", top: 0, bottom: 44 },
      { key: "b", top: 46, bottom: 90 }
    ];
    const zones = contiguousZones(rows);
    expect(zones[0]).toEqual({ key: "a", top: Number.NEGATIVE_INFINITY, bottom: 45 });
    expect(zones[1]).toEqual({ key: "b", top: 45, bottom: Number.POSITIVE_INFINITY });
    // 缝里的 45 归 b（中点正好等于 45 → 落在 b 的 [45, ∞)）
    expect(zoneAt(zones, 45, null, DRAG_BAND_PX)).toBe("b");
    expect(zoneAt(zones, 44.9, null, DRAG_BAND_PX)).toBe("a");
    expect(zoneAt(zones, 44, null, DRAG_BAND_PX)).toBe("a");
    // 首尾之外不再返回 null——拖动时指针跑到列表上下边缘也该有落点
    expect(zoneAt(zones, -500, null, DRAG_BAND_PX)).toBe("a");
    expect(zoneAt(zones, 500, null, DRAG_BAND_PX)).toBe("b");
  });

  it("乱序输入也能连成一片", () => {
    const zones = contiguousZones([
      { key: "b", top: 46, bottom: 90 },
      { key: "a", top: 0, bottom: 44 }
    ]);
    expect(zones.map((zone) => zone.key)).toEqual(["a", "b"]);
  });
});

describe("positionInRow（行内 before/after/inside）", () => {
  const row: DragZone = { key: "a", top: 100, bottom: 144 };
  it("普通行按中点分两段，首次判定不含偏置", () => {
    expect(positionInRow(row, 110, null, DRAG_BAND_PX, false)).toBe("before");
    expect(positionInRow(row, 130, null, DRAG_BAND_PX, false)).toBe("after");
  });
  it("普通行：停在中点上抖动不翻（越过中点 N 像素才换）", () => {
    // 中点 122；从 before 出发，要到 126 才翻成 after
    expect(positionInRow(row, 124, "before", DRAG_BAND_PX, false)).toBe("before");
    expect(positionInRow(row, 127, "before", DRAG_BAND_PX, false)).toBe("after");
    // 从 after 回 before 要跌破 118
    expect(positionInRow(row, 120, "after", DRAG_BAND_PX, false)).toBe("after");
    expect(positionInRow(row, 117, "after", DRAG_BAND_PX, false)).toBe("before");
  });
  it("分类行三段，边界带迟滞", () => {
    // 25% = 111、75% = 133
    expect(positionInRow(row, 105, null, DRAG_BAND_PX, true)).toBe("before");
    expect(positionInRow(row, 120, null, DRAG_BAND_PX, true)).toBe("inside");
    expect(positionInRow(row, 140, null, DRAG_BAND_PX, true)).toBe("after");
    // 从 before 出发要到 115 才进 inside
    expect(positionInRow(row, 113, "before", DRAG_BAND_PX, true)).toBe("before");
    expect(positionInRow(row, 116, "before", DRAG_BAND_PX, true)).toBe("inside");
    // 从 after 退回要到 129
    expect(positionInRow(row, 131, "after", DRAG_BAND_PX, true)).toBe("after");
    expect(positionInRow(row, 128, "after", DRAG_BAND_PX, true)).toBe("inside");
  });
});

describe("keepsPreviousDecision（布局在动 vs 用户在动）", () => {
  const base = { pointerY: 497, lastPointerY: 495, rowTop: 482, lastRowTop: 468, band: DRAG_BAND_PX };

  it("行动了、指针几乎没动 → 保持现判（分组头让位上移导致的假翻转）", () => {
    expect(keepsPreviousDecision(base)).toBe(true);
  });

  it("指针自己动了就照常重算（在行内往下挪一点也该能改判）", () => {
    expect(keepsPreviousDecision({ ...base, pointerY: 502 })).toBe(false);
  });

  it("行没动就照常重算（普通情况不走这条捷径）", () => {
    expect(keepsPreviousDecision({ ...base, rowTop: 482, lastRowTop: 482 })).toBe(false);
  });

  it("慢慢挪也算位移（累计超过迟滞带的一半就重算，不会被卡死）", () => {
    const crept = { ...base, pointerY: 500, lastPointerY: 495 };
    expect(keepsPreviousDecision(crept)).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import { activeSlogans, nextRotation, sloganIconName, withNewItem, SLOGAN_ICON_DEFAULT } from "../slogan";
import { normalizeSlogan } from "../defaults";
import type { SloganState } from "../types";

function state(partial: Partial<SloganState> = {}): SloganState {
  return {
    items: [],
    activeIds: [],
    icon: "",
    position: "top",
    enabled: true,
    updatedAt: "",
    ...partial
  };
}

const items = [
  { id: "s1", text: "多喝水", createdAt: "2026-01-01T00:00:00Z" },
  { id: "s2", text: "久坐了起来活动", createdAt: "2026-01-02T00:00:00Z" },
  { id: "s3", text: "今日事今日毕", createdAt: "2026-01-03T00:00:00Z" }
];

describe("图标语义", () => {
  it("空 = 默认喇叭；none = 不加图标；其余原样", () => {
    expect(sloganIconName("")).toBe(SLOGAN_ICON_DEFAULT);
    expect(sloganIconName("none")).toBeNull();
    expect(sloganIconName("Flame")).toBe("Flame");
  });
});

describe("展示名单与轮播", () => {
  it("activeSlogans 按 activeIds 顺序取，失效 id 跳过", () => {
    const s = state({ items, activeIds: ["s3", "ghost", "s1"] });
    expect(activeSlogans(s).map((item) => item.id)).toEqual(["s3", "s1"]);
    expect(activeSlogans(state({ items }))).toEqual([]);
  });

  it("nextRotation 单条/空恒 0，多条循环", () => {
    expect(nextRotation(0, 0)).toBe(0);
    expect(nextRotation(0, 1)).toBe(0);
    expect(nextRotation(1, 3)).toBe(2);
    expect(nextRotation(2, 3)).toBe(0);
  });

  it("withNewItem trim 截断、空文本不加、上限 50", () => {
    const s = state({ items });
    expect(withNewItem(s, "s4", "  新标语  ", "now")).toHaveLength(4);
    expect(withNewItem(s, "s4", "   ", "now")).toBe(items);
    expect(withNewItem(s, "s4", "x".repeat(120), "now")[3].text).toHaveLength(80);
    const full = state({ items: Array.from({ length: 50 }, (_, i) => ({ id: `s${i}`, text: `t${i}`, createdAt: "" })) });
    expect(withNewItem(full, "extra", "溢出", "now")).toHaveLength(50);
  });
});

describe("normalizeSlogan 白名单", () => {
  it("脏形状回落默认、activeIds 收敛到存在的 id、position/enabled 兜底", () => {
    const s = normalizeSlogan({
      items: [...items, { id: "s1", text: "重复 id" }, { id: "", text: "无 id" }, { id: "s9", text: "   " }],
      activeIds: ["s2", "ghost", "s2", "s1"],
      icon: "Flame",
      position: "weird",
      enabled: false,
      updatedAt: "2026-01-01T00:00:00Z"
    });
    expect(s.items.map((item) => item.id)).toEqual(["s1", "s2", "s3"]);
    expect(s.activeIds).toEqual(["s2", "s1"]);
    expect(s.position).toBe("top");
    expect(s.enabled).toBe(false);
    expect(s.icon).toBe("Flame");
  });

  it("缺省 = 空名单 + top + enabled", () => {
    expect(normalizeSlogan(undefined)).toEqual(state());
    expect(normalizeSlogan({}).enabled).toBe(true);
    expect(normalizeSlogan({ position: "bottom" }).position).toBe("bottom");
  });

  it("快照往返保留全部字段", () => {
    const s = normalizeSlogan({ items, activeIds: ["s1", "s2"], icon: "none", position: "bottom", enabled: false, updatedAt: "x" });
    expect(normalizeSlogan(JSON.parse(JSON.stringify(s)))).toEqual(s);
  });
});
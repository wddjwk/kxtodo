import { describe, expect, it } from "vitest";
import {
  activeSlogans,
  isSloganEmoji,
  nextRotation,
  sloganIconKind,
  sloganIconName,
  sloganIntervalMs,
  sloganTextFromMarkdown,
  SLOGAN_POSITIONS,
  SLOGAN_POSITION_LABELS,
  withNewItem,
  SLOGAN_ICON_DEFAULT
} from "../slogan";
import { normalizeSlogan } from "../defaults";
import type { SloganState } from "../types";

function state(partial: Partial<SloganState> = {}): SloganState {
  return {
    items: [],
    activeIds: [],
    bounds: "field",
    defaultIcon: "",
    accent: "",
    textColor: "",
    opacity: 0.94,
    intervalSeconds: 8,
    position: "top-right",
    enabled: true,
    updatedAt: "",
    ...partial
  };
}

const items = [
  { id: "s1", text: "多喝水", icon: "Flame", createdAt: "2026-01-01T00:00:00Z" },
  { id: "s2", text: "久坐了起来活动", icon: "none", createdAt: "2026-01-02T00:00:00Z" },
  { id: "s3", text: "今日事今日毕", icon: "", createdAt: "2026-01-03T00:00:00Z" }
];

describe("图标语义（条目级，含 emoji）", () => {
  it("空 = 默认喇叭；none = 不加图标；其余原样", () => {
    expect(sloganIconName("")).toBe(SLOGAN_ICON_DEFAULT);
    expect(sloganIconName("none")).toBeNull();
    expect(sloganIconName("Flame")).toBe("Flame");
  });

  it("emoji 判定与三分类（与 core 同口径）", () => {
    expect(isSloganEmoji("🔥")).toBe(true);
    expect(isSloganEmoji("💧")).toBe(true);
    expect(isSloganEmoji("👍🏻")).toBe(true);
    expect(isSloganEmoji("Flame")).toBe(false);
    expect(isSloganEmoji("")).toBe(false);
    expect(isSloganEmoji("none")).toBe(false);
    expect(isSloganEmoji("a💧")).toBe(true);
    expect(isSloganEmoji("💧 x")).toBe(false); // 空白不算 emoji 图标
    expect(sloganIconKind("")).toBe("lucide");
    expect(sloganIconKind("Flame")).toBe("lucide");
    expect(sloganIconKind("none")).toBe("none");
    expect(sloganIconKind("🔥")).toBe("emoji");
  });

  it("sloganTextFromMarkdown 取第一行并剥标记", () => {
    expect(sloganTextFromMarkdown("## 多喝水\n第二行")).toBe("多喝水");
    expect(sloganTextFromMarkdown("- [ ] 久坐了起来活动")).toBe("久坐了起来活动");
    expect(sloganTextFromMarkdown("1. 每日复盘")).toBe("每日复盘");
    expect(sloganTextFromMarkdown("**加粗**的提醒")).toBe("加粗的提醒");
    expect(sloganTextFromMarkdown("   \n\n  ")).toBe("");
    expect(sloganTextFromMarkdown("x".repeat(200))).toHaveLength(80);
  });
});

describe("位置与轮播", () => {
  it("六个位置齐全且都有中文标签", () => {
    expect(SLOGAN_POSITIONS).toEqual([
      "top-left",
      "top-center",
      "top-right",
      "bottom-left",
      "bottom-center",
      "bottom-right"
    ]);
    for (const position of SLOGAN_POSITIONS) {
      expect(SLOGAN_POSITION_LABELS[position]).toBeTruthy();
    }
  });

  it("sloganIntervalMs 秒→毫秒并带区间兜底", () => {
    expect(sloganIntervalMs(5)).toBe(5000);
    expect(sloganIntervalMs(1)).toBe(2000);
    expect(sloganIntervalMs(500)).toBe(120000);
    expect(sloganIntervalMs(Number.NaN)).toBe(8000);
  });

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

  it("withNewItem trim 截断、空文本不加、上限 50、新条目用默认图标", () => {
    const s = state({ items, defaultIcon: "💧" });
    const next = withNewItem(s, "s4", "  新标语  ", "now");
    expect(next).toHaveLength(4);
    expect(next[3]).toMatchObject({ id: "s4", text: "新标语", icon: "💧" });
    expect(withNewItem(s, "s4", "   ", "now")).toBe(items);
    expect(withNewItem(s, "s4", "x".repeat(120), "now")[3].text).toHaveLength(80);
    const full = state({ items: Array.from({ length: 50 }, (_, i) => ({ id: `s${i}`, text: `t${i}`, icon: "", createdAt: "" })) });
    expect(withNewItem(full, "extra", "溢出", "now")).toHaveLength(50);
  });
});

describe("normalizeSlogan 白名单（v0.9.5 形状）", () => {
  it("逐字段收敛：位置/颜色/透明度/间隔/条目图标", () => {
    const s = normalizeSlogan({
      items: [
        ...items,
        { id: "s1", text: "重复 id" },
        { id: "", text: "无 id" },
        { id: "s9", text: "   " },
        { id: "s4", text: "带图标", icon: "Flame" }
      ],
      activeIds: ["s2", "ghost", "s2", "s1"],
      accent: "#ABCDEF",
      textColor: "red",
      opacity: 5,
      intervalSeconds: 0,
      position: "weird",
      bounds: "window",
      defaultIcon: "🔥",
      enabled: false,
      updatedAt: "2026-01-01T00:00:00Z"
    });
    expect(s.items.map((item) => item.id)).toEqual(["s1", "s2", "s3", "s4"]);
    expect(s.items[0].icon).toBe("Flame");
    expect(s.items[3].icon).toBe("Flame");
    expect(s.activeIds).toEqual(["s2", "s1"]);
    expect(s.accent).toBe("#abcdef");
    expect(s.textColor).toBe(""); // 非 hex 回落默认
    expect(s.opacity).toBe(1); // 夹到上限
    expect(s.intervalSeconds).toBe(2); // 夹到下限
    expect(s.position).toBe("top-right"); // 未知回落默认
    expect(s.bounds).toBe("field"); // 未知回落默认
    expect(s.defaultIcon).toBe("🔥");
    expect(s.enabled).toBe(false);
  });

  it("缺省 = 空名单 + top-right + 透明度 0.94 + 间隔 8", () => {
    expect(normalizeSlogan(undefined)).toEqual(state());
    expect(normalizeSlogan({}).enabled).toBe(true);
    expect(normalizeSlogan({ position: "bottom-left" }).position).toBe("bottom-left");
    expect(normalizeSlogan({ bounds: "app" }).bounds).toBe("app");
    expect(normalizeSlogan({ defaultIcon: "💧" }).defaultIcon).toBe("💧");
    expect(normalizeSlogan({ opacity: 0.1 }).opacity).toBe(0.3);
    expect(normalizeSlogan({ intervalSeconds: 999 }).intervalSeconds).toBe(120);
  });

  it("快照往返保留全部字段", () => {
    const s = normalizeSlogan({
      items,
      activeIds: ["s1", "s2"],
      bounds: "app",
      defaultIcon: "🔥",
      accent: "#1f6feb",
      textColor: "#222222",
      opacity: 0.7,
      intervalSeconds: 3,
      position: "bottom-center",
      enabled: false,
      updatedAt: "x"
    });
    expect(normalizeSlogan(JSON.parse(JSON.stringify(s)))).toEqual(s);
  });
});
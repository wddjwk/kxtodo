import { describe, expect, it } from "vitest";

import {
  accentWithPreview,
  backgroundWithPreview,
  dueColorsForCard,
  dueColorsWithPreview,
  type ColorPreview
} from "../colorPreview";

const PREVIEW: ColorPreview = {
  scope: "entry-a",
  accent: "#b64a30",
  background: "#dbe4e6",
  due: { 1: "#00a000", 3: "#123456" }
};

describe("取色预览的选择器", () => {
  it("作用域对得上才生效（在日记页取色不该染到工作区）", () => {
    expect(accentWithPreview(PREVIEW, "entry-a", "#2564cf")).toBe("#b64a30");
    expect(accentWithPreview(PREVIEW, "entry-b", "#2564cf")).toBe("#2564cf");
    expect(accentWithPreview(null, "entry-a", "#2564cf")).toBe("#2564cf");
  });

  it("没在预览的槽位用落盘值兜底", () => {
    expect(accentWithPreview({ scope: "entry-a" }, "entry-a", "#2564cf")).toBe("#2564cf");
  });

  it("背景色预览只换 color，图片与透明度不动", () => {
    const bg = { color: "#f4f1ea", image: "img:a.png", imageOpacity: 0.5 };
    expect(backgroundWithPreview(PREVIEW, "entry-a", bg)).toEqual({
      color: "#dbe4e6",
      image: "img:a.png",
      imageOpacity: 0.5
    });
    expect(backgroundWithPreview(PREVIEW, "other", bg)).toBe(bg);
  });

  it("临期配色：没配过用默认四色，预览按档替换", () => {
    const defaults = ["#808080", "#d93025", "#eab308", "#3b82f6"];
    expect(dueColorsWithPreview(PREVIEW, "entry-a", undefined, defaults)).toEqual([
      "#808080",
      "#00a000",
      "#eab308",
      "#123456"
    ]);
    expect(dueColorsWithPreview(PREVIEW, "other", undefined, defaults)).toEqual(defaults);
  });

  it("临期配色：落盘值与默认档数不一致时按默认走（不猜）", () => {
    const defaults = ["#808080", "#d93025", "#eab308", "#3b82f6"];
    expect(dueColorsWithPreview(null, "entry-a", ["#111111"], defaults)).toEqual(defaults);
    expect(dueColorsWithPreview(null, "entry-a", ["#111111", "#222222", "#333333", "#444444"], defaults)).toEqual([
      "#111111",
      "#222222",
      "#333333",
      "#444444"
    ]);
  });

  it("临期配色的越界下标被忽略（不改坏数组）", () => {
    const defaults = ["#808080", "#d93025", "#eab308", "#3b82f6"];
    const preview: ColorPreview = { scope: "entry-a", due: { 9: "#000000", "-1": "#ffffff" } };
    expect(dueColorsWithPreview(preview, "entry-a", undefined, defaults)).toEqual(defaults);
  });
});

const DEFAULT4 = ["#808080", "#d93025", "#eab308", "#3b82f6"];

describe("卡片临期配色（v0.8.7 需求 2.6：系统视图双回退）", () => {
  it("条目页：只认自己条目的配置与草稿（行为与从前一致）", () => {
    const stored = { "entry-a": ["#010101", "#020202", "#030303", "#040404"] };
    expect(dueColorsForCard(null, "entry-a", "entry-a", stored, DEFAULT4)).toEqual([
      "#010101",
      "#020202",
      "#030303",
      "#040404"
    ]);
    const preview: ColorPreview = { scope: "entry-a", due: { 1: "#00a000" } };
    expect(dueColorsForCard(preview, "entry-a", "entry-a", stored, DEFAULT4)[1]).toBe("#00a000");
    // 别的条目在预览：不是自己的作用域，不生效
    const other: ColorPreview = { scope: "entry-b", due: { 1: "#00ff00" } };
    expect(dueColorsForCard(other, "entry-a", "entry-a", stored, DEFAULT4)[1]).toBe("#020202");
  });

  it("系统视图：自己没配过 → 读视图键（保存后卡片就该变）", () => {
    const stored = { "my-day": ["#111111", "#222222", "#333333", "#444444"] };
    expect(dueColorsForCard(null, "entry-a", "my-day", stored, DEFAULT4)).toEqual([
      "#111111",
      "#222222",
      "#333333",
      "#444444"
    ]);
  });

  it("系统视图：视图草稿实时染卡片（编辑端写视图键、消费端跟着预览）", () => {
    const preview: ColorPreview = { scope: "my-day", due: { 1: "#00a000" } };
    expect(dueColorsForCard(preview, "entry-a", "my-day", undefined, DEFAULT4)).toEqual([
      "#808080",
      "#00a000",
      "#eab308",
      "#3b82f6"
    ]);
  });

  it("条目自己配过：视图配置与视图草稿都不参与（自己优先）", () => {
    const stored = {
      "entry-a": ["#010101", "#020202", "#030303", "#040404"],
      "my-day": ["#111111", "#222222", "#333333", "#444444"]
    };
    expect(dueColorsForCard(null, "entry-a", "my-day", stored, DEFAULT4)[0]).toBe("#010101");
    const preview: ColorPreview = { scope: "my-day", due: { 0: "#ff0000" } };
    expect(dueColorsForCard(preview, "entry-a", "my-day", stored, DEFAULT4)[0]).toBe("#010101");
  });

  it("普通条目页（viewId 不是系统视图）：视图键不参与回退", () => {
    const stored = { "entry-b": ["#111111", "#222222", "#333333", "#444444"] };
    expect(dueColorsForCard(null, "entry-a", "entry-b", stored, DEFAULT4)).toEqual(DEFAULT4);
  });
});

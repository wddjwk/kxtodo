import { describe, expect, it } from "vitest";

import {
  accentWithPreview,
  backgroundWithPreview,
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

/**
 * 取色盘的手输校验（v0.8.6 需求 11）：HEX / RGB 两路输入的合法性判定是纯函数，
 * 单测钉住「非法输入不写入、合法输入归一化」这条语义——实现里它们决定
 * 「写不写 color 属性」（写坏了色盘会跳到一个莫名其妙的位置）。
 */
import { describe, expect, it } from "vitest";
import { hexToRgbInput, normalizeHexInput, parseChannelInput, rgbToHexInput } from "../colorPickerPanel";

describe("normalizeHexInput", () => {
  it("认 3 位与 6 位，大小写与 # 都归一", () => {
    expect(normalizeHexInput("#aabbcc")).toBe("#aabbcc");
    expect(normalizeHexInput("AABBCC")).toBe("#aabbcc");
    expect(normalizeHexInput("#ABC")).toBe("#aabbcc");
    expect(normalizeHexInput("  #AbC  ")).toBe("#aabbcc");
  });

  it("非法输入返回 null（调用方据此给行内提示、不写入）", () => {
    expect(normalizeHexInput("")).toBeNull();
    expect(normalizeHexInput("#12")).toBeNull();
    expect(normalizeHexInput("#12345")).toBeNull();
    expect(normalizeHexInput("#gggggg")).toBeNull();
    expect(normalizeHexInput("rgb(1,2,3)")).toBeNull();
  });
});

describe("parseChannelInput", () => {
  it("0–255 的整数才认", () => {
    expect(parseChannelInput("0")).toBe(0);
    expect(parseChannelInput(" 255 ")).toBe(255);
    expect(parseChannelInput("128")).toBe(128);
  });

  it("越界、负数、小数、字母一律 null", () => {
    expect(parseChannelInput("256")).toBeNull();
    expect(parseChannelInput("-1")).toBeNull();
    expect(parseChannelInput("12.5")).toBeNull();
    expect(parseChannelInput("12a")).toBeNull();
    expect(parseChannelInput("")).toBeNull();
  });
});

describe("HEX ↔ RGB 双向联动", () => {
  it("互转一致", () => {
    expect(hexToRgbInput("#ff8800")).toEqual({ r: 255, g: 136, b: 0 });
    expect(rgbToHexInput(255, 136, 0)).toBe("#ff8800");
    expect(rgbToHexInput(0, 0, 0)).toBe("#000000");
    expect(rgbToHexInput(255, 255, 255)).toBe("#ffffff");
  });

  it("非法输入不产出颜色", () => {
    expect(hexToRgbInput("nope")).toBeNull();
    expect(rgbToHexInput(256, 0, 0)).toBeNull();
    expect(rgbToHexInput(1.5, 0, 0)).toBeNull();
  });
});

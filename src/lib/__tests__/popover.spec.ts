/**
 * 浮层定位的几何（v0.8.6 需求 4，纯逻辑有单测）。
 *
 * 屏幕四角都要不溢出，且"向下放不下就翻到锚点上方、下边缘与锚点对齐"。
 */
import { describe, expect, it } from "vitest";
import { placePopover, POPOVER_MARGIN_PX } from "../popover";

const view = { width: 1000, height: 800 };
const menu = { width: 232, height: 300 };

describe("placePopover", () => {
  it("上方空间充足：向下展开，左上对齐锚点", () => {
    const placed = placePopover({ x: 100, y: 100 }, menu, view);
    expect(placed).toEqual({ left: 100, top: 100, maxHeight: 0 });
  });

  it("右下角：翻到锚点上方，下边缘与锚点对齐且不溢出", () => {
    const placed = placePopover({ x: 980, y: 780 }, menu, view);
    expect(placed.top + menu.height).toBe(780);
    expect(placed.left + menu.width).toBe(view.width - POPOVER_MARGIN_PX);
    expect(placed.top).toBeGreaterThanOrEqual(POPOVER_MARGIN_PX);
    expect(placed.maxHeight).toBe(0);
  });

  it("左下角：向下展开且左缘钳制在边距内", () => {
    const placed = placePopover({ x: 2, y: 60 }, menu, view);
    expect(placed.left).toBe(POPOVER_MARGIN_PX);
    expect(placed.top).toBe(60);
    expect(placed.maxHeight).toBe(0);
  });

  it("下方空间不足以放下但还够最小高度、且不比上方差：原地限高、贴视口下边界（不溢出）", () => {
    // 高菜单（500）锚在 300：下方 492 放不下、但比上方（292）宽松 → 留在下方限高
    const tall = { width: 232, height: 500 };
    const placed = placePopover({ x: 100, y: 300 }, tall, view);
    expect(placed.top).toBe(300);
    expect(placed.maxHeight).toBe(800 - POPOVER_MARGIN_PX - 300);
    expect(placed.top + placed.maxHeight).toBeLessThanOrEqual(view.height - POPOVER_MARGIN_PX);
  });

  it("下方连最小高度都没有：翻到上方并下边缘对齐（旧实现会溢出视口底）", () => {
    // 锚点 790 → 下方 2px、上方 782px
    const placed = placePopover({ x: 100, y: 790 }, menu, view);
    expect(placed.top + menu.height).toBe(790);
    expect(placed.maxHeight).toBe(0);
    // 贴着视口底部的锚点也不再向下溢出（回归：max(120, spaceBelow) 的老写法会）
    const tight = placePopover({ x: 100, y: 796 }, menu, view);
    expect(tight.top + Math.min(menu.height, tight.maxHeight || menu.height)).toBeLessThanOrEqual(796);
    expect(tight.top).toBeGreaterThanOrEqual(POPOVER_MARGIN_PX);
  });

  it("浮层比视口还高：限高且上下都不越界", () => {
    const tall = { width: 232, height: 900 };
    const placed = placePopover({ x: 100, y: 790 }, tall, view);
    expect(placed.maxHeight).toBeGreaterThan(0);
    expect(placed.top).toBeGreaterThanOrEqual(POPOVER_MARGIN_PX);
    expect(placed.top + placed.maxHeight).toBeLessThanOrEqual(view.height - POPOVER_MARGIN_PX);
  });

  it("右对齐锚定（三点菜单/日期浮层）：右缘贴锚点", () => {
    const placed = placePopover({ x: 900, y: 100 }, menu, view, { xAlign: "right" });
    expect(placed.left + menu.width).toBe(900);
    const clamped = placePopover({ x: 100, y: 100 }, menu, view, { xAlign: "right" });
    expect(clamped.left).toBe(POPOVER_MARGIN_PX);
  });

  it("窄视口：宽度先收敛到可用宽度", () => {
    const narrow = { width: 240, height: 300 };
    const placed = placePopover({ x: 10, y: 10 }, narrow, { width: 200, height: 700 });
    expect(placed.left).toBe(POPOVER_MARGIN_PX);
    expect(placed.left + Math.min(narrow.width, 200 - POPOVER_MARGIN_PX * 2)).toBeLessThanOrEqual(200);
  });
});

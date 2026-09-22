/**
 * 浮层定位的几何（v0.8.7 需求 1 重写，纯逻辑有单测）。
 *
 * 终裁语义只有两条分支：
 * ① 下方放得下 → 开在下方、左上角顶点贴鼠标；
 * ② 下方放不下 → 整个翻到上方、下边缘贴锚点（镜像时右下角顶点贴鼠标）；
 * 唯一的滚动兜底是「上下都放不下」的 maxHeight 钳制——**没有「原地限高」这一档**。
 */
import { describe, expect, it } from "vitest";
import { placePopover, POPOVER_MARGIN_PX } from "../popover";

const view = { width: 1000, height: 800 };
const menu = { width: 232, height: 300 };

describe("placePopover", () => {
  it("下方放得下：开在下方，左上角顶点贴鼠标", () => {
    const placed = placePopover({ x: 100, y: 100 }, menu, view);
    expect(placed).toEqual({ left: 100, top: 100, maxHeight: 0 });
  });

  it("下方 319 放不下 420 的菜单：翻上、底边贴锚点、无 maxHeight", () => {
    // spaceBelow = 800 − 8 − 473 = 319 < 420 → 翻上；used = min(420, 465) = 420，top = 473 − 420 = 53
    const tall = { width: 232, height: 420 };
    const placed = placePopover({ x: 500, y: 473 }, tall, view);
    expect(placed.top).toBe(53);
    expect(placed.top + tall.height).toBe(473);
    expect(placed.maxHeight).toBe(0);
  });

  it("镜像（右键/长按菜单）：翻上后右下角顶点贴鼠标", () => {
    const tall = { width: 232, height: 420 };
    const placed = placePopover({ x: 500, y: 473 }, tall, view, { mirrorXOnFlip: true });
    expect(placed.left + tall.width).toBe(500);
    expect(placed.top + tall.height).toBe(473);
    // 下方放得下时不镜像——契约①的「左上角贴鼠标」优先
    const below = placePopover({ x: 500, y: 100 }, menu, view, { mirrorXOnFlip: true });
    expect(below.left).toBe(500);
  });

  it("下方不是「差一点」而是明显更宽松：仍然翻上（不再原地限高滚动）", () => {
    // anchor 300：下方 492 放不下 500、但比上方 292 宽松——老实现会在这里原地限高
    const tall = { width: 232, height: 500 };
    const placed = placePopover({ x: 100, y: 300 }, tall, view);
    expect(placed.top).toBe(POPOVER_MARGIN_PX);
    expect(placed.maxHeight).toBe(300 - POPOVER_MARGIN_PX);
    expect(placed.top + placed.maxHeight).toBe(300);
  });

  it("上下都放不下（浮层比视口还高）：maxHeight 钳制、上下不越界", () => {
    const tall = { width: 232, height: 900 };
    const placed = placePopover({ x: 100, y: 790 }, tall, view);
    expect(placed.maxHeight).toBeGreaterThan(0);
    expect(placed.top).toBeGreaterThanOrEqual(POPOVER_MARGIN_PX);
    expect(placed.top + placed.maxHeight).toBeLessThanOrEqual(view.height - POPOVER_MARGIN_PX);
    expect(placed.top + placed.maxHeight).toBe(790);
  });

  it("mirror 对 xAlign=\"right\" 结构性恒等", () => {
    const flip = { x: 900, y: 790 };
    const plain = placePopover(flip, menu, view, { xAlign: "right" });
    const mirrored = placePopover(flip, menu, view, { xAlign: "right", mirrorXOnFlip: true });
    expect(mirrored).toEqual(plain);
    const below = placePopover({ x: 900, y: 100 }, menu, view, { xAlign: "right" });
    const belowMirror = placePopover({ x: 900, y: 100 }, menu, view, { xAlign: "right", mirrorXOnFlip: true });
    expect(belowMirror).toEqual(below);
    expect(below.left + menu.width).toBe(900);
  });

  it("四边钳制：左下角向下展开左缘钳制、右下角翻上右缘钳制", () => {
    const lowLeft = placePopover({ x: 2, y: 60 }, menu, view);
    expect(lowLeft.left).toBe(POPOVER_MARGIN_PX);
    expect(lowLeft.top).toBe(60);
    const lowRight = placePopover({ x: 980, y: 780 }, menu, view);
    expect(lowRight.top + menu.height).toBe(780);
    expect(lowRight.left + menu.width).toBe(view.width - POPOVER_MARGIN_PX);
  });

  it("窄视口：宽度先收敛到可用宽度", () => {
    const narrow = { width: 240, height: 300 };
    const placed = placePopover({ x: 10, y: 10 }, narrow, { width: 200, height: 700 });
    const clampedWidth = 200 - POPOVER_MARGIN_PX * 2;
    expect(placed.left).toBe(POPOVER_MARGIN_PX);
    expect(placed.left + clampedWidth).toBeLessThanOrEqual(200 - POPOVER_MARGIN_PX);
  });
});

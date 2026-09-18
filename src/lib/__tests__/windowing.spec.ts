import { describe, expect, it } from "vitest";

import {
  anchorAt,
  indexAtOffset,
  initialCount,
  prefixSums,
  scrollTopForAnchor,
  windowRange
} from "../windowing";

const heights = [100, 50, 200, 80, 120];

describe("窗口化的纯逻辑（v0.8.4 需求 3）", () => {
  it("前缀和：长度比项数多一，末尾就是总高", () => {
    expect(prefixSums(heights)).toEqual([0, 100, 150, 350, 430, 550]);
    expect(prefixSums([])).toEqual([0]);
    expect(prefixSums([-5, 10])).toEqual([0, 0, 10]); // 负高度按 0 处理
  });

  it("偏移落点：边界归给后一项，越界钳在末尾", () => {
    const prefix = prefixSums(heights);
    expect(indexAtOffset(prefix, 0)).toBe(0);
    expect(indexAtOffset(prefix, 99)).toBe(0);
    expect(indexAtOffset(prefix, 100)).toBe(1); // 第 1 项的起点
    expect(indexAtOffset(prefix, 149)).toBe(1);
    expect(indexAtOffset(prefix, 150)).toBe(2);
    expect(indexAtOffset(prefix, 10_000)).toBe(4);
  });

  it("窗口：上下各留 overscan，占位高度补满剩余", () => {
    const prefix = prefixSums(heights);
    // 视口从 100 起、高 100 → 覆盖第 1..2 项；overscan 1 → 第 0..3 项
    const range = windowRange(prefix, 100, 100, 1);
    expect(range.start).toBe(0);
    expect(range.end).toBe(3);
    expect(range.offsetTop).toBe(0);
    expect(range.offsetBottom).toBe(120); // 总高 550 - 到第 3 项为止 430
  });

  it("窗口：首项与末项都在视口内时不会被裁掉", () => {
    const prefix = prefixSums(heights);
    const range = windowRange(prefix, 0, 550, 0);
    expect(range.start).toBe(0);
    expect(range.end).toBe(4);
    expect(range.offsetBottom).toBe(0);
  });

  it("窗口：空列表返回空区间（不越界）", () => {
    expect(windowRange(prefixSums([]), 0, 300, 5)).toEqual({ start: 0, end: 0, offsetTop: 0, offsetBottom: 0 });
  });

  it("窗口：maxCount 是硬上限", () => {
    const many = prefixSums(Array.from({ length: 1000 }, () => 20));
    const range = windowRange(many, 0, 20_000, 50, 100);
    expect(range.end - range.start + 1).toBe(100);
  });

  it("首屏至少 30 项，并按视口估一屏能装几项", () => {
    expect(initialCount(120, 900, 10)).toBe(40); // 30 与 ceil(900/120)=8 取大 + overscan
    expect(initialCount(20, 900, 10)).toBe(55); // 45 + 10
  });

  it("滚动锚点：按 key 还原（头部插入也不漂）", () => {
    const keys = ["a", "b", "c", "d", "e"];
    const prefix = prefixSums(heights);
    const anchor = anchorAt(prefix, 120, keys); // 落在第 1 项内部 20px
    expect(anchor).toEqual({ index: 1, offset: 20, key: "b" });

    // 头部插入一项（高 300）：同样的「第 b 项内部 20px」现在要滚到 300 + 100 + 20
    const grown = prefixSums([300, ...heights]);
    expect(scrollTopForAnchor(grown, ["new", ...keys], anchor)).toBe(420);
  });

  it("滚动锚点：锚点项被删掉时按原下标钳住", () => {
    const prefix = prefixSums(heights);
    const anchor = anchorAt(prefix, 120, ["a", "b", "c", "d", "e"]);
    const shrunk = prefixSums([100, 200, 80, 120]);
    expect(scrollTopForAnchor(shrunk, ["a", "c", "d", "e"], anchor)).toBe(120);
  });
});

import { describe, expect, it } from "vitest";
import {
  SWIPE_DELETE_MAX_PX,
  SWIPE_DELETE_THRESHOLD_PX,
  SWIPE_DAMPING,
  SWIPE_REVEAL_MAX_PX,
  SWIPE_REVEAL_THRESHOLD_PX,
  SWIPE_SLOP_PX,
  dampedSwipeOffset,
  swipeLock,
  swipeRevealRatio
} from "../swipeRow";

describe("swipeLock（横纵方向锁）", () => {
  it("位移没过 slop 时不下结论", () => {
    expect(swipeLock(0, 0)).toBe("none");
    expect(swipeLock(SWIPE_SLOP_PX - 1, 0)).toBe("none");
    expect(swipeLock(2, 3)).toBe("none");
  });

  it("横向明显占优 = horizontal", () => {
    expect(swipeLock(20, 4)).toBe("horizontal");
    expect(swipeLock(-20, 10)).toBe("horizontal");
    // 1.4 倍的门槛上：|dx| 恰好等于 |dy| × 1.4 仍算横向
    expect(swipeLock(14, 10)).toBe("horizontal");
  });

  it("纵向占优 = vertical（下拉同步与列表滚动靠它拿回手势）", () => {
    expect(swipeLock(4, 40)).toBe("vertical");
    expect(swipeLock(-6, 30)).toBe("vertical");
    expect(swipeLock(10, 12)).toBe("vertical");
  });

  it("两边都不占优的斜滑先不下结论（等下一帧位移再判）", () => {
    // 12/10 既不满足横向的 1.4 倍、纵向也没压过横向：这时任何一方都不接管，
    // 浏览器照常纵向滚动，手势继续在 tracking 里等一个明确的意图。
    expect(swipeLock(12, 10)).toBe("none");
    expect(swipeLock(10, 10)).toBe("none");
  });

  it("纯横向的斜滑不会被误判成纵向", () => {
    expect(swipeLock(60, 6)).toBe("horizontal");
  });
});

describe("dampedSwipeOffset（过阈值阻尼）", () => {
  it("阈值之前 1:1 跟手", () => {
    expect(dampedSwipeOffset(0, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)).toBe(0);
    expect(dampedSwipeOffset(30, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)).toBe(30);
    expect(dampedSwipeOffset(SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)).toBe(
      SWIPE_DELETE_THRESHOLD_PX
    );
  });

  it("过阈值后按阻尼衰减", () => {
    const raw = SWIPE_DELETE_THRESHOLD_PX + 40;
    const expected = SWIPE_DELETE_THRESHOLD_PX + 40 * SWIPE_DAMPING;
    expect(dampedSwipeOffset(raw, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)).toBeCloseTo(expected, 6);
  });

  it("封顶在 max，再用力也不多走", () => {
    expect(dampedSwipeOffset(4000, SWIPE_DELETE_THRESHOLD_PX, SWIPE_DELETE_MAX_PX)).toBe(SWIPE_DELETE_MAX_PX);
    expect(dampedSwipeOffset(4000, SWIPE_REVEAL_THRESHOLD_PX, SWIPE_REVEAL_MAX_PX)).toBe(SWIPE_REVEAL_MAX_PX);
  });

  it("入参取绝对值：左右滑共用一条曲线", () => {
    expect(dampedSwipeOffset(-50, SWIPE_REVEAL_THRESHOLD_PX, SWIPE_REVEAL_MAX_PX)).toBe(50);
  });

  it("max 比阈值大（否则阻尼段一进来就被夹死）", () => {
    expect(SWIPE_DELETE_MAX_PX).toBeGreaterThan(SWIPE_DELETE_THRESHOLD_PX);
    expect(SWIPE_REVEAL_MAX_PX).toBeGreaterThan(SWIPE_REVEAL_THRESHOLD_PX);
  });
});

describe("swipeRevealRatio（两块动作同时缩放的唯一驱动）", () => {
  it("0 位移 = 0，到位 = 1，超出仍是 1", () => {
    expect(swipeRevealRatio(0, 100)).toBe(0);
    expect(swipeRevealRatio(50, 100)).toBe(0.5);
    expect(swipeRevealRatio(100, 100)).toBe(1);
    expect(swipeRevealRatio(148, 100)).toBe(1);
  });

  it("负向位移同样算进度（左滑）", () => {
    expect(swipeRevealRatio(-76, SWIPE_REVEAL_THRESHOLD_PX)).toBeCloseTo(0.5, 6);
  });

  it("阈值为 0 时不给 NaN", () => {
    expect(swipeRevealRatio(10, 0)).toBe(0);
  });

  it("两个阈值都大于 slop（否则一次误触就到位）", () => {
    expect(SWIPE_DELETE_THRESHOLD_PX).toBeGreaterThan(SWIPE_SLOP_PX);
    expect(SWIPE_REVEAL_THRESHOLD_PX).toBeGreaterThan(SWIPE_SLOP_PX);
  });
});

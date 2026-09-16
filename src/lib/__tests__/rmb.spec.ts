import { describe, expect, it } from "vitest";

import { toChineseYuan } from "../rmb";

describe("toChineseYuan（人民币大写）", () => {
  it("基本档位", () => {
    expect(toChineseYuan("1")).toBe("壹元整");
    expect(toChineseYuan("10")).toBe("壹拾元整");
    expect(toChineseYuan("110")).toBe("壹佰壹拾元整");
    expect(toChineseYuan("1234.56")).toBe("壹仟贰佰叁拾肆元伍角陆分");
  });

  it("零的合并：连续多个零只补一个，末尾零不补", () => {
    expect(toChineseYuan("1001")).toBe("壹仟零壹元整");
    expect(toChineseYuan("1010")).toBe("壹仟零壹拾元整");
    expect(toChineseYuan("1000")).toBe("壹仟元整");
    expect(toChineseYuan("10000000")).toBe("壹仟万元整");
  });

  it("万 / 亿分级与跨节补零", () => {
    expect(toChineseYuan("10000")).toBe("壹万元整");
    expect(toChineseYuan("10001")).toBe("壹万零壹元整");
    expect(toChineseYuan("100000001")).toBe("壹亿零壹元整");
    expect(toChineseYuan("100000000")).toBe("壹亿元整");
    expect(toChineseYuan("1000000000")).toBe("壹拾亿元整");
    expect(toChineseYuan("10000000000")).toBe("壹佰亿元整");
    expect(toChineseYuan("100000000000")).toBe("壹仟亿元整");
  });

  it("角分：整 / 只有角 / 有分无角补零 / 只有分", () => {
    expect(toChineseYuan("0")).toBe("零元整");
    expect(toChineseYuan("0.5")).toBe("零元伍角");
    expect(toChineseYuan("1.05")).toBe("壹元零伍分");
    expect(toChineseYuan("0.05")).toBe("零元零伍分");
    expect(toChineseYuan("12.30")).toBe("壹拾贰元叁角");
  });

  it("第三位小数四舍五入（与 parseYuanToCents 同口径）", () => {
    expect(toChineseYuan("1.005")).toBe("壹元零壹分");
    expect(toChineseYuan("1.004")).toBe("壹元整");
  });

  it("负数与千分位逗号", () => {
    expect(toChineseYuan("-1")).toBe("负壹元整");
    expect(toChineseYuan("1,234.56")).toBe("壹仟贰佰叁拾肆元伍角陆分");
    // 负零按零处理（parseYuanToCents 里的 cents !== 0 就这条）
    expect(toChineseYuan("-0")).toBe("零元整");
  });

  it("解析不出来的一律回 null（由调用方提示，不猜）", () => {
    expect(toChineseYuan("")).toBeNull();
    expect(toChineseYuan("abc")).toBeNull();
    expect(toChineseYuan("1.2.3")).toBeNull();
    expect(toChineseYuan("1e5")).toBeNull();
  });

  it("超出一万亿回 null（没有对应的分级词）", () => {
    expect(toChineseYuan("999999999999")).toBe("玖仟玖佰玖拾玖亿玖仟玖佰玖拾玖万玖仟玖佰玖拾玖元整");
    expect(toChineseYuan("1000000000000")).toBeNull();
  });
});

import { describe, expect, it } from "vitest";

import { formatYuanNumber, fromChineseYuan, toChineseYuan } from "../rmb";

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

describe("fromChineseYuan（反向：中文金额 → 数字）", () => {
  it("大写全流程：与正向互为逆运算", () => {
    expect(fromChineseYuan("壹仟贰佰叁拾肆元伍角陆分")).toBe(1234.56);
    expect(fromChineseYuan("壹元整")).toBe(1);
    expect(fromChineseYuan("零元整")).toBe(0);
    expect(fromChineseYuan("壹仟零壹元整")).toBe(1001);
    expect(fromChineseYuan("壹万零壹元整")).toBe(10001);
    expect(fromChineseYuan("壹亿零壹元整")).toBe(100000001);
    expect(fromChineseYuan("壹元零伍分")).toBe(1.05);
    expect(fromChineseYuan("零元伍角")).toBe(0.5);
    expect(fromChineseYuan("负壹元整")).toBe(-1);
    expect(fromChineseYuan("玖仟玖佰玖拾玖亿玖仟玖佰玖拾玖万玖仟玖佰玖拾玖元整")).toBe(999999999999);
  });

  it("小写（一二三…）与繁体变体（貳參陸萬億圓）都认", () => {
    expect(fromChineseYuan("一千二百三十四元五角六分")).toBe(1234.56);
    expect(fromChineseYuan("一千二百三十四圆整")).toBe(1234);
    expect(fromChineseYuan("壹仟貳佰參拾肆元整")).toBe(1234);
    expect(fromChineseYuan("壹萬贰仟元整")).toBe(12000);
    expect(fromChineseYuan("壹億零壹元整")).toBe(100000001);
    expect(fromChineseYuan("两千元整")).toBe(2000);
    expect(fromChineseYuan("兩佰元整")).toBe(200);
  });

  it("口语与宽松写法：拾伍、没有元的零头、结尾「正」、空格逗号", () => {
    expect(fromChineseYuan("拾伍元整")).toBe(15);
    expect(fromChineseYuan("伍角")).toBe(0.5);
    expect(fromChineseYuan("叁角伍分")).toBe(0.35);
    expect(fromChineseYuan("壹佰元正")).toBe(100);
    expect(fromChineseYuan(" 壹仟 贰佰元整 ")).toBe(1200);
    expect(fromChineseYuan("壹仟,贰佰元整")).toBe(1200);
  });

  it("认不出来的一律回 null（宽进严出，不猜）", () => {
    expect(fromChineseYuan("")).toBeNull();
    expect(fromChineseYuan("abc")).toBeNull();
    expect(fromChineseYuan("壹仟元叁")).toBeNull(); // 零头没落单位
    expect(fromChineseYuan("元整")).toBeNull(); // 什么都没有
    expect(fromChineseYuan("兆元整")).toBeNull(); // 不支持的量级字
    expect(fromChineseYuan("壹万亿亿元整")).toBeNull(); // 超出一万亿
    expect(fromChineseYuan("角")).toBeNull();
  });

  it("formatYuanNumber：最多两位小数、尾零不啰嗦、负号", () => {
    expect(formatYuanNumber(1234)).toBe("1234");
    expect(formatYuanNumber(1234.5)).toBe("1234.5");
    expect(formatYuanNumber(1234.56)).toBe("1234.56");
    expect(formatYuanNumber(-1)).toBe("-1");
    expect(formatYuanNumber(0)).toBe("0");
  });
});

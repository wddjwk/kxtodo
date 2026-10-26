import { describe, expect, it } from "vitest";
import { autoTabName, charWidthUnits, TAB_NAME_WIDTH_BUDGET, truncateToWidth } from "../scratchpadName";

describe("charWidthUnits", () => {
  it("CJK / 假名 / 韩文 / 全角标点记 2 个单位", () => {
    for (const ch of ["汉", "字", "あ", "ア", "한", "，", "（", "Ａ"]) {
      expect(charWidthUnits(ch)).toBe(2);
    }
  });

  it("拉丁字母 / 数字 / 半角标点记 1 个单位", () => {
    for (const ch of ["a", "Z", "7", " ", ".", "("]) {
      expect(charWidthUnits(ch)).toBe(1);
    }
  });

  it("emoji 记 2 个单位", () => {
    expect(charWidthUnits("😀")).toBe(2);
  });
});

describe("truncateToWidth", () => {
  it("全角按 2 个单位截：预算 5 → 最多 2 个全角字", () => {
    expect(truncateToWidth("第二页的内容", TAB_NAME_WIDTH_BUDGET)).toBe("第二");
  });

  it("半角按 1 个单位截：预算 5 → 最多 5 个字母", () => {
    expect(truncateToWidth("shopping list", TAB_NAME_WIDTH_BUDGET)).toBe("shopp");
  });

  it("混排：全角 + 半角凑满预算，超出的那个全角放不下就不放", () => {
    // 会议x = 2+2+1 = 5 正好；再加一个全角 a 会到 7 > 5
    expect(truncateToWidth("会议x议程", TAB_NAME_WIDTH_BUDGET)).toBe("会议x");
  });

  it("短于预算原样返回，不加省略号", () => {
    expect(truncateToWidth("买菜", TAB_NAME_WIDTH_BUDGET)).toBe("买菜");
  });

  it("预算为 0 或首字符就放不下 → 空串", () => {
    expect(truncateToWidth("买", 0)).toBe("");
    expect(truncateToWidth("买", 1)).toBe("");
  });

  it("按码点截，不会把代理对劈成半个字符", () => {
    expect(truncateToWidth("😀x", 2)).toBe("😀");
    expect(truncateToWidth("😀x", 1)).toBe("");
  });
});

describe("autoTabName", () => {
  it("空正文 / 只有空白 → 空串（调用方回落数字名）", () => {
    expect(autoTabName("")).toBe("");
    expect(autoTabName("   \n\n  \t ")).toBe("");
  });

  it("取第一行有内容的文本，换行不进名字", () => {
    expect(autoTabName("购物清单\n牛奶\n鸡蛋")).toBe("购物");
  });

  it("第一行只有噪音时跳到下一行", () => {
    expect(autoTabName("```\nconst x = 1\n```")).toBe("const");
    expect(autoTabName("\n\n# \n买菜清单")).toBe("买菜");
  });

  it("剥掉标题井号 / 引用符 / 列表符 / 任务框", () => {
    expect(autoTabName("# 会议纪要")).toBe("会议");
    expect(autoTabName("> 引用的一段话")).toBe("引用");
    expect(autoTabName("- [ ] 交水电费")).toBe("交水");
    expect(autoTabName("* [x] 已办的事")).toBe("已办");
    expect(autoTabName("3. 有序列表项")).toBe("有序");
  });

  it("剥掉行内强调符与反引号", () => {
    expect(autoTabName("**重点**笔记")).toBe("重点");
    expect(autoTabName("`code` 开头")).toBe("code");
  });

  it("链接留文字、图片整个去掉", () => {
    expect(autoTabName("看 [这个链接](https://example.com) 的说明")).toBe("看 这");
    expect(autoTabName("![图](https://example.com/a.png)后面的字")).toBe("后面");
    // 整行只有一张图 → 这行没内容可取，落到下一行
    expect(autoTabName("![图](https://example.com/a.png)\n真正的标题")).toBe("真正");
  });

  it("行内多空白折叠成单个空格再截断", () => {
    expect(autoTabName("hello   wide   world")).toBe("hello");
    expect(autoTabName("买 菜 清 单")).toBe("买 菜");
  });

  it("拉丁正文按半角预算取 5 个字母", () => {
    expect(autoTabName("Meeting notes for Monday")).toBe("Meeti");
  });
});

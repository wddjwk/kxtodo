import { describe, expect, it } from "vitest";

import { preserveLeadingIndent } from "../markdownIndent";

const NBSP = "\u00A0";
/** 硬换行补在上一行行尾（两个空格），不是在本行前面插 `<br>` */
const HB = "  ";

describe("行首空白保留（需求 14）", () => {
  it("列表项后的缩进续行：上一行补硬换行，本行缩进成不间断空格", () => {
    expect(preserveLeadingIndent("1. xx\n\t1.1 xx")).toBe(`1. xx${HB}\n${NBSP.repeat(4)}1.1 xx`);
  });

  it("多层大纲逐层保留（tab 不限层数）", () => {
    expect(preserveLeadingIndent("1. 一\n\t1.1 一一\n\t\t1.1.1 一一一")).toBe(
      `1. 一${HB}\n${NBSP.repeat(4)}1.1 一一${HB}\n${NBSP.repeat(8)}1.1.1 一一一`
    );
  });

  it("空格缩进同样保留（1–3 空格）", () => {
    expect(preserveLeadingIndent("甲\n  乙")).toBe(`甲${HB}\n${NBSP.repeat(2)}乙`);
  });

  it("空行之后的缩进行是独立段落：不补硬换行", () => {
    expect(preserveLeadingIndent("甲\n\n  乙")).toBe(`甲\n\n${NBSP.repeat(2)}乙`);
  });

  it("标题后的缩进行是独立段落", () => {
    expect(preserveLeadingIndent("# 标题\n  乙")).toBe(`# 标题\n${NBSP.repeat(2)}乙`);
  });

  it("已经有硬换行的上一行不重复补", () => {
    expect(preserveLeadingIndent("甲\\\n  乙")).toBe(`甲\\\n${NBSP.repeat(2)}乙`);
    expect(preserveLeadingIndent("甲  \n  乙")).toBe(`甲  \n${NBSP.repeat(2)}乙`);
  });

  it("嵌套列表的缩进是结构，一个字都不动", () => {
    for (const src of [
      "- item\n  - sub\n    - subsub",
      "- item\n\t- sub\n\t\t- subsub",
      "1. 一\n   1) 二"
    ]) {
      expect(preserveLeadingIndent(src)).toBe(src);
    }
  });

  it("缩进的标题 / 引用 / 分隔线也不动（marked 本来就认）", () => {
    for (const src of ["甲\n  # 标题", "甲\n  > 引用", "甲\n\n  ---", "甲\n  ==="]) {
      expect(preserveLeadingIndent(src)).toBe(src);
    }
  });

  it("围栏代码块里一个字都不动", () => {
    const src = "```\n  code line\n\tmore\n```";
    expect(preserveLeadingIndent(src)).toBe(src);
  });

  it("未闭合的围栏之后也不动", () => {
    const src = "```\n  code line\n\tmore";
    expect(preserveLeadingIndent(src)).toBe(src);
  });

  it("缩进代码块（≥4 空格且无 tab）不动", () => {
    const src = "甲\n\n    code";
    expect(preserveLeadingIndent(src)).toBe(src);
  });

  it("空行与无缩进行原样（整篇没缩进时直接返回原字符串）", () => {
    const src = "甲\n\n乙\n丙";
    expect(preserveLeadingIndent(src)).toBe(src);
  });

  it("行数不变（任务勾选框与源码行的对应关系靠行数）", () => {
    const src = "- [ ] 甲\n\t子说明\n- [x] 乙\n\t\t更深的说明";
    expect(preserveLeadingIndent(src).split("\n")).toHaveLength(src.split("\n").length);
  });

  it("CRLF 的空行仍算空行（不会被换成「不间断空格 + \\r」把两段并成一段）", () => {
    const src = "甲\r\n\r\n  乙";
    expect(preserveLeadingIndent(src)).toBe(`甲\r\n\r\n${NBSP.repeat(2)}乙`);
  });
});

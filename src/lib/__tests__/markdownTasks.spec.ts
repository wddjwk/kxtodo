/**
 * 任务列表（`- [ ]` / `- [x]`）源码 ↔ 渲染结果对应的单元测试。
 *
 * 这里钉的是**索引对齐**：渲染出来的第 N 个勾选框必须正好是源码里第 N 个任务行，
 * 点错一个就是把别的任务勾掉——比不实现还糟。
 */
import { describe, expect, it } from "vitest";

import { markdownTaskLines, toggleMarkdownTask } from "../markdownTasks";

describe("markdownTaskLines（任务行定位）", () => {
  it("按顺序给出每个任务项的行号", () => {
    const md = ["# 标题", "- [ ] 甲", "正文", "- [x] 乙", "  - [ ] 嵌套"].join("\n");
    expect(markdownTaskLines(md)).toEqual([1, 3, 4]);
  });

  it("围栏代码块里的 - [ ] 不算（与 marked 同口径）", () => {
    const md = ["```", "- [ ] 代码里的", "```", "- [ ] 真的", "~~~", "- [ ] 波浪围栏里的", "~~~"].join("\n");
    expect(markdownTaskLines(md)).toEqual([3]);
  });

  it("紧跟文字的 [x] 不算（marked 的 listIsTask 要求后面有一个空格）", () => {
    const md = ["- [x]已办（没空格）", "- [x] 已办（有空格）"].join("\n");
    expect(markdownTaskLines(md)).toEqual([1]);
  });

  it("引用块里的任务列表算", () => {
    expect(markdownTaskLines("> - [ ] 引用里的")).toEqual([0]);
  });

  it("星号、加号与**有序**标记都算（marked 对有序列表同样产出任务框）", () => {
    const md = ["* [ ] 星号", "+ [x] 加号", "1. [ ] 有序", "2) [x] 括号有序", "10. [ ] 两位数"].join("\n");
    expect(markdownTaskLines(md)).toEqual([0, 1, 2, 3, 4]);
  });

  it("列表外的 ≥4 空格缩进是代码块，不算；列表里的深缩进是子列表，算", () => {
    // 与 marked 逐条对过：前者渲染成 <pre><code>（没有勾选框），后者是嵌套 <ul>（有）
    expect(markdownTaskLines("正文\n\n    - [ ] 代码块里的")).toEqual([]);
    expect(markdownTaskLines("\t- [ ] 一个 tab 也是代码块")).toEqual([]);
    expect(markdownTaskLines("   - [ ] 三空格还是列表")).toEqual([0]);
    expect(markdownTaskLines("- [ ] 甲\n    - [ ] 乙")).toEqual([0, 1]);
    expect(markdownTaskLines("1. [ ] 甲\n   1. [ ] 乙\n    - [ ] 丙")).toEqual([0, 1, 2]);
    // 松散列表：空一行再缩进的子项仍然算嵌套（返回的是**行号**，空行占 1）
    expect(markdownTaskLines("- [ ] 甲\n\n    - [ ] 乙")).toEqual([0, 2]);
    // 列表被顶格普通行收掉之后，深缩进又变回代码块
    expect(markdownTaskLines("- [ ] 甲\n\n收尾\n\n    - [ ] 乙")).toEqual([0]);
  });

  it("有序与无序混排时索引仍然对齐（点第 2 个框翻的就是第 2 行）", () => {
    const md = ["- [ ] 无序甲", "1. [ ] 有序乙", "- [x] 无序丙"].join("\n");
    expect(markdownTaskLines(md)).toEqual([0, 1, 2]);
    expect(toggleMarkdownTask(md, 1)).toBe(["- [ ] 无序甲", "1. [x] 有序乙", "- [x] 无序丙"].join("\n"));
  });
});

describe("toggleMarkdownTask（翻转勾选）", () => {
  it("打勾 / 取消打勾都只动目标那一行", () => {
    const md = ["- [ ] 甲", "- [x] 乙"].join("\n");
    expect(toggleMarkdownTask(md, 0)).toBe(["- [x] 甲", "- [x] 乙"].join("\n"));
    expect(toggleMarkdownTask(md, 1)).toBe(["- [ ] 甲", "- [ ] 乙"].join("\n"));
  });

  it("大小写 X 一律归一成小写 x / 空格", () => {
    expect(toggleMarkdownTask("- [X] 大写", 0)).toBe("- [ ] 大写");
  });

  it("索引越界原样返回（不抛、不改）", () => {
    const md = "- [ ] 只有一条";
    expect(toggleMarkdownTask(md, 3)).toBe(md);
  });

  it("行里还有别的方括号时不动它们", () => {
    expect(toggleMarkdownTask("- [ ] 看 [链接](https://a.b)", 0)).toBe("- [x] 看 [链接](https://a.b)");
  });

  it("缩进与引用前缀原样保留", () => {
    expect(toggleMarkdownTask("  - [ ] 缩进", 0)).toBe("  - [x] 缩进");
    expect(toggleMarkdownTask("> - [ ] 引用", 0)).toBe("> - [x] 引用");
  });
});

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

  it("星号与加号标记也算，有序列表不算（GFM 只有无序列表支持任务项）", () => {
    const md = ["* [ ] 星号", "+ [x] 加号", "1. [ ] 有序"].join("\n");
    expect(markdownTaskLines(md)).toEqual([0, 1]);
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

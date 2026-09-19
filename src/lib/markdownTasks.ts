/**
 * markdown 任务列表（`- [ ]` / `- [x]`）的源码 ↔ 渲染结果对应关系。
 *
 * 单独成一个模块（不 import marked / DOMPurify / highlight.js）：这三个函数是纯文本
 * 与纯 DOM 逻辑，独立出来才跑得进 node 环境的单元测试——`markdown.ts` 一 import 就要
 * 一个 window（DOMPurify 在模块级建实例）。
 */
import { indentWidthOf } from "./markdownIndent";

/** 列表项标记：无序 `- * +`，**有序 `1.` / `1)` 同样算**——marked 15 对有序列表也产出
 *  可点任务框（`1. [ ] xxx` 渲染出 `<ol><li><input type=checkbox>`）。
 *  v0.8.3 之前这里只认 `[-*+]`，于是文档里一出现有序任务项，渲染出的框与源码行就错位：
 *  纯有序任务点勾选是无声空操作却照样写一次盘，混排时点有序项会翻掉另一行的源码。 */
const LIST_ITEM = /^(?:[-*+]|\d{1,9}[.)])\s+/;
/** 任务项：标记 + `[ ]` + **一个空格**（`- [x]已办` marked 不当任务项，这里也不能算） */
const TASK_ITEM = /^(?:[-*+]|\d{1,9}[.)])\s+\[[ xX]\] /;
/** 缩进到这么深、又不在列表里，marked 就当缩进代码块渲染（没有勾选框） */
const CODE_INDENT = 4;

/**
 * 源码里每个任务项所在的**行号**，顺序与渲染出的勾选框一一对应。
 *
 * 围栏代码块（``` / ~~~）整段跳过；引用块里的列表（每层 `>` 前缀）算；
 * 缩进代码块不算——但「≥4 空格」要看上下文：列表**里面**的深缩进是子列表（有勾选框），
 * 列表**外面**的才是代码块，所以要一路带着 `listOpen` 走。
 */
export function markdownTaskLines(markdown: string): number[] {
  const lines = markdown.split("\n");
  const out: number[] = [];
  let fence: string | null = null;
  let listOpen = false;
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fenceMatch = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fenceMatch) {
      if (fence === null) fence = fenceMatch[1][0];
      else if (fenceMatch[1][0] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const body = line.replace(/^\s*(?:>\s*)*/, "");
    // 空行不改变列表状态：松散列表里空一行再缩进的子项仍然是子列表
    if (body.trim() === "") continue;
    const indent = indentWidthOf(line);
    if (indent >= CODE_INDENT && !listOpen) continue;
    const item = LIST_ITEM.test(body);
    // 顶格的普通行 = 列表结束（缩进的续行是 lazy continuation，列表还开着）
    if (!item && indent === 0) listOpen = false;
    if (item) listOpen = true;
    if (TASK_ITEM.test(body)) out.push(index);
  }
  return out;
}

/** 翻转第 `index` 个任务项的勾选状态（`index` 与渲染顺序一致）。 */
export function toggleMarkdownTask(markdown: string, index: number): string {
  const target = markdownTaskLines(markdown)[index];
  if (target === undefined) return markdown;
  const lines = markdown.split("\n");
  lines[target] = lines[target].replace(/\[([ xX])\]/, (_match, mark: string) =>
    mark === " " ? "[x]" : "[ ]"
  );
  return lines.join("\n");
}

/**
 * 第 `index` 个任务项在源码里是否已勾选（`- [x]`）。
 * 点击路径用它决定渲染态要收敛到哪一态——**以源码为准**而不是拿 DOM 的当前值取反：
 * 原生点击已经把 checked 翻过来了，再翻一次就翻回去了（v0.8.5 需求 33 踩过）。
 */
export function markdownTaskChecked(markdown: string, index: number): boolean {
  const target = markdownTaskLines(markdown)[index];
  if (target === undefined) return false;
  return /\s\[[xX]\]\s/.test(markdown.split("\n")[target]);
}

/**
 * 点击落在任务勾选框上就回它的序号（第几个），否则回 null。
 *
 * `scope` 必须是**这一份渲染结果**的容器：同一个页面上每张卡片各有一份 markdown-body，
 * 序号只在各自的容器里数才有意义。
 */
export function taskToggleIndex(event: Event, scope: Element | null): number | null {
  if (!scope) return null;
  const box = (event.target as HTMLElement | null)?.closest?.('input[type="checkbox"]');
  if (!box) return null;
  const boxes = [...scope.querySelectorAll('input[type="checkbox"]')];
  const index = boxes.indexOf(box as HTMLInputElement);
  return index >= 0 ? index : null;
}

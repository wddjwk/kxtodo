/**
 * markdown 任务列表（`- [ ]` / `- [x]`）的源码 ↔ 渲染结果对应关系。
 *
 * 单独成一个模块（不 import marked / DOMPurify / highlight.js）：这三个函数是纯文本
 * 与纯 DOM 逻辑，独立出来才跑得进 node 环境的单元测试——`markdown.ts` 一 import 就要
 * 一个 window（DOMPurify 在模块级建实例）。
 */

/**
 * 源码里每个任务项所在的**行号**，顺序与渲染出的勾选框一一对应。
 *
 * 判据照抄 marked 的 `listIsTask`（`/^\[[ xX]\] /`）：必须是无序列表标记 + `[ ]` +
 * **一个空格**——`- [x]已办`（紧跟文字）marked 不当任务项，这里也不能算，否则索引会错位。
 * 围栏代码块（``` / ~~~）整段跳过；引用块里的列表（每层 `>` 前缀）算。
 */
export function markdownTaskLines(markdown: string): number[] {
  const lines = markdown.split("\n");
  const out: number[] = [];
  let fence: string | null = null;
  for (let index = 0; index < lines.length; index++) {
    const fenceMatch = /^\s*(`{3,}|~{3,})/.exec(lines[index]);
    if (fenceMatch) {
      if (fence === null) fence = fenceMatch[1][0];
      else if (fenceMatch[1][0] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    if (/^[-*+]\s+\[[ xX]\] /.test(lines[index].replace(/^\s*(?:>\s*)*/, ""))) out.push(index);
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

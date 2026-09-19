// @vitest-environment happy-dom
/**
 * 渲染态勾选的**手术式更新**等价性（v0.8.5 需求 33）。
 *
 * 点击路径（`toggleRenderedTaskBox`：翻 checked + mark/unmark）必须与「改源后整篇重渲」
 * 得到同一份 DOM——否则豁免重渲之后界面就与源码分叉了。这里直接对比两边的 DOM 形态：
 * 左 = 渲染原文 → 手术翻转；右 = 渲染翻转后的源码。四种组合都过一遍
 * （勾上/勾掉 × 紧列表/松列表），外加嵌套子列表与「unmark 对未选项幂等」。
 */
import { describe, expect, it } from "vitest";
import { markCheckedItem, renderMarkdown, setRenderedTaskBox, unmarkCheckedItem } from "../markdown";
import { markdownTaskChecked, toggleMarkdownTask } from "../markdownTasks";

function mount(html: string): HTMLElement {
  const host = document.createElement("div");
  host.className = "markdown-body";
  host.innerHTML = html;
  document.body.appendChild(host);
  return host;
}

/** DOM 形态指纹：标签 + class + checked 属性 + 文本（空白节点忽略），用来判等价 */
function shape(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = (node.textContent ?? "").trim();
    return text ? `#${text}` : "";
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";
  const element = node as HTMLElement;
  const tag = element.tagName.toLowerCase();
  const cls = element.className ? `.${element.className.trim().split(/\s+/).join(".")}` : "";
  const checked = element instanceof HTMLInputElement && element.checked ? ":checked" : "";
  const kids = [...element.childNodes].map(shape).filter(Boolean).join(",");
  return `${tag}${cls}${checked}[${kids}]`;
}

function shapeOf(host: HTMLElement): string {
  return [...host.childNodes].map(shape).filter(Boolean).join("");
}

/** 手术路径：渲染原文 → 拨第 index 个渲染态勾选框（与卡片同一条路径） */
function surgical(markdown: string, index: number): string {
  const host = mount(renderMarkdown(markdown));
  const boxes = [...host.querySelectorAll<HTMLInputElement>("input.md-task-box")];
  const target = !markdownTaskChecked(markdown, index); // 目标态以源码为准
  boxes[index].checked = target; // 原生点击先把 checked 翻过去（卡片路径不再取反）
  setRenderedTaskBox(boxes[index], target);
  return shapeOf(host);
}

/** 对照路径：源码翻转后整篇重渲 */
function rerendered(markdown: string, index: number): string {
  return shapeOf(mount(renderMarkdown(toggleMarkdownTask(markdown, index))));
}

const TIGHT = ["- [ ] 甲", "- [x] 乙", "- [ ] 丙"].join("\n");
const LOOSE = ["- [ ] 甲", "", "- [x] 乙", "", "- [ ] 丙"].join("\n");
const NESTED = ["- [ ] 父", "    - [ ] 子一", "    - [x] 子二", "- [ ] 尾"].join("\n");

describe("勾选的手术式更新与整篇重渲等价", () => {
  it("紧列表：勾上 / 勾掉都一致", () => {
    expect(surgical(TIGHT, 0)).toBe(rerendered(TIGHT, 0)); // 勾上（含建 span.md-task-label）
    expect(surgical(TIGHT, 1)).toBe(rerendered(TIGHT, 1)); // 勾掉（含拆 span）
  });

  it("松列表：勾上 / 勾掉都一致（标在 <p> 上）", () => {
    expect(surgical(LOOSE, 0)).toBe(rerendered(LOOSE, 0));
    expect(surgical(LOOSE, 1)).toBe(rerendered(LOOSE, 1));
  });

  it("嵌套列表：父项与子项各自翻转都一致", () => {
    expect(surgical(NESTED, 0)).toBe(rerendered(NESTED, 0));
    expect(surgical(NESTED, 2)).toBe(rerendered(NESTED, 2));
  });

  it("unmark 对未勾选的项幂等（不动一个节点）", () => {
    const host = mount(renderMarkdown(TIGHT));
    const before = shapeOf(host);
    const li = host.querySelectorAll("li");
    unmarkCheckedItem(li[0]); // 未勾选
    unmarkCheckedItem(li[2]); // 未勾选
    expect(shapeOf(host)).toBe(before);
  });

  it("重复 mark 不二次搬节点（幂等）", () => {
    const host = mount(renderMarkdown(TIGHT));
    const box = host.querySelector<HTMLInputElement>("input.md-task-box")!;
    box.checked = true;
    setRenderedTaskBox(box, true);
    const once = shapeOf(host);
    // 渲染管线的打标再来一遍：已经打过的 li 不该再被搬一次
    markCheckedItem(host.querySelector("li")!);
    expect(shapeOf(host)).toBe(once);
  });
});

<script lang="ts">
  // 移动端编辑器的 markdown 快捷工具栏：贴在编辑器底部，输入法弹出时（adjustResize
  // 把视口压矮）自然停在输入法上方。按钮 pointerdown 一律 preventDefault——一抢焦点
  // 输入法就收下去了，「输入法上方的工具栏」就不成立。桌面端不渲染这个组件。
  import type { EditorView } from "@codemirror/view";
  import {
    Bold,
    Heading1,
    Heading2,
    Heading3,
    Highlighter,
    ImagePlus,
    Italic,
    Link2,
    List,
    ListOrdered,
    SquareCheckBig,
    Strikethrough,
    Underline
  } from "@lucide/svelte";
  import { insertLink, setHeading, toggleLinePrefix, wrapSelection } from "./codemirrorSetup";

  export let view: EditorView | null = null;
  export let onImage: () => void = () => {};

  function keepFocus(event: Event): void {
    event.preventDefault();
  }

  function run(action: (editor: EditorView) => void): void {
    if (view) action(view);
  }
</script>

<div class="editor-md-toolbar" role="toolbar" aria-label="Markdown 快捷输入">
  <button type="button" title="添加图片" on:pointerdown={keepFocus} on:click={() => onImage()}>
    <ImagePlus size={18} />
  </button>
  <button type="button" title="加粗" on:pointerdown={keepFocus} on:click={() => run((v) => wrapSelection(v, "**", "**", "加粗"))}>
    <Bold size={18} />
  </button>
  <button type="button" title="斜体" on:pointerdown={keepFocus} on:click={() => run((v) => wrapSelection(v, "*", "*", "斜体"))}>
    <Italic size={18} />
  </button>
  <button type="button" title="高亮" on:pointerdown={keepFocus} on:click={() => run((v) => wrapSelection(v, "==", "==", "高亮"))}>
    <Highlighter size={18} />
  </button>
  <button type="button" title="下划线" on:pointerdown={keepFocus} on:click={() => run((v) => wrapSelection(v, "<u>", "</u>", "下划线"))}>
    <Underline size={18} />
  </button>
  <button type="button" title="删除线" on:pointerdown={keepFocus} on:click={() => run((v) => wrapSelection(v, "~~", "~~", "删除线"))}>
    <Strikethrough size={18} />
  </button>
  <button type="button" title="超链接" on:pointerdown={keepFocus} on:click={() => run(insertLink)}>
    <Link2 size={18} />
  </button>
  <button type="button" title="一级标题" on:pointerdown={keepFocus} on:click={() => run((v) => setHeading(v, 1))}>
    <Heading1 size={18} />
  </button>
  <button type="button" title="二级标题" on:pointerdown={keepFocus} on:click={() => run((v) => setHeading(v, 2))}>
    <Heading2 size={18} />
  </button>
  <button type="button" title="三级标题" on:pointerdown={keepFocus} on:click={() => run((v) => setHeading(v, 3))}>
    <Heading3 size={18} />
  </button>
  <button type="button" title="待办项" on:pointerdown={keepFocus} on:click={() => run((v) => toggleLinePrefix(v, "- [ ] "))}>
    <SquareCheckBig size={18} />
  </button>
  <button type="button" title="无序列表" on:pointerdown={keepFocus} on:click={() => run((v) => toggleLinePrefix(v, "- "))}>
    <List size={18} />
  </button>
  <button type="button" title="有序列表" on:pointerdown={keepFocus} on:click={() => run((v) => toggleLinePrefix(v, "", true))}>
    <ListOrdered size={18} />
  </button>
</div>

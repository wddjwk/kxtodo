<script lang="ts">
  /**
   * 记账条目插图的全屏查看罩子：多张图左右翻页（键盘 ←→ 也行）。
   * editable（从记账编辑器里点开）时右上角多两个按钮：+ 继续添加、垃圾桶删除当前这张
   * ——三个按钮同一套圆形半透明风格。删除只作用于草稿/由调用方落盘，本组件不写数据。
   * 与 markdown 图全屏同一套语言（深色底、contain、移动端吃状态栏安全区），
   * 但独立成组件——记账的图片不在 markdown 画布里，markdownControls 那套接不到它。
   */
  import { onMount } from "svelte";
  import { ChevronLeft, ChevronRight, Plus, Trash2, X } from "@lucide/svelte";
  import { addBackInterceptor } from "../platform";
  import { suppressGhostClick } from "../ghostClick";

  export let items: { src: string; title: string }[] = [];
  export let index = 0;
  export let editable = false;
  export let onClose: () => void = () => {};
  export let onAdd: () => void = () => {};
  export let onDelete: (index: number) => void = () => {};

  let current = Math.min(Math.max(0, index), Math.max(0, items.length - 1));
  // 调用方删图后 items 变短：把游标收回来
  $: if (current >= items.length) current = Math.max(0, items.length - 1);
  $: item = items[current];

  function close(at?: { x: number; y: number }): void {
    if (at) suppressGhostClick(at);
    onClose();
  }

  function step(delta: number): void {
    if (items.length === 0) return;
    current = (current + delta + items.length) % items.length;
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    close({ x: event.clientX, y: event.clientY });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (event.key === "ArrowLeft") step(-1);
    else if (event.key === "ArrowRight") step(1);
  }

  onMount(() => {
    window.addEventListener("keydown", handleKeydown, true);
    return addBackInterceptor(() => {
      close();
      return true;
    });
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ledger-image-overlay" on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
  {#if item}
    <img class="ledger-image-stage" src={item.src} alt={item.title} />
  {/if}

  {#if items.length > 1}
    <button type="button" class="ledger-image-nav" title="上一张" aria-label="上一张" on:click={() => step(-1)}>
      <ChevronLeft size={22} />
    </button>
    <button type="button" class="ledger-image-nav next" title="下一张" aria-label="下一张" on:click={() => step(1)}>
      <ChevronRight size={22} />
    </button>
    <span class="ledger-image-counter">{current + 1} / {items.length}</span>
  {/if}

  <div class="ledger-image-actions">
    {#if editable}
      <button type="button" class="ledger-image-tool" title="添加图片" aria-label="添加图片" on:click={onAdd}>
        <Plus size={20} />
      </button>
      <button
        type="button"
        class="ledger-image-tool danger"
        title="删除这张图片"
        aria-label="删除这张图片"
        on:click={() => onDelete(current)}
      ><Trash2 size={18} /></button>
    {/if}
    <button type="button" class="ledger-image-tool" title="关闭" aria-label="关闭" on:click={() => close()}>
      <X size={20} />
    </button>
  </div>
</div>

<script lang="ts">
  /**
   * 记账条目插图的全屏查看罩子：一张图 + 右上角关闭。
   * 与 markdown 图全屏同一套语言（深色底、object-fit contain、移动端吃状态栏安全区），
   * 但独立成组件——记账的图片不在 markdown 画布里，markdownControls 那套接不到它。
   */
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import { addBackInterceptor } from "../platform";
  import { suppressGhostClick } from "../ghostClick";

  export let src: string;
  export let title = "";
  export let onClose: () => void = () => {};

  function close(at?: { x: number; y: number }): void {
    if (at) suppressGhostClick(at);
    onClose();
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    close({ x: event.clientX, y: event.clientY });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode !== 229) return;
    event.preventDefault();
    event.stopPropagation();
    close();
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
  <img class="ledger-image-stage" {src} alt={title} />
  <button
    type="button"
    class="ledger-image-close"
    title="关闭"
    aria-label="关闭"
    on:click={() => close()}
  ><X size={20} /></button>
</div>

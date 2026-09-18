<script lang="ts">
  /**
   * 取色草稿的「取消 / 保存」行（v0.8.4 需求 9）：主题色 / 背景色 / 临期配色三处共用。
   *
   * 语义只有一条：**保存才落盘**。取色（拖色盘、点预设色块）只改草稿并把活值推给界面
   * 预览，点保存才写 `setConfigAction`；取消 = 丢掉草稿、预览一起回退。
   * 菜单直接关掉也走取消（宿主在销毁时清草稿与预览）。
   */
  import { Check, Undo2 } from "@lucide/svelte";

  export let onSave: () => void;
  export let onCancel: () => void;
</script>

<div class="color-draft-actions">
  <button class="menu-action-button" type="button" on:click={onCancel}>
    <Undo2 size={13} /> 取消
  </button>
  <button class="menu-action-button primary" type="button" on:click={onSave}>
    <Check size={13} /> 保存
  </button>
</div>

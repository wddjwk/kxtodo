<script lang="ts">
  /**
   * 标语图标渲染（v0.9.5）：lucide 名 → 图标组件；emoji → 文本字形；none → 不渲染。
   * 悬浮层 / 工具页卡片 / 预览共用这一份，别各写一套分支。
   */
  import { ledgerIcon } from "./ledgerIcons";
  import { sloganIconKind, sloganIconName } from "./slogan";

  export let icon = "";
  export let size = 14;

  $: kind = sloganIconKind(icon);
  $: name = sloganIconName(icon);
</script>

{#if kind === "lucide"}
  <svelte:component this={ledgerIcon(name ?? "Megaphone", "Megaphone")} {size} />
{:else if kind === "emoji"}
  <span class="slogan-emoji" aria-hidden="true">{icon}</span>
{/if}
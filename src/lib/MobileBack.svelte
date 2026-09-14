<script lang="ts">
  /**
   * 移动端页面左上角的返回箭头（v0.7.6）：特性开关 features.mobileBack 控制，
   * 默认关——安卓用户习惯用系统返回键，多一个箭头会挤掉标题的位置。
   * 点击 = 与系统返回键同一条历史栈（goBackLevel），不是"直接回列表"。
   */
  import { ChevronLeft } from "@lucide/svelte";
  import { appSettings } from "./stores";
  import { isMobile, goBackLevel } from "./platform";
</script>

{#if $isMobile && $appSettings.features.mobileBack}
  <button class="mobile-back" type="button" title="返回" aria-label="返回" on:click|stopPropagation={goBackLevel}>
    <ChevronLeft size={26} />
  </button>
{/if}

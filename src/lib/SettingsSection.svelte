<script lang="ts">
  /**
   * 设置抽屉的一个可折叠分区：标题行左边是折叠按钮（chevron 折起时转 -90°，走全局
   * svg.collapsed），右边留给分区自己的动作（数据同步的历史图标就是这么挂进来的——
   * 折叠按钮是 <button>，动作按钮不能再嵌在它里面）。
   * 折叠状态记在 localStorage：设置项越来越多，改一个开关不该一直翻长列表。
   * 本机 UI 状态，不进同步。
   */
  import { ChevronDown } from "@lucide/svelte";

  export let title: string;
  /** localStorage 键的后半段（每个分区一个稳定标识，别用标题——标题会改） */
  export let storageKey: string;

  const STORAGE_PREFIX = "kxtodo-settings-section";

  function readCollapsed(): boolean {
    try {
      return window.localStorage.getItem(`${STORAGE_PREFIX}:${storageKey}`) === "1";
    } catch {
      return false;
    }
  }

  let collapsed = readCollapsed();

  function toggle(): void {
    collapsed = !collapsed;
    try {
      window.localStorage.setItem(`${STORAGE_PREFIX}:${storageKey}`, collapsed ? "1" : "0");
    } catch {
      // 隐私模式写不进去：这次会话内照常折叠，只是不记下来
    }
  }
</script>

<section class="settings-section" class:folded={collapsed}>
  <div class="settings-section-head">
    <button type="button" class="settings-section-toggle" aria-expanded={!collapsed} on:click={toggle}>
      <ChevronDown class={collapsed ? "collapsed" : ""} size={16} />
      <h3>{title}</h3>
    </button>
    <span class="settings-section-actions">
      <slot name="actions" />
    </span>
  </div>
  {#if !collapsed}
    <div class="settings-section-body">
      <slot />
    </div>
  {/if}
</section>

<script lang="ts">
  /**
   * 工具箱（移动端预留能力位）：注册表驱动的全页工具集合。
   * 新工具 = 往 TOOLS 注册表加一项 + 在 {#if activeTool.id === ...} 分支加子视图；
   * 子视图是纯组件内部状态（activeToolId），不占历史栈层级。
   */
  import { ArrowLeft, ChevronLeft, Dice5, Toolbox } from "@lucide/svelte";
  import { showMobileList } from "./platform";
  import NumberField from "./NumberField.svelte";
  import type { Component } from "svelte";

  type ToolDef = { id: string; name: string; desc: string; icon: Component };

  const TOOLS: ToolDef[] = [
    { id: "random", name: "随机数生成", desc: "在指定范围内生成随机整数", icon: Dice5 }
  ];

  let activeToolId: string | null = null;
  $: activeTool = TOOLS.find((tool) => tool.id === activeToolId) ?? null;

  // ---- 随机数生成（纯组件状态，无持久化） ----
  const RANDOM_LIMIT = 1_000_000;
  const RANDOM_MAX_COUNT = 200;
  let randomMin = 1;
  let randomMax = 100;
  let randomCount = 1;
  let randomResults: number[] = [];

  function generateRandom(): void {
    let lo = Math.trunc(randomMin);
    let hi = Math.trunc(randomMax);
    if (lo > hi) {
      // min > max：静默交换并回写输入框
      const swap = lo;
      lo = hi;
      hi = swap;
      randomMin = lo;
      randomMax = hi;
    }
    const count = Math.min(RANDOM_MAX_COUNT, Math.max(1, Math.trunc(randomCount) || 1));
    randomCount = count;
    const span = hi - lo + 1;
    randomResults = Array.from({ length: count }, () => lo + Math.floor(Math.random() * span));
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="toolbox-view" on:click|stopPropagation>
  <header class="toolbox-header">
    <button class="mobile-back" type="button" aria-label="返回列表" on:click={showMobileList}>
      <ArrowLeft size={26} />
    </button>
    <span class="toolbox-header-icon"><Toolbox size={26} /></span>
    <strong class="toolbox-header-title">工具箱</strong>
  </header>

  {#if !activeTool}
    <div class="toolbox-list">
      {#each TOOLS as tool (tool.id)}
        <button class="toolbox-card" type="button" on:click={() => (activeToolId = tool.id)}>
          <span class="toolbox-card-icon"><svelte:component this={tool.icon} size={22} /></span>
          <span class="toolbox-card-text">
            <strong>{tool.name}</strong>
            <span>{tool.desc}</span>
          </span>
        </button>
      {/each}
    </div>
  {:else if activeTool.id === "random"}
    <div class="toolbox-sub">
      <button class="toolbox-sub-back" type="button" on:click={() => (activeToolId = null)}>
        <ChevronLeft size={18} /> 返回工具箱
      </button>
      <div class="toolbox-sub-title">
        <Dice5 size={18} /> 随机数生成
      </div>
      <div class="toolbox-field-row">
        <span>最小值</span>
        <NumberField
          ariaLabel="最小值"
          min={-RANDOM_LIMIT}
          max={RANDOM_LIMIT}
          value={randomMin}
          onCommit={(v) => (randomMin = v)}
        />
      </div>
      <div class="toolbox-field-row">
        <span>最大值</span>
        <NumberField
          ariaLabel="最大值"
          min={-RANDOM_LIMIT}
          max={RANDOM_LIMIT}
          value={randomMax}
          onCommit={(v) => (randomMax = v)}
        />
      </div>
      <div class="toolbox-field-row">
        <span>数量</span>
        <NumberField
          ariaLabel="数量"
          min={1}
          max={RANDOM_MAX_COUNT}
          value={randomCount}
          onCommit={(v) => (randomCount = v)}
        />
      </div>
      <button class="settings-button primary" type="button" on:click={generateRandom}>生成</button>
      {#if randomResults.length}
        <div class="toolbox-results">
          {#each randomResults as value, index (index)}
            <span class="toolbox-result-chip">{value}</span>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</section>

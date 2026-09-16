<script lang="ts">
  /**
   * 随机数生成（工具注册表的第一件工具，v0.7.5 从 ToolboxView 的内联分支抽出来）：
   * 纯前端小工具，两端共用同一份实现。子视图不含返回按钮——那是壳（ToolboxView）的事。
   */
  import { Dice5 } from "@lucide/svelte";
  import NumberField from "../NumberField.svelte";

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

<div class="toolbox-sub">
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
  {#if randomResults.length}
    <div class="toolbox-results">
      {#each randomResults as value, index (index)}
        <span class="toolbox-result-chip">{value}</span>
      {/each}
    </div>
  {/if}
  <div class="toolbox-sub-actions">
    <button class="settings-button primary" type="button" on:click={generateRandom}>生成</button>
  </div>
</div>

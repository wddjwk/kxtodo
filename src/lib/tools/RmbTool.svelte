<script lang="ts">
  /**
   * 人民币金额大写（工具注册表的第二件工具）：纯前端小工具，两端共用同一份实现。
   * 换算逻辑在 `src/lib/rmb.ts`（单独成模块是为了能跑单元测试），这里只管输入输出。
   * 子视图不含返回按钮——那是壳（ToolboxView）的事。
   */
  import { Banknote, Copy } from "@lucide/svelte";
  import { toChineseYuan } from "../rmb";
  import { copyText } from "../clipboard";

  let amount = "";
  let copied = false;

  $: result = amount.trim() ? toChineseYuan(amount) : null;
  $: invalid = amount.trim().length > 0 && result === null;

  async function copyResult(): Promise<void> {
    if (!result) return;
    const ok = await copyText(result);
    copied = ok;
    if (ok) window.setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="toolbox-sub">
  <div class="toolbox-sub-title">
    <Banknote size={18} /> 人民币金额大写
  </div>
  <div class="toolbox-field-row">
    <span>金额（元）</span>
    <input
      class="toolbox-text-input"
      type="text"
      inputmode="decimal"
      placeholder="例如 1234.56"
      bind:value={amount}
    />
  </div>
  {#if invalid}
    <p class="toolbox-empty">认不出这个金额，请输入数字（可带小数点与负号，最多两位小数）。</p>
  {:else if result}
    <div class="toolbox-field-row">
      <span>大写</span>
      <span class="toolbox-rmb-result">{result}</span>
    </div>
    <div class="toolbox-sub-actions">
      <button class="settings-button primary" type="button" on:click={() => void copyResult()}>
        <Copy size={15} /> {copied ? "已复制" : "复制"}
      </button>
    </div>
  {/if}
</div>

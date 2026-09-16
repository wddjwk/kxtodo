<script lang="ts">
  /**
   * 人民币金额大小写（工具注册表）：纯前端小工具，两端共用同一份实现。
   * **双向**：输入数字转大写，输入中文金额（大写/小写/繁体都认）转回数字——
   * 按首个字符自动判向，不用切模式。换算逻辑在 `src/lib/rmb.ts`（单独成模块
   * 是为了能跑单元测试），这里只管输入输出。
   * 子视图不含返回按钮——那是壳（ToolboxView）的事。
   */
  import { Banknote, Copy } from "@lucide/svelte";
  import { formatYuanNumber, fromChineseYuan, toChineseYuan } from "../rmb";
  import { copyText } from "../clipboard";

  let amount = "";
  let copied = false;

  $: trimmed = amount.trim();
  /** 数字/符号开头 = 正向（金额 → 大写）；其余按中文金额反向识别 */
  $: reverse = trimmed.length > 0 && !/^[-+.\d]/.test(trimmed);
  $: converted = !trimmed
    ? null
    : reverse
      ? (() => {
          const value = fromChineseYuan(trimmed);
          return value === null ? null : formatYuanNumber(value);
        })()
      : toChineseYuan(trimmed);
  $: invalid = trimmed.length > 0 && converted === null;

  async function copyResult(): Promise<void> {
    if (!converted) return;
    const ok = await copyText(converted);
    copied = ok;
    if (ok) window.setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="toolbox-sub">
  <div class="toolbox-sub-title">
    <Banknote size={18} /> 人民币金额大小写
  </div>
  <div class="toolbox-field-row">
    <span>{reverse ? "中文金额" : "金额（元）"}</span>
    <input
      class="toolbox-text-input"
      type="text"
      inputmode="text"
      placeholder="1234.56 或 壹仟贰佰叁拾肆元伍角陆分"
      bind:value={amount}
    />
  </div>
  {#if invalid}
    <p class="toolbox-empty">
      {reverse
        ? "认不出这个中文金额：支持大写（壹贰叁）、小写（一二三）与繁体（貳參陸），可带 元/角/分/整 与「负」。"
        : "认不出这个金额，请输入数字（可带小数点与负号，最多两位小数）。"}
    </p>
  {:else if converted}
    <div class="toolbox-field-row">
      <span>{reverse ? "金额（元）" : "大写"}</span>
      <span class="toolbox-rmb-result">{converted}</span>
    </div>
    <div class="toolbox-sub-actions">
      <button class="settings-button primary" type="button" on:click={() => void copyResult()}>
        <Copy size={15} /> {copied ? "已复制" : "复制"}
      </button>
    </div>
  {/if}
</div>

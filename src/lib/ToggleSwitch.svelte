<script lang="ts">
  /**
   * 主题化开关（v0.9.4）：替代裸 checkbox 的拨动开关，按钮实现（role=switch），
   * 悬浮是柔和的光晕不是矩形高亮。标语栏与定时任务共用。
   */
  export let checked = false;
  export let label = "";
  export let disabled = false;
  export let onChange: (value: boolean) => void = () => {};
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label || undefined}
  class="toggle-switch"
  class:on={checked}
  {disabled}
  on:click={() => onChange(!checked)}
>
  <span class="toggle-switch-knob"></span>
</button>
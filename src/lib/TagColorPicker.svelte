<script lang="ts">
  /**
   * 标签配色选择：**两排各五枚胶囊**（九个具名色 + 末位色盘），flex 均分、
   * 任何宽度下两排都刚好左右填满。色盘那枚平时是炫彩渐变，点它出系统调色板；
   * 选过色之后胶囊就显示用户选的那个颜色（所见即所得）。
   */
  import { createEventDispatcher } from "svelte";
  import { TAG_COLOR_SPECS } from "./tagColors";
  import { openColorPicker } from "./colorPickerPanel";
  import type { TagColor } from "./types";

  /** 当前选中的色（`custom` 时用 `hex`） */
  export let color: TagColor = "yellow";
  export let hex = "";

  const dispatch = createEventDispatcher<{ change: { color: TagColor; hex: string } }>();

  function pick(next: TagColor, nextHex = ""): void {
    dispatch("change", { color: next, hex: nextHex });
  }
</script>

<div class="tag-color-grid">
  <div class="tag-color-row">
    {#each TAG_COLOR_SPECS.slice(0, 5) as spec (spec.color)}
      <button
        class="tag-color-pill"
        class:selected={color === spec.color}
        style={`--pill: ${spec.swatch}`}
        type="button"
        title={spec.label}
        aria-label={spec.label}
        on:click={() => pick(spec.color)}
      ></button>
    {/each}
  </div>
  <div class="tag-color-row">
    {#each TAG_COLOR_SPECS.slice(5) as spec (spec.color)}
      <button
        class="tag-color-pill"
        class:selected={color === spec.color}
        style={`--pill: ${spec.swatch}`}
        type="button"
        title={spec.label}
        aria-label={spec.label}
        on:click={() => pick(spec.color)}
      ></button>
    {/each}
    <button
      class="tag-color-pill tag-color-custom"
      class:selected={color === "custom"}
      style={hex ? `--pill: ${hex}` : ""}
      type="button"
      title="自定义颜色"
      aria-label="自定义颜色"
      data-color-anchor
      on:click={(event) =>
        openColorPicker({
          key: "tag:custom-color",
          color: hex || "#8430ce",
          anchor: event.currentTarget,
          onPreview: (color) => pick("custom", color),
          onConfirm: (color) => pick("custom", color),
          onCancel: () => undefined
        })}
    ></button>
  </div>
</div>

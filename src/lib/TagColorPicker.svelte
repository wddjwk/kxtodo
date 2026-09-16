<script lang="ts">
  /**
   * 标签配色选择：**九宫格两排**——第一排七彩虹里的五个，第二排三个加一个色盘。
   * 胶囊形状、两排各自左右填满（flex 均分），所以任何宽度下都排得整整齐齐。
   * 「色盘」那一格是个 `<input type="color">` 盖在胶囊上，点它出系统调色板。
   */
  import { createEventDispatcher } from "svelte";
  import { TAG_COLOR_SPECS } from "./tagColors";
  import type { TagColor } from "./types";

  /** 当前选中的色（`custom` 时用 `hex`） */
  export let color: TagColor = "yellow";
  export let hex = "";

  const dispatch = createEventDispatcher<{ change: { color: TagColor; hex: string } }>();

  const DEFAULT_CUSTOM = "#8430ce";

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
    <label
      class="tag-color-pill tag-color-custom"
      class:selected={color === "custom"}
      style={`--pill: ${hex || DEFAULT_CUSTOM}`}
      title="自定义颜色"
    >
      <input
        type="color"
        value={hex || DEFAULT_CUSTOM}
        aria-label="自定义颜色"
        on:input={(event) => pick("custom", event.currentTarget.value)}
      />
    </label>
  </div>
</div>

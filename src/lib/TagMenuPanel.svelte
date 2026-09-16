<script lang="ts">
  /**
   * 右键菜单「标签」面板（条目与日记条目共用一套）。
   *
   * 自上而下三块（**都没有标题文字**——面板本来就窄，「预置标签」「颜色」两行小字
   * 只会把它撑得更碎）：
   * ① **预置标签**——胶囊按内容自动排版；点一下加到当前条目上，右侧删除叉删掉这条
   *    预置；末尾的「+」也是一枚同款胶囊，跟着一起排（把输入框里的「文字 + 颜色」
   *    存成预置，**不加到条目上**）；
   * ② **输入框**（回车即添加）+ 右侧「存入预置」勾选框：同一行、同高、同圆角，
   *    输入框的占位文字就是勾选框的说明；
   * ③ **两排配色胶囊**（见 TagColorPicker，末位是炫彩色盘）。
   *
   * 输入框样式复用 0.7.8 就有的 `.tag-editor-input-row`（编辑器里的加标签行同款）——
   * v0.8.1 曾自创了一个 CSS 里根本不存在的 `.tag-input-row`，输入框裸奔、勾选框换行，
   * 与整个项目的观感格格不入。
   */
  import { Plus, X } from "@lucide/svelte";
  import { appSettings, showToast } from "./stores";
  import { setConfig } from "./actions";
  import { tagChipStyle, tagKey } from "./tagColors";
  import TagColorPicker from "./TagColorPicker.svelte";
  import type { TagColor } from "./types";

  /** 把标签加到当前条目上（各宿主的写路径不同，由它们自己实现） */
  export let onAdd: (tag: { color: TagColor; hex?: string; text?: string }) => void;
  /** 预置标签面板要窄一些（编辑器里的浮窗比菜单小） */
  export let compact = false;

  /** 与 core 的 `expect_tag_presets` 同一个上限 */
  const MAX_PRESETS = 64;

  let draft = "";
  let keepInPresets = true;
  let color: TagColor = "yellow";
  let hex = "";

  $: presets = $appSettings.appearance.tagPresets;

  async function writePresets(next: typeof presets): Promise<void> {
    const ok = await setConfig("appearance.tagPresets", next);
    if (!ok) showToast("预置标签保存失败");
  }

  /** 输入框提交（回车）：加到条目上；勾了「存入预置」就顺手存一份 */
  function submit(): void {
    const text = draft.trim();
    if (!text) return;
    onAdd({ color, hex: color === "custom" ? hex || undefined : undefined, text });
    if (keepInPresets) void addPreset({ color, hex, text }, false);
    draft = "";
  }

  /** 加一条预置。`fromDraft` 为真时（预置流末尾的「+」胶囊）只存预置、不加到条目上。 */
  async function addPreset(
    tag: { color: TagColor; hex?: string; text?: string },
    fromDraft: boolean
  ): Promise<void> {
    const text = (tag.text ?? "").trim();
    if (!text) return;
    const entry = {
      id: `tagpreset-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
      color: tag.color,
      text,
      hex: tag.color === "custom" && tag.hex ? tag.hex : undefined
    };
    if (presets.some((preset) => tagKey(preset) === tagKey(entry))) {
      if (fromDraft) showToast("这条标签已经在预置里了");
      return;
    }
    if (presets.length >= MAX_PRESETS) {
      showToast(`预置标签最多 ${MAX_PRESETS} 条，先删掉几条再加`);
      return;
    }
    await writePresets([...presets, entry]);
    if (fromDraft) draft = "";
  }

  function removePreset(id: string): void {
    void writePresets(presets.filter((preset) => preset.id !== id));
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    submit();
  }
</script>

<div class="tag-panel" class:compact>
  <div class="tag-preset-list">
    {#each presets as preset (preset.id)}
      <span class={`tag-preset tag-${preset.color}`} style={tagChipStyle(preset)}>
        <button
          class="tag-preset-main"
          type="button"
          title="加到这条上"
          on:click={() => onAdd({ color: preset.color, hex: preset.hex, text: preset.text })}
        >{preset.text || "（无色名）"}</button>
        <button
          class="tag-preset-delete"
          type="button"
          title="删除这条预置"
          aria-label="删除这条预置"
          on:click={() => removePreset(preset.id)}
        ><X size={12} /></button>
      </span>
    {/each}
    <button
      class="tag-preset tag-preset-new"
      type="button"
      title="把输入框里的标签存为预置（不加到这条上）"
      aria-label="添加预置标签"
      disabled={!draft.trim()}
      on:click={() => void addPreset({ color, hex, text: draft }, true)}
    ><Plus size={14} /></button>
  </div>

  <div class="tag-editor-input-row">
    <input
      bind:value={draft}
      placeholder="勾选将新标签自动添加到预置"
      maxlength={20}
      on:keydown={handleKeydown}
    />
    <label class="tag-keep-box" title="勾选时，从这里添加的标签会同时存进预置标签，下次一点就有">
      <input type="checkbox" bind:checked={keepInPresets} aria-label="新标签自动添加到预置" />
    </label>
  </div>

  <TagColorPicker
    {color}
    {hex}
    on:change={(event) => {
      color = event.detail.color;
      hex = event.detail.hex;
    }}
  />
</div>

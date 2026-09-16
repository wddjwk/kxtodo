<script lang="ts">
  /**
   * 右键菜单「标签」面板（条目与日记条目共用一套）。
   *
   * v0.8.1 起的结构，自上而下三块：
   * ① **预置标签**——点一下加到当前条目上；每个右侧一个删除叉删掉这条预置；
   *    末尾右下的加号把输入框里的「文字 + 颜色」存成预置（**不加到条目上**）；
   * ② **输入框** + 右侧「存入预置」勾选框（默认勾）：回车/加号提交时，
   *    勾着就顺手把这条标签也存进预置；
   * ③ **两排配色**（见 TagColorPicker）。
   *
   * 早先这里展示的是「当前条目已有的标签」——那件事卡片上的标签条已经做了
   * （还带编辑与删除），重复一遍反而占了预置标签的位置。清除按钮同理删掉：
   * 删单个标签在卡片上点一下就行，为了「一次性清空」在菜单里留一个危险按钮不值当。
   */
  import { Plus, Trash2, X } from "@lucide/svelte";
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

  function currentTag(): { color: TagColor; hex?: string; text?: string } {
    return {
      color,
      hex: color === "custom" ? hex || undefined : undefined,
      text: draft.trim() || undefined
    };
  }

  async function writePresets(next: typeof presets): Promise<void> {
    const ok = await setConfig("appearance.tagPresets", next);
    if (!ok) showToast("预置标签保存失败");
  }

  /** 输入框提交：加到条目上；勾了「存入预置」就顺手存一份 */
  function submit(): void {
    const text = draft.trim();
    if (!text) return;
    onAdd({ color, hex: color === "custom" ? hex || undefined : undefined, text });
    if (keepInPresets) void addPreset({ color, hex, text }, false);
    draft = "";
  }

  /** 加一条预置。`fromDraft` 为真时（面板右下角的加号）只存预置、不加到条目上。 */
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
  <div class="tag-panel-block">
    <span class="tag-panel-label">预置标签</span>
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
      {:else}
        <p class="tag-preset-empty">还没有预置标签：先在下面写好文字与颜色，点右下角的加号存一条。</p>
      {/each}
    </div>
    <div class="tag-preset-actions">
      <button
        class="tag-preset-add"
        type="button"
        title="把输入框里的标签存为预置（不加到这条上）"
        aria-label="添加预置标签"
        disabled={!draft.trim()}
        on:click={() => void addPreset({ color, hex, text: draft }, true)}
      ><Plus size={15} /></button>
    </div>
  </div>

  <div class="tag-panel-block">
    <div class="tag-input-row">
      <input
        bind:value={draft}
        placeholder={keepInPresets ? "输入标签文字…" : "输入标签文字…（不存入预置）"}
        maxlength={20}
        on:keydown={handleKeydown}
      />
      <button class="tag-submit" type="button" disabled={!draft.trim()} on:click={submit} title="添加标签">
        <Plus size={15} />
      </button>
    </div>
    <label class="tag-keep-row" title="勾上时，从这里添加的标签会同时存进预置标签，下次一点就有">
      <input type="checkbox" bind:checked={keepInPresets} />
      <span>勾选将新标签加入预置</span>
    </label>
  </div>

  <div class="tag-panel-block">
    <span class="tag-panel-label">颜色</span>
    <TagColorPicker
      {color}
      {hex}
      on:change={(event) => {
        color = event.detail.color;
        hex = event.detail.hex;
      }}
    />
  </div>
</div>

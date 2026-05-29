<script lang="ts">
  import "emoji-picker-element";
  import IconGlyph from "./IconGlyph.svelte";

  export let selected = "list";
  export let onPick: (icon: string) => void;
  export let onClose: () => void;

  const iconPresets = [
    "list",
    "lightbulb",
    "notebook",
    "folder",
    "file",
    "book",
    "bookmark",
    "code",
    "cpu",
    "wrench",
    "inbox",
    "tag",
    "archive",
    "briefcase",
    "check-square",
    "gift",
    "lock",
    "heart",
    "music",
    "home",
    "bell",
    "brain",
    "camera",
    "car",
    "palette",
    "plane",
    "rocket",
    "shopping-cart"
  ];

  const emojiFallback = ["💡", "✅", "📝", "📌", "📚", "🎁", "🔐", "🎧", "🚗", "🛒", "❤️", "⭐"];

  function handleEmojiClick(event: CustomEvent<{ unicode: string }>): void {
    if (event.detail?.unicode) {
      onPick(event.detail.unicode);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="icon-picker-panel" on:click|stopPropagation>
  <div class="picker-header">
    <strong>选择图标</strong>
    <button type="button" on:click={onClose}>×</button>
  </div>

  <div class="icon-grid" aria-label="标准图标">
    {#each iconPresets as preset}
      <button class:selected={selected === preset} type="button" title={preset} on:click={() => onPick(preset)}>
        <IconGlyph icon={preset} size={21} />
      </button>
    {/each}
  </div>

  <div class="emoji-grid" aria-label="常用表情">
    {#each emojiFallback as emoji}
      <button class:selected={selected === emoji} type="button" on:click={() => onPick(emoji)}>{emoji}</button>
    {/each}
  </div>

  <emoji-picker class="emoji-picker" locale="zh" on:emoji-click={handleEmojiClick}></emoji-picker>
</div>

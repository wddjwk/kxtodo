<script lang="ts">
  import "emoji-picker-element";
  import IconGlyph from "./IconGlyph.svelte";

  export let selected = "";
  export let mode: "icon" | "emoji" = "icon";
  export let onPick: (icon: string) => void;
  export let onClose: () => void;

  const iconPresets = [
    "list", "lightbulb", "notebook", "folder", "file", "book", "bookmark",
    "code", "cpu", "wrench", "inbox", "tag", "archive", "briefcase",
    "check-square", "gift", "lock", "heart", "music", "home", "bell",
    "brain", "camera", "car", "palette", "plane", "rocket", "shopping-cart"
  ];

  const iconEmojiPresets = ["💡", "✅", "📝", "📌", "📚", "🎁", "🔐", "🎧", "🚗", "🛒", "❤️", "⭐"];

  const taskEmojiPresets = [
    "🚩", "🏁", "⚑", "🔴", "🟡", "🟢", "🔵", "⚪",
    "❗", "⚡", "🔥", "💯", "✅", "☑️", "✔️", "❌",
    "⏳", "🕐", "📅", "🗓️", "⌛", "🔄", "🚀", "🎯",
    "📊", "📈", "📉", "🏆", "🥇", "🥈", "🥉", "⭐",
    "💡", "🔑", "📌", "📎", "🗂️", "📁", "🔔", "💤"
  ];

  function handleEmojiClick(event: CustomEvent<{ unicode: string }>): void {
    if (event.detail?.unicode) {
      onPick(event.detail.unicode);
    }
  }

  function handleBackdropClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="icon-picker-backdrop" on:click={handleBackdropClick} on:contextmenu|preventDefault|stopPropagation>
  <div class="icon-picker" on:click|stopPropagation>
    <header>
      <strong>{mode === "icon" ? "选择图标" : "选择表情"}</strong>
      <button type="button" on:click={onClose}>×</button>
    </header>

    {#if mode === "icon"}
      <div class="icon-grid" aria-label="标准图标">
        {#each iconPresets as preset}
          <button class:selected={selected === preset} type="button" title={preset} on:click={() => onPick(preset)}>
            <IconGlyph icon={preset} size={21} />
          </button>
        {/each}
      </div>

      <div class="picker-section-label">常用表情</div>
      <div class="emoji-grid" aria-label="常用表情">
        {#each iconEmojiPresets as emoji}
          <button class:selected={selected === emoji} type="button" on:click={() => onPick(emoji)}>{emoji}</button>
        {/each}
      </div>
    {:else}
      <div class="picker-section-label">常用</div>
      <div class="emoji-grid emoji-grid-wide" aria-label="常用表情">
        {#each taskEmojiPresets as emoji}
          <button class:selected={selected === emoji} type="button" on:click={() => onPick(emoji)}>{emoji}</button>
        {/each}
      </div>
    {/if}

    <div class="picker-section-label">全部表情</div>
    <emoji-picker class="emoji-picker" locale="zh" on:emoji-click={handleEmojiClick}></emoji-picker>
  </div>
</div>

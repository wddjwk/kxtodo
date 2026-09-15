<script lang="ts">
  /**
   * 图标 / 表情选择器（分组与条目的「选择图标」、任务的「添加表情」、两个编辑器共用）。
   * v0.7.5：简笔画从 28 个零散预设换成与记账共用的分组目录（LEDGER_ICON_GROUPS，
   * 20 组两百多个）——分组 chips + 图标格子与记账管理器同一套视觉语言；
   * 选中的简笔画存 lucide 的 PascalCase 名字（IconGlyph 认它，旧的 kebab 名照旧渲染）。
   * v0.7.7：顶部新增「常用图标」= 最近用过的（表情与简笔画混排，最多两行），
   * 简笔画区固定五行高、自己滚（滚动条藏起来），整体与 emoji 区同一套观感。
   * emoji-picker-element 的滚动区在 shadow DOM 里，外部样式表够不着——挂载后注入
   * 一段样式把滚动条藏掉（「选 emoji 不要展示滚动条」）。
   */
  import "emoji-picker-element";
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import IconGlyph from "./IconGlyph.svelte";
  import { LEDGER_ICON_GROUPS } from "./ledgerIcons";
  import { loadRecentIcons, rememberIcon } from "./recentIcons";

  export let selected = "";
  export let mode: "icon" | "emoji" = "icon";
  export let onPick: (icon: string) => void;
  export let onClose: () => void;

  const ALL_GROUP = "全部";

  const taskEmojiPresets = [
    "🚩", "🏁", "⚑", "🔴", "🟡", "🟢", "🔵", "⚪",
    "❗", "⚡", "🔥", "💯", "✅", "☑️", "✔️", "❌",
    "⏳", "🕐", "📅", "🗓️", "⌛", "🔄", "🚀", "🎯",
    "📊", "📈", "📉", "🏆", "🥇", "🥈", "🥉", "⭐",
    "💡", "🔑", "📌", "📎", "🗂️", "📁", "🔔", "💤"
  ];

  let iconGroup = ALL_GROUP;
  let pickerEl: HTMLElement;
  /** 最近用过的（表情 + 简笔画混排）；每选一次刷新，供顶部「常用图标」用 */
  let recents = loadRecentIcons();

  $: iconChoices =
    iconGroup === ALL_GROUP
      ? LEDGER_ICON_GROUPS.flatMap((group) => [...group.icons]).filter(
          (name, index, all) => all.indexOf(name) === index
        )
      : LEDGER_ICON_GROUPS.find((group) => group.name === iconGroup)?.icons ?? [];

  /** 所有选择都过这里：记一笔最近使用，再交给调用方 */
  function pick(value: string): void {
    rememberIcon(value);
    recents = loadRecentIcons();
    onPick(value);
  }

  function handleEmojiClick(event: CustomEvent<{ unicode: string }>): void {
    if (event.detail?.unicode) {
      pick(event.detail.unicode);
    }
  }

  function handleBackdropClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  onMount(() => {
    // shadow DOM 里的滚动条只能注进去藏（外层样式表够不着）
    const host = pickerEl?.querySelector("emoji-picker");
    const root = host?.shadowRoot;
    if (root && !root.querySelector("#kx-no-scrollbar")) {
      const style = document.createElement("style");
      style.id = "kx-no-scrollbar";
      style.textContent =
        "* { scrollbar-width: none !important; } *::-webkit-scrollbar { display: none !important; width: 0 !important; height: 0 !important; }";
      root.appendChild(style);
    }
    window.addEventListener("keydown", handleKeydown, true);
    return () => window.removeEventListener("keydown", handleKeydown, true);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="icon-picker-backdrop" on:click={handleBackdropClick} on:contextmenu|preventDefault|stopPropagation>
  <div class="icon-picker" bind:this={pickerEl} on:click|stopPropagation>
    <header>
      <strong>{mode === "icon" ? "选择图标" : "选择表情"}</strong>
      <button type="button" class="icon-picker-close" title="关闭" aria-label="关闭" on:click={onClose}>
        <X size={17} />
      </button>
    </header>

    {#if mode === "icon"}
      {#if recents.length > 0}
        <div class="picker-section-label">常用图标</div>
        <div class="emoji-grid" aria-label="常用图标">
          {#each recents as item (item)}
            <button type="button" class:selected={selected === item} title={item} on:click={() => pick(item)}>
              <IconGlyph icon={item} size={20} />
            </button>
          {/each}
        </div>
      {/if}

      <div class="icon-group-chips" role="tablist" aria-label="图标分组">
        <button
          type="button"
          role="tab"
          class="icon-group-chip"
          class:active={iconGroup === ALL_GROUP}
          on:click={() => (iconGroup = ALL_GROUP)}
        >{ALL_GROUP}</button>
        {#each LEDGER_ICON_GROUPS as group (group.name)}
          <button
            type="button"
            role="tab"
            class="icon-group-chip"
            class:active={iconGroup === group.name}
            on:click={() => (iconGroup = group.name)}
          >{group.name}</button>
        {/each}
      </div>
      <div class="icon-grid" aria-label="简笔画图标">
        {#each iconChoices as name (name)}
          <button type="button" class:selected={selected === name} title={name} on:click={() => pick(name)}>
            <IconGlyph icon={name} size={20} />
          </button>
        {/each}
      </div>
    {:else}
      <div class="picker-section-label">常用</div>
      <div class="emoji-grid emoji-grid-wide" aria-label="常用表情">
        {#each taskEmojiPresets as emoji (emoji)}
          <button type="button" class:selected={selected === emoji} on:click={() => pick(emoji)}>{emoji}</button>
        {/each}
      </div>
    {/if}

    <div class="picker-section-label">全部表情</div>
    <emoji-picker class="emoji-picker" locale="zh" on:emoji-click={handleEmojiClick}></emoji-picker>
  </div>
</div>

<script lang="ts">
  /**
   * 标语栏工具页（v0.9.4）：展示配置（总开关 / 位置 / 图标）+ 标语列表维护。
   * 勾选 = 选中展示（多条轮播）；行内编辑 / 两步删除；底部输入框添加（新标语自动选中）。
   * 数据是 data.json 的单例（gui.set-slogan，与草稿纸同谱系同步）。
   * 悬浮展示本体是 SloganBanner（挂 App 壳）；移动端只在这里维护内容，不实际展示。
   */
  import { Check, ChevronRight, Megaphone, Pencil, Plus, Trash2, X } from "@lucide/svelte";
  import { get } from "svelte/store";
  import { appState, showToast } from "../stores";
  import { setSlogan } from "../actions";
  import { activeSlogans, sloganIconName, withNewItem } from "../slogan";
  import { ledgerIcon } from "../ledgerIcons";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import ToggleSwitch from "../ToggleSwitch.svelte";
  import type { SloganState } from "../types";

  let draft = "";
  let editingId: string | null = null;
  let editDraft = "";
  let confirmDelete: string | null = null;
  let iconMenuAt: { x: number; y: number; anchor: HTMLElement } | null = null;
  let iconPickerOpen = false;

  $: slogan = $appState.slogan;
  $: shown = activeSlogans(slogan);
  $: iconName = sloganIconName(slogan.icon);
  $: iconLabel = slogan.icon === "none" ? "不加图标" : slogan.icon ? slogan.icon : "默认喇叭";
  $: preview = shown[0] ?? null;

  function commit(changes: Partial<SloganState>): void {
    void setSlogan({ ...get(appState).slogan, ...changes });
  }

  function newItemId(): string {
    if (crypto.randomUUID) return `slogan-${crypto.randomUUID().replace(/-/g, "")}`;
    return `slogan-${Math.random().toString(16).slice(2, 10)}${Math.random().toString(16).slice(2, 10)}`;
  }

  function addItem(): void {
    const text = draft.trim();
    if (!text) return;
    const id = newItemId();
    const items = withNewItem(slogan, id, text, new Date().toISOString());
    if (items === slogan.items) return;
    draft = "";
    // 新标语自动选中展示：写完立刻能在页面角落看到，才是这个工具的意义
    commit({ items, activeIds: [...slogan.activeIds, id] });
  }

  function toggleActive(id: string): void {
    const activeIds = slogan.activeIds.includes(id)
      ? slogan.activeIds.filter((item) => item !== id)
      : [...slogan.activeIds, id];
    commit({ activeIds });
  }

  function startEdit(id: string, text: string): void {
    editingId = id;
    editDraft = text;
    confirmDelete = null;
  }

  function commitEdit(): void {
    if (!editingId) return;
    const id = editingId;
    const text = editDraft.trim().slice(0, 80);
    editingId = null;
    if (!text) return;
    commit({ items: slogan.items.map((item) => (item.id === id ? { ...item, text } : item)) });
  }

  function cancelEdit(): void {
    editingId = null;
  }

  function removeItem(id: string): void {
    confirmDelete = null;
    commit({
      items: slogan.items.filter((item) => item.id !== id),
      activeIds: slogan.activeIds.filter((item) => item !== id)
    });
    showToast("标语已删除", 1500);
  }

  function openIconMenu(event: MouseEvent): void {
    const anchor = event.currentTarget as HTMLElement;
    const rect = anchor.getBoundingClientRect();
    iconMenuAt = { x: rect.right, y: rect.bottom + 6, anchor };
  }

  function pickIcon(icon: string): void {
    iconMenuAt = null;
    commit({ icon });
  }
</script>

<div class="slogan-tool">
  <section class="slogan-panel">
    <div class="slogan-setting-row">
      <span class="slogan-setting-label">在所有页面显示</span>
      <ToggleSwitch checked={slogan.enabled} label="在所有页面显示标语" onChange={(value) => commit({ enabled: value })} />
    </div>
    <div class="slogan-setting-row">
      <span class="slogan-setting-label">位置</span>
      <div class="segmented slogan-position-switch" role="group" aria-label="标语位置">
        <button type="button" class:active={slogan.position === "top"} aria-pressed={slogan.position === "top"} on:click={() => commit({ position: "top" })}>标题上方</button>
        <button type="button" class:active={slogan.position === "bottom"} aria-pressed={slogan.position === "bottom"} on:click={() => commit({ position: "bottom" })}>输入框下方</button>
      </div>
    </div>
    <div class="slogan-setting-row">
      <span class="slogan-setting-label">图标</span>
      <button type="button" class="slogan-icon-trigger" aria-expanded={iconMenuAt !== null} on:click={openIconMenu}>
        {#if iconName}
          <svelte:component this={ledgerIcon(iconName, "Megaphone")} size={15} />
        {:else}
          <Megaphone size={15} class="slogan-icon-none" />
        {/if}
        <span>{iconLabel}</span>
        <ChevronRight size={14} />
      </button>
    </div>
    <p class="slogan-note">标语右对齐悬浮在页面角落，不挤占排版；多条选中时轮播。移动端只在这里维护内容，不实际展示。</p>
  </section>

  {#if preview}
    <div class="slogan-preview-wrap">
      <span class="slogan-preview-label">预览</span>
      <span class="slogan-banner static">
        <span class="slogan-banner-body">
          {#if iconName}
            <svelte:component this={ledgerIcon(iconName, "Megaphone")} size={14} />
          {/if}
          <span class="slogan-banner-text">{preview.text}</span>
        </span>
      </span>
    </div>
  {/if}

  <header class="slogan-list-head">
    <span>标语列表</span>
    <span class="slogan-list-count">{slogan.items.length} 条 · 展示 {slogan.activeIds.length} 条{slogan.activeIds.length > 1 ? "（轮播）" : ""}</span>
  </header>

  <div class="slogan-list">
    {#each slogan.items as item (item.id)}
      <div class="slogan-row" class:active={slogan.activeIds.includes(item.id)}>
        <button
          type="button"
          class="slogan-check"
          class:on={slogan.activeIds.includes(item.id)}
          role="checkbox"
          aria-checked={slogan.activeIds.includes(item.id)}
          aria-label="展示「{item.text}」"
          title={slogan.activeIds.includes(item.id) ? "取消展示" : "选中展示"}
          on:click={() => toggleActive(item.id)}
        >
          {#if slogan.activeIds.includes(item.id)}<Check size={13} />{/if}
        </button>
        {#if editingId === item.id}
          <input
            class="slogan-edit-input"
            type="text"
            maxlength="80"
            bind:value={editDraft}
            on:keydown={(event) => {
              if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) {
                event.preventDefault();
                commitEdit();
              } else if (event.key === "Escape") {
                event.preventDefault();
                cancelEdit();
              }
            }}
            on:blur={commitEdit}
          />
        {:else}
          <span class="slogan-text" title={item.text}>{item.text}</span>
        {/if}
        <button type="button" class="slogan-row-btn" title="编辑" aria-label="编辑标语" on:click={() => startEdit(item.id, item.text)}>
          <Pencil size={14} />
        </button>
        {#if confirmDelete === item.id}
          <button type="button" class="slogan-row-btn danger" title="确认删除" aria-label="确认删除" on:click={() => removeItem(item.id)}>
            <Check size={14} />
          </button>
          <button type="button" class="slogan-row-btn" title="取消" aria-label="取消删除" on:click={() => (confirmDelete = null)}>
            <X size={14} />
          </button>
        {:else}
          <button type="button" class="slogan-row-btn danger" title="删除" aria-label="删除标语" on:click={() => (confirmDelete = item.id)}>
            <Trash2 size={14} />
          </button>
        {/if}
      </div>
    {/each}
    {#if slogan.items.length === 0}
      <p class="slogan-empty">还没有标语，在下面写一句提醒自己的话</p>
    {/if}
  </div>

  <form class="slogan-add" on:submit|preventDefault={addItem}>
    <input class="slogan-add-input" type="text" maxlength="80" placeholder="写一句标语，如：久坐了起来活动一下" bind:value={draft} />
    <button type="submit" class="settings-button primary" disabled={draft.trim() === ""}>
      <Plus size={15} />添加
    </button>
  </form>
</div>

{#if iconMenuAt}
  <ContextMenu x={iconMenuAt.x} y={iconMenuAt.y} xAlign="right" anchor={iconMenuAt.anchor} onClose={() => (iconMenuAt = null)}>
    <MenuItem checkable active={slogan.icon === ""} label="默认喇叭" onSelect={() => pickIcon("")} />
    <MenuItem checkable active={slogan.icon === "none"} label="不加图标" onSelect={() => pickIcon("none")} />
    <MenuSeparator />
    <MenuItem icon={Megaphone} label="选择图标…" onSelect={() => { iconMenuAt = null; iconPickerOpen = true; }} />
  </ContextMenu>
{/if}

{#if iconPickerOpen}
  {#await import("../IconPicker.svelte")}
    <p class="toolbox-empty">正在打开图标目录…</p>
  {:then module}
    <svelte:component
      this={module.default}
      mode="icon"
      selected={iconName ?? "Megaphone"}
      onPick={(picked) => { iconPickerOpen = false; commit({ icon: picked }); }}
      onClose={() => (iconPickerOpen = false)}
    />
  {:catch}
    <p class="toolbox-empty">图标目录打开失败</p>
  {/await}
{/if}
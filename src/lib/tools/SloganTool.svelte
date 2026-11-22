<script lang="ts">
  /**
   * 标语栏工具页（v0.9.4；v0.9.5 重做）：展示配置 + 标语卡片列表。
   * 配置：总开关 / 六个悬浮位置 / 左右边界基准（输入框边界 vs 程序边界，默认输入框） /
   * 主题色（图标与描边） / 字体颜色 / 背景透明度 / 轮播间隔（秒） / 新标语默认图标。
   * 列表：每张卡片自带图标（默认喇叭/不加/lucide/emoji），点文字直接行内编辑（保存/取消/
   * Enter/Esc），两步删除；底部输入框添加并自动选中（用默认图标）。
   * 数据是 data.json 单例（gui.set-slogan）。悬浮展示本体是 SloganBanner（挂 App 壳）；
   * 移动端只在这里维护内容，不实际展示。
   */
  import { Check, Megaphone, Palette, Plus, Smile, Trash2, X } from "@lucide/svelte";
  import { get } from "svelte/store";
  import { appState, showToast } from "../stores";
  import { setSlogan } from "../actions";
  import { activeSlogans, sloganIconKind, SLOGAN_POSITION_LABELS, withNewItem } from "../slogan";
  import { openColorPicker } from "../colorPickerPanel";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import NumberField from "../NumberField.svelte";
  import SloganIcon from "../SloganIcon.svelte";
  import ToggleSwitch from "../ToggleSwitch.svelte";
  import type { SloganPosition, SloganState } from "../types";

  const POSITION_GRID: SloganPosition[] = [
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "top-center",
    "bottom-center"
  ];

  let draft = "";
  let editingId: string | null = null;
  let editDraft = "";
  let confirmDelete: string | null = null;
  let iconMenu: { x: number; y: number; anchor: HTMLElement; itemId: string } | null = null;
  /** 菜单关闭会清 iconMenu；onSelect 在关闭之后才跑，所以 id 单独留一份。 */
  let iconMenuItemId: string | null = null;
  let defaultIconMenu: { x: number; y: number; anchor: HTMLElement } | null = null;
  /** 图标/emoji 选择器的目标：itemId = 改某条；null = 改新标语默认图标。 */
  let pickerTarget: { itemId: string | null; mode: "icon" | "emoji" } | null = null;
  /** 取色器的拖动预览只住这里（onPreview），确认才落盘——别把拖动过程写进 core。 */
  let colorDraft: { kind: "accent" | "textColor"; value: string } | null = null;

  $: slogan = $appState.slogan;
  $: shown = activeSlogans(slogan);
  $: preview = shown[0] ?? null;
  $: accentValue = colorDraft?.kind === "accent" ? colorDraft.value : slogan.accent;
  $: textValue = colorDraft?.kind === "textColor" ? colorDraft.value : slogan.textColor;
  $: themeStyle = `--slogan-accent: ${accentValue || "var(--accent)"}; --slogan-text: ${textValue || "#4b5563"}; --slogan-bg-alpha: ${slogan.opacity}`;

  function commit(changes: Partial<SloganState>): void {
    void setSlogan({ ...get(appState).slogan, ...changes });
  }

  function iconLabel(icon: string): string {
    if (icon === "none") return "不加图标";
    if (!icon) return "默认喇叭";
    return sloganIconKind(icon) === "emoji" ? icon : icon;
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
    if (!text) {
      showToast("标语内容不能为空");
      return;
    }
    editingId = null;
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

  function setItemIcon(itemId: string, icon: string): void {
    iconMenu = null;
    commit({ items: slogan.items.map((item) => (item.id === itemId ? { ...item, icon } : item)) });
  }

  function setDefaultIcon(icon: string): void {
    defaultIconMenu = null;
    commit({ defaultIcon: icon });
  }

  function openItemIconMenu(event: MouseEvent, itemId: string): void {
    const anchor = event.currentTarget as HTMLElement;
    const rect = anchor.getBoundingClientRect();
    iconMenuItemId = itemId;
    iconMenu = { x: rect.right, y: rect.bottom + 6, anchor, itemId };
  }

  function openDefaultIconMenu(event: MouseEvent): void {
    const anchor = event.currentTarget as HTMLElement;
    const rect = anchor.getBoundingClientRect();
    defaultIconMenu = { x: rect.right, y: rect.bottom + 6, anchor };
  }

  function openPicker(itemId: string | null, mode: "icon" | "emoji"): void {
    iconMenu = null;
    defaultIconMenu = null;
    pickerTarget = { itemId, mode };
  }
  function applyPicked(value: string): void {
    if (!pickerTarget) return;
    const itemId = pickerTarget.itemId;
    pickerTarget = null;
    if (itemId === null) commit({ defaultIcon: value });
    else setItemIcon(itemId, value);
  }

  function pickCustomColor(anchor: HTMLElement, kind: "accent" | "textColor", fallback: string): void {
    openColorPicker({
      key: `slogan:${kind}`,
      color: (kind === "accent" ? accentValue : textValue) || fallback,
      anchor,
      onPreview: (next) => (colorDraft = { kind, value: next }),
      onConfirm: (next) => {
        colorDraft = null;
        commit({ [kind]: next });
      },
      onCancel: () => (colorDraft = null)
    });
  }
</script>

<div class="slogan-tool">
  <section class="slogan-panel">
    <div class="slogan-setting-row">
      <span class="slogan-setting-label">在所有页面显示</span>
      <ToggleSwitch checked={slogan.enabled} label="在所有页面显示标语" onChange={(value) => commit({ enabled: value })} />
    </div>

    <div class="slogan-setting-block">
      <span class="slogan-setting-label">位置</span>
      <div class="slogan-position-grid" role="group" aria-label="标语位置">
        {#each POSITION_GRID as position (position)}
          <button
            type="button"
            class="slogan-position-btn"
            class:active={slogan.position === position}
            aria-pressed={slogan.position === position}
            on:click={() => commit({ position })}
          >{SLOGAN_POSITION_LABELS[position]}</button>
        {/each}
      </div>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">左右边界</span>
      <div class="slogan-bounds-switch" role="group" aria-label="左右边界基准">
        <button type="button" class="slogan-position-btn" class:active={slogan.bounds === "field"} aria-pressed={slogan.bounds === "field"} on:click={() => commit({ bounds: "field" })}>输入框边界</button>
        <button type="button" class="slogan-position-btn" class:active={slogan.bounds === "app"} aria-pressed={slogan.bounds === "app"} on:click={() => commit({ bounds: "app" })}>程序边界</button>
      </div>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">主题色 <small>图标与描边</small></span>
      <span class="slogan-color-control">
        <button
          type="button"
          class="slogan-color-dot"
          style={`--dot: ${accentValue || "var(--accent)"}`}
          title="选择主题色"
          aria-label="选择主题色"
          on:click={(event) => pickCustomColor(event.currentTarget, "accent", "#1f6feb")}
        ><Palette size={13} /></button>
        <span class="slogan-color-value">{accentValue || "跟随主题"}</span>
        {#if accentValue}
          <button type="button" class="slogan-color-reset" on:click={() => commit({ accent: "" })}>默认</button>
        {/if}
      </span>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">字体颜色</span>
      <span class="slogan-color-control">
        <button
          type="button"
          class="slogan-color-dot"
          style={`--dot: ${textValue || "#4b5563"}`}
          title="选择字体颜色"
          aria-label="选择字体颜色"
          on:click={(event) => pickCustomColor(event.currentTarget, "textColor", "#4b5563")}
        ><Palette size={13} /></button>
        <span class="slogan-color-value">{textValue || "默认"}</span>
        {#if textValue}
          <button type="button" class="slogan-color-reset" on:click={() => commit({ textColor: "" })}>默认</button>
        {/if}
      </span>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">背景透明度</span>
      <span class="slogan-param-control">
        <NumberField
          value={Math.round(slogan.opacity * 100)}
          min={30}
          max={100}
          suffix="%"
          ariaLabel="标语背景透明度"
          onCommit={(value) => commit({ opacity: Math.min(1, Math.max(0.3, value / 100)) })}
        />
      </span>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">轮播间隔</span>
      <span class="slogan-param-control">
        <NumberField
          value={slogan.intervalSeconds}
          min={2}
          max={120}
          suffix="秒"
          ariaLabel="标语轮播间隔秒数"
          onCommit={(value) => commit({ intervalSeconds: Math.min(120, Math.max(2, value)) })}
        />
      </span>
    </div>

    <div class="slogan-setting-row">
      <span class="slogan-setting-label">新标语默认图标</span>
      <button type="button" class="slogan-icon-trigger" aria-expanded={defaultIconMenu !== null} on:click={openDefaultIconMenu}>
        <span class="slogan-default-icon">
          <SloganIcon icon={slogan.defaultIcon} size={15} />
        </span>
        <span>{iconLabel(slogan.defaultIcon)}</span>
      </button>
    </div>
  </section>

  {#if preview}
    <div class="slogan-preview-wrap">
      <span class="slogan-preview-label">预览</span>
      <span class="slogan-banner static" style={themeStyle}>
        <span class="slogan-banner-body">
          <SloganIcon icon={preview.icon} size={14} />
          <span class="slogan-banner-text-wrap">
            <span class="slogan-banner-text">{preview.text}</span>
          </span>
        </span>
      </span>
    </div>
  {/if}

  <header class="slogan-list-head">
    <span>标语列表</span>
    <span class="slogan-list-count">{slogan.items.length} 条 · 展示 {slogan.activeIds.length} 条{slogan.activeIds.length > 1 ? `（每 ${slogan.intervalSeconds} 秒轮播）` : ""}</span>
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
        <button
          type="button"
          class="slogan-item-icon"
          class:none={sloganIconKind(item.icon) === "none"}
          title="本条图标"
          aria-label="选择本条标语图标"
          on:click={(event) => openItemIconMenu(event, item.id)}
        >
          <SloganIcon icon={item.icon} size={15} />
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
          />
          <button type="button" class="slogan-row-btn save" title="保存" aria-label="保存标语" on:click={commitEdit}>
            <Check size={14} />
          </button>
          <button type="button" class="slogan-row-btn" title="取消" aria-label="取消编辑" on:click={cancelEdit}>
            <X size={14} />
          </button>
        {:else}
          <button type="button" class="slogan-text slogan-text-btn" title="点击编辑" on:click={() => startEdit(item.id, item.text)}>{item.text}</button>
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

{#if iconMenu}
  <ContextMenu x={iconMenu.x} y={iconMenu.y} xAlign="right" anchor={iconMenu.anchor} onClose={() => (iconMenu = null)}>
    <MenuItem checkable active={slogan.items.find((item) => item.id === iconMenuItemId)?.icon === ""} label="默认喇叭" onSelect={() => iconMenuItemId && setItemIcon(iconMenuItemId, "")} />
    <MenuItem checkable active={slogan.items.find((item) => item.id === iconMenuItemId)?.icon === "none"} label="不加图标" onSelect={() => iconMenuItemId && setItemIcon(iconMenuItemId, "none")} />
    <MenuSeparator />
    <MenuItem icon={Megaphone} label="选择图标…" onSelect={() => openPicker(iconMenuItemId, "icon")} />
    <MenuItem icon={Smile} label="选择 Emoji…" onSelect={() => openPicker(iconMenuItemId, "emoji")} />
  </ContextMenu>
{/if}

{#if defaultIconMenu}
  <ContextMenu x={defaultIconMenu.x} y={defaultIconMenu.y} xAlign="right" anchor={defaultIconMenu.anchor} onClose={() => (defaultIconMenu = null)}>
    <MenuItem checkable active={slogan.defaultIcon === ""} label="默认喇叭" onSelect={() => setDefaultIcon("")} />
    <MenuItem checkable active={slogan.defaultIcon === "none"} label="不加图标" onSelect={() => setDefaultIcon("none")} />
    <MenuSeparator />
    <MenuItem icon={Megaphone} label="选择图标…" onSelect={() => openPicker(null, "icon")} />
    <MenuItem icon={Smile} label="选择 Emoji…" onSelect={() => openPicker(null, "emoji")} />
  </ContextMenu>
{/if}

{#if pickerTarget}
  {#await import("../IconPicker.svelte")}
    <p class="toolbox-empty">正在打开选择器…</p>
  {:then module}
    <svelte:component
      this={module.default}
      mode={pickerTarget.mode}
      selected={pickerTarget.itemId === null
        ? slogan.defaultIcon
        : (slogan.items.find((item) => item.id === pickerTarget?.itemId)?.icon ?? "")}
      onPick={applyPicked}
      onClose={() => (pickerTarget = null)}
    />
  {:catch}
    <p class="toolbox-empty">选择器打开失败</p>
  {/await}
{/if}
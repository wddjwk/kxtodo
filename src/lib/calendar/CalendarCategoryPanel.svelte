<script lang="ts">
  /**
   * 日程分类管理（v0.9.3 照业界标准重做：Outlook「管理分类」/ Todoist 标签管理器那一套）：
   * 列表 = 色点 + 名称 + 星形默认 + 删除，整行可点进编辑；底部一枚「添加分类」。
   * 编辑表单 = 名称 + 色板（选中打勾）+ 自定义取色 + 默认开关 + 迷你事件块预览。
   * 不写解释性文案（同步范围 / 删分类后果等一律不进界面）。
   *
   * 骨架照抄日记/日程编辑器的 `editor-overlay + editor-dialog`（返回键、Esc、点遮罩
   * 关闭、移动端安全区都是那一套），取色走全应用唯一的 `openColorPicker` 单例。
   * 删分类**不删日程**：用它的日程退回「无分类」（core 那边同时清引用）。
   */
  import { ArrowLeft, Check, Palette, Plus, Star, Trash2, X } from "@lucide/svelte";
  import { appSettings, calendarCategories, showToast } from "../stores";
  import { calendarAccent } from "../styles";
  import { addCalendarCategory, deleteCalendarCategory, updateCalendarCategory } from "../actions";
  import { eventColors } from "../calendar";
  import { openColorPicker } from "../colorPickerPanel";
  import { createBackGuard } from "../platform";
  import { imeInset } from "../imeInset";
  import { suppressGhostClick } from "../ghostClick";
  import type { CalendarCategory } from "../types";

  export let onClose: () => void = () => {};
  /** 打开就直接进新建表单（从日程编辑器的「新建分类…」过来时用） */
  export let startWithAdd = false;
  /** 新建成功后把这个分类 id 回传给调用方（编辑器据此选中它） */
  export let onCreated: (category: CalendarCategory) => void = () => {};

  const PALETTE = [
    "#7c5cbf", "#3d8bfd", "#2f9e6e", "#e0654f",
    "#d94f70", "#e8a33d", "#9b59b6", "#7cb342",
    "#0f9b9b", "#8d6e63", "#7f8c8d", "#34495e"
  ];

  let editingId: string | null = startWithAdd ? "" : null;
  let nameDraft = "";
  let colorDraft = "";
  let defaultDraft = false;
  let busy = false;

  $: categories = $calendarCategories;
  /** 分类没配色就跟日程主题色（与各视图同一条口径） */
  $: accent = calendarAccent($appSettings.calendar);
  $: editing = editingId === null ? null : categories.find((item) => item.id === editingId) ?? null;
  $: previewColors = eventColors({ categoryId: "" }, [], colorDraft || accent);
  $: nameTrimmed = nameDraft.trim();
  $: duplicated =
    nameTrimmed !== "" &&
    categories.some((item) => item.name === nameTrimmed && item.id !== editingId);

  const backGuard = createBackGuard();
  $: backGuard(true, () => {
    if (editingId !== null) {
      editingId = null;
      // 从编辑器进来的（没有列表可退）直接关
      if (startWithAdd) onClose();
      return;
    }
    onClose();
  });

  function startAdd(): void {
    editingId = "";
    nameDraft = "";
    colorDraft = PALETTE[categories.length % PALETTE.length];
    defaultDraft = categories.length === 0;
  }

  function startEdit(category: CalendarCategory): void {
    editingId = category.id;
    nameDraft = category.name;
    colorDraft = category.color;
    defaultDraft = category.isDefault;
  }

  function closeForm(): void {
    editingId = null;
    if (startWithAdd) onClose();
  }

  async function save(): Promise<void> {
    if (busy || nameTrimmed === "" || duplicated) return;
    busy = true;
    try {
      if (editingId === "") {
        const created = await addCalendarCategory({
          name: nameTrimmed,
          color: colorDraft,
          isDefault: defaultDraft
        });
        if (created) {
          showToast(`分类「${created.name}」已添加`);
          onCreated(created);
          if (startWithAdd) {
            onClose();
            return;
          }
          editingId = null;
        }
        return;
      }
      const target = editingId;
      if (!target) return;
      const ok = await updateCalendarCategory(target, {
        name: nameTrimmed,
        color: colorDraft,
        isDefault: defaultDraft
      });
      if (ok) {
        showToast("分类已保存");
        editingId = null;
      }
    } finally {
      busy = false;
    }
  }

  async function remove(category: CalendarCategory): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await deleteCalendarCategory(category.id);
      showToast(`分类「${category.name}」已删除`);
      if (editingId === category.id) editingId = null;
    } finally {
      busy = false;
    }
  }

  async function toggleDefault(category: CalendarCategory): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await updateCalendarCategory(category.id, { isDefault: !category.isDefault });
    } finally {
      busy = false;
    }
  }

  function pickCustomColor(anchor: HTMLElement): void {
    openColorPicker({
      key: "calendar:category-color",
      color: colorDraft || PALETTE[0],
      anchor,
      onPreview: (color) => (colorDraft = color),
      onConfirm: (color) => (colorDraft = color),
      onCancel: () => undefined
    });
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="editor-overlay" use:imeInset on:pointerdown={handleBackdropPointerDown}>
  <div
    class="editor-dialog calendar-sheet calendar-category-panel"
    role="dialog"
    aria-label="日程分类"
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="calendar-sheet-head">
      {#if editingId !== null}
        <button type="button" class="editor-icon-button" title="返回列表" aria-label="返回列表" on:click={closeForm}>
          <ArrowLeft size={17} />
        </button>
        <span class="calendar-sheet-title">{editingId === "" ? "新建分类" : "编辑分类"}</span>
      {:else}
        <span class="calendar-sheet-title">日程分类</span>
      {/if}
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="editor-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
        <X size={17} />
      </button>
    </header>

    {#if editingId !== null}
      <div class="calendar-sheet-body">
        <label class="calendar-field">
          <span class="calendar-field-label">名称</span>
          <input
            class="calendar-field-input"
            type="text"
            maxlength="24"
            placeholder="分类名称"
            bind:value={nameDraft}
            on:keydown={(event) => {
              if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) {
                event.preventDefault();
                void save();
              }
            }}
          />
        </label>

        <div class="calendar-field">
          <span class="calendar-field-label">颜色</span>
          <div class="calendar-cat-colors">
            {#each PALETTE as color (color)}
              <button
                type="button"
                class="calendar-cat-dot"
                class:active={colorDraft.toLowerCase() === color.toLowerCase()}
                style={`background: ${color}`}
                title={color}
                aria-label="使用颜色 {color}"
                on:click={() => (colorDraft = color)}
              >
                {#if colorDraft.toLowerCase() === color.toLowerCase()}<Check size={13} />{/if}
              </button>
            {/each}
            <button
              type="button"
              class="calendar-cat-dot custom"
              title="自定义颜色"
              aria-label="自定义颜色"
              style={`--dot: ${colorDraft || PALETTE[0]}`}
              on:click={(event) => pickCustomColor(event.currentTarget)}
            ><Palette size={13} /></button>
          </div>
        </div>

        <div class="calendar-field calendar-field-inline">
          <span class="calendar-field-label">默认分类</span>
          <button
            type="button"
            class="menu-action-button calendar-default-toggle"
            class:on={defaultDraft}
            aria-pressed={defaultDraft}
            title={defaultDraft ? "取消默认" : "设为默认"}
            on:click={() => (defaultDraft = !defaultDraft)}
          >
            <Star size={14} fill={defaultDraft ? "currentColor" : "none"} />
            {defaultDraft ? "默认" : "设为默认"}
          </button>
        </div>

        <div class="calendar-field">
          <span class="calendar-field-label">预览</span>
          <div class="calendar-cat-preview" style={`--cal-main: ${previewColors.main}; --cal-soft: ${previewColors.soft}`}>
            <span class="calendar-cat-preview-time">09:00 – 10:00</span>
            <span class="calendar-cat-preview-title">{nameTrimmed || "日程标题"}</span>
          </div>
        </div>

        {#if duplicated}
          <p class="calendar-field-error">已经有叫「{nameTrimmed}」的分类了</p>
        {/if}
      </div>

      <footer class="calendar-sheet-foot">
        {#if editing}
          <button type="button" class="settings-button danger" disabled={busy} on:click={() => void remove(editing as CalendarCategory)}>
            <Trash2 size={15} />删除
          </button>
        {/if}
        <span class="calendar-sheet-spacer"></span>
        <button type="button" class="settings-button" on:click={closeForm}>取消</button>
        <button
          type="button"
          class="settings-button primary"
          disabled={busy || nameTrimmed === "" || duplicated}
          on:click={() => void save()}
        ><Check size={15} />保存</button>
      </footer>
    {:else}
      <div class="calendar-sheet-body calendar-cat-list">
        {#if categories.length === 0}
          <p class="calendar-cat-empty">还没有分类</p>
        {/if}
        {#each categories as category (category.id)}
          <div
            class="calendar-cat-row"
            role="button"
            tabindex="0"
            title={`编辑 ${category.name}`}
            on:click={() => startEdit(category)}
            on:keydown={(event) => {
              if (event.target !== event.currentTarget) return;
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                startEdit(category);
              }
            }}
          >
            <span class="calendar-cat-color" style={`--dot: ${category.color || accent}`}></span>
            <span class="calendar-cat-name">{category.name}</span>
            <button
              type="button"
              class="editor-icon-button calendar-cat-star"
              class:on={category.isDefault}
              title={category.isDefault ? "取消默认" : "设为默认"}
              aria-pressed={category.isDefault}
              on:click|stopPropagation={() => void toggleDefault(category)}
            ><Star size={15} fill={category.isDefault ? "currentColor" : "none"} /></button>
            <button
              type="button"
              class="editor-icon-button danger calendar-cat-remove"
              title="删除"
              aria-label={`删除 ${category.name}`}
              disabled={busy}
              on:click|stopPropagation={() => void remove(category)}
            ><Trash2 size={15} /></button>
          </div>
        {/each}
      </div>

      <footer class="calendar-sheet-foot">
        <button type="button" class="settings-button primary calendar-cat-add" on:click={startAdd}><Plus size={15} />添加分类</button>
      </footer>
    {/if}
  </div>
</div>
<script lang="ts">
  /**
   * 分类管理：支出/收入两套两级分类的增删改（名字 + 图标 + 颜色 + 挂哪个大类）。
   * 与记账面板同一套浮层语言（桌面居中对话框 / 移动端底部抽屉）；表单是面板内的子层，
   * 打开表单时列表让位——不做弹窗套弹窗，也不让对话框里出现滚动条。
   *
   * 表单字段一律是平铺的 let 变量，不用 `{@const f = form}` 再 bind 到 `f.name`：
   * 那样改的是对象内部属性，Svelte 不会失效 form，保存按钮会一直停在 disabled。
   */
  import { ArrowLeft, PenLine, Plus, Trash2, X } from "@lucide/svelte";
  import { appSettings } from "../stores";
  import { imeInset } from "../imeInset";
  import { fieldKeydown } from "../shortcuts";
  import { ledgerAccent } from "../styles";
  import { categoryTree } from "../ledger";
  import { LEDGER_ICON_CHOICES, ledgerIcon, softColor } from "../ledgerIcons";
  import { addLedgerCategory, deleteLedgerCategory, updateLedgerCategory } from "../actions";
  import type { LedgerBook, LedgerCategory, LedgerSide } from "../types";

  export let book: LedgerBook;
  export let side: LedgerSide = "expense";
  export let onClose: () => void = () => {};

  /** 12 个预设色：日常收支场景够用，另有取色器兜底 */
  const COLORS = [
    "#f0862c", "#e0654f", "#d94f70", "#9b59b6",
    "#6b7fd7", "#3d8bfd", "#2f9e6e", "#7cb342",
    "#e8a33d", "#8d6e63", "#7f8c8d", "#34495e"
  ];

  /** null = 列表；否则是正在编辑的分类 id（空串 = 新建） */
  let editingId: string | null = null;
  let nameDraft = "";
  let iconDraft = "";
  let colorDraft = "";
  let parentDraft = "";
  let busy = false;
  let nameInput: HTMLInputElement;

  $: tree = categoryTree(book, side);
  $: accent = ledgerAccent($appSettings.ledger);
  $: parents = tree.map((item) => item.parent);
  $: formOpen = editingId !== null;
  $: fallbackColor = side === "income" ? "#2f9e6e" : "#f0862c";
  $: previewColor = colorDraft || fallbackColor;

  function switchSide(next: LedgerSide): void {
    if (next === side) return;
    side = next;
    editingId = null;
  }

  function focusName(): void {
    void Promise.resolve().then(() => nameInput?.focus());
  }

  function beginAdd(parentId = ""): void {
    editingId = "";
    nameDraft = "";
    iconDraft = "";
    colorDraft = "";
    parentDraft = parentId;
    focusName();
  }

  function beginEdit(category: LedgerCategory): void {
    editingId = category.id;
    nameDraft = category.name;
    iconDraft = category.icon;
    colorDraft = category.color;
    parentDraft = category.parentId ?? "";
    focusName();
  }

  function closeForm(): void {
    editingId = null;
  }

  async function saveForm(): Promise<void> {
    if (busy) return;
    const name = nameDraft.trim();
    if (!name) return;
    busy = true;
    const draft = {
      name,
      side,
      parentId: parentDraft || undefined,
      icon: iconDraft,
      color: colorDraft
    };
    const ok = editingId ? await updateLedgerCategory(editingId, draft) : await addLedgerCategory(draft);
    busy = false;
    if (!ok) return;
    editingId = null;
  }

  async function removeForm(): Promise<void> {
    if (!editingId || busy) return;
    busy = true;
    const ok = await deleteLedgerCategory(editingId);
    busy = false;
    if (!ok) return;
    editingId = null;
  }

  async function removeCategory(category: LedgerCategory): Promise<void> {
    if (busy) return;
    busy = true;
    await deleteLedgerCategory(category.id);
    busy = false;
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target === event.currentTarget) onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      event.stopPropagation();
      if (formOpen) closeForm();
      else onClose();
    } else if (event.key === "Enter" && formOpen && !event.isComposing && event.keyCode !== 229) {
      const element = event.target as HTMLElement | null;
      if (element?.tagName === "INPUT") {
        event.preventDefault();
        void saveForm();
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="editor-overlay ledger-overlay" use:imeInset on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
  <div
    class="editor-dialog ledger-sheet ledger-manager"
    style={`--accent: ${accent}`}
    role="dialog"
    aria-label="分类管理"
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="ledger-sheet-head">
      {#if formOpen}
        <button type="button" class="ledger-icon-button" title="返回列表" aria-label="返回列表" on:click={closeForm}>
          <ArrowLeft size={18} />
        </button>
        <span class="ledger-sheet-title">{editingId ? "编辑分类" : "添加分类"}</span>
      {:else}
        <div class="ledger-kind-tabs" role="tablist" aria-label="收支两侧">
          <button type="button" role="tab" class:active={side === "expense"} on:click={() => switchSide("expense")}>支出分类</button>
          <button type="button" role="tab" class:active={side === "income"} on:click={() => switchSide("income")}>收入分类</button>
        </div>
        <span class="ledger-sheet-title">分类管理</span>
      {/if}
      <div class="ledger-sheet-actions">
        <button type="button" class="ledger-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
          <X size={18} />
        </button>
      </div>
    </header>

    {#if formOpen}
      <div class="ledger-sheet-body ledger-form-body">
        <label class="ledger-field-row">
          <span>名称</span>
          <input bind:this={nameInput} bind:value={nameDraft} type="text" maxlength="12" placeholder="例如 早餐" on:keydown={fieldKeydown} />
        </label>

        <div class="ledger-field-row ledger-field-column">
          <span>归属</span>
          <div class="ledger-choice-row">
            <button type="button" class="ledger-choice" class:active={!parentDraft} on:click={() => (parentDraft = "")}>作为大类</button>
            {#each parents as parent (parent.id)}
              {#if parent.id !== editingId}
                <button type="button" class="ledger-choice" class:active={parentDraft === parent.id} on:click={() => (parentDraft = parent.id)}>
                  {parent.name}
                </button>
              {/if}
            {/each}
          </div>
        </div>

        <div class="ledger-field-row ledger-field-column">
          <span>图标</span>
          <div class="ledger-icon-grid">
            {#each LEDGER_ICON_CHOICES as name (name)}
              {@const icon = ledgerIcon(name, name)}
              <button
                type="button"
                class="ledger-icon-cell"
                class:active={iconDraft === name}
                title={name}
                on:click={() => (iconDraft = iconDraft === name ? "" : name)}
              >
                <svelte:component this={icon} size={18} />
              </button>
            {/each}
          </div>
        </div>

        <div class="ledger-field-row ledger-field-column">
          <span>颜色</span>
          <div class="ledger-color-row">
            {#each COLORS as color (color)}
              <button
                type="button"
                class="ledger-color-dot"
                class:active={colorDraft === color}
                style="background: {color}"
                title={color}
                on:click={() => (colorDraft = colorDraft === color ? "" : color)}
              ></button>
            {/each}
            <label class="ledger-color-custom" title="自定义颜色">
              <input type="color" value={colorDraft || fallbackColor} on:input={(event) => (colorDraft = event.currentTarget.value)} />
            </label>
            {#if colorDraft}
              <button type="button" class="ledger-choice" on:click={() => (colorDraft = "")}>跟随大类</button>
            {/if}
          </div>
        </div>

        <div class="ledger-form-preview">
          <span class="ledger-tile-icon" style="--cat: {previewColor}; background: {softColor(previewColor)}">
            <svelte:component this={ledgerIcon(iconDraft, "Package")} size={20} />
          </span>
          <em>{nameDraft.trim() || "分类名称"}</em>
        </div>
      </div>

      <footer class="ledger-sheet-foot">
        {#if editingId}
          <button type="button" class="settings-button danger" disabled={busy} on:click={() => void removeForm()}>
            <Trash2 size={15} />删除分类
          </button>
        {/if}
        <span class="ledger-foot-spacer"></span>
        <button type="button" class="settings-button" on:click={closeForm}>取消</button>
        <button type="button" class="settings-button primary" disabled={busy || !nameDraft.trim()} on:click={() => void saveForm()}>保存</button>
      </footer>
    {:else}
      <div class="ledger-sheet-body">
        {#each tree as item (item.parent.id)}
          {@const parentColor = item.parent.color || fallbackColor}
          <section class="ledger-cat-group">
            <header class="ledger-cat-group-head">
              <span class="ledger-tile-icon" style="--cat: {parentColor}; background: {softColor(parentColor)}">
                <svelte:component this={ledgerIcon(item.parent.icon, "Package")} size={17} />
              </span>
              <strong>{item.parent.name}</strong>
              <em>{item.children.length} 个子分类</em>
              <span class="ledger-cat-group-actions">
                <button type="button" class="ledger-mini-button" title="在这个大类下添加" on:click={() => beginAdd(item.parent.id)}>
                  <Plus size={14} />
                </button>
                <button type="button" class="ledger-mini-button" title="编辑大类" on:click={() => beginEdit(item.parent)}>
                  <PenLine size={14} />
                </button>
                <button type="button" class="ledger-mini-button danger" title="删除大类（连带子分类）" on:click={() => void removeCategory(item.parent)}>
                  <Trash2 size={14} />
                </button>
              </span>
            </header>
            <div class="ledger-cat-children">
              {#each item.children as child (child.id)}
                {@const color = child.color || parentColor}
                <button type="button" class="ledger-cat-chip" title="编辑这个子分类" on:click={() => beginEdit(child)}>
                  <span class="ledger-chip-icon" style="--cat: {color}; background: {softColor(color)}">
                    <svelte:component this={ledgerIcon(child.icon, "Package")} size={14} />
                  </span>
                  {child.name}
                </button>
              {/each}
              <button type="button" class="ledger-cat-chip add" on:click={() => beginAdd(item.parent.id)}>
                <Plus size={14} />添加
              </button>
            </div>
          </section>
        {:else}
          <div class="ledger-day-empty">这一侧还没有分类，点下面的「添加大类」建一个。</div>
        {/each}
      </div>

      <footer class="ledger-sheet-foot">
        <span class="ledger-foot-hint">两级封顶：大类 → 子分类</span>
        <button type="button" class="settings-button primary" on:click={() => beginAdd("")}>
          <Plus size={15} />添加大类
        </button>
      </footer>
    {/if}
  </div>
</div>

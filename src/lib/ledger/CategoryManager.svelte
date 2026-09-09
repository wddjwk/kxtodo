<script lang="ts">
  /**
   * 分类管理：支出/收入两套两级分类的增删改（图标 + 颜色 + 挂哪个大类）。
   * 表单是面板内的子层，不做弹窗套弹窗；删除直接生效（core 会拦「大类连带子分类」的说明在确认文案里）。
   */
  import { createEventDispatcher } from "svelte";
  import { Pencil, Plus, Trash2, X } from "@lucide/svelte";
  import type { LedgerBook, LedgerSide } from "../types";
  import { categoryTree, categoryColor } from "../ledger";
  import { LEDGER_ICON_CHOICES, ledgerIcon, SIDE_LABEL } from "../ledgerIcons";
  import { addLedgerCategory, updateLedgerCategory, deleteLedgerCategory } from "../actions";

  export let book: LedgerBook;
  export let side: LedgerSide = "expense";
  export let onClose: () => void = () => {};

  const dispatch = createEventDispatcher<{ close: void }>();

  const COLORS = [
    "#f0862c",
    "#4a90d9",
    "#2f8f6b",
    "#9b59b6",
    "#e67e9c",
    "#e74c3c",
    "#16a085",
    "#d35400",
    "#8e6e53",
    "#7f8c8d"
  ];

  let form: {
    id: string | null;
    name: string;
    icon: string;
    color: string;
    parentId: string;
  } | null = null;

  $: tree = categoryTree(book, side);

  function openAdd(parentId: string): void {
    form = { id: null, name: "", icon: "", color: "", parentId };
  }

  function openEdit(id: string): void {
    const category = book.categories.find((item) => item.id === id);
    if (!category) return;
    form = {
      id: category.id,
      name: category.name,
      icon: category.icon,
      color: category.color,
      parentId: category.parentId ?? ""
    };
  }

  async function submit(): Promise<void> {
    if (!form || form.name.trim() === "") return;
    const payload = {
      name: form.name.trim(),
      icon: form.icon,
      color: form.color,
      parentId: form.parentId || undefined
    };
    if (form.id) {
      await updateLedgerCategory(form.id, payload);
    } else {
      await addLedgerCategory({ ...payload, side });
    }
    form = null;
  }

  async function remove(id: string): Promise<void> {
    await deleteLedgerCategory(id);
    if (form?.id === id) form = null;
  }

  function switchSide(next: LedgerSide): void {
    side = next;
    form = null;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (form) form = null;
      else onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="ledger-editor-backdrop" on:click={() => onClose()}></div>
<div class="ledger-manager" role="dialog" aria-label="分类管理">
  <header class="ledger-editor-head">
    <span class="ledger-editor-tabs">
      {#each ["expense", "income"] as value (value)}
        <button
          type="button"
          class:active={side === value}
          on:click={() => switchSide(value as LedgerSide)}
        >
          {SIDE_LABEL[value as LedgerSide]}
        </button>
      {/each}
    </span>
    <button type="button" class="ledger-editor-close" on:click={() => onClose()}>
      <X size={18} />
    </button>
  </header>

  {#if form}
    {@const f = form}
    <div class="ledger-manager-form">
      <input type="text" maxlength="20" placeholder="分类名称" bind:value={f.name} />
      <label class="ledger-manager-field">
        <span>挂在大类下</span>
        <select bind:value={f.parentId}>
          <option value="">（作为大类）</option>
          {#each tree as item (item.parent.id)}
            {#if item.parent.id !== f.id}
              <option value={item.parent.id}>{item.parent.name}</option>
            {/if}
          {/each}
        </select>
      </label>
      <div class="ledger-manager-swatches">
        {#each COLORS as color (color)}
          <button
            type="button"
            class="ledger-swatch"
            class:active={f.color === color}
            style="background:{color}"
            on:click={() => (f.color = f.color === color ? "" : color)}
            aria-label="颜色 {color}"
          ></button>
        {/each}
      </div>
      <div class="ledger-manager-icons">
        {#each LEDGER_ICON_CHOICES as name (name)}
          {@const icon = ledgerIcon(name, name)}
          <button
            type="button"
            class:active={f.icon === name}
            on:click={() => (f.icon = f.icon === name ? "" : name)}
            title={name}
          >
            <svelte:component this={icon} size={16} />
          </button>
        {/each}
      </div>
      <footer class="ledger-editor-foot">
        <button type="button" class="settings-button" on:click={() => (form = null)}>取消</button>
        <button type="button" class="settings-button primary" on:click={submit}>保存</button>
      </footer>
    </div>
  {:else}
    <div class="ledger-manager-list">
      {#each tree as item (item.parent.id)}
        {@const parentIcon = ledgerIcon(item.parent.icon, "Package")}
        <div class="ledger-manager-row ledger-manager-parent">
          <span class="ledger-row-icon" style="background:{categoryColor(book, item.parent)}">
            <svelte:component this={parentIcon} size={16} />
          </span>
          <strong>{item.parent.name}</strong>
          <em>{item.children.length} 个子分类</em>
          <span class="ledger-manager-row-actions">
            <button type="button" on:click={() => openEdit(item.parent.id)} title="编辑">
              <Pencil size={14} />
            </button>
            <button type="button" class="danger" on:click={() => remove(item.parent.id)} title="删除">
              <Trash2 size={14} />
            </button>
          </span>
        </div>
        {#each item.children as child (child.id)}
          {@const childIcon = ledgerIcon(child.icon, "Package")}
          <div class="ledger-manager-row ledger-manager-child">
            <span class="ledger-row-icon" style="background:{categoryColor(book, child)}">
              <svelte:component this={childIcon} size={14} />
            </span>
            <strong>{child.name}</strong>
            <span class="ledger-manager-row-actions">
              <button type="button" on:click={() => openEdit(child.id)} title="编辑">
                <Pencil size={14} />
              </button>
              <button type="button" class="danger" on:click={() => remove(child.id)} title="删除">
                <Trash2 size={14} />
              </button>
            </span>
          </div>
        {/each}
      {:else}
        <p class="ledger-day-empty">这一侧还没有分类</p>
      {/each}
    </div>
    <footer class="ledger-editor-foot">
      <button type="button" class="settings-button primary" on:click={() => openAdd("")}>
        <Plus size={15} /> 新增分类
      </button>
    </footer>
  {/if}
</div>

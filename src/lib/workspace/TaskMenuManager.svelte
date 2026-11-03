<script lang="ts">
  /**
   * 任务右键菜单的「管理菜单项」面板（v0.9.2）：拖动排序 + 逐项显隐。
   *
   * 顺序与隐藏项落在 `settings.appearance.taskMenu`（本机偏好，不进同步共享子集），
   * 每次改动即落盘——管理面板里没有「草稿/确认」两态，所见即所得，少一次「改了没确认
   * 就关掉全丢」的事故面。id 白名单与默认顺序在 `taskMenuConfig.ts`（唯一一份）。
   *
   * 拖动是 pointer 一套（桌面与触屏同一条路）：手柄上 `touch-action: none`，
   * 移动时按各行中点实时换位（与侧栏固定区拖动同一种「让位即预览」的观感）。
   */
  import { Eye, EyeOff, GripVertical, RotateCcw, X } from "@lucide/svelte";
  import { appSettings, showToast } from "../stores";
  import { setConfig } from "../actions";
  import {
    TASK_MENU_DEFAULT_ORDER,
    TASK_MENU_ITEMS,
    isTaskMenuSeparator,
    normalizeTaskMenuConfig,
    taskMenuItemLabel,
    type TaskMenuItemId
  } from "../taskMenuConfig";
  import { createBackGuard } from "../platform";
  import { suppressGhostClick } from "../ghostClick";

  export let onClose: () => void = () => {};

  const initial = normalizeTaskMenuConfig($appSettings.appearance.taskMenu);
  let order: TaskMenuItemId[] = initial.order;
  let hidden: TaskMenuItemId[] = initial.hidden;

  let dragId: TaskMenuItemId | null = null;
  let rowEls: Partial<Record<TaskMenuItemId, HTMLElement>> = {};

  const backGuard = createBackGuard();
  $: backGuard(true, () => onClose());

  function commit(): void {
    void setConfig("appearance.taskMenu", { order, hidden });
  }

  function toggleHidden(id: TaskMenuItemId): void {
    hidden = hidden.includes(id) ? hidden.filter((item) => item !== id) : [...hidden, id];
    commit();
  }

  /** 指针落在哪一行的上半区之前 = 该行的位置（落到最后一名之后 = 尾部） */
  function rowAt(clientY: number): TaskMenuItemId | null {
    for (const id of order) {
      const el = rowEls[id];
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) return id;
    }
    return order[order.length - 1] ?? null;
  }

  function handleDragStart(event: PointerEvent, id: TaskMenuItemId): void {
    if (event.button !== 0) return;
    event.preventDefault();
    dragId = id;
    const onMove = (move: PointerEvent): void => {
      if (!dragId) return;
      const target = rowAt(move.clientY);
      if (target === null || target === dragId) return;
      const next = order.filter((item) => item !== dragId);
      const index = next.indexOf(target);
      next.splice(index < 0 ? next.length : index, 0, dragId);
      order = next;
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      if (dragId) commit();
      dragId = null;
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  }

  function resetDefault(): void {
    order = [...TASK_MENU_DEFAULT_ORDER];
    hidden = TASK_MENU_ITEMS.filter((item) => item.defaultHidden).map((item) => item.id);
    commit();
    showToast("已恢复默认顺序与显隐");
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
<div class="editor-overlay" on:pointerdown={handleBackdropPointerDown}>
  <div
    class="editor-dialog calendar-sheet task-menu-manager"
    role="dialog"
    aria-label="管理菜单项"
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="calendar-sheet-head">
      <span class="calendar-sheet-title">管理菜单项</span>
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="editor-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
        <X size={17} />
      </button>
    </header>

    <div class="calendar-sheet-body task-menu-manager-body">
      <p class="calendar-sheet-hint">拖动手柄排序；眼睛控制该项是否出现在任务右键菜单里。</p>
      {#each order as id (id)}
        <div
          class="task-menu-manager-row"
          class:hidden-row={hidden.includes(id)}
          class:dragging={dragId === id}
          bind:this={rowEls[id]}
        >
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="task-menu-manager-grip"
            title="拖动排序"
            role="presentation"
            on:pointerdown={(event) => handleDragStart(event, id)}
          ><GripVertical size={14} /></span>
          <span class="task-menu-manager-label">
            {#if isTaskMenuSeparator(id)}分隔线{:else}{taskMenuItemLabel(id)}{/if}
          </span>
          <button
            type="button"
            class="editor-icon-button"
            title={hidden.includes(id) ? "在菜单中显示" : "从菜单中隐藏"}
            aria-label={hidden.includes(id) ? `显示${taskMenuItemLabel(id)}` : `隐藏${taskMenuItemLabel(id)}`}
            on:click={() => toggleHidden(id)}
          >
            {#if hidden.includes(id)}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
          </button>
        </div>
      {/each}
    </div>

    <footer class="calendar-sheet-foot">
      <button type="button" class="settings-button" on:click={resetDefault}>
        <RotateCcw size={15} />恢复默认
      </button>
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="settings-button primary" on:click={onClose}>完成</button>
    </footer>
  </div>
</div>

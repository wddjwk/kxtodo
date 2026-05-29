<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { CalendarDays, Check, Pencil, Star, Sun } from "@lucide/svelte";
  import { firstMarkdownLine, renderMarkdown } from "./markdown";
  import type { TodoTask, TodoTaskPatch } from "./types";

  export let task: TodoTask;
  export let selected = false;
  export let accent = "#b64a30";
  export let density: "comfortable" | "compact" = "comfortable";
  export let onSelect: (id: string) => void;
  export let onUpdate: (id: string, patch: TodoTaskPatch) => void;

  let editing = false;
  let showContextMenu = false;
  let showDateInput = false;
  let menuX = 0;
  let menuY = 0;
  let textarea: HTMLTextAreaElement;

  $: isMyDay = task.dueDate === "今天" || task.dueDate === todayIso();
  $: titleHtml = renderMarkdown(firstMarkdownLine(task.markdown));
  $: fullHtml = renderMarkdown(task.markdown);

  function handleWindowClick(): void {
    showContextMenu = false;
    if (editing) {
      editing = false;
    }
  }

  window.addEventListener("click", handleWindowClick);
  onDestroy(() => window.removeEventListener("click", handleWindowClick));

  async function startEditing(): Promise<void> {
    showContextMenu = false;
    editing = true;
    onUpdate(task.id, { expanded: true });
    await tick();
    resizeEditor();
    textarea?.focus();
  }

  async function toggleEditing(): Promise<void> {
    if (editing) {
      editing = false;
    } else {
      await startEditing();
    }
  }

  function updateMarkdown(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLTextAreaElement) {
      onUpdate(task.id, { markdown: target.value });
      resizeEditor();
    }
  }

  function resizeEditor(): void {
    if (!textarea) {
      return;
    }
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 420)}px`;
  }

  function toggleExpanded(): void {
    if (editing) {
      return;
    }
    showContextMenu = false;
    onSelect(task.id);
  }

  function openMenu(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    showContextMenu = true;
    showDateInput = false;
    menuX = event.clientX;
    menuY = event.clientY;
  }

  function toggleMyDay(): void {
    onUpdate(task.id, { dueDate: isMyDay ? null : todayIso() });
    showContextMenu = false;
  }

  function toggleFavorite(): void {
    onUpdate(task.id, { important: !task.important });
    showContextMenu = false;
  }

  function updateDate(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      onUpdate(task.id, { dueDate: target.value || null });
      showContextMenu = false;
    }
  }

  function todayIso(): string {
    const date = new Date();
    const offset = date.getTimezoneOffset();
    const local = new Date(date.getTime() - offset * 60_000);
    return local.toISOString().slice(0, 10);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<article
  class:completed={task.completed}
  class:selected
  class:expanded={task.expanded}
  class:compact={density === "compact"}
  class:editing
  class="task-card"
  style={`--accent: ${accent}`}
  on:click|stopPropagation
  on:contextmenu={openMenu}
  on:dblclick|stopPropagation={startEditing}
>
  <button
    class="task-check"
    type="button"
    aria-label="切换完成"
    on:click|stopPropagation={() => onUpdate(task.id, { completed: !task.completed })}
  >
    {#if task.completed}
      <Check size={14} strokeWidth={3.2} />
    {/if}
  </button>

  <div class="task-body">
    {#if editing}
      <textarea
        bind:this={textarea}
        class="markdown-editor"
        spellcheck="false"
        value={task.markdown}
        on:input={updateMarkdown}
        on:click|stopPropagation
      ></textarea>
    {:else}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="markdown-title-row markdown-body" on:click|stopPropagation={toggleExpanded}>
        {@html titleHtml}
      </div>
      {#if task.expanded}
        <div class="markdown-body markdown-content">
          {@html fullHtml}
        </div>
      {/if}
    {/if}
  </div>

  <button class="edit-button" type="button" title="编辑 Markdown" on:click|stopPropagation={toggleEditing}>
    <Pencil size={18} />
  </button>

  {#if showContextMenu}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="task-context-menu" style={`left: ${menuX}px; top: ${menuY}px`} on:click|stopPropagation>
      <button type="button" on:click={toggleMyDay}>
        <Sun size={16} /> {isMyDay ? "从我的一天中移除" : "添加到我的一天"}
      </button>
      <button type="button" on:click={() => (showDateInput = !showDateInput)}>
        <CalendarDays size={16} /> 添加日期
      </button>
      {#if showDateInput}
        <input type="date" value={task.dueDate ?? ""} on:change={updateDate} />
      {/if}
      <button type="button" on:click={toggleFavorite}>
        <Star size={16} /> {task.important ? "取消收藏" : "收藏"}
      </button>
      <button type="button" on:click={startEditing}>
        <Pencil size={16} /> 编辑
      </button>
    </div>
  {/if}
</article>

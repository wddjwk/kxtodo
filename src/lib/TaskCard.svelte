<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { Check, Pencil } from "@lucide/svelte";
  import { firstMarkdownLine, renderMarkdown } from "./markdown";
  import type { Task } from "./types";

  export let task: Task;
  export let selected = false;
  export let linkOpenMode: "app" | "system" = "app";

  const dispatch = createEventDispatcher<{
    toggle: string;
    expand: string;
    edit: string;
    commit: { id: string; markdown: string };
    context: { id: string; x: number; y: number };
    openLink: string;
  }>();

  let draft = "";
  let editingTaskId = "";
  let editorEl: HTMLTextAreaElement;

  $: title = firstMarkdownLine(task.markdown);
  $: fullHtml = renderMarkdown(task.markdown);
  $: if (task.editing && editingTaskId !== task.id) {
    draft = task.markdown;
    editingTaskId = task.id;
  }
  $: if (!task.editing && editingTaskId === task.id) {
    editingTaskId = "";
  }

  async function startEdit(): Promise<void> {
    draft = task.markdown;
    dispatch("edit", task.id);
    await tick();
    resizeEditor();
    editorEl?.focus();
  }

  function commitEdit(): void {
    if (!task.editing) {
      return;
    }
    dispatch("commit", { id: task.id, markdown: draft });
  }

  function toggleEdit(): void {
    if (task.editing) {
      commitEdit();
    } else {
      void startEdit();
    }
  }

  function toggleExpand(event: MouseEvent): void {
    if (task.editing) {
      return;
    }
    event.stopPropagation();
    dispatch("expand", task.id);
  }

  function handleBodyDblClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    if (target?.closest("button, a, input, textarea")) {
      return;
    }
    void startEdit();
  }

  function openContext(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    dispatch("context", { id: task.id, x: event.clientX, y: event.clientY });
  }

  function handleMarkdownClick(event: MouseEvent): void {
    event.stopPropagation();
    const target = event.target as HTMLElement | null;
    const link = target?.closest("a[href]");
    if (!(link instanceof HTMLAnchorElement)) {
      return;
    }
    if (linkOpenMode === "system") {
      event.preventDefault();
      dispatch("openLink", link.href);
    }
  }

  function resizeEditor(): void {
    if (!editorEl) {
      return;
    }
    editorEl.style.height = "auto";
    editorEl.style.height = `${Math.min(editorEl.scrollHeight, 420)}px`;
  }
</script>

<article
  class:completed={task.completed}
  class:compact={!task.expanded && !task.editing}
  class:editing={task.editing}
  class:selected
  class="task-card"
  on:contextmenu={openContext}
>
  <div class="task-title-grid">
    <button class="task-check" type="button" aria-label="切换完成" on:click|stopPropagation={() => dispatch("toggle", task.id)}>
      {#if task.completed}
        <Check size={14} strokeWidth={3.2} />
      {/if}
    </button>

    <section class="task-body" on:dblclick={handleBodyDblClick}>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="markdown-title-row" on:click={toggleExpand}>{title}</div>
    </section>

    {#if task.dueDate}
      <span class="task-due-date">{task.dueDate}</span>
    {:else}
      <span class="task-due-spacer" aria-hidden="true"></span>
    {/if}

    <button class="edit-button" type="button" title="编辑 Markdown" on:mousedown|preventDefault on:click|stopPropagation={toggleEdit}>
      <Pencil size={18} />
    </button>
  </div>

  {#if task.editing}
    <section class="task-detail task-editor-detail" on:dblclick={handleBodyDblClick}>
      <textarea
        bind:this={editorEl}
        bind:value={draft}
        class="markdown-editor"
        spellcheck="false"
        on:input={resizeEditor}
        on:blur={commitEdit}
      ></textarea>
    </section>
  {:else if task.expanded}
    <div class="markdown-body markdown-content task-detail" on:click={handleMarkdownClick} on:dblclick={handleBodyDblClick}>
      {@html fullHtml}
    </div>
  {/if}
</article>

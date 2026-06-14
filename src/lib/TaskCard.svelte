<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { Check, ChevronUp, ImagePlus, PenLine } from "@lucide/svelte";
  import { collapsedMarkdownLine, renderInlineMarkdown, renderMarkdown } from "./markdown";
  import { mdImageCache, resolveMarkdownImages, primeMdImageCache } from "./images";
  import { isTauriRuntime, pickImageFile, saveMdImage, mdImageUrl } from "./backend";
  import { showToast } from "./stores";
  import DatePicker from "./DatePicker.svelte";
  import type { Task } from "./types";

  export let task: Task;
  export let nodeId = "";
  export let selected = false;
  export let linkOpenMode: "app" | "system" = "app";

  const dispatch = createEventDispatcher<{
    toggle: string;
    expand: string;
    edit: string;
    commit: { id: string; markdown: string };
    context: { id: string; x: number; y: number };
    openLink: string;
    setDate: { id: string; date: string };
  }>();

  let draft = "";
  let editingTaskId = "";
  let editorEl: HTMLTextAreaElement;
  let showPicker = false;

  $: resolvedMd = resolveMarkdownImages(task.markdown, nodeId, $mdImageCache);
  $: collapsedHtml = renderInlineMarkdown(collapsedMarkdownLine(task.markdown));
  $: fullHtml = renderMarkdown(resolvedMd);
  $: formattedDate = task.dueDate ? formatDate(task.dueDate) : "";
  $: if (task.editing && editingTaskId !== task.id) {
    draft = task.markdown;
    editingTaskId = task.id;
  }
  $: if (!task.editing && editingTaskId === task.id) {
    editingTaskId = "";
  }

  async function insertImage(): Promise<void> {
    if (!isTauriRuntime || !nodeId) return;
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      const filename = await saveMdImage(srcPath, nodeId);
      const url = await mdImageUrl(nodeId, filename);
      primeMdImageCache(nodeId, filename, url);
      draft += `\n![](${filename})`;
      dispatch("commit", { id: task.id, markdown: draft });
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    }
  }

  function formatDate(dateStr: string): string {
    const parts = dateStr.slice(0, 10).split("-").map(Number);
    return `${parts[1]}月${parts[2]}日`;
  }

  function toggleDatePicker(): void {
    showPicker = !showPicker;
  }

  function handlePick(date: string): void {
    showPicker = false;
    dispatch("setDate", { id: task.id, date });
  }

  function handleClearDate(): void {
    showPicker = false;
    dispatch("setDate", { id: task.id, date: "" });
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

  function handleEditZoneMouseDown(event: MouseEvent): void {
    if (event.button !== 0) {
      return;
    }
    const card = event.currentTarget as HTMLElement;
    const rect = card.getBoundingClientRect();
    const inEditZone = event.clientX >= rect.right - 58 && event.clientY <= rect.top + 58;
    if (!inEditZone) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    toggleEdit();
  }

  function toggleExpand(event?: MouseEvent): void {
    if (task.editing) {
      return;
    }
    event?.stopPropagation();
    dispatch("expand", task.id);
  }

  function handleBodyDblClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    if (task.editing || target?.closest("button, input, textarea")) {
      return;
    }
    event.preventDefault();
    window.getSelection()?.removeAllRanges();
    toggleExpand(event);
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

<svelte:window on:click={() => (showPicker = false)} />

<article
  class:completed={task.completed}
  class:compact={!task.expanded && !task.editing}
  class:expanded={task.expanded && !task.editing}
  class:editing={task.editing}
  class:selected
  class="task-card"
  on:mousedown|capture={handleEditZoneMouseDown}
  on:contextmenu={openContext}
>
  <div class="task-title-grid">
    <button class="task-check" type="button" aria-label="切换完成" on:click|stopPropagation={() => dispatch("toggle", task.id)}>
      {#if task.completed}
        <Check size={14} strokeWidth={3.2} />
      {/if}
    </button>

    <section class="task-body" on:dblclick={handleBodyDblClick}>
      {#if task.editing}
        <textarea
          bind:this={editorEl}
          bind:value={draft}
          class="markdown-editor"
          spellcheck="false"
          on:input={resizeEditor}
          on:blur={commitEdit}
        ></textarea>
      {:else if task.expanded}
        <div class="markdown-body markdown-content" on:click={handleMarkdownClick} on:dblclick={handleBodyDblClick}>
          {@html fullHtml}
        </div>
      {:else}
        <div class="markdown-body markdown-title-row">
          {@html collapsedHtml}
        </div>
      {/if}
    </section>

    {#if !task.expanded && !task.editing && task.dueDate}
      <div class="task-due-wrap">
        <button class="task-due-date" type="button" on:click|stopPropagation={toggleDatePicker}>{formattedDate}</button>
        {#if showPicker}
          <div class="task-date-popover">
            <DatePicker value={task.dueDate?.slice(0, 10) ?? ""} on:select={(e) => handlePick(e.detail)} on:clear={handleClearDate} />
          </div>
        {/if}
      </div>
    {:else}
      <span class="task-due-spacer" aria-hidden="true"></span>
    {/if}

    {#if task.editing && isTauriRuntime && nodeId}
      <button class="insert-image-button" type="button" title="插入图片" on:mousedown|preventDefault|stopPropagation={insertImage} on:click|preventDefault|stopPropagation>
        <ImagePlus size={18} />
      </button>
    {/if}

    <button class="edit-button" type="button" title="编辑 Markdown" on:mousedown|preventDefault|stopPropagation={toggleEdit} on:click|preventDefault|stopPropagation>
      <PenLine size={18} />
    </button>

    {#if task.expanded && !task.editing}
      <button class="collapse-button" type="button" title="收起卡片" on:click|stopPropagation={toggleExpand}>
        <ChevronUp size={18} />
      </button>
    {/if}
  </div>

</article>

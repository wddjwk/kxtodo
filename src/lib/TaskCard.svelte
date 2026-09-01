<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { Check, ChevronUp, PenLine, Plus, X } from "@lucide/svelte";
  import { collapsedMarkdownLine, hasMultipleMarkdownLines, renderInlineMarkdown, renderMarkdown } from "./markdown";
  import { mdImageCache, resolveMarkdownImages } from "./images";
  import { appSettings } from "./stores";
  import { uiScaleValue } from "./styles";
  import DatePicker from "./DatePicker.svelte";
  import type { Task } from "./types";

  export let task: Task;
  export let nodeId = "";
  export let selected = false;

  const dispatch = createEventDispatcher<{
    toggle: string;
    expand: string;
    edit: string;
    context: { id: string; x: number; y: number };
    openLink: string;
    setDate: { id: string; date: string };
    removeTag: { id: string; tagId: string };
    editTag: { id: string; tagId: string; text: string };
    removeEmoji: { id: string; index: number };
    pickEmoji: { id: string; index: number };
  }>();

  let showPicker = false;
  let editingTagId = "";
  let editingTagText = "";
  let tagEditEl: HTMLInputElement;
  let dueButtonEl: HTMLButtonElement;
  let datePopoverStyle = "";

  $: resolvedMd = resolveMarkdownImages(task.markdown, nodeId, $mdImageCache);
  $: collapsedHtml = renderInlineMarkdown(collapsedMarkdownLine(task.markdown));
  $: fullHtml = renderMarkdown(resolvedMd);
  $: formattedDate = task.dueDate ? formatDate(task.dueDate) : "";
  $: canExpand = hasMultipleMarkdownLines(task.markdown);
  $: isExpanded = task.expanded && canExpand;

  function formatDate(dateStr: string): string {
    const parts = dateStr.slice(0, 10).split("-").map(Number);
    return `${parts[1]}月${parts[2]}日`;
  }

  /** 日期弹窗用 fixed 浮层：absolute 会被卡片/任务列表的 overflow 裁剪。
   * fixed 在 transform 缩放的 app-shell 内相对其左上角定位，按钮的屏幕坐标
   * 除以 scale 换算回逻辑坐标；贴近视口底部时向上翻转。 */
  function toggleDatePicker(): void {
    showPicker = !showPicker;
    if (!showPicker || !dueButtonEl) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const rect = dueButtonEl.getBoundingClientRect();
    const estVisualHeight = 360;
    const openBelow = rect.bottom + estVisualHeight <= window.innerHeight;
    const anchorEdge = openBelow ? rect.bottom + 6 : rect.top - estVisualHeight - 6;
    const topLogical = anchorEdge / scale;
    const rightLogical = (window.innerWidth - rect.right) / scale;
    datePopoverStyle = `top: ${topLogical}px; right: ${rightLogical}px;`;
  }

  function handlePick(date: string): void {
    showPicker = false;
    dispatch("setDate", { id: task.id, date });
  }

  function handleClearDate(): void {
    showPicker = false;
    dispatch("setDate", { id: task.id, date: "" });
  }

  function toggleExpand(): void {
    if (!canExpand) return;
    dispatch("expand", task.id);
  }

  function openEditor(): void {
    dispatch("edit", task.id);
  }

  /**
   * 双击展开/收起。第二次 mousedown（detail >= 2）preventDefault 阻止选词，
   * 保证双击只触发展开、不留下文本选区；单击不受影响，仍可正常选中复制。
   */
  function handleCardMouseDown(event: MouseEvent): void {
    if (event.detail < 2) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("button, input, textarea, a")) return;
    event.preventDefault();
  }

  function handleCardDblClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    if (target?.closest("button, input, textarea, a")) return;
    if (!canExpand) return;
    event.preventDefault();
    window.getSelection()?.removeAllRanges();
    toggleExpand();
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
    event.preventDefault();
    dispatch("openLink", link.href);
  }

  function startTagEdit(tagId: string, currentText: string): void {
    editingTagId = tagId;
    editingTagText = currentText || "";
    void Promise.resolve().then(() => tagEditEl?.focus());
  }

  function commitTagEdit(): void {
    if (editingTagId) {
      dispatch("editTag", { id: task.id, tagId: editingTagId, text: editingTagText.trim() });
      editingTagId = "";
    }
  }

  function removeTag(tagId: string): void {
    dispatch("removeTag", { id: task.id, tagId });
  }
</script>

<svelte:window on:click={() => (showPicker = false)} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<article
  class:completed={task.completed}
  class:compact={!isExpanded}
  class:expanded={isExpanded}
  class:multiline={canExpand}
  class:selected
  class="task-card"
  on:mousedown={handleCardMouseDown}
  on:dblclick={handleCardDblClick}
  on:contextmenu={openContext}
>
  <div class="task-title-grid">
    <button class="task-check" type="button" aria-label="切换完成" on:click|stopPropagation={() => dispatch("toggle", task.id)}>
      {#if canExpand}
        <Plus size={14} strokeWidth={3.1} />
      {:else if task.completed}
        <Check size={14} strokeWidth={3.2} />
      {/if}
    </button>

    <section class="task-body">
      {#if isExpanded}
        <div class="markdown-body markdown-content" on:click={handleMarkdownClick}>
          {@html fullHtml}
        </div>
      {:else}
        <div class="markdown-body markdown-title-row" on:click={handleMarkdownClick}>
          {@html collapsedHtml}
        </div>
      {/if}
    </section>

    <div class="task-tags">
      {#each task.emojis as emoji, index (`${task.id}-emoji-${index}`)}
        <span
          class="task-emoji-badge"
          title="点击更换表情"
          on:click|stopPropagation={() => dispatch("pickEmoji", { id: task.id, index })}
        >
          {emoji}
          <button class="tag-delete" type="button" aria-label="移除表情" on:click|stopPropagation={() => dispatch("removeEmoji", { id: task.id, index })}>
            <X size={10} strokeWidth={3} />
          </button>
        </span>
      {/each}
      {#each task.tags as tag (tag.id)}
        {#if editingTagId === tag.id}
          <input
            bind:this={tagEditEl}
            bind:value={editingTagText}
            class="tag-edit-input"
            maxlength="20"
            on:blur={commitTagEdit}
            on:click|stopPropagation
            on:keydown|stopPropagation={(e) => { if (e.key === "Enter") commitTagEdit(); }}
          />
        {:else}
          <span
            class={`task-tag tag-${tag.color}`}
            title={tag.text || "点击编辑标签"}
            on:click|stopPropagation={() => startTagEdit(tag.id, tag.text || "")}
          >
            {#if tag.text}{tag.text}{/if}
            <button class="tag-delete" type="button" aria-label="删除标签" on:click|stopPropagation={() => removeTag(tag.id)}>
              <X size={10} strokeWidth={3} />
            </button>
          </span>
        {/if}
      {/each}
    </div>

    {#if !isExpanded && task.dueDate}
      <div class="task-due-wrap">
        <button bind:this={dueButtonEl} class="task-due-date" type="button" on:click|stopPropagation={toggleDatePicker}>{formattedDate}</button>
        {#if showPicker}
          <div class="task-date-popover" style={datePopoverStyle}>
            <DatePicker value={task.dueDate?.slice(0, 10) ?? ""} on:select={(e) => handlePick(e.detail)} on:clear={handleClearDate} />
          </div>
        {/if}
      </div>
    {:else}
      <span class="task-due-spacer" aria-hidden="true"></span>
    {/if}

    <button class="edit-button" type="button" title="编辑 Markdown" on:click|stopPropagation={openEditor}>
      <PenLine size={18} />
    </button>

    {#if isExpanded}
      <button class="collapse-button" type="button" title="收起卡片" on:click|stopPropagation={toggleExpand}>
        <ChevronUp size={18} />
      </button>
    {/if}
  </div>
</article>

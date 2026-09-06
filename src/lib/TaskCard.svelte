<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { Check, ChevronUp, PenLine, Plus, X } from "@lucide/svelte";
  import { collapsedMarkdownLine, hasMultipleMarkdownLines, renderInlineMarkdown, renderMarkdown } from "./markdown";
  import { mdImageCache, resolveMarkdownImages } from "./images";
  import { appSettings } from "./stores";
  import { isMobile as isMobileStore } from "./platform";
  import { uiScaleValue } from "./styles";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import DatePicker from "./DatePicker.svelte";
  import type { Task } from "./types";

  export let task: Task;
  export let nodeId = "";
  export let selected = false;

  const dispatch = createEventDispatcher<{
    toggle: string;
    expand: { id: string; expanded: boolean };
    edit: string;
    context: { id: string; x: number; y: number };
    openLink: { href: string; title: string };
    setDate: { id: string; date: string };
    removeTag: { id: string; tagId: string };
    editTag: { id: string; tagId: string; text: string };
    removeEmoji: { id: string; index: number };
    pickEmoji: { id: string; index: number };
  }>();

  /** 两击判定窗口：移动端单击的动作要等到这个窗口过去才执行 */
  const DOUBLE_TAP_MS = 300;

  let showPicker = false;
  let editingTagId = "";
  let editingTagText = "";
  let tagEditEl: HTMLInputElement;
  let dueButtonEl: HTMLButtonElement;
  let datePopoverStyle = "";
  let tapTimer: number | undefined;
  let lastTapAt = 0;
  /** 折叠态标题是否显示不全（单行但很长）——是的话这张卡片也可以展开 */
  let titleOverflow = false;
  // isMobile 是 store：当布尔直接用会永远为真，桌面端就会误走移动端手势
  $: mobile = $isMobileStore;

  $: resolvedMd = resolveMarkdownImages(task.markdown, nodeId, $mdImageCache);
  $: collapsedHtml = renderInlineMarkdown(collapsedMarkdownLine(task.markdown));
  $: fullHtml = renderMarkdown(resolvedMd);
  $: formattedDate = task.dueDate ? formatDate(task.dueDate) : "";
  $: canExpand = hasMultipleMarkdownLines(task.markdown) || titleOverflow;
  $: isExpanded = task.expanded && canExpand;

  onDestroy(() => {
    if (tapTimer !== undefined) window.clearTimeout(tapTimer);
  });

  /**
   * 量折叠态标题有没有被截断：桌面是单行 nowrap（比宽度），移动端折到两行封顶（比高度）。
   * 截断了就说明「这一行显示不完整」，展开即把它显示完整——单行内容也要能展开。
   * 参数是渲染后的 HTML，内容一变就重量；窗口尺寸变了也重量。
   */
  function measureTitle(
    node: HTMLElement,
    html: string
  ): { update: (next: string) => void; destroy: () => void } {
    const check = (): void => {
      titleOverflow =
        node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1;
    };
    check();
    window.addEventListener("resize", check);
    let last = html;
    return {
      update(next: string): void {
        // 参数就是渲染后的 HTML：变了说明内容变了，重新量一次
        if (next === last) return;
        last = next;
        check();
      },
      destroy(): void {
        window.removeEventListener("resize", check);
      }
    };
  }

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
    dispatch("expand", { id: task.id, expanded: !isExpanded });
  }

  function openEditor(): void {
    dispatch("edit", task.id);
  }

  function isInteractiveTarget(event: MouseEvent): boolean {
    const target = event.target as HTMLElement | null;
    return Boolean(target?.closest("button, input, textarea, a"));
  }

  /**
   * 移动端手势：单击展开/折叠，双击进编辑器。
   * 单击的动作延后到双击窗口结束才执行——立刻执行的话双击会先折叠再打开编辑器。
   * 两击判定同时看 `event.detail` 与时间间隔：WebView 合成 click 时 detail 不一定可靠。
   */
  function handleMobileTap(event: MouseEvent): void {
    if (isInteractiveTarget(event)) return;
    const now = Date.now();
    const doubled = event.detail >= 2 || now - lastTapAt < DOUBLE_TAP_MS;
    lastTapAt = now;
    if (tapTimer !== undefined) window.clearTimeout(tapTimer);
    tapTimer = undefined;
    // 长按出菜单时不许留下文本选区（菜单是长按的产物，不是选词的产物）
    event.preventDefault();
    window.getSelection()?.removeAllRanges();
    if (doubled) {
      openEditor();
      return;
    }
    tapTimer = window.setTimeout(() => {
      tapTimer = undefined;
      toggleExpand();
    }, DOUBLE_TAP_MS);
  }

  /**
   * 双击展开/收起（桌面）。第二次 mousedown（detail >= 2）preventDefault 阻止选词，
   * 保证双击只触发展开、不留下文本选区；单击不受影响，仍可正常选中复制。
   */
  function handleCardMouseDown(event: MouseEvent): void {
    if (event.detail < 2) return;
    if (isInteractiveTarget(event)) return;
    event.preventDefault();
  }

  function handleCardDblClick(event: MouseEvent): void {
    // 移动端的双击语义在 handleMobileTap（进编辑器），且单击的展开动作已被它取消；
    // 这里再跑桌面的「双击展开/收起」就会让双击既开编辑器又改变展开状态。
    if (mobile) return;
    if (isInteractiveTarget(event)) return;
    if (!canExpand) return;
    event.preventDefault();
    window.getSelection()?.removeAllRanges();
    toggleExpand();
  }

  function openContext(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    // 触摸长按已开过菜单时，Chromium 补发的原生 contextmenu 直接吞掉
    if (isLongPressSuppressed()) return;
    dispatch("context", { id: task.id, x: event.clientX, y: event.clientY });
  }

  /** 移动端触摸长按：以原始触点为锚打开任务菜单（桌面不受影响）。 */
  function handleLongPress(pos: { x: number; y: number }): void {
    dispatch("context", { id: task.id, x: pos.x, y: pos.y });
  }

  /** 长按抬手补发的 click 会冒泡到 app-shell 关掉刚开的菜单，抑制窗内吞掉。 */
  function handleCardClick(event: MouseEvent): void {
    if (isLongPressSuppressed()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (mobile) handleMobileTap(event);
  }

  function handleMarkdownClick(event: MouseEvent): void {
    // 内容区的点击不冒泡到卡片（否则移动端一次点击会被两套逻辑各处理一遍），
    // 但手势语义要在这里补上：展开态点内容区 = 折叠，双击内容区 = 进编辑器。
    event.stopPropagation();
    if (isLongPressSuppressed()) {
      event.preventDefault();
      return;
    }
    const target = event.target as HTMLElement | null;
    const link = target?.closest("a[href]");
    if (link instanceof HTMLAnchorElement) {
      event.preventDefault();
      dispatch("openLink", { href: link.href, title: (link.textContent ?? "").trim() });
      return;
    }
    if (mobile) handleMobileTap(event);
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
  use:longpress={handleLongPress}
  on:mousedown={handleCardMouseDown}
  on:click={handleCardClick}
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
        <div class="markdown-body markdown-title-row" use:measureTitle={collapsedHtml} on:click={handleMarkdownClick}>
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

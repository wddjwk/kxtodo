<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { Check, ChevronUp, PenLine, Plus, X } from "@lucide/svelte";
  import { collapsedMarkdownLine, hasMultipleMarkdownLines, renderInlineMarkdown } from "./markdown";
  import { taskToggleIndex, toggleMarkdownTask } from "./markdownTasks";
  import { tagChipStyle } from "./tagColors";
  import { dueHighlightOf, dueHighlightStyle, DEFAULT_DUE_COLORS } from "./dueHighlight";
  import { colorPreview, dueColorsWithPreview } from "./colorPreview";
  import { currentMinute } from "./currentTime";
  import { fullDayLabel } from "./diary";
  import { createDeferredMarkdown } from "./deferredMarkdown";
  import { preloadMarkdownImages } from "./images";
  import { appSettings } from "./stores";
  import { saveTaskMarkdown } from "./actions";
  import { isMobile as isMobileStore, touchOnly } from "./platform";
  import { uiScaleValue } from "./styles";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import { markdownWire } from "./markdownControls";
  import { observeResize } from "./measureBus";
  import TaskDateReminderPanel from "./TaskDateReminderPanel.svelte";
  import type { ReminderRule, Task } from "./types";

  export let task: Task;
  export let nodeId = "";
  export let selected = false;
  /** 分组类型：todo = 可勾选的待办卡片（默认）；card = 一般卡片（无勾选框，内容占满） */
  export let cardStyle: "todo" | "card" = "todo";

  const dispatch = createEventDispatcher<{
    toggle: string;
    expand: { id: string; expanded: boolean };
    /** 量出来的「可展开性」报给列表：「展开全部/收起全部」要知道
     *  单行超长（要折行）的卡片也算可展开，光看 markdown 行数是漏的 */
    measure: { id: string; canExpand: boolean };
    edit: string;
    context: { id: string; x: number; y: number };
    openLink: { href: string; title: string };
    /** 「日期与提醒」面板保存/清除：日期、时刻、提醒一次交上去（core 侧是一个事务） */
    setSchedule: { id: string; dueDate: string; dueTime: string; reminders: ReminderRule[] };
    removeTag: { id: string; tagId: string };
    editTag: { id: string; tagId: string; text: string };
    removeEmoji: { id: string; index: number };
    pickEmoji: { id: string; index: number };
  }>();

  /** 两击判定窗口：移动端单击的动作要等到这个窗口过去才执行。
   * 这个值直接决定单击的「跟手感」——太大单击就发闷；太小双击会漏判。
   * 220ms 是实测折中：单击几乎无感延迟，正常双击（100~250ms 间隔）仍稳。 */
  const DOUBLE_TAP_MS = 220;

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
  /** 触屏上被点了一下、露出删除叉的标签/表情（桌面靠 hover，不用它） */
  let revealedTagId = "";
  let revealedEmojiIndex = -1;
  // isMobile 是 store：当布尔直接用会永远为真，桌面端就会误走移动端手势
  $: mobile = $isMobileStore;
  // 展开态只认存储值：canExpand 是量出来的易失值（列表增减导致滚动条出现/消失、
  // 宽度一变标题溢出判定就翻转），拿它门控渲染会出现「动了别的任务这张卡自己展开」。
  // canExpand 只留给手势/按钮当「有没有内容可展开」的判据。
  $: isExpanded = task.expanded === true;

  // 插图预热：渲染吃**原始 markdown**（图片是占位符，由 markdownWire 从缓存异步填 src），
  // 这里只负责在卡片挂载时把字节提前要过来——展开时图已经在缓存里，占位一挂上就填掉。
  $: preloadMarkdownImages(task.markdown, nodeId);
  $: collapsedHtml = renderInlineMarkdown(collapsedMarkdownLine(task.markdown));
  // **只在展开时渲染完整 markdown**：Svelte 的 `$:` 是急切求值，与模板消不消费无关，
  // 早先折叠态的卡片也白跑一遍完整渲染（12 步，含 DOMPurify 的完整 DOM 解析），
  // 而结果只有下面 `{#if isExpanded}` 那一支会用到。一屏 300 张折叠卡就是 300 次白渲染，
  // 而且每次列表变化都要重来。
  //
  // 展开那一刻的时机由 createDeferredMarkdown 调度：命中记忆化或短文本一步渲染到位；
  // 长文本先同步给「快速版」（结构文字齐全，只缺代码高亮与公式排版），完整装饰在
  // 浏览器画过一两帧后补上——点击立刻看到完整可读的内容，没有空白块阶段。
  let fullHtml = "";
  const fullRender = createDeferredMarkdown((html) => {
    fullHtml = html;
  });
  $: syncFullRender(isExpanded, task.markdown, nodeId);

  function syncFullRender(expanded: boolean, markdown: string, node: string): void {
    if (!expanded) {
      fullRender.cancel();
      if (fullHtml !== "") fullHtml = "";
      return;
    }
    fullRender.schedule(markdown, node);
  }
  // 日期展示：`9月8日 周二` / `9月8日 周二 18:30`（有时刻才带时刻）
  $: formattedDate = task.dueDate
    ? `${fullDayLabel(task.dueDate.slice(0, 10))}${task.dueTime ? ` ${task.dueTime}` : ""}`
    : "";
  // 临期高亮：配色按**本页**（这个节点）自己的四色走，没配过就用默认灰/红/黄/蓝。
  // 已完成的卡片不画——它有自己的一整套完成态样式。
  // `now` 用一分钟一跳的 store：「已过期」档要在跨过时刻的那一分钟自己翻色，
  // 只依赖任务与设置的话得等下一次无关重渲才换。
  // 取色预览（需求 9）：三点菜单里拖自己那一档的色块时，本页卡片立刻换色；
  // 菜单一关（没保存）预览就清掉，卡片回到落盘值。
  $: dueColors = dueColorsWithPreview($colorPreview, nodeId, $appSettings.appearance.dueColors[nodeId], DEFAULT_DUE_COLORS);
  $: dueHighlight =
    task.completed || !task.dueDate
      ? null
      : dueHighlightOf(task, $appSettings.features.dueHighlight, dueColors, $currentMinute);
  // 可展开 = 多行 ∪ 折叠态量出来显示不全 ∪ **当前就是展开的**。
  // 第三项治的是「折行卡片以展开态挂载」（编辑器保存后、展开全部后重挂载）：
  // 那时 measureTitle 根本不在树上（它只挂在折叠分支），titleOverflow 永远是 false，
  // 单行超长的卡片就被判成不可折叠——勾选框不画加号、双击也收不起来（toggleExpand
  // 被 canExpand 门控）。展开着的卡片天然「可以收起」，直接并进判据即可；收起瞬间
  // measureTitle 重新挂载、同步重量，判据随即回到量出来的真值。
  $: canExpand = hasMultipleMarkdownLines(task.markdown) || titleOverflow || isExpanded;
  // 把可展开性同步给列表：**延后一个微任务**再派发——首次检查发生在组件挂载期间，
  // 同步派发会让父组件在渲染途中改状态。
  let reportedExpand: boolean | null = null;
  $: if (canExpand !== reportedExpand) {
    reportedExpand = canExpand;
    const value = canExpand;
    void Promise.resolve().then(() => dispatch("measure", { id: task.id, canExpand: value }));
  }
  $: plain = cardStyle === "card";

  onDestroy(() => {
    if (tapTimer !== undefined) window.clearTimeout(tapTimer);
    fullRender.cancel();
  });

  /**
   * 量折叠态标题有没有显示不全。
   * - 桌面：单行 nowrap，比宽度；显示不全就加省略号并可展开。
   * - 移动端：折行显示、两行封顶。**两行放得下就不夹行、不加省略号、不算可展开**；
   *   只有两行放不下才夹成两行、在第二行末尾加省略号并变成可展开。
   *   所以要比的是**自然高度**与两行预算：夹行开着时 clientHeight 已被夹住，量不出需要几行，
   *   量之前临时摘掉 clamped 类、量完恢复（同步完成，中间不会重绘）。
   */
  function measureTitle(
    node: HTMLElement,
    html: string
  ): { update: (next: string) => void; destroy: () => void } {
    const check = (): void => {
      if (!mobile) {
        titleOverflow = node.scrollWidth > node.clientWidth + 1;
        return;
      }
      const wasClamped = node.classList.contains("clamped");
      if (wasClamped) node.classList.remove("clamped");
      const lineHeight = parseFloat(getComputedStyle(node).lineHeight) || 0;
      const natural = node.scrollHeight;
      if (wasClamped) node.classList.add("clamped");
      titleOverflow = lineHeight > 0 && natural > lineHeight * 2 + 1;
    };
    check();
    // 元素自身尺寸变化也要重量：移动端首屏卡片在 view-list 下是 display:none，
    // 挂载时量到的全是 0；点进内容页变可见时没有任何 window 事件，只有 ResizeObserver 能接到。
    // observer 与 window 监听都由 measureBus 单例托管（一张卡一份的话，300 张卡就是
    // 300 个 observer + 300 个 window 监听，一次窗口缩放触发 600 次强制同步布局）。
    const release = observeResize(node, check);
    let last = html;
    return {
      update(next: string): void {
        // 参数就是渲染后的 HTML：变了说明内容变了，重新量一次
        if (next === last) return;
        last = next;
        check();
      },
      destroy(): void {
        release();
      }
    };
  }

  /** 「日期与提醒」浮层用 fixed 定位：absolute 会被卡片/任务列表的 overflow 裁剪。
   * fixed 在 transform 缩放的 app-shell 内相对其左上角定位，按钮的屏幕坐标
   * 除以 scale 换算回逻辑坐标；贴近视口底部时向上翻转。 */
  function toggleSchedulePanel(): void {
    showPicker = !showPicker;
    if (!showPicker || !dueButtonEl) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const rect = dueButtonEl.getBoundingClientRect();
    // 面板 = 日历 + 时刻行 + 提醒行 + 页脚，比纯日历高一截；估高了只是提前向上翻转
    const estVisualHeight = 470;
    const openBelow = rect.bottom + estVisualHeight <= window.innerHeight;
    const anchorEdge = openBelow ? rect.bottom + 6 : rect.top - estVisualHeight - 6;
    const topLogical = anchorEdge / scale;
    const rightLogical = (window.innerWidth - rect.right) / scale;
    datePopoverStyle = `top: ${topLogical}px; right: ${rightLogical}px;`;
  }

  function handleSchedule(patch: { dueDate: string; dueTime: string; reminders: ReminderRule[] }): void {
    showPicker = false;
    dispatch("setSchedule", { id: task.id, ...patch });
  }

  function handleScheduleClear(): void {
    showPicker = false;
    dispatch("setSchedule", { id: task.id, dueDate: "", dueTime: "", reminders: [] });
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
    // 量不出可展开内容但存储态是展开的（标题又放得下了）也要能收起
    if (!canExpand && !isExpanded) return;
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
    // 任务列表的勾选框：点一下就把源码里的 `- [ ]` / `- [x]` 翻过来存回去
    const boxIndex = taskToggleIndex(event, event.currentTarget as Element);
    if (boxIndex !== null) {
      event.preventDefault();
      void saveTaskMarkdown(task.id, toggleMarkdownTask(task.markdown, boxIndex), isExpanded);
      return;
    }
    if (mobile) handleMobileTap(event);
  }

  function startTagEdit(tagId: string, currentText: string): void {
    editingTagId = tagId;
    editingTagText = currentText || "";
    void Promise.resolve().then(() => tagEditEl?.focus());
  }

  /**
   * 标签点按：触屏第一下只露出删除叉（红叉缩在角上，不挡着的话第一下就直接删了），
   * 第二下点文字才进编辑；桌面 hover 已经露叉，点文字直接编辑。
   */
  function handleTagTap(tagId: string, currentText: string): void {
    if (touchOnly && revealedTagId !== tagId) {
      revealedTagId = tagId;
      revealedEmojiIndex = -1;
      return;
    }
    revealedTagId = "";
    startTagEdit(tagId, currentText);
  }

  /** 表情同标签：触屏第一下露叉，第二下才换表情。 */
  function handleEmojiTap(index: number): void {
    if (touchOnly && revealedEmojiIndex !== index) {
      revealedEmojiIndex = index;
      revealedTagId = "";
      return;
    }
    revealedEmojiIndex = -1;
    dispatch("pickEmoji", { id: task.id, index });
  }

  /** 点到别处收回露出的删除叉 */
  function handleWindowPointerDown(event: PointerEvent): void {
    if (!revealedTagId && revealedEmojiIndex < 0) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".task-tag, .task-emoji-badge")) return;
    revealedTagId = "";
    revealedEmojiIndex = -1;
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

<svelte:window on:click={() => (showPicker = false)} on:pointerdown={handleWindowPointerDown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<article
  class:completed={task.completed}
  class:compact={!isExpanded}
  class:expanded={isExpanded}
  class:multiline={canExpand}
  class:plain
  class:selected
  class:due-soon={dueHighlight !== null}
  class:due-strong={dueHighlight?.strong === true}
  class="task-card"
  style={dueHighlightStyle(dueHighlight)}
  use:longpress={handleLongPress}
  on:mousedown={handleCardMouseDown}
  on:click={handleCardClick}
  on:dblclick={handleCardDblClick}
  on:contextmenu={openContext}
>
  <div class="task-title-grid">
    {#if !plain}
      <button class="task-check" type="button" aria-label="切换完成" on:click|stopPropagation={() => dispatch("toggle", task.id)}>
        {#if canExpand}
          <Plus size={14} strokeWidth={3.1} />
        {:else if task.completed}
          <Check size={14} strokeWidth={3.2} />
        {/if}
      </button>
    {/if}

    <section class="task-body">
      {#if isExpanded}
        <div class="markdown-body markdown-content" use:markdownWire on:click={handleMarkdownClick}>
          {#if fullHtml}{@html fullHtml}{:else}{@html collapsedHtml}{/if}
        </div>
      {:else}
        <div class="markdown-body markdown-title-row" class:clamped={titleOverflow} use:measureTitle={collapsedHtml} on:click={handleMarkdownClick}>
          {@html collapsedHtml}
        </div>
      {/if}

      <div class="task-tags">
        {#each task.emojis as emoji, index (`${task.id}-emoji-${index}`)}
          <span
            class="task-emoji-badge"
            title="点击更换表情"
            class:reveal-delete={revealedEmojiIndex === index}
            on:click|stopPropagation={() => handleEmojiTap(index)}
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
              style={tagChipStyle(tag)}
              title={tag.text || "点击编辑标签"}
              class:reveal-delete={revealedTagId === tag.id}
              on:click|stopPropagation={() => handleTagTap(tag.id, tag.text || "")}
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
          <button bind:this={dueButtonEl} class="task-due-date" type="button" title="日期与提醒" on:click|stopPropagation={toggleSchedulePanel}>{formattedDate}</button>
          {#if showPicker}
            <div class="task-date-popover" style={datePopoverStyle}>
              <TaskDateReminderPanel
                dueDate={task.dueDate?.slice(0, 10) ?? ""}
                dueTime={task.dueTime ?? ""}
                reminders={task.reminders ?? []}
                onSave={handleSchedule}
                onClear={handleScheduleClear}
                onClose={() => (showPicker = false)}
              />
            </div>
          {/if}
        </div>
      {/if}
    </section>

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

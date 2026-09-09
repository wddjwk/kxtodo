<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { Image as ImageIcon, PenLine } from "@lucide/svelte";
  import { markdownTitle, renderMarkdown } from "../markdown";
  import { markdownWire } from "../markdownControls";
  import { mdImageCache, resolveMarkdownImages } from "../images";
  import { isMobile as isMobileStore } from "../platform";
  import { longpress, isLongPressSuppressed } from "../longpress";
  import {
    DIARY_IMAGE_NODE, diaryExcerpt, diaryImageCount, moodLabel,
    relativeDayLabel, timeOf, weatherLabel, withoutFirstLine
  } from "../diary";
  import type { DiaryEntry } from "../types";

  export let entry: DiaryEntry;
  export let selected = false;
  /** 分组/日历视图里日期已经写在分区标题上，卡片就不再重复一遍 */
  export let showDate = true;
  /** 相对哪一天算「今天/昨天」（列表视图随时间走，日历视图跟着选中日期） */
  export let today = "";

  const dispatch = createEventDispatcher<{
    expand: { id: string; expanded: boolean };
    edit: string;
    context: { id: string; x: number; y: number };
    openLink: { href: string; title: string };
  }>();

  /** 移动端两击判定窗口：与 TaskCard 同一个折中值（太大单击发闷，太小双击漏判）。 */
  const DOUBLE_TAP_MS = 220;
  const EXCERPT_LIMIT = 180;

  let tapTimer: number | undefined;
  let lastTapAt = 0;
  /** 折叠态摘要是否两行放不下（单行但特别长的正文）——是的话这张卡片也可以展开 */
  let excerptOverflow = false;
  /** 折叠态标题是否单行显示不全 */
  let titleOverflow = false;

  // isMobile 是 store：当布尔直接用会永远为真，桌面端就会误走移动端手势
  $: mobile = $isMobileStore;
  $: explicitTitle = entry.title.trim();
  $: heading = explicitTitle || (entry.markdown.trim() ? markdownTitle(entry.markdown) : "无题");
  // 没有标题时首行已经被当标题显示了，摘要从第二行开始，否则同一句话在卡片上出现两遍
  $: excerptSource = explicitTitle ? entry.markdown : withoutFirstLine(entry.markdown);
  $: excerpt = diaryExcerpt(excerptSource, EXCERPT_LIMIT);
  $: images = diaryImageCount(entry.markdown);
  // 与 todo 卡片同一条口径：多行/带图/摘要被截断/摘要两行放不下/标题显示不全，都算可展开。
  // 「单行但特别长」靠量（excerptOverflow/titleOverflow），字符数阈值识别不了它。
  $: canExpand =
    Boolean(entry.markdown.trim()) &&
    (excerpt.length >= EXCERPT_LIMIT ||
      excerptSource.trim().split(/\r?\n/).filter((line) => line.trim()).length > 1 ||
      images > 0 ||
      excerptOverflow ||
      titleOverflow);
  // 展开态只认存储值：canExpand 是量出来的易失值（滚动条出现/消失、宽度变化都会翻转），
  // 拿它门控渲染会出现「动了别的卡片这张自己展开/收起」（v0.6.8 在 todo 卡片修过同一病）。
  $: isExpanded = entry.expanded === true;
  $: fullHtml = renderMarkdown(resolveMarkdownImages(entry.markdown, DIARY_IMAGE_NODE, $mdImageCache));
  $: dayNumber = entry.date.slice(8, 10);
  $: monthLabel = `${Number.parseInt(entry.date.slice(5, 7), 10)}月`;
  $: dayLabel = relativeDayLabel(entry.date, today);
  $: clock = timeOf(entry.createdAt);
  $: weatherText = weatherLabel(entry.weather);
  $: moodText = moodLabel(entry.mood);

  onDestroy(() => {
    if (tapTimer !== undefined) window.clearTimeout(tapTimer);
  });

  /** 标题是单行 nowrap + 省略号：比宽度就知道显示全不全。 */
  function measureTitle(node: HTMLElement, text: string): { update: (next: string) => void; destroy: () => void } {
    const check = (): void => {
      titleOverflow = node.scrollWidth > node.clientWidth + 1;
    };
    check();
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => check()) : null;
    observer?.observe(node);
    window.addEventListener("resize", check);
    let last = text;
    return {
      update(next: string): void {
        if (next === last) return;
        last = next;
        check();
      },
      destroy(): void {
        observer?.disconnect();
        window.removeEventListener("resize", check);
      }
    };
  }

  /** 摘要固定夹两行：「单行但特别长」的正文要量**自然高度**才知道两行放不放得下，
   *  量之前临时摘掉夹行（同步完成，中间不会重绘），量完恢复。 */
  function measureExcerpt(node: HTMLElement, text: string): { update: (next: string) => void; destroy: () => void } {
    const check = (): void => {
      const lineHeight = parseFloat(getComputedStyle(node).lineHeight) || 0;
      node.style.setProperty("-webkit-line-clamp", "unset");
      const natural = node.scrollHeight;
      node.style.setProperty("-webkit-line-clamp", "");
      excerptOverflow = lineHeight > 0 && natural > lineHeight * 2 + 1;
    };
    check();
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => check()) : null;
    observer?.observe(node);
    window.addEventListener("resize", check);
    let last = text;
    return {
      update(next: string): void {
        if (next === last) return;
        last = next;
        check();
      },
      destroy(): void {
        observer?.disconnect();
        window.removeEventListener("resize", check);
      }
    };
  }

  function toggleExpand(): void {
    // 量不出可展开内容但存储态是展开的（宽度又放得下了）也要能收起
    if (!canExpand && !isExpanded) return;
    dispatch("expand", { id: entry.id, expanded: !isExpanded });
  }

  function openEditor(): void {
    dispatch("edit", entry.id);
  }

  function isInteractiveTarget(event: MouseEvent): boolean {
    const target = event.target as HTMLElement | null;
    return Boolean(target?.closest("button, input, textarea, a"));
  }

  /** 移动端：单击展开/折叠（延后一个双击窗口），双击进编辑器。 */
  function handleMobileTap(event: MouseEvent): void {
    if (isInteractiveTarget(event)) return;
    const stamp = Date.now();
    const doubled = event.detail >= 2 || stamp - lastTapAt < DOUBLE_TAP_MS;
    lastTapAt = stamp;
    if (tapTimer !== undefined) window.clearTimeout(tapTimer);
    tapTimer = undefined;
    // 长按出菜单时不许留下文本选区
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

  /** 桌面双击展开：第二次 mousedown 阻止选词，单击仍可正常选中复制。 */
  function handleMouseDown(event: MouseEvent): void {
    if (event.detail < 2 || isInteractiveTarget(event)) return;
    event.preventDefault();
  }

  function handleDblClick(event: MouseEvent): void {
    // 移动端的双击语义在 handleMobileTap（进编辑器），这里再跑一遍会既开编辑器又改展开态
    if (mobile || isInteractiveTarget(event)) return;
    event.preventDefault();
    window.getSelection()?.removeAllRanges();
    toggleExpand();
  }

  function handleClick(event: MouseEvent): void {
    // 长按抬手补发的 click 会冒泡关掉刚开的菜单，抑制窗内吞掉
    if (isLongPressSuppressed()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (mobile) handleMobileTap(event);
  }

  function handleBodyClick(event: MouseEvent): void {
    // 正文区的点击不冒泡到卡片（否则移动端一次点击被两套逻辑各处理一遍），手势语义在这里补上
    event.stopPropagation();
    if (isLongPressSuppressed()) {
      event.preventDefault();
      return;
    }
    const link = (event.target as HTMLElement | null)?.closest("a[href]");
    if (link instanceof HTMLAnchorElement) {
      event.preventDefault();
      dispatch("openLink", { href: link.href, title: (link.textContent ?? "").trim() });
      return;
    }
    if (mobile) handleMobileTap(event);
  }

  function openContext(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    // 触摸长按已开过菜单时，Chromium 补发的原生 contextmenu 直接吞掉
    if (isLongPressSuppressed()) return;
    dispatch("context", { id: entry.id, x: event.clientX, y: event.clientY });
  }

  function handleLongPress(pos: { x: number; y: number }): void {
    dispatch("context", { id: entry.id, x: pos.x, y: pos.y });
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<article
  class="diary-card"
  class:expanded={isExpanded}
  class:expandable={canExpand}
  class:selected
  use:longpress={handleLongPress}
  on:mousedown={handleMouseDown}
  on:click={handleClick}
  on:dblclick={handleDblClick}
  on:contextmenu={openContext}
>
  {#if showDate}
    <div class="diary-date-block" title={dayLabel}>
      <span class="diary-date-day">{dayNumber}</span>
      <span class="diary-date-month">{monthLabel}</span>
    </div>
  {/if}

  <div class="diary-card-main">
    <header class="diary-card-head">
      <h3 class="diary-card-title" use:measureTitle={heading}>{heading}</h3>
      <span class="diary-card-meta">
        {#if entry.mood}<span class="diary-meta-chip" title={moodText || "心情"}>{entry.mood}</span>{/if}
        {#if entry.weather}<span class="diary-meta-chip" title={weatherText || "天气"}>{entry.weather}</span>{/if}
        {#if images}<span class="diary-meta-chip diary-meta-images" title="{images} 张插图"><ImageIcon size={13} />{images}</span>{/if}
        {#if clock}<span class="diary-meta-time">{clock}</span>{/if}
      </span>
    </header>

    {#if isExpanded}
      <div class="markdown-body markdown-content diary-card-content" use:markdownWire on:click={handleBodyClick}>
        {@html fullHtml}
      </div>
    {:else if excerpt}
      <p class="diary-card-excerpt" use:measureExcerpt={excerpt}>{excerpt}</p>
    {/if}

    {#if entry.tags.length}
      <div class="diary-card-tags">
        {#each entry.tags as tag (tag.id)}
          <span class={`task-tag tag-${tag.color}`}>{tag.text || ""}</span>
        {/each}
      </div>
    {/if}
  </div>

  <button class="diary-edit-button" type="button" title="编辑这篇日记" on:click|stopPropagation={openEditor}>
    <PenLine size={17} />
  </button>
</article>

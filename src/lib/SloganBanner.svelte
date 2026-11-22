<script lang="ts">
  /**
   * 标语栏全局悬浮层（v0.9.4）：挂在 App 壳上（Toast 同级），**不改任何页面**。
   * 右对齐悬浮在「页面标题上方」或「输入框下方」，不挤占排版；多条轮播（8s 一帧，
   * 底部进度条走满一轮），悬停暂停；X 关闭 = 把 enabled 拨 false（内容保留）。
   * 点标语本体直达标语栏工具页。移动端不展示（工具页里仍可维护内容）。
   *
   * 定位：fixed 挂在缩放壳内，坐标一律布局像素（rect 差 ÷ uiScale，与 popoverFrame
   * 同口径）。锚点取当前可见页面的 .list-header / .add-task-bar，页面切换（DOM 变更）
   * 与窗口缩放时重算；不跟随内容滚动——它是悬浮提醒，不是页面元素。
   */
  import { X } from "@lucide/svelte";
  import { get } from "svelte/store";
  import { onDestroy } from "svelte";
  import { appState, appSettings, diaryOpen, ledgerOpen, calendarOpen, toolboxOpen } from "./stores";
  import { isMobile } from "./platform";
  import { setSlogan } from "./actions";
  import { activeSlogans, nextRotation, sloganIconName, SLOGAN_ROTATE_MS } from "./slogan";
  import { ledgerIcon } from "./ledgerIcons";
  import { openToolboxTool } from "./tools/navigation";

  let bannerEl: HTMLElement | null = null;
  let top = 42;
  let placed = false;
  let index = 0;
  let hovered = false;

  $: slogan = $appState.slogan;
  $: shown = activeSlogans(slogan);
  $: visible = !$isMobile && slogan.enabled && shown.length > 0;
  $: iconName = sloganIconName(slogan.icon);
  $: if (index >= shown.length) index = 0;
  $: current = shown[index] ?? shown[0];

  // 轮播定时器：单条/悬停/隐藏时不走。Svelte 4 的响应式语句不能 return cleanup，
  // 用受管 timer + onDestroy 收尾；rotating/shown 变化时重建。
  let timer: ReturnType<typeof setInterval> | null = null;
  function stopTimer(): void {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }
  $: rotating = visible && shown.length > 1 && !hovered;
  $: if (rotating) {
    stopTimer();
    timer = setInterval(() => {
      index = nextRotation(index, shown.length);
    }, SLOGAN_ROTATE_MS);
  } else {
    stopTimer();
  }
  onDestroy(stopTimer);

  function isShown(el: Element): boolean {
    let node: Element | null = el;
    while (node && !node.classList.contains("app-shell")) {
      const style = getComputedStyle(node);
      if (style.display === "none" || style.visibility === "hidden") return false;
      node = node.parentElement;
    }
    return true;
  }

  function firstShown(selector: string): Element | null {
    for (const el of document.querySelectorAll(selector)) {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0 && isShown(el)) return el;
    }
    return null;
  }

  function place(): void {
    const shell = document.querySelector(".app-shell");
    if (!shell || !bannerEl) return;
    const scale = get(appSettings).appearance.uiScale || 1;
    const shellRect = shell.getBoundingClientRect();
    const height = bannerEl.offsetHeight;
    const shellHeight = shellRect.height / scale;
    let y: number;
    if (slogan.position === "bottom") {
      const bar = firstShown(".add-task-bar");
      y = bar ? (bar.getBoundingClientRect().bottom - shellRect.top) / scale + 8 : shellHeight - height - 96;
    } else {
      const header = firstShown(".list-header") ?? firstShown(".toolbox-sub-bar");
      // 标题上方：贴着头部上缘再往上一条；没有头部（异常态）回落标题栏下
      y = header ? (header.getBoundingClientRect().top - shellRect.top) / scale - height - 6 : 42;
    }
    top = Math.max(42, Math.min(y, shellHeight - height - 8));
    placed = true;
  }

  let placeQueued = false;
  function queuePlace(): void {
    if (placeQueued) return;
    placeQueued = true;
    requestAnimationFrame(() => {
      placeQueued = false;
      place();
    });
  }

  $: if (visible && bannerEl) {
    // 位置/名单变化后重算（slogan.position 字面出现才是依赖）
    void slogan.position;
    void shown.length;
    queuePlace();
  }

  function mountPlace(node: HTMLElement): { destroy: () => void } {
    queuePlace();
    const shell = node.closest(".app-shell");
    const observer = new MutationObserver(queuePlace);
    if (shell) observer.observe(shell, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style"] });
    return { destroy: () => observer.disconnect() };
  }

  function openConfig(): void {
    diaryOpen.set(false);
    ledgerOpen.set(false);
    calendarOpen.set(false);
    toolboxOpen.set(true);
    openToolboxTool("slogan");
  }

  function disable(event: MouseEvent): void {
    event.stopPropagation();
    void setSlogan({ ...slogan, enabled: false });
  }
</script>

<svelte:window on:resize={queuePlace} />

{#if visible && current}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions a11y_no_noninteractive_element_interactions -->
  <div
    class="slogan-banner"
    class:bottom={slogan.position === "bottom"}
    style={`top: ${top}px; visibility: ${placed ? "visible" : "hidden"}`}
    bind:this={bannerEl}
    use:mountPlace
    role="status"
    title={current.text}
    on:pointerenter={() => (hovered = true)}
    on:pointerleave={() => (hovered = false)}
    on:click|stopPropagation={openConfig}
  >
    {#key index}
      <span class="slogan-banner-body">
        {#if iconName}
          <svelte:component this={ledgerIcon(iconName, "Megaphone")} size={14} />
        {/if}
        <span class="slogan-banner-text">{current.text}</span>
      </span>
      {#if shown.length > 1}
        <span class="slogan-banner-progress" class:paused={hovered} style={`animation-duration: ${SLOGAN_ROTATE_MS}ms`}></span>
      {/if}
    {/key}
    <button
      type="button"
      class="slogan-banner-x"
      title="关闭标语展示（可在工具箱·标语栏重新打开）"
      aria-label="关闭标语展示"
      on:click|stopPropagation={disable}
    >
      <X size={12} />
    </button>
  </div>
{/if}
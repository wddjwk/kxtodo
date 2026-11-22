<script lang="ts">
  /**
   * 标语栏全局悬浮层（v0.9.4；v0.9.5 重做）：挂在 App 壳上（Toast 同级），不改任何页面。
   * 六个位置：顶部/底部对齐**内容区**（不锚定输入框，没有输入框的页面也正常），
   * 左/中/右水平对齐；左右边界跟卡片区对齐（输入框的左右边缘）。
   * 多条轮播只按配置间隔切换（无进度条）；悬停暂停；X 关闭 = enabled false（内容保留）。
   * 每个条目带自己的图标；胶囊主题色（图标与描边）、字体颜色、背景透明度可配。
   * 文字最多展示 30 字宽，超出横向来回滚动（跑马灯）。
   * 点标语本体直达标语栏工具页。移动端不展示（工具页里仍可维护内容）。
   *
   * 定位：fixed 挂在缩放壳内，坐标一律布局像素（rect 差 ÷ uiScale，与 popoverFrame
   * 同口径）。页面切换（DOM 变更）与窗口缩放时重算；不跟随内容滚动——它是悬浮提醒。
   */
  import { get } from "svelte/store";
  import { onDestroy } from "svelte";
  import { appState, appSettings, diaryOpen, ledgerOpen, calendarOpen, toolboxOpen } from "./stores";
  import { isMobile } from "./platform";
  import { activeSlogans, nextRotation, sloganIntervalMs } from "./slogan";
  import SloganIcon from "./SloganIcon.svelte";
  import { openToolboxTool } from "./tools/navigation";

  let bannerEl: HTMLElement | null = null;
  let wrapEl: HTMLElement | null = null;
  let textEl: HTMLElement | null = null;
  let layerLeft = 0;
  let layerWidth = 0;
  let top = 42;
  let placed = false;
  let index = 0;
  let hovered = false;
  let marqueeShift = 0;
  let marqueeDuration = 0;

  $: slogan = $appState.slogan;
  $: shown = activeSlogans(slogan);
  $: visible = !$isMobile && slogan.enabled && shown.length > 0;
  $: if (index >= shown.length) index = 0;
  $: current = shown[index] ?? shown[0];
  $: isTop = slogan.position.startsWith("top");
  $: horizontal = slogan.position.endsWith("left") ? "left" : slogan.position.endsWith("right") ? "right" : "center";
  $: themeStyle = `--slogan-accent: ${slogan.accent || "var(--accent)"}; --slogan-text: ${slogan.textColor || "#4b5563"}; --slogan-bg-alpha: ${slogan.opacity}`;

  // 轮播定时器：单条/悬停/隐藏时不走。Svelte 4 的响应式语句不能 return cleanup，
  // 用受管 timer + onDestroy 收尾；rotationKey 里把「名单长度 + 间隔」当字面依赖。
  let timer: ReturnType<typeof setInterval> | null = null;
  function stopTimer(): void {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }
  $: rotating = visible && shown.length > 1 && !hovered;
  $: rotationKey = rotating ? `${shown.length}:${sloganIntervalMs(slogan.intervalSeconds)}` : "";
  $: if (rotationKey) {
    stopTimer();
    timer = setInterval(() => {
      index = nextRotation(index, shown.length);
    }, sloganIntervalMs(slogan.intervalSeconds));
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
    const shellWidth = shellRect.width / scale;
    const shellHeight = shellRect.height / scale;

    // 左右边界基准（v0.9.5 可配置）：field = 输入框/卡片区（默认）；app = 程序内容区（右半区）
    const addBar = firstShown(".add-task-bar");
    const appRoot =
      firstShown(".workspace") ??
      firstShown(".diary-view") ??
      firstShown(".ledger-view") ??
      firstShown(".calendar-view") ??
      firstShown(".toolbox-view");
    const fieldAnchor =
      addBar ??
      firstShown(".task-list") ??
      firstShown(".list-header") ??
      firstShown(".toolbox-view") ??
      firstShown(".diary-view") ??
      firstShown(".ledger-view") ??
      firstShown(".calendar-view");
    const anchor = slogan.bounds === "app" ? (appRoot ?? fieldAnchor) : fieldAnchor;
    if (!anchor) {
      placed = false;
      return;
    }
    const rect = anchor.getBoundingClientRect();
    layerLeft = Math.max(8, Math.min(shellWidth - 40, (rect.left - shellRect.left) / scale));
    const right = Math.min(shellWidth - 8, Math.max(layerLeft + 40, (rect.right - shellRect.left) / scale));
    layerWidth = right - layerLeft;

    const header = firstShown(".list-header") ?? firstShown(".toolbox-sub-bar") ?? firstShown(".toolbox-view");
    const contentTop = header
      ? (header.getBoundingClientRect().top - shellRect.top) / scale
      : (rect.top - shellRect.top) / scale;
    // 底部对齐内容区底部（不锚定输入框）；有浮动加号时抬到加号上方，避免遮挡
    const bottomAnchor =
      addBar ??
      firstShown(".scheduler-panel") ??
      firstShown(".diary-scroll") ??
      firstShown(".ledger-scroll") ??
      firstShown(".toolbox-view") ??
      firstShown(".diary-view") ??
      firstShown(".ledger-view") ??
      firstShown(".calendar-view") ??
      anchor;
    const contentBottom = (bottomAnchor.getBoundingClientRect().bottom - shellRect.top) / scale;
    const fab = firstShown(".diary-fab-row, .ledger-fab-row, .calendar-fab-row, .scheduler-floating-add");
    const fabTop = fab ? (fab.getBoundingClientRect().top - shellRect.top) / scale : null;

    const height = bannerEl.offsetHeight;
    const y = isTop
      ? contentTop - height - 6
      : fabTop !== null
        ? fabTop - height - 8
        : contentBottom + 6;
    top = Math.max(40, Math.min(y, shellHeight - height - 8));
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
    // 位置/边界变化后重算（字面依赖：slogan.position / slogan.bounds / shown.length）
    void slogan.position;
    void slogan.bounds;
    void shown.length;
    queuePlace();
  }

  // 跑马灯：文字超过 30 字宽（wrap 的实际可用宽度）才横向来回滚动
  let measureQueued = false;
  function queueMeasure(): void {
    if (measureQueued) return;
    measureQueued = true;
    requestAnimationFrame(() => {
      measureQueued = false;
      if (!wrapEl || !textEl) return;
      const overflow = textEl.scrollWidth - wrapEl.clientWidth;
      if (overflow > 2) {
        marqueeShift = -overflow;
        marqueeDuration = Math.min(24, Math.max(6, 4 + overflow / 40));
      } else {
        marqueeShift = 0;
        marqueeDuration = 0;
      }
    });
  }
  $: if (visible && current) {
    void current.text;
    queueMeasure();
  }

  function mountPlace(node: HTMLElement): { destroy: () => void } {
    queuePlace();
    queueMeasure();
    const shell = node.closest(".app-shell");
    const observer = new MutationObserver(() => {
      queuePlace();
      queueMeasure();
    });
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
</script>

<svelte:window on:resize={queuePlace} />

{#if visible && current}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions a11y_no_noninteractive_element_interactions -->
  <div
    class="slogan-layer"
    style={`left: ${layerLeft}px; width: ${layerWidth}px; top: ${top}px; visibility: ${placed ? "visible" : "hidden"}`}
    use:mountPlace
  >
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions a11y_no_noninteractive_element_interactions -->
    <div
      class="slogan-banner"
      class:pos-left={horizontal === "left"}
      class:pos-center={horizontal === "center"}
      class:pos-right={horizontal === "right"}
      style={themeStyle}
      bind:this={bannerEl}
      role="status"
      title={marqueeShift < 0 ? current.text : undefined}
      on:pointerenter={() => (hovered = true)}
      on:pointerleave={() => (hovered = false)}
      on:click|stopPropagation={openConfig}
    >
      {#key index}
        <span class="slogan-banner-body">
          <SloganIcon icon={current.icon} size={14} />
          <span class="slogan-banner-text-wrap" bind:this={wrapEl}>
            <span
              class="slogan-banner-text"
              class:marquee={marqueeShift < 0}
              style={marqueeShift < 0 ? `--marquee-shift: ${marqueeShift}px; --marquee-duration: ${marqueeDuration}s` : ""}
              bind:this={textEl}
            >{current.text}</span>
          </span>
        </span>
      {/key}
    </div>
  </div>
{/if}
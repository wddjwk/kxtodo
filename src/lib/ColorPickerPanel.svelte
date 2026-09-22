<script lang="ts">
  /**
   * 全应用统一的取色盘（v0.8.6 需求 11/12）：内嵌 iro.js 色盘 + RGB/HEX 手动输入
   * + 取消/确认按钮，挂在 App 里一次，由 `colorPickerPanel.ts` 的单例请求驱动。
   *
   * 语义（与 colorPreview 那套铁律一致）：
   * - 拖动色盘 / 手输 RGB HEX = **只改草稿 + 实时预览**；
   * - 「确认」才落盘（`onConfirm`）；「取消」/ 点浮层外 / Esc = 丢弃草稿、预览回退。
   *
   * 三件必须在实现里做对的事：
   * ① **rAF 合帧**：拖动时 `input:change` 连续触发，每个事件都写一次预览会让
   *    store 写入次数随事件数线性增长（还有跨页预览回退风险）；一帧只写一次。
   * ② **touch-action: none**：否则安卓上拖动选色会顺手把页面滚走。
   * ③ 键盘可达性**只补了 Esc = 取消**（与点浮层外同义）；方向键调值本版不做，
   *    如实留档、不假装有。
   *
   * iro.js 是 vanilla 库（`new iro.ColorPicker(container, …)`）：与
   * `IconPicker.svelte` 里 emoji-picker-element 同一种接法，不踩 runes/snippet 的坑。
   * 上游 2021 年后停止活跃开发，锁 5.5.2；真要换外观只换色盘本体（这个外壳与草稿
   * 语义留在原地）。
   */
  import { get } from "svelte/store";
  import { onDestroy, onMount, tick } from "svelte";
  import { appSettings } from "./stores";
  import { uiScaleValue } from "./styles";
  import { placePopover } from "./popover";
  import { imeInset } from "./imeInset";
  import {
    cancelColorPicker, colorPickRequest, colorPickerDebug, confirmColorPicker, normalizeHexInput, parseChannelInput,
    publishColorPickerDebug
  } from "./colorPickerPanel";

  type IroColorLike = {
    hexString: string;
    rgb: { r: number; g: number; b: number };
    set: (value: string) => void;
  };
  type IroPickerLike = {
    color: IroColorLike;
    on: (event: string | string[], callback: () => void) => void;
    off: (event: string | string[], callback: () => void) => void;
    resize: (width: number) => void;
    base?: HTMLElement | null;
  };

  type EyeDropperConstructor = new () => { open: () => Promise<{ sRGBHex: string }> };
  // 吸管：只有 Chromium 系（Windows WebView2）有，Linux WebKitGTK / 安卓不渲染按钮——
  // v0.8.5 的吸管是原生 input[type=color] 的系统选色器**自带**的，v0.8.6 换 iro 时静默丢了。
  const EyeDropperApi =
    typeof window === "undefined"
      ? undefined
      : (window as unknown as { EyeDropper?: EyeDropperConstructor }).EyeDropper;

  let host: HTMLElement;
  /** 反缩放层（见 applyPickerSize 的说明）：iro 挂在这一层里，不是直接挂宿主 */
  let canvasHost: HTMLElement;
  let panelEl: HTMLElement;
  let style = "";
  let placed = false;
  let ready = false;
  let picker: IroPickerLike | null = null;
  let eyedropperBusy = false;

  let hexDraft = "";
  let redDraft = "";
  let greenDraft = "";
  let blueDraft = "";
  let hint = "";

  $: request = $colorPickRequest;

  /** 预览写 store 走 rAF 合帧：拖动一秒能来上百个事件，逐个写就是上百次 store 更新 */
  let previewFrame = 0;
  let pendingColor = "";

  function schedulePreview(): void {
    colorPickerDebug.previewEvents += 1;
    publishColorPickerDebug();
    if (previewFrame !== 0) return;
    previewFrame = requestAnimationFrame(() => {
      previewFrame = 0;
      const current = $colorPickRequest;
      if (!current) return;
      colorPickerDebug.previewWrites += 1;
      publishColorPickerDebug();
      current.onPreview(pendingColor);
    });
  }

  function syncDrafts(color: IroColorLike): void {
    hexDraft = color.hexString;
    redDraft = String(color.rgb.r);
    greenDraft = String(color.rgb.g);
    blueDraft = String(color.rgb.b);
  }

  /** `color:change` = **任何**变化（拖动、手输 HEX/RGB 写属性都算）：同步输入框 + 预览 */
  function handleColorChange(): void {
    if (!picker) return;
    syncDrafts(picker.color);
    pendingColor = picker.color.hexString;
    schedulePreview();
  }

  /** `input:change` = 只有用户在色盘上的鼠标/触摸操作：预览（草稿已由上面那条同步） */
  function handleInputChange(): void {
    if (!picker) return;
    pendingColor = picker.color.hexString;
    schedulePreview();
  }

  /** 手输 HEX：合法才写；写属性会触发 color:change（色盘同步 + 预览） */
  function commitHex(): void {
    const hex = normalizeHexInput(hexDraft);
    if (!hex || !picker) {
      hint = "HEX 需要 3 位或 6 位十六进制，例如 #4a90d9";
      return;
    }
    hint = "";
    if (hex !== picker.color.hexString) picker.color.set(hex);
    else syncDrafts(picker.color);
  }

  /** 手输 RGB：三个都合法才写 */
  function commitChannel(): void {
    const r = parseChannelInput(redDraft);
    const g = parseChannelInput(greenDraft);
    const b = parseChannelInput(blueDraft);
    if (r === null || g === null || b === null || !picker) {
      hint = "RGB 需要三个 0–255 的整数";
      return;
    }
    hint = "";
    const current = picker.color.rgb;
    if (current.r !== r || current.g !== g || current.b !== b) {
      picker.color.set(`rgb(${r}, ${g}, ${b})`);
    } else {
      syncDrafts(picker.color);
    }
  }

  async function layout(): Promise<void> {
    await tick();
    if (!panelEl) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale);
    const anchor = request?.anchor?.getBoundingClientRect();
    const panelRect = panelEl.getBoundingClientRect();
    const point = anchor
      ? { x: anchor.right / scale, y: anchor.bottom / scale }
      : { x: 8, y: 8 };
    const placedBox = placePopover(
      point,
      // 高度口径：rect 是视觉像素要 ÷scale，scrollHeight 本就是布局像素（不能再除）
      { width: panelRect.width / scale, height: Math.max(panelRect.height / scale, panelEl.scrollHeight) },
      { width: window.innerWidth / scale, height: window.innerHeight / scale },
      { xAlign: anchor ? "right" : "left", gap: 6 }
    );
    style = `top: ${placedBox.top}px; left: ${placedBox.left}px;${
      placedBox.maxHeight ? ` max-height: ${placedBox.maxHeight}px; overflow-y: auto;` : ""
    }`;
    placed = true;
  }

  const follow = (): void => {
    applyPickerSize();
    void layout();
  };

  /**
   * 当前正在挂载 / 已挂载的请求 key。**不能用 `picker` 是否为真当判据**：
   * 动态 import 在途时 picker 还是 null，重跑这个响应式块就会再挂一次——
   * 每次 `await tick()` 都是微任务、永远不给 fetch 让路，整页主线程被这个循环饿死
   * （实测：点一下色块页面直接卡死）。
   */
  let activeKey: string | null = null;

  /** 面板挂上后才建 iro 实例：容器得先在 DOM 里 */
  onMount(() => {
    window.addEventListener("resize", follow);
    // 软键盘弹起时 visualViewport 变了：面板要重新落位（别被键盘盖住）
    window.visualViewport?.addEventListener("resize", follow);
  });

  onDestroy(() => {
    window.removeEventListener("resize", follow);
    window.visualViewport?.removeEventListener("resize", follow);
    releasePicker();
  });

  function releasePicker(): void {
    if (picker) {
      picker.off("color:change", handleColorChange);
      picker.off("input:change", handleInputChange);
      picker.base?.parentNode?.removeChild(picker.base);
      picker = null;
    }
  }

  $: if (request && activeKey !== request.key) {
    activeKey = request.key;
    ready = false;
    placed = false;
    hint = "";
    previewFrame = 0;
    pendingColor = request.color;
    void openRequest(request);
  } else if (!request && activeKey !== null) {
    activeKey = null;
    previewFrame = 0;
    releasePicker();
  }

  async function openRequest(current: typeof request): Promise<void> {
    if (!current) return;
    await tick();
    await mountPicker(current.color);
    if (!picker) return;
    syncDrafts(picker.color);
    ready = true;
    applyPickerSize();
    await layout();
  }

  /**
   * 色盘尺寸（v0.8.7 需求 2.2）。两个口径必须分清：
   *
   * - iro 的指针数学是「光标**视觉**坐标 ÷ svg **布局**宽」——外壳 `transform: scale(uiScale)`
   *   让两个数在缩放下差一个 uiScale，拖动时把手指脱靶（实测偏移 × (1−uiScale)，44px）。
   *   修法三件套：① 反缩放层 `.kx-color-canvas-scale`（scale(1/uiScale)，iro 挂它里面，
   *   让 svg 的视觉尺寸 == 布局尺寸）；② 宽度传**视觉口径**（宿主布局宽 × uiScale）；
   *   ③ 宿主高度显式补偿（svg 布局盒 ≠ 视觉盒，不补会把面板下方的字段区压住）。
   *
   * - 宽度不能写死（写死 196 在小字号下溢出 41px），也不能只读 clientWidth（那是布局宽）。
   * - 比较一律「布局对布局」：offsetWidth 是布局像素，拿 getBoundingClientRect（视觉）
   *   跟 clientWidth（布局）比，scale ≠ 1 时每次都会误判「宽了」白 resize。
   */
  function applyPickerSize(): void {
    if (!picker || !host) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale) || 1;
    const width = Math.max(120, Math.floor(host.clientWidth * scale));
    if (width !== picker.base?.offsetWidth) picker.resize(width);
    if (picker.base) host.style.height = `${Math.round(picker.base.offsetHeight / scale)}px`;
  }

  async function mountPicker(color: string): Promise<void> {
    releasePicker();
    if (!host || !canvasHost) return;
    const { default: iro } = await import("@jaames/iro");
    // import 期间用户可能已经点了别处（面板收起）：那时别再建实例
    if (!$colorPickRequest) return;
    const scale = uiScaleValue($appSettings.appearance.uiScale) || 1;
    const instance = iro.ColorPicker(canvasHost, {
      width: Math.max(120, Math.floor(host.clientWidth * scale)),
      color,
      borderWidth: 1,
      borderColor: "#e5e7eb",
      layout: [
        { component: iro.ui.Box },
        { component: iro.ui.Slider, options: { sliderType: "hue" } }
      ]
    }) as unknown as IroPickerLike;
    picker = instance;
    // input:change = 只有用户鼠标/触摸操作；color:change = 任何变化（含输入框写入）
    instance.on("color:change", handleColorChange);
    instance.on("input:change", handleInputChange);
  }

  function handleBackdrop(event: PointerEvent): void {
    const target = event.target as HTMLElement | null;
    // 取色入口按钮豁免：它自己负责开关，否则 pointerdown 先关、click 再开
    if (target?.closest(".kx-color-panel") || target?.closest("[data-color-anchor]")) return;
    cancelColorPicker();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    if (!get(colorPickRequest)) return;
    event.preventDefault();
    cancelColorPicker();
  }

  let listening = false;
  $: if (typeof document !== "undefined" && Boolean(request) !== listening) {
    // 挂载式监听：面板在树上就听，摘掉就停（createBackGuard 那套的同样思路）
    listening = Boolean(request);
    if (listening) {
      document.addEventListener("pointerdown", handleBackdrop, true);
      window.addEventListener("keydown", handleKeydown, true);
    } else {
      document.removeEventListener("pointerdown", handleBackdrop, true);
      window.removeEventListener("keydown", handleKeydown, true);
    }
  }

  /** 把输入框里还没提交的编辑并进 picker（写属性会走 color:change 同一条链路）。 */
  function flushDraftsIntoPicker(): void {
    if (!picker) return;
    const hex = normalizeHexInput(hexDraft);
    if (hex && hex !== picker.color.hexString) {
      picker.color.set(hex);
      return;
    }
    const r = parseChannelInput(redDraft);
    const g = parseChannelInput(greenDraft);
    const b = parseChannelInput(blueDraft);
    if (r === null || g === null || b === null) return;
    const current = picker.color.rgb;
    if (current.r !== r || current.g !== g || current.b !== b) {
      picker.color.set(`rgb(${r}, ${g}, ${b})`);
    }
  }

  /**
   * 确认前把还没跑的合帧预览**同步**落地（v0.8.7 需求 2.3，治本的一处）：
   * `confirmColorPicker` 会把 store 置 null，飞行中的 rAF 醒来就被守卫丢弃——而 HEX/RGB
   * 的提交挂在 blur 上，**确认点击自己的 mousedown 就是那次 blur**：rAF 是确认点击自己
   * 排进去、又被它自己的 click 作废的，等再久也没用。消费端（尤其多档位的临期色）
   * 单靠 `onConfirm` 的实参接不上档位，靠这里的冲刷拿到最后一笔草稿。
   */
  function flushPendingPreview(): void {
    if (previewFrame === 0) return;
    cancelAnimationFrame(previewFrame);
    previewFrame = 0;
    const current = $colorPickRequest;
    if (!current) return;
    colorPickerDebug.previewWrites += 1;
    publishColorPickerDebug();
    current.onPreview(pendingColor);
  }

  function confirm(): void {
    flushDraftsIntoPicker();
    const hex = picker ? picker.color.hexString : (normalizeHexInput(hexDraft) ?? pendingColor);
    flushPendingPreview();
    confirmColorPicker(hex);
  }

  /** 吸管（v0.8.7 需求 2.5）：取到的色走既有 color:change 链路（同步输入框 + 预览），确认才落盘。 */
  async function pickFromScreen(): Promise<void> {
    if (!EyeDropperApi || !picker || eyedropperBusy) return;
    eyedropperBusy = true;
    try {
      const result = await new EyeDropperApi().open();
      const hex = normalizeHexInput(result.sRGBHex);
      if (hex && picker) picker.color.set(hex);
    } catch {
      // 用户取消（AbortError）：静默
    } finally {
      eyedropperBusy = false;
    }
  }
</script>

{#if request}
  <!-- 挂进 .app-shell（App 里渲染）：安卓安全区的 --safe-inv 从壳上继承 -->
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div
    class="kx-color-panel"
    class:placed
    bind:this={panelEl}
    style={style}
    role="dialog"
    aria-label="选择颜色"
    tabindex="-1"
    use:imeInset
    on:click|stopPropagation
    on:pointerdown|stopPropagation
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="kx-color-canvas" bind:this={host}>
      <!-- 反缩放层：iro 挂这一层里，让 svg 的视觉尺寸 == 布局尺寸（见 applyPickerSize） -->
      <div class="kx-color-canvas-scale" bind:this={canvasHost}></div>
    </div>
    <div class="kx-color-fields">
      <label class="kx-color-field">
        <span>HEX</span>
        <input
          class="kx-color-hex"
          value={hexDraft}
          spellcheck="false"
          on:input={(event) => (hexDraft = event.currentTarget.value)}
          on:blur={commitHex}
          on:keydown={(event) => { if (event.key === "Enter") (event.target as HTMLInputElement).blur(); }}
        />
      </label>
      <div class="kx-color-rgb">
        <label>
          <span>R</span>
          <input
            value={redDraft}
            inputmode="numeric"
            on:input={(event) => (redDraft = event.currentTarget.value)}
            on:blur={commitChannel}
          />
        </label>
        <label>
          <span>G</span>
          <input
            value={greenDraft}
            inputmode="numeric"
            on:input={(event) => (greenDraft = event.currentTarget.value)}
            on:blur={commitChannel}
          />
        </label>
        <label>
          <span>B</span>
          <input
            value={blueDraft}
            inputmode="numeric"
            on:input={(event) => (blueDraft = event.currentTarget.value)}
            on:blur={commitChannel}
          />
        </label>
      </div>
    </div>
    {#if hint}
      <p class="kx-color-hint">{hint}</p>
    {/if}
    <div class="kx-color-actions">
      {#if EyeDropperApi}
        <button
          class="menu-action-button kx-color-eyedropper"
          type="button"
          title="从屏幕上取色"
          on:click={() => void pickFromScreen()}
        >吸管</button>
      {/if}
      <button class="menu-action-button" type="button" on:click={cancelColorPicker}>取消</button>
      <button class="menu-action-button primary" type="button" on:click={confirm} data-color-confirm>确认</button>
    </div>
  </div>
{/if}

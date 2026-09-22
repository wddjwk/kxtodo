<script lang="ts">
  /**
   * 工具箱的壳（两端整页视图，v0.7.5 起桌面也有）：注册表驱动——
   * 列表页画 `availableTools()`（平台过滤在 src/lib/tools/registry.ts 里做），
   * 点开一个工具就懒加载它的子视图组件挂进来，返回收回列表。
   * 壳不认识任何具体工具：新增工具只动注册表与组件文件。
   *
   * v0.8.4 起**打开哪一个工具只有一处真源**——`tools/navigation.ts` 的 `toolRoute`：
   * 列表点卡片、侧栏钉住的行直达，都只是 `toolRoute.set(id)`；子视图从它纯派生，
   * 加载交给 `{#await}` + 记忆化的 `loadToolModule`。壳里不再有 `activeToolId`
   * 这样的影子状态，也就没有「路由到了、界面不动」的两份状态对不齐。
   * （v0.8.3 的 `fromPin` 也随之删掉：返回一律回工具箱主界面。）
   *
   * 子视图的头部（v0.8.4 需求 7）：不再有「返回工具箱」那一行——工具该拿到一块干净的
   * 画布；改成右上角两个按钮：左 = 回到工具箱，右 = ⋯ 菜单（只提供更换背景色 / 主题色）。
   */
  import { ArrowLeft, MoreHorizontal, Pin, PinOff, Radio, RotateCcw, Toolbox } from "@lucide/svelte";
  import MobileBack from "./MobileBack.svelte";
  import { appSettings } from "./stores";
  import { setConfig } from "./actions";
  import { createBackGuard } from "./platform";
  import { setToolPinned } from "./actions";
  import { openColorPicker } from "./colorPickerPanel";
  import { navToolId } from "./nav";
  import { openToolboxTool, resetToolRoute, toolRoute } from "./tools/navigation";
  import { availableTools, loadToolModule, type ToolDefinition } from "./tools/registry";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import { clearColorPreview, accentWithPreview, backgroundWithPreview, colorPreview, setColorPreview } from "./colorPreview";
  import { PAGE_HEADER_ICON_SIZE, toolboxAccent, toolboxBackground } from "./styles";
  import { defaultSettings, themePresets } from "./defaults";
  import { applyRelay } from "./transferStore";

  let cardMenu: { tool: ToolDefinition; x: number; y: number } | null = null;

  $: tools = availableTools();
  $: activeTool = $toolRoute === null ? null : (tools.find((tool) => tool.id === $toolRoute) ?? null);

  // ---- relay 服务（v0.8.5 需求 31；v0.8.7 追加需求改成三选一 + 勾选在左）----
  // core 的 transfer.relay 语义：空 = 复用同步的 p2p relay；`default` = n0 公共 relay；
  // 其它 = 自部署地址（`disabled` 只走直连，CLI 仍支持，GUI 不再提供这一项）。
  type RelayChoice = "follow" | "default" | "custom";

  let relayChoice: RelayChoice = "default";
  let relayCustom = "";

  function relayChoiceOf(value: string): RelayChoice {
    const raw = value.trim();
    if (raw === "") return "follow";
    if (raw === "default") return "default";
    return "custom";
  }

  /** 「复用同步配置」的前提：同步功能开着且已经配对（与设置页同一口径的简化版）。 */
  $: syncConfigured =
    $appSettings.features?.sync !== false &&
    Boolean(($appSettings.sync?.username ?? "").trim()) &&
    Boolean(($appSettings.sync?.secret ?? "").trim());

  function pickRelayChoice(choice: RelayChoice): void {
    if (choice === "follow" && !syncConfigured) return;
    relayChoice = choice;
    if (choice === "follow") void applyRelay("");
    else if (choice === "default") void applyRelay("default");
    // 「自定义服务」等用户把地址填完再保存（下面那个输入框失焦 / 回车）
  }

  function commitRelayCustom(): void {
    const value = relayCustom.trim();
    if (value === "") return;
    void applyRelay(value);
  }

  $: pinnedIds = $appSettings.appearance.navItems
    .filter((id) => id.startsWith("tool:"))
    .map((id) => id.slice("tool:".length));
  // 列表一露头就把各工具的 chunk 取回来：工具箱里的组件都是小件（注册表约定），
  // 而「点一下先看到正在打开…」是纯亏——用户看到的是没反馈，省下的是几 KB。
  // 放在这里而不是启动时：启动包不受影响，进工具箱又一定是瞬开。
  $: if (!activeTool) void Promise.all(tools.map((tool) => loadToolModule(tool).catch(() => undefined)));

  /** 打开一个工具 = 把路由指向它（列表 = null）。 */
  function openTool(tool: ToolDefinition): void {
    openToolboxTool(tool.id);
  }

  /** 返回：一律回工具箱主界面（列表）。移动端再按一次返回键才轮到整页退出。 */
  function backFromTool(): void {
    resetToolRoute();
  }

  // ---- 三点菜单的外观（主题色 / 背景色）：**工具箱主界面与每个工具子页各调各的**（v0.8.7）----
  // 菜单与其它页面完全同一份：点锚定的 ContextMenu——贴在按钮正下方 + 右缘对齐按钮右缘
  // （x=rect.right / y=rect.bottom，与「我的一天」那些页面的列表菜单同一套调用约定），
  // 取色走全应用统一的取色盘 + 草稿预览。
  const TOOLBOX_SCOPE = "toolbox";
  let appearanceMenu: { x: number; y: number; anchor: HTMLElement } | null = null;
  let draft: { accent?: string; background?: string } = {};

  /** 当前路由的取色作用域：主界面 = toolbox，工具子页 = toolbox:<工具id>（预览只染当前页） */
  $: activeToolId = activeTool?.id ?? "";
  $: colorScope = activeToolId ? `${TOOLBOX_SCOPE}:${activeToolId}` : TOOLBOX_SCOPE;
  /** 两层配置：工具子页没配过就跟主界面（toolboxAccent/toolboxBackground 的 toolId 参数） */
  $: baseAccent = toolboxAccent($appSettings.toolbox, activeToolId || undefined);
  $: baseBackground = toolboxBackground($appSettings.toolbox, activeToolId || undefined).color;
  $: accentShown = draft.accent ?? baseAccent;
  $: backgroundShown = draft.background ?? baseBackground;
  $: presets = $appSettings.appearance.themePresets.length ? $appSettings.appearance.themePresets : themePresets;
  // 预览（需求 9）：拖色盘时整页立刻跟着变，保存才落盘；菜单一关就回退
  $: previewAccent = accentWithPreview($colorPreview, colorScope, baseAccent);
  $: previewBackground = backgroundWithPreview($colorPreview, colorScope, { color: baseBackground });
  $: toolboxStyle = `--accent: ${previewAccent}; background: ${previewBackground.color};`;

  function pushPreview(): void {
    setColorPreview(colorScope, { accent: draft.accent, background: draft.background });
  }

  function closeAppearanceMenu(): void {
    appearanceMenu = null;
    draft = {};
    if ($colorPreview?.scope === colorScope) clearColorPreview();
  }

  /** 打开 ⋯ 菜单：锚在按钮右下角（视口像素，ContextMenu 内部除以缩放）。
   *  同一个按钮再点一次 = 收起（与记账/日记的齿轮同一条 toggle 语义）。 */
  function openAppearanceMenu(button: HTMLElement): void {
    if (appearanceMenu?.anchor === button) {
      closeAppearanceMenu();
      return;
    }
    const rect = button.getBoundingClientRect();
    const value = $appSettings.transfer?.relay ?? "";
    relayChoice = relayChoiceOf(value);
    relayCustom = relayChoice === "custom" ? value : "";
    appearanceMenu = { x: rect.right, y: rect.bottom + 6, anchor: button };
  }

  /** 主界面写 toolbox.accent / backgroundColor；工具子页写各自的 map（整份对象写） */
  function writeAccent(color: string): void {
    if (!activeToolId) {
      void setConfig("toolbox.accent", color);
      return;
    }
    void setConfig("toolbox.toolAccents", { ...($appSettings.toolbox.toolAccents ?? {}), [activeToolId]: color });
  }

  function writeBackground(color: string): void {
    if (!activeToolId) {
      void setConfig("toolbox.backgroundColor", color);
      return;
    }
    void setConfig("toolbox.toolBackgrounds", { ...($appSettings.toolbox.toolBackgrounds ?? {}), [activeToolId]: color });
  }

  /** 「默认」：主界面回应用默认；工具子页删掉自己这一键 → 跟主界面 */
  function resetAccent(): void {
    draft = { ...draft, accent: undefined };
    pushPreview();
    if (!activeToolId) {
      void setConfig("toolbox.accent", "");
      return;
    }
    const next = { ...($appSettings.toolbox.toolAccents ?? {}) };
    delete next[activeToolId];
    void setConfig("toolbox.toolAccents", next);
  }

  function resetBackground(): void {
    draft = { ...draft, background: undefined };
    pushPreview();
    if (!activeToolId) {
      void setConfig("toolbox.backgroundColor", defaultSettings.toolbox.backgroundColor);
      return;
    }
    const next = { ...($appSettings.toolbox.toolBackgrounds ?? {}) };
    delete next[activeToolId];
    void setConfig("toolbox.toolBackgrounds", next);
  }

  /** 主题色：走全应用统一的取色盘（v0.8.6 需求 11），拖动 = 草稿 + 预览 */
  function openAccentPanel(anchor: HTMLElement): void {
    openColorPicker({
      key: `${colorScope}:accent`,
      color: accentShown,
      anchor,
      onPreview: (color) => {
        draft = { ...draft, accent: color };
        pushPreview();
      },
      onConfirm: (color) => {
        draft = { ...draft, accent: color };
        pushPreview();
        writeAccent(color);
      },
      onCancel: cancelAccent
    });
  }

  function openBackgroundPanel(anchor: HTMLElement): void {
    openColorPicker({
      key: `${colorScope}:background`,
      color: backgroundShown,
      anchor,
      onPreview: (color) => pickBackground(color),
      onConfirm: (color) => {
        pickBackground(color);
        writeBackground(color);
      },
      onCancel: cancelBackground
    });
  }

  function pickBackground(color: string): void {
    draft = { ...draft, background: color };
    pushPreview();
  }

  /** 预设色块：离散选择，单击即落盘（只有取色器走「草稿 → 保存」） */
  function applyPresetBackground(color: string): void {
    draft = { ...draft, background: undefined };
    if ($colorPreview?.scope === colorScope) clearColorPreview();
    writeBackground(color);
  }

  function cancelAccent(): void {
    draft = { ...draft, accent: undefined };
    pushPreview();
  }

  function cancelBackground(): void {
    draft = { ...draft, background: undefined };
    pushPreview();
  }

  // ---- 卡片右键 / 长按：固定与取消固定 ----
  function openCardMenuAt(x: number, y: number, tool: ToolDefinition): void {
    cardMenu = { tool, x, y };
  }

  function handleCardContext(event: MouseEvent, tool: ToolDefinition): void {
    // 触摸长按后 Chromium 会补发一个 contextmenu：长按已经开过菜单，这里去重
    if (isLongPressSuppressed()) return;
    event.preventDefault();
    event.stopPropagation();
    openCardMenuAt(event.clientX, event.clientY, tool);
  }

  function togglePin(tool: ToolDefinition): void {
    const pinned = pinnedIds.includes(tool.id);
    cardMenu = null;
    void setToolPinned(tool.id, !pinned);
  }

  // 工具详情是工具箱整页里的一个层级（不占历史栈）：返回键先退回工具列表，
  // 再按一次才轮到历史栈把整页弹掉。
  const backGuard = createBackGuard();
  $: backGuard(activeTool !== null, backFromTool);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="toolbox-view" style={toolboxStyle} on:click|stopPropagation>
  {#if activeTool}
    <!-- 子视图的头部（v0.8.6 需求 6）：与普通页面同构——**左侧 = 返回箭头 + 图标 + 工具名，
         右侧 = ⋯ 菜单**。返回箭头两端都显示、不受系统设置的「移动端返回键」影响，
         所以**不能复用 `MobileBack`**（它按 features.mobileBack 在桌面隐藏）。 -->
    <header class="toolbox-sub-bar">
      <span class="toolbox-sub-bar-title">
        <button class="toolbox-icon-button" type="button" title="返回工具箱" aria-label="返回工具箱" on:click={backFromTool}>
          <ArrowLeft size={20} />
        </button>
        <!-- 图标口径与我的一天 / 日记同一处（PAGE_HEADER_ICON_SIZE，v0.8.7 追加需求 1） -->
        <span class="toolbox-header-icon"><svelte:component this={activeTool.icon} size={PAGE_HEADER_ICON_SIZE} /></span>
        <strong class="toolbox-header-title">{activeTool.name}</strong>
      </span>
      <span class="toolbox-sub-bar-actions">
        <button
          class="toolbox-icon-button"
          type="button"
          title="外观"
          aria-label="外观"
          on:click={(event) => openAppearanceMenu(event.currentTarget)}
        >
          <MoreHorizontal size={19} />
        </button>
      </span>
    </header>
    {#await loadToolModule(activeTool)}
      <p class="toolbox-empty">正在打开…</p>
    {:then module}
      <svelte:component this={module.default} />
    {:catch error}
      <!-- 懒加载的 chunk 拉不到（离线 / 缓存出问题）时要说一声：`loadToolModule`
           失败不留缓存，返回后再点一次就是一次真正的重试。 -->
      <p class="toolbox-empty">打开失败：{String(error)}</p>
    {/await}
  {:else}
    <header class="toolbox-header">
      <MobileBack />
      <span class="toolbox-header-icon"><Toolbox size={PAGE_HEADER_ICON_SIZE} /></span>
      <strong class="toolbox-header-title">工具箱</strong>
      <!-- 主界面同样有 ⋯ 菜单（v0.8.7 追加需求）：调工具箱自己的主题色 / 背景色，
           与工具子页各自独立 -->
      <span class="toolbox-header-actions">
        <button
          class="toolbox-icon-button"
          type="button"
          title="外观"
          aria-label="外观"
          on:click={(event) => openAppearanceMenu(event.currentTarget)}
        >
          <MoreHorizontal size={19} />
        </button>
      </span>
    </header>
    <div class="toolbox-list">
      {#each tools as tool (tool.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <button
          class="toolbox-card"
          class:toolbox-card-pinned={pinnedIds.includes(tool.id)}
          type="button"
          on:click={() => openTool(tool)}
          on:contextmenu={(event) => handleCardContext(event, tool)}
          use:longpress={(pos) => openCardMenuAt(pos.x, pos.y, tool)}
        >
          <span class="toolbox-card-icon"><svelte:component this={tool.icon} size={22} /></span>
          <span class="toolbox-card-text">
            <strong>{tool.name}</strong>
            <span>{tool.desc}</span>
          </span>
          {#if pinnedIds.includes(tool.id)}
            <span class="toolbox-card-pin" title="已固定到侧栏"><Pin size={13} /></span>
          {/if}
        </button>
      {:else}
        <p class="toolbox-empty">这一端暂时没有可用的工具。</p>
      {/each}
    </div>
  {/if}
</section>

{#if appearanceMenu}
  {@const menu = appearanceMenu}
  <!-- ⋯ 菜单与普通页面（ListMenu）同一套结构与同一套定位：**点锚定**——贴在按钮正下方、
       右缘对齐按钮右缘（x=rect.right / y=rect.bottom，ContextMenu 内部除缩放）。
       结构也照抄那边：配置在上 → 分割线 → UI颜色 / 背景颜色在下。 -->
  <ContextMenu
    x={menu.x}
    y={menu.y}
    xAlign="right"
    anchor={menu.anchor}
    minWidth={228}
    onClose={closeAppearanceMenu}
  >
    {#if activeTool?.id === "transfer"}
      <!-- relay 服务（v0.8.5 需求 31）：独立一级菜单项 + 二级钻取，与「移动到分组」同一套机制——
           MenuItem 的 submenu 插槽，移动端自动钻入、桌面自动贴边翻转。
           relay 在 go_online 时固化进端点，保存后 store 自己重新上线；提示只说「下次上线时生效」。 -->
      <MenuItem icon={Radio} label="relay 服务" active={relayChoice !== "default"}>
        <div slot="submenu" class="submenu-list relay-submenu">
          <MenuItem
            label="复用同步配置"
            checkable
            disabled={!syncConfigured}
            active={relayChoice === "follow"}
            onSelect={() => pickRelayChoice("follow")}
          />
          <MenuItem
            label="使用默认服务"
            checkable
            active={relayChoice === "default"}
            onSelect={() => pickRelayChoice("default")}
          />
          <MenuItem
            label="自定义服务"
            checkable
            active={relayChoice === "custom"}
            onSelect={() => pickRelayChoice("custom")}
          />
          {#if relayChoice === "custom"}
            <!-- 输入框自己 stopPropagation：否则点在它上面会被菜单的「点外部关闭」收掉 -->
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div class="relay-custom" on:click|stopPropagation>
              <input
                class="relay-input"
                value={relayCustom}
                placeholder="https://relay.example.com"
                spellcheck="false"
                on:input={(event) => (relayCustom = event.currentTarget.value)}
                on:blur={commitRelayCustom}
                on:keydown={(event) => { if (event.key === "Enter") (event.target as HTMLInputElement).blur(); }}
              />
            </div>
          {/if}
          <div class="relay-hint">下次上线时生效</div>
        </div>
      </MenuItem>
      <MenuSeparator />
    {/if}

    <div class="menu-section-title">UI颜色</div>
    <div class="ui-color-row">
      <button
        class="ui-color-picker"
        type="button"
        title="工具页的主题色"
        data-color-anchor
        on:click|stopPropagation={(event) => openAccentPanel(event.currentTarget)}
      >
        <span style={`--swatch: ${accentShown}`}></span>
      </button>
      <span class="ui-color-value">{accentShown}</span>
      <button class="menu-action-button" type="button" on:click={resetAccent}>默认</button>
    </div>

    <div class="menu-section-title">背景颜色</div>
    <div class="color-grid">
      {#each presets as preset, index (preset.name + index)}
        <button
          type="button"
          title={preset.name}
          class:active={backgroundShown === preset.color}
          style={`--swatch: ${preset.color}; --accent-color: ${preset.color}`}
          on:click={() => applyPresetBackground(preset.color)}
        ></button>
      {/each}
      <button
        type="button"
        class="palette-button"
        title="自定义颜色"
        data-color-anchor
        on:click|stopPropagation={(event) => openBackgroundPanel(event.currentTarget)}
      ></button>
      <button
        type="button"
        class="reset-bg-button"
        title="恢复默认背景色"
        on:click={resetBackground}
      >
        <RotateCcw size={14} />
      </button>
    </div>
  </ContextMenu>
{/if}

{#if cardMenu}
  {@const menu = cardMenu}
  <ContextMenu x={menu.x} y={menu.y} minWidth={168} onClose={() => (cardMenu = null)}>
    {#if pinnedIds.includes(menu.tool.id)}
      <MenuItem icon={PinOff} label="取消固定" onSelect={() => togglePin(menu.tool)} />
    {:else}
      <MenuItem icon={Pin} label="固定此工具" onSelect={() => togglePin(menu.tool)} />
    {/if}
  </ContextMenu>
{/if}

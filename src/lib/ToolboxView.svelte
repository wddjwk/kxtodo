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
  import { ArrowLeft, MoreHorizontal, Palette, Pin, PinOff, RotateCcw, Toolbox } from "@lucide/svelte";
  import MobileBack from "./MobileBack.svelte";
  import { appSettings } from "./stores";
  import { setConfig } from "./actions";
  import { createBackGuard } from "./platform";
  import { setToolPinned } from "./actions";
  import { navToolId } from "./nav";
  import { openToolboxTool, resetToolRoute, toolRoute } from "./tools/navigation";
  import { availableTools, loadToolModule, type ToolDefinition } from "./tools/registry";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import ColorDraftActions from "./ColorDraftActions.svelte";
  import { clearColorPreview, accentWithPreview, backgroundWithPreview, colorPreview, setColorPreview } from "./colorPreview";
  import { toolboxAccent } from "./styles";
  import { defaultBackground, themePresets } from "./defaults";
  import { applyRelay, transferState } from "./transferStore";

  let cardMenu: { tool: ToolDefinition; x: number; y: number } | null = null;

  $: tools = availableTools();
  $: activeTool = $toolRoute === null ? null : (tools.find((tool) => tool.id === $toolRoute) ?? null);

  // ---- relay 服务（v0.8.5 需求 31）：跟着传输工具页的 ⋯ 菜单走 ----
  // core 的 transfer.relay 语义：空 = 跟 p2p 同步；`disabled` = 不用 relay；其它 = 自部署地址
  let relayMode: "follow" | "disabled" | "custom" = "follow";
  let relayCustom = "";

  function relayModeOf(value: string): "follow" | "disabled" | "custom" {
    if (value === "") return "follow";
    if (value === "disabled") return "disabled";
    return "custom";
  }

  function pickRelayMode(mode: "follow" | "disabled" | "custom"): void {
    relayMode = mode;
    if (mode === "follow") void applyRelay("");
    else if (mode === "disabled") void applyRelay("disabled");
    // 「自定义」等用户把地址填完再保存（下面那个输入框失焦 / 回车）
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

  // ---- 子视图的 ⋯ 菜单：工具页的外观（背景色 / 主题色，走需求 9 的「草稿 → 保存」）----
  const TOOLBOX_SCOPE = "toolbox";
  let appearanceMenu: { x: number; y: number } | null = null;
  let draft: { accent?: string; background?: string } = {};
  let customPicker: HTMLInputElement;
  $: toolboxAccentValue = toolboxAccent($appSettings.toolbox);
  $: toolboxBackgroundValue = $appSettings.toolbox.backgroundColor || defaultBackground.color;
  $: accentShown = draft.accent ?? toolboxAccentValue;
  $: backgroundShown = draft.background ?? toolboxBackgroundValue;
  $: accentDirty = draft.accent !== undefined && draft.accent !== toolboxAccentValue;
  $: backgroundDirty = draft.background !== undefined && draft.background !== toolboxBackgroundValue;
  $: presets = $appSettings.appearance.themePresets.length ? $appSettings.appearance.themePresets : themePresets;
  // 预览（需求 9）：拖色盘时整页立刻跟着变，保存才落盘；菜单一关就回退
  $: previewAccent = accentWithPreview($colorPreview, TOOLBOX_SCOPE, toolboxAccentValue);
  $: previewBackground = backgroundWithPreview($colorPreview, TOOLBOX_SCOPE, { color: toolboxBackgroundValue });
  $: toolboxStyle = `--accent: ${previewAccent}; background: ${previewBackground.color};`;

  function pushPreview(): void {
    setColorPreview(TOOLBOX_SCOPE, { accent: draft.accent, background: draft.background });
  }

  function closeAppearanceMenu(): void {
    appearanceMenu = null;
    draft = {};
    if ($colorPreview?.scope === TOOLBOX_SCOPE) clearColorPreview();
  }

  /** 打开 ⋯ 菜单：顺手把 relay 三态与自定义地址按当前设置填好 */
  function openAppearanceMenu(event: MouseEvent): void {
    const value = $appSettings.transfer?.relay ?? "";
    relayMode = relayModeOf(value);
    relayCustom = relayMode === "custom" ? value : "";
    appearanceMenu = { x: event.clientX, y: event.clientY };
  }

  function pickAccent(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) return;
    draft = { ...draft, accent: target.value };
    pushPreview();
  }

  function pickBackground(color: string): void {
    draft = { ...draft, background: color };
    pushPreview();
  }

  /** 预设色块：离散选择，单击即落盘（只有取色器走「草稿 → 保存」） */
  function applyPresetBackground(color: string): void {
    draft = { ...draft, background: undefined };
    if ($colorPreview?.scope === TOOLBOX_SCOPE) clearColorPreview();
    void setConfig("toolbox.backgroundColor", color);
  }

  function pickBackgroundFromInput(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) pickBackground(target.value);
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
    <!-- 子视图的头部（需求 7）：左边「图标 + 名称」（与工具箱主界面、我的一天同一格式），
         右边两枚按钮——返回工具箱 + ⋯ 外观菜单。工具该拿到一块干净的画布，
         「返回工具箱」占一整行太扎眼。 -->
    <header class="toolbox-sub-bar">
      <span class="toolbox-sub-bar-title">
        <span class="toolbox-header-icon"><svelte:component this={activeTool.icon} size={22} /></span>
        <strong class="toolbox-header-title">{activeTool.name}</strong>
      </span>
      <span class="toolbox-sub-bar-actions">
        <button class="toolbox-icon-button" type="button" title="返回工具箱" aria-label="返回工具箱" on:click={backFromTool}>
          <ArrowLeft size={19} />
        </button>
        <button
          class="toolbox-icon-button"
          type="button"
          title="外观"
          aria-label="外观"
          on:click={openAppearanceMenu}
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
      <span class="toolbox-header-icon"><Toolbox size={26} /></span>
      <strong class="toolbox-header-title">工具箱</strong>
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
  <ContextMenu x={menu.x} y={menu.y} minWidth={228} onClose={closeAppearanceMenu}>
    <div class="menu-section-title">主题颜色</div>
    <div class="ui-color-row">
      <label class="ui-color-picker" title="工具页的主题色">
        <span style={`--swatch: ${accentShown}`}></span>
        <!-- input 与 change 都只改草稿并预览：整页与色块一起变，保存才落盘（需求 9） -->
        <input type="color" value={accentShown} on:input={pickAccent} on:change={pickAccent} />
      </label>
      <span class="ui-color-value">{accentShown}</span>
      <button
        class="menu-action-button"
        type="button"
        on:click={() => { draft = { ...draft, accent: undefined }; pushPreview(); void setConfig("toolbox.accent", ""); }}
      >默认</button>
    </div>
    {#if accentDirty}
      <ColorDraftActions
        onSave={() => { const value = draft.accent; if (value !== undefined) void setConfig("toolbox.accent", value); }}
        onCancel={cancelAccent}
      />
    {/if}

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
      <button type="button" class="palette-button" title="自定义颜色" on:click={() => customPicker?.click()}></button>
      <button
        type="button"
        class="reset-bg-button"
        title="恢复默认背景色"
        on:click={() => { draft = { ...draft, background: undefined }; pushPreview(); void setConfig("toolbox.backgroundColor", defaultBackground.color); }}
      >
        <RotateCcw size={14} />
      </button>
    </div>
    {#if backgroundDirty}
      <ColorDraftActions
        onSave={() => { const color = draft.background; if (color !== undefined) void setConfig("toolbox.backgroundColor", color); }}
        onCancel={cancelBackground}
      />
    {/if}
    <input
      bind:this={customPicker}
      class="hidden-file"
      type="color"
      value={backgroundShown}
      on:change={pickBackgroundFromInput}
    />

    {#if activeTool?.id === "transfer"}
      <!-- relay 服务（v0.8.5 需求 31）：为以后自部署 relay 做准备。
           relay 在 go_online 时固化进端点，保存后 store 会自己重新上线。 -->
      <div class="menu-section-title">relay 服务</div>
      <div class="relay-options">
        <label class="relay-row">
          <input type="radio" name="relay-mode" checked={relayMode === "follow"} on:change={() => pickRelayMode("follow")} />
          <span>跟随同步设置</span>
        </label>
        <label class="relay-row">
          <input type="radio" name="relay-mode" checked={relayMode === "disabled"} on:change={() => pickRelayMode("disabled")} />
          <span>禁用（只走直连）</span>
        </label>
        <label class="relay-row">
          <input type="radio" name="relay-mode" checked={relayMode === "custom"} on:change={() => pickRelayMode("custom")} />
          <span>自定义地址</span>
        </label>
      </div>
      {#if relayMode === "custom"}
        <input
          class="relay-input"
          value={relayCustom}
          placeholder="https://relay.example.com"
          spellcheck="false"
          on:input={(event) => (relayCustom = event.currentTarget.value)}
          on:blur={commitRelayCustom}
          on:keydown={(event) => { if (event.key === "Enter") (event.target as HTMLInputElement).blur(); }}
        />
      {/if}
      <div class="relay-hint">
        {$transferState.online ? "传输助手在线：保存后自动重新上线" : "传输助手未上线：下次上线时生效"}
      </div>
    {/if}
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

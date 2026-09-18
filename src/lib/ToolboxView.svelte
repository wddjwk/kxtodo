<script lang="ts">
  /**
   * 工具箱的壳（两端整页视图，v0.7.5 起桌面也有）：注册表驱动——
   * 列表页画 `availableTools()`（平台过滤在 src/lib/tools/registry.ts 里做），
   * 点开一个工具就懒加载它的子视图组件挂进来，返回按钮收回列表。
   * 壳不认识任何具体工具：新增工具只动注册表与组件文件。
   *
   * v0.8.3 加了两件事：
   * - **固定此工具**：卡片右键 / 长按出菜单，把工具钉进侧栏固定区（`tool:<id>` 行）；
   * - **直达路由**：从钉住的行进来时（`toolRoute.fromPin`）直接落在子视图上，
   *   返回时连工具箱整页一起收——停在列表上等于给用户一个他没来过的页面。
   */
  import { ChevronLeft, Pin, PinOff, Toolbox } from "@lucide/svelte";
  import type { Component } from "svelte";
  import MobileBack from "./MobileBack.svelte";
  import { appSettings, showToast, toolboxOpen } from "./stores";
  import { createBackGuard, isMobile, showMobileList } from "./platform";
  import { setToolPinned } from "./actions";
  import { navToolId } from "./nav";
  import { resetToolRoute, toolRoute } from "./tools/navigation";
  import { availableTools, type ToolDefinition } from "./tools/registry";
  import { longpress, isLongPressSuppressed } from "./longpress";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";

  let activeToolId: string | null = null;
  let toolComponent: Component | null = null;
  let loadFailed = false;
  let cardMenu: { tool: ToolDefinition; x: number; y: number } | null = null;

  $: tools = availableTools();
  $: activeTool = tools.find((tool) => tool.id === activeToolId) ?? null;
  $: pinnedIds = $appSettings.appearance.navItems
    .filter((id) => id.startsWith("tool:"))
    .map((id) => id.slice("tool:".length));
  // 列表一露头就把各工具的 chunk 取回来：工具箱里的组件都是小件（注册表约定），
  // 而「点一下先看到正在打开…」是纯亏——用户看到的是没反馈，省下的是几 KB。
  // 放在这里而不是启动时：启动包不受影响，进工具箱又一定是瞬开。
  $: if (!activeTool) void Promise.all(tools.map((tool) => tool.load().catch(() => undefined)));

  // 从钉住的行进来：直达子视图（路由变了也要跟上，比如用户在侧栏换了另一个钉住的行）
  $: if ($toolRoute.id && $toolRoute.id !== activeToolId) {
    const target = tools.find((tool) => tool.id === $toolRoute.id);
    if (target) void openTool(target);
  }

  async function openTool(tool: ToolDefinition): Promise<void> {
    activeToolId = tool.id;
    toolComponent = null;
    loadFailed = false;
    // 首次点开（预取还没回来）时 chunk 请求可能失败：**不能让它永远停在「正在打开…」**，
    // 重试一次再失败就明确报错给用户。
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const module = await tool.load();
        if (activeToolId === tool.id) toolComponent = module.default;
        return;
      } catch (error) {
        if (attempt === 1 && activeToolId === tool.id) {
          loadFailed = true;
          showToast(`打开「${tool.name}」失败：${String(error)}`);
        }
      }
    }
  }

  function clearToolView(): void {
    activeToolId = null;
    toolComponent = null;
    loadFailed = false;
  }

  /** 返回：从钉住的行进来的连整页一起收，否则只退回工具列表 */
  function backFromTool(): void {
    if ($toolRoute.fromPin) {
      resetToolRoute();
      clearToolView();
      if ($isMobile) showMobileList();
      else toolboxOpen.set(false);
      return;
    }
    clearToolView();
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
  $: backGuard(activeToolId !== null, backFromTool);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="toolbox-view" on:click|stopPropagation>
  <header class="toolbox-header">
    <MobileBack />
    <span class="toolbox-header-icon"><Toolbox size={26} /></span>
    <strong class="toolbox-header-title">工具箱</strong>
  </header>

  {#if !activeTool}
    <div class="toolbox-list">
      {#each tools as tool (tool.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <button
          class="toolbox-card"
          class:toolbox-card-pinned={pinnedIds.includes(tool.id)}
          type="button"
          on:click={() => void openTool(tool)}
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
  {:else}
    <div class="toolbox-sub-host">
      <button class="toolbox-sub-back" type="button" on:click={backFromTool}>
        <ChevronLeft size={18} /> 返回工具箱
      </button>
      {#if toolComponent}
        <svelte:component this={toolComponent} />
      {:else if loadFailed}
        <p class="toolbox-empty">打开失败，请返回后重试。</p>
      {:else}
        <p class="toolbox-empty">正在打开…</p>
      {/if}
    </div>
  {/if}
</section>

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

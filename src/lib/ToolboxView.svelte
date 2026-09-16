<script lang="ts">
  /**
   * 工具箱的壳（两端整页视图，v0.7.5 起桌面也有）：注册表驱动——
   * 列表页画 `availableTools()`（平台过滤在 src/lib/tools/registry.ts 里做），
   * 点开一个工具就懒加载它的子视图组件挂进来，返回按钮收回列表。
   * 壳不认识任何具体工具：新增工具只动注册表与组件文件。
   */
  import { ChevronLeft, Toolbox } from "@lucide/svelte";
  import type { Component } from "svelte";
  import MobileBack from "./MobileBack.svelte";
  import { showToast } from "./stores";
  import { createBackGuard } from "./platform";
  import { availableTools, type ToolDefinition } from "./tools/registry";

  let activeToolId: string | null = null;
  let toolComponent: Component | null = null;
  let loadFailed = false;

  $: tools = availableTools();
  $: activeTool = tools.find((tool) => tool.id === activeToolId) ?? null;
  // 列表一露头就把各工具的 chunk 取回来：工具箱里的组件都是小件（注册表约定），
  // 而「点一下先看到正在打开…」是纯亏——用户看到的是没反馈，省下的是几 KB。
  // 放在这里而不是启动时：启动包不受影响，进工具箱又一定是瞬开。
  $: if (!activeTool) void Promise.all(tools.map((tool) => tool.load().catch(() => undefined)));

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

  function backToList(): void {
    activeToolId = null;
    toolComponent = null;
    loadFailed = false;
  }

  // 工具详情是工具箱整页里的一个层级（不占历史栈）：返回键先退回工具列表，
  // 再按一次才轮到历史栈把整页弹掉。
  const backGuard = createBackGuard();
  $: backGuard(activeToolId !== null, backToList);
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
        <button class="toolbox-card" type="button" on:click={() => void openTool(tool)}>
          <span class="toolbox-card-icon"><svelte:component this={tool.icon} size={22} /></span>
          <span class="toolbox-card-text">
            <strong>{tool.name}</strong>
            <span>{tool.desc}</span>
          </span>
        </button>
      {:else}
        <p class="toolbox-empty">这一端暂时没有可用的工具。</p>
      {/each}
    </div>
  {:else}
    <div class="toolbox-sub-host">
      <button class="toolbox-sub-back" type="button" on:click={backToList}>
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

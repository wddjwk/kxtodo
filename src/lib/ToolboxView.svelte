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
  import { availableTools, type ToolDefinition } from "./tools/registry";

  let activeToolId: string | null = null;
  let toolComponent: Component | null = null;

  $: tools = availableTools();
  $: activeTool = tools.find((tool) => tool.id === activeToolId) ?? null;

  async function openTool(tool: ToolDefinition): Promise<void> {
    activeToolId = tool.id;
    toolComponent = null;
    const module = await tool.load();
    // 加载期间用户可能已经返回列表或换了工具：只有还停在它身上才挂
    if (activeToolId === tool.id) toolComponent = module.default;
  }

  function backToList(): void {
    activeToolId = null;
    toolComponent = null;
  }
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
      {:else}
        <p class="toolbox-empty">正在打开…</p>
      {/if}
    </div>
  {/if}
</section>

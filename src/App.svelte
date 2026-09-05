<script lang="ts">
  import { onMount } from "svelte";
  import { matchesShortcut } from "./lib/shortcuts";
  import { buildAppShellStyle, buildMobileShellStyle } from "./lib/styles";
  import {
    appSettings, appState, showSettings, searchQuery,
    taskEmojiPicker, editorTaskId, appVersion, showToast,
    isHydrated,
    hydrate as hydrateStores
  } from "./lib/stores";
  import { replaceTaskEmojis, selectNode as selectNodeAction } from "./lib/actions";
  import { isMobile, mobileView, startMobileRouter } from "./lib/platform";
  import { startAutoSync } from "./lib/syncRunner";
  import { revealMainWindow } from "./lib/backend";
  import { checkForUpdate } from "./lib/updater";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toast from "./lib/Toast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Workspace from "./lib/Workspace.svelte";
  import ToolboxView from "./lib/ToolboxView.svelte";
  import SettingsDrawer from "./lib/SettingsDrawer.svelte";
  import IconPicker from "./lib/IconPicker.svelte";

  let sidebarRef: Sidebar;
  let workspaceRef: Workspace;

  $: appShellStyle = $isMobile
    ? buildMobileShellStyle($appSettings.appearance)
    : buildAppShellStyle($appSettings.appearance);

  $: emojiPickerTask = $taskEmojiPicker
    ? $appState.tasks.find((t) => t.id === $taskEmojiPicker?.taskId) ?? null
    : null;

  // 移动端没有调度引擎：若选中节点是"定时任务"，水合后一次性重定向到我的一天
  //（my-day 是合法系统节点，条件随即自清，不会成环）
  $: if ($isMobile && $isHydrated && $appState.selectedNodeId === "scheduled") {
    void selectNodeAction("my-day");
  }

  onMount(() => {
    // 移动端历史栈路由：必须在模块全部初始化后挂载（platform 与 stores 循环依赖）
    startMobileRouter();
    // 调度引擎在 Rust Background Host 中运行，前端不再持有调度循环。
    void hydrateStores();
    void revealMainWindow();
    // 自动同步循环（全平台：配对后按 intervalSeconds 周期 pull+push）
    startAutoSync();
    window.addEventListener("keydown", handleShortcut);
    // 启动后静默检查一次更新（全平台，可在设置关闭）
    const timer = window.setTimeout(() => {
      if ($appSettings.updates.autoCheck && $appVersion) {
        void checkForUpdate($appVersion).then((result) => {
          if (result.status === "available") {
            showToast(`发现新版本 v${result.info.version}，可在设置中更新`, 6000);
          }
        });
      }
    }, 5000);
    return () => {
      window.removeEventListener("keydown", handleShortcut);
      window.clearTimeout(timer);
    };
  });

  function closeOverlays(): void {
    // sidebar 的一次性抑制标志只保护 sidebar 自身浮层，不应阻断设置抽屉关闭
    if (!sidebarRef?.shouldSuppressClose()) {
      sidebarRef?.closeOverlays();
    }
    workspaceRef?.closeOverlays();
    showSettings.set(false);
  }

  function handleShortcut(event: KeyboardEvent): void {
    if ($editorTaskId) return;
    if (matchesShortcut(event, $appSettings.shortcuts.focusSearch)) {
      event.preventDefault();
      sidebarRef?.focusSearch();
    } else if (matchesShortcut(event, $appSettings.shortcuts.newTask)) {
      event.preventDefault();
      workspaceRef?.focusComposer();
    } else if (matchesShortcut(event, $appSettings.shortcuts.openSettings)) {
      event.preventDefault();
      showSettings.update((v) => !v);
    }
  }

  function handleEmojiPick(emoji: string): void {
    const target = $taskEmojiPicker;
    if (!target) return;
    const { taskId, index } = target;
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      const emojis = [...task.emojis];
      if (index >= 0 && index < emojis.length) {
        emojis[index] = emoji;
      } else {
        emojis.push(emoji);
      }
      void replaceTaskEmojis(taskId, emojis);
    }
    taskEmojiPicker.set(null);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="app-shell"
  class:mobile={$isMobile}
  class:view-list={$isMobile && $mobileView === "list"}
  class:view-content={$isMobile && $mobileView === "content"}
  class:view-toolbox={$isMobile && $mobileView === "toolbox"}
  class:view-settings={$isMobile && $showSettings}
  style={appShellStyle}
  on:click={closeOverlays}
>
  {#if !$isMobile}
    <TitleBar />
  {/if}

  <div class="layout">
    <Sidebar bind:this={sidebarRef} />

    <Workspace bind:this={workspaceRef} />

    {#if $isMobile && $mobileView === "toolbox"}
      <ToolboxView />
    {/if}

    {#if $showSettings}
      <!-- 抽屉外任意点击一律关闭：遮罩在抽屉下层，挡住背后容器的 stopPropagation -->
      <button class="settings-backdrop" aria-label="关闭设置" on:click={() => showSettings.set(false)}></button>
      <SettingsDrawer />
    {/if}
  </div>

  <Toast />

  {#if $editorTaskId}
    {#await import("./lib/editor/MarkdownEditorModal.svelte") then module}
      <svelte:component
        this={module.default}
        taskId={$editorTaskId}
        onClose={() => editorTaskId.set(null)}
        onOpenLink={(url) => workspaceRef?.openLinkUrl(url)}
      />
    {/await}
  {/if}

  {#if emojiPickerTask && $taskEmojiPicker}
    <IconPicker
      mode="emoji"
      selected={$taskEmojiPicker.index >= 0 ? (emojiPickerTask.emojis[$taskEmojiPicker.index] ?? "") : ""}
      onPick={handleEmojiPick}
      onClose={() => taskEmojiPicker.set(null)}
    />
  {/if}
</div>

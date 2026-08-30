<script lang="ts">
  import { onMount } from "svelte";
  import { matchesShortcut } from "./lib/shortcuts";
  import { buildAppShellStyle, buildMobileShellStyle } from "./lib/styles";
  import {
    appSettings, appState, showSettings, searchQuery,
    taskEmojiPicker, editorTaskId, appVersion, showToast,
    hydrate as hydrateStores
  } from "./lib/stores";
  import { replaceTaskEmojis } from "./lib/actions";
  import { isMobile, mobileView } from "./lib/platform";
  import { revealMainWindow } from "./lib/backend";
  import { checkForUpdate } from "./lib/updater";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toast from "./lib/Toast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Workspace from "./lib/Workspace.svelte";
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

  onMount(() => {
    // 调度引擎在 Rust Background Host 中运行，前端不再持有调度循环。
    void hydrateStores();
    void revealMainWindow();
    window.addEventListener("keydown", handleShortcut);
    // 启动后静默检查一次更新（桌面端，可关）
    const timer = window.setTimeout(() => {
      if (!$isMobile && $appSettings.updates.autoCheck && $appVersion) {
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
  style={appShellStyle}
  on:click={closeOverlays}
>
  {#if !$isMobile}
    <TitleBar />
  {/if}

  <div class="layout">
    <Sidebar bind:this={sidebarRef} />

    <Workspace bind:this={workspaceRef} />

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

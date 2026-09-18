<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { matchesShortcut } from "./lib/shortcuts";
  import { buildAppShellStyle, buildMobileShellStyle } from "./lib/styles";
  import {
    appSettings, appState, showSettings, searchQuery, isSearching,
    taskEmojiPicker, editorTaskId, appVersion, showToast,
    diaryOpen, diaryEditor, editorDraftNode, ledgerOpen, ledgerEditor, ledgerData, toolboxOpen,
    hydrate as hydrateStores
  } from "./lib/stores";
  import { replaceTaskEmojis, setConfig, syncNow as syncNowAction } from "./lib/actions";
  import { isMobile, mobileView, startMobileRouter } from "./lib/platform";
  import { imeViewport, startImeViewport } from "./lib/imeViewport";
  import { startAutoSync } from "./lib/syncRunner";
  import { revealMainWindow } from "./lib/backend";
  import { oversizedAvatarShrink } from "./lib/images";
  import { checkForUpdate } from "./lib/updater";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toast from "./lib/Toast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Workspace from "./lib/Workspace.svelte";
  import DiaryView from "./lib/DiaryView.svelte";
  import LedgerView from "./lib/LedgerView.svelte";
  import ToolboxView from "./lib/ToolboxView.svelte";
  import SettingsDrawer from "./lib/SettingsDrawer.svelte";

  let sidebarRef: Sidebar;
  let workspaceRef: Workspace;
  let diaryViewRef: DiaryView;
  let ledgerViewRef: LedgerView;

  $: appShellStyle = $isMobile
    ? buildMobileShellStyle($appSettings.appearance, $imeViewport)
    : buildAppShellStyle($appSettings.appearance);

  /** 日记占着主区域：桌面看 diaryOpen，移动端看历史栈驱动的 mobileView。 */
  $: diaryVisible = $isMobile ? $mobileView === "diary" : $diaryOpen;
  /** 记账占着主区域：与日记同一条口径（移动端看历史栈，桌面看 ledgerOpen）。 */
  $: ledgerVisible = $isMobile ? $mobileView === "ledger" : $ledgerOpen;
  /** 工具箱占着主区域（v0.7.5 起桌面也有）：同一条口径。 */
  $: toolboxVisible = $isMobile ? $mobileView === "toolbox" : $toolboxOpen;

  $: emojiPickerTask = $taskEmojiPicker
    ? $appState.tasks.find((t) => t.id === $taskEmojiPicker?.taskId) ?? null
    : null;

  onMount(() => {
    // 移动端历史栈路由：必须在模块全部初始化后挂载（platform 与 stores 循环依赖）
    startMobileRouter();
    // 移动端输入法跟随：键盘弹起时把 shell 收到键盘之上，页面不再被浏览器顶上去
    const stopImeViewport = get(isMobile) ? startImeViewport() : () => {};
    // 调度引擎在 Rust Background Host 中运行，前端不再持有调度循环。
    void hydrateStores().then(() => {
      // 旧安装的大头像一次性收缩（v0.8.1 之前移动端上传的没有压缩闸）：几 MB 的
      // dataURL 撑爆首帧资料缓存、拖大每轮同步——压到与新上传同口径（256px）再写回。
      const shrunk = oversizedAvatarShrink($appSettings.profile.avatar);
      void shrunk.then((avatar) => {
        if (avatar) void setConfig("profile.avatar", avatar);
      });
    });
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
      stopImeViewport();
    };
  });

  /** 懒加载失败时把对应的浮层状态收掉（不然浮层状态还挂着，点别处都进不来） */
  function dismissLazyFailure(): void {
    closeTaskEditor();
    diaryEditor.set(null);
    ledgerEditor.set(null);
    taskEmojiPicker.set(null);
  }

  function closeOverlays(): void {
    // sidebar 的一次性抑制标志只保护 sidebar 自身浮层，不应阻断设置抽屉关闭
    if (!sidebarRef?.shouldSuppressClose()) {
      sidebarRef?.closeOverlays();
    }
    workspaceRef?.closeOverlays();
    diaryViewRef?.closeOverlays();
    ledgerViewRef?.closeOverlays();
    showSettings.set(false);
  }

  function handleShortcut(event: KeyboardEvent): void {
    if ($editorTaskId || $editorDraftNode || $diaryEditor || $ledgerEditor) return;
    if (matchesShortcut(event, $appSettings.shortcuts.focusSearch)) {
      event.preventDefault();
      sidebarRef?.focusSearch();
    } else if (matchesShortcut(event, $appSettings.shortcuts.newTask)) {
      event.preventDefault();
      workspaceRef?.focusComposer();
    } else if (matchesShortcut(event, $appSettings.shortcuts.openSettings)) {
      event.preventDefault();
      showSettings.update((v) => !v);
    } else if (matchesShortcut(event, $appSettings.shortcuts.syncNow)) {
      if ($appSettings.features?.sync === false) return;
      event.preventDefault();
      void syncNowAction();
    }
  }

  /** 两种模式共用一个编辑器实例，关闭时两个 store 一起清 */
  function closeTaskEditor(): void {
    editorTaskId.set(null);
    editorDraftNode.set(null);
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
  class:view-diary={$isMobile && $mobileView === "diary"}
  class:view-ledger={$isMobile && $mobileView === "ledger"}
  class:diary-open={!$isMobile && $diaryOpen}
  class:ledger-open={!$isMobile && $ledgerOpen}
  class:toolbox-open={!$isMobile && $toolboxOpen}
  class:searching={$isSearching}
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

    {#if diaryVisible}
      <DiaryView bind:this={diaryViewRef} onOpenLink={(url, title) => workspaceRef?.openLinkUrl(url, title)} />
    {/if}

    {#if ledgerVisible}
      <LedgerView bind:this={ledgerViewRef} />
    {/if}

    {#if toolboxVisible}
      <ToolboxView />
    {/if}

    {#if $showSettings}
      <!-- 抽屉外任意点击一律关闭：遮罩在抽屉下层，挡住背后容器的 stopPropagation -->
      <button class="settings-backdrop" aria-label="关闭设置" on:click={() => showSettings.set(false)}></button>
      <SettingsDrawer />
    {/if}
  </div>

  <Toast />

  {#if $editorTaskId || $editorDraftNode}
    {#await import("./lib/editor/MarkdownEditorModal.svelte") then module}
      <svelte:component
        this={module.default}
        taskId={$editorTaskId ?? ""}
        draftNodeId={$editorDraftNode ?? ""}
        onClose={closeTaskEditor}
        onOpenLink={(url) => workspaceRef?.openLinkUrl(url)}
      />
    {:catch error}
      <!-- 懒加载的 chunk 拉不到（网络/缓存出问题）时**必须说一声并收掉浮层**：
           早先只写了 `then`，失败就是无限空白——用户点开什么都没有，也退不出去。 -->
      <div class="lazy-fallback" role="alert">
        打开失败：{String(error)}
        <button class="settings-button" type="button" on:click={dismissLazyFailure}>关闭</button>
      </div>
    {/await}
  {/if}

  {#if $diaryEditor}
    {#await import("./lib/diary/DiaryEditor.svelte") then module}
      <svelte:component
        this={module.default}
        target={$diaryEditor}
        onClose={() => diaryEditor.set(null)}
        onOpenLink={(url, title) => workspaceRef?.openLinkUrl(url, title)}
      />
    {:catch error}
      <!-- 懒加载的 chunk 拉不到（网络/缓存出问题）时**必须说一声并收掉浮层**：
           早先只写了 `then`，失败就是无限空白——用户点开什么都没有，也退不出去。 -->
      <div class="lazy-fallback" role="alert">
        打开失败：{String(error)}
        <button class="settings-button" type="button" on:click={dismissLazyFailure}>关闭</button>
      </div>
    {/await}
  {/if}

  {#if $ledgerEditor}
    {#await import("./lib/ledger/LedgerEditor.svelte") then module}
      <svelte:component
        this={module.default}
        target={$ledgerEditor}
        book={$ledgerData}
        onClose={() => ledgerEditor.set(null)}
      />
    {:catch error}
      <!-- 懒加载的 chunk 拉不到（网络/缓存出问题）时**必须说一声并收掉浮层**：
           早先只写了 `then`，失败就是无限空白——用户点开什么都没有，也退不出去。 -->
      <div class="lazy-fallback" role="alert">
        打开失败：{String(error)}
        <button class="settings-button" type="button" on:click={dismissLazyFailure}>关闭</button>
      </div>
    {/await}
  {/if}

  {#if emojiPickerTask && $taskEmojiPicker}
    <!-- 懒加载：IconPicker 静态引入了 emoji-picker-element（自带整份 emoji 数据库），
         而它是个低频对话框。挂在首屏链上就是白付一两百 KB 的 entry 体积。
         与上面三个编辑器同一套写法；Sidebar 里那处也必须一起改，否则又被拉回首屏。 -->
    {#await import("./lib/IconPicker.svelte") then module}
      <svelte:component
        this={module.default}
        mode="emoji"
        selected={$taskEmojiPicker.index >= 0 ? (emojiPickerTask.emojis[$taskEmojiPicker.index] ?? "") : ""}
        onPick={handleEmojiPick}
        onClose={() => taskEmojiPicker.set(null)}
      />
    {:catch error}
      <!-- 懒加载的 chunk 拉不到（网络/缓存出问题）时**必须说一声并收掉浮层**：
           早先只写了 `then`，失败就是无限空白——用户点开什么都没有，也退不出去。 -->
      <div class="lazy-fallback" role="alert">
        打开失败：{String(error)}
        <button class="settings-button" type="button" on:click={dismissLazyFailure}>关闭</button>
      </div>
    {/await}
  {/if}
</div>

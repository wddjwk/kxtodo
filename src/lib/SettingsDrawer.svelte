<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    appSettings, showSettings, showToast, showNotification, fileToDataUrl, appVersion,
    syncConnection, nextSyncAt
  } from "./stores";
  import { setConfig as setConfigAction } from "./actions";
  import {
    syncRegister as syncRegisterAction,
    syncLogin as syncLoginAction,
    syncStatus as syncStatusAction,
    syncNow as syncNowAction,
    syncUnpair as syncUnpairAction,
    syncProbe as syncProbeAction,
    syncDiscover as syncDiscoverAction,
    syncHistory as syncHistoryAction,
    syncHistoryRemove as syncHistoryRemoveAction,
    setSyncScopes,
    setSyncEnabled,
    type DiscoveredServer,
    type SyncHistoryEntry
  } from "./actions";
  import { checkForUpdate, startUpdate, updateProgress, type UpdateInfo } from "./updater";
  import { isMobile } from "./platform";
  import { caps } from "./capabilities";
  import { ArrowLeft, Eye, EyeOff, History, LoaderCircle, Search, X } from "@lucide/svelte";
  import {
    scalePercentValue, buildSettingsDrawerStyle, avatarStyle, avatarInitial
  } from "./styles";
  import {
    isTauriRuntime, pickImageFile, saveAvatarImage, avatarImageUrl
  } from "./backend";
  import { avatarCache, resolveAvatarSrc } from "./images";
  import Dropdown from "./Dropdown.svelte";
  import NumberField from "./NumberField.svelte";
  import type { Settings } from "./types";

  let avatarFileInput: HTMLInputElement;

  const notificationPositionOptions: Array<{ value: Settings["notifications"]["position"]; label: string }> = [
    { value: "bottom-right", label: "右下角" },
    { value: "top-right", label: "右上角" },
    { value: "bottom-left", label: "左下角" },
    { value: "top-left", label: "左上角" }
  ];

  $: drawerStyle = buildSettingsDrawerStyle($appSettings.appearance);
  $: resolvedAvatar = resolveAvatarSrc($appSettings.profile.avatar, $avatarCache);
  $: avStyle = avatarStyle(resolvedAvatar);
  $: avInitial = avatarInitial($appSettings.profile.displayName);

  function updateProfile(field: keyof Settings["profile"], value: string): void {
    void setConfigAction(`profile.${field}`, value);
  }

  function updateAppearance<K extends keyof Settings["appearance"]>(field: K, value: Settings["appearance"][K]): void {
    void setConfigAction(`appearance.${field}`, value);
  }

  function updateNotifications<K extends keyof Settings["notifications"]>(field: K, value: Settings["notifications"][K]): void {
    void setConfigAction(`notifications.${field}`, value);
  }

  function testNotification(): void {
    void showNotification("这是一条测试消息，会按当前时长自动隐藏。", {
      title: "KXToDo 通知",
      tone: "success"
    });
  }

  async function updateLifecycle<K extends keyof Settings["lifecycle"]>(field: K, value: Settings["lifecycle"][K]): Promise<void> {
    // 原生副作用由 Host 在 config.set 时同步应用（§3.6）。
    const ok = await setConfigAction(`lifecycle.${field}`, Boolean(value));
    if (!ok) {
      showToast("系统设置更新失败");
    }
  }

  function updateShortcut(field: keyof Settings["shortcuts"], value: string): void {
    void setConfigAction(`shortcuts.${field}`, value);
  }

  // ---- 数据同步 ----
  let syncForm = { serverUrl: "", username: "", secret: "", syncSettings: true, syncSchedules: false };
  let syncBusy = false;
  let syncStatus: import("./actions").SyncStatus | null = null;
  let syncFormPrefilled = false;
  let discovering = false;
  let discovered: DiscoveredServer[] = [];
  let discoveryOpen = false;
  let historyOpen = false;
  let history: SyncHistoryEntry[] = [];
  let historySecretsVisible = false;
  /** 秒级 ticker：只用于「下次同步」倒计时显示 */
  let clock = Date.now();
  let clockTimer: number | undefined;

  onMount(() => {
    clockTimer = window.setInterval(() => {
      clock = Date.now();
    }, 1000);
  });
  onDestroy(() => {
    if (clockTimer !== undefined) window.clearInterval(clockTimer);
  });

  // 配对 = 有服务器地址 + 有密码；enabled=false 只是「暂停」，配置全部保留
  $: syncPaired = Boolean($appSettings.sync?.serverUrl && $appSettings.sync?.secret);
  $: syncPaused = syncPaired && !$appSettings.sync?.enabled;
  $: connection = $syncConnection;
  $: connectionState = connection.online === null
    ? "unknown"
    : connection.online
      ? "online"
      : "offline";
  $: connectionText = connection.checking
    ? "探测中"
    : connection.online === null
      ? "未探测"
      : connection.online
        ? "已连接"
        : "已掉线";
  // 倒计时：让「自动同步间隔到底有没有生效」一眼可见
  $: nextSyncText = !syncPaired || syncPaused
    ? ""
    : $nextSyncAt === null
      ? ""
      : `下次同步 ${Math.max(0, Math.round(($nextSyncAt - clock) / 1000))}s`;

  let syncStatusLoaded = false;
  $: if ($showSettings && syncPaired && !syncStatusLoaded) {
    syncStatusLoaded = true;
    void refreshSyncStatus();
  }
  $: if (!syncPaired) syncStatusLoaded = false;
  // 未配对时预填：解除配对后地址与用户名仍在设置里，直接带出来省得重敲
  $: if (!syncFormPrefilled && !syncPaired) {
    syncFormPrefilled = true;
    syncForm.serverUrl = $appSettings.sync?.serverUrl || "";
    syncForm.username = $appSettings.sync?.username || $appSettings.profile.displayName || "";
  }

  /** 本地状态立即渲染，网络探测丢到后台（掉线时也不阻塞设置界面）。 */
  async function refreshSyncStatus(options: { probe?: boolean } = {}): Promise<void> {
    syncStatus = await syncStatusAction();
    if (options.probe === false) return;
    void syncProbeAction().then((status) => {
      if (status) syncStatus = status;
    });
  }

  /** 放大镜：UDP 广播/组播查询局域网内的 kxtodo-server，结果以下拉浮层展示。 */
  async function runDiscovery(): Promise<void> {
    if (discovering) return;
    discovering = true;
    discoveryOpen = true;
    try {
      const result = await syncDiscoverAction();
      if (result === null) {
        // 当前环境不支持（浏览器预览），actions 已提示过
        discoveryOpen = false;
        return;
      }
      discovered = result;
      if (result.length === 0) {
        showToast("局域网内没发现服务器（需在跑、同一局域网、监听 0.0.0.0）");
      }
    } finally {
      discovering = false;
    }
  }

  function pickDiscovered(server: DiscoveredServer): void {
    syncForm.serverUrl = server.url;
    discoveryOpen = false;
    discovered = [];
    showToast(`已填入 ${server.name || server.host}`);
  }

  async function toggleHistory(): Promise<void> {
    historyOpen = !historyOpen;
    if (historyOpen) await loadHistory();
  }

  async function loadHistory(): Promise<void> {
    history = (await syncHistoryAction()) ?? [];
  }

  function applyHistory(entry: SyncHistoryEntry): void {
    historyOpen = false;
    if (syncPaired) {
      showToast("当前已配对：先解除配对，再用历史账户登录");
      return;
    }
    syncForm.serverUrl = entry.serverUrl;
    syncForm.username = entry.username;
    syncForm.secret = entry.secret;
    showToast("已回填历史账户");
  }

  async function removeHistory(index: number): Promise<void> {
    history = (await syncHistoryRemoveAction(index)) ?? [];
  }

  async function registerSync(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      if (await syncRegisterAction(syncForm)) {
        await refreshSyncStatus();
      }
    } finally {
      syncBusy = false;
    }
  }

  async function loginSync(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      if (await syncLoginAction(syncForm)) {
        await refreshSyncStatus();
      }
    } finally {
      syncBusy = false;
    }
  }

  async function runSyncNow(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      await syncNowAction();
      await refreshSyncStatus({ probe: false });
    } finally {
      syncBusy = false;
    }
  }

  /** 暂停/恢复：只切开关，服务器地址与账户凭据全部保留。 */
  async function togglePause(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      await setSyncEnabled(!$appSettings.sync?.enabled);
      await refreshSyncStatus({ probe: false });
    } finally {
      syncBusy = false;
    }
  }

  async function unpairSync(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      if (await syncUnpairAction()) {
        syncStatus = null;
        syncFormPrefilled = false;
      }
    } finally {
      syncBusy = false;
    }
  }

  async function updateSyncScope(
    field: "syncData" | "syncSettings" | "syncSchedules",
    value: boolean
  ): Promise<void> {
    await setSyncScopes({ [field]: value });
    await refreshSyncStatus({ probe: false });
  }

  async function updateSyncInterval(seconds: number): Promise<void> {
    // 低于 5 秒按 5 秒生效（core 侧同样夹取）
    await setSyncScopes({ intervalSeconds: Math.max(5, Math.min(86400, Math.round(seconds))) });
    await refreshSyncStatus({ probe: false });
  }

  async function updateReconnectSeconds(seconds: number): Promise<void> {
    await setSyncScopes({ reconnectSeconds: Math.max(5, Math.min(86400, Math.round(seconds))) });
    await refreshSyncStatus({ probe: false });
  }

  // ---- 更新 ----
  let checkingUpdate = false;
  let pendingUpdate: UpdateInfo | null = null;
  let upToDate = false;
  let updateCheckError = "";

  $: progress = $updateProgress;
  $: updateBusy = progress.phase === "downloading" || progress.phase === "installing" || progress.phase === "restarting";

  async function checkUpdates(): Promise<void> {
    if (checkingUpdate || updateBusy) return;
    checkingUpdate = true;
    pendingUpdate = null;
    upToDate = false;
    updateCheckError = "";
    const result = await checkForUpdate($appVersion || "0.0.0");
    checkingUpdate = false;
    if (result.status === "up-to-date") {
      upToDate = true;
    } else if (result.status === "available") {
      pendingUpdate = result.info;
    } else {
      updateCheckError = result.message;
    }
  }

  async function downloadAndApply(): Promise<void> {
    if (!pendingUpdate || updateBusy) return;
    try {
      await startUpdate(pendingUpdate);
    } catch (error) {
      updateProgress.set({ phase: "failed", stage: "", percent: 0, message: String(error) });
    }
  }

  function updateAutoCheck(value: boolean): void {
    void setConfigAction("updates.autoCheck", value);
  }

  function updateFeature<K extends keyof Settings["features"]>(field: K, value: Settings["features"][K]): void {
    void setConfigAction(`features.${field}`, value);
  }

  async function uploadAvatar(): Promise<void> {
    // 移动端（无原生对话框）与浏览器一样走隐藏 <input type=file> → dataURL。
    if (!isTauriRuntime || !caps.nativeFileDialogs) {
      avatarFileInput.click();
      return;
    }
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      const filename = await saveAvatarImage(srcPath);
      const url = await avatarImageUrl(filename);
      avatarCache.update((map) => ({ ...map, [filename]: url }));
      void setConfigAction("profile.avatar", filename);
    } catch (error) {
      showToast(`头像上传失败：${String(error)}`);
    }
  }

  async function uploadAvatarFromInput(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      const avatar = await fileToDataUrl(target.files[0]);
      void setConfigAction("profile.avatar", avatar);
    } catch (error) {
      showToast(`头像读取失败：${String(error)}`);
    } finally {
      target.value = "";
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<aside class="settings-drawer" style={drawerStyle} on:click|stopPropagation>
  <div class="drawer-header">
    {#if $isMobile}
      <button class="mobile-back" type="button" aria-label="返回" on:click={() => showSettings.set(false)}><ArrowLeft size={24} /></button>
    {/if}
    <h2>设置</h2>
    {#if !$isMobile}
      <button type="button" on:click={() => showSettings.set(false)}>×</button>
    {/if}
  </div>

  <section>
    <h3>个人资料</h3>
    <div class="avatar-setting">
      <span class="avatar large" style={avStyle}>{$appSettings.profile.avatar ? "" : avInitial}</span>
      <button class="settings-button" type="button" on:click={uploadAvatar}>上传头像</button>
      <input bind:this={avatarFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadAvatarFromInput} />
    </div>
    <label class="settings-row">
      名字
      <input value={$appSettings.profile.displayName} on:input={(event) => updateProfile("displayName", event.currentTarget.value)} />
    </label>
    <label class="settings-row">
      邮箱
      <input value={$appSettings.profile.email} on:input={(event) => updateProfile("email", event.currentTarget.value)} />
    </label>
  </section>

  <section>
    <h3>显示与链接</h3>
    <!-- 界面缩放对移动端同样生效（CSS transform 缩放）；原生 setWebviewZoom
         仍由 backend.ts 的 caps.desktop 门控在移动端 no-op。 -->
    <div class="settings-row number-row">
      <span>界面缩放</span>
      <NumberField
        ariaLabel="界面缩放"
        suffix="%"
        min={50}
        max={150}
        live={true}
        value={scalePercentValue($appSettings.appearance.uiScale)}
        onCommit={(v) => updateAppearance("uiScale", v / 100)}
      />
    </div>
    <div class="settings-row number-row">
      <span>UI 字号</span>
      <NumberField
        ariaLabel="UI 字号"
        suffix="px"
        min={14}
        max={22}
        live={true}
        value={$appSettings.appearance.uiFontSize}
        onCommit={(v) => updateAppearance("uiFontSize", v)}
      />
    </div>
    <div class="settings-row number-row">
      <span>Markdown 字号</span>
      <NumberField
        ariaLabel="Markdown 字号"
        suffix="px"
        min={14}
        max={26}
        live={true}
        value={$appSettings.appearance.markdownFontSize}
        onCommit={(v) => updateAppearance("markdownFontSize", v)}
      />
    </div>
    <div class="settings-row number-row">
      <span>编辑器字号</span>
      <NumberField
        ariaLabel="编辑器字号"
        suffix="px"
        min={14}
        max={26}
        live={true}
        value={$appSettings.appearance.editorFontSize}
        onCommit={(v) => updateAppearance("editorFontSize", v)}
      />
    </div>
    <div class="settings-row number-row">
      <span>标签字号</span>
      <NumberField
        ariaLabel="标签字号"
        suffix="px"
        min={11}
        max={30}
        live={true}
        value={$appSettings.appearance.tagFontSize}
        onCommit={(v) => updateAppearance("tagFontSize", v)}
      />
    </div>
    <div class="settings-row">
      <span>链接打开</span>
      <Dropdown
        ariaLabel="链接打开"
        value={$appSettings.appearance.linkOpenMode}
        options={[
          { value: "app", label: "应用内打开" },
          { value: "system", label: "系统浏览器" }
        ]}
        on:change={(event) => updateAppearance("linkOpenMode", event.detail as Settings["appearance"]["linkOpenMode"])}
      />
    </div>
  </section>

  <section>
    <h3>特性开关</h3>
    <label class="toggle-row">
      <span>显示分类角标</span>
      <input
        type="checkbox"
        checked={$appSettings.features.showCategoryBadges}
        on:change={(event) => updateFeature("showCategoryBadges", event.currentTarget.checked)}
      />
    </label>
    <p class="muted">在左侧栏分类行显示该分类下未完成条目数。</p>
  </section>

  {#if caps.trayLifecycle}
    <section>
      <h3>窗口与系统</h3>
      <div class="settings-row">
        <span>关闭按钮</span>
        <Dropdown
          ariaLabel="关闭按钮"
          value={$appSettings.lifecycle.closeToTray ? "tray" : "exit"}
          options={[
            { value: "tray", label: "退到系统托盘" },
            { value: "exit", label: "直接退出应用" }
          ]}
          on:change={(event) => void updateLifecycle("closeToTray", event.detail === "tray")}
        />
      </div>
      <label class="toggle-row">
        <span>开机自启</span>
        <input
          type="checkbox"
          checked={$appSettings.lifecycle.launchAtStartup}
          on:change={(event) => void updateLifecycle("launchAtStartup", event.currentTarget.checked)}
        />
      </label>
      <p class="muted">托盘图标右键菜单可打开窗口或退出应用；再次启动程序会聚焦已运行窗口。</p>
    </section>
  {/if}

  <section>
    <h3>消息通知</h3>
    {#if caps.popupNotificationWindow}
      <div class="settings-row number-row">
        <span>自动隐藏</span>
        <NumberField
          ariaLabel="通知自动隐藏时长"
          suffix="ms"
          min={1200}
          max={60000}
          value={$appSettings.notifications.durationMs}
          onCommit={(v) => updateNotifications("durationMs", v)}
        />
      </div>
      <div class="settings-row">
        <span>弹窗位置</span>
        <Dropdown
          ariaLabel="通知弹窗位置"
          value={$appSettings.notifications.position}
          options={notificationPositionOptions}
          on:change={(event) => updateNotifications("position", event.detail as Settings["notifications"]["position"])}
        />
      </div>
      <div class="settings-row number-row">
        <span>弹窗宽度</span>
        <NumberField
          ariaLabel="通知弹窗宽度"
          suffix="px"
          min={280}
          max={600}
          value={$appSettings.notifications.width}
          onCommit={(v) => updateNotifications("width", v)}
        />
      </div>
      <div class="settings-row number-row">
        <span>弹窗高度</span>
        <NumberField
          ariaLabel="通知弹窗高度"
          suffix="px"
          min={50}
          max={200}
          value={$appSettings.notifications.height}
          onCommit={(v) => updateNotifications("height", v)}
        />
      </div>
      <div class="settings-row number-row">
        <span>标题字号</span>
        <NumberField
          ariaLabel="通知标题字号"
          suffix="px"
          min={10}
          max={24}
          value={$appSettings.notifications.titleFontSize}
          onCommit={(v) => updateNotifications("titleFontSize", v)}
        />
      </div>
      <div class="settings-row number-row">
        <span>正文字号</span>
        <NumberField
          ariaLabel="通知正文字号"
          suffix="px"
          min={8}
          max={20}
          value={$appSettings.notifications.bodyFontSize}
          onCommit={(v) => updateNotifications("bodyFontSize", v)}
        />
      </div>
      <div class="notification-setting-card">
        <span>通知会以独立悬浮小窗展示，适用于命令行 notify 和定时任务。</span>
        <button type="button" on:click={testNotification}>发送测试通知</button>
      </div>
    {:else if caps.systemNotifications}
      <div class="notification-setting-card">
        <span>通知通过系统通知发送。</span>
        <button type="button" on:click={testNotification}>发送测试通知</button>
      </div>
    {/if}
  </section>

  {#if caps.globalShortcuts}
    <section>
      <h3>快捷键</h3>
      <label class="shortcut-row">
        新建内容
        <input value={$appSettings.shortcuts.newTask} on:change={(event) => updateShortcut("newTask", event.currentTarget.value)} />
        <small>聚焦下方输入框</small>
      </label>
      <label class="shortcut-row">
        搜索
        <input value={$appSettings.shortcuts.focusSearch} on:change={(event) => updateShortcut("focusSearch", event.currentTarget.value)} />
        <small>聚焦搜索框</small>
      </label>
      <label class="shortcut-row">
        全局唤起
        <input value={$appSettings.shortcuts.toggleWindow} on:change={(event) => updateShortcut("toggleWindow", event.currentTarget.value)} />
        <small>系统级显示/隐藏</small>
      </label>
      <label class="shortcut-row">
        设置
        <input value={$appSettings.shortcuts.openSettings} on:change={(event) => updateShortcut("openSettings", event.currentTarget.value)} />
        <small>打开或关闭设置</small>
      </label>
    </section>
  {/if}

  <section>
    <div class="section-head">
      <h3>数据同步</h3>
      <button
        class="icon-button"
        type="button"
        title="配对历史：此前用过的服务器地址与账户"
        aria-label="配对历史"
        on:click={toggleHistory}
      ><History size={16} /></button>
    </div>

    {#if historyOpen}
      <div class="sync-popover">
        <div class="sync-popover-head">
          <span>配对历史</span>
          <span class="spacer"></span>
          <button
            class="icon-button"
            type="button"
            title={historySecretsVisible ? "隐藏密码" : "显示密码"}
            aria-label={historySecretsVisible ? "隐藏密码" : "显示密码"}
            on:click={() => (historySecretsVisible = !historySecretsVisible)}
          >{#if historySecretsVisible}<EyeOff size={14} />{:else}<Eye size={14} />{/if}</button>
          <button class="icon-button" type="button" title="关闭" aria-label="关闭" on:click={() => (historyOpen = false)}><X size={14} /></button>
        </div>
        {#if history.length === 0}
          <p class="muted">还没有历史记录：成功配对一次后会自动记住（最多 8 条，只存本机）。</p>
        {:else}
          <div class="sync-popover-list">
            {#each history as entry, index (entry.serverUrl + "|" + entry.username)}
              <div class="sync-popover-row">
                <button class="menu-action-button discovery-item" type="button" on:click={() => applyHistory(entry)}>
                  <span class="discovery-name">{entry.username || "（无用户名）"}</span>
                  <span class="muted">{entry.serverUrl}</span>
                  <span class="muted secret">{historySecretsVisible ? entry.secret : "••••••"}</span>
                </button>
                <button
                  class="icon-button"
                  type="button"
                  title="删除这条历史"
                  aria-label="删除这条历史"
                  on:click={() => removeHistory(index)}
                ><X size={13} /></button>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    {#if syncPaired}
      <div class="sync-card">
        <div class="settings-row">
          <span class="sync-label">
            服务器
            <span class="conn-state" data-state={connectionState}>
              <i class="conn-dot"></i>
              <span class="conn-text">{connectionText}</span>
            </span>
          </span>
          <span class="muted sync-side">{nextSyncText}</span>
        </div>
        <div class="sync-url">{$appSettings.sync.serverUrl}</div>
        <div class="settings-row"><span>账户</span><span class="muted sync-side">{$appSettings.sync.username}</span></div>
        {#if syncPaused}
          <p class="sync-note">同步已暂停：不会自动同步，服务器与账户配置都还在。</p>
        {/if}
        {#if syncStatus?.lastSyncAt}
          <div class="settings-row">
            <span>最近同步</span>
            <span class="muted sync-side">
              {new Date(syncStatus.lastSyncAt).toLocaleString()}
              {#if syncStatus?.lastResult}
                （拉 {syncStatus.lastResult.pulled} / 推 {syncStatus.lastResult.pushed}
                {#if syncStatus.lastResult.imagesPulled || syncStatus.lastResult.imagesPushed}
                  / 图片 ↓{syncStatus.lastResult.imagesPulled ?? 0} ↑{syncStatus.lastResult.imagesPushed ?? 0}
                {/if}
                {#if syncStatus.lastResult.conflicts} / 冲突 {syncStatus.lastResult.conflicts}{/if}）
              {/if}
            </span>
          </div>
        {/if}
        {#if connection.online === false && connection.lastError}
          <p class="update-error">服务器不可达：{connection.lastError}（每 {$appSettings.sync.reconnectSeconds || 300} 秒静默重连）</p>
        {/if}
        <div class="settings-row"><span>同步数据（节点/任务/插图）</span>
          <input type="checkbox" checked={$appSettings.sync.syncData} on:change={(event) => updateSyncScope("syncData", event.currentTarget.checked)} />
        </div>
        <div class="settings-row"><span>同步设置（配置/配色/背景）</span>
          <input type="checkbox" checked={$appSettings.sync.syncSettings} on:change={(event) => updateSyncScope("syncSettings", event.currentTarget.checked)} />
        </div>
        <div class="settings-row"><span>同步任务（跨平台路径通常不可执行）</span>
          <input type="checkbox" checked={$appSettings.sync.syncSchedules} on:change={(event) => updateSyncScope("syncSchedules", event.currentTarget.checked)} />
        </div>
        <div class="settings-row number-row">
          <span>自动同步间隔（秒）</span>
          <NumberField
            ariaLabel="自动同步间隔（秒）"
            suffix="s"
            min={5}
            max={86400}
            value={$appSettings.sync.intervalSeconds || 30}
            onCommit={(v) => updateSyncInterval(v)}
          />
        </div>
        <div class="settings-row number-row">
          <span>掉线重连间隔（秒）</span>
          <NumberField
            ariaLabel="掉线重连间隔（秒）"
            suffix="s"
            min={5}
            max={86400}
            value={$appSettings.sync.reconnectSeconds || 300}
            onCommit={(v) => updateReconnectSeconds(v)}
          />
        </div>
        <div class="settings-row sync-actions">
          <button class="settings-button" type="button" disabled={syncBusy} on:click={unpairSync}>解除配对</button>
          <button class="settings-button" type="button" disabled={syncBusy} on:click={togglePause}>
            {syncPaused ? "恢复同步" : "暂停同步"}
          </button>
          <button class="settings-button primary" type="button" disabled={syncBusy || syncPaused} on:click={runSyncNow}>
            {syncBusy ? "同步中…" : "立即同步"}
          </button>
        </div>
      </div>
    {:else}
      <div class="sync-card">
        <label class="settings-row">
          服务器地址
          <span class="sync-input-wrap">
            <input
              bind:value={syncForm.serverUrl}
              placeholder="http://192.168.1.10:52177"
              on:input={() => (discoveryOpen = false)}
            />
            <button
              class="icon-button inline"
              type="button"
              disabled={discovering}
              title="搜索局域网内的服务器"
              aria-label="搜索局域网内的服务器"
              on:click={runDiscovery}
            >{#if discovering}<span class="spin"><LoaderCircle size={15} /></span>{:else}<Search size={15} />{/if}</button>
          </span>
        </label>
        {#if discoveryOpen}
          <div class="sync-popover">
            <div class="sync-popover-head">
              <span>{discovering ? "正在搜索…" : `发现 ${discovered.length} 台服务器`}</span>
              <span class="spacer"></span>
              <button class="icon-button" type="button" title="关闭" aria-label="关闭" on:click={() => (discoveryOpen = false)}><X size={14} /></button>
            </div>
            {#if discovering}
              <p class="muted">在 UDP 52177 上广播查询，收集局域网内 kxtodo-server 的应答…</p>
            {:else if discovered.length === 0}
              <p class="muted">没发现服务器：确认它在跑、与本机同一局域网、监听在非回环地址（0.0.0.0）。</p>
            {:else}
              <div class="sync-popover-list">
                {#each discovered as server (server.url)}
                  <button class="menu-action-button discovery-item" type="button" on:click={() => pickDiscovered(server)}>
                    <span class="discovery-name">{server.name || "未命名服务器"}</span>
                    <span class="muted">{server.host}:{server.port}</span>
                    {#if !server.verified}<span class="muted">（未复核）</span>{/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
        <label class="settings-row">
          用户名
          <input bind:value={syncForm.username} placeholder="账户名" />
        </label>
        <label class="settings-row">
          密码
          <input type="password" bind:value={syncForm.secret} placeholder="用于派生加密密钥，丢失无法找回" />
        </label>
        <div class="settings-row sync-scope-row">
          <input id="sync-scope-settings" type="checkbox" bind:checked={syncForm.syncSettings} />
          <label for="sync-scope-settings" class="inline-label">同步设置（配置/配色/背景）</label>
          <input id="sync-scope-schedules" type="checkbox" bind:checked={syncForm.syncSchedules} />
          <label for="sync-scope-schedules" class="inline-label">同步任务</label>
        </div>
        <div class="settings-row sync-actions">
          <button class="settings-button primary" type="button" disabled={syncBusy} on:click={registerSync}>
            {syncBusy ? "处理中…" : "注册新账户"}
          </button>
          <button class="settings-button" type="button" disabled={syncBusy} on:click={loginSync}>登录已有账户</button>
        </div>
        <p class="muted">注册 = 创建账户并配对本机；已有账户在其它设备上用「登录」。数据（节点/任务/插图）默认同步。</p>
      </div>
    {/if}
  </section>

  <section>
    <h3>关于与更新</h3>
    <div class="settings-row">
      <span>当前版本</span>
      <span class="muted">v{$appVersion || "…"}</span>
    </div>
    <div class="settings-row">
      <span>自动检查更新</span>
      <input
        type="checkbox"
        checked={$appSettings.updates.autoCheck}
        on:change={(event) => updateAutoCheck(event.currentTarget.checked)}
      />
    </div>
    <div class="settings-row">
      <span>检查更新</span>
      <button class="settings-button" type="button" disabled={checkingUpdate || updateBusy} on:click={checkUpdates}>
        {checkingUpdate ? "检查中…" : "检查更新"}
      </button>
    </div>
    {#if progress.phase === "downloading"}
      <p class="update-status">正在下载 {progress.stage || "更新"} {progress.percent}%</p>
    {:else if progress.phase === "installing"}
      <p class="update-status">已下载，请在系统安装界面完成更新</p>
    {:else if progress.phase === "restarting"}
      <p class="update-status">{progress.message || "下载完成，正在重启应用…"}</p>
    {:else if progress.phase === "failed"}
      <p class="update-error">更新失败：{progress.message}</p>
    {/if}
    {#if pendingUpdate}
      <div class="update-available">
        <p>发现新版本 <strong>v{pendingUpdate.version}</strong></p>
        <button class="settings-button primary" type="button" disabled={updateBusy} on:click={downloadAndApply}>
          {caps.updateChannel === "apk" ? "下载并安装更新" : "下载并重启更新"}
        </button>
      </div>
    {/if}
    {#if upToDate && !pendingUpdate}
      <p class="update-status">您当前已经是最新版本</p>
    {/if}
    {#if updateCheckError}
      <p class="update-error">{updateCheckError}</p>
    {/if}
  </section>
</aside>

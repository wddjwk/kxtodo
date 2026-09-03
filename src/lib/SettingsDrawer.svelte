<script lang="ts">
  import {
    appSettings, showSettings, showToast, showNotification, fileToDataUrl, appVersion
  } from "./stores";
  import { setConfig as setConfigAction } from "./actions";
  import { checkForUpdate, startUpdate, updateProgress, type UpdateInfo } from "./updater";
  import { isMobile } from "./platform";
  import { caps } from "./capabilities";
  import { ArrowLeft } from "@lucide/svelte";
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

  function updateCloudEndpoint(value: string): void {
    void setConfigAction("cloud.endpoint", value);
  }

  function updateCloudProvider(value: Settings["cloud"]["provider"]): void {
    void setConfigAction("cloud.provider", value);
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
    {#if caps.windowZoom}
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
    {/if}
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
        <span>通知通过 Android 系统通知发送。</span>
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

  {#if caps.desktop}
    <section>
      <h3>云同步预留</h3>
      <div class="sync-card">
        <label class="settings-row">
          提供方
          <Dropdown
            value={$appSettings.cloud.provider}
            options={[
              { value: "none", label: "未启用" },
              { value: "webdav", label: "WebDAV" },
              { value: "s3", label: "S3" },
              { value: "custom", label: "自定义 HTTP" }
            ]}
            on:change={(event) => updateCloudProvider(event.detail as Settings["cloud"]["provider"])}
          />
        </label>
        <label class="settings-row">
          地址
          <input value={$appSettings.cloud.endpoint} placeholder="后续实现时使用" on:input={(event) => updateCloudEndpoint(event.currentTarget.value)} />
        </label>
        <p class="muted">当前版本只保留配置结构，不执行任何网络同步。</p>
      </div>
    </section>
  {/if}

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
      <p class="update-status">下载完成，正在重启应用…</p>
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

<script lang="ts">
  import {
    appSettings, showSettings, showToast, showNotification, fileToDataUrl, appVersion
  } from "./stores";
  import { setConfig as setConfigAction } from "./actions";
  import { checkForUpdate, startUpdate, updateProgress, type UpdateInfo } from "./updater";
  import { isMobile } from "./platform";
  import {
    uiScaleValue, scalePercentValue, clampNumber, isNumberInRange,
    buildSettingsDrawerStyle, avatarStyle, avatarInitial
  } from "./styles";
  import { defaultSettings } from "./defaults";
  import {
    isTauriRuntime, pickImageFile, saveAvatarImage, avatarImageUrl
  } from "./backend";
  import { avatarCache, resolveAvatarSrc } from "./images";
  import Dropdown from "./Dropdown.svelte";
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

  function updateScalePercent(value: number): void {
    if (isNumberInRange(value, 50, 150)) {
      updateAppearance("uiScale", Math.round(value) / 100);
    }
  }

  function commitScalePercent(value: number): void {
    const nextPercent = clampNumber(value, scalePercentValue($appSettings.appearance.uiScale), 50, 150);
    updateAppearance("uiScale", nextPercent / 100);
  }

  function updateAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize" | "tagFontSize", value: number): void {
    const max = field === "uiFontSize" ? 22 : field === "tagFontSize" ? 30 : 26;
    const min = field === "tagFontSize" ? 11 : 14;
    if (isNumberInRange(value, min, max)) {
      updateAppearance(field, Math.round(value));
    }
  }

  function commitAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize" | "tagFontSize", value: number): void {
    const fallback = defaultSettings.appearance[field];
    const max = field === "uiFontSize" ? 22 : field === "tagFontSize" ? 30 : 26;
    const min = field === "tagFontSize" ? 11 : 14;
    updateAppearance(field, clampNumber(value, fallback, min, max));
  }

  function updateNotificationDuration(value: number): void {
    if (isNumberInRange(value, 1200, 60000)) {
      updateNotifications("durationMs", Math.round(value));
    }
  }

  function commitNotificationDuration(value: number): void {
    updateNotifications("durationMs", clampNumber(value, defaultSettings.notifications.durationMs, 1200, 60000));
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
  $: updateBusy = progress.phase === "downloading" || progress.phase === "restarting";

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
    if (!isTauriRuntime) {
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
    <h2>设置</h2>
    <button type="button" on:click={() => showSettings.set(false)}>×</button>
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
    <div class="settings-row number-row">
      <span>界面缩放</span>
      <span class="number-control">
        <input
          aria-label="界面缩放"
          type="number"
          min="50"
          max="150"
          step="1"
          value={scalePercentValue($appSettings.appearance.uiScale)}
          on:input={(event) => updateScalePercent(event.currentTarget.valueAsNumber)}
          on:change={(event) => commitScalePercent(event.currentTarget.valueAsNumber)}
        />
        <span>%</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>UI 字号</span>
      <span class="number-control">
        <input
          aria-label="UI 字号"
          type="number"
          min="14"
          max="22"
          step="1"
          value={$appSettings.appearance.uiFontSize}
          on:input={(event) => updateAppearanceFont("uiFontSize", event.currentTarget.valueAsNumber)}
          on:change={(event) => commitAppearanceFont("uiFontSize", event.currentTarget.valueAsNumber)}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>Markdown 字号</span>
      <span class="number-control">
        <input
          aria-label="Markdown 字号"
          type="number"
          min="14"
          max="26"
          step="1"
          value={$appSettings.appearance.markdownFontSize}
          on:input={(event) => updateAppearanceFont("markdownFontSize", event.currentTarget.valueAsNumber)}
          on:change={(event) => commitAppearanceFont("markdownFontSize", event.currentTarget.valueAsNumber)}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>编辑器字号</span>
      <span class="number-control">
        <input
          aria-label="编辑器字号"
          type="number"
          min="14"
          max="26"
          step="1"
          value={$appSettings.appearance.editorFontSize}
          on:input={(event) => updateAppearanceFont("editorFontSize", event.currentTarget.valueAsNumber)}
          on:change={(event) => commitAppearanceFont("editorFontSize", event.currentTarget.valueAsNumber)}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>标签字号</span>
      <span class="number-control">
        <input
          aria-label="标签字号"
          type="number"
          min="11"
          max="30"
          step="1"
          value={$appSettings.appearance.tagFontSize}
          on:input={(event) => updateAppearanceFont("tagFontSize", event.currentTarget.valueAsNumber)}
          on:change={(event) => commitAppearanceFont("tagFontSize", event.currentTarget.valueAsNumber)}
        />
        <span>px</span>
      </span>
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

  <section>
    <h3>消息通知</h3>
    <div class="settings-row number-row">
      <span>自动隐藏</span>
      <span class="number-control">
        <input
          aria-label="通知自动隐藏时长"
          type="number"
          min="1200"
          max="60000"
          step="100"
          value={$appSettings.notifications.durationMs}
          on:input={(event) => updateNotificationDuration(event.currentTarget.valueAsNumber)}
          on:change={(event) => commitNotificationDuration(event.currentTarget.valueAsNumber)}
        />
        <span>ms</span>
      </span>
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
      <span class="number-control">
        <input aria-label="通知弹窗宽度" type="number" min="280" max="600" step="10"
          value={$appSettings.notifications.width}
          on:input={(event) => { const v = event.currentTarget.valueAsNumber; if (Number.isFinite(v)) updateNotifications("width", Math.min(600, Math.max(280, Math.round(v)))); }}
          on:change={(event) => updateNotifications("width", clampNumber(event.currentTarget.valueAsNumber, defaultSettings.notifications.width, 280, 600))}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>弹窗高度</span>
      <span class="number-control">
        <input aria-label="通知弹窗高度" type="number" min="50" max="200" step="2"
          value={$appSettings.notifications.height}
          on:input={(event) => { const v = event.currentTarget.valueAsNumber; if (Number.isFinite(v)) updateNotifications("height", Math.min(200, Math.max(50, Math.round(v)))); }}
          on:change={(event) => updateNotifications("height", clampNumber(event.currentTarget.valueAsNumber, defaultSettings.notifications.height, 50, 200))}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>标题字号</span>
      <span class="number-control">
        <input aria-label="通知标题字号" type="number" min="10" max="24" step="1"
          value={$appSettings.notifications.titleFontSize}
          on:input={(event) => { const v = event.currentTarget.valueAsNumber; if (Number.isFinite(v)) updateNotifications("titleFontSize", Math.min(24, Math.max(10, Math.round(v)))); }}
          on:change={(event) => updateNotifications("titleFontSize", clampNumber(event.currentTarget.valueAsNumber, defaultSettings.notifications.titleFontSize, 10, 24))}
        />
        <span>px</span>
      </span>
    </div>
    <div class="settings-row number-row">
      <span>正文字号</span>
      <span class="number-control">
        <input aria-label="通知正文字号" type="number" min="8" max="20" step="1"
          value={$appSettings.notifications.bodyFontSize}
          on:input={(event) => { const v = event.currentTarget.valueAsNumber; if (Number.isFinite(v)) updateNotifications("bodyFontSize", Math.min(20, Math.max(8, Math.round(v)))); }}
          on:change={(event) => updateNotifications("bodyFontSize", clampNumber(event.currentTarget.valueAsNumber, defaultSettings.notifications.bodyFontSize, 8, 20))}
        />
        <span>px</span>
      </span>
    </div>
    <div class="notification-setting-card">
      <span>通知会以独立悬浮小窗展示，适用于命令行 notify 和定时任务。</span>
      <button type="button" on:click={testNotification}>发送测试通知</button>
    </div>
  </section>

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

  {#if !$isMobile}
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
      {:else if progress.phase === "restarting"}
        <p class="update-status">下载完成，正在重启应用…</p>
      {:else if progress.phase === "failed"}
        <p class="update-error">更新失败：{progress.message}</p>
      {/if}
      {#if pendingUpdate}
        <div class="update-available">
          <p>发现新版本 <strong>v{pendingUpdate.version}</strong></p>
          <button class="settings-button primary" type="button" disabled={updateBusy} on:click={downloadAndApply}>
            下载并重启更新
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
  {/if}
</aside>

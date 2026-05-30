<script lang="ts">
  import {
    appSettings, commitSettings, showSettings, showToast, fileToDataUrl
  } from "./stores";
  import {
    uiScaleValue, scalePercentValue, clampNumber, isNumberInRange,
    buildSettingsDrawerStyle, avatarStyle, avatarInitial
  } from "./styles";
  import { defaultSettings } from "./defaults";
  import {
    setCloseToTray, setAutostart, registerGlobalShortcut
  } from "./backend";
  import type { Settings } from "./types";

  let avatarFileInput: HTMLInputElement;

  $: drawerStyle = buildSettingsDrawerStyle($appSettings.appearance);
  $: avStyle = avatarStyle($appSettings.profile.avatar);
  $: avInitial = avatarInitial($appSettings.profile.displayName);

  function updateProfile(field: keyof Settings["profile"], value: string): void {
    commitSettings({
      ...$appSettings,
      profile: { ...$appSettings.profile, [field]: value }
    });
  }

  function updateAppearance<K extends keyof Settings["appearance"]>(field: K, value: Settings["appearance"][K]): void {
    commitSettings({
      ...$appSettings,
      appearance: { ...$appSettings.appearance, [field]: value }
    });
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

  function updateAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize", value: number): void {
    const max = field === "uiFontSize" ? 22 : 26;
    if (isNumberInRange(value, 14, max)) {
      updateAppearance(field, Math.round(value));
    }
  }

  function commitAppearanceFont(field: "uiFontSize" | "markdownFontSize" | "editorFontSize", value: number): void {
    const fallback = defaultSettings.appearance[field];
    const nextSize = clampNumber(value, fallback, 14, field === "uiFontSize" ? 22 : 26);
    updateAppearance(field, nextSize);
  }

  async function updateLifecycle<K extends keyof Settings["lifecycle"]>(field: K, value: Settings["lifecycle"][K]): Promise<void> {
    try {
      if (field === "closeToTray") await setCloseToTray(Boolean(value));
      if (field === "launchAtStartup") await setAutostart(Boolean(value));
      commitSettings({
        ...$appSettings,
        lifecycle: { ...$appSettings.lifecycle, [field]: value }
      });
    } catch (error) {
      showToast(`系统设置更新失败：${String(error)}`);
    }
  }

  function updateShortcut(field: keyof Settings["shortcuts"], value: string): void {
    const next = {
      ...$appSettings,
      shortcuts: { ...$appSettings.shortcuts, [field]: value }
    };
    commitSettings(next);
    if (field === "toggleWindow") {
      registerGlobalShortcut(value).catch((error) => showToast(`全局快捷键注册失败：${String(error)}`));
    }
  }

  function updateCloudEndpoint(value: string): void {
    commitSettings({ ...$appSettings, cloud: { ...$appSettings.cloud, endpoint: value } });
  }

  function updateCloudProvider(value: Settings["cloud"]["provider"]): void {
    commitSettings({ ...$appSettings, cloud: { ...$appSettings.cloud, provider: value } });
  }

  async function uploadAvatar(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      const avatar = await fileToDataUrl(target.files[0]);
      commitSettings({ ...$appSettings, profile: { ...$appSettings.profile, avatar } });
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
      <button type="button" on:click={() => avatarFileInput.click()}>上传头像</button>
      <input bind:this={avatarFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadAvatar} />
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
    <div class="settings-row">
      <span>链接打开</span>
      <select
        aria-label="链接打开"
        value={$appSettings.appearance.linkOpenMode}
        on:change={(event) => updateAppearance("linkOpenMode", event.currentTarget.value as Settings["appearance"]["linkOpenMode"])}
      >
        <option value="app">应用内打开</option>
        <option value="system">系统浏览器</option>
      </select>
    </div>
  </section>

  <section>
    <h3>窗口与系统</h3>
    <label class="settings-row">
      关闭按钮
      <select
        value={$appSettings.lifecycle.closeToTray ? "tray" : "exit"}
        on:change={(event) => void updateLifecycle("closeToTray", event.currentTarget.value === "tray")}
      >
        <option value="tray">退到系统托盘</option>
        <option value="exit">直接退出应用</option>
      </select>
    </label>
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
        <select value={$appSettings.cloud.provider} on:change={(event) => updateCloudProvider(event.currentTarget.value as Settings["cloud"]["provider"])}>
          <option value="none">未启用</option>
          <option value="webdav">WebDAV</option>
          <option value="s3">S3</option>
          <option value="custom">自定义 HTTP</option>
        </select>
      </label>
      <label class="settings-row">
        地址
        <input value={$appSettings.cloud.endpoint} placeholder="后续实现时使用" on:input={(event) => updateCloudEndpoint(event.currentTarget.value)} />
      </label>
      <p class="muted">当前版本只保留配置结构，不执行任何网络同步。</p>
    </div>
  </section>
</aside>

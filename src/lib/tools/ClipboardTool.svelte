<script lang="ts">
  /**
   * 剪切板同步（v0.8.10）：工具箱里的一个工具页。
   *
   * 界面上不放任何解释性段落：所有说明收进右上角的 ⓘ 浮层（需求 1）。
   * 设置一次写整份（`configureClipboard` → `gui.clipboard-configure`）；
   * 设备勾选是纯本机偏好，立刻生效、不重启后台服务（`gui.clipboard-peer`）。
   */
  import { onDestroy, onMount } from "svelte";
  import {
    ChevronDown,
    ClipboardCopy,
    FolderOpen,
    FolderOutput,
    Info,
    RefreshCw,
    Save,
    X
  } from "@lucide/svelte";
  import { appSettings, showToast } from "../stores";
  import { configureClipboard, setClipboardPeer } from "../actions";
  import { isMobile, createBackGuard, hostOs } from "../platform";
  import { clipboardDefaultDir, clipboardRefresh, pickDirectory, transferOpenPath } from "../backend";
  import { clipboardSyncStatus, refreshClipboardStatus } from "../clipboardSyncStore";
  import { listNativeSaveDirs, openNativeFolder, type NativeSaveDir } from "../androidBridge";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import type { ClipboardPeer } from "../backend";
  import type { Settings } from "../types";

  type Draft = Settings["clipboard"];
  const copy = (value: Draft): Draft => JSON.parse(JSON.stringify(value)) as Draft;

  let draft = copy($appSettings.clipboard);
  let lastSaved = $appSettings.clipboard;
  let saving = false;
  let refreshing = false;
  let defaultDir = "";
  let timer: number | undefined;
  let infoOpen = false;
  let methodsOpen = true;
  let contentOpen = false;
  let pendingPeers = new Set<string>();

  // 浮层开/关自动接管返回键：组件销毁时 createBackGuard 内部会注销拦截器。
  const backGuard = createBackGuard();
  $: backGuard(infoOpen, () => (infoOpen = false));

  $: if ($appSettings.clipboard !== lastSaved) {
    lastSaved = $appSettings.clipboard;
    draft = copy($appSettings.clipboard);
  }
  $: dirty = JSON.stringify(draft) !== JSON.stringify($appSettings.clipboard);
  $: status = $clipboardSyncStatus;
  $: lanPeers = (status?.peers ?? []).filter((peer) => peer.transports.includes("lan"));
  $: wanPeers = (status?.peers ?? []).filter(
    (peer) => peer.transports.includes("server") || peer.transports.includes("p2p")
  );
  $: wanDraftOn = draft.serverEnabled || draft.wanP2pEnabled;
  $: anyTransport = draft.lanEnabled || draft.serverEnabled || draft.wanP2pEnabled;

  onMount(() => {
    void refreshClipboardStatus();
    void clipboardDefaultDir()
      .then((dir) => {
        defaultDir = dir ?? "";
      })
      .catch(() => undefined);
    timer = window.setInterval(() => void refreshClipboardStatus(), 3000);
  });
  onDestroy(() => {
    if (timer) window.clearInterval(timer);
  });

  function transportLabel(id: string): string {
    if (id === "server") return "服务器";
    if (id === "p2p") return "P2P";
    return "局域网";
  }

  function validate(next: Draft): boolean {
    if (!(next.lanEnabled || next.serverEnabled || next.wanP2pEnabled)) {
      showToast("请至少开启一种同步方式", 4000);
      return false;
    }
    if ((next.serverEnabled || next.wanP2pEnabled) && (!next.account.trim() || !next.password.trim())) {
      methodsOpen = true;
      showToast("广域网同步需要填写账户名和密码", 4000);
      return false;
    }
    if (next.serverEnabled && !next.serverUrl.trim()) {
      methodsOpen = true;
      showToast("统一服务器同步需要填写服务器地址", 4000);
      return false;
    }
    return true;
  }

  function normalizedLimits(next: Draft): Draft {
    return {
      ...next,
      limits: {
        textMb: Math.min(64, Math.max(1, Math.round(Number(next.limits.textMb) || 1))),
        imageMb: Math.min(256, Math.max(1, Math.round(Number(next.limits.imageMb) || 1))),
        filesMb: Math.min(1024, Math.max(1, Math.round(Number(next.limits.filesMb) || 1)))
      }
    };
  }

  async function save(enable?: boolean): Promise<void> {
    const next: Draft = normalizedLimits({ ...draft, enabled: enable ?? draft.enabled });
    if (next.enabled && !validate(next)) return;
    draft = next;
    saving = true;
    const ok = await configureClipboard(next);
    saving = false;
    if (!ok) return;
    lastSaved = $appSettings.clipboard;
    await refreshClipboardStatus();
    showToast(next.enabled ? "剪切板同步已保存并应用" : "剪切板同步已关闭", 2200);
  }

  async function toggleMaster(checked: boolean): Promise<void> {
    if (checked) {
      const next = { ...draft, enabled: true };
      if (!validate(next)) return;
    }
    await save(checked);
  }

  async function refresh(): Promise<void> {
    if (refreshing) return;
    refreshing = true;
    try {
      clipboardSyncStatus.set(await clipboardRefresh());
    } catch {
      await refreshClipboardStatus();
    }
    refreshing = false;
  }

  async function togglePeer(peer: ClipboardPeer, selected: boolean): Promise<void> {
    if (pendingPeers.has(peer.id)) return;
    pendingPeers.add(peer.id);
    // 先本地翻状态（勾选是纯本机偏好，核心命令不会失败到需要回滚整页的程度）。
    clipboardSyncStatus.update((current) =>
      current
        ? {
            ...current,
            peers: current.peers.map((item) =>
              item.id === peer.id ? { ...item, selected } : item
            )
          }
        : current
    );
    const ok = await setClipboardPeer(peer.id, selected);
    pendingPeers.delete(peer.id);
    if (!ok) {
      await refreshClipboardStatus();
      return;
    }
    await refreshClipboardStatus();
  }

  async function chooseDir(): Promise<void> {
    const dir = await pickDirectory();
    if (dir) draft = { ...draft, saveDir: dir };
  }

  function resetDir(): void {
    draft = { ...draft, saveDir: "" };
  }

  // ---- 保存位置三件事（v0.9.0 bugfix 4.2）：打开文件夹 / 选择路径 / 设置默认 ----

  const isAndroid = hostOs === "android";
  let dirMenu: { x: number; y: number; anchor: HTMLElement } | null = null;
  let nativeDirs: NativeSaveDir[] = [];
  const backGuardDirMenu = createBackGuard();
  $: backGuardDirMenu(dirMenu !== null, () => (dirMenu = null));

  /** 打开文件夹：桌面走系统资源管理器；安卓尽力而为（相册集合 / DocumentsUI）。 */
  async function openSaveDir(): Promise<void> {
    const target = draft.saveDir.trim() || defaultDir;
    if (!target) {
      showToast("保存位置还没确定，先保存一次设置", 3500);
      return;
    }
    if (isAndroid) {
      const error = openNativeFolder(target);
      if (error) showToast(error, 6000);
      return;
    }
    try {
      await transferOpenPath(target);
    } catch (error) {
      showToast(`打开文件夹失败：${String(error)}`, 5000);
    }
  }

  /** 选择路径：桌面是系统目录对话框；安卓给一份真实可写的候选目录菜单。 */
  function openDirPicker(event: MouseEvent): void {
    const button = event.currentTarget as HTMLElement;
    if (!isAndroid) {
      void chooseDir();
      return;
    }
    nativeDirs = listNativeSaveDirs();
    if (nativeDirs.length === 0) {
      showToast("拿不到可选目录", 4000);
      return;
    }
    const rect = button.getBoundingClientRect();
    dirMenu = { x: rect.right, y: rect.bottom + 6, anchor: button };
  }
</script>

<svelte:window
  on:keydown={(event) => {
    if (event.key === "Escape" && infoOpen) infoOpen = false;
  }}
/>

<div class="toolbox-sub clipboard-page">
  <div class="clipboard-hero-wrap">
    <div class="clipboard-hero">
      <div class="clipboard-hero-icon"><ClipboardCopy size={24} /></div>
      <div class="clipboard-hero-text">
        <strong>剪切板同步</strong>
        <span class="clipboard-device" title={status?.deviceId ?? ""}>
          {status?.deviceName || "kxtodo·平台(自动)"}
        </span>
      </div>
      <div class="clipboard-hero-actions">
        <button
          class="clipboard-icon-button"
          type="button"
          title="说明"
          aria-label="说明"
          on:click|stopPropagation={() => (infoOpen = !infoOpen)}
        >
          <Info size={17} />
        </button>
        <label class="ui-switch" title={draft.enabled ? "关闭剪切板同步" : "开启剪切板同步"}>
          <input
            type="checkbox"
            checked={$appSettings.clipboard.enabled}
            disabled={saving}
            on:change={(event) => void toggleMaster((event.currentTarget as HTMLInputElement).checked)}
          />
          <span class="ui-switch-track" aria-hidden="true"></span>
        </label>
      </div>
    </div>

    {#if infoOpen}
      <div class="clipboard-info-backdrop" on:click={() => (infoOpen = false)} role="presentation"></div>
      <div class="clipboard-info" role="dialog" aria-label="剪切板同步说明">
        <div class="clipboard-info-head">
          <strong>剪切板同步</strong>
          <button class="clipboard-icon-button" type="button" title="关闭" on:click={() => (infoOpen = false)}>
            <X size={15} />
          </button>
        </div>
        <h4>配对</h4>
        <p>局域网：同一网络下开启本工具的 KXToDo 会自动出现；双方都勾选后开始同步。</p>
        <p>广域网：填写账户名和密码。统一服务器还需要服务器地址；P2P 不需要。相同账户的设备会自动出现，同样双方勾选后同步。</p>
        <h4>内容与安全</h4>
        <p>只转发文字、HTML、图片和文件，不保存剪切板历史。设备之间加密传输；统一服务器只做设备发现，不保存内容。</p>
        <p>收到文件会存到「文件保存位置」；其它内容只写入系统剪切板。</p>
        <h4>Android</h4>
        <p>系统限制：只有 KXToDo 在前台时才能读写剪切板。</p>
      </div>
    {/if}
  </div>

  <div class="clipboard-status-line">
    <span class="clipboard-dot" class:on={status?.running}></span>
    <span class="clipboard-status-text">
      {status?.running ? "运行中" : status?.enabled ? "未运行" : "已关闭"}
    </span>
    {#if status?.running && status.transports.server}
      <span class="clipboard-badge" class:on={status.serverRegistered}>服务器</span>
    {/if}
    {#if status?.running && status.transports.p2p}
      <span class="clipboard-badge" class:on={status.p2pActive}>P2P</span>
    {/if}
    <button
      class="clipboard-icon-button clipboard-refresh"
      type="button"
      title="立刻刷新设备"
      aria-label="立刻刷新设备"
      disabled={refreshing}
      on:click={() => void refresh()}
    >
      <span class="clipboard-spin" class:on={refreshing}><RefreshCw size={15} /></span>
    </button>
  </div>
  {#if status?.lastError && (status.running || status.enabled)}
    <p class="clipboard-status-note" class:error={status.running}>{status.lastError}</p>
  {/if}
  {#if status?.running && status.lastEvent}
    <p class="clipboard-status-note muted">
      最近：{status.lastEvent.detail}{status.lastEvent.peer ? ` · ${status.lastEvent.peer}` : ""}
    </p>
  {/if}

  <section class="clipboard-card">
    <label class="clipboard-field-inline">
      <span>设备名</span>
      <input
        class="clipboard-text-input"
        value={draft.deviceName}
        spellcheck="false"
        placeholder={status?.deviceName || "kxtodo·平台(自动)"}
        on:input={(event) =>
          (draft = { ...draft, deviceName: (event.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <div class="clipboard-field-inline clipboard-dir-field">
      <span>文件保存位置</span>
      <div class="clipboard-dir-row">
        <!-- 路径要完整看得见（v0.9.0 bugfix 4.2）：单行 input 只能省略号截断，
             换成两行可折行的 textarea（仍可手改），安卓的长路径不再藏头露尾 -->
        <textarea
          class="clipboard-text-input clipboard-dir-input"
          rows="2"
          value={draft.saveDir}
          spellcheck="false"
          placeholder={defaultDir || "下载目录 / kxtodo-clipboard"}
          on:input={(event) =>
            (draft = { ...draft, saveDir: (event.currentTarget as HTMLTextAreaElement).value })}
        ></textarea>
        <span class="clipboard-dir-actions">
          <button class="menu-action-button" type="button" title="打开文件夹" on:click={() => void openSaveDir()}>
            <FolderOutput size={13} />
          </button>
          <button class="menu-action-button" type="button" title="选择路径" on:click={openDirPicker}>
            <FolderOpen size={13} />
          </button>
          <button class="menu-action-button" type="button" title="恢复默认保存位置" on:click={resetDir}>默认</button>
        </span>
      </div>
    </div>
  </section>

  {#if dirMenu}
    <ContextMenu
      x={dirMenu.x}
      y={dirMenu.y}
      xAlign="right"
      anchor={dirMenu.anchor}
      minWidth={220}
      onClose={() => (dirMenu = null)}
    >
      {#each nativeDirs as dir (dir.path)}
        <MenuItem
          label={dir.label}
          active={draft.saveDir === dir.path}
          onSelect={() => {
            draft = { ...draft, saveDir: dir.path };
            dirMenu = null;
          }}
        />
      {/each}
    </ContextMenu>
  {/if}

  <section class="clipboard-card">
    <button class="clipboard-section-head" type="button" on:click={() => (methodsOpen = !methodsOpen)}>
      <span>同步方式</span>
      <span class="clipboard-chevron" class:down={!methodsOpen}><ChevronDown size={16} /></span>
    </button>
    {#if methodsOpen}
      <div class="clipboard-method">
        <div class="clipboard-method-head">
          <strong>局域网同步</strong>
          <label class="ui-switch sm">
            <input type="checkbox" bind:checked={draft.lanEnabled} />
            <span class="ui-switch-track" aria-hidden="true"></span>
          </label>
        </div>
        {#if draft.lanEnabled}
          <div class="clipboard-peer-list">
            {#if lanPeers.length === 0}
              <span class="clipboard-empty">未发现在线设备</span>
            {:else}
              {#each lanPeers as peer (peer.id)}
                <label class="clipboard-peer-row" class:off={!peer.selected}>
                  <input
                    type="checkbox"
                    checked={peer.selected}
                    on:change={(event) =>
                      void togglePeer(peer, (event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span class="clipboard-peer-name">{peer.name}</span>
                  <span class="clipboard-peer-tags">
                    {#each peer.transports as transport (transport)}
                      <em>{transportLabel(transport)}</em>
                    {/each}
                  </span>
                </label>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      <div class="clipboard-divider"></div>

      <div class="clipboard-method">
        <div class="clipboard-method-head"><strong>广域网同步</strong></div>
        {#if wanDraftOn}
          <div class="clipboard-form-grid">
            <label class="clipboard-field">
              <span>账户名</span>
              <input
                class="clipboard-text-input"
                value={draft.account}
                spellcheck="false"
                autocomplete="username"
                placeholder="账户名"
                on:input={(event) =>
                  (draft = { ...draft, account: (event.currentTarget as HTMLInputElement).value })}
              />
            </label>
            <label class="clipboard-field">
              <span>密码</span>
              <input
                class="clipboard-text-input"
                type="password"
                value={draft.password}
                autocomplete="current-password"
                placeholder="密码"
                on:input={(event) =>
                  (draft = { ...draft, password: (event.currentTarget as HTMLInputElement).value })}
              />
            </label>
          </div>
        {/if}
        <div class="clipboard-inline-switch">
          <span>统一服务器</span>
          <label class="ui-switch sm">
            <input type="checkbox" bind:checked={draft.serverEnabled} />
            <span class="ui-switch-track" aria-hidden="true"></span>
          </label>
        </div>
        {#if draft.serverEnabled}
          <input
            class="clipboard-text-input clipboard-server-url"
            value={draft.serverUrl}
            spellcheck="false"
            inputmode="url"
            placeholder="服务器地址，例如 192.168.1.10:52177"
            on:input={(event) =>
              (draft = { ...draft, serverUrl: (event.currentTarget as HTMLInputElement).value })}
          />
        {/if}
        <div class="clipboard-inline-switch">
          <span>P2P</span>
          <label class="ui-switch sm">
            <input type="checkbox" bind:checked={draft.wanP2pEnabled} />
            <span class="ui-switch-track" aria-hidden="true"></span>
          </label>
        </div>
        {#if wanDraftOn}
          <div class="clipboard-peer-list">
            {#if wanPeers.length === 0}
              <span class="clipboard-empty">未发现在线设备</span>
            {:else}
              {#each wanPeers as peer (peer.id)}
                <label class="clipboard-peer-row" class:off={!peer.selected}>
                  <input
                    type="checkbox"
                    checked={peer.selected}
                    on:change={(event) =>
                      void togglePeer(peer, (event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span class="clipboard-peer-name">{peer.name}</span>
                  <span class="clipboard-peer-tags">
                    {#each peer.transports as transport (transport)}
                      <em>{transportLabel(transport)}</em>
                    {/each}
                  </span>
                </label>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section class="clipboard-card">
    <button class="clipboard-section-head" type="button" on:click={() => (contentOpen = !contentOpen)}>
      <span>接收内容</span>
      <span class="clipboard-chevron" class:down={!contentOpen}><ChevronDown size={16} /></span>
    </button>
    {#if contentOpen}
      <div class="clipboard-types">
        <div class="clipboard-type-row">
          <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.text} />文字</label>
          <input class="clipboard-limit" type="number" min="1" max="64" bind:value={draft.limits.textMb} /><span>MB</span>
        </div>
        <!-- HTML 不再是独立一档（v0.9.0 bugfix 4.1）：剪切板里的 html 折成 .html 文件
             走「文件」这一档（大小也吃文件的上限），界面上不再单列 -->
        <div class="clipboard-type-row">
          <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.image} />图片</label>
          <input class="clipboard-limit" type="number" min="1" max="256" bind:value={draft.limits.imageMb} /><span>MB</span>
        </div>
        <div class="clipboard-type-row">
          <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.files} />文件</label>
          <input class="clipboard-limit" type="number" min="1" max="1024" bind:value={draft.limits.filesMb} /><span>MB</span>
        </div>
      </div>
    {/if}
  </section>

  <div class="clipboard-actions">
    <button class="settings-button primary" type="button" disabled={saving || !dirty} on:click={() => void save()}>
      <Save size={15} /> {saving ? "保存中…" : "保存并应用"}
    </button>
  </div>
</div>

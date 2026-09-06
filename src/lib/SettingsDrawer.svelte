<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    appSettings, showSettings, showToast, showNotification, fileToDataUrl, appVersion,
    syncConnection, nextSyncAt
  } from "./stores";
  import { setConfig as setConfigAction } from "./actions";
  import {
    syncPair as syncPairAction,
    syncStatus as syncStatusAction,
    syncNow as syncNowAction,
    syncUnpair as syncUnpairAction,
    syncProbe as syncProbeAction,
    syncDiscover as syncDiscoverAction,
    syncPeers as syncPeersAction,
    syncHistory as syncHistoryAction,
    syncHistoryRemove as syncHistoryRemoveAction,
    syncHistoryLabel,
    setSyncScopes,
    setSyncEnabled,
    setSyncMode as setSyncModeAction,
    setSyncLanHost as setSyncLanHostAction,
    setSyncLanName as setSyncLanNameAction,
    setSyncLanPeer as setSyncLanPeerAction,
    type DiscoveredServer,
    type SyncHistoryEntry,
    type SyncPeers
  } from "./actions";
  import { checkForUpdate, startUpdate, updateProgress, type UpdateInfo } from "./updater";
  import { isMobile } from "./platform";
  import { caps } from "./capabilities";
  import { ArrowLeft, Eye, EyeOff, History, X } from "@lucide/svelte";
  import {
    scalePercentValue, buildSettingsDrawerStyle, avatarStyle, avatarInitial
  } from "./styles";
  import {
    isTauriRuntime, pickImageFile, saveAvatarImage, avatarImageUrl, openExternalUrl
  } from "./backend";
  import { avatarCache, resolveAvatarSrc } from "./images";
  import Dropdown from "./Dropdown.svelte";
  import NumberField from "./NumberField.svelte";
  import type { Settings, SyncMode } from "./types";

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
  let syncForm = { serverUrl: "", username: "", secret: "" };
  let syncBusy = false;
  let syncStatus: import("./actions").SyncStatus | null = null;
  let syncFormPrefilled = false;
  let discovering = false;
  let discovered: DiscoveredServer[] = [];
  let discoveryOpen = false;
  let historyOpen = false;
  let history: SyncHistoryEntry[] = [];
  let historySecretsVisible = false;
  /** 主机名字输入框的草稿：提交（失焦/回车）才落盘——改名字会重启内置服务器，不能每敲一个字重启一次 */
  let lanNameDraft = "";
  let lanNameFocused = false;
  /** 内置主机管理台密码的显隐（自动生成的，用户要能抄下来登录自己的管理台） */
  let adminSecretVisible = false;
  /** 已配对后点「修改」展开账户输入框：本机做服务器时账户就建在自己的库里，必须能改 */
  let accountEditing = false;
  /** 秒级 ticker：只用于「下次同步」倒计时显示 */
  let clock = Date.now();
  let clockTimer: number | undefined;
  /** P2P 设备列表浮层与高级覆盖（自部署 relay/目录）的展开态 */
  let peersOpen = false;
  let peers: SyncPeers | null = null;
  let peersLoading = false;
  let p2pAdvancedOpen = false;
  let p2pRelayDraft = "";
  let p2pDirectoryDraft = "";
  let p2pDraftsMode: SyncMode | null = null;

  const syncModeOptions: Array<{ value: SyncMode; label: string }> = [
    { value: "lan", label: "局域网" },
    { value: "server", label: "自建服务" },
    { value: "p2p", label: "P2P" }
  ];

  onMount(() => {
    clockTimer = window.setInterval(() => {
      clock = Date.now();
    }, 1000);
  });
  onDestroy(() => {
    if (clockTimer !== undefined) window.clearInterval(clockTimer);
  });

  $: syncMode = $appSettings.sync?.mode ?? "lan";
  $: lanHost = Boolean($appSettings.sync?.lanHost);
  $: lanName = ($appSettings.sync?.lanName ?? "").trim();
  $: lanPeer = ($appSettings.sync?.lanPeer ?? "").trim();
  $: lanPort = $appSettings.sync?.lanPort ?? 52177;
  $: hostStatus = syncStatus?.host ?? null;
  $: p2pInfo = syncStatus?.p2p ?? null;
  /** 枢纽的展示名：目录里学到的名字，没学过就显示 id 前缀 */
  $: p2pHubLabel = (() => {
    const info = p2pInfo;
    if (!info?.hubId) return "";
    if (info.hubIsSelf) return "本机";
    const hit = (info.peers ?? []).find((peer) => peer.id === info.hubId);
    return hit?.name || `${info.hubId.slice(0, 8)}…`;
  })();
  $: p2pSummary = !p2pInfo
    ? "运行时未启动"
    : !p2pInfo.running
      ? "运行时未启动"
      : p2pInfo.hubIsSelf === null || p2pInfo.hubIsSelf === undefined
        ? "解析目录中…"
        : p2pInfo.hubIsSelf
          ? `本机是枢纽 · ${p2pInfo.onlinePeers ?? 0} 台在线`
          : `枢纽「${p2pHubLabel}」 · ${p2pInfo.onlinePeers ?? 0} 台在线`;
  /** 设备浮层标题：在线数 + 枢纽是谁 */
  $: peersHeadline = !peers
    ? "解析中…"
    : `在线 ${peers.count} 台 · 枢纽 ${
        peers.hubIsSelf
          ? "本机"
          : peers.peers.find((peer) => peer.id === peers?.hubId)?.name || `${(peers.hubId ?? "").slice(0, 8)}…`
      }`;
  // 名字从别处变了（CLI、另一台设备的设置同步）就跟上，别把用户的草稿冲掉
  $: if (!lanNameFocused && lanNameDraft.trim() !== lanName) lanNameDraft = lanName;
  // 切进 P2P 模式时把自部署覆盖的当前值带进草稿（打字期间不回灌）
  $: if (p2pDraftsMode !== syncMode) {
    p2pDraftsMode = syncMode;
    p2pRelayDraft = $appSettings.sync?.p2pRelay ?? "";
    p2pDirectoryDraft = $appSettings.sync?.p2pDirectory ?? "";
  }

  /**
   * 配对 = 有账户密码 + 当前通信方式有一个明确对端（与 core 的 is_paired 同口径）。
   * enabled=false 只是「暂停」，配置全部保留。
   */
  $: syncPaired = Boolean(
    ($appSettings.sync?.username ?? "").trim() &&
      ($appSettings.sync?.secret ?? "").trim() &&
      (syncMode === "server"
        ? Boolean(($appSettings.sync?.serverUrl ?? "").trim())
        : syncMode === "lan"
          ? lanHost || Boolean(lanPeer)
          : true)
  );
  $: syncPaused = syncPaired && !$appSettings.sync?.enabled;
  $: connection = $syncConnection;
  /** 内置主机是否把端口上移了（配置端口被占用）——只有这种情况才值得占字符 */
  $: hostPortBumped = Boolean(hostStatus?.running) && (hostStatus?.port ?? 0) !== lanPort;
  $: lanEndpointText = syncStatus?.lanEndpoint
    ? `${syncStatus.lanEndpoint.host}:${syncStatus.lanEndpoint.port}`
    : "";
  /**
   * 整张卡片只有这一条状态指示：本机做主机时不再和「对端已连接」并排出现两个圆点。
   * 主机与自己仍然要同步（内置库就是别的设备来读的那一份），但用户只需要看到一个状态。
   */
  $: statusText =
    syncMode === "lan"
      ? lanHost
        ? hostStatus?.running
          ? `本机服务器 · 端口 ${hostStatus.port}${hostPortBumped ? `（${lanPort} 被占用）` : ""}${isMobile ? " · 需保持前台" : ""}`
          : hostStatus?.lastError
            ? `主机启动失败：${hostStatus.lastError}`
            : "主机启动中…"
        : lanPeer
          ? `主机「${lanPeer}」${lanEndpointText ? ` · ${lanEndpointText}` : ""}`
          : "未选定局域网主机"
      : syncMode === "p2p"
        ? p2pSummary
        : ($appSettings.sync?.serverUrl ?? "").trim() || "未填服务器地址";
  $: statusDot = lanHost && syncMode === "lan"
    ? hostStatus?.running
      ? connection.online === false
        ? "offline"
        : "online"
      : hostStatus?.lastError
        ? "offline"
        : "unknown"
    : connection.online === null
      ? "unknown"
      : connection.online
        ? "online"
        : "offline";
  /** 「开始同步」可点条件：账户密码齐了，且当前方式有明确对端 */
  $: canStartSync =
    Boolean(syncForm.username.trim()) &&
    Boolean(syncForm.secret) &&
    (syncMode === "server"
      ? Boolean(syncForm.serverUrl.trim())
      : syncMode === "lan"
        ? lanHost || Boolean(lanPeer)
        : true);
  // 倒计时：让「自动同步间隔到底有没有生效」一眼可见
  $: nextSyncText = !syncPaired || syncPaused
    ? ""
    : $nextSyncAt === null
      ? ""
      : `下次同步 ${Math.max(0, Math.round(($nextSyncAt - clock) / 1000))}s`;

  /** 最近同步压成一行：时间 + 拉/推，图片与冲突只在非零时才出现 */
  function formatLastSync(status: import("./actions").SyncStatus): string {
    if (!status.lastSyncAt) return "";
    const result = status.lastResult;
    const parts = [`拉 ${result?.pulled ?? 0}`, `推 ${result?.pushed ?? 0}`];
    if (result?.imagesPulled || result?.imagesPushed) {
      parts.push(`图 ↓${result.imagesPulled ?? 0} ↑${result.imagesPushed ?? 0}`);
    }
    if (result?.conflicts) parts.push(`冲突 ${result.conflicts}`);
    return `${new Date(status.lastSyncAt).toLocaleString()} · ${parts.join(" ")}`;
  }
  $: lastSyncText = syncStatus ? formatLastSync(syncStatus) : "";

  let syncStatusLoaded = false;
  // 未配对也要读状态：「本机作为服务器」是配对**之前**就要勾的，得看得到主机起没起来
  $: if ($showSettings && !syncStatusLoaded) {
    syncStatusLoaded = true;
    void refreshSyncStatus();
  }
  $: if (!$showSettings) syncStatusLoaded = false;
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

  /** 打开 P2P 设备列表：目录解析走 60s 缓存，第一次会打一次目录服务 */
  async function loadPeers(): Promise<void> {
    peersOpen = !peersOpen;
    if (!peersOpen || peersLoading) return;
    peersLoading = true;
    try {
      peers = await syncPeersAction();
      await refreshSyncStatus({ probe: false });
    } finally {
      peersLoading = false;
    }
  }

  /** 自部署覆盖提交（空串 = 恢复免费公共服务）；宿主收到设置事件会重启 P2P 运行时 */
  async function commitP2pOverrides(): Promise<void> {
    const relay = p2pRelayDraft.trim();
    const directory = p2pDirectoryDraft.trim();
    if (relay !== ($appSettings.sync?.p2pRelay ?? "") || directory !== ($appSettings.sync?.p2pDirectory ?? "")) {
      await setConfigAction("sync.p2pRelay", relay);
      await setConfigAction("sync.p2pDirectory", directory);
      await refreshSyncStatus({ probe: false });
    }
  }

  /** 放大镜：UDP 广播/组播查询局域网内的主机，结果以下拉浮层展示（点选即选定）。 */
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
        showToast("局域网内没发现主机：对方要开着 KXToDo 并勾选「本机作为服务器」");
      }
    } finally {
      discovering = false;
    }
  }

  /**
   * 点选发现结果：选定的是**名字**，不是 ip:port。
   * 名字是主机在局域网里的身份，路由器给它换了 IP、端口因占用上移了都照样连得上。
   */
  async function pickDiscovered(server: DiscoveredServer): Promise<void> {
    discoveryOpen = false;
    discovered = [];
    const name = (server.name ?? "").trim();
    if (!name) {
      showToast("这台主机没报名字，没法选定（把它的 KXToDo 升级到 v0.6.0 以上）");
      return;
    }
    if (await setSyncLanPeerAction(name)) {
      await refreshSyncStatus({ probe: false });
    }
  }

  /**
   * 内置服务器是在宿主的异步运行时里起的，描述符（实际端口/库身份）晚一点点才落盘。
   * 轮询到就绪为止，否则用户勾完就看到「未运行」，以为没生效。
   */
  async function waitForHost(wantRunning: boolean): Promise<void> {
    for (let attempt = 0; attempt < 20; attempt += 1) {
      const status = await syncStatusAction();
      if (status) syncStatus = status;
      if (Boolean(status?.host?.running) === wantRunning) return;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }

  async function changeMode(mode: SyncMode): Promise<void> {
    if (mode === syncMode) return;
    await setSyncModeAction(mode);
    await refreshSyncStatus({ probe: false });
  }

  /** 勾选/取消「本机作为服务器」：取消只停内置服务器，不影响自己继续当客户端。 */
  async function toggleLanHost(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      const next = !lanHost;
      // 名字先一起提交：core 要在局域网里查这个名字有没有被别的主机占用
      const ok = await setSyncLanHostAction(next, next ? lanNameDraft.trim() || lanName : undefined);
      if (ok) {
        await waitForHost(next);
        await refreshSyncStatus({ probe: false });
      }
    } finally {
      syncBusy = false;
    }
  }

  /** 提交主机名字（失焦/回车）。改名会重启内置服务器——名字就是它在局域网里的身份。 */
  async function commitLanName(): Promise<void> {
    lanNameFocused = false;
    const name = lanNameDraft.trim();
    if (!name || name === lanName) {
      lanNameDraft = lanName;
      return;
    }
    if (await setSyncLanNameAction(name)) {
      if (lanHost) await waitForHost(true);
      await refreshSyncStatus({ probe: false });
    } else {
      lanNameDraft = lanName;
    }
  }

  /** 打开本机内置主机的管理台（系统浏览器；地址是本机回环，手机上也一样能开）。 */
  async function openAdminConsole(): Promise<void> {
    const url = hostStatus?.adminUrl;
    if (!url) return;
    try {
      await openExternalUrl(url);
    } catch (error) {
      showToast(`打开管理台失败：${String(error)}`);
    }
  }

  async function toggleHistory(): Promise<void> {
    historyOpen = !historyOpen;
    if (historyOpen) await loadHistory();
  }

  async function loadHistory(): Promise<void> {
    history = (await syncHistoryAction()) ?? [];
  }

  /** 一键回填历史：通信方式与主机名/地址也一起恢复，不只是账户密码。 */
  async function applyHistory(entry: SyncHistoryEntry): Promise<void> {
    historyOpen = false;
    if (syncPaired) {
      showToast("当前已配对：先解除配对，再用历史账户回填");
      return;
    }
    const mode: SyncMode = entry.mode === "lan" ? "lan" : "server";
    syncForm.serverUrl = entry.serverUrl ?? "";
    syncForm.username = entry.username;
    syncForm.secret = entry.secret;
    if (mode !== syncMode) await setSyncModeAction(mode);
    const peer = (entry.lanPeer ?? "").trim();
    if (mode === "lan" && peer && peer !== lanPeer) await setSyncLanPeerAction(peer);
    await refreshSyncStatus({ probe: false });
    showToast("已回填历史配对信息");
  }

  async function removeHistory(index: number): Promise<void> {
    history = (await syncHistoryRemoveAction(index)) ?? [];
  }

  /** 展开账户输入框（已配对时用）：把设置里的账户密码带进表单，保存即重新配对一次 */
  function beginEditAccount(): void {
    syncForm.serverUrl = $appSettings.sync?.serverUrl || "";
    syncForm.username = $appSettings.sync?.username || "";
    syncForm.secret = $appSettings.sync?.secret || "";
    accountEditing = true;
  }

  function cancelEditAccount(): void {
    accountEditing = false;
    syncFormPrefilled = false;
  }

  /**
   * 统一「开始同步」：账户不存在就当场注册，存在就登录。
   * 用户不需要知道这台设备上有没有注册过——由服务端回答。
   */
  async function startSync(): Promise<void> {
    if (syncBusy) return;
    syncBusy = true;
    try {
      const ok = await syncPairAction({
        mode: syncMode,
        serverUrl: syncMode === "server" ? syncForm.serverUrl.trim() : undefined,
        username: syncForm.username,
        secret: syncForm.secret,
        syncSettings: $appSettings.sync?.syncSettings ?? true,
        syncSchedules: $appSettings.sync?.syncSchedules ?? false
      });
      if (ok) {
        syncFormPrefilled = false;
        accountEditing = false;
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
            {#each history as entry, index ((entry.mode ?? "") + "|" + entry.serverUrl + "|" + (entry.lanPeer ?? "") + "|" + entry.username)}
              <div class="sync-popover-row">
                <button class="menu-action-button discovery-item" type="button" on:click={() => applyHistory(entry)}>
                  <span class="discovery-name">{entry.username || "（无用户名）"}</span>
                  <span class="muted">{syncHistoryLabel(entry)}</span>
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

    <div class="sync-card">
      <div class="settings-row sync-mode-row">
        <span>通信方式</span>
        <Dropdown
          ariaLabel="同步通信方式"
          value={syncMode}
          options={syncModeOptions}
          on:change={(event) => changeMode(event.detail as SyncMode)}
        />
      </div>

      {#if syncMode === "lan"}
        <div class="settings-row" title="内置服务器随应用启停，同一局域网里的其它设备用「发现」就能连上它">
          <span>本机作为服务器</span>
          <input type="checkbox" checked={lanHost} disabled={syncBusy} on:change={toggleLanHost} />
        </div>
        {#if lanHost}
          <label class="settings-row">
            主机名字
            <input
              bind:value={lanNameDraft}
              placeholder="局域网内唯一"
              disabled={syncBusy}
              on:focus={() => (lanNameFocused = true)}
              on:blur={commitLanName}
              on:keydown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commitLanName();
                }
              }}
            />
          </label>
          {#if hostStatus?.running}
            <div class="settings-row">
              <span>管理台</span>
              <span class="sync-side admin-cell">
                <span class="muted" title={hostStatus?.adminUrl}>
                  {hostStatus?.adminUser}
                  {#if adminSecretVisible && hostStatus?.adminPassword} / {hostStatus.adminPassword}{/if}
                </span>
                <button
                  class="icon-button"
                  type="button"
                  title={adminSecretVisible ? "隐藏密码" : "显示密码"}
                  aria-label={adminSecretVisible ? "隐藏密码" : "显示密码"}
                  on:click={() => (adminSecretVisible = !adminSecretVisible)}
                >{#if adminSecretVisible}<EyeOff size={13} />{:else}<Eye size={13} />{/if}</button>
                <button class="settings-button" type="button" on:click={openAdminConsole}>打开</button>
              </span>
            </div>
          {/if}
        {:else}
          <div class="settings-row sync-peer-row">
            <span title="选定的是主机名字：它换了 IP、换了端口都照样连得上">局域网主机</span>
            <span class="muted sync-side">{lanPeer ? `「${lanPeer}」` : "未选定"}</span>
            <button class="settings-button" type="button" disabled={discovering} on:click={runDiscovery}>
              {discovering ? "搜索中…" : "发现"}
            </button>
          </div>
          {#if discoveryOpen}
            <div class="sync-popover">
              <div class="sync-popover-head">
                <span>{discovering ? "正在搜索…" : `发现 ${discovered.length} 台主机`}</span>
                <span class="spacer"></span>
                <button class="icon-button" type="button" title="关闭" aria-label="关闭" on:click={() => (discoveryOpen = false)}><X size={14} /></button>
              </div>
              {#if discovering}
                <p class="muted">正在搜索局域网内的主机…</p>
              {:else if discovered.length === 0}
                <p class="muted">没发现主机（对方需开着应用并勾选「本机作为服务器」）</p>
              {:else}
                <div class="sync-popover-list">
                  {#each discovered as server (server.url)}
                    <button class="menu-action-button discovery-item" type="button" on:click={() => pickDiscovered(server)}>
                      <span class="discovery-name">{server.name || "未命名主机"}</span>
                      <span class="muted">{server.host}:{server.port}</span>
                      {#if !server.verified}<span class="muted">（未复核）</span>{/if}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      {/if}

      {#if syncMode === "p2p"}
        <div class="settings-row sync-peer-row">
          <span title="同账户的设备靠目录互相发现；每轮连「枢纽」（目录里最小的那台），对方不在线就按重连间隔静默重试">设备</span>
          <span class="muted sync-side">{p2pInfo?.running ? `${p2pInfo.onlinePeers ?? 0} 台在线` : "未启动"}</span>
          <button class="settings-button" type="button" disabled={peersLoading} on:click={loadPeers}>
            {peersLoading ? "解析中…" : "列表"}
          </button>
        </div>
        {#if peersOpen}
          <div class="sync-popover">
            <div class="sync-popover-head">
              <span>{peersHeadline}</span>
              <span class="spacer"></span>
              <button class="icon-button" type="button" title="关闭" aria-label="关闭" on:click={() => (peersOpen = false)}><X size={14} /></button>
            </div>
            {#if !peers}
              <p class="muted">正在解析账户目录…</p>
            {:else if peers.peers.length === 0}
              <p class="muted">目录里只有本机：另一台设备配对同一账户后会出现</p>
            {:else}
              <div class="sync-popover-list">
                {#each peers?.peers ?? [] as peer (peer.id)}
                  <div class="sync-popover-row">
                    <span class="discovery-item">
                      <span class="discovery-name">{peer.name || `${peer.id.slice(0, 8)}…`}</span>
                      {#if peer.id === peers?.hubId}<span class="muted">枢纽</span>{/if}
                      {#if peer.lastOk === false}<span class="muted">上次拨号失败</span>{/if}
                    </span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
        {#if p2pAdvancedOpen}
          <label class="settings-row" title="自部署 iroh relay 地址；留空 = n0 免费公共服务；disabled = 不用 relay">
            relay
            <input
              bind:value={p2pRelayDraft}
              placeholder="留空 = 免费公共服务"
              on:blur={commitP2pOverrides}
              on:keydown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commitP2pOverrides();
                }
              }}
            />
          </label>
          <label class="settings-row" title="自部署 pkarr 目录地址；留空 = n0 免费公共服务">
            目录
            <input
              bind:value={p2pDirectoryDraft}
              placeholder="留空 = 免费公共服务"
              on:blur={commitP2pOverrides}
              on:keydown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commitP2pOverrides();
                }
              }}
            />
          </label>
        {:else}
          <div class="settings-row">
            <span>自部署</span>
            <span class="sync-side">
              <button class="settings-button" type="button" on:click={() => (p2pAdvancedOpen = true)}>relay / 目录</button>
            </span>
          </div>
        {/if}
      {/if}

      <div class="sync-status" data-state={statusDot}>
        <i class="conn-dot"></i>
        <span class="sync-status-text" title={statusText}>{statusText}</span>
        {#if nextSyncText}<span class="sync-status-meta">{nextSyncText}</span>{/if}
      </div>

      {#if syncPaused}
        <p class="sync-note">同步已暂停</p>
      {/if}
      {#if syncPaired && connection.online === false && connection.lastError}
        <p class="update-error">{connection.lastError}</p>
      {/if}

      {#if syncPaired && !accountEditing}
        <div class="settings-row">
          <span>账户</span>
          <span class="sync-side account-cell">
            <span class="muted">{$appSettings.sync.username}</span>
            <button class="settings-button" type="button" disabled={syncBusy} on:click={beginEditAccount}>修改</button>
          </span>
        </div>
        {#if lastSyncText}
          <div class="settings-row">
            <span>最近同步</span>
            <span class="muted sync-side">{lastSyncText}</span>
          </div>
        {/if}
      {:else}
        {#if syncMode === "server"}
          <label class="settings-row">
            服务器地址
            <input bind:value={syncForm.serverUrl} placeholder="http://192.168.1.10:52177" />
          </label>
        {/if}
        <label class="settings-row">
          用户名
          <input bind:value={syncForm.username} placeholder="账户名" />
        </label>
        <label class="settings-row">
          密码
          <input type="password" bind:value={syncForm.secret} placeholder="派生加密密钥，丢失无法找回" />
        </label>
      {/if}

      <div class="settings-row sync-scope-row">
        <span class="sync-group-label">同步内容</span>
        <label class="sync-scope" title="节点、任务与插图">
          <input type="checkbox" checked={$appSettings.sync.syncData} on:change={(event) => updateSyncScope("syncData", event.currentTarget.checked)} />数据
        </label>
        <label class="sync-scope" title="配置、配色与背景">
          <input type="checkbox" checked={$appSettings.sync.syncSettings} on:change={(event) => updateSyncScope("syncSettings", event.currentTarget.checked)} />设置
        </label>
        <label class="sync-scope" title="定时任务：跨平台路径通常不可执行">
          <input type="checkbox" checked={$appSettings.sync.syncSchedules} on:change={(event) => updateSyncScope("syncSchedules", event.currentTarget.checked)} />任务
        </label>
      </div>
      <div class="settings-row sync-rate-row">
        <label class="sync-rate">
          同步间隔
          <NumberField
            ariaLabel="自动同步间隔（秒）"
            suffix="s"
            min={5}
            max={86400}
            value={$appSettings.sync.intervalSeconds || 30}
            onCommit={(v) => updateSyncInterval(v)}
          />
        </label>
        <label class="sync-rate">
          掉线重连
          <NumberField
            ariaLabel="掉线重连间隔（秒）"
            suffix="s"
            min={5}
            max={86400}
            value={$appSettings.sync.reconnectSeconds || 300}
            onCommit={(v) => updateReconnectSeconds(v)}
          />
        </label>
      </div>

      {#if accountEditing}
        <div class="settings-row sync-actions two">
          <button class="settings-button" type="button" disabled={syncBusy} on:click={cancelEditAccount}>取消</button>
          <button class="settings-button primary" type="button" disabled={syncBusy || !canStartSync} on:click={startSync}>
            {syncBusy ? "处理中…" : "保存并同步"}
          </button>
        </div>
      {:else if syncPaired}
        <div class="settings-row sync-actions">
          <button class="settings-button" type="button" disabled={syncBusy} on:click={unpairSync}>解除配对</button>
          <button class="settings-button" type="button" disabled={syncBusy} on:click={togglePause}>
            {syncPaused ? "恢复同步" : "暂停同步"}
          </button>
          <button class="settings-button primary" type="button" disabled={syncBusy || syncPaused} on:click={runSyncNow}>
            {syncBusy ? "同步中…" : "立即同步"}
          </button>
        </div>
      {:else}
        <div class="settings-row sync-actions single">
          <button class="settings-button primary" type="button" disabled={syncBusy || !canStartSync} on:click={startSync}>
            {syncBusy ? "处理中…" : "开始同步"}
          </button>
        </div>
      {/if}
    </div>
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

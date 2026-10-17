<script lang="ts">
  /**
   * 剪切板同步（v0.8.9）：工具箱里的一个工具页。
   *
   * 设置一次写整份（`configureClipboard` → `gui.clipboard-configure`）：逐字段保存会让
   * 后台服务按字段数重启。总开关是唯一「一键生效」的入口，改完立即应用；
   * 其它字段改完点「保存并应用」。
   */
  import { onDestroy, onMount } from "svelte";
  import { ClipboardCopy, Copy, RefreshCw, Save, Wifi } from "@lucide/svelte";
  import { appSettings, showToast } from "../stores";
  import { configureClipboard } from "../actions";
  import { isMobile } from "../platform";
  import { copyText } from "../clipboard";
  import { clipboardDefaultDir, pickDirectory } from "../backend";
  import { clipboardSyncStatus, refreshClipboardStatus } from "../clipboardSyncStore";
  import type { Settings } from "../types";

  type Draft = Settings["clipboard"];
  const copy = (value: Draft): Draft => JSON.parse(JSON.stringify(value)) as Draft;

  let draft = copy($appSettings.clipboard);
  let lastSaved = $appSettings.clipboard;
  let saving = false;
  let defaultDir = "";
  let timer: number | undefined;

  $: if ($appSettings.clipboard !== lastSaved) {
    lastSaved = $appSettings.clipboard;
    draft = copy($appSettings.clipboard);
  }
  $: dirty = JSON.stringify(draft) !== JSON.stringify($appSettings.clipboard);
  $: codeOk = draft.code.trim().length >= 8;
  $: anyTransport = draft.lanEnabled || draft.p2pEnabled || draft.directEnabled;
  $: status = $clipboardSyncStatus;
  $: transportLabels = { direct: "直接 IP", lan: "局域网", p2p: "P2P" } as Record<string, string>;

  onMount(() => {
    void refreshClipboardStatus();
    void clipboardDefaultDir().then((dir) => {
      defaultDir = dir ?? "";
    }).catch(() => undefined);
    timer = window.setInterval(() => void refreshClipboardStatus(), 3000);
  });
  onDestroy(() => {
    if (timer) window.clearInterval(timer);
  });

  function looksLikeAddressList(raw: string): boolean {
    return raw
      .split(/[\s,;]+/)
      .filter(Boolean)
      .every((item) => /^\[?[0-9a-fA-F:.]+\]?(:\d{1,5})?$/.test(item));
  }

  async function save(enable?: boolean): Promise<void> {
    const next: Draft = { ...draft, enabled: enable ?? draft.enabled };
    const limits = [next.limits.textMb, next.limits.htmlMb, next.limits.imageMb, next.limits.filesMb];
    if (limits.some((value) => !Number.isFinite(value) || value < 1)) {
      showToast("大小上限需要填写 ≥1 的整数（MB）", 4000);
      return;
    }
    // 与 core normalize_settings 的夹取区间对齐，保存后界面不出现「填 999 显示 64」
    next.limits = {
      textMb: Math.min(64, Math.max(1, Math.round(next.limits.textMb))),
      htmlMb: Math.min(64, Math.max(1, Math.round(next.limits.htmlMb))),
      imageMb: Math.min(256, Math.max(1, Math.round(next.limits.imageMb))),
      filesMb: Math.min(1024, Math.max(1, Math.round(next.limits.filesMb)))
    };
    if (next.enabled) {
      if (!codeOk) {
        showToast("请先设置至少 8 位同步口令", 4000);
        return;
      }
      if (!anyTransport) {
        showToast("请至少启用一种同步方式", 4000);
        return;
      }
      if (next.directEnabled && next.directAddresses.trim() && !looksLikeAddressList(next.directAddresses)) {
        showToast("直接 IP 列表格式不对：一行一个 ip 或 ip:port", 5000);
        return;
      }
    }
    draft = next;
    saving = true;
    const ok = await configureClipboard(next);
    saving = false;
    if (!ok) return;
    lastSaved = $appSettings.clipboard;
    await refreshClipboardStatus();
    showToast(next.enabled ? "剪切板同步已保存并应用" : "剪切板同步已关闭", 2500);
  }

  function generateCode(): void {
    const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    const bytes = new Uint8Array(12);
    crypto.getRandomValues(bytes);
    draft = { ...draft, code: [...bytes].map((byte) => alphabet[byte % alphabet.length]).join("") };
  }

  async function copyCode(): Promise<void> {
    if (!draft.code) return;
    if (await copyText(draft.code)) showToast("口令已复制，去另一台设备填同一句", 3000);
  }

  async function chooseDir(): Promise<void> {
    const dir = await pickDirectory();
    if (dir) draft = { ...draft, saveDir: dir };
  }

  function resetDir(): void {
    draft = { ...draft, saveDir: defaultDir };
  }

  function transportBadge(id: string): string {
    return transportLabels[id] ?? id;
  }
</script>

<div class="toolbox-sub clipboard-page">
  <div class="clipboard-hero">
    <div class="clipboard-hero-icon"><ClipboardCopy size={24} /></div>
    <div>
      <strong>剪切板同步</strong>
      <p>复制即同步，不保存历史。同一句口令的设备按「直接 IP → 局域网 → P2P」择优发送。</p>
    </div>
    <label class="clipboard-master">
      <input
        type="checkbox"
        checked={draft.enabled}
        disabled={saving}
        on:change={(event) => void save((event.currentTarget as HTMLInputElement).checked)}
      />
      <span>{draft.enabled ? "已开启" : "已关闭"}</span>
    </label>
  </div>

  {#if $isMobile}
    <p class="clipboard-note">
      Android 10 起系统只允许前台应用读取剪切板：本机复制内容仅在本应用前台时同步；
      远端写入也需要本应用在前台。这是系统限制，不是开关没生效。
    </p>
  {/if}

  <section class="clipboard-card">
    <div class="clipboard-status-line">
      <span class="clipboard-dot" class:on={status?.running}></span>
      <strong>{status?.running ? "运行中" : status ? (status.enabled ? "未运行" : "未启用") : draft.enabled ? "当前环境不支持" : "未启用"}</strong>
      {#if status?.deviceId}
        <span class="clipboard-mono" title={status.deviceId}>{status.deviceId.slice(0, 12)}</span>
      {/if}
      <button class="menu-action-button" type="button" on:click={() => void refreshClipboardStatus()}>
        <RefreshCw size={13} /> 刷新
      </button>
    </div>
    {#if status?.lastError}
      {#if status.running}
        <p class="clipboard-error">{status.lastError}</p>
      {:else}
        <p class="clipboard-meta">{status.lastError}</p>
      {/if}
    {/if}
    {#if status?.running}
      <p class="clipboard-meta">
        <Wifi size={13} />
        本机 {status.deviceName} · 端口 {status.port} · 发现端口 {status.discoveryPort}
        {#if status.localAddresses.length} · {status.localAddresses.join(" / ")}{/if}
      </p>
      <p class="clipboard-meta">保存目录：{status.saveDir || defaultDir || "下载目录/kxtodo-clipboard"}</p>
      {#if status.lastEvent}
        <p class="clipboard-meta">最近：{status.lastEvent.detail}{status.lastEvent.peer ? ` · ${status.lastEvent.peer}` : ""}</p>
      {/if}
      <div class="clipboard-peers">
        {#if status.peers.length === 0}
          <span class="clipboard-empty">还没有发现在线设备（设备需要开启同一口令、同一局域网/账号）。</span>
        {:else}
          {#each status.peers as peer (peer.id)}
            <span class="clipboard-peer" title={peer.id}>
              <span class="clipboard-dot on"></span>
              <strong>{peer.name || "未命名设备"}</strong>
              {#each peer.transports as transport (transport)}
                <em>{transportBadge(transport)}</em>
              {/each}
            </span>
          {/each}
        {/if}
      </div>
    {:else if status?.enabled}
      <p class="clipboard-meta">核心服务会在保存后自动启动；若一直停着，检查口令与同步方式。</p>
    {/if}
  </section>

  <section class="clipboard-card">
    <h3>配对</h3>
    <div class="clipboard-code-row">
      <input class="clipboard-text-input" value={draft.code} spellcheck="false" placeholder="至少 8 位同步口令" on:input={(event) => (draft = { ...draft, code: (event.currentTarget as HTMLInputElement).value })} />
      <button class="menu-action-button" type="button" title="生成随机口令" on:click={generateCode}>生成</button>
      <button class="menu-action-button" type="button" title="复制口令" disabled={!draft.code} on:click={() => void copyCode()}><Copy size={13} />复制</button>
    </div>
    <p class="clipboard-hint">同一句口令 = 同一组设备；LAN、直接 IP、P2P 三种方式共用，不会明文传输。P2P 用口令在 pkarr 房间互认，不复用同步账户（剪贴板事件不进同步数据），relay 沿用文件传输/同步的设置。</p>
    <label class="clipboard-field">
      <span>本机设备名</span>
      <input class="clipboard-text-input" value={draft.deviceName} placeholder="留空显示 KXToDo·平台" on:input={(event) => (draft = { ...draft, deviceName: (event.currentTarget as HTMLInputElement).value })} />
    </label>
  </section>

  <section class="clipboard-card">
    <h3>同步方式</h3>
    <label class="toggle-row">
      <span><strong>直接 IP</strong><small>填了地址就优先走它（仍需同一口令）</small></span>
      <input type="checkbox" bind:checked={draft.directEnabled} />
    </label>
    {#if draft.directEnabled}
      <textarea class="clipboard-addresses" rows="2" placeholder="192.168.1.20 或 192.168.1.20:52179（一行一个）" value={draft.directAddresses} on:input={(event) => (draft = { ...draft, directAddresses: (event.currentTarget as HTMLTextAreaElement).value })}></textarea>
    {/if}
    <label class="toggle-row">
      <span><strong>局域网发现</strong><small>UDP 广播自动找同一局域网里的设备，最省事</small></span>
      <input type="checkbox" bind:checked={draft.lanEnabled} />
    </label>
    <label class="toggle-row">
      <span><strong>P2P（异地）</strong><small>iroh 打洞，relay 沿用文件传输/同步的设置</small></span>
      <input type="checkbox" bind:checked={draft.p2pEnabled} />
    </label>
  </section>

  <section class="clipboard-card">
    <h3>接收类型与上限</h3>
    <div class="clipboard-types">
      <div class="clipboard-type-row">
        <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.text} />文字</label>
        <input class="clipboard-limit" type="number" min="1" max="64" bind:value={draft.limits.textMb} /><span>MB</span>
      </div>
      <div class="clipboard-type-row">
        <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.html} />HTML</label>
        <input class="clipboard-limit" type="number" min="1" max="64" bind:value={draft.limits.htmlMb} /><span>MB</span>
      </div>
      <div class="clipboard-type-row">
        <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.image} />图片</label>
        <input class="clipboard-limit" type="number" min="1" max="256" bind:value={draft.limits.imageMb} /><span>MB</span>
      </div>
      <div class="clipboard-type-row">
        <label class="checkbox-line"><input type="checkbox" bind:checked={draft.types.files} />文件</label>
        <input class="clipboard-limit" type="number" min="1" max="1024" bind:value={draft.limits.filesMb} /><span>MB</span>
      </div>
    </div>
    <p class="clipboard-hint">超过上限或关闭了该类型时，远端内容会被丢弃；整条都不可用则不写本机剪切板。</p>
  </section>

  <section class="clipboard-card">
    <h3>文件保存位置</h3>
    <div class="clipboard-code-row">
      <input class="clipboard-text-input" value={draft.saveDir} placeholder={defaultDir || "下载目录/kxtodo-clipboard"} on:input={(event) => (draft = { ...draft, saveDir: (event.currentTarget as HTMLInputElement).value })} />
      <button class="menu-action-button" type="button" on:click={() => void chooseDir()}>选择</button>
      <button class="menu-action-button" type="button" on:click={resetDir}>恢复默认</button>
    </div>
    <p class="clipboard-hint">只有收到的文件会落盘；文字、HTML 和图片只写系统剪切板，不写历史。</p>
  </section>

  <div class="clipboard-actions">
    <button class="settings-button primary" type="button" disabled={saving || !dirty} on:click={() => void save()}>
      <Save size={15} /> {saving ? "保存中…" : "保存并应用"}
    </button>
  </div>
</div>

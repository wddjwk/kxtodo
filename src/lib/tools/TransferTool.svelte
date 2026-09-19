<script lang="ts">
  /**
   * 文件传输助手（v0.8.4 重做交互；v0.8.5 状态提升 + 生命周期定案）。
   *
   * 形态照 LocalSend，**分区卡片**：身份区（设备名 + 口令 + 在线状态）/ 收发区
   * （滑块 + 四图标 + 已选清单 + 设备卡 + 发送按钮）/ 传输中 / 已完成 / 历史。
   *
   * 状态全部住在 `transferStore.ts`（模块级）：工具页只是它的一层视图，退出页面
   * 不回滚任何东西——已配对保持在线、传输继续、进度与收到的文本都还在。页面的
   * 职责只剩「把 store 画出来」与「把交互转给 store 的动作」。
   */
  import { onDestroy, onMount } from "svelte";
  import { ArrowLeftRight, Clipboard, FileText, FolderUp, PencilLine, ShieldCheck, Upload, X } from "@lucide/svelte";
  import { appSettings, showToast } from "../stores";
  import { setConfig } from "../actions";
  import { caps } from "../capabilities";
  import {
    isTauriRuntime, pickAnyFiles, pickDirectory, transferListFolder, transferOutboxPath, transferSaveText,
    transferSpoolClear, transferSpoolWrite, transferStatFiles
  } from "../backend";
  import {
    addPicked, applyAutoAccept, applyDeviceName, cancelSession, clearHistory, clearPicked, decideRequest,
    dismissSession, ensureTransferRuntime, goOffline, goOnline, leaveTransferPage, refreshHistory, removePicked,
    selectDevice, setCode, setSaveDir, setTab, setTransferViewOpen, startSend, transferState,
    type PickedItem, type SendKind, type TransferSession
  } from "../transferStore";

  const CODE_MIN = 8;

  $: state = $transferState;
  $: codeOk = state.code.trim().length >= CODE_MIN;
  $: activeSessions = state.sessions.filter((item) => item.state === "waiting" || item.state === "connected");
  $: finishedSessions = state.sessions.filter((item) => item.state !== "waiting" && item.state !== "connected" && !item.collapsed);
  $: sendBytes = state.picked.reduce((sum, item) => sum + item.size, 0);
  $: settings = $appSettings.transfer;
  $: deviceName = settings?.deviceName ?? "";
  $: autoAccept = Boolean(settings?.autoAccept);

  let codeVisible = false;
  let deviceNameDraft = "";
  let deviceNameFocused = false;
  let textDraft = "";
  let textPopOpen = false;
  let historyOpen = false;
  let spooling = false;
  let unsubscribeDrop: (() => void) | null = null;

  // 设备名输入框：聚焦时显示草稿，其余时候跟着设置走（同步拉回来的名字也能看到）
  $: if (!deviceNameFocused && deviceNameDraft !== deviceName) deviceNameDraft = deviceName;

  onMount(() => {
    setTransferViewOpen(true);
    ensureTransferRuntime();
    void refreshHistory();
    // 桌面：拖文件 / 文件夹进发送区直接进清单（需求 1.7）
    if (isTauriRuntime && caps.desktop) {
      void (async () => {
        try {
          const { getCurrentWebview } = await import("@tauri-apps/api/webview");
          unsubscribeDrop = await getCurrentWebview().onDragDropEvent((event) => {
            if (event.payload.type !== "drop") return;
            const paths = event.payload.paths ?? [];
            if (paths.length === 0) return;
            void addDroppedPaths(paths);
          });
        } catch {
          // 不支持拖放的环境（浏览器预览）静默跳过
        }
      })();
    }
  });

  onDestroy(() => {
    setTransferViewOpen(false);
    unsubscribeDrop?.();
    // 已配对保持在线、有任务继续传：清理与否由 store 按生命周期规则决定
    leaveTransferPage();
  });

  async function commitDeviceName(): Promise<void> {
    const value = deviceNameDraft.trim();
    deviceNameFocused = false;
    if (value === deviceName) return;
    await applyDeviceName(value);
  }

  // ---- 发送：选东西 ----

  async function pickFiles(): Promise<void> {
    if (caps.nativeFileDialogs) {
      const paths = await pickAnyFiles();
      if (paths.length === 0) return;
      const stats = await transferStatFiles(paths).catch(() => []);
      addPicked(
        stats.map((item) => ({
          key: `f:${item.rel}`,
          rel: item.rel,
          size: item.size,
          kind: "file" as SendKind,
          preview: item.rel.split(/[\\/]/).pop() ?? item.rel
        }))
      );
      return;
    }
    document.getElementById("transfer-file-input")?.click();
  }

  async function pickFolder(): Promise<void> {
    const dir = await pickDirectory();
    if (!dir) {
      showToast(caps.nativeFileDialogs ? "没选文件夹" : "移动端暂不支持发文件夹");
      return;
    }
    try {
      const items = await transferListFolder(dir);
      if (items.length === 0) {
        showToast("这个文件夹里没有文件");
        return;
      }
      addPicked([
        {
          key: `d:${dir}`,
          rel: dir,
          size: items.reduce((sum, item) => sum + item.size, 0),
          kind: "folder",
          preview: dir.split(/[\\/]/).filter(Boolean).pop() ?? dir
        }
      ]);
    } catch (error) {
      showToast(`读取文件夹失败：${String(error)}`);
    }
  }

  function openTextPop(): void {
    textPopOpen = true;
    textDraft = "";
  }

  function commitText(): void {
    const text = textDraft.trim();
    if (!text) {
      textPopOpen = false;
      return;
    }
    addPicked([{ key: `t:${Date.now()}`, rel: text, size: text.length, kind: "text", preview: text.slice(0, 40) }]);
    textPopOpen = false;
    textDraft = "";
  }

  /** 剪贴板：是图片就按文件处理（分块暂存到 outbox），是文本就进文本清单（需求 1.3）。 */
  async function pickClipboard(): Promise<void> {
    // 先看图片：navigator.clipboard.read() 能拿到 image/png 的 blob
    try {
      const clipItems = await navigator.clipboard.read();
      for (const entry of clipItems) {
        const type = entry.types.find((value) => value.startsWith("image/"));
        if (!type) continue;
        const blob = await entry.getType(type);
        const rel = `剪贴板图片-${Date.now()}.png`;
        const bytes = new Uint8Array(await blob.arrayBuffer());
        let offset = 0;
        while (offset < bytes.length) {
          const slice = bytes.subarray(offset, offset + 512 * 1024);
          let binary = "";
          for (let i = 0; i < slice.length; i++) binary += String.fromCharCode(slice[i]);
          await transferSpoolWrite(rel, btoa(binary), offset > 0);
          offset += slice.length;
        }
        const root = await transferOutboxPath();
        addPicked([{ key: `c:${rel}`, rel: `${root}\\${rel}`, size: bytes.length, kind: "clipboard", preview: rel }]);
        return;
      }
    } catch {
      // 没有图片权限 / 剪贴板里没有图片：落到文本分支
    }
    let text = "";
    try {
      text = await navigator.clipboard.readText();
    } catch {
      text = "";
    }
    if (text.trim()) {
      addPicked([{ key: `c:${Date.now()}`, rel: text, size: text.length, kind: "clipboard", preview: text.slice(0, 40) }]);
      return;
    }
    showToast("剪贴板里没有文本或图片");
  }

  /** 拖进来的路径：文件夹递归展开成清单，文件直接进（需求 1.7）。 */
  async function addDroppedPaths(paths: string[]): Promise<void> {
    const next: PickedItem[] = [];
    for (const path of paths) {
      try {
        const listed = await transferListFolder(path);
        if (listed.length > 0) {
          next.push({
            key: `d:${path}`,
            rel: path,
            size: listed.reduce((sum, item) => sum + item.size, 0),
            kind: "folder",
            preview: path.split(/[\\/]/).filter(Boolean).pop() ?? path
          });
          continue;
        }
      } catch {
        // 不是文件夹：当文件处理
      }
      const stats = await transferStatFiles([path]);
      const item = stats[0];
      if (item) {
        next.push({
          key: `f:${path}`,
          rel: path,
          size: item.size,
          kind: "file",
          preview: path.split(/[\\/]/).pop() ?? path
        });
      }
    }
    if (next.length === 0) {
      showToast("拖进来的内容为空");
      return;
    }
    addPicked(next);
  }

  // ---- 接收侧动作 ----

  async function chooseSaveDir(): Promise<void> {
    const dir = await pickDirectory();
    if (dir) await setSaveDir(dir);
  }

  async function openSaveDir(): Promise<void> {
    if (!isTauriRuntime) {
      showToast("浏览器预览里没有「打开文件夹」");
      return;
    }
    const { transferOpenPath } = await import("../backend");
    await transferOpenPath(state.saveDir).catch((error: unknown) => showToast(String(error)));
  }

  async function copyText(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      showToast("已复制");
    } catch {
      showToast("复制失败");
    }
  }

  async function saveTextAsFile(text: string): Promise<void> {
    if (!isTauriRuntime) {
      showToast("浏览器预览不支持另存为");
      return;
    }
    try {
      // 桌面：系统「另存为」；移动端不走对话框——Android 的 save() 返回 content:// URI，
      // Rust 侧写不了那个东西（pitfalls-android #4），直接落进接收目录，
      // 与收到的文件同一个地方、同一套「打开文件夹」入口
      if (!caps.nativeFileDialogs) {
        const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
        await transferSaveText(`${state.saveDir}/收到的文本-${stamp}.txt`, text);
        showToast("已保存到接收目录");
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: `${state.saveDir}\\收到的文本.txt` });
      if (!path) return;
      await transferSaveText(path, text);
      showToast("已保存为 .txt");
    } catch (error) {
      showToast(`保存失败：${String(error)}`);
    }
  }

  // ---- 移动端发送：webview 文件输入 → 分块暂存（scoped storage 下只能这样拿到内容） ----

  async function spoolFromInput(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement | null;
    const files = [...(input?.files ?? [])];
    if (input) input.value = "";
    if (files.length === 0) return;
    spooling = true;
    try {
      const root = await transferOutboxPath();
      await transferSpoolClear().catch(() => undefined);
      const items: PickedItem[] = [];
      const seen = new Set<string>();
      for (const file of files) {
        let rel = file.name.replace(/[\\/]/g, "_");
        let bump = 2;
        while (seen.has(rel)) rel = `${bump++}-${file.name.replace(/[\\/]/g, "_")}`;
        seen.add(rel);
        // 分块写：一次把整个文件读进内存对大文件不友好
        let offset = 0;
        while (offset < file.size) {
          const blob = file.slice(offset, offset + 512 * 1024);
          const bytes = new Uint8Array(await blob.arrayBuffer());
          let binary = "";
          for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
          await transferSpoolWrite(rel, btoa(binary), offset > 0);
          offset += bytes.length;
        }
        if (file.size === 0) await transferSpoolWrite(rel, "", false);
        items.push({ key: `f:${root}\\${rel}`, rel: `${root}\\${rel}`, size: file.size, kind: "file", preview: rel });
      }
      addPicked(items);
    } catch (error) {
      showToast(`暂存文件失败：${String(error)}`);
    } finally {
      spooling = false;
    }
  }

  // ---- 展示辅助 ----

  function sizeText(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  function percentOf(session: TransferSession): number {
    const total = session.totalBytes || session.files.reduce((sum, item) => sum + item.total, 0);
    if (total <= 0) return 0;
    const sent = session.files.reduce((sum, item) => sum + Math.min(item.sent, item.total || item.sent), 0);
    return Math.min(100, Math.round((sent / total) * 100));
  }

  function etaText(session: TransferSession): string {
    const sent = session.files.reduce((sum, item) => sum + item.sent, 0);
    const total = session.totalBytes || session.files.reduce((sum, item) => sum + item.total, 0);
    if (session.speed <= 0 || total <= sent) return "";
    const left = (total - sent) / session.speed;
    if (left > 3600) return `剩 ${Math.round(left / 3600)} 小时`;
    if (left > 60) return `剩 ${Math.round(left / 60)} 分`;
    return `剩 ${Math.max(1, Math.round(left))} 秒`;
  }

  function currentFile(session: TransferSession): string {
    const active = session.files.find((item) => !item.done);
    const name = (active ?? session.files[session.files.length - 1])?.rel ?? "";
    return name.split(/[\\/]/).pop() ?? name;
  }
</script>

<div class="transfer-page">
  <!-- 第一块：本机身份（设备名 + 配对口令 + 在线状态） -->
  <section class="transfer-card transfer-identity">
    <label class="transfer-field">
      <span>设备名称</span>
      <input
        bind:value={deviceNameDraft}
        placeholder="输入设备名，以向对方展示您的身份"
        maxlength="32"
        on:focus={() => (deviceNameFocused = true)}
        on:blur={() => void commitDeviceName()}
        on:keydown={(event) => { if (event.key === "Enter") (event.target as HTMLInputElement).blur(); }}
      />
    </label>
    <label class="transfer-field">
      <span>配对口令</span>
      <div class="transfer-code-row">
        <input
          value={state.code}
          type={codeVisible ? "text" : "password"}
          placeholder="输入和对方约定的密钥（至少8位）"
          autocomplete="off"
          on:input={(event) => setCode(event.currentTarget.value)}
          on:keydown={(event) => { if (event.key === "Enter") void goOnline(); }}
        />
        <button class="transfer-eye" type="button" title={codeVisible ? "隐藏" : "显示"} on:click={() => (codeVisible = !codeVisible)}>
          {codeVisible ? "🙈" : "👁"}
        </button>
      </div>
    </label>
    <div class="transfer-status-line">
      <span class="transfer-dot" class:on={state.online}></span>
      <span>{state.online ? `房间在线 · ${state.devices.length} 台设备` : codeOk ? "未上线" : `口令至少 ${CODE_MIN} 位`}</span>
      {#if state.online}
        <button class="menu-action-button" type="button" on:click={() => void goOffline()}>离线</button>
      {:else}
        <button class="menu-action-button primary" type="button" disabled={!codeOk || state.busy} on:click={() => void goOnline()}>上线</button>
      {/if}
    </div>
  </section>

  <!-- 第二块：发送 / 接收（滑块与其下所有内容合成一块） -->
  <section class="transfer-card transfer-work">
    <div class="transfer-tabs" role="tablist">
      <button type="button" role="tab" class:active={state.tab === "send"} on:click={() => setTab("send")}>发送</button>
      <button type="button" role="tab" class:active={state.tab === "receive"} on:click={() => setTab("receive")}>接收</button>
    </div>

    {#if state.tab === "send"}
      <div class="transfer-actions">
        <button class="transfer-action-button" type="button" on:click={() => void pickFiles()}>
          <Upload size={26} /><span>文件</span>
        </button>
        <button class="transfer-action-button" type="button" on:click={() => void pickFolder()}>
          <FolderUp size={26} /><span>文件夹</span>
        </button>
        <button class="transfer-action-button" type="button" on:click={openTextPop}>
          <PencilLine size={26} /><span>文本</span>
        </button>
        <button class="transfer-action-button" type="button" on:click={() => void pickClipboard()}>
          <Clipboard size={26} /><span>剪贴板</span>
        </button>
      </div>

      {#if textPopOpen}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="transfer-text-pop" on:click|stopPropagation>
          <textarea bind:value={textDraft} placeholder="要发送的文本…" rows="4"></textarea>
          <div class="transfer-text-actions">
            <button class="menu-action-button" type="button" on:click={() => (textPopOpen = false)}>取消</button>
            <button class="menu-action-button primary" type="button" on:click={commitText}>加入清单</button>
          </div>
        </div>
      {/if}

      {#if state.picked.length > 0}
        <div class="transfer-picked-head">
          <span>已选 {state.picked.length} 项 · {sizeText(sendBytes)}</span>
          <button class="menu-action-button" type="button" on:click={clearPicked}>清空</button>
        </div>
        <div class="transfer-picked">
          {#each state.picked as item (item.key)}
            <span class="transfer-chip">
              {#if item.kind === "folder"}<FolderUp size={13} />{:else if item.kind === "text"}<FileText size={13} />{:else if item.kind === "clipboard"}<Clipboard size={13} />{:else}<Upload size={13} />{/if}
              <em>{item.preview || item.rel}</em>
              <button type="button" aria-label="移除" on:click={() => removePicked(item.key)}><X size={11} strokeWidth={3} /></button>
            </span>
          {/each}
        </div>
      {/if}

      <div class="transfer-devices-head">
        <span>匹配到的设备</span>
        {#if state.devices.length > 0}<em>在线 {state.devices.length}</em>{/if}
      </div>
      {#if state.devices.length === 0}
        <div class="transfer-empty">
          <ShieldCheck size={18} />
          <span>对方输完同一句口令，就会出现在这里</span>
          <em>端到端加密</em>
        </div>
      {:else}
        <div class="transfer-devices">
          {#each state.devices as device (device.id)}
            <button
              type="button"
              class="transfer-device"
              class:selected={state.selectedDevice === device.id}
              on:click={() => selectDevice(device.id)}
            >
              <span class="transfer-device-icon"><ArrowLeftRight size={16} /></span>
              <span>{device.name || "未命名设备"}</span>
              {#if state.selectedDevice === device.id}<em>✓</em>{/if}
            </button>
          {/each}
        </div>
      {/if}

      <div class="transfer-send-row">
        <button
          class="transfer-send-button"
          type="button"
          disabled={!state.online || !state.selectedDevice || state.picked.length === 0 || state.busy || spooling}
          on:click={() => void startSend()}
        >
          <ArrowLeftRight size={17} /> {spooling ? "正在准备…" : "发送"}
        </button>
      </div>
    {:else}
      <div class="transfer-receive-head">
        <div class="transfer-status-line">
          <span class="transfer-dot" class:on={state.online}></span>
          <span>{state.online ? "待命接收中" : "还没上线"}</span>
        </div>
        <div class="transfer-save-row">
          <span class="transfer-save-dir" title={state.saveDir}>{state.saveDir || "（默认保存位置）"}</span>
          {#if caps.nativeFileDialogs}
            <button class="menu-action-button" type="button" on:click={() => void chooseSaveDir()}>更改</button>
          {/if}
          <button class="menu-action-button" type="button" on:click={() => void openSaveDir()}>打开文件夹</button>
        </div>
        <label class="transfer-auto">
          <input type="checkbox" checked={autoAccept} on:change={() => void applyAutoAccept(!autoAccept)} />
          <span>自动接收（不再逐次确认）</span>
        </label>
      </div>

      {#if state.requests.length > 0}
        <div class="transfer-request-list">
          {#each state.requests as request (request.sessionId)}
            <section class="transfer-request">
              <strong>{request.peerName} 想发送 {request.files.length} 个文件 · {sizeText(request.totalBytes)}</strong>
              <div class="transfer-request-files">
                {#each request.files.slice(0, 6) as file (file.rel)}
                  <span>{file.rel.split(/[\\/]/).pop()}</span>
                {/each}
                {#if request.files.length > 6}<span>…等 {request.files.length} 个</span>{/if}
              </div>
              <div class="transfer-request-actions">
                <button class="menu-action-button" type="button" on:click={() => void decideRequest(request.sessionId, false)}>拒绝</button>
                <button class="menu-action-button primary" type="button" on:click={() => void decideRequest(request.sessionId, true)}>接收</button>
              </div>
            </section>
          {/each}
        </div>
      {/if}

      {#if state.texts.length > 0}
        <div class="transfer-text-cards">
          {#each state.texts as card (card.id)}
            <section class="transfer-text-card">
              <header>
                <strong>{card.mine ? "已发送" : card.peer}</strong>
                <em>{card.at}</em>
              </header>
              <p>{card.text}</p>
              <div class="transfer-text-actions">
                <button class="menu-action-button" type="button" on:click={() => void copyText(card.text)}>复制</button>
                <button class="menu-action-button" type="button" on:click={() => void saveTextAsFile(card.text)}>保存为 .txt</button>
              </div>
            </section>
          {/each}
        </div>
      {/if}
    {/if}
  </section>

  <!-- 第三块：传输中的会话 -->
  {#if activeSessions.length > 0}
    <section class="transfer-card transfer-sessions">
      <div class="transfer-card-title">传输中（{activeSessions.length}）</div>
      {#each activeSessions as session (session.id)}
        <article class="transfer-session">
          <header>
            <span class="transfer-session-dir">{session.direction === "send" ? "↑" : "↓"}</span>
            <strong>{currentFile(session) || (session.direction === "send" ? "等待对方接收…" : "等待对方发送…")}</strong>
            <em>
              {#if session.fileCount > 1}
                第 {Math.min(session.files.filter((item) => item.done).length + 1, session.fileCount)}/{session.fileCount} 个 ·
              {/if}
              {percentOf(session)}%
            </em>
            <button class="transfer-session-cancel" type="button" title="取消" on:click={() => void cancelSession(session.id)}><X size={13} /></button>
          </header>
          <div class="transfer-bar"><span style={`width: ${percentOf(session)}%`}></span></div>
          <footer>
            {#if session.speed > 0}<span>{sizeText(session.speed)}/s</span>{/if}
            {#if etaText(session)}<span>{etaText(session)}</span>{/if}
            {#if session.state === "waiting"}<span>正在连接…</span>{/if}
          </footer>
        </article>
      {/each}
    </section>
  {/if}

  <!-- 第四块：刚完成 / 失败 / 取消的（5 秒后自动收起） -->
  {#if finishedSessions.length > 0}
    <section class="transfer-card transfer-sessions">
      {#each finishedSessions as session (session.id)}
        <article class="transfer-session done" class:failed={session.state !== "done"}>
          <header>
            <span class="transfer-session-dir">{session.state === "done" ? "✓" : session.state === "cancelled" ? "✕" : "!"}</span>
            <strong>
              {session.direction === "send" ? "发送" : "接收"}{session.state === "done" ? "完成" : session.state === "cancelled" ? "已取消" : "失败"}
            </strong>
            <em>{session.peerName || ""}{session.error ? ` · ${session.error}` : ""}</em>
            {#if session.direction === "send" && session.retryable && session.state === "error"}
              <button class="menu-action-button" type="button" on:click={() => dismissSession(session.id)}>知道了</button>
            {/if}
            {#if session.state === "done" && session.direction === "receive"}
              <button class="menu-action-button" type="button" on:click={() => void openSaveDir()}>打开文件夹</button>
            {/if}
          </header>
        </article>
      {/each}
    </section>
  {/if}

  <!-- 第五块：传输历史 -->
  <section class="transfer-card transfer-history">
    <button class="transfer-history-head" type="button" on:click={() => (historyOpen = !historyOpen)}>
      <span>传输历史</span>
      <em>共 {state.historyEntries.length} 条</em>
      <span class="transfer-history-caret" class:open={historyOpen}>▾</span>
    </button>
    {#if historyOpen}
      {#if state.historyEntries.length === 0}
        <div class="transfer-empty"><span>还没有传输记录</span></div>
      {:else}
        <ul class="transfer-history-list">
          {#each state.historyEntries as entry (entry.id)}
            <li>
              <span class={entry.status === "done" ? "ok" : entry.status === "text" ? "text" : "bad"}>
                {entry.status === "done" ? "✓" : entry.status === "text" ? "✉" : "✕"}
              </span>
              <strong>{entry.names?.[0] ?? (entry.status === "text" ? "文本消息" : "传输")}</strong>
              <em>{sizeText(entry.bytes)}</em>
              <em>{entry.direction === "send" ? "发给" : "来自"}{entry.peerName || "对方"}</em>
              <em>{entry.at.slice(11, 16)}</em>
            </li>
          {/each}
        </ul>
        <div class="transfer-history-actions">
          {#if state.deviceHistory.length > 0}
            <span>常连设备 {state.deviceHistory.length} 台</span>
          {/if}
          <button class="menu-action-button" type="button" on:click={() => void clearHistory()}>清空历史</button>
        </div>
      {/if}
    {/if}
  </section>

  <input id="transfer-file-input" class="hidden-file" type="file" multiple on:change={spoolFromInput} />
</div>

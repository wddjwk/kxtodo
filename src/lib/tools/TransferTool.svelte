<script lang="ts">
  /**
   * 文件传输助手（v0.8.3）：LocalSend 式的发送 / 接收两栏 + croc 式的口令配对。
   *
   * 配对与传输全在 core（`crates/core/src/transfer.rs`，有离线端到端集成测试）：
   * 两台设备输同一句口令（≥8 位），接收方把自己发布到「口令派生的 pkarr zone」，
   * 发送方轮询到就拨号打洞，逐文件推字节、逐文件落盘确认。这里只负责交互：
   * 选文件 / 选保存位置、画每个文件的进度条、把 `kxtodo://transfer` 事件翻译成界面状态。
   *
   * 与同步账户、系统账号完全无关——口令就是全部凭据。
   * 移动端没有原生文件对话框：发送走 webview 的文件输入 + 分块暂存（spool），
   * 文件夹发送与自选保存位置是桌面能力（移动端收进应用外部文件目录的 Transfer）。
   */
  import { onDestroy, onMount } from "svelte";
  import { ArrowLeftRight, Download, FolderOpen, FolderPlus, Plus, Upload, X } from "@lucide/svelte";
  import { appSettings, showToast } from "../stores";
  import { setConfig } from "../actions";
  import { caps } from "../capabilities";
  import {
    isTauriRuntime, listenTransfer, pickAnyFiles, pickDirectory, transferCancel, transferDefaultSaveDir,
    transferListFolder, transferOutboxPath, transferReceive, transferSend, transferSpoolClear,
    transferSpoolWrite, transferStatFiles,
    type TransferEvent, type TransferItemDto
  } from "../backend";

  const CODE_MIN = 8;

  let code = "";
  let relayMode: "follow" | "disabled" | "custom" = "follow";
  let relayCustom = "";

  type RoleState = "idle" | "waiting" | "connected" | "done" | "error" | "cancelled";
  type Bar = { index: number; file: string; sent: number; total: number; done: boolean };

  let sendBatch: { root: string | null; items: TransferItemDto[] } = { root: null, items: [] };
  let saveDir = "";
  let sendSession: string | null = null;
  let recvSession: string | null = null;
  let sendState: RoleState = "idle";
  let recvState: RoleState = "idle";
  let sendNote = "";
  let recvNote = "";
  let sendBars: Bar[] = [];
  let recvBars: Bar[] = [];
  let spooling = false;
  let stopListen: (() => void) | null = null;

  $: codeOk = code.trim().length >= CODE_MIN;
  $: sendBusy = sendState === "waiting" || sendState === "connected";
  $: recvBusy = recvState === "waiting" || recvState === "connected";
  $: sendBytes = sendBatch.items.reduce((sum, item) => sum + item.size, 0);

  onMount(() => {
    const relay = $appSettings.transfer?.relay ?? "";
    relayMode = relay === "" ? "follow" : relay === "disabled" ? "disabled" : "custom";
    relayCustom = relayMode === "custom" ? relay : "";
    // 浏览器预览没有壳：invoke 会直接抛，别把未处理的 rejection 留给页面
    if (!isTauriRuntime) return;
    void listenTransfer(handleEvent)
      .then((unlisten) => (stopListen = unlisten))
      .catch(() => undefined);
    void transferDefaultSaveDir()
      .then((dir) => (saveDir = dir))
      .catch(() => (saveDir = ""));
  });

  onDestroy(() => {
    stopListen?.();
    // 离开工具时把没发完的会话收掉：进度事件不该继续推给一个不存在的界面
    if (sendSession) void transferCancel(sendSession).catch(() => undefined);
    if (recvSession) void transferCancel(recvSession).catch(() => undefined);
    void transferSpoolClear().catch(() => undefined);
  });

  // ---- 事件 → 界面状态 ----

  function handleEvent(event: TransferEvent): void {
    if (event.sessionId === sendSession) applyEvent("send", event);
    else if (event.sessionId === recvSession) applyEvent("receive", event);
  }

  function setState(role: "send" | "receive", state: RoleState, note: string): void {
    if (role === "send") {
      sendState = state;
      sendNote = note;
      if (state !== "waiting" && state !== "connected") sendSession = null;
    } else {
      recvState = state;
      recvNote = note;
      if (state !== "waiting" && state !== "connected") recvSession = null;
    }
  }

  function upsertBar(role: "send" | "receive", event: TransferEvent): void {
    const index = event.index ?? 0;
    const bar: Bar = {
      index,
      file: event.file ?? "",
      sent: event.sent ?? 0,
      total: event.total ?? 0,
      done: event.kind === "fileDone"
    };
    if (role === "send") {
      sendBars = sendBars.map((old) => (old.index === index ? bar : old)).concat(sendBars.some((old) => old.index === index) ? [] : [bar]);
    } else {
      recvBars = recvBars.map((old) => (old.index === index ? bar : old)).concat(recvBars.some((old) => old.index === index) ? [] : [bar]);
    }
  }

  function applyEvent(role: "send" | "receive", event: TransferEvent): void {
    switch (event.kind) {
      case "waiting":
        setState(role, "waiting", role === "send" ? "等待对方上线…" : "已上线，等待对方拨入…");
        break;
      case "connected":
        setState(role, "connected", `已连上对方 · ${event.files ?? 0} 个文件`);
        break;
      case "progress":
      case "fileDone":
        upsertBar(role, event);
        break;
      case "done":
        setState(role, "done", `完成 · ${event.files ?? 0} 个文件`);
        if (role === "send") void transferSpoolClear().catch(() => undefined);
        break;
      case "cancelled":
        setState(role, "cancelled", "已取消");
        break;
      case "error":
        setState(role, "error", event.message ?? "传输失败");
        break;
    }
  }

  // ---- 发送侧 ----

  async function chooseFiles(): Promise<void> {
    if (caps.nativeFileDialogs) {
      const paths = await pickAnyFiles();
      if (paths.length === 0) return;
      const items = await transferStatFiles(paths).catch(() => []);
      sendBatch = sendBatch.root === null
        ? { root: null, items: [...sendBatch.items, ...items] }
        : { root: null, items };
      return;
    }
    // 移动端：webview 文件输入 → 分块暂存到 outbox，再走同一条发送引擎
    document.getElementById("transfer-file-input")?.click();
  }

  async function spoolFromInput(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement | null;
    const files = [...(input?.files ?? [])];
    input?.value && (input.value = "");
    if (files.length === 0) return;
    spooling = true;
    try {
      const root = await transferOutboxPath();
      const items: TransferItemDto[] = [];
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
          const buffer = await blob.arrayBuffer();
          const bytes = new Uint8Array(buffer);
          let binary = "";
          for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
          await transferSpoolWrite(rel, btoa(binary), offset > 0);
          offset += bytes.length;
        }
        if (file.size === 0) await transferSpoolWrite(rel, "", false);
        items.push({ rel, size: file.size });
      }
      sendBatch = { root, items };
    } catch (error) {
      showToast(`暂存文件失败：${String(error)}`);
    } finally {
      spooling = false;
    }
  }

  async function chooseFolder(): Promise<void> {
    const dir = await pickDirectory();
    if (!dir) return;
    try {
      const items = await transferListFolder(dir);
      if (items.length === 0) {
        showToast("这个文件夹里没有文件");
        return;
      }
      sendBatch = { root: dir, items };
    } catch (error) {
      showToast(`读取文件夹失败：${String(error)}`);
    }
  }

  function clearBatch(): void {
    sendBatch = { root: null, items: [] };
    void transferSpoolClear().catch(() => undefined);
  }

  async function startSend(): Promise<void> {
    if (!codeOk || sendBatch.items.length === 0 || sendBusy) return;
    sendBars = [];
    setState("send", "waiting", "正在起端点…");
    try {
      sendSession = await transferSend(code, sendBatch.root, sendBatch.items);
    } catch (error) {
      setState("send", "error", String(error));
    }
  }

  // ---- 接收侧 ----

  async function chooseSaveDir(): Promise<void> {
    const dir = await pickDirectory();
    if (dir) saveDir = dir;
  }

  async function startReceive(): Promise<void> {
    if (!codeOk || !saveDir || recvBusy) return;
    recvBars = [];
    setState("receive", "waiting", "正在起端点…");
    try {
      recvSession = await transferReceive(code, saveDir);
    } catch (error) {
      setState("receive", "error", String(error));
    }
  }

  // ---- relay 设置（右上角） ----

  function commitRelay(): void {
    const value = relayMode === "follow" ? "" : relayMode === "disabled" ? "disabled" : relayCustom.trim();
    if (relayMode === "custom" && !value) return;
    void setConfig("transfer.relay", value);
  }

  function humanBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }
</script>

<div class="toolbox-sub transfer-tool">
  <div class="transfer-head">
    <div class="toolbox-sub-title">
      <ArrowLeftRight size={18} /> 文件传输助手
    </div>
    <label class="transfer-relay" title="自选 iroh relay；默认跟 p2p 同步用同一个">
      <span>relay</span>
      <select class="toolbox-select" bind:value={relayMode} on:change={commitRelay}>
        <option value="follow">默认（跟随同步）</option>
        <option value="disabled">不用 relay</option>
        <option value="custom">自定义…</option>
      </select>
      {#if relayMode === "custom"}
        <input
          class="toolbox-text-input"
          type="text"
          placeholder="https://relay.example.com"
          bind:value={relayCustom}
          on:change={commitRelay}
        />
      {/if}
    </label>
  </div>

  <div class="toolbox-field-row">
    <span>配对口令</span>
    <input
      class="toolbox-text-input toolbox-text-input-wide"
      type="text"
      placeholder="两台设备输同一句（至少 {CODE_MIN} 位）"
      bind:value={code}
    />
  </div>
  {#if code.trim() && !codeOk}
    <p class="toolbox-empty">口令至少 {CODE_MIN} 位：它是两台设备之间的唯一暗号。</p>
  {/if}

  <div class="transfer-grid">
    <!-- 发送 -->
    <section class="transfer-pane">
      <header><Upload size={15} /> 发送</header>
      <div class="transfer-pane-actions">
        <button class="settings-button" type="button" disabled={sendBusy || spooling} on:click={() => void chooseFiles()}>
          <Plus size={14} /> 选择文件
        </button>
        {#if caps.nativeFileDialogs}
          <button class="settings-button" type="button" disabled={sendBusy || spooling} on:click={() => void chooseFolder()}>
            <FolderPlus size={14} /> 选择文件夹
          </button>
        {/if}
        {#if sendBatch.items.length > 0 && !sendBusy}
          <button class="settings-button" type="button" on:click={clearBatch}>
            <X size={14} /> 清空
          </button>
        {/if}
      </div>
      <input
        id="transfer-file-input"
        type="file"
        multiple
        hidden
        on:change={(event) => void spoolFromInput(event)}
      />
      {#if sendBatch.items.length > 0}
        <p class="transfer-batch-note">
          {sendBatch.items.length} 个文件 · {humanBytes(sendBytes)}
          {#if sendBatch.root}（文件夹，按原目录结构发送）{/if}
        </p>
      {:else}
        <p class="transfer-batch-note">还没选文件{spooling ? "（正在暂存…）" : ""}。</p>
      {/if}
      <button
        class="settings-button primary"
        type="button"
        disabled={!codeOk || sendBatch.items.length === 0 || sendBusy}
        on:click={() => void startSend()}
      >
        <Upload size={14} /> 开始发送
      </button>
      {#if sendBusy && sendSession}
        <button class="settings-button" type="button" on:click={() => void transferCancel(sendSession ?? "")}>取消</button>
      {/if}
      <p class="transfer-state transfer-state-{sendState}">{sendNote}</p>
      <div class="transfer-bars">
        {#each sendBars as bar (bar.index)}
          <div class="transfer-bar">
            <span class="transfer-bar-name" title={bar.file}>{bar.file}</span>
            <span class="transfer-bar-num">{humanBytes(bar.sent)} / {humanBytes(bar.total)}</span>
            <span class="transfer-bar-track"><span class:transfer-bar-done={bar.done} style="width: {bar.total ? Math.min(100, (bar.sent / bar.total) * 100) : 0}%"></span></span>
          </div>
        {/each}
      </div>
    </section>

    <!-- 接收 -->
    <section class="transfer-pane">
      <header><Download size={15} /> 接收</header>
      <div class="transfer-pane-actions">
        {#if caps.nativeFileDialogs}
          <button class="settings-button" type="button" disabled={recvBusy} on:click={() => void chooseSaveDir()}>
            <FolderOpen size={14} /> 选择保存位置
          </button>
        {/if}
      </div>
      <p class="transfer-batch-note" title={saveDir}>
        保存到：{saveDir || "（正在读取默认位置…）"}
      </p>
      <button
        class="settings-button primary"
        type="button"
        disabled={!codeOk || !saveDir || recvBusy}
        on:click={() => void startReceive()}
      >
        <Download size={14} /> 开始接收
      </button>
      {#if recvBusy && recvSession}
        <button class="settings-button" type="button" on:click={() => void transferCancel(recvSession ?? "")}>取消</button>
      {/if}
      <p class="transfer-state transfer-state-{recvState}">{recvNote}</p>
      <div class="transfer-bars">
        {#each recvBars as bar (bar.index)}
          <div class="transfer-bar">
            <span class="transfer-bar-name" title={bar.file}>{bar.file}</span>
            <span class="transfer-bar-num">{humanBytes(bar.sent)} / {humanBytes(bar.total)}</span>
            <span class="transfer-bar-track"><span class:transfer-bar-done={bar.done} style="width: {bar.total ? Math.min(100, (bar.sent / bar.total) * 100) : 0}%"></span></span>
          </div>
        {/each}
      </div>
    </section>
  </div>
</div>

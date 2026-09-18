<script lang="ts">
  /**
   * 文件传输助手（v0.8.4 重做交互；配对与传输仍在 core 的 `transfer.rs`）。
   *
   * 形态照 LocalSend：顶部身份区（设备名 + 配对口令）→ 发送/接收分段滑块 →
   * 发送侧四个大图标（文件/文件夹/文本/剪贴板）+ 已选清单 + 单选的设备卡片 +
   * 右下角胶囊发送按钮；接收侧待命 + 保存位置 + 接收确认卡 + 收到文本卡片 + 历史。
   *
   * 与 v0.8.3 的结构差异：**在线是一个常驻会话**（`transfer_online`）——输完口令就
   * 发布自己进房间、每 2 秒刷新匹配到的设备、收到拨入就地处理；发送是「挑一台设备发」。
   * 进度/速度/剩余时间/历史都在前端聚合（core 只报每个文件已发字节）。
   */
  import { onDestroy, onMount } from "svelte";
  import { ArrowLeftRight, Clipboard, FileText, FolderOpen, FolderUp, PencilLine, ShieldCheck, Upload, X } from "@lucide/svelte";
  import { appSettings, showNotification, showToast } from "../stores";
  import { setConfig } from "../actions";
  import { caps } from "../capabilities";
  import {
    isTauriRuntime, listenTransfer, pickAnyFiles, pickDirectory, transferCancel, transferClearHistory,
    transferDefaultSaveDir, transferHistory, transferListFolder, transferLoadCode, transferOffline,
    transferOnline, transferOpenPath, transferOutboxPath, transferSaveCode, transferSaveText, transferSend,
    transferSetAutoAccept, transferSetName, transferSpoolClear, transferSpoolWrite, transferStatFiles,
    type TransferDeviceDto, type TransferEvent
  } from "../backend";

  const CODE_MIN = 8;
  /** 完成后的会话卡自动收起（LocalSend 同款） */
  const AUTO_COLLAPSE_MS = 5000;
  /** 历史的折叠区默认收起 */

  type SendKind = "file" | "folder" | "text" | "clipboard";
  type PickedItem = { key: string; rel: string; size: number; kind: SendKind; preview: string };

  type FileBar = { index: number; rel: string; sent: number; total: number; done: boolean };
  type Session = {
    id: string;
    direction: "send" | "receive";
    peerName: string;
    files: FileBar[];
    totalBytes: number;
    /** 期望的总文件数（connected 事件带的） */
    fileCount: number;
    startedAt: number;
    speed: number;
    state: "waiting" | "connected" | "done" | "error" | "cancelled";
    error: string;
    /** 完成/失败后的自动收起定时器 */
    collapsed: boolean;
    /** 出错时保留重试所需的信息 */
    retryable: boolean;
  };

  type ReceivedText = { id: string; peer: string; text: string; at: string; mine: boolean };

  let tab: "send" | "receive" = "send";
  let code = "";
  let codeVisible = false;
  let deviceName = "";
  let deviceNameDraft = "";
  let deviceNameFocused = false;
  let online = false;
  let selfId = "";
  let devices: TransferDeviceDto[] = [];
  let selectedDevice = "";
  let saveDir = "";
  let autoAccept = false;
  let busy = false;

  let picked: PickedItem[] = [];
  let textDraft = "";
  let textPopOpen = false;

  let sessions: Session[] = [];
  let request: { sessionId: string; peerId: string; peerName: string; files: Array<{ rel: string; size: number }>; totalBytes: number } | null = null;
  let receivedTexts: ReceivedText[] = [];
  let historyOpen = false;
  let historyEntries: Array<{ id: string; at: string; direction: string; peerName: string; status: string; files: number; bytes: number; names?: string[] }> = [];
  let deviceHistory: Array<{ id: string; name: string; lastAt: string; count: number }> = [];

  let stopListen: (() => void) | null = null;
  let unsubscribeDrop: (() => void) | null = null;
  /** 传输中保持屏幕常亮（移动端锁屏会断连；WebView2/Android WebView 都支持 Web 标准 API） */
  let wakeLock: { release?: () => Promise<void> } | null = null;
  let autoCollapse: Record<string, number> = {};

  $: codeOk = code.trim().length >= CODE_MIN;
  $: activeSessions = sessions.filter((item) => item.state === "waiting" || item.state === "connected");
  $: finishedSessions = sessions.filter((item) => item.state === "done" || item.state === "error" || item.state === "cancelled");
  $: sendBytes = picked.reduce((sum, item) => sum + item.size, 0);
  $: shownName = deviceName.trim() || "我的设备";

  onMount(() => {
    // 桌面：拖文件 / 文件夹进发送区直接进清单（需求 1.7）
    if (isTauriRuntime && caps.desktop) {
      void (async () => {
        try {
          const { getCurrentWebview } = await import("@tauri-apps/api/webview");
          unsubscribeDrop = await getCurrentWebview().onDragDropEvent((event) => {
            if (event.payload.type !== "drop") return;
            const paths = event.payload.paths ?? [];
            if (paths.length === 0) return;
            tab = "send";
            void addDroppedPaths(paths);
          });
        } catch {
          // 不支持拖放的环境（浏览器预览）静默跳过
        }
      })();
    }
    void transferDefaultSaveDir()
      .then((dir) => (saveDir = dir))
      .catch(() => (saveDir = ""));
    autoAccept = Boolean($appSettings.transfer?.autoAccept);
    deviceName = $appSettings.transfer?.deviceName ?? "";
    deviceNameDraft = deviceName;
    void refreshHistory();
    if (!isTauriRuntime) return;
    void listenTransfer(handleEvent)
      .then((unlisten) => (stopListen = unlisten))
      .catch(() => undefined);
    // 口令记住：下次启动自动恢复在线（需求 1.2）
    void transferLoadCode()
      .then((stored) => {
        const remembered = stored.code?.trim() ?? "";
        if (remembered.length >= CODE_MIN) {
          code = remembered;
          void goOnline();
        }
      })
      .catch(() => undefined);
  });

  onDestroy(() => {
    stopListen?.();
    unsubscribeDrop?.();
    for (const timer of Object.values(autoCollapse)) window.clearTimeout(timer);
    void releaseWakeLock();
    // 离开工具时下线：房间里的条目撤掉，别让对方一直看到一个不会应答的设备
    void transferOffline().catch(() => undefined);
    void transferSpoolClear().catch(() => undefined);
  });

  // ---- 在线与设备 ----

  async function goOnline(): Promise<void> {
    if (!codeOk || busy || !isTauriRuntime) return;
    busy = true;
    try {
      const result = await transferOnline(code.trim(), saveDir, deviceName.trim(), autoAccept);
      online = true;
      selfId = result.deviceId;
      await transferSaveCode(code.trim());
    } catch (error) {
      online = false;
      showToast(String(error));
    } finally {
      busy = false;
    }
  }

  async function goOffline(): Promise<void> {
    try {
      await transferOffline();
    } catch {
      // 离线失败无实质后果
    }
    online = false;
    devices = [];
    request = null;
  }

  async function commitDeviceName(): Promise<void> {
    const value = deviceNameDraft.trim();
    deviceNameFocused = false;
    if (value === deviceName) return;
    deviceName = value;
    await setConfig("transfer.deviceName", value);
    if (online) await transferSetName(value).catch(() => undefined);
  }

  async function toggleAutoAccept(): Promise<void> {
    autoAccept = !autoAccept;
    await setConfig("transfer.autoAccept", autoAccept);
    if (online) await transferSetAutoAccept(autoAccept).catch(() => undefined);
  }

  // ---- 事件 → 界面状态 ----

  function sessionOf(id: string, direction: "send" | "receive"): Session {
    const found = sessions.find((item) => item.id === id);
    if (found) return found;
    const created: Session = {
      id,
      direction,
      peerName: "",
      files: [],
      totalBytes: 0,
      fileCount: 0,
      startedAt: Date.now(),
      speed: 0,
      state: "waiting",
      error: "",
      collapsed: false,
      retryable: false
    };
    sessions = [...sessions, created];
    return created;
  }

  function updateSession(id: string, direction: "send" | "receive", patch: Partial<Session>): void {
    sessionOf(id, direction);
    sessions = sessions.map((item) => (item.id === id ? { ...item, ...patch } : item));
  }

  function upsertBar(id: string, direction: "send" | "receive", event: TransferEvent): void {
    const session = sessionOf(id, direction);
    const index = event.index ?? 0;
    const bar: FileBar = {
      index,
      rel: event.file ?? "",
      sent: event.sent ?? 0,
      total: event.total ?? 0,
      done: event.kind === "fileDone"
    };
    const files = session.files.some((item) => item.index === index)
      ? session.files.map((item) => (item.index === index ? bar : item))
      : [...session.files, bar];
    updateSession(id, direction, { files, state: "connected" });
    updateSpeed(id, direction);
  }

  /** 平均速度用 EMA 平滑（前几秒会跳）；剩余时间 = 剩余字节 ÷ EMA。 */
  function updateSpeed(id: string, direction: "send" | "receive"): void {
    const session = sessions.find((item) => item.id === id);
    if (!session) return;
    const sent = session.files.reduce((sum, item) => sum + item.sent, 0);
    const elapsed = (Date.now() - session.startedAt) / 1000;
    if (elapsed <= 0.1) return;
    const instant = sent / elapsed;
    const next = session.speed === 0 ? instant : session.speed * 0.72 + instant * 0.28;
    if (Math.abs(next - session.speed) < 1) return;
    updateSession(id, direction, { speed: next });
  }

  function handleEvent(event: TransferEvent): void {
    switch (event.kind) {
      case "online":
        online = true;
        selfId = event.deviceId ?? selfId;
        void refreshHistory();
        return;
      case "offline":
        online = false;
        devices = [];
        return;
      case "devices":
        devices = (event.devices ?? []).filter((item) => item.id !== selfId);
        if (!devices.some((item) => item.id === selectedDevice)) {
          selectedDevice = devices.length === 1 ? devices[0].id : "";
        }
        return;
      case "request":
        request = {
          sessionId: event.sessionId,
          peerId: typeof event.peer === "object" ? event.peer.id : String(event.peer ?? ""),
          peerName: typeof event.peer === "object" ? event.peer.name || "对方设备" : "对方设备",
          files: (event.files as unknown as Array<{ rel: string; size: number }>) ?? [],
          totalBytes: event.totalBytes ?? 0
        };
        tab = "receive";
        return;
      case "text": {
        const peer = typeof event.peer === "object" ? event.peer.name || "对方设备" : "对方设备";
        const card: ReceivedText = {
          id: event.sessionId,
          peer,
          text: event.text ?? "",
          at: new Date().toLocaleTimeString().slice(0, 5),
          mine: event.received === false
        };
        receivedTexts = [card, ...receivedTexts.filter((item) => item.id !== card.id)].slice(0, 30);
        tab = "receive";
        void refreshHistory();
        return;
      }
      case "waiting":
        updateSession(event.sessionId, event.role === "send" ? "send" : "receive", { state: "waiting" });
        return;
      case "connected": {
        const peerName = event.peerName ?? "";
        const session = sessionOf(event.sessionId, event.role === "send" ? "send" : "receive");
        updateSession(event.sessionId, session.direction, {
          state: "connected",
          startedAt: Date.now(),
          peerName: peerName || session.peerName,
          fileCount: event.files ?? session.fileCount,
          totalBytes: event.totalBytes ?? session.totalBytes
        });
        void acquireWakeLock();
        return;
      }
      case "progress":
      case "fileDone":
        upsertBar(event.sessionId, event.role === "send" ? "send" : "receive", event);
        return;
      case "done": {
        const session = sessionOf(event.sessionId, event.role === "send" ? "send" : "receive");
        updateSession(event.sessionId, session.direction, {
          state: "done",
          totalBytes: event.totalBytes ?? session.totalBytes
        });
        scheduleCollapse(event.sessionId);
        void refreshHistory();
        void releaseWakeLockIfIdle();
        const dir = event.dir ?? saveDir;
        if (session.direction === "send") {
          void showNotification(`已发送给 ${session.peerName || "对方"}`, { title: "传输完成", tone: "success" });
        } else {
          void showNotification(`已保存到 ${dir}`, { title: "接收完成", tone: "success" });
        }
        showToast(session.direction === "send" ? "发送完成" : "接收完成 · 已保存到保存位置");
        return;
      }
      case "cancelled": {
        const session = sessionOf(event.sessionId, event.role === "send" ? "send" : "receive");
        updateSession(event.sessionId, session.direction, { state: "cancelled" });
        scheduleCollapse(event.sessionId);
        void refreshHistory();
        void releaseWakeLockIfIdle();
        return;
      }
      case "error": {
        if (event.role === "online") {
          showToast(event.message ?? "传输连接断了");
          online = false;
          return;
        }
        const session = sessionOf(event.sessionId, event.role === "send" ? "send" : "receive");
        const finished = session.state === "connected";
        updateSession(event.sessionId, session.direction, {
          state: "error",
          error: finished ? "对方离线了" : event.message ?? "传输失败",
          retryable: session.direction === "send"
        });
        scheduleCollapse(event.sessionId);
        void refreshHistory();
        void releaseWakeLockIfIdle();
        return;
      }
    }
  }

  function scheduleCollapse(id: string): void {
    window.clearTimeout(autoCollapse[id]);
    autoCollapse[id] = window.setTimeout(() => {
      sessions = sessions.map((item) => (item.id === id ? { ...item, collapsed: true } : item));
    }, AUTO_COLLAPSE_MS);
  }

  // ---- 保持常亮 ----

  async function acquireWakeLock(): Promise<void> {
    if (wakeLock) return;
    const api = (navigator as unknown as { wakeLock?: { request: (type: string) => Promise<{ release?: () => Promise<void> }> } }).wakeLock;
    if (!api) return;
    try {
      wakeLock = await api.request("screen");
    } catch {
      wakeLock = null;
    }
  }

  async function releaseWakeLock(): Promise<void> {
    const lock = wakeLock;
    wakeLock = null;
    try {
      await lock?.release?.();
    } catch {
      // 已经释放过就算了
    }
  }

  async function releaseWakeLockIfIdle(): Promise<void> {
    if (sessions.some((item) => item.state === "waiting" || item.state === "connected")) return;
    await releaseWakeLock();
  }

  // ---- 发送：选东西 ----

  function addItems(items: PickedItem[]): void {
    const keys = new Set(picked.map((item) => item.key));
    picked = [...picked, ...items.filter((item) => !keys.has(item.key))];
    tab = "send";
  }

  async function pickFiles(): Promise<void> {
    if (caps.nativeFileDialogs) {
      const paths = await pickAnyFiles();
      if (paths.length === 0) return;
      const stats = await transferStatFiles(paths).catch(() => []);
      addItems(
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
      addItems([
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
    addItems([{ key: `t:${Date.now()}`, rel: text, size: text.length, kind: "text", preview: text.slice(0, 40) }]);
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
        addItems([{ key: `c:${rel}`, rel: `${root}\\${rel}`, size: bytes.length, kind: "clipboard", preview: rel }]);
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
      addItems([{ key: `c:${Date.now()}`, rel: text, size: text.length, kind: "clipboard", preview: text.slice(0, 40) }]);
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
            preview: path.split(/[\/]/).filter(Boolean).pop() ?? path
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
          preview: path.split(/[\/]/).pop() ?? path
        });
      }
    }
    if (next.length === 0) {
      showToast("拖进来的内容为空");
      return;
    }
    addItems(next);
  }

  function removePicked(key: string): void {
    picked = picked.filter((item) => item.key !== key);
  }

  // ---- 发送 / 接收动作 ----

  async function startSend(): Promise<void> {
    if (!online || !selectedDevice || picked.length === 0 || busy) return;
    busy = true;
    try {
      const single = picked.length === 1 ? picked[0] : null;
      // 单条文本/剪贴板走文本通道；文件夹走 root；多选文件 rel 就是绝对路径
      if (single && (single.kind === "text" || single.kind === "clipboard")) {
        // 正文在 rel 里（preview 只是截断过的展示值，别拿它发送）
        await transferSend(selectedDevice, { mode: "text", text: single.rel });
      } else if (single && single.kind === "folder") {
        const items = await transferListFolder(single.rel);
        await transferSend(selectedDevice, { mode: "files", root: single.rel, items });
      } else {
        const files = picked.filter((item) => item.kind === "file");
        const texts = picked.filter((item) => item.kind === "text" || item.kind === "clipboard");
        if (files.length > 0) {
          await transferSend(selectedDevice, {
            mode: "files",
            root: null,
            items: files.map((item) => ({ rel: item.rel, size: item.size }))
          });
        }
        for (const text of texts) {
          await transferSend(selectedDevice, { mode: "text", text: text.rel });
        }
      }
      picked = [];
    } catch (error) {
      showToast(String(error));
    } finally {
      busy = false;
    }
  }

  async function decideRequest(accept: boolean): Promise<void> {
    const current = request;
    if (!current) return;
    request = null;
    await transferDemand(current.sessionId, accept);
  }

  async function transferDemand(requestId: string, accept: boolean): Promise<void> {
    const { transferDecide } = await import("../backend");
    await transferDecide(requestId, accept).catch((error) => showToast(String(error)));
  }

  function retrySession(session: Session): void {
    sessions = sessions.filter((item) => item.id !== session.id);
    void startSend();
  }

  async function cancelSession(id: string): Promise<void> {
    await transferCancel(id).catch(() => undefined);
  }

  async function chooseSaveDir(): Promise<void> {
    const dir = await pickDirectory();
    if (dir) {
      saveDir = dir;
      if (online) await goOnline();
    }
  }

  async function openSaveDir(): Promise<void> {
    if (!isTauriRuntime) {
      showToast("浏览器预览里没有「打开文件夹」");
      return;
    }
    await transferOpenPath(saveDir).catch((error: unknown) => showToast(String(error)));
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
        await transferSaveText(`${saveDir}/收到的文本-${stamp}.txt`, text);
        showToast("已保存到接收目录");
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: `${saveDir}\\收到的文本.txt` });
      if (!path) return;
      await transferSaveText(path, text);
      showToast("已保存为 .txt");
    } catch (error) {
      showToast(`保存失败：${String(error)}`);
    }
  }

  async function refreshHistory(): Promise<void> {
    try {
      const data = await transferHistory();
      historyEntries = data.entries ?? [];
      deviceHistory = data.devices ?? [];
    } catch {
      historyEntries = [];
      deviceHistory = [];
    }
  }

  async function clearHistory(): Promise<void> {
    await transferClearHistory().catch(() => undefined);
    await refreshHistory();
  }

  // ---- 移动端发送：webview 文件输入 → 分块暂存（scoped storage 下只能这样拿到内容） ----

  let spooling = false;

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
      addItems(items);
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

  function percentOf(session: Session): number {
    const total = session.totalBytes || session.files.reduce((sum, item) => sum + item.total, 0);
    if (total <= 0) return 0;
    const sent = session.files.reduce((sum, item) => sum + Math.min(item.sent, item.total || item.sent), 0);
    return Math.min(100, Math.round((sent / total) * 100));
  }

  function etaText(session: Session): string {
    const sent = session.files.reduce((sum, item) => sum + item.sent, 0);
    const total = session.totalBytes || session.files.reduce((sum, item) => sum + item.total, 0);
    if (session.speed <= 0 || total <= sent) return "";
    const left = (total - sent) / session.speed;
    if (left > 3600) return `剩 ${Math.round(left / 3600)} 小时`;
    if (left > 60) return `剩 ${Math.round(left / 60)} 分`;
    return `剩 ${Math.max(1, Math.round(left))} 秒`;
  }

  function currentFile(session: Session): string {
    const active = session.files.find((item) => !item.done);
    const name = (active ?? session.files[session.files.length - 1])?.rel ?? "";
    return name.split(/[\\/]/).pop() ?? name;
  }

  function durationText(seconds: number): string {
    if (seconds < 60) return `${seconds} 秒`;
    return `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
  }
</script>

<div class="transfer">
  <!-- 顶部身份区：设备名 + 配对口令 + 在线状态 -->
  <section class="transfer-identity">
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
          bind:value={code}
          type={codeVisible ? "text" : "password"}
          placeholder="输入和对方约定的密钥（至少8位）"
          autocomplete="off"
          on:keydown={(event) => { if (event.key === "Enter") void goOnline(); }}
        />
        <button class="transfer-eye" type="button" title={codeVisible ? "隐藏" : "显示"} on:click={() => (codeVisible = !codeVisible)}>
          {codeVisible ? "🙈" : "👁"}
        </button>
      </div>
    </label>
    <div class="transfer-status-line">
      <span class="transfer-dot" class:on={online}></span>
      <span>{online ? `房间在线 · ${devices.length} 台设备` : codeOk ? "未上线" : `口令至少 ${CODE_MIN} 位`}</span>
      {#if online}
        <button class="menu-action-button" type="button" on:click={() => void goOffline()}>下线</button>
      {:else}
        <button class="menu-action-button primary" type="button" disabled={!codeOk || busy} on:click={() => void goOnline()}>上线</button>
      {/if}
    </div>
  </section>

  <!-- 发送 / 接收分段滑块 -->
  <div class="transfer-tabs" role="tablist">
    <button type="button" role="tab" class:active={tab === "send"} on:click={() => (tab = "send")}>发送</button>
    <button type="button" role="tab" class:active={tab === "receive"} on:click={() => (tab = "receive")}>接收</button>
  </div>

  {#if tab === "send"}
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

    {#if picked.length > 0}
      <div class="transfer-picked-head">
        <span>已选 {picked.length} 项 · {sizeText(sendBytes)}</span>
        <button class="menu-action-button" type="button" on:click={() => (picked = [])}>清空</button>
      </div>
      <div class="transfer-picked">
        {#each picked as item (item.key)}
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
      {#if devices.length > 0}<em>在线 {devices.length}</em>{/if}
    </div>
    {#if devices.length === 0}
      <div class="transfer-empty">
        <ShieldCheck size={18} />
        <span>对方输完同一句口令，就会出现在这里</span>
        <em>端到端加密</em>
      </div>
    {:else}
      <div class="transfer-devices">
        {#each devices as device (device.id)}
          <button
            type="button"
            class="transfer-device"
            class:selected={selectedDevice === device.id}
            on:click={() => (selectedDevice = device.id)}
          >
            <span class="transfer-device-icon"><ArrowLeftRight size={16} /></span>
            <span>{device.name || "未命名设备"}</span>
            {#if selectedDevice === device.id}<em>✓</em>{/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="transfer-send-row">
      <button class="transfer-send-button" type="button" disabled={!online || !selectedDevice || picked.length === 0 || busy} on:click={() => void startSend()}>
        <ArrowLeftRight size={17} /> 发送
      </button>
    </div>
  {:else}
    <section class="transfer-receive-head">
      <div class="transfer-status-line">
        <span class="transfer-dot" class:on={online}></span>
        <span>{online ? "待命接收中" : "还没上线"}</span>
      </div>
      <div class="transfer-save-row">
        <span class="transfer-save-dir" title={saveDir}>{saveDir || "（默认保存位置）"}</span>
        {#if caps.nativeFileDialogs}
          <button class="menu-action-button" type="button" on:click={() => void chooseSaveDir()}>更改</button>
        {/if}
        <button class="menu-action-button" type="button" on:click={() => void openSaveDir()}>打开文件夹</button>
      </div>
      <label class="transfer-auto">
        <input type="checkbox" checked={autoAccept} on:change={() => void toggleAutoAccept()} />
        <span>自动接收（不再逐次确认）</span>
      </label>
    </section>

    {#if request}
      <section class="transfer-request">
        <strong>{request.peerName} 想发送 {request.files.length} 个文件 · {sizeText(request.totalBytes)}</strong>
        <div class="transfer-request-list">
          {#each request.files.slice(0, 6) as file (file.rel)}
            <span>{file.rel.split(/[\\/]/).pop()}</span>
          {/each}
          {#if request.files.length > 6}<span>…等 {request.files.length} 个</span>{/if}
        </div>
        <div class="transfer-request-actions">
          <button class="menu-action-button" type="button" on:click={() => void decideRequest(false)}>拒绝</button>
          <button class="menu-action-button primary" type="button" on:click={() => void decideRequest(true)}>接收</button>
        </div>
      </section>
    {/if}

    {#if receivedTexts.length > 0}
      <div class="transfer-text-cards">
        {#each receivedTexts as card (card.id)}
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

  <!-- 传输中的会话卡片（两端共有） -->
  {#if activeSessions.length > 0}
    <section class="transfer-sessions">
      <div class="transfer-devices-head"><span>传输中（{activeSessions.length}）</span></div>
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

  <!-- 完成 / 失败 / 取消的会话卡（5 秒后自动收起） -->
  {#if finishedSessions.some((item) => !item.collapsed)}
    <section class="transfer-sessions">
      {#each finishedSessions.filter((item) => !item.collapsed) as session (session.id)}
        <article class="transfer-session done" class:failed={session.state !== "done"}>
          <header>
            <span class="transfer-session-dir">{session.state === "done" ? "✓" : session.state === "cancelled" ? "✕" : "!"}</span>
            <strong>
              {session.direction === "send" ? "发送" : "接收"}{session.state === "done" ? "完成" : session.state === "cancelled" ? "已取消" : "失败"}
            </strong>
            <em>{session.peerName || ""}{session.error ? ` · ${session.error}` : ""}</em>
            {#if session.retryable && session.state === "error"}
              <button class="menu-action-button" type="button" on:click={() => retrySession(session)}>重试</button>
            {/if}
            {#if session.state === "done" && session.direction === "receive"}
              <button class="menu-action-button" type="button" on:click={() => void openSaveDir()}>打开文件夹</button>
            {/if}
          </header>
        </article>
      {/each}
    </section>
  {/if}

  <!-- 传输历史 -->
  <section class="transfer-history">
    <button class="transfer-history-head" type="button" on:click={() => (historyOpen = !historyOpen)}>
      <span>传输历史</span>
      <em>共 {historyEntries.length} 条</em>
      <span class="transfer-history-caret" class:open={historyOpen}>▾</span>
    </button>
    {#if historyOpen}
      {#if historyEntries.length === 0}
        <div class="transfer-empty"><span>还没有传输记录</span></div>
      {:else}
        <ul class="transfer-history-list">
          {#each historyEntries as entry (entry.id)}
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
          {#if deviceHistory.length > 0}
            <span>常连设备 {deviceHistory.length} 台</span>
          {/if}
          <button class="menu-action-button" type="button" on:click={() => void clearHistory()}>清空历史</button>
        </div>
      {/if}
    {/if}
  </section>

  <input id="transfer-file-input" class="hidden-file" type="file" multiple on:change={spoolFromInput} />
</div>

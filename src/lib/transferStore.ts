/**
 * 文件传输助手的**全局运行时状态**（v0.8.5 需求 4）：工具页只是它的一层视图。
 *
 * 早先所有状态都住在 `TransferTool.svelte` 的组件局部变量里，页面一关全没了——
 * 「退出工具页还继续传」实现了也看不见。现在提到模块级：
 *
 * - **生命周期（定案）**：① 有活跃传输任务，退出工具页 / 应用退到后台都不断连接，
 *   传输不中断；② 已配对（在线过、口令已记住）但没有任务，退出工具页保持在线，
 *   手动点「离线」才下线（应用退出随 Host 收尾）；③ 未配对，退出工具页即清理界面状态。
 * - **事件订阅也在模块级**（`ensureTransferRuntime`，应用启动时挂一次）：工具页没开
 *   时收到的 request / text 要有出口——人不在工具页就得给系统通知，否则「保持在线」
 *   反而变成「60 秒无人确认、静默拒绝」。
 * - 传输中的唤醒锁（wake lock）也在这里：页面隐藏时浏览器会强制释放，
 *   回前台看见还有活跃会话就重新申请。
 */
import { get, writable } from "svelte/store";
import { appSettings, showNotification, showToast } from "./stores";
import { setConfig } from "./actions";
import {
  isTauriRuntime, listenTransfer, transferCancel, transferClearHistory, transferDecide, transferDefaultSaveDir,
  transferHistory, transferListFolder, transferLoadCode, transferOffline, transferOnline, transferSaveCode,
  transferSend, transferSetAutoAccept, transferSetName, transferSpoolClear, transferStatus,
  type TransferDeviceDto, type TransferEvent
} from "./backend";

const CODE_MIN = 8;
/** 完成后的会话卡自动收起（LocalSend 同款） */
const AUTO_COLLAPSE_MS = 5000;

export type SendKind = "file" | "folder" | "text" | "clipboard";
export type PickedItem = { key: string; rel: string; size: number; kind: SendKind; preview: string };

export type FileBar = { index: number; rel: string; sent: number; total: number; done: boolean };
export type TransferSession = {
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

export type ReceivedText = { id: string; peer: string; text: string; at: string; mine: boolean };
export type TransferRequest = {
  sessionId: string;
  peerId: string;
  peerName: string;
  files: Array<{ rel: string; size: number }>;
  totalBytes: number;
};

export type TransferHistoryEntry = {
  id: string;
  at: string;
  direction: string;
  peerName: string;
  status: string;
  files: number;
  bytes: number;
  names?: string[];
};
export type TransferDeviceHistory = { id: string; name: string; lastAt: string; count: number };

export type TransferState = {
  /** 页面内的分段：发送 / 接收（request 到达会切到接收） */
  tab: "send" | "receive";
  online: boolean;
  selfId: string;
  code: string;
  saveDir: string;
  devices: TransferDeviceDto[];
  selectedDevice: string;
  sessions: TransferSession[];
  texts: ReceivedText[];
  requests: TransferRequest[];
  picked: PickedItem[];
  historyEntries: TransferHistoryEntry[];
  deviceHistory: TransferDeviceHistory[];
  busy: boolean;
};

export const transferState = writable<TransferState>({
  tab: "send",
  online: false,
  selfId: "",
  code: "",
  saveDir: "",
  devices: [],
  selectedDevice: "",
  sessions: [],
  texts: [],
  requests: [],
  picked: [],
  historyEntries: [],
  deviceHistory: [],
  busy: false
});

function patch(update: Partial<TransferState>): void {
  transferState.update((state) => ({ ...state, ...update }));
}

/** 工具页当前是否开着：开着就不要再弹系统通知（确认卡就在眼前） */
let viewOpen = false;
export function setTransferViewOpen(open: boolean): void {
  viewOpen = open;
}

/** 工具页的分段与口令框都直接写 store（组件里 `${'$'}transferState` 是派生值，不能 bind） */
export function setTab(tab: "send" | "receive"): void {
  patch({ tab });
}

export function setCode(value: string): void {
  patch({ code: value });
}

function sizeText(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function hasActiveSessions(state: TransferState): boolean {
  return state.sessions.some((item) => item.state === "waiting" || item.state === "connected");
}

// ---------------------------------------------------------------------------
// 事件 → 状态
// ---------------------------------------------------------------------------

function sessionOf(id: string, direction: "send" | "receive"): TransferSession {
  const state = get(transferState);
  const found = state.sessions.find((item) => item.id === id);
  if (found) return found;
  const created: TransferSession = {
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
  transferState.update((current) => ({ ...current, sessions: [...current.sessions, created] }));
  return created;
}

function updateSession(id: string, patchValue: Partial<TransferSession>): void {
  transferState.update((state) => ({
    ...state,
    sessions: state.sessions.map((item) => (item.id === id ? { ...item, ...patchValue } : item))
  }));
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
  updateSession(id, { files, state: "connected" });
  updateSpeed(id);
}

/** 平均速度用 EMA 平滑（前几秒会跳）；剩余时间 = 剩余字节 ÷ EMA。 */
function updateSpeed(id: string): void {
  const session = get(transferState).sessions.find((item) => item.id === id);
  if (!session) return;
  const sent = session.files.reduce((sum, item) => sum + item.sent, 0);
  const elapsed = (Date.now() - session.startedAt) / 1000;
  if (elapsed <= 0.1) return;
  const instant = sent / elapsed;
  const next = session.speed === 0 ? instant : session.speed * 0.72 + instant * 0.28;
  if (Math.abs(next - session.speed) < 1) return;
  updateSession(id, { speed: next });
}

/** 完成/失败/取消 5 秒后自动收起（定时器住模块里：页面关了也照收） */
function scheduleCollapse(id: string): void {
  window.clearTimeout(collapseTimers.get(id));
  collapseTimers.set(
    id,
    window.setTimeout(() => {
      collapseTimers.delete(id);
      updateSession(id, { collapsed: true });
    }, AUTO_COLLAPSE_MS)
  );
}

const collapseTimers = new Map<string, number>();

function directionOf(event: TransferEvent): "send" | "receive" {
  return event.role === "send" ? "send" : "receive";
}

export function handleTransferEvent(event: TransferEvent): void {
  switch (event.kind) {
    case "online":
      patch({ online: true, selfId: event.deviceId ?? get(transferState).selfId });
      void refreshHistory();
      void acquireWakeLockIfActive();
      return;
    case "offline":
      patch({ online: false, devices: [], requests: [] });
      return;
    case "devices": {
      const state = get(transferState);
      const devices = (event.devices ?? []).filter((item) => item.id !== state.selfId);
      const selected = devices.some((item) => item.id === state.selectedDevice)
        ? state.selectedDevice
        : devices.length === 1
          ? devices[0].id
          : "";
      patch({ devices, selectedDevice: selected });
      return;
    }
    case "request": {
      const peerId = typeof event.peer === "object" ? event.peer.id : String(event.peer ?? "");
      const peerName = typeof event.peer === "object" ? event.peer.name || "对方设备" : "对方设备";
      const files = (event.files as unknown as Array<{ rel: string; size: number }>) ?? [];
      patch({ tab: "receive" });
      // 自动接收开着：core 直接放行，不挂确认卡（挂了按钮也是死的——core 那边没有待确认了）
      if (event.auto) {
        if (viewOpen) showToast(`已自动接收 ${peerName} 发来的 ${files.length} 个文件`);
        return;
      }
      const request: TransferRequest = {
        sessionId: event.sessionId,
        peerId,
        peerName,
        files,
        totalBytes: event.totalBytes ?? 0
      };
      transferState.update((state) => ({
        ...state,
        requests: [...state.requests.filter((item) => item.sessionId !== request.sessionId), request]
      }));
      if (!viewOpen) {
        // 人不在工具页：对方在等确认，60 秒不答就当拒绝——必须给一个全局出口
        void showNotification(`${peerName} 想发送 ${files.length} 个文件（${sizeText(request.totalBytes)}），打开「工具箱 → 文件传输助手」确认`, {
          title: "文件传输",
          tone: "info",
          durationMs: 15000
        });
      }
      return;
    }
    case "text": {
      const peerName = typeof event.peer === "object" ? event.peer.name || "对方设备" : "对方设备";
      const card: ReceivedText = {
        id: event.sessionId,
        peer: peerName,
        text: event.text ?? "",
        at: new Date().toLocaleTimeString().slice(0, 5),
        mine: event.received === false
      };
      transferState.update((state) => ({
        ...state,
        texts: [card, ...state.texts.filter((item) => item.id !== card.id)].slice(0, 30)
      }));
      if (!card.mine) {
        patch({ tab: "receive" });
        if (!viewOpen) {
          void showNotification(`收到来自 ${peerName} 的文本：${card.text.slice(0, 40)}`, {
            title: "文件传输",
            tone: "info",
            durationMs: 12000
          });
        }
      }
      void refreshHistory();
      return;
    }
    case "waiting":
      updateSession(event.sessionId, { state: "waiting" });
      return;
    case "connected": {
      const peerName = event.peerName ?? "";
      const session = sessionOf(event.sessionId, directionOf(event));
      updateSession(event.sessionId, {
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
      upsertBar(event.sessionId, directionOf(event), event);
      return;
    case "done": {
      const session = sessionOf(event.sessionId, directionOf(event));
      updateSession(event.sessionId, {
        state: "done",
        totalBytes: event.totalBytes ?? session.totalBytes
      });
      scheduleCollapse(event.sessionId);
      void refreshHistory();
      releaseWakeLockIfIdle();
      const dir = event.dir ?? get(transferState).saveDir;
      if (session.direction === "send") {
        void showNotification(`已发送给 ${session.peerName || "对方"}`, { title: "传输完成", tone: "success" });
      } else {
        void showNotification(`已保存到 ${dir}`, { title: "接收完成", tone: "success" });
      }
      return;
    }
    case "cancelled": {
      const session = sessionOf(event.sessionId, directionOf(event));
      updateSession(event.sessionId, { state: "cancelled" });
      scheduleCollapse(event.sessionId);
      void refreshHistory();
      releaseWakeLockIfIdle();
      return;
    }
    case "error": {
      if (event.role === "online") {
        // 在线会话整个断了：设备列表与待确认请求都失效
        patch({ online: false, devices: [], requests: [] });
        showToast(event.message ?? "传输连接断了");
        return;
      }
      const session = sessionOf(event.sessionId, directionOf(event));
      const finished = session.state === "connected";
      updateSession(event.sessionId, {
        state: "error",
        error: finished ? "对方离线了" : event.message ?? "传输失败",
        retryable: session.direction === "send"
      });
      scheduleCollapse(event.sessionId);
      void refreshHistory();
      releaseWakeLockIfIdle();
      return;
    }
  }
}

// ---------------------------------------------------------------------------
// 保持常亮（移动端锁屏会断连）
// ---------------------------------------------------------------------------

let wakeLock: { release?: () => Promise<void> } | null = null;

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

function releaseWakeLockIfIdle(): void {
  if (hasActiveSessions(get(transferState))) return;
  void releaseWakeLock();
}

function acquireWakeLockIfActive(): void {
  if (hasActiveSessions(get(transferState))) void acquireWakeLock();
}

/**
 * 页面隐藏时浏览器会强制释放屏幕唤醒锁（规范要求），回前台要自己重新申请，
 * 否则「传输中切出去看一眼再回来」之后锁屏又会断连。
 */
function onVisibilityChange(): void {
  if (document.visibilityState === "visible") acquireWakeLockIfActive();
}

// ---------------------------------------------------------------------------
// 动作
// ---------------------------------------------------------------------------

export async function goOnline(): Promise<void> {
  const state = get(transferState);
  const code = state.code.trim();
  if (code.length < CODE_MIN || state.busy || !isTauriRuntime) return;
  const settings = get(appSettings).transfer;
  patch({ busy: true });
  try {
    const result = await transferOnline(code, state.saveDir, settings?.deviceName ?? "", Boolean(settings?.autoAccept));
    patch({ online: true, selfId: result.deviceId, code });
    await transferSaveCode(code);
  } catch (error) {
    patch({ online: false });
    showToast(String(error));
  } finally {
    patch({ busy: false });
  }
}

async function disconnect(): Promise<void> {
  try {
    await transferOffline();
  } catch {
    // 离线失败无实质后果
  }
  patch({ online: false, devices: [], requests: [], selectedDevice: "" });
}

/** 手动离线：这次配对结束——记住的口令一并清掉，免得重启应用又自己上线 */
export async function goOffline(): Promise<void> {
  await disconnect();
  await transferSaveCode("").catch(() => undefined);
}

/**
 * relay 地址在 go_online 时固化进端点：改完要重新上线才生效（v0.8.5 需求 31）。
 * 走内部的重启，**不清记住的口令**（这不是「手动离线」那种配对结束）。
 */
export async function applyRelay(value: string): Promise<void> {
  await setConfig("transfer.relay", value.trim());
  const state = get(transferState);
  if (!state.online) return;
  await disconnect();
  await goOnline();
  showToast("relay 已保存，传输已重新上线");
}

/** 改名：写设置 + 通知 core 更新房间里的名字记录 */
export async function applyDeviceName(name: string): Promise<void> {
  await setConfig("transfer.deviceName", name);
  if (get(transferState).online) {
    await transferSetName(name).catch(() => undefined);
  }
}

export async function applyAutoAccept(value: boolean): Promise<void> {
  await setConfig("transfer.autoAccept", value);
  if (get(transferState).online) {
    await transferSetAutoAccept(value).catch(() => undefined);
  }
}

/** 换保存位置：在线时要重新上线才生效（core 把保存目录固化在会话里） */
export async function setSaveDir(dir: string): Promise<void> {
  patch({ saveDir: dir });
  if (!get(transferState).online) return;
  await disconnect();
  await goOnline();
}

export function addPicked(items: PickedItem[]): void {
  const state = get(transferState);
  const keys = new Set(state.picked.map((item) => item.key));
  patch({ picked: [...state.picked, ...items.filter((item) => !keys.has(item.key))], tab: "send" });
}

export function removePicked(key: string): void {
  patch({ picked: get(transferState).picked.filter((item) => item.key !== key) });
}

export function clearPicked(): void {
  patch({ picked: [] });
}

export function selectDevice(id: string): void {
  patch({ selectedDevice: id });
}

export async function startSend(): Promise<void> {
  const state = get(transferState);
  if (!state.online || !state.selectedDevice || state.picked.length === 0 || state.busy) return;
  const target = state.selectedDevice;
  const picked = state.picked;
  patch({ busy: true });
  try {
    const single = picked.length === 1 ? picked[0] : null;
    // 单条文本/剪贴板走文本通道；文件夹走 root；多选文件 rel 就是绝对路径
    if (single && (single.kind === "text" || single.kind === "clipboard")) {
      // 正文在 rel 里（preview 只是截断过的展示值，别拿它发送）
      await transferSend(target, { mode: "text", text: single.rel });
    } else if (single && single.kind === "folder") {
      const items = await transferListFolder(single.rel);
      await transferSend(target, { mode: "files", root: single.rel, items });
    } else {
      const files = picked.filter((item) => item.kind === "file");
      const texts = picked.filter((item) => item.kind === "text" || item.kind === "clipboard");
      if (files.length > 0) {
        await transferSend(target, {
          mode: "files",
          root: null,
          items: files.map((item) => ({ rel: item.rel, size: item.size }))
        });
      }
      for (const text of texts) {
        await transferSend(target, { mode: "text", text: text.rel });
      }
    }
    patch({ picked: [] });
  } catch (error) {
    showToast(String(error));
  } finally {
    patch({ busy: false });
  }
}

export async function decideRequest(requestId: string, accept: boolean): Promise<void> {
  await transferDecide(requestId, accept).catch((error) => showToast(String(error)));
  transferState.update((state) => ({
    ...state,
    requests: state.requests.filter((item) => item.sessionId !== requestId)
  }));
}
export async function cancelSession(id: string): Promise<void> {
  await transferCancel(id).catch(() => undefined);
}

export function dismissSession(id: string): void {
  transferState.update((state) => ({ ...state, sessions: state.sessions.filter((item) => item.id !== id) }));
}

export async function refreshHistory(): Promise<void> {
  try {
    const data = await transferHistory();
    patch({ historyEntries: data.entries ?? [], deviceHistory: data.devices ?? [] });
  } catch {
    patch({ historyEntries: [], deviceHistory: [] });
  }
}

export async function clearHistory(): Promise<void> {
  await transferClearHistory().catch(() => undefined);
  await refreshHistory();
}

/**
 * 退出工具页（组件 onDestroy）：
 * 已配对（在线）保持一切——在线会话、传输、清单都留着；
 * 未配对且没有活任务时清理界面状态（会话、收到文本、待确认请求、清单）。
 */
export function leaveTransferPage(): void {
  const state = get(transferState);
  if (state.online || hasActiveSessions(state)) return;
  patch({ tab: "send", sessions: [], texts: [], requests: [], picked: [], selectedDevice: "" });
  // 清单没了，暂存区里的字节也没人要了（在线时**不能**清：可能有正在发的东西）
  void transferSpoolClear().catch(() => undefined);
}

/**
 * 应用级初始化（App.svelte 调一次）：订阅传输事件 + 恢复记住的口令。
 * 幂等；浏览器预览（非 Tauri）下什么都不做。
 */
let started = false;
export function ensureTransferRuntime(): void {
  if (started || !isTauriRuntime) return;
  started = true;
  void listenTransfer(handleTransferEvent)
    .then(() => {
      // 订阅就绪之后再问 core 当前状态：窗口重开 / 页面刷新后界面与 core 对账
      void transferStatus()
        .then((status) => {
          patch({
            online: Boolean(status.online),
            selfId: status.deviceId ?? "",
            devices: status.devices ?? []
          });
          if (get(transferState).online) acquireWakeLockIfActive();
        })
        .catch(() => undefined);
    })
    .catch(() => undefined);
  document.addEventListener("visibilitychange", onVisibilityChange);
  void transferDefaultSaveDir()
    .then((dir) => patch({ saveDir: dir }))
    .catch(() => undefined);
  void refreshHistory();
  // 记住的口令：应用起来就恢复在线（工具页没开也收得到拨入）
  void transferLoadCode()
    .then((stored) => {
      const remembered = stored.code?.trim() ?? "";
      if (remembered.length < CODE_MIN) return;
      patch({ code: remembered });
      void goOnline();
    })
    .catch(() => undefined);
}

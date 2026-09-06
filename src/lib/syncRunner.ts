// ---------------------------------------------------------------------------
// 自动同步循环（v0.5.0 重写，v0.5.1 校准节奏）。
//
// v0.5.0 修的是「整条自动同步是死代码」：App onMount 里 `startAutoSync()` 跑在
// `hydrate()` 完成之前，那时 `coreMode` 还是 false，函数第一行就 return ——
// 定时器从来没建立过，所以间隔设成多少都没用。
//
// v0.5.1 修的是节奏：
// - **按绝对截止时间排程**：下一轮 = 本轮开始时间 + 间隔。以前是「跑完再等一个间隔」，
//   周期 = 间隔 + 同步耗时，一轮慢一点就逐轮累积漂移，看起来跟设定值对不上；
// - **配置改动实时生效**：改了间隔立刻按新值重算截止时间，已经过期就马上同步一次
//   （不是等旧的排程跑完）；
// - **暂停**：`sync.enabled = false` 时循环停止（配对信息保留），恢复即刻继续；
// - 排程时间写进 `nextSyncAt` store，设置面板显示倒计时，节奏对不对一眼可见。
//
// 其余语义不变：等水合完成再启动并立即同步一次（初次连接）；递归 setTimeout 而非
// setInterval（跑完再排，绝不并发）；掉线按 `reconnectSeconds` 静默重连，恢复即同步；
// 自动同步不发通知；回前台且距上次同步超过一个间隔时补一次（Android 会节流后台定时器）。
// ---------------------------------------------------------------------------

import { get } from "svelte/store";
import { appSettings, coreMode, isHydrated, nextSyncAt, syncConnection } from "./stores";
import { syncNow } from "./actions";

/** 与 core 侧一致的下限：低于 5 秒按 5 秒生效。 */
const MIN_INTERVAL_SECONDS = 5;
const MAX_INTERVAL_SECONDS = 86400;
const DEFAULT_INTERVAL_SECONDS = 30;
const DEFAULT_RECONNECT_SECONDS = 300;
/** 上一轮还没跑完时的重试间隔（绝不并发两次同步） */
const BUSY_RETRY_MS = 1500;

type SyncConfig = {
  /**
   * 配对 = 有账户密码 + 当前通信方式有一个明确对端（与 core 的 `SyncSettings::is_paired`
   * 同口径）。解除配对才会清密码；`enabled = false` 是暂停，不是未配对。
   */
  paired: boolean;
  /** false = 用户暂停同步（配置保留） */
  enabled: boolean;
  /**
   * 对端标识：自建服务是地址，局域网是主机名（本机作为主机时是 `@self`）。
   * 只用于「配置变了要不要立刻重排/补一轮」的签名比较。
   */
  target: string;
  intervalSeconds: number;
  reconnectSeconds: number;
};

let timer: ReturnType<typeof setTimeout> | null = null;
let running = false;
let started = false;
let booted = false;
let signature = "";
let lastShouldRun = false;
let roundStartedAt = 0;
let lastFinishedAt = 0;

function clamp(value: unknown, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(MAX_INTERVAL_SECONDS, Math.max(MIN_INTERVAL_SECONDS, Math.round(value)));
}

function currentConfig(): SyncConfig {
  const sync = get(appSettings)?.sync;
  const mode = sync?.mode ?? "lan";
  const username = (sync?.username ?? "").trim();
  const secret = (sync?.secret ?? "").trim();
  const serverUrl = (sync?.serverUrl ?? "").trim();
  const lanPeer = (sync?.lanPeer ?? "").trim();
  const lanHost = Boolean(sync?.lanHost);
  // 局域网：本机是主机就连自己，否则必须已经选定了一台主机（角色二选一）
  const hasPeer =
    mode === "server" ? serverUrl.length > 0 : mode === "lan" ? lanHost || lanPeer.length > 0 : true;
  const target = mode === "server" ? serverUrl : mode === "lan" ? (lanHost ? "@self" : lanPeer) : "@p2p";
  return {
    paired: username.length > 0 && secret.length > 0 && hasPeer,
    enabled: Boolean(sync?.enabled),
    target,
    intervalSeconds: clamp(sync?.intervalSeconds, DEFAULT_INTERVAL_SECONDS),
    reconnectSeconds: clamp(sync?.reconnectSeconds, DEFAULT_RECONNECT_SECONDS)
  };
}

function shouldRun(config: SyncConfig): boolean {
  return config.paired && config.enabled;
}

function signatureOf(config: SyncConfig): string {
  return [config.paired, config.enabled, config.target, config.intervalSeconds, config.reconnectSeconds].join("|");
}

function clearTimer(): void {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
  nextSyncAt.set(null);
}

/** 排下一轮：同时把绝对截止时间写进 store（面板倒计时读它）。 */
function arm(delayMs: number): void {
  clearTimer();
  if (!shouldRun(currentConfig())) return;
  const wait = Math.max(1000, delayMs);
  nextSyncAt.set(Date.now() + wait);
  timer = setTimeout(() => void tick(), wait);
}

/** 按「本轮开始时间 + 间隔」算下一轮，周期严格等于设定值。 */
function scheduleNext(online: boolean): void {
  const config = currentConfig();
  if (!shouldRun(config)) {
    clearTimer();
    return;
  }
  // 在线按用户配置的间隔；掉线按重连间隔静默重试
  const intervalMs = (online ? config.intervalSeconds : config.reconnectSeconds) * 1000;
  const due = (roundStartedAt || Date.now()) + intervalMs;
  arm(due - Date.now());
}

async function tick(): Promise<void> {
  const config = currentConfig();
  if (!shouldRun(config)) {
    clearTimer();
    return;
  }
  if (running) {
    // 上一轮还没跑完（例如用户刚点了「立即同步」）：稍后再排，绝不并发
    arm(BUSY_RETRY_MS);
    return;
  }
  running = true;
  roundStartedAt = Date.now();
  let online = false;
  try {
    online = await syncNow({ silent: true });
  } catch {
    online = false;
  } finally {
    running = false;
    lastFinishedAt = Date.now();
  }
  scheduleNext(online);
}

/** 水合完成后调用：启动循环并立刻同步一次（初次连接）。 */
function boot(): void {
  if (booted || !coreMode || !get(isHydrated)) return;
  booted = true;
  const config = currentConfig();
  signature = signatureOf(config);
  lastShouldRun = shouldRun(config);
  if (!lastShouldRun) {
    clearTimer();
    syncConnection.set({ online: null });
    return;
  }
  void tick();
}

function handleSettingsChange(): void {
  if (!booted) return;
  const config = currentConfig();
  const next = signatureOf(config);
  if (next === signature) return;
  const wasRunning = lastShouldRun;
  signature = next;
  lastShouldRun = shouldRun(config);
  if (!lastShouldRun) {
    // 解除配对或暂停：停掉排程（暂停时保留上次的连接结论，不误报掉线）
    clearTimer();
    if (!config.paired) syncConnection.set({ online: null });
    return;
  }
  if (!wasRunning) {
    // 刚配对 / 刚从暂停恢复 / 刚换服务器地址：立即同步一次
    void tick();
    return;
  }
  // 只是改了间隔或重连节奏：按新间隔从上一轮开始时间重算，已过期就立刻同步
  const due = (roundStartedAt || lastFinishedAt || Date.now()) + config.intervalSeconds * 1000;
  const wait = due - Date.now();
  if (wait <= 0) void tick();
  else arm(wait);
}

function handleVisibility(): void {
  if (typeof document === "undefined" || document.visibilityState !== "visible") return;
  if (!booted || running) return;
  const config = currentConfig();
  if (!shouldRun(config)) return;
  const base = Math.max(roundStartedAt, lastFinishedAt);
  if (base > 0 && Date.now() - base < config.intervalSeconds * 1000) return;
  void tick();
}

/**
 * App onMount 调用。注意：**不能**在这里用 coreMode 门控——onMount 时水合还没完成，
 * coreMode 恒为 false，那正是旧实现整条自动同步形同虚设的原因。
 */
export function startAutoSync(): void {
  if (started) return;
  started = true;
  boot();
  isHydrated.subscribe((ready) => {
    if (ready) boot();
  });
  appSettings.subscribe(handleSettingsChange);
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", handleVisibility);
  }
}

// ---------------------------------------------------------------------------
// 自动同步循环（v0.5.0 重写）。
//
// 旧实现有一个致命 bug：App onMount 里 `startAutoSync()` 跑在 `hydrate()` 完成之前，
// 那时 `coreMode` 还是 false，函数第一行就 return —— 定时器**从来没建立过**，
// 所以「自动同步间隔」看起来完全不生效，只能两端各自手点「立即同步」。
//
// 现在的语义：
// - 等水合完成（coreMode + isHydrated）再启动，启动即同步一次（初次连接）；
// - 每轮跑完再排下一轮（递归 setTimeout 而非 setInterval）：同步耗时超过间隔时
//   不会堆积并发，也便于在「在线节奏」与「掉线节奏」之间切换；
// - 掉线后按 `reconnectSeconds`（默认 5 分钟）静默重连，恢复即立刻同步；
// - 自动同步不发通知（周期性弹通知很烦人），结果只在设置页「最近同步」里体现；
// - 间隔/开关/服务器地址变化自动重排；从未配对变为配对时立即同步一次；
// - 页面从后台回到前台时，若距上次同步已超过一个间隔则立即补一次
//   （Android 会节流后台定时器，这条保证一回到应用就能看到对端改动）。
// ---------------------------------------------------------------------------

import { get } from "svelte/store";
import { appSettings, coreMode, isHydrated, syncConnection } from "./stores";
import { syncNow } from "./actions";

/** 与 core 侧一致的下限：低于 5 秒按 5 秒生效。 */
const MIN_INTERVAL_SECONDS = 5;
const MAX_INTERVAL_SECONDS = 86400;
const DEFAULT_RECONNECT_SECONDS = 300;

type SyncConfig = {
  enabled: boolean;
  serverUrl: string;
  intervalSeconds: number;
  reconnectSeconds: number;
};

let timer: ReturnType<typeof setTimeout> | null = null;
let running = false;
let started = false;
let booted = false;
let signature = "";
let lastFinishedAt = 0;

function clamp(value: unknown, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(MAX_INTERVAL_SECONDS, Math.max(MIN_INTERVAL_SECONDS, Math.round(value)));
}

function currentConfig(): SyncConfig {
  const sync = get(appSettings)?.sync;
  return {
    enabled: Boolean(sync?.enabled),
    serverUrl: (sync?.serverUrl ?? "").trim(),
    intervalSeconds: clamp(sync?.intervalSeconds, 30),
    reconnectSeconds: clamp(sync?.reconnectSeconds, DEFAULT_RECONNECT_SECONDS)
  };
}

function isPaired(config: SyncConfig): boolean {
  return config.enabled && config.serverUrl.length > 0;
}

function signatureOf(config: SyncConfig): string {
  return [config.enabled, config.serverUrl, config.intervalSeconds, config.reconnectSeconds].join("|");
}

function clearTimer(): void {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

function arm(delayMs: number): void {
  clearTimer();
  if (!isPaired(currentConfig())) return;
  timer = setTimeout(() => void tick(), Math.max(1000, delayMs));
}

async function tick(): Promise<void> {
  if (running) {
    // 上一轮还没跑完（例如用户刚点了「立即同步」）：稍后再排，绝不并发
    arm(2000);
    return;
  }
  running = true;
  let online = false;
  try {
    online = await syncNow({ silent: true });
  } catch {
    online = false;
  } finally {
    running = false;
    lastFinishedAt = Date.now();
  }
  const config = currentConfig();
  // 在线按用户配置的间隔；掉线按重连间隔静默重试
  arm((online ? config.intervalSeconds : config.reconnectSeconds) * 1000);
}

/** 水合完成后调用：启动循环并立刻同步一次（初次连接）。 */
function boot(): void {
  if (booted || !coreMode || !get(isHydrated)) return;
  booted = true;
  const config = currentConfig();
  signature = signatureOf(config);
  if (!isPaired(config)) {
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
  const [wasEnabled, wasUrl] = signature.split("|");
  const wasPaired = wasEnabled === "true" && Boolean(wasUrl);
  signature = next;
  if (!isPaired(config)) {
    clearTimer();
    syncConnection.set({ online: null });
    return;
  }
  if (!wasPaired) {
    // 刚配对 / 刚换服务器地址：立即同步一次
    void tick();
    return;
  }
  // 只是改了间隔或重连节奏：按新配置重排
  arm(config.intervalSeconds * 1000);
}

function handleVisibility(): void {
  if (typeof document === "undefined" || document.visibilityState !== "visible") return;
  if (!booted || running || !isPaired(currentConfig())) return;
  const intervalMs = currentConfig().intervalSeconds * 1000;
  if (lastFinishedAt > 0 && Date.now() - lastFinishedAt < intervalMs) return;
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

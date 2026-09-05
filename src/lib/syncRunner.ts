// 自动同步循环（v0.4.1）：配对后按 sync.intervalSeconds 周期 pull+push。
// 桌面与移动端通用：App onMount 启动一次；浏览器预览（无 core）不启动。

import { get } from "svelte/store";
import { appSettings, coreMode } from "./stores";
import { syncNow } from "./actions";

let timer: ReturnType<typeof setInterval> | null = null;
let running = false;

function currentIntervalSeconds(): number {
  const sync = get(appSettings)?.sync;
  if (!sync?.enabled || !sync.serverUrl) return 0;
  const seconds = sync.intervalSeconds;
  return typeof seconds === "number" && seconds >= 5 ? seconds : 30;
}

function schedule(): void {
  const seconds = currentIntervalSeconds();
  if (seconds > 0) {
    timer = setInterval(() => void tick(), seconds * 1000);
  }
}

async function tick(): Promise<void> {
  if (running) return;
  running = true;
  try {
    await syncNow();
  } catch {
    // 网络抖动等失败静默（结果在设置页最近同步里可见）
  } finally {
    running = false;
  }
}

/** App onMount 调用（避免模块顶层订阅 stores 的循环依赖 TDZ）。 */
export function startAutoSync(): void {
  if (!coreMode || timer !== null) return;
  schedule();
  // 设置变化（间隔/开关/配对状态）后重排定时器
  appSettings.subscribe(() => {
    if (!coreMode) return;
    const seconds = currentIntervalSeconds();
    const wasActive = timer !== null;
    const shouldRun = seconds > 0;
    if (shouldRun !== wasActive || (shouldRun && timer !== null && currentIntervalSeconds() !== seconds)) {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
      if (shouldRun) {
        schedule();
      }
    }
  });
}

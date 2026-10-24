/**
 * 安卓后台常驻（v0.9.0）的前端一侧。
 *
 * 真正的保活是 Kotlin 的前台服务（`KeepAliveService`）；这里只做两件事：
 * ① 把设置里的开关与标语**对齐**到服务；② 桥没就绪时重试（token 由 MainActivity
 * 反复注入，冷启动的头几秒可能还没写进主 frame，一次调用失败就等于设置不生效）。
 *
 * 非安卓端一律空操作，所以调用方不必自己判平台——「设置变了就 reconcile」可以写成
 * 一条无条件的响应式语句。
 */
import { androidBridge, androidToken, readNativeKeepAlive, setNativeKeepAlive } from "./androidBridge";
import { hostOs } from "./platform";
import { showToast } from "./stores";

const RETRY_INTERVAL_MS = 500;
const MAX_ATTEMPTS = 40;

let timer: number | undefined;
let pending: { enabled: boolean; notice: string } | null = null;
let attempts = 0;
let applied = "";

function step(): void {
  timer = undefined;
  const want = pending;
  if (!want) return;
  if (!androidBridge() || !androidToken()) {
    if (attempts < MAX_ATTEMPTS) {
      attempts += 1;
      timer = window.setTimeout(step, RETRY_INTERVAL_MS);
    }
    return;
  }
  attempts = 0;
  applied = signature(want.enabled, want.notice);
  const error = setNativeKeepAlive(want.enabled, want.notice);
  if (error) showToast(`后台常驻没能生效：${error}`);
}

function signature(enabled: boolean, notice: string): string {
  return `${enabled ? "on" : "off"}|${notice}`;
}

/** 把服务对齐到设置。同值重复调用会被跳过（服务已经在跑就不必再惊动它一次）。 */
export function reconcileKeepAlive(enabled: boolean, notice: string): void {
  if (hostOs !== "android") return;
  const next = signature(enabled, notice);
  if (next === applied && pending === null) return;
  pending = { enabled, notice };
  attempts = 0;
  if (timer !== undefined) return; // 已有一轮在跑，它取的是最新的 pending
  timer = window.setTimeout(step, 0);
}

/** 常驻通知的状态；桥不可用（桌面 / 浏览器 dev）时返回 null。 */
export function keepAliveState() {
  if (hostOs !== "android") return null;
  return readNativeKeepAlive();
}

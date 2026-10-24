/**
 * 剪切板同步的前端运行时（v0.8.9）。
 *
 * 桌面的「读本机剪切板 / 写回系统剪切板」都在 Rust 侧完成（窗口隐藏到托盘也要同步），
 * 这里只订阅状态事件。Android 受系统限制：10+ 只有前台应用能读剪切板，所以由
 * MainActivity 在 onResume 注册监听、变化时发 `kxtodo-native-clipboard`，
 * 本模块再经 Kotlin 桥读内容并调用 `clipboard_local_changed`；远端内容也由这里写回。
 *
 * 回声抑制：写回远端内容后 3 秒内忽略一次原生变化事件（Android 会把我们自己的写入
 * 再报一次），避免 A → B → A 的乒乓。核心侧还有按内容指纹的第二道闸。
 */
import { get, writable } from "svelte/store";
import { appSettings, showToast } from "./stores";
import { isMobile } from "./platform";
import {
  clipboardLocalChanged, clipboardStatus, listenClipboard,
  type ClipboardEvent, type ClipboardStatus
} from "./backend";
import { exportNativeImageToGallery, readNativeClipboard, writeNativeClipboard } from "./androidBridge";

export const clipboardSyncStatus = writable<ClipboardStatus | null>(null);

// 仅开发服务器：给 Playwright 注入运行态（真实 Tauri 里状态由后端事件驱动）。
// 不用动态 import 模块路径——HMR 后模块 URL 带 ?t=，测试拿到的是另一份实例。
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__kxtodoClipboardStatus = clipboardSyncStatus;
}

let started = false;
let suppressNativeUntil = 0;
let lastLocalSignature = "";
/** 首次读到剪切板只用来建基线，不发出去：Android 上那可能是后台时复制的旧内容。 */
let primed = false;

export async function refreshClipboardStatus(): Promise<void> {
  try {
    clipboardSyncStatus.set(await clipboardStatus());
  } catch {
    // 浏览器预览/宿主未注册：保持 null，工具页显示「不可用」
  }
}

function clipboardEnabled(): boolean {
  const settings = get(appSettings).clipboard;
  return (
    settings.enabled &&
    (settings.lanEnabled || settings.serverEnabled || settings.wanP2pEnabled)
  );
}

function handleNativeClipboard(): void {
  if (!get(isMobile) || !clipboardEnabled()) return;
  // 刚写回远端内容：Android 自己也会收到 OnPrimaryClipChanged，忽略这一次。
  if (suppressNativeUntil > Date.now()) {
    suppressNativeUntil = 0;
    return;
  }
  const content = readNativeClipboard();
  if (!content) return;
  if (!content.text && !content.imagePath && content.files.length === 0) return;
  const signature = [
    content.text,
    content.imagePath ? "image" : "",
    ...content.files.map((file) => `${file.name}:${file.path}`)
  ].join("\u0000");
  if (signature === lastLocalSignature) return;
  lastLocalSignature = signature;
  void clipboardLocalChanged(content).catch((error) => {
    showToast(`发送剪切板失败：${String(error)}`, 4000);
  });
}

async function handleEvent(event: ClipboardEvent): Promise<void> {
  if (event.kind === "status") {
    clipboardSyncStatus.set(event.status);
    return;
  }
  if (event.kind !== "remote" || !get(isMobile)) return;
  suppressNativeUntil = Date.now() + 3000;
  const error = writeNativeClipboard(event.content);
  if (error) {
    showToast(`写入本机剪切板失败：${error}`, 5000);
    return;
  }
  // 收到的图片另存一份进相册（v0.9.0 bugfix 4.2）：Pictures/kxtodo，相册里直接可见。
  // 失败只提示不打断——剪切板里那一份已经写好了。
  if (event.content.imagePath) {
    const galleryError = exportNativeImageToGallery(event.content.imagePath);
    if (galleryError && galleryError !== "需要存储权限") {
      showToast(`图片没能存进相册：${galleryError}`, 5000);
    }
  }
  await refreshClipboardStatus();
}

export function ensureClipboardRuntime(): void {
  if (started) return;
  started = true;
  void listenClipboard((event) => void handleEvent(event)).catch(() => undefined);
  void refreshClipboardStatus();

  if (!get(isMobile)) return;
  // 只在前台读：MainActivity 只在 onResume 注册系统监听，这里的事件也来自那里。
  window.addEventListener("kxtodo-native-clipboard", handleNativeClipboard);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") handleNativeClipboard();
  });
  // 设置里刚打开同步：补读一次当前剪切板（否则要等用户下一次复制）。
  let wasEnabled = clipboardEnabled();
  appSettings.subscribe(() => {
    const enabled = clipboardEnabled();
    if (enabled && !wasEnabled) {
      primed = false;
      window.setTimeout(handleNativeClipboard, 400);
    }
    wasEnabled = enabled;
  });
}

import { get, writable } from "svelte/store";
import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { editorTaskId, showSettings } from "./stores";

/**
 * Mobile detection is intentionally user-agent based so the Windows desktop
 * experience is never affected (the desktop window enforces a large min-width).
 * Only Android / iOS web views flip the app into the stacked mobile layout.
 */
function detectMobile(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }
  const ua = navigator.userAgent || "";
  return /Android|iPhone|iPad|iPod/i.test(ua);
}

export const isMobile = writable(detectMobile());

export type HostOs = "windows" | "linux" | "macos" | "android" | "ios" | "unknown";

/**
 * 宿主 OS 检测以官方 os 插件为准（同步读注入的 internals，浏览器 dev / 未注册
 * 插件的端——如 Android APK——会抛异常），UA 仅是这些场景下的回退。
 */
function detectHostOs(): HostOs {
  try {
    const value = tauriPlatform();
    if (value === "android" || value === "ios" || value === "linux" || value === "windows" || value === "macos") {
      return value;
    }
    return "unknown";
  } catch {
    if (typeof navigator === "undefined") {
      return "unknown";
    }
    const ua = navigator.userAgent || "";
    if (/Android|iPhone|iPad|iPod/i.test(ua)) {
      return /Android/i.test(ua) ? "android" : "ios";
    }
    if (/Linux|X11/i.test(ua)) return "linux";
    if (/Windows/i.test(ua)) return "windows";
    if (/Mac OS X|Macintosh/i.test(ua)) return "macos";
    return "unknown";
  }
}

export const hostOs: HostOs = detectHostOs();

/**
 * Microsoft To-Do style mobile navigation: the app opens on the category list
 * and tapping an entry pushes the content view. The back button returns here.
 */
export type MobileView = "list" | "content" | "toolbox";

export const mobileView = writable<MobileView>("list");

type MobileLayer = "content" | "settings" | "editor" | "toolbox";

function currentLayer(): string | undefined {
  if (typeof history === "undefined") return undefined;
  return (history.state as { mv?: string } | null)?.mv;
}

function pushLayer(layer: MobileLayer): void {
  if (typeof history === "undefined") return;
  history.pushState({ mv: layer }, "");
}

/**
 * popstate 回写 store 时，store 订阅回调不能再执行 history 操作（back/push），
 * 否则会形成 push/back 循环。守卫在 popstate 处理期间置位，异步释放，保证所有
 * 同步触发的订阅回调都能看到它。
 */
let applyingHistory = false;

function releaseGuardLater(): void {
  window.setTimeout(() => {
    applyingHistory = false;
  }, 0);
}

function handlePopState(event: PopStateEvent): void {
  if (!get(isMobile)) return;
  applyingHistory = true;
  const layer = (event.state as { mv?: string } | null)?.mv;
  switch (layer) {
    case "content":
      mobileView.set("content");
      showSettings.set(false);
      editorTaskId.set(null);
      break;
    case "toolbox":
      mobileView.set("toolbox");
      showSettings.set(false);
      editorTaskId.set(null);
      break;
    case "settings":
      // 设置页覆盖在列表之上：底层固定回列表视图
      mobileView.set("list");
      showSettings.set(true);
      editorTaskId.set(null);
      break;
    case "editor":
      // 编辑器仍在顶层，由 editorTaskId 订阅驱动，这里不回写 store
      break;
    default:
      // 回到基础层（列表）
      mobileView.set("list");
      showSettings.set(false);
      editorTaskId.set(null);
      break;
  }
  releaseGuardLater();
}

export function startMobileRouter(): void {
  if (typeof window === "undefined") return;
  window.addEventListener("popstate", handlePopState);

  // 设置抽屉：打开压一层 {mv:"settings"}；关闭时若顶层正是它则 history.back()，
  // 让硬件返回键与关闭按钮走同一条历史栈。
  showSettings.subscribe((open) => {
    if (!get(isMobile) || applyingHistory) return;
    if (open) {
      if (currentLayer() !== "settings") pushLayer("settings");
    } else if (currentLayer() === "settings") {
      history.back();
    }
  });

  // 浮窗编辑器：null → id 压层 {mv:"editor"}；id → null 且顶层是它则回退。
  editorTaskId.subscribe((id) => {
    if (!get(isMobile) || applyingHistory) return;
    if (id !== null) {
      if (currentLayer() !== "editor") pushLayer("editor");
    } else if (currentLayer() === "editor") {
      history.back();
    }
  });
}

// 模块顶层不能挂路由：platform→stores→backend→capabilities→platform 存在
// 循环依赖，stores 的 showSettings/editorTaskId 在本模块体执行时仍处于 TDZ
// （启动即 ReferenceError 白屏）。由 App.svelte 在 onMount 中调用。

export function showMobileContent(): void {
  if (!get(isMobile)) {
    return;
  }
  mobileView.set("content");
  // 已经在 content 或更深层（settings/editor）时不重复压栈，
  // 让 Android 硬件返回键经 popstate 回到列表而不是退出应用。
  if (currentLayer() === undefined) {
    pushLayer("content");
  }
}

export function showMobileToolbox(): void {
  if (!get(isMobile)) {
    return;
  }
  mobileView.set("toolbox");
  // 已在工具箱层时不重复压栈（硬件返回键经 popstate 回列表）。
  if (currentLayer() !== "toolbox") {
    pushLayer("toolbox");
  }
}

export function showMobileList(): void {
  if (get(isMobile) && typeof history !== "undefined") {
    const layer = currentLayer();
    if (layer === "content" || layer === "toolbox") {
      // Let popstate drive the state change so browser history stays in sync.
      history.back();
      return;
    }
  }
  mobileView.set("list");
}

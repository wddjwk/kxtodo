import { get, writable } from "svelte/store";
import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { diaryEditor, editorDraftNode, editorTaskId, searchQuery, showSettings } from "./stores";

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

/**
 * 纯触屏设备（hover:none）。触屏补偿交互（点标签露出删除叉之类）只在这里启用，
 * 桌面（含触屏笔记本，hover 仍为 true）继续走 hover 语义。
 */
export const touchOnly: boolean =
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(hover: none)").matches;

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
export type MobileView = "list" | "content" | "toolbox" | "diary";

export const mobileView = writable<MobileView>("list");

type MobileLayer = "content" | "settings" | "editor" | "toolbox" | "diary" | "diary-editor";

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
      editorDraftNode.set(null);
      diaryEditor.set(null);
      break;
    case "toolbox":
      mobileView.set("toolbox");
      showSettings.set(false);
      editorTaskId.set(null);
      editorDraftNode.set(null);
      diaryEditor.set(null);
      break;
    case "diary":
      // 也是「日记编辑器被返回键关掉」时落到的那一层
      mobileView.set("diary");
      showSettings.set(false);
      editorTaskId.set(null);
      editorDraftNode.set(null);
      diaryEditor.set(null);
      break;
    case "diary-editor":
      // 编辑器仍在顶层，由 diaryEditor 订阅驱动，这里不回写 store
      break;
    case "settings":
      // 设置页覆盖在列表之上：底层固定回列表视图
      mobileView.set("list");
      showSettings.set(true);
      editorTaskId.set(null);
      editorDraftNode.set(null);
      diaryEditor.set(null);
      break;
    case "editor":
      // 编辑器仍在顶层，由 editorTaskId 订阅驱动，这里不回写 store
      break;
    default:
      // 回到基础层（列表）
      mobileView.set("list");
      showSettings.set(false);
      editorTaskId.set(null);
      editorDraftNode.set(null);
      diaryEditor.set(null);
      break;
  }
  releaseGuardLater();
}

/** 编辑器层的压栈/回退：任务编辑与新建草稿共用同一层（两者互斥）。 */
function syncEditorLayer(open: boolean): void {
  if (!get(isMobile) || applyingHistory) return;
  if (open) {
    if (currentLayer() !== "editor") pushLayer("editor");
  } else if (currentLayer() === "editor") {
    history.back();
  }
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
  // 「新建事项」模式（editorDraftNode）共用同一层：两者互斥，不会同时开着。
  editorTaskId.subscribe((id) => syncEditorLayer(id !== null));
  editorDraftNode.subscribe((nodeId) => syncEditorLayer(nodeId !== null));

  // 日记编辑器同一套路（与任务编辑器互斥，不会同时开着）
  diaryEditor.subscribe((target) => {
    if (!get(isMobile) || applyingHistory) return;
    if (target !== null) {
      if (currentLayer() !== "diary-editor") pushLayer("diary-editor");
    } else if (currentLayer() === "diary-editor") {
      history.back();
    }
  });

  // 搜索态不压历史栈（pushState 条目进 WebView 会话恢复后，「搜索过再退出、
  // 重开必闪退一次」就是从那来的）。返回键改由 MainActivity 问这条桥：
  // 搜索态消费掉（清词），其余返回 false 交给 WebView 历史 / finish。
  registerBackHandler();
}

/**
 * 返回键拦截器：覆盖层（全屏图预览这类不占历史栈的浮层）注册一个回调，
 * 返回 true = 消费掉这记返回。后注册的先问（栈语义）。返回注销函数。
 */
type BackInterceptor = () => boolean;
const backInterceptors: BackInterceptor[] = [];

export function addBackInterceptor(interceptor: BackInterceptor): () => void {
  backInterceptors.push(interceptor);
  return () => {
    const at = backInterceptors.indexOf(interceptor);
    if (at >= 0) backInterceptors.splice(at, 1);
  };
}

/**
 * 安卓硬件返回键的前端消费口。MainActivity 的 OnBackPressedCallback 会
 * evaluateJavascript 调它：返回 true = 这一记返回被覆盖层吃掉，不再 goBack/finish。
 * 桌面/浏览器没有这条回调链，注册了也无害。
 */
function registerBackHandler(): void {
  const w = window as Window & { kxtodoBackHandler?: () => boolean };
  w.kxtodoBackHandler = () => {
    if (!get(isMobile)) return false;
    for (let i = backInterceptors.length - 1; i >= 0; i--) {
      if (backInterceptors[i]()) return true;
    }
    if (get(searchQuery).trim()) {
      searchQuery.set("");
      return true;
    }
    return false;
  };
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

export function showMobileDiary(): void {
  if (!get(isMobile)) {
    return;
  }
  mobileView.set("diary");
  // 已在日记层时不重复压栈（硬件返回键经 popstate 回列表）。
  if (currentLayer() !== "diary") {
    pushLayer("diary");
  }
}

export function showMobileList(): void {
  if (get(isMobile) && typeof history !== "undefined") {
    const layer = currentLayer();
    if (layer === "content" || layer === "toolbox" || layer === "diary") {
      // Let popstate drive the state change so browser history stays in sync.
      history.back();
      return;
    }
  }
  mobileView.set("list");
}

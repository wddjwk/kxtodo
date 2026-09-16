import { get, writable } from "svelte/store";
import { onDestroy } from "svelte";
import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { diaryEditor, editorDraftNode, editorTaskId, ledgerEditor, searchQuery, showSettings, taskEmojiPicker } from "./stores";

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
export type MobileView = "list" | "content" | "toolbox" | "diary" | "ledger";

export const mobileView = writable<MobileView>("list");

type MobileLayer =
  | "content"
  | "settings"
  | "editor"
  | "toolbox"
  | "diary"
  | "diary-editor"
  | "ledger"
  | "ledger-editor";

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
      ledgerEditor.set(null);
      break;
    case "diary-editor":
      // 编辑器仍在顶层，由 diaryEditor 订阅驱动，这里不回写 store
      break;
    case "ledger":
      // 也是「记账面板被返回键关掉」时落到的那一层
      mobileView.set("ledger");
      showSettings.set(false);
      editorTaskId.set(null);
      editorDraftNode.set(null);
      diaryEditor.set(null);
      ledgerEditor.set(null);
      break;
    case "ledger-editor":
      // 面板仍在顶层，由 ledgerEditor 订阅驱动，这里不回写 store
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
      ledgerEditor.set(null);
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

  // 记账面板同一套路（与日记编辑器互斥，不会同时开着）
  ledgerEditor.subscribe((target) => {
    if (!get(isMobile) || applyingHistory) return;
    if (target !== null) {
      if (currentLayer() !== "ledger-editor") pushLayer("ledger-editor");
    } else if (currentLayer() === "ledger-editor") {
      history.back();
    }
  });

  // 表情/图标选择器不占历史栈，组件挂载后有自己的 guard——但它是**懒加载**的：
  // chunk 在途的窗口里（首开、冷缓存）组件还没挂载，返回键这一下会把底下的页面
  // 弹掉。这里按 store 兜底：开着就接管返回键直接关（组件挂载后它的 guard 注册得
  // 更晚、先被问到，两层不冲突；store 清空时这层自动注销）。
  let pickerGuardRelease: (() => void) | null = null;
  taskEmojiPicker.subscribe((target) => {
    if (!get(isMobile)) return;
    if (target !== null) {
      if (!pickerGuardRelease) {
        pickerGuardRelease = addBackInterceptor(() => {
          if (get(taskEmojiPicker) === null) return false;
          taskEmojiPicker.set(null);
          return true;
        });
      }
    } else if (pickerGuardRelease) {
      pickerGuardRelease();
      pickerGuardRelease = null;
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
 * 「浮层开着就接管系统返回键」的现成写法，给组件里的 `$:` 用：
 *
 * ```svelte
 * const guard = createBackGuard();
 * $: guard(showPicker, () => (showPicker = false));
 * ```
 *
 * 为什么要有它：`addBackInterceptor` 得配对地注销，直接在组件里手写很容易漏掉
 * 「关掉之后没注销」——那样返回键会一直喂给一个已经不可见的浮层，按下去什么也不发生
 *（用户眼里就是「返回键失灵了」）。这里按 `active` 的翻转注册/注销，回调只更新不重排，
 * 栈位置稳定；**只在移动端注册**，桌面没有系统返回键，也免得平白占一层。
 */
export type BackGuard = ((active: boolean, onBack: () => void) => void) & {
  /**
   * 立刻注销并复位。**「挂着就等于开着」的组件（`{#if}` 里挂载的菜单 / 选择器）
   * 必须在 `onDestroy` 里调它**：那种组件从头到尾只会用 `true` 调一次，
   * 卸载时不会自己走 `active=false` 那条分支——不注销的话拦截器永远留在栈里，
   * 返回键被一个已经不可见的浮层吃掉，用户眼里就是「返回键失灵」。
   */
  dispose: () => void;
};

export function createBackGuard(): BackGuard {
  let release: (() => void) | null = null;
  let wasActive = false;
  let current: () => void = () => undefined;
  let disposed = false;
  const sync = (active: boolean, onBack: () => void): void => {
    if (active === wasActive) {
      current = onBack;
      return;
    }
    wasActive = active;
    if (active) {
      current = onBack;
      if (get(isMobile)) {
        release = addBackInterceptor(() => {
          current();
          return true;
        });
      }
    } else if (release) {
      release();
      release = null;
    }
  };
  sync.dispose = (): void => {
    disposed = true;
    if (release) {
      release();
      release = null;
    }
    wasActive = false;
    current = () => undefined;
  };
  // **组件销毁时自动摘掉**，不需要每个调用点自己记得写 `onDestroy`。
  //
  // 早先只给了手动 dispose，靠自觉调用——而「挂着就等于开着」的浮层只要漏一处，
  // 拦截器就永久留在栈里、返回键被一个已经不可见的浮层吃掉（用户眼里=返回键失灵）。
  // 组件在 `{#if}` 里被拆掉时同样会走到这里，所以「开着浮层时整个页面被切走」这种情况
  // 也一并覆盖了。`onDestroy` 只能在组件初始化期间调，非组件环境（不存在）回退成手动。
  try {
    onDestroy(() => {
      if (!disposed) sync.dispose();
    });
  } catch {
    // 不在组件初始化上下文中：交给调用方自己 dispose
  }
  return sync;
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
    if (consumeBackInterceptors()) return true;
    if (get(searchQuery).trim()) {
      searchQuery.set("");
      return true;
    }
    return false;
  };
}

/** 后注册的先问（栈语义）；第一个返回 true 的消费掉这一记返回。 */
function consumeBackInterceptors(): boolean {
  for (let i = backInterceptors.length - 1; i >= 0; i--) {
    if (backInterceptors[i]()) return true;
  }
  return false;
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

export function showMobileLedger(): void {
  if (!get(isMobile)) {
    return;
  }
  mobileView.set("ledger");
  // 已在记账层时不重复压栈（硬件返回键经 popstate 回列表）。
  if (currentLayer() !== "ledger") {
    pushLayer("ledger");
  }
}

export function showMobileList(): void {
  if (get(isMobile) && typeof history !== "undefined") {
    const layer = currentLayer();
    if (layer === "content" || layer === "toolbox" || layer === "diary" || layer === "ledger") {
      // Let popstate drive the state change so browser history stays in sync.
      history.back();
      return;
    }
  }
  mobileView.set("list");
}

/**
 * 「返回上一级」的按钮口径（移动端页面左上角的返回箭头）：与安卓返回键**同一条链**——
 * 先问浮层拦截器（齿轮面板、菜单、选择器开着时先收它们），再退历史栈的一层，
 * 没有层（已经在列表上）就什么都不做。早先它绕过拦截器直接 history.back()：
 * 开着浮层点箭头，浮层底下的整页被弹掉、浮层反而留在原地。
 * Esc 那种"再点一次退出"的语义刻意不给——按钮在列表页根本不渲染。
 */
export function goBackLevel(): void {
  if (!get(isMobile) || typeof history === "undefined") return;
  if (consumeBackInterceptors()) return;
  if (currentLayer() !== undefined) {
    history.back();
    return;
  }
  mobileView.set("list");
}

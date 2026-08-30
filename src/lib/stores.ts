import { writable, derived, get } from "svelte/store";
import type { AppNotification, AppState, AppNode, EmojiPickerTarget, NotificationTone, SchedulerState, Settings, Task } from "./types";
import { defaultSchedulerRuntimes, defaultSettings, emptyState, normalizeState, normalizeSettings, schedulerRuntimeKeys } from "./defaults";
import {
  loadState, saveState, loadSettings, saveSettings, loadScheduler, saveScheduler,
  registerGlobalShortcut, setCloseToTray, setAutostart,
  setWebviewZoom, isTauriRuntime, resolveExecutorPaths, sendNativeNotification,
  hasCoreDispatch, coreSnapshot, getAppVersion
} from "./backend";
import { buildListCounts, buildVisibleTasks, getBackground } from "./nodes";
import { accentForNode, uiScaleValue } from "./styles";
import { entryToUi, type ScheduleEntryV9 } from "./scheduleAdapter";

/** 运行时版本号：构建期由 build.rs 从 git tag/commit 注入（KXTODO_VERSION），hydrate 时填充。 */
export const appVersion = writable("");

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function now(): string {
  return new Date().toISOString();
}

export function todayIso(): string {
  const date = new Date();
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 10);
}

export function yesterdayIso(): string {
  const date = new Date();
  date.setDate(date.getDate() - 1);
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 10);
}

export function dateOnly(value?: string): string | undefined {
  return value?.slice(0, 10) || undefined;
}

export function createTaskId(): string {
  if (crypto.randomUUID) return `task-${crypto.randomUUID().slice(0, 8)}`;
  return `task-${Math.random().toString(36).slice(2, 10)}`;
}

export function safeFileName(name: string): string {
  return name.replace(/[<>:"/\\|?*\u0000-\u001F]/g, "-").slice(0, 80) || "todo-note";
}

export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
    reader.readAsDataURL(file);
  });
}

// ---------------------------------------------------------------------------
// Core stores
// ---------------------------------------------------------------------------

export const appState = writable<AppState>(emptyState());
export const appSettings = writable<Settings>(clone(defaultSettings));
export const isHydrated = writable(false);
export const showSettings = writable(false);
export const searchQuery = writable("");
export const taskEmojiPicker = writable<EmojiPickerTarget | null>(null);
/** 正在浮窗编辑器中编辑的任务 ID（null = 编辑器关闭）。 */
export const editorTaskId = writable<string | null>(null);

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

export const toastMessage = writable("");
let toastTimer: number | undefined;

export function showToast(message: string, durationMs = 3200): void {
  toastMessage.set(message);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastMessage.set(""), durationMs);
}

export function showNotification(
  message: string,
  options: Partial<Omit<AppNotification, "message">> = {}
): Promise<void> {
  const settings = get(appSettings);
  const notification: AppNotification = {
    title: options.title?.trim() || "KXToDo",
    message: message.trim() || "通知",
    durationMs: Math.min(60_000, Math.max(1_200, Math.round(options.durationMs ?? settings.notifications.durationMs))),
    tone: (options.tone ?? "info") as NotificationTone,
    position: options.position ?? settings.notifications.position
  };

  if (!isTauriRuntime) {
    showToast(`${notification.title}：${notification.message}`, notification.durationMs);
    return Promise.resolve();
  }

  void sendNativeNotification(notification).catch((error) => {
    showToast(`通知发送失败：${String(error)}`);
  });
  return Promise.resolve();
}

// ---------------------------------------------------------------------------
// Derived stores
// ---------------------------------------------------------------------------

export const systemNodes = derived(appState, ($s) =>
  $s.nodes.filter((n) => n.kind === "system")
);

export const firstEntry = derived(appState, ($s) =>
  $s.nodes.find((n) => n.kind === "entry")
);

export const selectedNode = derived(
  [appState, firstEntry, systemNodes],
  ([$s, $fe, $sn]) =>
    $s.nodes.find((n) => n.id === $s.selectedNodeId) ?? $fe ?? $sn[0]
);

export const listCounts = derived(appState, ($s) => buildListCounts($s));

export const visibleTasks = derived(
  [appState, selectedNode, searchQuery],
  ([$s, $node, $q]) => buildVisibleTasks($s, $node, $q)
);

export const selectedBackground = derived(
  [selectedNode, appState],
  ([$node, $s]) => getBackground($node?.id, $s.backgrounds)
);

export const accent = derived(
  [selectedNode, appSettings],
  ([$node, $settings]) => accentForNode($node, $settings.appearance.uiColors)
);

export const isSearching = derived(searchQuery, ($q) => $q.trim().length > 0);

// ---------------------------------------------------------------------------
// Legacy persistence（mobile / 浏览器预览路径；桌面 v9 走 actions.ts 命令化写入）
// ---------------------------------------------------------------------------

let stateSaveTimer: number | undefined;
let settingsSaveTimer: number | undefined;
let schedulerSaveTimer: number | undefined;

export function commit(next: AppState): void {
  appState.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(stateSaveTimer);
  stateSaveTimer = window.setTimeout(() => {
    saveState(next).catch((error) => showToast(`保存失败：${String(error)}`));
  }, 180);
}

export function commitScheduler(next: SchedulerState): void {
  appState.update((state) => ({ ...state, scheduler: next }));
  if (!get(isHydrated)) return;
  window.clearTimeout(schedulerSaveTimer);
  schedulerSaveTimer = window.setTimeout(() => {
    saveScheduler(next).catch((error) => showToast(`保存定时任务失败：${String(error)}`));
  }, 180);
}

export function commitSettings(next: Settings): void {
  const previousScale = uiScaleValue(get(appSettings).appearance.uiScale);
  const nextScale = uiScaleValue(next.appearance.uiScale);
  appSettings.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(settingsSaveTimer);
  settingsSaveTimer = window.setTimeout(() => {
    saveSettings(next).catch((error) => showToast(`保存设置失败：${String(error)}`));
  }, 180);
  if (previousScale !== nextScale) {
    void syncNativeAppearance(next);
  }
}

// ---------------------------------------------------------------------------
// Native sync
// ---------------------------------------------------------------------------

async function syncNativeAppearance(nextSettings: Settings): Promise<void> {
  if (!isTauriRuntime) return;
  try {
    await setWebviewZoom(uiScaleValue(nextSettings.appearance.uiScale));
  } catch (error) {
    showToast(`界面缩放同步失败：${String(error)}`);
  }
}

async function syncNativeLifecycle(nextSettings: Settings): Promise<void> {
  try {
    await setCloseToTray(nextSettings.lifecycle.closeToTray);
  } catch (error) {
    showToast(`系统设置同步失败：${String(error)}`);
  }
  try {
    await setAutostart(nextSettings.lifecycle.launchAtStartup);
  } catch (error) {
    if (nextSettings.lifecycle.launchAtStartup) {
      showToast(`开机自启设置失败：${String(error)}`);
    }
  }
}

// ---------------------------------------------------------------------------
// v9 core mode：快照刷新 + 领域事件 + 编辑冲突策略（§4.3）
// ---------------------------------------------------------------------------

/** v9 ScheduleEntry → 侧表（GUI 编辑器据此构建 patch，保留 CLI 专属字段）。 */
export const scheduleEntries = writable<Map<string, ScheduleEntryV9>>(new Map());

/** 编辑基准：itemId → 开始编辑时的 updatedAt（用于保存冲突检测）。 */
const editBases = new Map<string, string | undefined>();

export function markEditStart(task: Task): void {
  editBases.set(task.id, task.updatedAt);
}

export function clearEditBase(taskId: string): void {
  editBases.delete(taskId);
}

export function rebaseEditBase(taskId: string, updatedAt?: string): void {
  if (editBases.has(taskId)) {
    editBases.set(taskId, updatedAt);
  }
}

export function editBaseUpdatedAt(taskId: string): string | undefined {
  return editBases.get(taskId);
}

export let coreMode = false;

function applySnapshot(snapshot: Awaited<ReturnType<typeof coreSnapshot>>, domains?: Set<string>): void {
  const wantAll = !domains;
  if (wantAll || domains?.has("data")) {
    const current = get(appState);
    const normalized = normalizeState({
      ...snapshot.data,
      scheduler: current.scheduler
    });
    appState.set({ ...normalized, scheduler: current.scheduler });
  }
  if (wantAll || domains?.has("settings")) {
    appSettings.set(normalizeSettings(snapshot.settings));
  }
  if (wantAll || domains?.has("schedule")) {
    const current = get(appState);
    const entries = snapshot.schedule.tasks as ScheduleEntryV9[];
    scheduleEntries.set(new Map(entries.map((entry) => [entry.id, entry])));
    const runtimes = schedulerRuntimeKeys.reduce((acc, key) => {
      acc[key] = snapshot.schedule.runtimes?.[key] || "";
      return acc;
    }, { ...defaultSchedulerRuntimes });
    appState.set({
      ...get(appState),
      scheduler: {
        runtimes,
        tasks: entries.map(entryToUi)
      }
    });
  }
}

let snapshotInFlight = false;
let snapshotPending = false;
let pendingAllDomains = false;
const pendingDomains = new Set<string>();

function queueSnapshot(domains?: string[]): void {
  snapshotPending = true;
  if (!domains) {
    pendingAllDomains = true;
    pendingDomains.clear();
    return;
  }
  if (!pendingAllDomains) {
    domains.forEach((domain) => pendingDomains.add(domain));
  }
}

/** 从 Host 拉取最新快照；in-flight 期间的事件合并后立即再拉取。 */
export async function refreshFromCore(domains?: string[]): Promise<void> {
  if (!coreMode) return;
  queueSnapshot(domains);
  if (snapshotInFlight) return;
  snapshotInFlight = true;
  try {
    while (snapshotPending) {
      const all = pendingAllDomains;
      const requested = all ? undefined : new Set(pendingDomains);
      snapshotPending = false;
      pendingAllDomains = false;
      pendingDomains.clear();
      try {
        const snapshot = await coreSnapshot();
        applySnapshot(snapshot, requested);
      } catch {
        // 保留一次 pending；后续领域事件会重新触发，不丢失并发事件。
        snapshotPending = true;
        break;
      }
    }
  } finally {
    snapshotInFlight = false;
  }
}

async function listenCoreEvents(): Promise<void> {
  const { listen } = await import("@tauri-apps/api/event");
  await listen<{ domain?: string }>("kxtodo://domain-changed", (event) => {
    const domain = event.payload?.domain;
    void refreshFromCore(domain ? [domain] : undefined);
  });
}

// ---------------------------------------------------------------------------
// Hydration
// ---------------------------------------------------------------------------

export async function hydrate(): Promise<void> {
  try {
    appVersion.set(await getAppVersion());
  } catch {
    // 版本号缺失不阻塞启动
  }
  try {
    coreMode = await hasCoreDispatch();
  } catch (error) {
    // Desktop capability failures fail closed: never enter legacy full-save mode.
    coreMode = true;
    isHydrated.set(true);
    showToast(`Domain Core 初始化失败，已禁止写入以保护数据：${String(error)}`, 10_000);
    return;
  }
  if (coreMode) {
    await listenCoreEvents();
    let snapshotLoaded = false;
    try {
      const snapshot = await coreSnapshot();
      applySnapshot(snapshot);
      snapshotLoaded = true;
    } catch (error) {
      showToast(`数据加载失败，已禁止 legacy 覆盖；请运行 kxtodo doctor：${String(error)}`, 10_000);
    } finally {
      isHydrated.set(true);
    }
    if (!snapshotLoaded) return;
    const settings = get(appSettings);
    await syncNativeAppearance(settings);
    try {
      await registerGlobalShortcut(settings.shortcuts.toggleWindow);
    } catch (error) {
      showToast(`全局快捷键注册失败：${String(error)}`);
    }
    await syncNativeLifecycle(settings);
    return;
  }

  // Legacy 路径（mobile / 浏览器预览）
  let loadedSettings = clone(defaultSettings);
  try {
    const [storedState, storedScheduler, storedSettings, resolvedExecutors] = await Promise.all([
      loadState(),
      loadScheduler(),
      loadSettings(),
      resolveExecutorPaths().catch(() => defaultSchedulerRuntimes)
    ]);
    const scheduler: SchedulerState = {
      ...storedScheduler,
      runtimes: schedulerRuntimeKeys.reduce((acc, key) => {
        acc[key] = storedScheduler.runtimes[key] || resolvedExecutors[key] || "";
        return acc;
      }, { ...storedScheduler.runtimes })
    };
    appState.set({
      ...normalizeState(storedState),
      scheduler
    });
    const normalizedSettings = normalizeSettings(storedSettings);
    appSettings.set(normalizedSettings);
    loadedSettings = normalizedSettings;
  } catch {
    // First launch or corrupted data — silently use defaults
  } finally {
    isHydrated.set(true);
  }

  await syncNativeAppearance(loadedSettings);

  try {
    await registerGlobalShortcut(loadedSettings.shortcuts.toggleWindow);
  } catch (error) {
    showToast(`全局快捷键注册失败：${String(error)}`);
  }

  await syncNativeLifecycle(loadedSettings);
}

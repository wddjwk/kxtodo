import { writable, derived, get } from "svelte/store";
import type { AppNotification, AppState, AppNode, NotificationTone, SchedulerState, Settings } from "./types";
import { defaultSchedulerRuntimes, defaultSettings, emptyState, normalizeState, normalizeSettings, schedulerRuntimeKeys } from "./defaults";
import {
  loadState, saveState, loadSettings, saveSettings, loadScheduler, saveScheduler,
  registerGlobalShortcut, setCloseToTray, setAutostart,
  setWebviewZoom, isTauriRuntime, resolveExecutorPaths, sendNativeNotification
} from "./backend";
import { buildListCounts, buildVisibleTasks, getBackground } from "./nodes";
import { accentForNode, uiScaleValue } from "./styles";

export const APP_VERSION = "8.2.0";

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
// Persistence
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
// Hydration
// ---------------------------------------------------------------------------

export async function hydrate(): Promise<void> {
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











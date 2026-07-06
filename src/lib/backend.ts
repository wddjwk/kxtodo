import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { defaultSettings, emptySchedulerState, emptyState, normalizeSchedulerState, normalizeSettings, normalizeState } from "./defaults";
import type { AppNotification, AppState, ScheduledTaskAction, SchedulerRuntimePaths, SchedulerState, Settings } from "./types";

const stateKey = "todo-note-state-v3";
const settingsKey = "todo-note-settings-v3";
const schedulerKey = "todo-note-scheduler-v8";

export const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function readLocal<T>(key: string, fallback: T): T {
  const raw = localStorage.getItem(key);
  if (!raw) {
    return fallback;
  }
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export async function loadState(): Promise<AppState> {
  if (isTauriRuntime) {
    return normalizeState(await invoke<unknown>("load_state"));
  }
  return normalizeState(readLocal(stateKey, emptyState()));
}

export async function saveState(state: AppState): Promise<void> {
  const { scheduler: _scheduler, ...persistedState } = state;
  if (isTauriRuntime) {
    await invoke("save_state", { state: persistedState });
    return;
  }
  localStorage.setItem(stateKey, JSON.stringify(persistedState));
}

export async function loadSettings(): Promise<Settings> {
  if (isTauriRuntime) {
    return normalizeSettings(await invoke<unknown>("load_settings"));
  }
  return normalizeSettings(readLocal(settingsKey, clone(defaultSettings)));
}

export async function saveSettings(settings: Settings): Promise<void> {
  if (isTauriRuntime) {
    await invoke("save_settings", { settings });
    return;
  }
  localStorage.setItem(settingsKey, JSON.stringify(settings));
}

export async function loadScheduler(): Promise<SchedulerState> {
  if (isTauriRuntime) {
    return normalizeSchedulerState(await invoke<unknown>("load_scheduler"));
  }
  return normalizeSchedulerState(readLocal(schedulerKey, emptySchedulerState()));
}

export async function saveScheduler(scheduler: SchedulerState): Promise<void> {
  if (isTauriRuntime) {
    await invoke("save_scheduler", { scheduler });
    return;
  }
  localStorage.setItem(schedulerKey, JSON.stringify(scheduler));
}

export type ScheduledActionOutput = {
  exitCode: number | null;
  stdout: string;
  stderr: string;
};

export async function resolveExecutorPaths(): Promise<SchedulerRuntimePaths> {
  const empty = emptySchedulerState().runtimes;
  if (!isTauriRuntime) {
    return empty;
  }
  return { ...empty, ...(await invoke<Partial<SchedulerRuntimePaths>>("resolve_executor_paths")) };
}

export async function runScheduledAction(action: ScheduledTaskAction, runtimes: SchedulerRuntimePaths, taskId?: string): Promise<ScheduledActionOutput> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览模式不支持执行定时任务");
  }
  return invoke<ScheduledActionOutput>("run_scheduled_action", { action, runtimes, taskId });
}

export async function stopScheduledAction(taskId: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("stop_scheduled_action", { taskId });
}

export async function sendNativeNotification(notification: AppNotification): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("send_notification", { notification });
}

export async function exportData(payload: unknown, defaultName: string): Promise<void> {
  if (isTauriRuntime) {
    const filePath = await save({
      defaultPath: defaultName,
      filters: [{ name: "KXToDo JSON", extensions: ["json"] }]
    });
    if (!filePath) {
      return;
    }
    await invoke("export_data", { payload, path: filePath });
    return;
  }

  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = defaultName;
  link.click();
  URL.revokeObjectURL(url);
}

export async function saveBackgroundImage(dataUrl: string): Promise<string> {
  if (isTauriRuntime) {
    return invoke<string>("save_background_image", { dataUrl });
  }
  return dataUrl;
}

export async function loadBackgroundImage(filename: string): Promise<string> {
  if (!isTauriRuntime) {
    return filename;
  }
  return invoke<string>("load_background_image", { filename });
}

export async function deleteBackgroundImage(filename: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("delete_background_image", { filename });
}

/** Open a native file picker for an image; returns the chosen path or null. */
export async function pickImageFile(): Promise<string | null> {
  if (!isTauriRuntime) {
    return null;
  }
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"] }]
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickExecutableFile(): Promise<string | null> {
  if (!isTauriRuntime) {
    return null;
  }
  const selected = await open({
    multiple: false,
    directory: false
  });
  return typeof selected === "string" ? selected : null;
}

export async function resolveExecutablePath(name: string): Promise<string | null> {
  if (!isTauriRuntime || !name.trim()) {
    return null;
  }
  return invoke<string | null>("resolve_executable_path", { name });
}

/** Copy a picked image into the data dir (no base64) and return its filename. */
export async function importBackgroundImage(srcPath: string): Promise<string> {
  return invoke<string>("import_background_image", { srcPath });
}

/** Resolve a stored background image filename to a webview-displayable URL (asset protocol, no base64). */
export async function backgroundImageUrl(filename: string): Promise<string> {
  const path = await invoke<string>("background_image_path", { filename });
  return convertFileSrc(path);
}

/** Copy a picked file into the avatar directory. Returns stored filename. */
export async function saveAvatarImage(srcPath: string): Promise<string> {
  return invoke<string>("save_avatar_image", { srcPath });
}

/** Delete the avatar image file. */
export async function deleteAvatarImage(filename: string): Promise<void> {
  if (!isTauriRuntime) return;
  await invoke("delete_avatar_image", { filename });
}

/** Resolve avatar filename to asset URL. */
export async function avatarImageUrl(filename: string): Promise<string> {
  const path = await invoke<string>("avatar_image_path", { filename });
  return convertFileSrc(path);
}

/** Copy a picked file into img/<nodeId>/ for markdown. Returns stored filename. */
export async function saveMdImage(srcPath: string, nodeId: string): Promise<string> {
  return invoke<string>("save_md_image", { srcPath, nodeId });
}

/** Delete a single markdown image file. */
export async function deleteMdImage(nodeId: string, filename: string): Promise<void> {
  if (!isTauriRuntime) return;
  await invoke("delete_md_image", { nodeId, filename });
}

/** Delete all markdown images for a node. */
export async function deleteNodeImages(nodeId: string): Promise<void> {
  if (!isTauriRuntime) return;
  await invoke("delete_node_images", { nodeId });
}

/** Resolve markdown image filename to asset URL. */
export async function mdImageUrl(nodeId: string, filename: string): Promise<string> {
  const path = await invoke<string>("md_image_path", { nodeId, filename });
  return convertFileSrc(path);
}

/** Save a base64 data URL as a markdown image (for clipboard paste). Returns stored filename. */
export async function saveMdImageFromDataUrl(dataUrl: string, nodeId: string): Promise<string> {
  return invoke<string>("save_md_image_data", { dataUrl, nodeId });
}

export async function registerGlobalShortcut(shortcut: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("register_global_shortcut", { shortcut });
}

export async function setCloseToTray(enabled: boolean): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("set_close_to_tray", { enabled });
}

export async function setAutostart(enabled: boolean): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("set_autostart", { enabled });
}

export async function getAutostartEnabled(): Promise<boolean> {
  if (!isTauriRuntime) {
    return false;
  }
  return invoke<boolean>("get_autostart_enabled");
}

export async function setWebviewZoom(scale: number): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("set_webview_zoom", { scale });
}

export async function openExternalUrl(rawUrl: string): Promise<void> {
  const url = new URL(rawUrl, window.location.href);
  if (!["http:", "https:", "mailto:"].includes(url.protocol)) {
    throw new Error(`Unsupported link protocol: ${url.protocol}`);
  }

  if (isTauriRuntime) {
    await invoke("open_url", { url: url.href });
    return;
  }

  window.open(url.href, "_blank", "noopener,noreferrer");
}

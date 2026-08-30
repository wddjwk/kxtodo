import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { defaultSettings, emptySchedulerState, emptyState, normalizeSchedulerState, normalizeSettings, normalizeState } from "./defaults";
import type { AppNotification, AppState, SchedulerRuntimePaths, SchedulerState, Settings } from "./types";

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

export async function resolveExecutorPaths(): Promise<SchedulerRuntimePaths> {
  const empty = emptySchedulerState().runtimes;
  if (!isTauriRuntime) {
    return empty;
  }
  return { ...empty, ...(await invoke<Partial<SchedulerRuntimePaths>>("resolve_executor_paths")) };
}

export async function sendNativeNotification(notification: AppNotification): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("send_notification", { notification });
}

/** 主窗口创建时隐藏（避免黑边），前端挂载后调用显示。 */
export async function revealMainWindow(): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("reveal_main_window");
}

/** 构建期由 build.rs 从 git tag/commit 注入的版本号。 */
export async function getAppVersion(): Promise<string> {
  if (!isTauriRuntime) {
    return "dev";
  }
  return invoke<string>("app_version");
}

/** 增量写入更新包（base64 分块），返回临时文件路径。 */
export async function writeUpdatePackage(chunk: string, append: boolean): Promise<string> {
  return invoke<string>("write_update_package", { chunk, append });
}

/** 启动更新脚本并退出当前进程（脚本等待退出后替换 exe 并重启）。 */
export async function applyUpdateAndRestart(): Promise<void> {
  await invoke("apply_update_and_restart");
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

/** Resolve avatar filename to asset URL. */
export async function avatarImageUrl(filename: string): Promise<string> {
  const path = await invoke<string>("avatar_image_path", { filename });
  return convertFileSrc(path);
}

/** Copy a picked file into img/<nodeId>/ for markdown. Returns stored filename. */
export async function saveMdImage(srcPath: string, nodeId: string): Promise<string> {
  return invoke<string>("save_md_image", { srcPath, nodeId });
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

// ---------------------------------------------------------------------------
// v9 Domain Core bridge (desktop). Mobile/browser fall back to legacy paths.
// ---------------------------------------------------------------------------

let coreAvailable: boolean | null = null;

export async function hasCoreDispatch(): Promise<boolean> {
  if (!isTauriRuntime) {
    return false;
  }
  if (coreAvailable !== null) {
    return coreAvailable;
  }
  try {
    const capability = await invoke<{ available: boolean }>("core_ping");
    coreAvailable = capability.available;
  } catch (error) {
    // Tauri desktop must fail closed: an unavailable probe is not permission
    // to fall back to full-file legacy writes.
    const mobile = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
    if (!mobile) {
      throw error;
    }
    coreAvailable = false;
  }
  return coreAvailable;
}

export type CoreEnvelope<T = unknown> = {
  ok: boolean;
  command: string;
  data: T;
  meta: Record<string, unknown>;
  error?: { code: string; message: string; hint?: string };
};

export class CoreCommandError extends Error {
  code: string;
  hint?: string;
  constructor(code: string, message: string, hint?: string) {
    super(message);
    this.code = code;
    this.hint = hint;
  }
}

/** Invoke a Domain Core command; rejects with CoreCommandError on failure. */
export async function coreDispatch<T = unknown>(command: string, params: unknown = {}): Promise<CoreEnvelope<T>> {
  try {
    return await invoke<CoreEnvelope<T>>("core_dispatch", { command, params });
  } catch (error) {
    // core_dispatch returns the error envelope as a serialized string on failure.
    if (typeof error === "string") {
      try {
        const envelope = JSON.parse(error) as CoreEnvelope;
        if (envelope.error) {
          throw new CoreCommandError(envelope.error.code, envelope.error.message, envelope.error.hint);
        }
      } catch (parseError) {
        if (parseError instanceof CoreCommandError) {
          throw parseError;
        }
      }
      throw new CoreCommandError("CORE_ERROR", error);
    }
    throw new CoreCommandError("CORE_ERROR", String(error));
  }
}

export type CoreSnapshot = {
  data: {
    nodes: AppState["nodes"];
    tasks: AppState["tasks"];
    backgrounds: AppState["backgrounds"];
    selectedNodeId: string;
  };
  settings: unknown;
  schedule: {
    runtimes: SchedulerRuntimePaths;
    tasks: unknown[];
  };
  revisions: { data: number; settings: number; schedule: number };
};

export async function coreSnapshot(): Promise<CoreSnapshot> {
  return invoke<CoreSnapshot>("core_snapshot");
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

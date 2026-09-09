import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { caps } from "./capabilities";
import { defaultSettings, emptySchedulerState, emptyState, normalizeDiaryEntries, normalizeSchedulerState, normalizeSettings, normalizeState } from "./defaults";
import type { AppNotification, AppState, DiaryEntry, LedgerBook, SchedulerRuntimePaths, SchedulerState, Settings } from "./types";

const stateKey = "todo-note-state-v3";
const settingsKey = "todo-note-settings-v3";
const schedulerKey = "todo-note-scheduler-v8";
const diaryKey = "todo-note-diary-v1";
const ledgerKey = "todo-note-ledger-v1";

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

// 浏览器预览（npm run dev）专用的 localStorage 持久化。
// Tauri 平台（桌面 + 移动端）一律走 Domain Core 命令层，绝不再整文件读写。

export async function loadState(): Promise<AppState> {
  return normalizeState(readLocal(stateKey, emptyState()));
}

export async function saveState(state: AppState): Promise<void> {
  const { scheduler: _scheduler, ...persistedState } = state;
  localStorage.setItem(stateKey, JSON.stringify(persistedState));
}

export async function loadSettings(): Promise<Settings> {
  return normalizeSettings(readLocal(settingsKey, clone(defaultSettings)));
}

export async function saveSettings(settings: Settings): Promise<void> {
  localStorage.setItem(settingsKey, JSON.stringify(settings));
}

export async function loadScheduler(): Promise<SchedulerState> {
  return normalizeSchedulerState(readLocal(schedulerKey, emptySchedulerState()));
}

export async function saveScheduler(scheduler: SchedulerState): Promise<void> {
  localStorage.setItem(schedulerKey, JSON.stringify(scheduler));
}

export async function loadDiary(): Promise<DiaryEntry[]> {
  return normalizeDiaryEntries(readLocal(diaryKey, { entries: [] }));
}

export async function saveDiary(entries: DiaryEntry[]): Promise<void> {
  localStorage.setItem(diaryKey, JSON.stringify({ entries }));
}

export async function loadLedger(): Promise<unknown> {
  return readLocal(ledgerKey, { accounts: [], categories: [], entries: [] });
}

export async function saveLedger(book: LedgerBook): Promise<void> {
  localStorage.setItem(ledgerKey, JSON.stringify(book));
}

export async function resolveExecutorPaths(): Promise<SchedulerRuntimePaths> {
  const empty = emptySchedulerState().runtimes;
  if (!isTauriRuntime || !caps.desktop) {
    return empty;
  }
  return { ...empty, ...(await invoke<Partial<SchedulerRuntimePaths>>("resolve_executor_paths")) };
}

export async function sendNativeNotification(notification: AppNotification): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
    return;
  }
  await invoke("send_notification", { notification });
}

/** 主窗口创建时隐藏（避免黑边），前端挂载后调用显示。仅桌面。 */
export async function revealMainWindow(): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
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

export async function exportData(payload: unknown, defaultName: string): Promise<void> {
  if (isTauriRuntime && caps.nativeFileDialogs) {
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

  if (isTauriRuntime) {
    // 移动端：dialog save() 返回 content:// URI，Rust fs 无法写入；
    // 走 Kotlin 分享桥（FileProvider + ACTION_SEND 分享面板）。
    const bridge = window.kxtodoAndroid;
    if (!bridge?.shareText) {
      throw new Error("当前 APK 不支持导出");
    }
    const err = bridge.shareText(defaultName, "application/json", JSON.stringify(payload, null, 2));
    if (err) {
      throw new Error(err);
    }
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

export type DiaryArchiveResult = { imported?: number; skipped?: number; images?: number; entries?: number; cards?: number; path?: string; name?: string };

/** 压缩包类命令（日记/卡片/记账）返回的是 core envelope，失败时是序列化后的错误串——与 coreDispatch 同一套解包。 */
async function invokeArchiveEnvelope(command: string, args: Record<string, unknown>): Promise<DiaryArchiveResult> {
  try {
    const envelope = await invoke<CoreEnvelope<DiaryArchiveResult>>(command, args);
    return envelope.data;
  } catch (error) {
    if (typeof error === "string") {
      try {
        const envelope = JSON.parse(error) as CoreEnvelope;
        if (envelope.error) {
          throw new Error(envelope.error.message);
        }
      } catch (parseError) {
        if (parseError instanceof Error && parseError.message !== error) {
          throw parseError;
        }
      }
    }
    throw new Error(String(error));
  }
}

export type DiaryExportRange = { from?: string; to?: string };

/**
 * 导出日记压缩包，返回导出的篇数（0 = 用户取消）。
 * 桌面走「另存为」对话框；移动端 dialog 的 save() 只给 content:// URI（Rust 写不了），
 * 所以不传 path，让 Rust 落进应用缓存目录，再交给 Kotlin 分享桥拉起系统分享面板。
 */
export async function exportDiaryZip(range: DiaryExportRange): Promise<number> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导出日记压缩包");
  }
  const from = range.from ?? null;
  const to = range.to ?? null;
  if (caps.nativeFileDialogs) {
    const filePath = await save({
      defaultPath: "kxtodo-diary.zip",
      filters: [{ name: "日记压缩包", extensions: ["zip"] }]
    });
    if (!filePath) {
      return 0;
    }
    const result = await invokeArchiveEnvelope("diary_export_zip", { path: filePath, from, to });
    return result.entries ?? 0;
  }
  const result = await invokeArchiveEnvelope("diary_export_zip", { path: null, from, to });
  const bridge = window.kxtodoAndroid;
  if (!bridge?.shareFile) {
    throw new Error("当前 APK 不支持分享压缩包");
  }
  if (!result.path) {
    throw new Error("导出没有产出文件");
  }
  const error = bridge.shareFile(result.path, "application/zip");
  if (error) {
    throw new Error(error);
  }
  return result.entries ?? 0;
}

/** 桌面：原生「打开」对话框选一个 zip 导入。返回 null 表示用户取消。 */
export async function importDiaryZipFromDialog(): Promise<DiaryArchiveResult | null> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入日记压缩包");
  }
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "日记压缩包", extensions: ["zip"] }]
  });
  if (!picked || typeof picked !== "string") {
    return null;
  }
  return invokeArchiveEnvelope("diary_import_zip", { path: picked, base64: null });
}

/** 移动端：无原生对话框，用隐藏 file input 读字节后以 base64 交给 Rust。 */
export async function importDiaryZipFromFile(file: File): Promise<DiaryArchiveResult> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入日记压缩包");
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 32768) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 32768));
  }
  return invokeArchiveEnvelope("diary_import_zip", { path: null, base64: btoa(binary) });
}

/**
 * 导出记账 Excel 压缩包，返回导出的笔数（0 = 用户取消）。
 * 与日记压缩包同一条路径：桌面「另存为」，移动端落缓存目录再交分享桥。
 */
export async function exportLedgerZip(range: DiaryExportRange): Promise<number> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导出记账压缩包");
  }
  const from = range.from ?? null;
  const to = range.to ?? null;
  if (caps.nativeFileDialogs) {
    const filePath = await save({
      defaultPath: "kxtodo-ledger.zip",
      filters: [{ name: "记账压缩包", extensions: ["zip"] }]
    });
    if (!filePath) {
      return 0;
    }
    const result = await invokeArchiveEnvelope("ledger_export_zip", { path: filePath, from, to });
    return result.entries ?? 0;
  }
  const result = await invokeArchiveEnvelope("ledger_export_zip", { path: null, from, to });
  const bridge = window.kxtodoAndroid;
  if (!bridge?.shareFile) {
    throw new Error("当前 APK 不支持分享压缩包");
  }
  if (!result.path) {
    throw new Error("导出没有产出文件");
  }
  const error = bridge.shareFile(result.path, "application/zip");
  if (error) {
    throw new Error(error);
  }
  return result.entries ?? 0;
}

/** 桌面：原生「打开」对话框选一个 zip/xlsx 导入。返回 null 表示用户取消。 */
export async function importLedgerZipFromDialog(): Promise<DiaryArchiveResult | null> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入记账压缩包");
  }
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "记账压缩包", extensions: ["zip", "xlsx"] }]
  });
  if (!picked || typeof picked !== "string") {
    return null;
  }
  return invokeArchiveEnvelope("ledger_import_zip", { path: picked, base64: null });
}

/** 移动端：隐藏 file input 读字节后以 base64 交给 Rust（zip 与裸 xlsx 都收）。 */
export async function importLedgerZipFromFile(file: File): Promise<DiaryArchiveResult> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入记账压缩包");
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 32768) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 32768));
  }
  return invokeArchiveEnvelope("ledger_import_zip", { path: null, base64: btoa(binary) });
}

/**
 * 导出一般卡片条目为 Markdown 压缩包，返回导出的卡片数（0 = 用户取消）。
 * 与日记压缩包同一条路径：桌面「另存为」，移动端落缓存目录再交分享桥。
 */
export async function exportCardsZip(nodeId: string): Promise<number> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导出 Markdown 压缩包");
  }
  if (caps.nativeFileDialogs) {
    const filePath = await save({
      defaultPath: "kxtodo-cards.zip",
      filters: [{ name: "Markdown 压缩包", extensions: ["zip"] }]
    });
    if (!filePath) {
      return 0;
    }
    const result = await invokeArchiveEnvelope("cards_export_zip", { path: filePath, nodeId });
    return result.cards ?? 0;
  }
  const result = await invokeArchiveEnvelope("cards_export_zip", { path: null, nodeId });
  const bridge = window.kxtodoAndroid;
  if (!bridge?.shareFile) {
    throw new Error("当前 APK 不支持分享压缩包");
  }
  if (!result.path) {
    throw new Error("导出没有产出文件");
  }
  const error = bridge.shareFile(result.path, "application/zip");
  if (error) {
    throw new Error(error);
  }
  return result.cards ?? 0;
}

/** 桌面：原生「打开」对话框选一个 Markdown 压缩包导入。null = 用户取消。 */
export async function importCardsZipFromDialog(nodeId: string): Promise<DiaryArchiveResult | null> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入 Markdown 压缩包");
  }
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Markdown 压缩包", extensions: ["zip"] }]
  });
  if (!picked || typeof picked !== "string") {
    return null;
  }
  return invokeArchiveEnvelope("cards_import_zip", { path: picked, base64: null, nodeId });
}

/** 移动端：隐藏 file input 读字节后以 base64 交给 Rust。 */
export async function importCardsZipFromFile(nodeId: string, file: File): Promise<DiaryArchiveResult> {
  if (!isTauriRuntime) {
    throw new Error("浏览器预览不支持导入 Markdown 压缩包");
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 32768) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 32768));
  }
  return invokeArchiveEnvelope("cards_import_zip", { path: null, base64: btoa(binary), nodeId });
}

export async function deleteBackgroundImage(filename: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("delete_background_image", { filename });
}

/** Open a native file picker for an image; returns the chosen path or null. 移动端无原生对话框，返回 null。 */
export async function pickImageFile(): Promise<string | null> {
  if (!isTauriRuntime || !caps.nativeFileDialogs) {
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
  if (!isTauriRuntime || !caps.nativeFileDialogs) {
    return null;
  }
  const selected = await open({
    multiple: false,
    directory: false
  });
  return typeof selected === "string" ? selected : null;
}

export async function resolveExecutablePath(name: string): Promise<string | null> {
  if (!isTauriRuntime || !caps.desktop || !name.trim()) {
    return null;
  }
  return invoke<string | null>("resolve_executable_path", { name });
}

/** Copy a picked image into the data dir (no base64) and return its filename. */
export async function importBackgroundImage(srcPath: string): Promise<string> {
  return invoke<string>("import_background_image", { srcPath });
}

/** Linux：WebKitGTK 部分环境对 asset 协议子资源不发请求，图像改走 dataURL（与移动端同款）。 */
export async function imageDataUrl(
  kind: "avatar" | "background" | "md",
  filename: string,
  nodeId?: string
): Promise<string> {
  return invoke<string>("image_data_url", { kind, filename, nodeId: nodeId ?? null });
}

/** Resolve a stored background image filename to a webview-displayable URL (asset protocol, no base64). */
export async function backgroundImageUrl(filename: string): Promise<string> {
  if (caps.dataUrlImages) {
    return imageDataUrl("background", filename);
  }
  const path = await invoke<string>("background_image_path", { filename });
  return convertFileSrc(path);
}

/** Copy a picked file into the avatar directory. Returns stored filename. */
export async function saveAvatarImage(srcPath: string): Promise<string> {
  return invoke<string>("save_avatar_image", { srcPath });
}

/** Resolve avatar filename to asset URL. */
export async function avatarImageUrl(filename: string): Promise<string> {
  if (caps.dataUrlImages) {
    return imageDataUrl("avatar", filename);
  }
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
  if (caps.dataUrlImages) {
    return imageDataUrl("md", filename, nodeId);
  }
  const path = await invoke<string>("md_image_path", { nodeId, filename });
  return convertFileSrc(path);
}

/** Save a base64 data URL as a markdown image (for clipboard paste). Returns stored filename. */
export async function saveMdImageFromDataUrl(dataUrl: string, nodeId: string): Promise<string> {
  return invoke<string>("save_md_image_data", { dataUrl, nodeId });
}

/** Save a base64 data URL as the list background image (<input type=file> 移动端路径)。Returns stored filename. */
export async function saveBackgroundImageFromDataUrl(dataUrl: string): Promise<string> {
  return invoke<string>("save_background_image", { dataUrl });
}

// ---------------------------------------------------------------------------
// v9 Domain Core bridge（桌面 + 移动端）。浏览器预览回退 localStorage legacy 路径。
// ---------------------------------------------------------------------------

let coreAvailable: boolean | null = null;

export async function hasCoreDispatch(): Promise<boolean> {
  if (!isTauriRuntime) {
    return false;
  }
  if (coreAvailable !== null) {
    return coreAvailable;
  }
  // 所有 Tauri 平台（桌面 + 移动端）一律 fail closed：core_ping 失败直接抛错，
  // 绝不回退到整文件 legacy 写入。
  const capability = await invoke<{ available: boolean }>("core_ping");
  coreAvailable = capability.available;
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
  diary: unknown;
  ledger: unknown;
  revisions: { data: number; settings: number; schedule: number; diary: number; ledger: number };
};

export async function coreSnapshot(): Promise<CoreSnapshot> {
  return invoke<CoreSnapshot>("core_snapshot");
}

export async function registerGlobalShortcut(shortcut: string): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
    return;
  }
  await invoke("register_global_shortcut", { shortcut });
}

export async function setCloseToTray(enabled: boolean): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
    return;
  }
  await invoke("set_close_to_tray", { enabled });
}

/**
 * 托盘是否真的建起来了（Linux 缺 appindicator 动态库时为 false）。
 * 设置页据此提醒并回落；查询失败（移动端/浏览器）按 true 处理——那种环境
 * 根本不显示这一节，而 Host 侧不可用时已经自己回落成退出了。
 */
export async function trayAvailable(): Promise<boolean> {
  if (!isTauriRuntime || !caps.desktop) {
    return true;
  }
  try {
    return await invoke<boolean>("tray_available");
  } catch {
    return true;
  }
}

export async function setAutostart(enabled: boolean): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
    return;
  }
  await invoke("set_autostart", { enabled });
}

export async function setWebviewZoom(scale: number): Promise<void> {
  if (!isTauriRuntime || !caps.desktop) {
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

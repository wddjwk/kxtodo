import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { defaultSettings, emptyState, normalizeSettings, normalizeState } from "./defaults";
import type { AppState, Settings } from "./types";

const stateKey = "todo-note-state-v3";
const settingsKey = "todo-note-settings-v3";

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
  if (isTauriRuntime) {
    await invoke("save_state", { state });
    return;
  }
  localStorage.setItem(stateKey, JSON.stringify(state));
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

export async function exportData(payload: unknown, defaultName: string): Promise<void> {
  if (isTauriRuntime) {
    const filePath = await save({
      defaultPath: defaultName,
      filters: [{ name: "Todo Note JSON", extensions: ["json"] }]
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

export async function registerGlobalShortcut(shortcut: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  await invoke("register_global_shortcut", { shortcut });
}

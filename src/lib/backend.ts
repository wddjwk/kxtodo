import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { cloneSettings, cloneState, defaultSettings, defaultState, normalizeSettings, normalizeState } from "./defaults";
import type { AppSettings, AppState, ExportPayload } from "./types";

interface ExportRequest {
  scope: "all" | "list";
  listId?: string;
  state: AppState;
  outputPath?: string;
}

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
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

function downloadJson(payload: unknown, name: string): string {
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  URL.revokeObjectURL(url);
  return `browser-download:${name}`;
}

function descendants(state: AppState, rootId: string): Set<string> {
  const ids = new Set([rootId]);
  let changed = true;

  while (changed) {
    changed = false;
    for (const list of state.lists) {
      if (list.parentId && ids.has(list.parentId) && !ids.has(list.id)) {
        ids.add(list.id);
        changed = true;
      }
    }
  }

  return ids;
}

function listExportPayload(request: ExportRequest): ExportPayload {
  if (request.scope === "all") {
    return {
      schemaVersion: 3,
      exportedAt: new Date().toISOString(),
      scope: "all",
      state: request.state
    };
  }

  const listId = request.listId ?? request.state.selectedListId;
  const listIds = descendants(request.state, listId);
  return {
    schemaVersion: 3,
    exportedAt: new Date().toISOString(),
    scope: "list",
    rootListId: listId,
    lists: request.state.lists.filter((list) => listIds.has(list.id)),
    tasks: request.state.tasks.filter((task) => listIds.has(task.listId))
  };
}

export async function loadState(): Promise<AppState> {
  if (isTauriRuntime()) {
    return normalizeState(await invoke<unknown>("load_state"));
  }

  return normalizeState(readLocal("todo-note.state", cloneState(defaultState())));
}

export async function saveState(state: AppState): Promise<void> {
  if (isTauriRuntime()) {
    await invoke("save_state", { state });
    return;
  }

  localStorage.setItem("todo-note.state", JSON.stringify(state));
}

export async function loadSettings(): Promise<AppSettings> {
  if (isTauriRuntime()) {
    return normalizeSettings(await invoke<unknown>("load_settings"));
  }

  return normalizeSettings(readLocal("todo-note.settings", cloneSettings(defaultSettings())));
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  if (isTauriRuntime()) {
    await invoke("save_settings", { settings });
    return;
  }

  localStorage.setItem("todo-note.settings", JSON.stringify(settings));
}

export async function exportData(request: ExportRequest): Promise<string> {
  let outputPath = request.outputPath;
  if (isTauriRuntime()) {
    outputPath =
      outputPath ??
      ((await save({
        defaultPath:
          request.scope === "all"
            ? `todo-note-all-${new Date().toISOString().slice(0, 10)}.json`
            : `todo-note-${request.listId ?? request.state.selectedListId}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }]
      })) as string | null | undefined) ??
      undefined;

    if (!outputPath) {
      return "cancelled";
    }

    return invoke<string>("export_data", { request: { ...request, outputPath } });
  }

  const payload = listExportPayload(request);
  return downloadJson(payload, request.scope === "all" ? "todo-note-all.json" : `todo-note-${payload.rootListId}.json`);
}

export async function registerGlobalShortcut(shortcut: string): Promise<void> {
  if (isTauriRuntime()) {
    await invoke("register_global_shortcut", { shortcut });
  }
}

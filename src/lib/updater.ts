// ---------------------------------------------------------------------------
// updater.ts — GitHub latest release 检查 + Rust 侧下载/shim/重启。
// 检查走 WebView2 fetch（api.github.com 允许 CORS）；下载、落盘、换链、重启
// 全部由 Rust 的 update_download_and_apply 在后台线程完成，进度经事件回传。
// ---------------------------------------------------------------------------

import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "./backend";

const RELEASE_API = "https://api.github.com/repos/wddjwk/kxtodo/releases/latest";
const FETCH_TIMEOUT_MS = 15_000;

export type UpdateInfo = {
  version: string;
  tag: string;
  guiUrl: string;
  cliUrl: string;
  notes: string;
};

export type UpdateCheckResult =
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo }
  | { status: "error"; message: string };

export type UpdatePhase = "idle" | "downloading" | "restarting" | "failed";

export type UpdateProgress = {
  phase: UpdatePhase;
  stage: "GUI" | "CLI" | "";
  percent: number;
  message: string;
};

/** 下载/重启全过程状态：SettingsDrawer 直接订阅渲染。 */
export const updateProgress = writable<UpdateProgress>({
  phase: "idle",
  stage: "",
  percent: 0,
  message: ""
});

if (isTauriRuntime) {
  void listen<{ stage: string; percent: number }>("update://progress", (event) => {
    updateProgress.update((state) => ({
      ...state,
      phase: "downloading",
      stage: event.payload.stage === "CLI" ? "CLI" : "GUI",
      percent: event.payload.percent
    }));
  });
  void listen("update://applied", () => {
    updateProgress.set({ phase: "restarting", stage: "", percent: 100, message: "" });
  });
  void listen<{ message: string }>("update://failed", (event) => {
    updateProgress.set({ phase: "failed", stage: "", percent: 0, message: event.payload.message });
  });
}

function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((n) => Number(n) || 0);
  const pb = b.split(".").map((n) => Number(n) || 0);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  }
  return 0;
}

async function fetchJson(url: string): Promise<unknown> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(url, { signal: controller.signal });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

/** 查询 GitHub latest release；有更新返回版本与 GUI/CLI 下载地址。 */
export async function checkForUpdate(currentVersion: string): Promise<UpdateCheckResult> {
  try {
    const data = (await fetchJson(RELEASE_API)) as {
      tag_name?: string;
      body?: string;
      assets?: Array<{ name?: string; browser_download_url?: string }>;
    };
    const tag = data.tag_name ?? "";
    const latest = tag.replace(/^v/, "");
    if (!latest || compareVersions(latest, currentVersion) <= 0) {
      return { status: "up-to-date" };
    }
    const assets = data.assets ?? [];
    const gui = assets.find((item) => {
      const name = item.name ?? "";
      return /^KXToDo-.*\.exe$/.test(name) && !name.includes("CLI");
    });
    const cli = assets.find((item) => /^KXToDo-CLI-.*\.exe$/.test(item.name ?? ""));
    if (!gui?.browser_download_url || !cli?.browser_download_url) {
      return { status: "error", message: "最新发布中缺少 GUI 或 CLI 安装包，无法更新" };
    }
    return {
      status: "available",
      info: {
        version: latest,
        tag,
        guiUrl: gui.browser_download_url,
        cliUrl: cli.browser_download_url,
        notes: data.body ?? ""
      }
    };
  } catch (error) {
    return { status: "error", message: `检查更新失败：${String(error)}` };
  }
}

/** 触发 Rust 侧下载 → shim → 重启；进度与结果经 updateProgress store 回传。 */
export async function startUpdate(info: UpdateInfo): Promise<void> {
  updateProgress.set({ phase: "downloading", stage: "GUI", percent: 0, message: "" });
  await invoke("update_download_and_apply", {
    params: { version: info.version, guiUrl: info.guiUrl, cliUrl: info.cliUrl }
  });
}

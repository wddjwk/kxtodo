// ---------------------------------------------------------------------------
// updater.ts — GitHub latest release 检查 + 平台分支下载。
// 桌面（Windows/Linux）：Rust 的 update_download_and_apply 下载固定名制品、替换后重启，进度经事件回传。
//   - Windows：替换 exe 同目录的 KXToDo.exe / kxtodo-cli.exe（bat 等本进程退出后换文件再重启）。
//   - Linux：替换 ~/.local/share/kxtodo/bin/ 下的 KXToDo.AppImage / kxtodo-cli，尝试重启；
//     拉起失败则经 "update://applied" 的 manualRestart 提示用户手动重启。
// 移动端：update_download_apk 下载 APK 到 cacheDir，"update://applied" 带 path，
// 由 Kotlin 桥 window.kxtodoAndroid.installApk 拉起系统安装器。
// 检查走 WebView fetch（api.github.com 允许 CORS）。
// ---------------------------------------------------------------------------

import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "./backend";
import { caps } from "./capabilities";
import { hostOs } from "./platform";

const RELEASE_API = "https://api.github.com/repos/wddjwk/kxtodo/releases/latest";
const FETCH_TIMEOUT_MS = 15_000;

export type UpdateInfo = {
  version: string;
  tag: string;
  notes: string;
  guiUrl?: string;
  cliUrl?: string;
  apkUrl?: string;
};

export type UpdateCheckResult =
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo }
  | { status: "error"; message: string };

export type UpdatePhase = "idle" | "downloading" | "installing" | "restarting" | "failed";

export type UpdateProgress = {
  phase: UpdatePhase;
  stage: "GUI" | "CLI" | "APK" | "";
  percent: number;
  message: string;
  /** 下载阶段的补充说明：回退加速代理的提示，或拿不到 content-length 时的已下载体积 */
  note?: string;
  /** 是否知道总大小；不知道就没有百分比可显示（只能显示已下载体积）。缺省按知道处理 */
  knownTotal?: boolean;
};

/** 下载/安装/重启全过程状态：SettingsDrawer 直接订阅渲染。 */
export const updateProgress = writable<UpdateProgress>({
  phase: "idle",
  stage: "",
  percent: 0,
  message: "",
  note: "",
  knownTotal: true
});

function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

if (isTauriRuntime) {
  void listen<{
    stage: string;
    percent: number;
    received?: number;
    total?: number;
    note?: string;
  }>("update://progress", (event) => {
    const stage = event.payload.stage;
    const total = event.payload.total ?? 0;
    const received = event.payload.received ?? 0;
    updateProgress.update((state) => ({
      ...state,
      phase: "downloading",
      stage: stage === "CLI" ? "CLI" : stage === "APK" ? "APK" : "GUI",
      percent: event.payload.percent,
      knownTotal: total > 0,
      // 后端的提示（例如「改用加速代理重试」）优先；没有提示且不知道总大小时报已下载体积
      note: event.payload.note || (total > 0 ? "" : `已下载 ${megabytes(received)}`)
    }));
  });
  void listen<{ path?: string; manualRestart?: boolean; message?: string }>("update://applied", (event) => {
    const path = event.payload?.path;
    if (path) {
      // 移动端：APK 已落盘，交给 Kotlin 桥拉起 PackageInstaller。
      const bridge = window.kxtodoAndroid;
      if (bridge?.installApk) {
        const err = bridge.installApk(path);
        if (err) {
          updateProgress.set({ phase: "failed", stage: "", percent: 0, message: err });
          return;
        }
      }
      updateProgress.set({ phase: "installing", stage: "", percent: 100, message: "" });
      return;
    }
    if (event.payload?.manualRestart) {
      // Linux：新制品已替换到 ~/.local/share/kxtodo/bin，但自动拉起失败，提示用户手动重启。
      updateProgress.set({
        phase: "restarting",
        stage: "",
        percent: 100,
        message: event.payload.message || "已下载新版本，请手动重启应用完成更新"
      });
      return;
    }
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

/** 查询 GitHub latest release；有更新时按平台返回 GUI/CLI 或 APK 下载地址。 */
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

    if (caps.updateChannel === "apk") {
      // 发布资产固定命名 KXToDo.apk（不带版本号，覆盖安装不留历史包）
      const apk = assets.find((item) => item.name === "KXToDo.apk");
      if (!apk?.browser_download_url) {
        return { status: "error", message: "最新发布中缺少 APK 安装包，无法更新" };
      }
      return {
        status: "available",
        info: {
          version: latest,
          tag,
          notes: data.body ?? "",
          apkUrl: apk.browser_download_url
        }
      };
    }

    // 桌面发布资产是固定名（不带版本号）：Linux 用 KXToDo.AppImage + kxtodo-cli，Windows 用 KXToDo.exe + kxtodo-cli.exe。
    const isLinux = hostOs === "linux";
    const guiName = isLinux ? "KXToDo.AppImage" : "KXToDo.exe";
    const cliName = isLinux ? "kxtodo-cli" : "kxtodo-cli.exe";
    const gui = assets.find((item) => item.name === guiName);
    const cli = assets.find((item) => item.name === cliName);
    if (!gui?.browser_download_url || !cli?.browser_download_url) {
      return { status: "error", message: `最新发布中缺少 ${guiName} 或 ${cliName}，无法更新` };
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

/** 触发 Rust 侧下载；桌面继续 下载→替换→重启，移动端落 APK 后经事件交给系统安装器。 */
export async function startUpdate(info: UpdateInfo): Promise<void> {
  if (caps.updateChannel === "apk") {
    if (!info.apkUrl) {
      throw new Error("缺少 APK 下载地址");
    }
    updateProgress.set({ phase: "downloading", stage: "APK", percent: 0, message: "" });
    await invoke("update_download_apk", {
      params: { version: info.version, apkUrl: info.apkUrl }
    });
    return;
  }
  if (!info.guiUrl || !info.cliUrl) {
    throw new Error("缺少 GUI 或 CLI 下载地址");
  }
  updateProgress.set({ phase: "downloading", stage: "GUI", percent: 0, message: "" });
  await invoke("update_download_and_apply", {
    params: { guiUrl: info.guiUrl, cliUrl: info.cliUrl }
  });
}

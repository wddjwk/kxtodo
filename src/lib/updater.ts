// ---------------------------------------------------------------------------
// updater.ts — GitHub latest release 检查更新 + 单 binary 替换更新。
// 检查走 WebView2 fetch（系统代理生效）；下载分块经 IPC 落盘；
// 替换由 Rust 侧生成的 updater bat 在进程退出后完成并重启。
// ---------------------------------------------------------------------------

import { writeUpdatePackage, applyUpdateAndRestart } from "./backend";

const RELEASE_API = "https://api.github.com/repos/wddjwk/kxtodo/releases/latest";
const FETCH_TIMEOUT_MS = 15_000;
const CHUNK_SIZE = 2 * 1024 * 1024;

export type UpdateInfo = {
  version: string;
  tag: string;
  downloadUrl: string;
  notes: string;
};

export type UpdateCheckResult =
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo }
  | { status: "error"; message: string };

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

/** 查询 GitHub latest release；有更新返回版本与 GUI exe 下载地址。 */
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
    const asset = (data.assets ?? []).find((item) => {
      const name = item.name ?? "";
      return /^KXToDo-.*\.exe$/.test(name) && !name.includes("CLI");
    });
    if (!asset?.browser_download_url) {
      return { status: "error", message: "最新发布中没有找到安装包" };
    }
    return {
      status: "available",
      info: {
        version: latest,
        tag,
        downloadUrl: asset.browser_download_url,
        notes: data.body ?? ""
      }
    };
  } catch (error) {
    return { status: "error", message: `检查更新失败：${String(error)}` };
  }
}

/** 下载更新包并落盘（分块），返回后由调用方决定是否重启应用。 */
export async function downloadUpdate(
  info: UpdateInfo,
  onProgress: (percent: number) => void
): Promise<void> {
  const response = await fetch(info.downloadUrl);
  if (!response.ok || !response.body) throw new Error(`下载失败：HTTP ${response.status}`);
  const total = Number(response.headers.get("content-length")) || 0;
  const reader = response.body.getReader();
  let received = 0;
  let first = true;
  let pending: Uint8Array[] = [];
  let pendingBytes = 0;
  const flush = async () => {
    if (pendingBytes === 0) return;
    const merged = new Uint8Array(pendingBytes);
    let offset = 0;
    for (const part of pending) {
      merged.set(part, offset);
      offset += part.length;
    }
    pending = [];
    pendingBytes = 0;
    let binary = "";
    for (let i = 0; i < merged.length; i += 32768) {
      binary += String.fromCharCode(...merged.subarray(i, i + 32768));
    }
    await writeUpdatePackage(btoa(binary), !first);
    first = false;
  };
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    pending.push(value);
    pendingBytes += value.length;
    received += value.length;
    if (pendingBytes >= CHUNK_SIZE) await flush();
    if (total > 0) onProgress(Math.round((received / total) * 100));
  }
  await flush();
}

/** 应用更新并重启（bat 等待进程退出后替换 exe）。 */
export async function applyUpdate(): Promise<void> {
  await applyUpdateAndRestart();
}

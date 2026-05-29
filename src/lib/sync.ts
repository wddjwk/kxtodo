import type { AppState, CloudSyncSettings } from "./types";

export interface SyncAdapter {
  id: CloudSyncSettings["provider"];
  label: string;
  push(state: AppState): Promise<void>;
  pull(): Promise<AppState | null>;
}

export const syncRoadmap = [
  "保持本地 JSON 模型稳定，避免和具体云服务耦合。",
  "后续优先添加 WebDAV 适配器，再扩展 S3/自定义 HTTP。",
  "同步前先比较 updatedAt，并保留冲突副本。"
];

export function createDisabledSyncAdapter(): SyncAdapter {
  return {
    id: "none",
    label: "未配置",
    async push() {
      throw new Error("Cloud sync is reserved but not implemented yet.");
    },
    async pull() {
      return null;
    }
  };
}

/**
 * 传输事件 → 界面文案的纯映射（v0.8.6 需求 5.3/5.4，无 Tauri 依赖、有单测）。
 *
 * 单独成模块的理由：`transferStore.ts` 静态 import 了 Tauri 的 invoke/listen，
 * 在 node 单测里根本 import 不进来；而「错误怎么显示、历史三态怎么画」这类规则
 * 恰恰是最容易写错、也最该被钉住的部分。
 */

/**
 * 会话失败时的文案。
 *
 * 早先只要会话进过 connected，一律显示「对方离线了」——把本地真实原因
 * （`TRANSFER_PATH_UNSAFE` / `TRANSFER_NAME_EXHAUSTED` / 写盘失败……）全吞了，
 * 用户看到「接收失败，对方已离线」而真正的问题在本地（v0.8.6 需求 5.4.3）。
 * 现在只有 core 明确标了 `TRANSFER_CONNECTION_LOST` 才是「对方离线」。
 */
export function transferErrorText(code: string | undefined, message: string | undefined): string {
  const text = (message ?? "").trim() || "传输失败";
  if (code === "TRANSFER_CONNECTION_LOST") return `对方离线了（${text}）`;
  if (code === "TRANSFER_REJECTED") return "接收方拒绝了这次传输";
  return text;
}

export type HistoryTone = "ok" | "text" | "rejected" | "bad";

/** 历史条目的三态（完成 / 文本 / 被拒绝 / 失败）：颜色类与图标一处定。 */
export function historyStatusStyle(status: string): { tone: HistoryTone; glyph: string; label: string } {
  if (status === "done") return { tone: "ok", glyph: "✓", label: "完成" };
  if (status === "text") return { tone: "text", glyph: "✉", label: "文本消息" };
  if (status === "rejected") return { tone: "rejected", glyph: "⊘", label: "已拒绝" };
  return { tone: "bad", glyph: "✕", label: "失败" };
}

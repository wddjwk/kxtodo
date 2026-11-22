/**
 * 标语栏纯逻辑（v0.9.4；v0.9.5 条目级图标/六位置/主题色/间隔）：展示名单解析 /
 * 图标语义 / 轮播推进 / 位置元数据。不 import store 与组件，node 单测可直接跑。
 */
import type { SloganItem, SloganPosition, SloganState } from "./types";

export const SLOGAN_ICON_DEFAULT = "Megaphone";
export const SLOGAN_ICON_NONE = "none";
/** 悬浮层最多展示的字数（约 30 个全角字符宽）；超出走横向滚动。 */
export const SLOGAN_DISPLAY_CHARS = 30;
export const SLOGAN_INTERVAL_DEFAULT = 8;
export const SLOGAN_INTERVAL_MIN = 2;
export const SLOGAN_INTERVAL_MAX = 120;

/** 六个悬浮位置（顶部/底部对齐内容区，左/右/居中水平对齐；不锚定输入框）。 */
export const SLOGAN_POSITIONS: SloganPosition[] = [
  "top-left",
  "top-center",
  "top-right",
  "bottom-left",
  "bottom-center",
  "bottom-right"
];

export const SLOGAN_POSITION_LABELS: Record<SloganPosition, string> = {
  "top-left": "左上",
  "top-center": "顶部居中",
  "top-right": "右上",
  "bottom-left": "左下",
  "bottom-center": "底部居中",
  "bottom-right": "右下"
};

/** 生效的展示图标名：空 = 默认喇叭；"none" = 不加图标（返回 null）。 */
export function sloganIconName(icon: string): string | null {
  if (icon === SLOGAN_ICON_NONE) return null;
  return icon || SLOGAN_ICON_DEFAULT;
}

/** 图标值是否是 emoji（v0.9.5）：非 ASCII、无空白、≤12 个码点——与 core
 *  `model::is_slogan_emoji` 同口径。 */
export function isSloganEmoji(value: string): boolean {
  if (!value || value === SLOGAN_ICON_NONE) return false;
  const chars = Array.from(value);
  return chars.length <= 12 && /[^\x00-\x7F]/.test(value) && !/\s/.test(value);
}

export type SloganIconKind = "none" | "emoji" | "lucide";

/** 图标三分类：none / emoji / lucide 名（空按默认喇叭算 lucide）。 */
export function sloganIconKind(icon: string): SloganIconKind {
  if (icon === SLOGAN_ICON_NONE) return "none";
  if (isSloganEmoji(icon)) return "emoji";
  return "lucide";
}

/** 从任务的 markdown 里抽一句标语：第一行非空文本，去掉标题/列表/勾选标记与强调符。 */
export function sloganTextFromMarkdown(markdown: string): string {
  const line = (markdown ?? "")
    .split(/\r?\n/)
    .map((item) => item.trim())
    .find((item) => item !== "") ?? "";
  const text = line
    .replace(/^#{1,6}\s*/, "")
    .replace(/^>\s*/, "")
    .replace(/^[-*+]\s+(\[[ xX]\]\s+)?/, "")
    .replace(/^\d+[.)]\s+/, "")
    .replace(/[*_`~]/g, "")
    .trim();
  return text.slice(0, 80);
}

/** 按 activeIds 的顺序取要展示的标语（失效 id 自动跳过）。 */
export function activeSlogans(state: SloganState): SloganItem[] {
  const byId = new Map(state.items.map((item) => [item.id, item]));
  return state.activeIds
    .map((id) => byId.get(id))
    .filter((item): item is SloganItem => item !== undefined);
}

/** 轮播下一帧索引（单条/空名单恒 0）。 */
export function nextRotation(index: number, length: number): number {
  return length <= 1 ? 0 : (index + 1) % length;
}

/** 轮播间隔毫秒（配置秒数 → timer 毫秒，带区间兜底）。 */
export function sloganIntervalMs(seconds: number): number {
  const clamped = Math.min(SLOGAN_INTERVAL_MAX, Math.max(SLOGAN_INTERVAL_MIN, Math.round(seconds) || SLOGAN_INTERVAL_DEFAULT));
  return clamped * 1000;
}

/** 新增一条标语后的新 items（文本 trim 截断，空文本不加；新条目用默认图标配置）。 */
export function withNewItem(state: SloganState, id: string, text: string, createdAt: string): SloganItem[] {
  const trimmed = text.trim().slice(0, 80);
  if (!trimmed) return state.items;
  return [...state.items, { id, text: trimmed, icon: state.defaultIcon, createdAt }].slice(0, 50);
}
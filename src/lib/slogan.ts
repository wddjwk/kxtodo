/**
 * 标语栏纯逻辑（v0.9.4）：展示名单解析 / 图标语义 / 轮播推进。
 * 不 import store 与组件，node 单测可直接跑。
 */
import type { SloganItem, SloganState } from "./types";

export const SLOGAN_ICON_DEFAULT = "Megaphone";
export const SLOGAN_ICON_NONE = "none";
/** 多条轮播的间隔（毫秒）；悬浮层底部的进度条按它走满一轮。 */
export const SLOGAN_ROTATE_MS = 8000;

/** 生效的展示图标名：空 = 默认喇叭；"none" = 不加图标（返回 null）。 */
export function sloganIconName(icon: string): string | null {
  if (icon === SLOGAN_ICON_NONE) return null;
  return icon || SLOGAN_ICON_DEFAULT;
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

/** 新增一条标语后的新 items（文本 trim 截断，空文本不加）。 */
export function withNewItem(state: SloganState, id: string, text: string, createdAt: string): SloganItem[] {
  const trimmed = text.trim().slice(0, 80);
  if (!trimmed) return state.items;
  return [...state.items, { id, text: trimmed, createdAt }].slice(0, 50);
}
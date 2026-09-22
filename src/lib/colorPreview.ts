/**
 * 取色预览（v0.8.4，需求 9）：三层颜色配置——**UI 主题色 / 背景色 / 临期高亮色**——
 * 统一成「拖动时界面先跟着变，点保存才落盘」。
 *
 * 从前是三种各写各的：主题色只动色块不动界面、背景色只动界面不动色块、临期高亮两个都不动。
 * 现在活值（草稿）住在 `ListMenu` / 工具箱外观菜单的组件状态里，**预览**经这里推给界面，
 * 落盘仍然只有一条路（`setConfigAction` / `setUiColorAction`），保存时才走。
 *
 * `scope` 是「这份草稿管哪一页」：条目页 = 节点 id，日记/记账/工具箱 = 各自的 settings 前缀。
 * 消费端只认自己那一份（`scope` 对不上就当没有）——不然在日记页改色会把工作区也染上。
 *
 * 纯函数与 store 分开：`accentWithPreview` 这几个选择器不碰 Svelte，能跑 node 单测
 * （分错、串页这类错误不做测试就只能靠肉眼）。
 */
import { writable } from "svelte/store";
import type { ListBackground } from "./types";

export type ColorPreview = {
  /** 这一份草稿属于哪一页：节点 id / "diary" / "ledger" / "toolbox" */
  scope: string;
  /** UI 主题色（--accent）的活值 */
  accent?: string;
  /** 背景色的活值 */
  background?: string;
  /** 临期高亮第 index 档的活值（档位下标 → 颜色，可能一次改多档） */
  due?: Record<number, string>;
};

export const colorPreview = writable<ColorPreview | null>(null);

/** 开一份草稿（同页再次取色就换掉活值，不叠加）。 */
export function setColorPreview(scope: string, patch: Omit<ColorPreview, "scope">): void {
  colorPreview.set({ scope, ...patch });
}

/** 收起草稿：保存之后、取消、以及菜单关闭（= 没保存就不算数）都要调。 */
export function clearColorPreview(): void {
  colorPreview.set(null);
}

function previewFor(preview: ColorPreview | null, scope: string): ColorPreview | null {
  return preview && preview.scope === scope ? preview : null;
}

export function accentWithPreview(preview: ColorPreview | null, scope: string, fallback: string): string {
  return previewFor(preview, scope)?.accent ?? fallback;
}

export function backgroundWithPreview(
  preview: ColorPreview | null,
  scope: string,
  fallback: ListBackground
): ListBackground {
  const color = previewFor(preview, scope)?.background;
  return color ? { ...fallback, color } : fallback;
}

/** 四档临期配色：`stored` 可以是 undefined（没配过 = 用默认）。 */
export function dueColorsWithPreview(
  preview: ColorPreview | null,
  scope: string,
  stored: string[] | undefined,
  defaults: string[]
): string[] {
  const base = stored && stored.length === defaults.length ? [...stored] : [...defaults];
  const due = previewFor(preview, scope)?.due;
  if (due) {
    for (const [key, color] of Object.entries(due)) {
      const index = Number(key);
      if (Number.isInteger(index) && index >= 0 && index < base.length) base[index] = color;
    }
  }
  return base;
}

/**
 * 系统视图的四个 id（聚合多个条目，自己不是任何卡片的「条目」）。
 * 编辑端（三点菜单）在这些视图里写的是 `dueColors[视图id]`，
 * 所以消费端（TaskCard）必须能在卡片自己的配置缺席时回退到它。
 */
export const SYSTEM_VIEW_IDS = ["my-day", "planned", "important", "scheduled"] as const;

/**
 * 一张卡片的临期配色（v0.8.7 需求 2.6）——**双回退**：
 *
 * ① 自己条目的配置优先（条目页里编辑端/消费端天然同键，行为与从前一致）；
 * ② 自己没配过、且当前正看系统视图时，跟**这个视图**的配置走（含视图草稿）。
 *
 * 修的是「编辑端写视图键、消费端读条目键」两头顶牛：在「我的一天」里改「今天」档，
 * 预览不染卡片、保存后卡片也不变（条目页两头键一致所以完全正常，别被它骗过去）。
 */
export function dueColorsForCard(
  preview: ColorPreview | null,
  nodeId: string,
  viewId: string,
  stored: Record<string, string[]> | undefined,
  defaults: string[]
): string[] {
  const own = stored?.[nodeId];
  if (own) return dueColorsWithPreview(preview, nodeId, own, defaults);
  if ((SYSTEM_VIEW_IDS as readonly string[]).includes(viewId)) {
    // 兜底基数是**视图键**的落盘值；再叠自己条目的草稿、最后叠视图草稿（同一条链）
    const base = dueColorsWithPreview(preview, nodeId, stored?.[viewId], defaults);
    return dueColorsWithPreview(preview, viewId, base, defaults);
  }
  return dueColorsWithPreview(preview, nodeId, undefined, defaults);
}

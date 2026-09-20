/**
 * 卡片级的全局唯一浮层与露出态（v0.8.6 需求 4）。
 *
 * 背景：每张 `TaskCard` 都自带一份 `<svelte:window on:click on:pointerdown>`，
 * 几千张卡 = 万级监听器。而这两份监听干的两件事本质上都是「全局唯一」的：
 *
 * - **「日期与提醒」浮层**在全应用同时只可能开一个 → 宿主任务 id 提到模块级 store，
 *   同一时刻只有一张卡在开；点其他任何地方（别的卡片、别的日期、空白）都可能
 *   「**不保存关闭**」——这就是约定的失焦语义，点另一张卡的日期自然满足「先关旧再开新」。
 * - **触屏露出删除叉**（`revealedTag` / `revealedEmoji`）同样是「点别处收回」→ 同一份 store。
 *
 * 两件事共用**一份 document 级 pointerdown**（capture 阶段，早于目标自身的处理器；
 * 面板与唤起按钮豁免），由 App 启动时调一次 `ensureCardOverlayRuntime`。
 */
import { get, writable } from "svelte/store";

/** 「日期与提醒」浮层的宿主任务 id（null = 没开） */
export const datePopoverTaskId = writable<string | null>(null);
/** 触屏第一下露出的标签删除叉 */
export const revealedTag = writable<{ taskId: string; tagId: string } | null>(null);
/** 触屏第一下露出的表情删除叉 */
export const revealedEmoji = writable<{ taskId: string; index: number } | null>(null);

/** 卡片点日期按钮：同一张再点一次 = 关闭；另一张点 = 先关旧的再开新的 */
export function toggleDatePopover(taskId: string): void {
  datePopoverTaskId.update((current) => (current === taskId ? null : taskId));
}

export function closeDatePopover(): void {
  datePopoverTaskId.set(null);
}

export function revealTag(taskId: string, tagId: string): void {
  revealedTag.set({ taskId, tagId });
  revealedEmoji.set(null);
}

export function revealEmoji(taskId: string, index: number): void {
  revealedEmoji.set({ taskId, index });
  revealedTag.set(null);
}

export function hideReveals(): void {
  revealedTag.set(null);
  revealedEmoji.set(null);
}

let installed = false;

/** 应用启动时装一次（App.svelte）。幂等；非浏览器环境直接跳过。 */
export function ensureCardOverlayRuntime(): void {
  if (installed || typeof document === "undefined") return;
  installed = true;
  document.addEventListener("pointerdown", handleDocumentPointerDown, true);
}

function handleDocumentPointerDown(event: PointerEvent): void {
  const target = event.target as HTMLElement | null;
  // 日期浮层：面板内部与「日期按钮」豁免（按钮自己 toggle，先关后开会让它永远关不掉）
  const insidePanel = Boolean(target?.closest("[data-date-popover]"));
  const onAnchor = Boolean(target?.closest("[data-date-anchor]"));
  if (!insidePanel && !onAnchor && get(datePopoverTaskId) !== null) closeDatePopover();
  // 露出的删除叉：点标签/表情自身之外就收回
  if (get(revealedTag) && !target?.closest(".task-tag")) revealedTag.set(null);
  if (get(revealedEmoji) && !target?.closest(".task-emoji-badge")) revealedEmoji.set(null);
}

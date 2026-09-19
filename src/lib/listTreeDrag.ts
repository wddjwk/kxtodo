/**
 * 分组树拖动的落点状态（v0.8.5 需求 29）。
 *
 * 为什么需要它：`ListTree` 是递归组件（`svelte:self`），每一层是一个**独立实例**。
 * 拖动逻辑跑在「指针按下的那一行所属的实例」里（window 上的 pointermove 是它挂的），
 * 而目标行常常属于另一个实例——落点状态住在实例里的话，`class:drop-inside` 这类
 * 反馈只有同层的行画得出来：拖条目到别的分组头（跨层）功能是对的，虚线框却永远不出现。
 *
 * 全局同时只有一个拖动，所以用单例 store 就够了。
 */
import { writable } from "svelte/store";

export type TreeDropPosition = "before" | "after" | "inside";

export type TreeDropState = {
  targetId: string | null;
  position: TreeDropPosition | null;
  /** 拖到空白区（列表末尾） */
  rootEnd: boolean;
};

const EMPTY: TreeDropState = { targetId: null, position: null, rootEnd: false };

export const treeDropState = writable<TreeDropState>(EMPTY);

export function setTreeDropTarget(targetId: string, position: TreeDropPosition): void {
  treeDropState.set({ targetId, position, rootEnd: false });
}

export function setTreeDropRootEnd(): void {
  treeDropState.set({ targetId: null, position: null, rootEnd: true });
}

export function clearTreeDropTarget(): void {
  treeDropState.set(EMPTY);
}

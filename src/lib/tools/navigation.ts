/**
 * 工具箱的路由（v0.8.3）：固定导航里钉了一个工具时，点它要**直接进那个工具的子视图**，
 * 而不是先落回工具列表——钉住的意义就是少点一下。
 *
 * 壳（ToolboxView）订阅 `toolRoute`；`fromPin` 记住「这一趟是从侧栏钉住的行进来的」，
 * 于是子视图里按返回时连工具箱整页一起收（停在列表上等于给用户一个他没来过的页面）。
 * 从工具箱列表自己点进去的，返回只退回列表。
 */
import { writable } from "svelte/store";
import type { ToolId } from "./catalog";

export type ToolRoute = {
  id: ToolId | null;
  fromPin: boolean;
};

export const toolRoute = writable<ToolRoute>({ id: null, fromPin: false });

/** 打开工具箱；`id` 给了就直达那个工具的子视图。 */
export function openToolboxTool(id: ToolId | null = null, fromPin = false): void {
  toolRoute.set({ id, fromPin });
}

/** 回到工具列表（清掉直达标记）。 */
export function resetToolRoute(): void {
  toolRoute.set({ id: null, fromPin: false });
}

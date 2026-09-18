/**
 * 工具箱的路由（v0.8.3）：固定导航里钉了一个工具时，点它要**直接进那个工具的子视图**，
 * 而不是先落回工具列表——钉住的意义就是少点一下。
 *
 * v0.8.4 起这里就是「当前打开哪一个工具」的**唯一真源**（`null` = 列表）：
 * 列表点卡片、侧栏钉住的行直达都只是 `set(id)`，壳（ToolboxView）从它纯派生子视图。
 * 壳里不再另存一份 `activeToolId`——两份状态一定会对不齐（v0.8.3 的症状是
 * 「路由到了、界面不动」，以及「点任何工具都跳回被固定的那个」）。
 * 子视图里返回一律回工具箱主界面（列表）；整页的收放由移动端历史栈 / 桌面开关负责。
 */
import { writable } from "svelte/store";
import type { ToolId } from "./catalog";

/** 当前打开的工具；`null` = 工具箱列表。 */
export const toolRoute = writable<ToolId | null>(null);

/** 打开工具箱；`id` 给了就直达那个工具的子视图。 */
export function openToolboxTool(id: ToolId | null = null): void {
  toolRoute.set(id);
}

/** 回到工具列表。 */
export function resetToolRoute(): void {
  toolRoute.set(null);
}

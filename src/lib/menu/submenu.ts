// 菜单子面板的开关状态：MenuItem 与 ContextMenu 之间没有父子 prop 通道
// （子面板是 slot 内容，层级由调用方决定），所以用一个模块级 store 传信号。
//
// 移动端靠它把菜单变成钻入式：二级面板贴右缘展开在窄屏上必然超出屏幕，
// 于是显示二级时隐藏一级，让二级占据菜单原来的位置。

import { writable } from "svelte/store";

/** 当前打开着的子菜单数量（0 = 都收着） */
export const openSubmenus = writable(0);

/** 递增即请求所有子菜单收起：移动端二级面板上的「返回」按钮用 */
export const submenuBack = writable(0);

export function submenuOpened(): void {
  openSubmenus.update((count) => count + 1);
}

export function submenuClosed(): void {
  openSubmenus.update((count) => Math.max(0, count - 1));
}

export function requestSubmenuClose(): void {
  submenuBack.update((count) => count + 1);
}

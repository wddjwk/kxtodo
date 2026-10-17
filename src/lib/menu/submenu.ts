// 菜单子面板的开关状态：MenuItem 与 ContextMenu 之间没有父子 prop 通道
// （子面板是 slot 内容，层级由调用方决定），所以用一个模块级 store 传信号。
//
// 移动端靠它把菜单变成钻入式：二级面板贴右缘展开在窄屏上必然超出屏幕，
// 于是显示二级时隐藏一级，让二级占据菜单原来的位置。
//
// **返回要逐级退**：三级菜单（固定区「添加固定 → 工具箱内的工具」）里一次退光
// 会把用户手里的草稿藏起来。这里记一份打开深度栈；请求关闭时先拍下当前最深
// 一层，让所有 MenuItem 按同一个快照判断，不受订阅回调顺序影响。

import { writable } from "svelte/store";

/** 当前打开着的子菜单数量（0 = 都收着） */
export const openSubmenus = writable(0);

/** 递增即请求最深一层子菜单收起：移动端「返回」按钮用 */
export const submenuBack = writable(0);

const depths: number[] = [];
let closingDepth = 0;

export function submenuOpened(depth: number): void {
  depths.push(depth);
  openSubmenus.set(depths.length);
}

export function submenuClosed(depth: number): void {
  const index = depths.lastIndexOf(depth);
  if (index >= 0) depths.splice(index, 1);
  openSubmenus.set(depths.length);
}

/** 一记返回只关最深这一层；快照在通知前拍好，回调顺序无关紧要。 */
export function requestSubmenuClose(): void {
  closingDepth = depths.length > 0 ? Math.max(...depths) : 0;
  submenuBack.update((count) => count + 1);
}

export function closingSubmenuDepth(): number {
  return closingDepth;
}

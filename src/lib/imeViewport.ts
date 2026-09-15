/**
 * 移动端输入法（视觉视口）跟随。
 *
 * 安卓 WebView 并不总按 `adjustResize` 缩布局视口：键盘弹起时缩的只是**视觉视口**，
 * 而 app-shell 的高度写死 `100vh`（布局视口），于是聚焦的「添加事项」输入框落在键盘
 * 底下——浏览器为了露出它会把整个视觉视口往上顶（`visualViewport.offsetTop` 不为 0），
 * 表现就是「一点输入框整个页面被顶上去、条目位置跟着变」。
 *
 * 这里把 shell 自己收到「键盘之上的可见区域」并跟随那次平移：
 * - 高度 = 视觉视口高（除以 uiScale 换回 shell 内部的逻辑像素）；
 * - 左上角对齐到可见区域左上角（translateY(offsetTop)，父容器没有缩放，
 *   这个位移就是布局 CSS 像素）；
 * 于是浏览器没有任何需要顶的东西，条目原地不动、输入框正好在键盘上沿。
 *
 * 只用**捏合缩放为 1**（scale ≈ 1）时的收缩：用户双指放大时视觉视口也会变小，
 * 那不是输入法，跟着收矮就错了。
 */

import { get, writable } from "svelte/store";

export type ImeViewport = {
  /** 输入法正把可见区域压小（shell 需要收矮并平移） */
  active: boolean;
  /** 视觉视口高度（CSS 像素，未除 uiScale） */
  height: number;
  /** 可见区域相对布局视口顶部的位移（CSS 像素） */
  offset: number;
};

const INACTIVE: ImeViewport = { active: false, height: 0, offset: 0 };

export const imeViewport = writable<ImeViewport>(INACTIVE);

/** 判定阈值：小于这么多像素的收缩当噪声（地址栏、缩放动画之类） */
const MIN_EATEN_PX = 80;

/** 开始跟踪（App onMount 调，移动端专属）。返回取消函数。 */
export function startImeViewport(): () => void {
  if (typeof window === "undefined" || !window.visualViewport) return () => {};
  const vv = window.visualViewport;

  const update = (): void => {
    const eaten = window.innerHeight - vv.height - vv.offsetTop;
    const active = vv.scale <= 1.01 && eaten > MIN_EATEN_PX;
    const next: ImeViewport = active
      ? { active: true, height: vv.height, offset: vv.offsetTop }
      : INACTIVE;
    const current = get(imeViewport);
    if (
      current.active === next.active &&
      current.height === next.height &&
      current.offset === next.offset
    ) {
      return;
    }
    imeViewport.set(next);
  };

  update();
  vv.addEventListener("resize", update);
  vv.addEventListener("scroll", update);
  window.addEventListener("resize", update);
  return () => {
    vv.removeEventListener("resize", update);
    vv.removeEventListener("scroll", update);
    window.removeEventListener("resize", update);
    imeViewport.set(INACTIVE);
  };
}

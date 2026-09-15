// 移动端输入法避让：编辑器浮层底部垫出输入法的高度，工具栏才能停在输入法上方。
//
// 不能只指望 adjustResize 把视口压矮——实测安卓上 WebView 并不总是缩（工具栏会留在
// 页面底部、被输入法盖住）。visualViewport 是权威来源：它的高度与 window.innerHeight
// 之差就是被输入法（或任何视觉视口收缩）吃掉的部分；WebView 真缩了的话两者一起缩、
// 差值为 0，不会重复补偿。浮层在 transform 缩放的 app-shell 里，视觉像素要除回 uiScale。
//
// **shell 自己在跟随时不能再垫**（v0.7.8）：键盘弹起时 app-shell 已经收矮到键盘上沿
// （见 imeViewport.ts），浮层的底边就是键盘上沿——这里再垫一次会把浮层整个顶上去。

import { get } from "svelte/store";
import { appSettings } from "./stores";
import { uiScaleValue } from "./styles";
import { imeViewport } from "./imeViewport";

export function imeInset(node: HTMLElement): { destroy: () => void } {
  if (typeof window === "undefined" || !window.visualViewport) return { destroy: () => {} };
  const vv = window.visualViewport;

  const update = (): void => {
    const eaten = get(imeViewport).active
      ? 0
      : window.innerHeight - vv.height - vv.offsetTop;
    const scale = uiScaleValue(get(appSettings).appearance.uiScale) || 1;
    const logical = Math.max(0, Math.round(eaten / scale));
    node.style.paddingBottom = logical > 0 ? `${logical}px` : "";
  };

  update();
  vv.addEventListener("resize", update);
  vv.addEventListener("scroll", update);
  window.addEventListener("resize", update);
  const unsubscribe = imeViewport.subscribe(() => update());
  return {
    destroy() {
      unsubscribe();
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    }
  };
}

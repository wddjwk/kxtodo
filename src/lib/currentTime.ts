/**
 * 一分钟一跳的「现在」（v0.8.3）。
 *
 * 临期高亮的第四档是「已过期」：一条带时刻的任务会在某个分钟点从「今天」翻成「过期」，
 * 而卡片的 `$:` 只依赖任务与设置——不给它一个会变的输入，它就得等下一次无关重渲才换色。
 * 这里提供一个 60 秒对齐的 store（切回前台时立刻补一跳），TaskCard 把它当 `now` 传进去。
 *
 * 60 秒而不是更细：高亮只精确到分钟，再细只是白重渲。
 */
import { readable } from "svelte/store";

function tickAligned(): Date {
  const now = new Date();
  now.setSeconds(0, 0);
  return now;
}

export const currentMinute = readable<Date>(tickAligned(), (set) => {
  const timer = window.setInterval(() => set(tickAligned()), 60_000);
  const refresh = (): void => set(tickAligned());
  window.addEventListener("visibilitychange", refresh);
  window.addEventListener("focus", refresh);
  return () => {
    window.clearInterval(timer);
    window.removeEventListener("visibilitychange", refresh);
    window.removeEventListener("focus", refresh);
  };
});

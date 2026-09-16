import { peekMarkdown, renderMarkdown } from "./markdown";

/**
 * 短于这个长度就同步渲染。
 *
 * 展开一张卡片有两个成本：一次完整渲染（marked + KaTeX + highlight.js + DOMPurify），
 * 与「让一帧」的 32ms 延迟。短卡片渲染本身只要一两毫秒，让帧反而更慢也更跳；
 * 长卡片（代码块、mermaid、callout、公式堆在一起）渲染能到几百毫秒，那才是必须先让帧的。
 */
const DEFER_MIN_CHARS = 1000;

/**
 * 卡片展开时的完整 markdown 渲染调度器（TaskCard / DiaryCard 共用）。
 *
 * v0.8.0 把完整渲染从「折叠态也白跑」改成「只在展开时算」，方向是对的，但**算的时机**
 * 落在了展开那一次同步 flush 里：用户点下去 → 状态变 → 同一帧里跑完整渲染 → 浏览器才
 * 画。长卡片表现成「点了没反应」，安卓上尤其明显。
 *
 * 这里把时机挪到**浏览器画过一帧之后**：
 * - 记忆化命中 → 同步给出（刚收起又展开、或列表刷新后重挂，都是零成本）；
 * - 短文本 → 同步渲染；
 * - 其余 → 双 rAF 后再算（单 rAF 的回调仍在当帧绘制之前触发，等于没让出时间）。
 *
 * 展开那一刻画出来的是调用方自己的折叠态内容，等完整 HTML 到了再换掉——
 * 卡片立刻有反馈，重活随后跟上。
 */
export function createDeferredMarkdown(apply: (html: string) => void): {
  schedule: (markdown: string) => void;
  cancel: () => void;
} {
  /** 最后一次被要求渲染的文本 */
  let wanted = "";
  /** 已经应用出去的那份 */
  let done = "";
  let frame = 0;

  function cancelFrame(): void {
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  }

  return {
    schedule(markdown: string): void {
      wanted = markdown;
      if (done === markdown) {
        cancelFrame();
        return;
      }
      const cached = peekMarkdown(markdown);
      if (cached !== null) {
        cancelFrame();
        done = markdown;
        apply(cached);
        return;
      }
      if (markdown.length <= DEFER_MIN_CHARS) {
        cancelFrame();
        done = markdown;
        apply(renderMarkdown(markdown));
        return;
      }
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => {
          frame = 0;
          // 用触发时最新的文本，不用闭包里那份：等帧期间内容可能又被改了
          done = wanted;
          apply(renderMarkdown(wanted));
        });
      });
    },
    cancel(): void {
      cancelFrame();
      wanted = "";
      done = "";
    }
  };
}

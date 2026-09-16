import { peekMarkdown, renderMarkdown, renderMarkdownFast } from "./markdown";

/**
 * 短于这个长度就一步渲染到位（不拆快速版/完整版两段）。
 *
 * 展开一张卡片有两个成本：一次渲染（marked + DOMPurify [+ KaTeX + highlight.js]）
 * 与两段式替换的一次额外 DOM 写入。短卡片整套渲染本身只要一两毫秒，拆两段反而
 * 多一次无谓的重排；长卡片（代码块、公式、callout 堆在一起）的**装饰**部分能到
 * 几百毫秒，那才值得先给素版、装饰异步跟上。
 */
const TWO_PHASE_MIN_CHARS = 1000;

/**
 * 卡片展开时的 markdown 渲染调度器（TaskCard / DiaryCard 共用）。
 *
 * 演进史：
 * - v0.8.0 之前：折叠态也白跑完整渲染（一屏几百次浪费）；
 * - v0.8.0：只在展开时算，但算在展开那一次同步 flush 里——长卡片「点了没反应」；
 * - v0.8.1：双 rAF 之后再算——点击立刻有反馈了，但反馈是**折叠态占位**，长卡片
 *   表现成「先展开一个空白块、再整页换上来」，顿挫感更差；
 * - 现在：**长卡片也立刻给完整可读的内容**——先同步渲染「快速版」（结构、文字、
 *   代码块、图框全在，只缺代码高亮与公式排版这两个纯装饰的重活），完整版在浏览器
 *   画过一两帧之后补上。用户从头到尾看不到空白块；短卡片与记忆化命中的照旧一步到位。
 *
 * 插图不在这条链上：渲染吃原始 markdown，图片是占位符、由 markdownWire 异步填 src
 * （与 mermaid 同模式），解析进度不再触发整卡重渲。
 */
export function createDeferredMarkdown(apply: (html: string) => void): {
  schedule: (markdown: string, nodeId?: string) => void;
  cancel: () => void;
} {
  /** 最后一次被要求渲染的文本（等帧期间内容可能又被改，升级时以它为准） */
  let wanted = "";
  let wantedNode = "";
  /** 已经应用出去的那份（快速版或完整版） */
  let applied = "";
  /** 已经应用出去、且是完整版的那份 */
  let appliedFull = "";
  /** 当前挂在 DOM 上的 HTML 原文（升级时用来跳过「完整版与快速版逐字节相同」的重写） */
  let appliedHtml = "";
  let frame = 0;

  function cancelFrame(): void {
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  }

  function applyOnce(markdown: string, html: string, full: boolean): void {
    applied = markdown;
    if (full) appliedFull = markdown;
    if (html !== appliedHtml) {
      appliedHtml = html;
      apply(html);
    }
  }

  /** 画过一帧之后再跑完整渲染（单 rAF 的回调仍在当帧绘制之前触发，等于没让出时间） */
  function scheduleUpgrade(): void {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        frame = 0;
        const markdown = wanted;
        const nodeId = wantedNode;
        if (appliedFull === markdown) return;
        // 没有代码块与公式的文档，快速版与完整版逐字节相同——applyOnce 会跳过重写
        applyOnce(markdown, renderMarkdown(markdown, nodeId), true);
      });
    });
  }

  return {
    schedule(markdown: string, nodeId = ""): void {
      wanted = markdown;
      wantedNode = nodeId;
      if (appliedFull === markdown) {
        cancelFrame();
        return;
      }
      const cached = peekMarkdown(markdown, nodeId);
      if (cached !== null) {
        cancelFrame();
        applyOnce(markdown, cached, true);
        return;
      }
      if (markdown.length <= TWO_PHASE_MIN_CHARS) {
        cancelFrame();
        applyOnce(markdown, renderMarkdown(markdown, nodeId), true);
        return;
      }
      if (applied !== markdown) {
        applyOnce(markdown, renderMarkdownFast(markdown, nodeId), false);
      }
      scheduleUpgrade();
    },
    cancel(): void {
      cancelFrame();
      wanted = "";
      wantedNode = "";
      applied = "";
      appliedFull = "";
      appliedHtml = "";
    }
  };
}

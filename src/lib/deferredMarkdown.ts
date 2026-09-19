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
  /** 声明「这份文本的 DOM 已经就位」（勾选的手术式更新，见 markdown.setRenderedTaskBox） */
  adopt: (markdown: string, nodeId?: string) => void;
  cancel: () => void;
} {
  /** 最后一次被要求渲染的文本（等帧期间内容可能又被改，升级时以它为准） */
  let wanted = "";
  let wantedNode = "";
  /** 已经应用出去的那份（快速版或完整版）。
   *  键是**文本 + 节点**而不是只有文本：同一份正文被搬到别的条目下时，本地图的解析
   *  结果完全不同（`renderMarkdown` 的 `transformLocalImages` 按 nodeId 找文件），
   *  只比文本会让搬走的卡片一直挂着指向旧节点的 `data-md-img` 占位与图片地址
   *  （v0.8.3 review #5：随后跑「释放空间」，那些图会被当孤儿真删掉）。 */
  let applied = "";
  /** 已经应用出去、且是完整版的那份 */
  let appliedFull = "";
  /** 当前挂在 DOM 上的 HTML 原文（升级时用来跳过「完整版与快速版逐字节相同」的重写） */
  let appliedHtml = "";
  let frame = 0;

  const keyOf = (markdown: string, nodeId: string): string => `${nodeId}\u0000${markdown}`;

  function cancelFrame(): void {
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  }

  function applyOnce(markdown: string, nodeId: string, html: string, full: boolean): void {
    applied = keyOf(markdown, nodeId);
    if (full) appliedFull = applied;
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
        if (appliedFull === keyOf(markdown, nodeId)) return;
        // 没有代码块与公式的文档，快速版与完整版逐字节相同——applyOnce 会跳过重写
        applyOnce(markdown, nodeId, renderMarkdown(markdown, nodeId), true);
      });
    });
  }

  return {
    schedule(markdown: string, nodeId = ""): void {
      wanted = markdown;
      wantedNode = nodeId;
      const key = keyOf(markdown, nodeId);
      if (appliedFull === key) {
        cancelFrame();
        return;
      }
      const cached = peekMarkdown(markdown, nodeId);
      if (cached !== null) {
        cancelFrame();
        applyOnce(markdown, nodeId, cached, true);
        return;
      }
      if (markdown.length <= TWO_PHASE_MIN_CHARS) {
        cancelFrame();
        applyOnce(markdown, nodeId, renderMarkdown(markdown, nodeId), true);
        return;
      }
      if (applied !== key) {
        applyOnce(markdown, nodeId, renderMarkdownFast(markdown, nodeId), false);
      }
      scheduleUpgrade();
    },
    /**
     * 声明「这份文本的 DOM 已经就位」：勾选的手术式更新只改了局部 DOM、跳过整篇重渲，
     * 渲染器的账本必须跟着改——否则将来文本**回退到更早的那一份**时（写失败回滚 /
     * 远端同步 / 编辑器改写），`applied`/`appliedFull` 还停在旧键上，`schedule` 会
     * 以为「已经渲过」而跳过，界面留在手术后的状态、与源码永久分叉。
     * `appliedHtml` 置空：手术结果与整篇重渲等价（有单测钉住），但它的字节不等于
     * 任何一份渲染产物，下一次 apply 必须真写。
     */
    adopt(markdown: string, nodeId = ""): void {
      applied = keyOf(markdown, nodeId);
      appliedFull = applied;
      appliedHtml = "";
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

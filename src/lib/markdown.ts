//! markdown 渲染扩展：callout（ksimple 同款语法/样式）、数学公式（KaTeX 本地）、
//! front-matter 美化、代码块折叠、mermaid/markmap 代码块 → 图（本地渲染，不走网络）。
//!
//! 顺序很重要：先摘代码（保护 `$`/`>` 不被数学与 callout 误吃）→ 摘数学 → 还原代码 →
//! marked（含 callout 扩展）→ 高亮 → 代码块/图框包装 → DOMPurify → 回填数学。
//! mermaid/markmap 是异步库，渲染阶段只产出占位框（源码放在 data-source 里），
//! 由 markdownControls 的 action 在挂载/更新后异步填进去。

import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dart from "highlight.js/lib/languages/dart";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import graphql from "highlight.js/lib/languages/graphql";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import lua from "highlight.js/lib/languages/lua";
import makefile from "highlight.js/lib/languages/makefile";
import markdownLanguage from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import r from "highlight.js/lib/languages/r";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import shell from "highlight.js/lib/languages/shell";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import katex from "katex";
import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
import { ADMONITION_ICONS, ADMONITION_ICON_BY_TYPE } from "./admonitionIcons";

marked.use({
  gfm: true,
  breaks: true
});

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("csharp", csharp);
hljs.registerLanguage("css", css);
hljs.registerLanguage("dart", dart);
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("dockerfile", dockerfile);
hljs.registerLanguage("go", go);
hljs.registerLanguage("graphql", graphql);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("java", java);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("kotlin", kotlin);
hljs.registerLanguage("lua", lua);
hljs.registerLanguage("makefile", makefile);
hljs.registerLanguage("markdown", markdownLanguage);
hljs.registerLanguage("md", markdownLanguage);
hljs.registerLanguage("php", php);
hljs.registerLanguage("powershell", powershell);
hljs.registerLanguage("ps1", powershell);
hljs.registerLanguage("python", python);
hljs.registerLanguage("py", python);
hljs.registerLanguage("r", r);
hljs.registerLanguage("ruby", ruby);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("rs", rust);
hljs.registerLanguage("shell", shell);
hljs.registerLanguage("sh", shell);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("toml", ini);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("yml", yaml);

export function escapeHtml(raw: string): string {
  return raw
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function unescapeHtml(raw: string): string {
  return raw
    .replace(/&#39;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

// ---------------------------------------------------------------------------
// callout（GitHub 告警语法，ksimple 一比一：> [!type] 标题）
// ---------------------------------------------------------------------------

interface AdmonitionToken extends Tokens.Generic {
  type: "admonition";
  raw: string;
  advType: string;
  title: string;
  tokens: Tokens.Generic[];
}

/** marked 的扩展回调里 this 才带 lexer/parser，类型声明没暴露，局部 cast 取用。 */
type ExtensionThis = {
  lexer: { blockTokens: (text: string) => Tokens.Generic[] };
  parser: { parse: (tokens: Tokens.Generic[]) => string };
};

const admonitionExtension = {
  name: "admonition",
  level: "block" as const,
  start(src: string): number | undefined {
    const match = /> *\[!/m.exec(src);
    return match ? match.index : undefined;
  },
  tokenizer(this: ExtensionThis, src: string): AdmonitionToken | undefined {
    const match = /^ {0,3}> *\[!([a-zA-Z]+)\] *([^\n]*)\n((?: {0,3}>[^\n]*\n?)*)/.exec(src);
    if (!match) return undefined;
    const inner = match[3].replace(/^ {0,3}> ?/gm, "");
    return {
      type: "admonition",
      raw: match[0],
      advType: match[1].toLowerCase(),
      title: match[2].trim(),
      tokens: this.lexer.blockTokens(inner)
    };
  },
  renderer(this: ExtensionThis, token: AdmonitionToken): string {
    const known = ADMONITION_ICON_BY_TYPE[token.advType] ? token.advType : "note";
    const iconKey = ADMONITION_ICON_BY_TYPE[known] ?? "file-pen-solid";
    const icon = ADMONITION_ICONS[iconKey];
    const svg = icon
      ? `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${icon.viewBox}" aria-hidden="true">${icon.body}</svg>`
      : "";
    const title = token.title || known;
    const body = this.parser.parse(token.tokens);
    return (
      `<div class="admonition ${known}">` +
      `<div class="admonition-header">${svg}<span>${escapeHtml(title)}</span></div>` +
      `<div class="admonition-content">${body}</div>` +
      `</div>`
    );
  }
};

marked.use({ extensions: [admonitionExtension as unknown as TokenizerAndRendererExtension] });

// ---------------------------------------------------------------------------
// front-matter：开头 --- 块渲染成键值表，不再露出两行 ---
// ---------------------------------------------------------------------------

function splitFrontMatter(markdown: string): { fields: Array<[string, string]>; rest: string } {
  const lines = markdown.split(/\r?\n/);
  if (lines[0]?.trim() !== "---") return { fields: [], rest: markdown };
  const end = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  if (end < 0) return { fields: [], rest: markdown };
  const fields: Array<[string, string]> = [];
  for (const line of lines.slice(1, end)) {
    const at = line.indexOf(":");
    if (at <= 0) continue;
    const key = line.slice(0, at).trim();
    const value = line.slice(at + 1).trim().replace(/^["']|["']$/g, "");
    if (key) fields.push([key, value]);
  }
  return { fields, rest: lines.slice(end + 1).join("\n") };
}

function frontMatterHtml(fields: Array<[string, string]>): string {
  if (fields.length === 0) return "";
  const rows = fields
    .map(
      ([key, value]) =>
        `<div class="fm-row"><span class="fm-key">${escapeHtml(key)}</span><span class="fm-value">${escapeHtml(value)}</span></div>`
    )
    .join("");
  return `<div class="front-matter">${rows}</div>`;
}

// ---------------------------------------------------------------------------
// 数学公式：先摘出来（保护代码），sanitize 之后用 KaTeX 回填
// ---------------------------------------------------------------------------

const MATH_TOKEN = (kind: "b" | "i", index: number): string => `@@KXMath${kind}${index}@@`;

function extractMath(markdown: string): { text: string; blocks: string[]; inlines: string[] } {
  const blocks: string[] = [];
  const inlines: string[] = [];
  // 块级 $$...$$（可跨行）
  let text = markdown.replace(/\$\$([\s\S]+?)\$\$/g, (_m, body: string) => {
    blocks.push(body.trim());
    return MATH_TOKEN("b", blocks.length - 1);
  });
  // 内联 $...$：两边不贴空白、内部不含换行，避免吃掉「$100 到 $200」这类普通文字
  text = text.replace(/\$([^$\n]+?)\$/g, (m, body: string) => {
    if (body.trim().length === 0 || /^\s|\s$/.test(body)) return m;
    inlines.push(body.trim());
    return MATH_TOKEN("i", inlines.length - 1);
  });
  return { text, blocks, inlines };
}

function renderMathToken(src: string, displayMode: boolean): string {
  try {
    const html = katex.renderToString(src, { displayMode, throwOnError: false, strict: false });
    return displayMode ? `<div class="kx-math-block">${html}</div>` : html;
  } catch {
    return escapeHtml(src);
  }
}

function restoreMath(html: string, blocks: string[], inlines: string[]): string {
  let out = html;
  blocks.forEach((src, index) => {
    out = out.split(MATH_TOKEN("b", index)).join(renderMathToken(src, true));
  });
  inlines.forEach((src, index) => {
    out = out.split(MATH_TOKEN("i", index)).join(renderMathToken(src, false));
  });
  return out;
}

// ---------------------------------------------------------------------------
// 代码保护 / 还原
// ---------------------------------------------------------------------------

function protectCode(markdown: string): { text: string; pieces: string[] } {
  const pieces: string[] = [];
  let text = markdown.replace(/```[\s\S]*?```/g, (m) => {
    pieces.push(m);
    return `\u0000KXCode${pieces.length - 1}\u0000`;
  });
  text = text.replace(/`[^`\n]+`/g, (m) => {
    pieces.push(m);
    return `\u0000KXCode${pieces.length - 1}\u0000`;
  });
  return { text, pieces };
}

function restoreCode(text: string, pieces: string[]): string {
  let out = text;
  pieces.forEach((piece, index) => {
    out = out.split(`\u0000KXCode${index}\u0000`).join(piece);
  });
  return out;
}

// ---------------------------------------------------------------------------
// 高亮 / 代码块包装 / 图框
// ---------------------------------------------------------------------------

function highlightCodeBlocks(html: string): string {
  if (typeof document === "undefined") return html;
  const template = document.createElement("template");
  template.innerHTML = html;
  template.content.querySelectorAll("pre code").forEach((block) => {
    const language = [...block.classList]
      .find((className) => className.startsWith("language-"))
      ?.replace("language-", "");
    const source = block.textContent ?? "";
    const highlighted =
      language && hljs.getLanguage(language)
        ? hljs.highlight(source, { language, ignoreIllegals: true }).value
        : hljs.highlightAuto(source).value;
    block.innerHTML = highlighted;
    block.classList.add("hljs");
  });
  return template.innerHTML;
}

const DIAGRAM_LANGS = ["mermaid", "markmap"];
const CODE_COLLAPSE_LINES = 18;

/**
 * diagram 源码进 data-source 前必须 base64：DOMPurify 会剥掉值里含 `-->` 的属性
 * （mermaid 的箭头必中），base64 是纯 ASCII，不会踩任何序列化坑。
 */
export function encodeDiagramSource(source: string): string {
  return btoa(unescape(encodeURIComponent(source)));
}

export function decodeDiagramSource(encoded: string): string {
  try {
    return decodeURIComponent(escape(atob(encoded)));
  } catch {
    return "";
  }
}

/** ```mermaid / ```markmap 代码块 → 图框（源码进 data-source，异步渲染见 markdownControls）。 */
function transformDiagrams(html: string): string {
  return html.replace(
    /<pre><code class="language-(mermaid|markmap)[^"]*">([\s\S]*?)<\/code><\/pre>/g,
    (_m, lang: string, escaped: string) => {
      const source = unescapeHtml(escaped).replace(/\n$/, "");
      const encoded = encodeDiagramSource(source);
      return (
        `<div class="diagram-frame" data-diagram="${lang}">` +
        `<div class="diagram-toolbar">` +
        `<span class="diagram-lang">${lang}</span>` +
        (lang === "mermaid"
          ? `<button type="button" class="diagram-btn diagram-zoom-out" title="缩小">−</button>` +
            `<button type="button" class="diagram-btn diagram-zoom-in" title="放大">＋</button>` +
            `<button type="button" class="diagram-btn diagram-reset" title="还原">1:1</button>`
          : "") +
        `<button type="button" class="diagram-btn diagram-source-toggle" title="查看/收起源码">源码</button>` +
        `<button type="button" class="diagram-btn diagram-fullscreen" title="全屏查看">全屏</button>` +
        `</div>` +
        `<div class="diagram-canvas" data-diagram="${lang}" data-source="${encoded}"><span class="diagram-loading">渲染中…</span></div>` +
        `<pre class="diagram-source" hidden><code>${escapeHtml(source)}</code></pre>` +
        `</div>`
      );
    }
  );
}

/** 其余代码块加语言条 + 复制 + 折叠（超过阈值默认折起）。 */
function transformCodeBlocks(html: string): string {
  return html.replace(/<pre><code class="([^"]*)">([\s\S]*?)<\/code><\/pre>/g, (m, classes: string, body: string) => {
    if (classes.includes("language-mermaid") || classes.includes("language-markmap")) return m;
    const language = classes
      .split(/\s+/)
      .find((className) => className.startsWith("language-"))
      ?.replace("language-", "");
    const lines = (unescapeHtml(body).match(/\n/g)?.length ?? 0) + 1;
    const collapsible = lines > CODE_COLLAPSE_LINES;
    return (
      `<div class="code-block${collapsible ? " code-collapsible collapsed" : ""}">` +
      `<div class="code-bar">` +
      `<span class="code-lang">${escapeHtml(language || "text")}</span>` +
      `<button type="button" class="code-btn code-copy" title="复制代码">复制</button>` +
      (collapsible ? `<button type="button" class="code-btn code-toggle" title="展开/收起代码">展开</button>` : "") +
      `</div>` +
      `<pre><code class="${classes}">${body}</code></pre>` +
      `</div>`
    );
  });
}

const SANITIZE_OPTIONS = {
  ADD_TAGS: ["mark", "input"],
  ADD_ATTR: ["target", "rel", "src", "type", "checked", "disabled", "hidden", "viewBox"],
  ALLOW_UNKNOWN_PROTOCOLS: true
};

function applyHighlights(markdown: string): string {
  return markdown.replace(/==([^=\n][\s\S]*?[^=\n])==/g, "<mark>$1</mark>");
}

/** 完整渲染：front-matter / callout / 公式 / 代码折叠 / 图框占位。 */
export function renderMarkdown(markdown: string): string {
  const normalized = markdown.trim().length > 0 ? markdown : "添加任务";
  const { fields, rest } = splitFrontMatter(normalized);
  const protectedCode = protectCode(rest);
  const math = extractMath(protectedCode.text);
  const withCodeBack = restoreCode(math.text, protectedCode.pieces);
  const raw = marked.parse(applyHighlights(withCodeBack), { async: false }) as string;
  const diagrammed = transformDiagrams(raw);
  const highlighted = highlightCodeBlocks(diagrammed);
  const wrapped = transformCodeBlocks(highlighted);
  const clean = DOMPurify.sanitize(wrapped, SANITIZE_OPTIONS);
  return frontMatterHtml(fields) + restoreMath(clean, math.blocks, math.inlines);
}

export function renderInlineMarkdown(markdown: string): string {
  const protectedCode = protectCode(markdown || "未命名任务");
  const math = extractMath(protectedCode.text);
  const withCodeBack = restoreCode(math.text, protectedCode.pieces);
  const raw = marked.parseInline(applyHighlights(withCodeBack)) as string;
  const clean = DOMPurify.sanitize(raw, SANITIZE_OPTIONS);
  return restoreMath(clean, math.blocks, math.inlines);
}

export function firstMarkdownLine(markdown: string): string {
  const { rest } = splitFrontMatter(markdown);
  const firstLine = rest
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0 && !line.startsWith("```"));

  return firstLine || "未命名任务";
}

export function collapsedMarkdownLine(markdown: string): string {
  return firstMarkdownLine(markdown)
    .replace(/^#{1,6}\s*/, "")
    .replace(/^> *\[![a-zA-Z]+\] */, "");
}

export function hasMultipleMarkdownLines(markdown: string): boolean {
  return markdown.trim().split(/\r?\n/).length > 1;
}

export function markdownTitle(markdown: string): string {
  return collapsedMarkdownLine(markdown)
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/==([^=]+)==/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\$([^$]+)\$/g, "$1");
}

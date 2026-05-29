import DOMPurify from "dompurify";
import { marked } from "marked";

marked.use({
  gfm: true,
  breaks: true
});

function applyHighlights(markdown: string): string {
  return markdown.replace(/==([^=\n][\s\S]*?[^=\n])==/g, "<mark>$1</mark>");
}

export function renderMarkdown(markdown: string): string {
  const normalized = markdown.trim().length > 0 ? markdown : "添加任务";
  const raw = marked.parse(applyHighlights(normalized), { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    ADD_TAGS: ["mark"],
    ADD_ATTR: ["target", "rel"]
  });
}

export function firstMarkdownLine(markdown: string): string {
  const firstLine = markdown
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0);

  return firstLine || "未命名任务";
}

export function markdownTitle(markdown: string): string {
  return firstMarkdownLine(markdown)
    .replace(/^#{1,6}\s*/, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/==([^=]+)==/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
}

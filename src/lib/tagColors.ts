/**
 * 标签配色表（七彩虹 + 灰 + 自定义）。
 *
 * 色值只在这一处定义：色盘圆点、菜单里的胶囊、卡片上的标签全部读这里，
 * 与 `src/styles/workspace.css` 里的 `.task-tag.tag-*` 保持同一组色——
 * 改色调只改这两处（自定义色走内联样式，没有对应类名）。
 */
import type { Tag, TagColor } from "./types";

export type TagColorSpec = {
  color: Exclude<TagColor, "custom">;
  label: string;
  /** 色盘圆点 */
  swatch: string;
  /** 胶囊底色 */
  background: string;
  /** 胶囊描边 */
  border: string;
  /** 胶囊文字 */
  text: string;
};

export const TAG_COLOR_SPECS: TagColorSpec[] = [
  { color: "red", label: "红", swatch: "#d93025", background: "#fce8e8", border: "#f4c3c3", text: "#d93025" },
  { color: "orange", label: "橙", swatch: "#e8710a", background: "#fdf0e3", border: "#f6d3b0", text: "#c2640a" },
  { color: "yellow", label: "黄", swatch: "#f9ab00", background: "#fef7e0", border: "#f5e3a3", text: "#b07000" },
  { color: "green", label: "绿", swatch: "#188038", background: "#e6f4ea", border: "#bce0c6", text: "#137333" },
  { color: "cyan", label: "青", swatch: "#0b7285", background: "#e0f3f6", border: "#b3e0e8", text: "#0b7285" },
  { color: "blue", label: "蓝", swatch: "#1a73e8", background: "#e8f0fe", border: "#c4dafc", text: "#1a73e8" },
  { color: "purple", label: "紫", swatch: "#8430ce", background: "#f3e8fd", border: "#ddc2f5", text: "#8430ce" },
  { color: "pink", label: "粉", swatch: "#d81b60", background: "#fde8f0", border: "#f3c1d4", text: "#c2185b" },
  { color: "gray", label: "灰", swatch: "#5f6368", background: "#f1f3f4", border: "#dadce0", text: "#5f6368" }
];

/** `#rrggbb` 的浅色底：拼一个低透明度的十六进制 alpha（8 位色值，WebView 都认）。 */
export function tagTint(hex: string, alpha = "1f"): string {
  return `${hex}${alpha}`;
}

/**
 * 标签胶囊的内联样式。自定义色没法用类名（颜色是用户选的），
 * 所以统一走 CSS 变量：`--tag-bg` / `--tag-fg` 由样式表的 `.tag-custom` 消费。
 */
export function tagChipStyle(tag: Tag): string {
  if (tag.color !== "custom" || !tag.hex) return "";
  return `--tag-bg: ${tagTint(tag.hex)}; --tag-fg: ${tag.hex};`;
}

/** 「颜色 + 文字」的稳定键：同名同色的标签算同一个（预置标签去重用）。 */
export function tagKey(tag: Pick<Tag, "color" | "text" | "hex">): string {
  return `${tag.color}:${tag.hex ?? ""}:${(tag.text ?? "").trim()}`;
}

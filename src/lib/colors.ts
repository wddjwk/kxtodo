/**
 * 颜色的小工具（v0.9.1）。
 *
 * 独立成模块、不 import 任何项目内文件：日程分类要把用户选的主色摊成「块内淡色底」，
 * 而 `styles.ts` 已经依赖 `defaults.ts`——放进 styles 会让 `calendar.ts` →
 * `styles.ts` → `defaults.ts` → `calendar.ts` 成环。
 */

const HEX_RE = /^#([0-9a-f]{6})$/i;

export type Rgb = { r: number; g: number; b: number };

/** `#rrggbb` → 通道值；认不出来返回 null（调用方自己回落）。 */
export function parseHex(value: unknown): Rgb | null {
  if (typeof value !== "string") return null;
  const match = HEX_RE.exec(value.trim());
  if (!match) return null;
  const int = Number.parseInt(match[1], 16);
  return { r: (int >> 16) & 255, g: (int >> 8) & 255, b: int & 255 };
}

export function isHex(value: unknown): value is string {
  return parseHex(value) !== null;
}

function toHex(rgb: Rgb): string {
  const hex = (value: number): string =>
    Math.max(0, Math.min(255, Math.round(value))).toString(16).padStart(2, "0");
  return `#${hex(rgb.r)}${hex(rgb.g)}${hex(rgb.b)}`;
}

/**
 * 与白色混合：`weight = 1` 是原色、`0` 是纯白。
 * 日程块内那层「不影响阅读的淡色」就是 `mixWithWhite(main, 0.14)`。
 * 混合放在 sRGB 空间——够用于浅底色，也不引入色彩管理的不确定性。
 */
export function mixWithWhite(hex: string, weight: number): string {
  const rgb = parseHex(hex);
  if (!rgb) return "#ffffff";
  const w = Math.max(0, Math.min(1, weight));
  return toHex({
    r: 255 + (rgb.r - 255) * w,
    g: 255 + (rgb.g - 255) * w,
    b: 255 + (rgb.b - 255) * w
  });
}

/** `#rrggbb` → `rgba(r, g, b, alpha)`；认不出来返回透明。 */
export function hexToRgba(hex: string, alpha: number): string {
  const rgb = parseHex(hex);
  if (!rgb) return "rgba(0, 0, 0, 0)";
  const a = Math.max(0, Math.min(1, alpha));
  return `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${a})`;
}

/** WCAG 相对亮度（0 = 黑，1 = 白）。 */
export function luminance(hex: string): number {
  const rgb = parseHex(hex);
  if (!rgb) return 1;
  const channel = (value: number): number => {
    const s = value / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b);
}

/** 在这个底色上该用白字还是深字（对比度 4.5:1 附近切换）。 */
export function readableOn(hex: string): "#ffffff" | "#23272f" {
  return luminance(hex) > 0.55 ? "#23272f" : "#ffffff";
}

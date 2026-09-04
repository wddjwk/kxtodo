import type { AppNode, ListBackground, Settings } from "./types";
import { defaultBackground, defaultSettings } from "./defaults";

const DEFAULT_ACCENT = "#2564cf";

export function escapeCssUrl(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, "%22").replace(/\n/g, "");
}

function isHexColor(value: unknown): value is string {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value);
}

export function defaultAccentForNode(node?: AppNode): string {
  if (!node) return DEFAULT_ACCENT;
  if (node.id === "planned") return "#2564cf";
  if (node.id === "important") return "#9f5f00";
  if (node.id === "my-day") return "#b64a30";
  if (node.id === "scheduled") return "#3f6b5a";
  return DEFAULT_ACCENT;
}

export function accentForNode(node?: AppNode, uiColors: Record<string, string> = {}): string {
  const customColor = node ? uiColors[node.id] : undefined;
  return isHexColor(customColor) ? customColor : defaultAccentForNode(node);
}

export function avatarStyle(avatar: string): string {
  return avatar ? `background-image: url("${escapeCssUrl(avatar)}");` : "";
}

export function avatarInitial(displayName: string): string {
  return (displayName.trim().charAt(0) || "E").toUpperCase();
}

export function uiScaleValue(scaleValue = defaultSettings.appearance.uiScale): number {
  return Math.min(1.5, Math.max(0.5, scaleValue || defaultSettings.appearance.uiScale));
}

export function scalePercentValue(scaleValue = defaultSettings.appearance.uiScale): number {
  return Math.round(uiScaleValue(scaleValue) * 100);
}

export function fontSizeValue(value: number, fallback: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value || fallback)));
}

export function clampNumber(value: number, fallback: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function isNumberInRange(value: number, min: number, max: number): boolean {
  return Number.isFinite(value) && value >= min && value <= max;
}

export function buildAppShellStyle(appearance: Settings["appearance"]): string {
  const scale = uiScaleValue(appearance.uiScale);
  const uiFontSize = fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22);
  const markdownFontSize = fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26);
  const editorFontSize = fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26);
  const tagFontSize = fontSizeValue(appearance.tagFontSize, defaultSettings.appearance.tagFontSize, 11, 30);
  return [
    `--ui-scale: ${scale}`,
    `--ui-font-size: ${uiFontSize}px`,
    `--markdown-font-size: ${markdownFontSize}px`,
    `--editor-font-size: ${editorFontSize}px`,
    `--tag-font-size: ${tagFontSize}px`,
    `--app-width: ${100 / scale}vw`,
    `--app-height: ${100 / scale}vh`,
    `--font-title: ${uiFontSize + 18}px`,
    `--font-list: ${uiFontSize + 1}px`,
    `--font-control: ${uiFontSize}px`,
    `--font-task: ${markdownFontSize}px`,
    `--font-composer: ${markdownFontSize}px`,
    `--font-drawer-title: ${uiFontSize + 6}px`
  ].join("; ");
}

export function buildMobileShellStyle(appearance: Settings["appearance"]): string {
  const scale = uiScaleValue(appearance.uiScale);
  const uiFontSize = fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22);
  const markdownFontSize = fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26);
  const editorFontSize = fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26);
  const tagFontSize = fontSizeValue(appearance.tagFontSize, defaultSettings.appearance.tagFontSize, 11, 30);
  return [
    `--ui-scale: ${scale}`,
    `--ui-font-size: ${uiFontSize}px`,
    `--markdown-font-size: ${markdownFontSize}px`,
    `--editor-font-size: ${editorFontSize}px`,
    `--tag-font-size: ${tagFontSize}px`,
    `--app-width: ${100 / scale}vw`,
    `--app-height: ${100 / scale}vh`,
    /* 安全区补偿系数：shell 被 transform 缩放后，env(safe-area-inset-*) 的物理像素
        clearance 需乘以 1/scale 才能在缩放后的逻辑坐标系里保持实际视觉尺寸。 */
    `--safe-inv: ${1 / scale}`,
    `--font-title: ${uiFontSize + 18}px`,
    `--font-list: ${uiFontSize + 1}px`,
    `--font-control: ${uiFontSize}px`,
    `--font-task: ${markdownFontSize}px`,
    `--font-composer: ${markdownFontSize}px`,
    `--font-drawer-title: ${uiFontSize + 6}px`
  ].join("; ");
}

export function buildSettingsDrawerStyle(appearance: Settings["appearance"]): string {
  const scale = uiScaleValue(appearance.uiScale);
  const viewportWidth = typeof window === "undefined" ? 1280 : window.innerWidth;
  const viewportHeight = typeof window === "undefined" ? 820 : window.innerHeight;
  const drawerWidth = 380;
  const titlebarHeight = 52;
  return [
    `left: ${(viewportWidth - drawerWidth) / scale}px`,
    `top: ${titlebarHeight / scale}px`,
    `width: ${drawerWidth / scale}px`,
    `height: ${(viewportHeight - titlebarHeight) / scale}px`,
    `--ui-scale: ${scale}`,
    `--ui-font-size: ${fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22)}px`,
    `--markdown-font-size: ${fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26)}px`,
    `--editor-font-size: ${fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26)}px`
  ].join("; ");
}

export function buildMainStyle(background: ListBackground, accentColor: string, resolvedImage = ""): string {
  const source = resolvedImage || (background.image && !background.image.startsWith("img:") ? background.image : "");
  const image = source ? `url("${escapeCssUrl(source)}")` : "none";
  const opacity = source ? background.imageOpacity ?? defaultBackground.imageOpacity ?? 0.28 : 0;
  return `--accent: ${accentColor}; --bg-image: ${image}; --bg-opacity: ${opacity}; background: ${background.color};`;
}


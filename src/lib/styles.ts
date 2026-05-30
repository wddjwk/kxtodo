import type { AppNode, ListBackground, Settings } from "./types";
import { defaultBackground, defaultSettings } from "./defaults";

const DEFAULT_ACCENT = "#2564cf";

export function escapeCssUrl(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, "%22").replace(/\n/g, "");
}

export function accentForNode(node?: AppNode): string {
  if (!node) return DEFAULT_ACCENT;
  if (node.id === "planned") return "#2564cf";
  if (node.id === "important") return "#9f5f00";
  if (node.id === "my-day") return "#b64a30";
  return DEFAULT_ACCENT;
}

export function avatarStyle(avatar: string): string {
  return avatar ? `background-image: url("${escapeCssUrl(avatar)}");` : "";
}

export function avatarInitial(displayName: string): string {
  return (displayName.trim().charAt(0) || "E").toUpperCase();
}

export function uiScaleValue(scaleValue = defaultSettings.appearance.uiScale): number {
  const staleScale = scaleValue === 0.62 || scaleValue === 0.72 || scaleValue === 0.86 || scaleValue === 0.92;
  const normalizedScale = staleScale ? defaultSettings.appearance.uiScale : scaleValue;
  return Math.min(1.5, Math.max(0.5, normalizedScale || defaultSettings.appearance.uiScale));
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
  return [
    `--ui-scale: ${scale}`,
    `--ui-font-size: ${uiFontSize}px`,
    `--markdown-font-size: ${markdownFontSize}px`,
    `--editor-font-size: ${editorFontSize}px`,
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

export function buildMainStyle(background: ListBackground, accentColor: string): string {
  const image = background.image ? `url("${escapeCssUrl(background.image)}")` : "none";
  const opacity = background.image ? background.imageOpacity ?? defaultBackground.imageOpacity ?? 0.28 : 0;
  return `--accent: ${accentColor}; --bg-image: ${image}; --bg-opacity: ${opacity}; background: ${background.color};`;
}

export function buildMenuStyle(clientX: number, clientY: number, width: number, height: number, scale: number): string {
  const viewportWidth = typeof window === "undefined" ? 1200 : window.innerWidth / scale;
  const viewportHeight = typeof window === "undefined" ? 800 : window.innerHeight / scale;
  const left = Math.max(8, Math.min(clientX / scale, viewportWidth - width - 10));
  const top = Math.max(8, Math.min(clientY / scale, viewportHeight - height - 10));
  return `left: ${left}px; top: ${top}px;`;
}

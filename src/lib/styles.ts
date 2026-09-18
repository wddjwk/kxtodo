import type { AppNode, ListBackground, Settings } from "./types";
import type { ImeViewport } from "./imeViewport";
import { defaultBackground, defaultSettings } from "./defaults";

const DEFAULT_ACCENT = "#2564cf";

/**
 * 日记不属于任何条目，`accentForNode` 给不出颜色，用它自己的主题色。
 * 墨蓝偏灰：和四个系统视图的主题色都不撞，也压得住默认的纸色背景。
 */
export const DIARY_ACCENT = "#4f5d8a";

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

/** 日记的主题色：设置里空着就用默认日记色（空串进 CSS 变量会把整块配色打没）。 */
export function diaryAccent(diary: Settings["diary"]): string {
  return isHexColor(diary.accent) ? diary.accent : DIARY_ACCENT;
}

/** 日记的背景：与条目背景同一个 ListBackground 形状，于是能直接喂给 buildMainStyle。 */
export function diaryBackground(diary: Settings["diary"]): ListBackground {
  return {
    color: isHexColor(diary.backgroundColor) ? diary.backgroundColor : defaultBackground.color,
    image: diary.backgroundImage || undefined,
    imageOpacity: diary.backgroundOpacity
  };
}

/**
 * 记账的主题色：账本绿。与日记的墨蓝、四个系统视图的主题色都错开，
 * 也和「收入绿」同族——记账界面一眼就该是钱的颜色。
 */
export const LEDGER_ACCENT = "#2f8f6b";

/** 记账的主题色：设置里空着就用默认记账色。 */
export function ledgerAccent(ledger: Settings["ledger"]): string {
  return isHexColor(ledger.accent) ? ledger.accent : LEDGER_ACCENT;
}

/** 记账的背景：与条目背景同一个 ListBackground 形状。 */
export function ledgerBackground(ledger: Settings["ledger"]): ListBackground {
  return {
    color: isHexColor(ledger.backgroundColor) ? ledger.backgroundColor : defaultBackground.color,
    image: ledger.backgroundImage || undefined,
    imageOpacity: ledger.backgroundOpacity
  };
}

/** 工具箱的主题色（v0.8.4）：设置里空着就用默认主题色。 */
export function toolboxAccent(toolbox: Settings["toolbox"]): string {
  return isHexColor(toolbox.accent) ? toolbox.accent : DEFAULT_ACCENT;
}

/** 工具箱的背景色（v0.8.4）：只有颜色，没有图（工具页三点菜单就这一个入口）。 */
export function toolboxBackground(toolbox: Settings["toolbox"]): ListBackground {
  return {
    color: isHexColor(toolbox.backgroundColor) ? toolbox.backgroundColor : defaultBackground.color
  };
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

/**
 * 语义字号变量：--font-* 是各区域实际吃的值。记账与日记各有一个自己的字号
 * （两页信息密度不同，跟着 UI 字号一起动并不合适），其余区域仍走 --font-control。
 */
function fontVars(appearance: Settings["appearance"]): string[] {
  const uiFontSize = fontSizeValue(appearance.uiFontSize, defaultSettings.appearance.uiFontSize, 14, 22);
  const markdownFontSize = fontSizeValue(appearance.markdownFontSize, defaultSettings.appearance.markdownFontSize, 14, 26);
  const ledgerFontSize = fontSizeValue(appearance.ledgerFontSize, defaultSettings.appearance.ledgerFontSize, 14, 26);
  const diaryFontSize = fontSizeValue(appearance.diaryFontSize, defaultSettings.appearance.diaryFontSize, 14, 26);
  return [
    `--ui-font-size: ${uiFontSize}px`,
    `--markdown-font-size: ${markdownFontSize}px`,
    `--ledger-font-size: ${ledgerFontSize}px`,
    `--diary-font-size: ${diaryFontSize}px`,
    `--font-title: ${uiFontSize + 18}px`,
    `--font-list: ${uiFontSize + 1}px`,
    `--font-control: ${uiFontSize}px`,
    `--font-task: ${markdownFontSize}px`,
    `--font-composer: ${markdownFontSize}px`,
    `--font-ledger: ${ledgerFontSize}px`,
    `--font-diary: ${diaryFontSize}px`,
    `--font-drawer-title: ${uiFontSize + 6}px`
  ];
}

export function buildAppShellStyle(appearance: Settings["appearance"]): string {
  const scale = uiScaleValue(appearance.uiScale);
  const editorFontSize = fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26);
  const tagFontSize = fontSizeValue(appearance.tagFontSize, defaultSettings.appearance.tagFontSize, 11, 30);
  return [
    `--ui-scale: ${scale}`,
    `--editor-font-size: ${editorFontSize}px`,
    `--tag-font-size: ${tagFontSize}px`,
    `--app-width: ${100 / scale}vw`,
    `--app-height: ${100 / scale}vh`,
    ...fontVars(appearance)
  ].join("; ");
}

/**
 * 移动端 shell 的尺寸变量。
 *
 * `ime` 是输入法把可见区域压小后的视觉视口（见 imeViewport.ts）：键盘弹起时
 * 布局视口（100vh）并不缩，shell 会把「添加事项」输入框留在键盘底下、浏览器
 * 只能把整个视觉视口往上顶。这时把 shell 收矮到可见区域（高度除以 uiScale 换回
 * 逻辑像素）并跟随那次平移，页面就没有可顶的东西了。
 */
export function buildMobileShellStyle(
  appearance: Settings["appearance"],
  ime: ImeViewport = { active: false, height: 0, offset: 0 }
): string {
  const scale = uiScaleValue(appearance.uiScale);
  const editorFontSize = fontSizeValue(appearance.editorFontSize, defaultSettings.appearance.editorFontSize, 14, 26);
  const tagFontSize = fontSizeValue(appearance.tagFontSize, defaultSettings.appearance.tagFontSize, 11, 30);
  const lines = [
    `--ui-scale: ${scale}`,
    `--editor-font-size: ${editorFontSize}px`,
    `--tag-font-size: ${tagFontSize}px`,
    `--app-width: ${100 / scale}vw`,
    `--app-height: ${ime.active ? ime.height / scale : 100 / scale}${ime.active ? "px" : "vh"}`,
    /* 安全区补偿系数：shell 被 transform 缩放后，env(safe-area-inset-*) 的物理像素
        clearance 需乘以 1/scale 才能在缩放后的逻辑坐标系里保持实际视觉尺寸。 */
    `--safe-inv: ${1 / scale}`,
    ...fontVars(appearance)
  ];
  // 平移量是布局视口里的 CSS 像素：父容器（#app）没有缩放，直接写 offset。
  // 必须把 scale 一起写回来（内联 transform 会覆盖 base.css 里那条）。
  if (ime.active && ime.offset > 0) {
    lines.push(`transform: translateY(${ime.offset}px) scale(${scale})`);
  }
  return lines.join("; ");
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


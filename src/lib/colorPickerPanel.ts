/**
 * 取色盘的**全局单例请求**（v0.8.6 需求 11）。
 *
 * 为什么是单例而不是「每个入口内联一个面板」：取色盘要能在任意宿主里浮出来——
 * 菜单里（会被限高裁剪）、卡片里（会被 overflow 裁）、编辑器里——而面板本体
 * 还得挂进 `.app-shell` 才能拿到 `--safe-inv`（安卓安全区）。全应用同时只可能
 * 有一个色盘开着，宿主只声明「要取色 + 三个回调用它做什么」：
 *
 * - `onPreview`：拖动/输入时实时推草稿预览（消费端自己调 `colorPreview` 那一套）
 * - `onConfirm`：点「确认」才落盘（永远只有这一条写路径）
 * - `onCancel`：点「取消」/点浮层外 / Esc —— 丢弃草稿、预览回退
 *
 * 面板本体在 `ColorPickerPanel.svelte`（App 里挂一次），它只认这个 store。
 */
import { get, writable } from "svelte/store";

export type ColorPickRequest = {
  /** 谁在取色（诊断与测试用，比如 "list-ui-color" / "toolbox-background"） */
  key: string;
  /** 初始颜色（#rrggbb） */
  color: string;
  /** 定位锚（取色入口按钮）；null = 用调用方给的坐标 */
  anchor: HTMLElement | null;
  /** 预览：拖动 / 手输时实时调用（**只改草稿，不落盘**） */
  onPreview: (color: string) => void;
  /** 确认：落盘 */
  onConfirm: (color: string) => void;
  /** 取消 / 点浮层外 / Esc：草稿作废、预览回退 */
  onCancel: () => void;
};

export const colorPickRequest = writable<ColorPickRequest | null>(null);

/** 取色盘是否开着（供「菜单关闭要连色盘一起收」之类的宿主查询）。 */
export function isColorPickerOpen(): boolean {
  return get(colorPickRequest) !== null;
}

export function openColorPicker(request: ColorPickRequest): void {
  // 换一个入口取色：先把上一份草稿作废（与「点别的日期先关旧再开新」同一条纪律）
  const current = get(colorPickRequest);
  if (current && current.key !== request.key) current.onCancel();
  colorPickRequest.set(request);
}

/** 取消：作废草稿并把面板收起来（确认路径由面板自己调 confirmColorPicker）。 */
export function cancelColorPicker(): void {
  const current = get(colorPickRequest);
  colorPickRequest.set(null);
  current?.onCancel();
}

/** 确认：先落盘再收面板。 */
export function confirmColorPicker(color: string): void {
  const current = get(colorPickRequest);
  colorPickRequest.set(null);
  current?.onConfirm(color);
}

/**
 * 取色盘的调试出口（与 `window.__kxtodoSearch` 同款）：性能回归要断言
 * 「拖动选色按 rAF 合帧」——预览写入次数不能随事件数线性增长。
 */
export type ColorPickerDebug = { previewWrites: number; previewEvents: number };

export const colorPickerDebug: ColorPickerDebug = { previewWrites: 0, previewEvents: 0 };

export function publishColorPickerDebug(): void {
  if (typeof window === "undefined") return;
  (window as Window & { __kxtodoColorPick?: ColorPickerDebug }).__kxtodoColorPick = { ...colorPickerDebug };
}

export function resetColorPickerDebug(): void {
  colorPickerDebug.previewEvents = 0;
  colorPickerDebug.previewWrites = 0;
  publishColorPickerDebug();
}

// ---------------------------------------------------------------------------
// RGB / HEX 的输入校验（纯函数，有单测）
// ---------------------------------------------------------------------------

/**
 * 手输的 HEX → 归一化的 `#rrggbb`；不合法返回 null（**不写入、给行内提示**）。
 * 三位简写按 CSS 规则展开（`#abc` → `#aabbcc`），大小写统一成小写。
 */
export function normalizeHexInput(value: string): string | null {
  const match = /^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.exec(value.trim());
  if (!match) return null;
  let hex = match[1].toLowerCase();
  if (hex.length === 3) hex = hex.split("").map((char) => char + char).join("");
  return `#${hex}`;
}

/** 手输的 RGB 分量 → 0–255 整数；不合法（非数字/越界）返回 null。 */
export function parseChannelInput(value: string): number | null {
  const trimmed = value.trim();
  if (!/^\d{1,3}$/.test(trimmed)) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return parsed >= 0 && parsed <= 255 ? parsed : null;
}

/** HEX 与 RGB 的互转（双向联动的胶水），非法输入一律 null。 */
export function hexToRgbInput(hex: string): { r: number; g: number; b: number } | null {
  const normalized = normalizeHexInput(hex);
  if (!normalized) return null;
  return {
    r: Number.parseInt(normalized.slice(1, 3), 16),
    g: Number.parseInt(normalized.slice(3, 5), 16),
    b: Number.parseInt(normalized.slice(5, 7), 16)
  };
}

export function rgbToHexInput(r: number, g: number, b: number): string | null {
  if ([r, g, b].some((channel) => !Number.isInteger(channel) || channel < 0 || channel > 255)) return null;
  return `#${[r, g, b].map((channel) => channel.toString(16).padStart(2, "0")).join("")}`;
}

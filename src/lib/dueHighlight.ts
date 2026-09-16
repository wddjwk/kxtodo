/**
 * 临期高亮（v0.8.1）：离到期越近，卡片颜色越重。
 *
 * 纯逻辑单独成模块（不 import 任何 stores / DOM），为的是能跑单元测试——
 * 分档、渐变插值、自定义配色三件事都是「算错了也看不出来，但用户会觉得颜色乱」的类型。
 *
 * 档位：
 * - `today`（今天到期）→ 用户配的第一个色（默认红）；
 * - `tomorrow`／`after`（明天／后天）→ 第二、三个色（默认是红色的两个渐浅档）；
 * - `none` → 不画高亮。
 *
 * 两种模式（特性开关里二选一）：
 * - `solid`：按档位取整色，今天就是今天色、明天就是明天色；
 * - `gradient`：**按实际剩余时间在三个锚点之间线性插值**。锚点是「今天 0 点 / 明天 0 点 /
 *   后天 0 点」——晚上到期的今天事项已经贴近明天色，凌晨到期的明天事项还几乎全是今天色，
 *   比按档跳变更符合「越近越重」的直觉。
 *
 * 另外：**设了具体时刻且不足 5 小时**的，用加重档（`strong`）——底色更实、描边更粗，
 * 一眼能在整页里挑出来；整色模式下还会把颜色本身压深一档。
 */
import type { Settings, Task } from "./types";

export type DueBucket = "today" | "tomorrow" | "after" | "none";

/** 默认配色：红 → 橙黄 → 浅黄（两个渐浅档），与「今天/明天/后天」对应 */
export const DEFAULT_DUE_COLORS = ["#d93025", "#f29900", "#f7cf6b"];

/** 不足这个时长（毫秒）且设了时刻 → 加重档 */
const STRONG_WINDOW_MS = 5 * 60 * 60 * 1000;

export type DueHighlight = {
  bucket: Exclude<DueBucket, "none">;
  /** 是否落在「5 小时内」的加重档 */
  strong: boolean;
  /** 最终展示色（`#rrggbb`） */
  color: string;
};

// ---------------------------------------------------------------------------
// 颜色
// ---------------------------------------------------------------------------

function clampByte(value: number): number {
  return Math.min(255, Math.max(0, Math.round(value)));
}

/** `#rrggbb` → [r,g,b]；非法回 null。 */
export function parseHexColor(hex: string): [number, number, number] | null {
  const raw = hex.trim().replace(/^#/, "");
  if (!/^[0-9a-f]{6}$/i.test(raw)) return null;
  return [
    Number.parseInt(raw.slice(0, 2), 16),
    Number.parseInt(raw.slice(2, 4), 16),
    Number.parseInt(raw.slice(4, 6), 16)
  ];
}

export function toHexColor(rgb: [number, number, number]): string {
  return `#${rgb.map((part) => clampByte(part).toString(16).padStart(2, "0")).join("")}`;
}

/** 两色之间按 `t`（0~1）线性插值 */
export function mixHex(from: string, to: string, t: number): string {
  const a = parseHexColor(from);
  const b = parseHexColor(to);
  if (!a || !b) return from;
  const ratio = Math.min(1, Math.max(0, t));
  return toHexColor([
    a[0] + (b[0] - a[0]) * ratio,
    a[1] + (b[1] - a[1]) * ratio,
    a[2] + (b[2] - a[2]) * ratio
  ]);
}

/** 加重：往黑色压一档（`amount` 0~1，0.18 ≈ 暗两成） */
export function darkenHex(hex: string, amount = 0.18): string {
  const rgb = parseHexColor(hex);
  if (!rgb) return hex;
  return toHexColor([rgb[0] * (1 - amount), rgb[1] * (1 - amount), rgb[2] * (1 - amount)]);
}

/** 提亮：往白色靠一档 */
export function lightenHex(hex: string, amount = 0.5): string {
  const rgb = parseHexColor(hex);
  if (!rgb) return hex;
  return toHexColor([
    rgb[0] + (255 - rgb[0]) * amount,
    rgb[1] + (255 - rgb[1]) * amount,
    rgb[2] + (255 - rgb[2]) * amount
  ]);
}

// ---------------------------------------------------------------------------
// 分档与配色
// ---------------------------------------------------------------------------

/** 本地日期的 0 点时间戳 */
function dayStart(now: Date, offsetDays = 0): number {
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offsetDays, 0, 0, 0, 0);
  return date.getTime();
}

/**
 * 到期时刻。只有日期没有时刻的按当天 **23:59:59** 算——
 * 「今天到期」的语义是今天结束前，不是今天 0 点（否则一过零点就变成逾期了）。
 */
export function dueMoment(task: Pick<Task, "dueDate" | "dueTime">): number | null {
  if (!task.dueDate) return null;
  const [year, month, day] = task.dueDate.slice(0, 10).split("-").map(Number);
  if (!year || !month || !day) return null;
  const time = (task.dueTime ?? "").trim();
  const clock = /^(\d{1,2}):(\d{2})/.exec(time);
  if (clock) {
    return new Date(year, month - 1, day, Number(clock[1]), Number(clock[2]), 0, 0).getTime();
  }
  return new Date(year, month - 1, day, 23, 59, 59, 0).getTime();
}

/** 档位：按到期日与今天的差算（0/1/2 天）；更远的、已过期的、没有日期的都不高亮。 */
export function dueBucketOf(task: Pick<Task, "dueDate" | "dueTime">, now = new Date()): DueBucket {
  const moment = dueMoment(task);
  if (moment === null) return "none";
  const start = dayStart(now);
  if (moment < start) return "none"; // 已逾期：那是另一套语义（计划内分组的「已逾期」），不掺和
  const days = Math.floor((moment - start) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "tomorrow";
  if (days === 2) return "after";
  return "none";
}

/** 用户没配过就用默认三色；配过但长度不足/非法也逐项回退。 */
export function dueColorsOf(colors: string[] | undefined): [string, string, string] {
  return [0, 1, 2].map((index) => {
    const candidate = colors?.[index];
    return candidate && parseHexColor(candidate) ? candidate : DEFAULT_DUE_COLORS[index];
  }) as [string, string, string];
}

/**
 * 算一张卡片的高亮。`mode` 为 `off` 或不该高亮时回 null。
 *
 * `colors` 是这一页（这个节点）自己的三色，缺省用默认。
 */
export function dueHighlightOf(
  task: Pick<Task, "dueDate" | "dueTime">,
  mode: Settings["features"]["dueHighlight"],
  colors: string[] | undefined,
  now = new Date()
): DueHighlight | null {
  if (mode === "off") return null;
  const bucket = dueBucketOf(task, now);
  if (bucket === "none") return null;
  const moment = dueMoment(task)!;
  const palette = dueColorsOf(colors);
  const index = bucket === "today" ? 0 : bucket === "tomorrow" ? 1 : 2;
  // 「不足 5 小时」：只认设了具体时刻的（只有日期的按当天 23:59:59 算，谈不上几小时）
  const hasClock = /^\d{1,2}:\d{2}/.test((task.dueTime ?? "").trim());
  const strong = hasClock && moment - now.getTime() <= STRONG_WINDOW_MS;

  if (mode === "solid") {
    return { bucket, strong, color: strong ? darkenHex(palette[0], 0.16) : palette[index] };
  }

  // 渐变：在「今天 0 点 → 明天 0 点 → 后天 0 点」三锚点之间按实际时间插值。
  // 今天 0 点之前（不可能，bucket 会判 none）与后天 0 点之后都夹到端点。
  // **渐变模式不再叠「5 小时压深」**：插值本身就在表达远近，再叠一层会在 5 小时
  // 那道线上颜色突变一下，反而比不分档更奇怪。`strong` 仍然回给样式层加浓底色与描边。
  const anchor0 = dayStart(now);
  const anchor1 = dayStart(now, 1);
  const anchor2 = dayStart(now, 2);
  const at = Math.min(anchor2, Math.max(anchor0, moment));
  const color =
    at <= anchor1
      ? mixHex(palette[0], palette[1], (at - anchor0) / (anchor1 - anchor0))
      : mixHex(palette[1], palette[2], (at - anchor1) / (anchor2 - anchor1));
  return { bucket, strong, color };
}

/** 卡片上要画的内联样式（`--due-*` 由 workspace.css 的 `.task-card.due-soon` 消费）。 */
export function dueHighlightStyle(highlight: DueHighlight | null): string {
  if (!highlight) return "";
  const tint = highlight.strong ? "2e" : "1c";
  const edge = highlight.strong ? "b3" : "7a";
  return `--due-color: ${highlight.color}; --due-tint: ${highlight.color}${tint}; --due-edge: ${highlight.color}${edge};`;
}

/**
 * 临期高亮（v0.8.1）：离到期越近，卡片颜色越重。
 *
 * 纯逻辑单独成模块（不 import 任何 stores / DOM），为的是能跑单元测试——
 * 分档、渐变插值、自定义配色三件事都是「算错了也看不出来，但用户会觉得颜色乱」的类型。
 *
 * 档位（v0.8.3 起四档，配色数组顺序即菜单色块顺序）：
 * - `overdue`（已过期）→ 第一个色（默认灰）；
 * - `today`（今天到期）→ 第二个色（默认红）；
 * - `tomorrow`／`after`（明天／后天）→ 第三、四个色（默认黄、蓝）；
 * - `none` → 不画高亮。
 *
 * 两种模式（特性开关里三选一：不高亮 / 配色 / 渐变色）：
 * - `solid`（配色）：按档位取整色，今天就是今天色、明天就是明天色；
 * - `gradient`（渐变色）：设了具体时刻的，**按实际剩余时间在三个锚点之间线性插值**。
 *   锚点是「今天 0 点 / 明天 0 点 / 后天 0 点」——晚上到期的今天事项已经贴近明天色，
 *   凌晨到期的明天事项还几乎全是今天色，比按档跳变更符合「越近越重」的直觉。
 *   只有日期没有时刻的不插值（精确不到天以内，插值反而把「今天」推成明天色），
 *   直接按档位取色。
 *
 * 另外：**设了具体时刻且不足 5 小时**的，用加重档（`strong`）——底色更实、描边更粗，
 * 一眼能在整页里挑出来；整色模式下还会把颜色本身压深一档。
 */
import type { Settings, Task } from "./types";

export type DueBucket = "overdue" | "today" | "tomorrow" | "after" | "none";

/** 默认配色：灰（已过期）/ 红（今天）/ 黄（明天）/ 蓝（后天）。
 *  顺序与三点菜单里的色块顺序一致：已过期在「今天」左侧。 */
export const DEFAULT_DUE_COLORS = ["#808080", "#d93025", "#eab308", "#3b82f6"];

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

/** 档位：先分「已过期」，再按到期日与今天的差算（0/1/2 天）；更远的、没有日期的不高亮。 */
export function dueBucketOf(task: Pick<Task, "dueDate" | "dueTime">, now = new Date()): DueBucket {
  const moment = dueMoment(task);
  if (moment === null) return "none";
  const start = dayStart(now);
  // 已过期（v0.8.3 第四档）：默认灰色。只精确到天的任务按当天 23:59:59 算，
  // 所以「今天到期」要跨过午夜才变灰，不会一早就灰掉。
  if (moment < now.getTime()) return "overdue";
  const days = Math.floor((moment - start) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "tomorrow";
  if (days === 2) return "after";
  return "none";
}

/** 用户没配过就用默认四色；配过但长度不足/非法也逐项回退。 */
export function dueColorsOf(colors: string[] | undefined): [string, string, string, string] {
  return [0, 1, 2, 3].map((index) => {
    const candidate = colors?.[index];
    return candidate && parseHexColor(candidate) ? candidate : DEFAULT_DUE_COLORS[index];
  }) as [string, string, string, string];
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
  const index = bucket === "overdue" ? 0 : bucket === "today" ? 1 : bucket === "tomorrow" ? 2 : 3;
  // 已过期不参与「5 小时内加重」与渐变插值：它已经过去了，灰就是它的全部语义
  if (bucket === "overdue") {
    return { bucket, strong: false, color: palette[0] };
  }
  // 「不足 5 小时」：只认设了具体时刻的（只有日期的按当天 23:59:59 算，谈不上几小时）
  const hasClock = /^\d{1,2}:\d{2}/.test((task.dueTime ?? "").trim());
  const strong = hasClock && moment - now.getTime() <= STRONG_WINDOW_MS;

  if (mode === "solid") {
    return { bucket, strong, color: strong ? darkenHex(palette[1], 0.16) : palette[index] };
  }

  // 渐变：**按实际剩余时间在三个锚点之间线性插值**。锚点是「今天 0 点 / 明天 0 点 /
  // 后天 0 点」——今晚到期的今天事项已经贴近明天色，凌晨到期的明天事项还几乎全是
  // 今天色，比按档跳变更符合「越近越重」的直觉。
  // **渐变模式不再叠「5 小时压深」**：插值本身就在表达远近，再叠一层会在 5 小时
  // 那道线上颜色突变一下，反而比不分档更奇怪。`strong` 仍然回给样式层加浓底色与描边。
  //
  // 只有日期、没有时刻的任务**不参与插值**：它的「实际时间」精确不到天以内，
  // 按 23:59:59 插值会把今天到期的推到几乎纯明天色（用户看到的就是「今天的卡片
  // 是黄的」）——没有时刻就按档位取整色，语义与配色模式一致。
  if (!hasClock) {
    return { bucket, strong, color: palette[index] };
  }
  const anchor0 = dayStart(now);
  const anchor1 = dayStart(now, 1);
  const anchor2 = dayStart(now, 2);
  const at = Math.min(anchor2, Math.max(anchor0, moment));
  const color =
    at <= anchor1
      ? mixHex(palette[1], palette[2], (at - anchor0) / (anchor1 - anchor0))
      : mixHex(palette[2], palette[3], (at - anchor1) / (anchor2 - anchor1));
  return { bucket, strong, color };
}

/** 卡片上要画的内联样式（`--due-*` 由 workspace.css 的 `.task-card.due-soon` 消费）。 */
export function dueHighlightStyle(highlight: DueHighlight | null): string {
  if (!highlight) return "";
  const tint = highlight.strong ? "2e" : "1c";
  const edge = highlight.strong ? "b3" : "7a";
  return `--due-color: ${highlight.color}; --due-tint: ${highlight.color}${tint}; --due-edge: ${highlight.color}${edge};`;
}

/**
 * 任务提醒的纯逻辑（v0.8.3）。
 *
 * 单独成模块、不 import stores / DOM，才跑得进 node 环境的单元测试（与 `markdownTasks.ts`、
 * `dueHighlight.ts`、`rmb.ts` 同一条纪律）。
 *
 * 与 core `crates/core/src/reminders.rs` **同口径**的三件事：
 * 1. 规则只有两种：绝对时刻（RFC3339）与「截止前 N 分钟」（0 = 到点时）；
 * 2. 线上传输一律是**字符串规格**（`due-60` / RFC3339 / `+1h`），与 `tags` 的
 *    `"color:text"` 同一套约定——CLI 与 GUI 发同一个形状，解析只在 core 一处；
 * 3. 「截止前」需要任务同时有截止日期与精确到分钟的时刻，否则算不出瞬时。
 *
 * 这里**不做**「哪一条已经响过」的判断：那是本机 runtime 台账（`runtime/reminders.json`）
 * 的事，既不进同步载荷也不进前端。
 */
import { formatClock, parseClock } from "./clock";
import type { ReminderRule } from "./types";

// ---------------------------------------------------------------------------
// 线上规格
// ---------------------------------------------------------------------------

/** 规则 → 命令层字符串规格（core `reminders::parse_spec` 的镜像）。 */
export function reminderParam(rule: ReminderRule): string {
  return rule.kind === "beforeDue" ? `due-${rule.minutes}` : rule.at;
}

/** 一组规则 → 命令层参数（整体替换语义：给什么就是什么）。 */
export function remindersParam(rules: ReminderRule[]): string[] {
  return rules.map(reminderParam);
}

// ---------------------------------------------------------------------------
// 归一化与解析
// ---------------------------------------------------------------------------

/**
 * 归一化快照里的提醒数组。脏数据（缺字段、分钟不是非负整数、时刻解析不了）一律丢掉——
 * 界面宁可少显示一条提醒，也不能拿 NaN 去算倒计时。
 */
export function normalizeReminders(raw: unknown): ReminderRule[] {
  if (!Array.isArray(raw)) return [];
  const out: ReminderRule[] = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== "object") continue;
    const source = entry as { kind?: unknown; at?: unknown; minutes?: unknown };
    if (source.kind === "beforeDue") {
      const minutes = Number(source.minutes);
      if (Number.isInteger(minutes) && minutes >= 0) out.push({ kind: "beforeDue", minutes });
      continue;
    }
    if (source.kind === "absolute" && typeof source.at === "string") {
      const at = new Date(source.at).getTime();
      if (!Number.isNaN(at)) out.push({ kind: "absolute", at: source.at });
    }
  }
  return out;
}

/**
 * 规则 → 本地 epoch 毫秒；算不出来（缺截止日期/时刻、字符串非法）回 null。
 * 与 core `reminders::resolve` 同口径（core 还额外拒绝夏令时歧义时刻，前端只做展示，
 * 真到写入时由 core 说了算）。
 */
export function reminderMoment(
  rule: ReminderRule,
  dueDate?: string,
  dueTime?: string
): number | null {
  if (rule.kind === "absolute") {
    const at = new Date(rule.at).getTime();
    return Number.isNaN(at) ? null : at;
  }
  const due = dueInstant(dueDate, dueTime);
  return due === null ? null : due - rule.minutes * 60_000;
}

/** 截止瞬时（本地时区）；没有日期或没有分钟时刻都回 null。 */
export function dueInstant(dueDate?: string, dueTime?: string): number | null {
  if (!dueDate) return null;
  const clock = parseClock(dueTime);
  if (!clock) return null;
  const [year, month, day] = dueDate.slice(0, 10).split("-").map(Number);
  if (!year || !month || !day) return null;
  return new Date(year, month - 1, day, clock.hour, clock.minute, 0, 0).getTime();
}

/** 「截止前」提醒能不能用：必须同时有截止日期与精确到分钟的时刻。 */
export function canUseBeforeDue(dueDate?: string, dueTime?: string): boolean {
  return dueInstant(dueDate, dueTime) !== null;
}

// ---------------------------------------------------------------------------
// 展示
// ---------------------------------------------------------------------------

function humanSpan(minutes: number): string {
  if (minutes > 0 && minutes % 1440 === 0) return `${minutes / 1440}天`;
  if (minutes > 0 && minutes % 60 === 0) return `${minutes / 60}小时`;
  return `${minutes}分钟`;
}

/**
 * 提醒胶囊上的文字：`截止前1小时` / `到点时` / `9/16 17:23`。
 * 绝对时刻跨年时补年份，免得「1/2 09:00」分不清是今年还是明年。
 */
export function reminderLabel(rule: ReminderRule, dueDate?: string, dueTime?: string): string {
  if (rule.kind === "beforeDue") {
    return rule.minutes === 0 ? "到点时" : `截止前${humanSpan(rule.minutes)}`;
  }
  const at = new Date(rule.at);
  if (Number.isNaN(at.getTime())) return "提醒";
  return formatReminderMoment(at);
}

/** `9/16 17:23`（跨年补成 `2026/9/16 17:23`）。 */
export function formatReminderMoment(at: Date, now = new Date()): string {
  const clock = formatClock(at.getHours(), at.getMinutes());
  const date = `${at.getMonth() + 1}/${at.getDate()}`;
  return at.getFullYear() === now.getFullYear() ? `${date} ${clock}` : `${at.getFullYear()}/${date} ${clock}`;
}

/** 带本地时区偏移的 RFC3339（秒精度）：`2026-09-20T09:00:00+08:00`。
 *  不用 `toISOString()`——那是 UTC，落盘后用户在 JSON 里看不出自己选的是几点。 */
export function toLocalRfc3339(at: Date): string {
  const pad = (value: number): string => String(value).padStart(2, "0");
  const offset = -at.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const absolute = Math.abs(offset);
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `T${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}` +
    `${sign}${pad(Math.floor(absolute / 60))}:${pad(absolute % 60)}`
  );
}

/** 本地瞬时 → 绝对规则。 */
export function absoluteReminder(ms: number): ReminderRule {
  return { kind: "absolute", at: toLocalRfc3339(new Date(ms)) };
}

/** 「自定义」面板选的日期 + 时刻 → 绝对规则；任何一段非法回 null。 */
export function absoluteReminderFromLocal(date: string, clock: string): ReminderRule | null {
  const parsed = parseClock(clock);
  if (!parsed) return null;
  const [year, month, day] = date.split("-").map(Number);
  if (!year || !month || !day) return null;
  return absoluteReminder(new Date(year, month - 1, day, parsed.hour, parsed.minute, 0, 0).getTime());
}

// ---------------------------------------------------------------------------
// 增删与排序
// ---------------------------------------------------------------------------

export type ReminderPresetId = "in1h" | "in2h" | "beforeDue60" | "beforeDue5" | "custom";

/** 「添加提醒」菜单的固定选项（顺序即界面顺序）。 */
export const REMINDER_PRESETS: Array<{ id: ReminderPresetId; label: string; needsDueTime: boolean }> = [
  { id: "in1h", label: "一小时后", needsDueTime: false },
  { id: "in2h", label: "两小时后", needsDueTime: false },
  { id: "beforeDue60", label: "截止前一小时", needsDueTime: true },
  { id: "beforeDue5", label: "截止前五分钟", needsDueTime: true },
  { id: "custom", label: "自定义", needsDueTime: false }
];

/** 预设 → 规则；`custom` 回 null（由面板的日期/时间两页自己拼）。 */
export function presetReminder(id: ReminderPresetId, now = new Date()): ReminderRule | null {
  switch (id) {
    case "in1h":
      return absoluteReminder(now.getTime() + 3_600_000);
    case "in2h":
      return absoluteReminder(now.getTime() + 7_200_000);
    case "beforeDue60":
      return { kind: "beforeDue", minutes: 60 };
    case "beforeDue5":
      return { kind: "beforeDue", minutes: 5 };
    default:
      return null;
  }
}

/**
 * 加一条提醒。**同一瞬时已经有了就不加**：提醒的身份就是「这个任务在这一刻响一次」，
 * 手写的绝对时刻正好等于「截止前 5 分钟」时也只该弹一次（与 core 的去重同口径）。
 */
export function withReminder(
  rules: ReminderRule[],
  added: ReminderRule,
  dueDate?: string,
  dueTime?: string
): ReminderRule[] {
  const at = reminderMoment(added, dueDate, dueTime);
  const duplicated =
    at !== null && rules.some((rule) => reminderMoment(rule, dueDate, dueTime) === at);
  return duplicated ? rules : [...rules, added];
}

/** 界面按触发时刻先后展示（core 不保证数组顺序）。算不出瞬时的排在最后。 */
export function sortReminders(
  rules: ReminderRule[],
  dueDate?: string,
  dueTime?: string
): ReminderRule[] {
  const at = (rule: ReminderRule): number => reminderMoment(rule, dueDate, dueTime) ?? Number.MAX_SAFE_INTEGER;
  return [...rules].sort((a, b) => at(a) - at(b));
}

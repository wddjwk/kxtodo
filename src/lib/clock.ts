/**
 * 时刻（HH:MM）这一小块到处要用：记账流水的 `time`、日记 `createdAt` 的时钟部分、
 * 任务的 `dueTime`。core 侧存的是 `HH:MM:SS`（秒只有导入导出会用），前端一律按
 * 分钟显示与编辑，读的时候容忍两种长度。
 */

export interface Clock {
  hour: number;
  minute: number;
}

/** 解析 `H:M` / `HH:MM` / `HH:MM:SS`。
 *
 * 与 core 的 `parse_clock`（`crates/core/src/time.rs`）**同口径**：三段都必须是纯数字、
 * 时 <24、分 <60、秒 <60，秒被舍掉。早先前端用的正则既没有尾锚（`9:05xyz` 会当成 09:05）
 * 又要求分钟必须两位（core 收的 `1:2` 前端拒），同一串输入两边结论相反。
 * 显示层宁可返回 null 也不要显示一个错的时间。
 */
export function parseClock(value?: string | null): Clock | null {
  if (typeof value !== "string") return null;
  const match = /^(\d{1,2}):(\d{1,2})(?::(\d{1,2}))?$/.exec(value.trim());
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  const second = match[3] === undefined ? 0 : Number(match[3]);
  if (!Number.isFinite(hour) || !Number.isFinite(minute) || !Number.isFinite(second)) return null;
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59 || second < 0 || second > 59) return null;
  return { hour, minute };
}

export function formatClock(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

export function nowClock(date = new Date()): string {
  return formatClock(date.getHours(), date.getMinutes());
}

/** 从 ISO 时间戳里取**本地** HH:MM；没有时钟部分（只有日期）就返回空串。
 *
 * 与 core 的 `time_of`（`crates/core/src/diary_archive.rs`，`parse_from_rfc3339` 后转 Local）
 * 同口径。早先这里是 `parseClock(timestamp.slice(11))`——按字面切第 11 位之后，而
 * `parseClock` 的正则带尾锚，于是**真实落盘的两种 createdAt 一律得到空串**：
 * `new Date().toISOString()` / core `now_iso()` 的 `"…T04:20:41.693Z"`、core
 * `compose_timestamp` 的 `"…T12:20:00+08:00"`。日记的写作时刻因此在编辑器与菜单里
 * 从来不显示（本机 diary.json 里 7 篇没有一篇能解析）。
 * 顺带修掉时区：字面切片对 `Z` 时间戳给出的是 UTC 钟点，与 core 差一个时区。
 */
export function clockOf(timestamp?: string | null): string {
  if (typeof timestamp !== "string") return "";
  // 长度门槛保留：裸时刻（`18:19:00`）与裸日期（`2026-09-08`）都不是时间戳，
  // 前者要归一请用 displayClock。Date 会把裸日期按 UTC 解析，放进来就错了。
  if (timestamp.trim().length < 11) return "";
  const parsed = new Date(timestamp.trim());
  if (Number.isNaN(parsed.getTime())) return "";
  return formatClock(parsed.getHours(), parsed.getMinutes());
}

/**
 * 把存下来的**时刻字段**（账目的 `time`，可能是 `HH:MM` 也可能是 `HH:MM:SS`）归一成
 * 显示的 `HH:MM`。别拿 `clockOf` 干这件事——它要的是 ISO 时间戳，喂它一个裸时刻
 * 一律得到空串（v0.7.3 的记账卡片不显示时刻就是这么来的）。
 */
export function displayClock(value?: string | null): string {
  const parsed = parseClock(value);
  return parsed ? formatClock(parsed.hour, parsed.minute) : "";
}

/** 把日期（YYYY-MM-DD）与时刻拼成一个本地 ISO 时间戳，秒固定 00。 */
export function composeTimestamp(date: string, clock: string, seconds = "00"): string {
  const parsed = parseClock(clock);
  if (!parsed) return date;
  return `${date}T${formatClock(parsed.hour, parsed.minute)}:${seconds}`;
}

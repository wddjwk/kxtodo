/**
 * 时刻（HH:MM）这一小块到处要用：记账流水的 `time`、日记 `createdAt` 的时钟部分、
 * 任务的 `dueTime`。core 侧存的是 `HH:MM:SS`（秒只有导入导出会用），前端一律按
 * 分钟显示与编辑，读的时候容忍两种长度。
 */

export interface Clock {
  hour: number;
  minute: number;
}

export function parseClock(value?: string | null): Clock | null {
  if (typeof value !== "string") return null;
  const match = /^(\d{1,2}):(\d{2})/.exec(value.trim());
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (!Number.isFinite(hour) || !Number.isFinite(minute)) return null;
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return null;
  return { hour, minute };
}

export function formatClock(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

export function nowClock(date = new Date()): string {
  return formatClock(date.getHours(), date.getMinutes());
}

/** 从 ISO 时间戳里取 HH:MM；没有时钟部分（只有日期）就返回空串。 */
export function clockOf(timestamp?: string | null): string {
  if (typeof timestamp !== "string" || timestamp.length < 11) return "";
  const parsed = parseClock(timestamp.slice(11));
  return parsed ? formatClock(parsed.hour, parsed.minute) : "";
}

/**
 * 把存下来的**时刻字段**（账目的 `time`，可能是 `HH:MM` 也可能是 `HH:MM:SS`）归一成
 * 显示的 `HH:MM`。别拿 `clockOf` 干这件事——它按 ISO 时间戳从第 11 位切，喂它一个
 * 裸时刻一律得到空串（v0.7.3 的记账卡片不显示时刻就是这么来的）。
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

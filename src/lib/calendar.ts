/**
 * 日程的纯逻辑（v0.9.0）：日期算术、时间网格的段切分与**冲突占位布局**、视图区间。
 *
 * 单独成模块、不 import stores / DOM，才跑得进 node 环境的单元测试（与 `diary.ts`、
 * `ledger.ts`、`dragHit.ts` 同一条纪律）。所有日期都是 `YYYY-MM-DD` 字符串的本地
 * 墙上时间口径（与 core `model::CalendarEvent` 一致）；日期加减一律经 `Date.UTC`
 * 计算，**断言与实现都与时区无关**。
 */
import { mixWithWhite, readableOn } from "./colors";
import type { CalendarCategory, CalendarEvent, CalendarRepeat, ReminderRule } from "./types";

export const MINUTES_PER_DAY = 24 * 60;

/**
 * 日程详情插图复用「按条目分目录」的图片通道（`img/data/<nodeId>/`），伪条目 id
 * 固定为 `calendar`——与 core `model::CALENDAR_IMAGE_NODE` 同名，图片的存储 /
 * 同步（entry 类别随「同步数据」范围）/ 释放空间盘点与日记、记账同一套。
 */
export const CALENDAR_IMAGE_NODE = "calendar";

// ---------------------------------------------------------------------------
// 日期算术（字符串口径，Date.UTC 内核）
// ---------------------------------------------------------------------------

const DATE_RE = /^(\d{4})-(\d{2})-(\d{2})$/;

function utcOf(date: string): number {
  const match = DATE_RE.exec(date);
  if (!match) return Date.UTC(1970, 0, 1);
  return Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
}

function fromUtc(ms: number): string {
  const date = new Date(ms);
  const month = `${date.getUTCMonth() + 1}`.padStart(2, "0");
  const day = `${date.getUTCDate()}`.padStart(2, "0");
  return `${date.getUTCFullYear()}-${month}-${day}`;
}

/** 今天的 YYYY-MM-DD（本地口径，与 diary.ts::todayDate 同一算法）。 */
export function calendarToday(): string {
  const date = new Date();
  const offset = date.getTimezoneOffset();
  return new Date(date.getTime() - offset * 60_000).toISOString().slice(0, 10);
}

export function addDays(date: string, days: number): string {
  return fromUtc(utcOf(date) + days * 86_400_000);
}

export function diffDays(from: string, to: string): number {
  return Math.round((utcOf(to) - utcOf(from)) / 86_400_000);
}

export function isSameDay(a: string, b: string): boolean {
  return a === b;
}

export function yearOf(date: string): number {
  return Number(date.slice(0, 4));
}

export function monthOf(date: string): number {
  return Number(date.slice(5, 7)) - 1;
}

export function dayNumber(date: string): number {
  return Number(date.slice(8, 10));
}

/** 周几（0 = 周日 … 6 = 周六），本地日期字符串按 UTC 取，避免时区偏移。 */
export function weekdayOf(date: string): number {
  return new Date(utcOf(date)).getUTCDay();
}

/** 所在周的第一天（weekStart：0 = 周日起，1 = 周一起，与 stores 的 weekStart 同口径）。 */
export function startOfWeek(date: string, weekStart: number): string {
  const shift = (weekdayOf(date) - weekStart + 7) % 7;
  return addDays(date, -shift);
}

/** 周视图的七天。 */
export function weekDates(anchor: string, weekStart: number): string[] {
  const first = startOfWeek(anchor, weekStart);
  return Array.from({ length: 7 }, (_, index) => addDays(first, index));
}

/** 三日视图：以锚点日为中心的三天。 */
export function threeDayDates(anchor: string): string[] {
  return [addDays(anchor, -1), anchor, addDays(anchor, 1)];
}

/** 月视图的 6×7 格子（含前后补位），第一格是当月 1 号所在周的第一天。 */
export function monthGridDates(anchor: string, weekStart: number): string[] {
  const firstOfMonth = `${anchor.slice(0, 7)}-01`;
  const first = startOfWeek(firstOfMonth, weekStart);
  return Array.from({ length: 42 }, (_, index) => addDays(first, index));
}

/** 视图翻页：month 按整月、week 按七天、threeday/day 按对应天数。 */
export function shiftRange(anchor: string, view: "month" | "week" | "threeday" | "day", direction: -1 | 1): string {
  switch (view) {
    case "month": {
      const first = `${anchor.slice(0, 7)}-01`;
      if (direction > 0) {
        // 加 32 天必落进下个月（最长的月 31 天），再取那个月的 1 号
        return `${addDays(first, 32).slice(0, 7)}-01`;
      }
      // 减一天必落进上个月
      return `${addDays(first, -1).slice(0, 7)}-01`;
    }
    case "week":
      return addDays(anchor, direction * 7);
    case "threeday":
      return addDays(anchor, direction * 3);
    case "day":
      return addDays(anchor, direction);
  }
}

// ---------------------------------------------------------------------------
// 时刻
// ---------------------------------------------------------------------------

const TIME_RE = /^(\d{2}):(\d{2})$/;

/** "HH:MM" → 当天分钟数；非法 = 0。 */
export function minutesOf(time: string): number {
  const match = TIME_RE.exec(time);
  if (!match) return 0;
  return Number(match[1]) * 60 + Number(match[2]);
}

/** 当天分钟数 → "HH:MM"（钳制在 0..1440）。 */
export function formatMinutes(minutes: number): string {
  const clamped = Math.min(MINUTES_PER_DAY, Math.max(0, Math.round(minutes)));
  const hours = Math.floor(clamped / 60) % 24;
  const mins = clamped % 60;
  return `${`${hours}`.padStart(2, "0")}:${`${mins}`.padStart(2, "0")}`;
}

/** 拖动吸附：把分钟数吸到 step 的整数倍（15 分钟一档，业界主流口径）。 */
export function snapMinutes(minutes: number, step: number): number {
  if (step <= 0) return Math.round(minutes);
  return Math.round(minutes / step) * step;
}

/** "HH:MM" 显示串（分钟为 0 时也保留 :00，时间网格里对齐比省字重要）。 */
export function timeLabel(time: string): string {
  return TIME_RE.test(time) ? time : "";
}

// ---------------------------------------------------------------------------
// 事件 → 某一天的段
// ---------------------------------------------------------------------------

/** 事件是否覆盖某一天（闭区间；全天事件同样按日期区间判）。 */
export function eventCoversDay(event: CalendarEvent, date: string): boolean {
  return event.startDate <= date && date <= event.endDate;
}

export type DaySegment = {
  event: CalendarEvent;
  /** 当天内的开始分钟（跨天事件在后续天从 0 开始） */
  startMin: number;
  /** 当天内的结束分钟（跨天事件在非末尾天到 1440；恰好午夜的结束折成 1440） */
  endMin: number;
  /** 是否是事件的第一天 / 最后一天（渲染圆角与时间文本用） */
  continuesBefore: boolean;
  continuesAfter: boolean;
};

/**
 * 事件在某一天的时间网格段；不覆盖这一天或全天事件（住全天栏，不占网格）返回 null。
 * 定时事件没写结束时刻按一小时算（与 core 的缺省一致）。
 */
export function segmentForDay(event: CalendarEvent, date: string): DaySegment | null {
  if (event.allDay || !eventCoversDay(event, date)) return null;
  const startMin = event.startDate === date ? minutesOf(event.startTime) : 0;
  let endMin: number;
  if (event.endDate === date) {
    endMin = event.endTime === "" ? startMin + 60 : minutesOf(event.endTime);
    // 结束恰好是次日 00:00（或多日段的午夜边界）→ 画满当天
    if (endMin === 0) endMin = MINUTES_PER_DAY;
  } else {
    endMin = MINUTES_PER_DAY;
  }
  if (endMin <= startMin) endMin = Math.min(MINUTES_PER_DAY, startMin + 30);
  return {
    event,
    startMin,
    endMin,
    continuesBefore: event.startDate < date,
    continuesAfter: event.endDate > date
  };
}

/** 某一天的全天事件（含跨天事件在这一天的展示）。 */
export function allDayEventsFor(events: CalendarEvent[], date: string): CalendarEvent[] {
  return events
    .filter((event) => event.allDay && eventCoversDay(event, date))
    .sort((a, b) => a.startDate.localeCompare(b.startDate) || a.id.localeCompare(b.id));
}

// ---------------------------------------------------------------------------
// 冲突占位布局（时间网格的列分配）
// ---------------------------------------------------------------------------

export type LayoutBlock = DaySegment & {
  /** 第几列（0 起） */
  column: number;
  /** 所在重叠簇的总列数（宽度 = 1/columns） */
  columns: number;
};

/**
 * 把一天的段排成「重叠簇内平分列」的布局（Google/飞书日历同款语义）：
 * 互相重叠的事件分进不同的列、等分宽度；只与簇里部分事件重叠的也按整簇列数走，
 * 列内按开始先后贪心放置。输入顺序无关（内部先排序），输出保持排序后的顺序。
 */
export function layoutDaySegments(segments: DaySegment[]): LayoutBlock[] {
  const sorted = [...segments].sort(
    (a, b) => a.startMin - b.startMin || b.endMin - a.endMin || a.event.id.localeCompare(b.event.id)
  );
  const out: LayoutBlock[] = [];
  let cluster: DaySegment[] = [];
  let clusterEnd = -1;

  const flush = (): void => {
    if (cluster.length === 0) return;
    // 簇内贪心分列：每列记住「最后一个事件的结束」，新事件放进第一个放得下的列
    const columnEnds: number[] = [];
    const placed = cluster.map((segment) => {
      let column = columnEnds.findIndex((end) => end <= segment.startMin);
      if (column < 0) {
        column = columnEnds.length;
        columnEnds.push(segment.endMin);
      } else {
        columnEnds[column] = segment.endMin;
      }
      return { segment, column };
    });
    const columns = columnEnds.length;
    for (const { segment, column } of placed) {
      out.push({ ...segment, column, columns });
    }
    cluster = [];
    clusterEnd = -1;
  };

  for (const segment of sorted) {
    if (cluster.length > 0 && segment.startMin >= clusterEnd) flush();
    cluster.push(segment);
    clusterEnd = Math.max(clusterEnd, segment.endMin);
  }
  flush();
  return out;
}

// ---------------------------------------------------------------------------
// 展示文本
// ---------------------------------------------------------------------------

/** 事件的时间展示串："09:00 – 10:00" / "全天" / 跨天 "10-25 09:00 – 10-26 18:00"。 */
export function eventTimeText(event: CalendarEvent): string {
  if (event.allDay) {
    return event.startDate === event.endDate ? "全天" : `全天 · ${shortDate(event.startDate)}–${shortDate(event.endDate)}`;
  }
  const start = timeLabel(event.startTime);
  const end = event.endTime === "" ? formatMinutes(minutesOf(event.startTime) + 60) : timeLabel(event.endTime);
  if (event.startDate === event.endDate) return `${start} – ${end}`;
  return `${shortDate(event.startDate)} ${start} – ${shortDate(event.endDate)} ${end}`;
}

/** "10-25"（跨天事件的紧凑日期）。 */
export function shortDate(date: string): string {
  return date.slice(5).replace("-", "-");
}

/** 头部区间标题：月 "2026年10月"；周/三日 "2026年10月20日 – 26日"（跨月/年展开）；日 "2026年10月25日"。 */
export function rangeTitle(dates: string[], view: "month" | "week" | "threeday" | "day", anchor: string): string {
  if (view === "month") {
    return `${yearOf(anchor)}年${monthOf(anchor) + 1}月`;
  }
  if (view === "day") {
    return `${yearOf(anchor)}年${monthOf(anchor) + 1}月${dayNumber(anchor)}日`;
  }
  const first = dates[0];
  const last = dates[dates.length - 1];
  if (yearOf(first) !== yearOf(last)) {
    return `${yearOf(first)}年${monthOf(first) + 1}月${dayNumber(first)}日 – ${yearOf(last)}年${monthOf(last) + 1}月${dayNumber(last)}日`;
  }
  if (monthOf(first) !== monthOf(last)) {
    return `${yearOf(first)}年${monthOf(first) + 1}月${dayNumber(first)}日 – ${monthOf(last) + 1}月${dayNumber(last)}日`;
  }
  return `${yearOf(first)}年${monthOf(first) + 1}月${dayNumber(first)}日 – ${dayNumber(last)}日`;
}

/** 月视图格子里的芯片排序：全天在前，其后按开始时刻。 */
export function monthCellEvents(events: CalendarEvent[], date: string): CalendarEvent[] {
  return events
    .filter((event) => eventCoversDay(event, date))
    .sort((a, b) => {
      if (a.allDay !== b.allDay) return a.allDay ? -1 : 1;
      const startA = a.allDay ? 0 : minutesOf(a.startTime);
      const startB = b.allDay ? 0 : minutesOf(b.startTime);
      return startA - startB || a.id.localeCompare(b.id);
    });
}

// ---------------------------------------------------------------------------
// 新建事件的缺省时间（legacy 浏览器路径与编辑器预置共用；core 有同一份语义）
// ---------------------------------------------------------------------------

export type CalendarTimes = {
  startDate: string;
  startTime: string;
  endDate: string;
  endTime: string;
  allDay: boolean;
};

/**
 * 归一时间（core `ops_calendar::normalize_times` 的镜像）：没有开始时刻 = 全天；
 * 定时事件没写结束 = 开始 + 1 小时（跨午夜进位）；结束早于开始原样返回由调用方报错。
 */
export function normalizeCalendarTimes(input: {
  startDate: string;
  startTime?: string;
  endDate?: string;
  endTime?: string;
  allDay?: boolean;
}): CalendarTimes {
  const startDate = input.startDate;
  const startTime = input.allDay ? "" : (input.startTime ?? "").trim();
  if (input.allDay || startTime === "") {
    const endDate = input.endDate && input.endDate >= startDate ? input.endDate : startDate;
    return { startDate, startTime: "", endDate, endTime: "", allDay: true };
  }
  let endDate = input.endDate && input.endDate !== "" ? input.endDate : startDate;
  let endTime = (input.endTime ?? "").trim();
  if (endTime === "") {
    const total = minutesOf(startTime) + 60;
    endDate = total >= MINUTES_PER_DAY ? addDays(startDate, 1) : startDate;
    endTime = formatMinutes(total % MINUTES_PER_DAY);
  }
  return { startDate, startTime, endDate, endTime, allDay: false };
}

// ---------------------------------------------------------------------------
// 提醒与时长的展示文本
// ---------------------------------------------------------------------------

/** 提醒规则的展示串（编辑器芯片用）。 */
export function reminderLabel(rule: ReminderRule): string {
  if (rule.kind === "beforeDue") {
    if (rule.minutes === 0) return "日程开始时";
    if (rule.minutes < 60) return `提前 ${rule.minutes} 分钟`;
    if (rule.minutes % 60 === 0 && rule.minutes < 1440) return `提前 ${rule.minutes / 60} 小时`;
    if (rule.minutes % 1440 === 0) return `提前 ${rule.minutes / 1440} 天`;
    return `提前 ${rule.minutes} 分钟`;
  }
  const at = new Date(rule.at);
  if (Number.isNaN(at.getTime())) return rule.at;
  const month = at.getMonth() + 1;
  const day = at.getDate();
  const hh = `${at.getHours()}`.padStart(2, "0");
  const mm = `${at.getMinutes()}`.padStart(2, "0");
  return `${month}月${day}日 ${hh}:${mm}`;
}

/** 时长展示串："1 小时 30 分钟" / "45 分钟" / "3 天"（全天跨天）。 */
export function durationText(times: CalendarTimes): string {
  if (times.allDay) {
    const days = diffDays(times.startDate, times.endDate) + 1;
    return days > 1 ? `${days} 天` : "全天";
  }
  const spanDays = diffDays(times.startDate, times.endDate);
  const total = spanDays * MINUTES_PER_DAY + minutesOf(times.endTime) - minutesOf(times.startTime);
  if (total <= 0) return "";
  const hours = Math.floor(total / 60);
  const minutes = total % 60;
  if (hours === 0) return `${minutes} 分钟`;
  if (minutes === 0) return `${hours} 小时`;
  return `${hours} 小时 ${minutes} 分钟`;
}

// ---------------------------------------------------------------------------
// 重复规则的展开（v0.9.1）——与 core `recurrence.rs` 逐条同口径
// ---------------------------------------------------------------------------

/** 重复次数上限（core `model::REPEAT_COUNT_MAX` 的孪生，`frontend_contract.rs` 钉住）。 */
export const REPEAT_COUNT_MAX = 999;

/** 展开的保险丝：步数与产出条数都封顶，坏规则不许把渲染拖死。 */
const MAX_STEPS = 4_000;
const MAX_RESULTS = 1_000;

/**
 * 渲染层用的「一次出现」：日期已经平移到那一次（跨天事件整体平移，时长不变），
 * 其余字段跟着系列走。`occurrence` 是身份的一部分——「只删这一次」要报它。
 */
export type CalendarOccurrence = CalendarEvent & {
  /** 这一次的开始日期；非重复事件 = startDate */
  occurrence: string;
  recurring: boolean;
};

/** 周频率的星期集合 → 相对**周一**的偏移（升序）。周次锚在周一，与用户的
 *  「一周从周几开始」显示偏好无关——否则改个偏好重复日就漂了。 */
function weekdayOffsets(repeat: CalendarRepeat, startDate: string): number[] {
  const fromMonday = (date: string): number => (weekdayOf(date) + 6) % 7;
  if (repeat.freq !== "week") return [fromMonday(startDate)];
  let offsets = repeat.byWeekday.filter((value) => value >= 0 && value <= 6).map((value) => (value + 6) % 7);
  if (offsets.length === 0) offsets = [fromMonday(startDate)];
  return [...new Set(offsets)].sort((a, b) => a - b);
}

/** 月/年频率的日号集合（升序）；空 = 按开始日期的日号。 */
function monthDaysOf(repeat: CalendarRepeat, startDate: string): number[] {
  if (repeat.freq !== "month" && repeat.freq !== "year") return [];
  const days = repeat.byMonthDay.filter((value) => value >= 1 && value <= 31);
  const out = days.length > 0 ? days : [dayNumber(startDate)];
  return [...new Set(out)].sort((a, b) => a - b);
}

function daysInMonth(year: number, month1: number): number {
  return new Date(Date.UTC(year, month1, 0)).getUTCDate();
}

/** 按月份偏移并强制日号；目标月没有这一天返回 null（跳过该次，不折算到月末）。 */
function addMonthsForced(date: string, months: number, day: number): string | null {
  const total = yearOf(date) * 12 + monthOf(date) + months;
  const year = Math.floor(total / 12);
  const month1 = (total % 12) + 1;
  if (day > daysInMonth(year, month1)) return null;
  return `${year}-${`${month1}`.padStart(2, "0")}-${`${day}`.padStart(2, "0")}`;
}

/** 第 step 步生成的日期（升序）。 */
function stepDates(
  repeat: CalendarRepeat,
  startDate: string,
  step: number,
  interval: number,
  offsets: number[],
  monthDays: number[]
): string[] {
  const k = step * interval;
  switch (repeat.freq) {
    case "day":
      return [addDays(startDate, k)];
    case "week": {
      const anchor = addDays(startDate, -((weekdayOf(startDate) + 6) % 7));
      const week = addDays(anchor, k * 7);
      return offsets.map((offset) => addDays(week, offset));
    }
    case "month":
      return monthDays
        .map((day) => addMonthsForced(startDate, k, day))
        .filter((date): date is string => date !== null);
    case "year":
      return monthDays
        .map((day) => addMonthsForced(startDate, k * 12, day))
        .filter((date): date is string => date !== null);
  }
}

/** 跳到「可能落进 from 之后」的第一步（保守回退 2 步）。只有不限次数时能跳——
 *  按次数结束的次数名额是全局的，必须从头数。 */
function skipAhead(repeat: CalendarRepeat, startDate: string, from: string, interval: number): number {
  if (from <= startDate) return 0;
  const days = diffDays(startDate, from);
  const perStep =
    repeat.freq === "day" ? interval : repeat.freq === "week" ? interval * 7 : repeat.freq === "month" ? interval * 28 : interval * 365;
  return Math.max(0, Math.floor(days / Math.max(1, perStep)) - 2);
}

/**
 * 重复事件在 `[from, to]`（闭区间）里的每一次**开始日期**（升序，已去掉例外日期）。
 * 非重复事件就是它自己的开始日期（在区间内才给）。
 */
export function occurrenceDates(event: CalendarEvent, from: string, to: string): string[] {
  const start = event.startDate;
  if (!event.repeat) {
    return from <= start && start <= to ? [start] : [];
  }
  if (to < from || to < start) return [];
  const repeat = event.repeat;
  const interval = Math.max(1, Math.round(repeat.interval) || 1);
  let limit = to;
  if (repeat.end === "until") {
    if (repeat.until === "" ) return [];
    if (repeat.until < start) return [];
    limit = repeat.until < to ? repeat.until : to;
  }
  const count = repeat.end === "count" ? Math.min(REPEAT_COUNT_MAX, Math.max(1, repeat.count)) : null;
  const offsets = weekdayOffsets(repeat, start);
  const monthDays = monthDaysOf(repeat, start);
  const except = new Set(event.exceptDates);
  const out: string[] = [];
  let index = 0;
  let step = count === null ? skipAhead(repeat, start, from, interval) : 0;
  for (let iteration = 0; iteration < MAX_STEPS; iteration++) {
    const dates = stepDates(repeat, start, step, interval, offsets, monthDays);
    step += 1;
    if (dates.length === 0) continue;
    if (dates[0] > limit) break;
    for (const date of dates) {
      if (date < start) continue;
      if (count !== null && index >= count) return out;
      if (date > limit) return out;
      index += 1;
      if (date < from || except.has(date)) continue;
      out.push(date);
      if (out.length >= MAX_RESULTS) return out;
    }
  }
  return out;
}

/**
 * 把事件列表摊成区间内的**逐次出现**（视图渲染只吃这个）：
 * 非重复事件原样带上（视图自己按天过滤），重复事件按次展开并整体平移日期。
 * 往前多扫一个身长——跨天事件开始日在区间之前、身体还在区间里的也要出现。
 */
export function expandForRange(events: CalendarEvent[], from: string, to: string): CalendarOccurrence[] {
  const out: CalendarOccurrence[] = [];
  for (const event of events) {
    if (!event.repeat) {
      out.push({ ...event, occurrence: event.startDate, recurring: false });
      continue;
    }
    const span = Math.max(0, diffDays(event.startDate, event.endDate));
    for (const occurrence of occurrenceDates(event, addDays(from, -span), to)) {
      const delta = diffDays(event.startDate, occurrence);
      if (delta === 0) {
        out.push({ ...event, occurrence, recurring: true });
        continue;
      }
      out.push({
        ...event,
        startDate: addDays(event.startDate, delta),
        endDate: addDays(event.endDate, delta),
        occurrence,
        recurring: true
      });
    }
  }
  return out;
}

/**
 * 拖动重复日程的**某一次** = 改整个系列（业界通行做法；「只改这一次」需要例外实体，
 * 本版不做，界面与文案都按「改的是系列」说话）。
 *
 * 平移多少：`occurrence → next.startDate` 的天数差，整个系列一起挪，时长跟着新的
 * 起止走。**但多星期的周规则（工作日、自定义周一三五）例外**：把整个系列挪到别的
 * 星期等于换了一条规则，用户拖的只是「这一次」，所以只改时刻与时长、不改哪几天。
 * 单星期的周规则与按月某一天的规则则跟着改（拖到周二就是以后都在周二）。
 */
export type SeriesDragChanges = {
  startDate: string;
  startTime: string;
  endDate: string;
  endTime: string;
  repeat?: CalendarRepeat;
  exceptDates?: string[];
};

export function seriesChangesFromDrag(
  event: CalendarEvent,
  occurrence: string,
  next: { startDate: string; startTime: string; endDate: string; endTime: string }
): SeriesDragChanges {
  const repeat = event.repeat;
  const spanDays = diffDays(next.startDate, next.endDate);
  if (!repeat) {
    return { ...next };
  }
  const multiDayWeek = repeat.freq === "week" && repeat.byWeekday.length > 1;
  const delta = multiDayWeek ? 0 : diffDays(occurrence || event.startDate, next.startDate);
  const startDate = addDays(event.startDate, delta);
  const changes: SeriesDragChanges = {
    startDate,
    startTime: next.startTime,
    endDate: addDays(startDate, spanDays),
    endTime: next.endTime
  };
  // 例外日期是系列时间轴上的点：系列整体挪了，它们也得跟着挪，
  // 否则「删掉的那一次」会错位到别的日期上（多星期周规则 delta=0，天然不动）
  if (delta !== 0 && event.exceptDates.length > 0) {
    changes.exceptDates = event.exceptDates.map((date) => addDays(date, delta)).sort();
  }
  if (repeat.freq === "week" && repeat.byWeekday.length === 1) {
    changes.repeat = { ...repeat, byWeekday: [weekdayOf(startDate)] };
  } else if (repeat.freq === "month" && repeat.byMonthDay.length === 1) {
    changes.repeat = { ...repeat, byMonthDay: [dayNumber(startDate)] };
  }
  return changes;
}

// ---------------------------------------------------------------------------
// 重复规则的预设与展示串（与 core `recurrence::describe` 同口径）
// ---------------------------------------------------------------------------

export type RepeatPresetId = "none" | "daily" | "weekdays" | "weekly" | "biweekly" | "monthly" | "yearly" | "custom";

export const REPEAT_PRESET_LABELS: Record<Exclude<RepeatPresetId, "custom">, string> = {
  none: "不重复",
  daily: "每天",
  weekdays: "工作日（周一至周五）",
  weekly: "每周",
  biweekly: "每两周",
  monthly: "每月",
  yearly: "每年"
};

/** 按预设造规则：每周/每两周/每月都要跟着开始日期落到具体的星期或日号。 */
export function buildRepeatPreset(id: RepeatPresetId, startDate: string): CalendarRepeat | null {
  const base = (freq: CalendarRepeat["freq"], interval: number): CalendarRepeat => ({
    freq,
    interval,
    byWeekday: [],
    byMonthDay: [],
    end: "never",
    count: 0,
    until: ""
  });
  switch (id) {
    case "none":
    case "custom":
      return null;
    case "daily":
      return base("day", 1);
    case "weekdays": {
      const rule = base("week", 1);
      rule.byWeekday = [1, 2, 3, 4, 5];
      return rule;
    }
    case "weekly":
      // 刻意**不**写死星期：空集合 = 「跟开始日期的星期走」（core 的 preset 同口径），
      // 于是拖动改开始日期时星期自己跟着变，不必再改规则
      return base("week", 1);
    case "biweekly":
      return base("week", 2);
    case "monthly": {
      const rule = base("month", 1);
      rule.byMonthDay = [dayNumber(startDate)];
      return rule;
    }
    case "yearly":
      return base("year", 1);
  }
}

/** 规则 → 预设 id（对不上就是「自定义」，编辑器据此展开细节面板）。 */
export function matchRepeatPreset(repeat: CalendarRepeat | null, startDate: string): RepeatPresetId {
  if (!repeat) return "none";
  for (const id of ["daily", "weekdays", "weekly", "biweekly", "monthly", "yearly"] as const) {
    const preset = buildRepeatPreset(id, startDate);
    if (preset && repeatSignature(preset) === repeatSignature(repeat)) return id;
  }
  return "custom";
}

/** 只比「决定展开结果」的字段：end/count/until 不参与预设匹配。 */
function repeatSignature(repeat: CalendarRepeat): string {
  return [repeat.freq, repeat.interval, [...repeat.byWeekday].sort().join(","), [...repeat.byMonthDay].sort().join(",")].join("|");
}

const WEEKDAY_NAMES = ["日", "一", "二", "三", "四", "五", "六"];

/** 重复规则的中文展示串（编辑器、悬浮详情与通知文案共用）。 */
export function repeatLabel(repeat: CalendarRepeat | null): string {
  if (!repeat) return "";
  const base = (unit: string): string =>
    repeat.interval <= 1 ? `每${unit}` : `每 ${repeat.interval} ${unit}`;
  let text: string;
  switch (repeat.freq) {
    case "day":
      text = base("天");
      break;
    case "week": {
      const days = repeat.byWeekday.filter((value) => value >= 0 && value <= 6).map((value) => WEEKDAY_NAMES[value]);
      text = days.length === 0 ? base("周") : `${base("周")}的周${days.join("、")}`;
      break;
    }
    case "month": {
      const days = repeat.byMonthDay.filter((value) => value >= 1 && value <= 31).map((day) => `${day} 日`);
      text = days.length === 0 ? base("个月") : `${base("个月")}的 ${days.join("、")}`;
      break;
    }
    case "year":
      text = base("年");
      break;
  }
  if (repeat.end === "count") return `${text}（共 ${repeat.count} 次）`;
  if (repeat.end === "until") return `${text}（至 ${shortDate(repeat.until)}）`;
  return text;
}

// ---------------------------------------------------------------------------
// 分类颜色（v0.9.1）
// ---------------------------------------------------------------------------

export type CalendarEventColors = {
  /** 主色：左侧边框、全天芯片底色、月视图圆点 */
  main: string;
  /** 块内那层淡色：与白混合到「不影响阅读」为止 */
  soft: string;
  /** 块的细边框：比主色淡、比 soft 深一点，把块从背景里拎出来 */
  line: string;
  /** 主色底上该用的字色 */
  onMain: "#ffffff" | "#23272f";
};

/**
 * 事件在各视图里的配色：有分类跟分类色，没分类跟日程主题色。
 * 淡色底一律按 `mixWithWhite(main, 0.14)` 算——用户自定义的颜色再深也不会
 * 把黑字压掉，这是「块内淡色不影响阅读」的实现口径。
 */
export function eventColors(
  event: Pick<CalendarEvent, "categoryId">,
  categories: CalendarCategory[],
  accent: string
): CalendarEventColors {
  const category = event.categoryId ? categories.find((item) => item.id === event.categoryId) : undefined;
  const main = category && category.color !== "" ? category.color : accent;
  return { main, soft: mixWithWhite(main, 0.14), line: mixWithWhite(main, 0.45), onMain: readableOn(main) };
}

/** 分类名的展示串（悬浮详情用）；没分类返回空串。 */
export function categoryName(event: Pick<CalendarEvent, "categoryId">, categories: CalendarCategory[]): string {
  if (!event.categoryId) return "";
  return categories.find((item) => item.id === event.categoryId)?.name ?? "";
}

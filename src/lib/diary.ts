import type { DiaryEntry } from "./types";

/**
 * 日记插图复用现成的「按条目分目录」图片通道（`img/data/<nodeId>/`），
 * 用这个伪条目 id 作目录名：图片存储与同步都不需要为日记单开一条路。
 * 卡片渲染与编辑器必须用同一个键，否则解析不到缓存。
 */
export const DIARY_IMAGE_NODE = "diary";

// ---------------------------------------------------------------------------
// 心情 / 天气预设（存的是 emoji 本身，标签只用于选择器与卡片展示）
// ---------------------------------------------------------------------------

export const MOOD_PRESETS: Array<{ emoji: string; label: string }> = [
  { emoji: "😀", label: "开心" },
  { emoji: "🙂", label: "还好" },
  { emoji: "😌", label: "平静" },
  { emoji: "🥳", label: "兴奋" },
  { emoji: "😍", label: "喜欢" },
  { emoji: "🤔", label: "在想" },
  { emoji: "😴", label: "疲惫" },
  { emoji: "😔", label: "低落" },
  { emoji: "😢", label: "难过" },
  { emoji: "😡", label: "生气" },
  { emoji: "🤒", label: "不舒服" },
  { emoji: "🥲", label: "复杂" }
];

export const WEATHER_PRESETS: Array<{ emoji: string; label: string }> = [
  { emoji: "☀️", label: "晴" },
  { emoji: "🌤", label: "晴间多云" },
  { emoji: "⛅", label: "多云" },
  { emoji: "☁️", label: "阴" },
  { emoji: "🌧", label: "雨" },
  { emoji: "⛈", label: "雷雨" },
  { emoji: "🌨", label: "雪" },
  { emoji: "🌫", label: "雾" },
  { emoji: "🌬", label: "风" },
  { emoji: "🌈", label: "彩虹" }
];

export function weatherLabel(emoji: string): string {
  return WEATHER_PRESETS.find((item) => item.emoji === emoji)?.label ?? "";
}

export function moodLabel(emoji: string): string {
  return MOOD_PRESETS.find((item) => item.emoji === emoji)?.label ?? "";
}

// ---------------------------------------------------------------------------
// 日期
// ---------------------------------------------------------------------------

const WEEKDAY_LABELS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

export function isoOf(year: number, month: number, day: number): string {
  const date = new Date(year, month, day);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/** 本地今天 YYYY-MM-DD（日记的「今天」必须是本地日历日）。 */
export function todayDate(): string {
  const now = new Date();
  return isoOf(now.getFullYear(), now.getMonth(), now.getDate());
}

export function shiftDays(date: string, delta: number): string {
  const parsed = new Date(`${date}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return date;
  parsed.setDate(parsed.getDate() + delta);
  return isoOf(parsed.getFullYear(), parsed.getMonth(), parsed.getDate());
}

function partsOf(date: string): { year: number; month: number; day: number } {
  const [year, month, day] = date.split("-").map((value) => Number.parseInt(value, 10));
  return { year: year || 0, month: (month || 1) - 1, day: day || 1 };
}

export function weekdayOf(date: string): string {
  const { year, month, day } = partsOf(date);
  return WEEKDAY_LABELS[new Date(year, month, day).getDay()] ?? "";
}

/** 「9月8日」 */
export function monthDayLabel(date: string): string {
  const { month, day } = partsOf(date);
  return `${month + 1}月${day}日`;
}

/** 「9月8日 周二」 */
export function fullDayLabel(date: string): string {
  return `${monthDayLabel(date)} ${weekdayOf(date)}`;
}

/** 相对今天的口语标签：今天 / 昨天 / 前天，其余走完整日期。 */
export function relativeDayLabel(date: string, today: string): string {
  if (date === today) return "今天";
  if (date === shiftDays(today, -1)) return "昨天";
  if (date === shiftDays(today, -2)) return "前天";
  const { year } = partsOf(date);
  const todayYear = partsOf(today).year;
  return year === todayYear ? fullDayLabel(date) : `${year}年${monthDayLabel(date)}`;
}

/** ISO 时间戳 → 本地 HH:mm。 */
export function timeOf(iso: string): string {
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return "";
  return `${String(parsed.getHours()).padStart(2, "0")}:${String(parsed.getMinutes()).padStart(2, "0")}`;
}

export type MonthCursor = { year: number; month: number };

export function shiftMonth(cursor: MonthCursor, delta: number): MonthCursor {
  const date = new Date(cursor.year, cursor.month + delta, 1);
  return { year: date.getFullYear(), month: date.getMonth() };
}

export function monthOf(date: string): MonthCursor {
  const { year, month } = partsOf(date);
  return { year, month };
}

// ---------------------------------------------------------------------------
// 排序与分组
// ---------------------------------------------------------------------------

/** `createdAt` → 毫秒（v0.8.7 需求 3.1：两种时间戳格式混存——`…Z` 与 `…+08:00`，
    按字面比较跨格式必错位；解析不出来退 0 沉底）。 */
function createdStamp(entry: DiaryEntry): number {
  const parsed = Date.parse(entry.createdAt);
  return Number.isNaN(parsed) ? 0 : parsed;
}

/** date → 当天的日记（天内按写作先后）。 */
export function diaryByDate(entries: DiaryEntry[]): Map<string, DiaryEntry[]> {
  const map = new Map<string, DiaryEntry[]>();
  for (const entry of entries) {
    const bucket = map.get(entry.date);
    if (bucket) {
      bucket.push(entry);
    } else {
      map.set(entry.date, [entry]);
    }
  }
  for (const bucket of map.values()) {
    bucket.sort((a, b) => {
      const delta = createdStamp(a) - createdStamp(b);
      return delta !== 0 ? delta : a.id.localeCompare(b.id);
    });
  }
  return map;
}

export type DiaryDayGroup = { date: string; entries: DiaryEntry[] };

/** 时间轴分组：一天一组，由近及远。 */
export function dayGroups(entries: DiaryEntry[]): DiaryDayGroup[] {
  return [...diaryByDate(entries).entries()]
    .map(([date, items]) => ({ date, entries: items }))
    .sort((a, b) => (a.date < b.date ? 1 : -1));
}

export type DiaryMonthGroup = {
  key: string;
  year: number;
  month: number;
  label: string;
  count: number;
  days: DiaryDayGroup[];
};

export type DiaryYearGroup = {
  key: string;
  year: number;
  label: string;
  count: number;
  months: DiaryMonthGroup[];
};

/** 分组视图：年 → 月 → 天，全部由近及远。 */
export function yearGroups(entries: DiaryEntry[]): DiaryYearGroup[] {
  const years = new Map<number, Map<number, DiaryDayGroup[]>>();
  for (const group of dayGroups(entries)) {
    const { year, month } = partsOf(group.date);
    const months = years.get(year) ?? new Map<number, DiaryDayGroup[]>();
    const days = months.get(month) ?? [];
    days.push(group);
    months.set(month, days);
    years.set(year, months);
  }
  return [...years.entries()]
    .sort((a, b) => b[0] - a[0])
    .map(([year, months]) => {
      const monthGroups = [...months.entries()]
        .sort((a, b) => b[0] - a[0])
        .map(([month, days]) => ({
          key: `${year}-${month}`,
          year,
          month,
          label: `${year}年${month + 1}月`,
          count: days.reduce((sum, day) => sum + day.entries.length, 0),
          days
        }));
      return {
        key: String(year),
        year,
        label: `${year}年`,
        count: monthGroups.reduce((sum, month) => sum + month.count, 0),
        months: monthGroups
      };
    });
}

// ---------------------------------------------------------------------------
// 日历
// ---------------------------------------------------------------------------

export type DiaryCalendarCell = {
  date: string;
  day: number;
  /** 是否属于当前展示的月份（前后补齐的格子为 false） */
  current: boolean;
  count: number;
  /** 当天第一篇的心情（日历格子里的那个小标记） */
  mood: string;
};

const WEEKDAY_HEADERS = ["日", "一", "二", "三", "四", "五", "六"];

/** 一周从周几开始：0 = 周日，1 = 周一。设置项是 `features.weekStart`（默认周一）。 */
export type WeekStart = 0 | 1;

/** 设置里的字符串归一成 0/1；认不出来一律当周一（默认档）。 */
export function weekStartIndex(value: unknown): WeekStart {
  return value === "sunday" ? 0 : 1;
}

/** 一周七天的表头，按周起始旋转——`weekStart=1` 出「一 二 三 四 五 六 日」。 */
export function calendarWeekdayHeaders(weekStart: WeekStart = 1): string[] {
  return WEEKDAY_HEADERS.slice(weekStart).concat(WEEKDAY_HEADERS.slice(0, weekStart));
}

/** 某月 1 号所在的那一周，从周起始到 1 号之间要垫几个空格子。 */
export function leadingBlanks(year: number, month: number, weekStart: WeekStart): number {
  return (new Date(year, month, 1).getDay() - weekStart + 7) % 7;
}

export function calendarCells(
  cursor: MonthCursor,
  byDate: Map<string, DiaryEntry[]>,
  weekStart: WeekStart = 1
): DiaryCalendarCell[] {
  const { year, month } = cursor;
  const cells: DiaryCalendarCell[] = [];
  const push = (cellYear: number, cellMonth: number, day: number, current: boolean): void => {
    const date = isoOf(cellYear, cellMonth, day);
    const entries = byDate.get(date) ?? [];
    cells.push({
      date,
      day,
      current,
      count: entries.length,
      mood: entries.find((entry) => entry.mood)?.mood ?? ""
    });
  };
  const first = new Date(year, month, 1);
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const leading = leadingBlanks(year, month, weekStart);
  const prevMonthDays = new Date(year, month, 0).getDate();
  for (let index = leading - 1; index >= 0; index--) {
    push(year, month - 1, prevMonthDays - index, false);
  }
  for (let day = 1; day <= daysInMonth; day++) {
    push(year, month, day, true);
  }
  const trailing = (7 - (cells.length % 7)) % 7;
  for (let day = 1; day <= trailing; day++) {
    push(year, month + 1, day, false);
  }
  return cells;
}

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

export type DiaryStats = {
  total: number;
  /** 当前月份写了几篇 */
  monthCount: number;
  /** 连续记录天数（今天还没写不算断，从昨天接着数） */
  streak: number;
  /** 有记录的天数 */
  days: number;
};

export function diaryStats(entries: DiaryEntry[], today: string): DiaryStats {
  const dates = new Set(entries.map((entry) => entry.date));
  const { year, month } = partsOf(today);
  const prefix = `${year}-${String(month + 1).padStart(2, "0")}`;
  const monthCount = entries.filter((entry) => entry.date.startsWith(prefix)).length;
  // 今天还没写不该把连续记录打断，所以从「今天或昨天」里存在的那一天往回数
  let cursor = dates.has(today) ? today : shiftDays(today, -1);
  let streak = 0;
  while (dates.has(cursor)) {
    streak += 1;
    cursor = shiftDays(cursor, -1);
  }
  return { total: entries.length, monthCount, streak, days: dates.size };
}

// ---------------------------------------------------------------------------
// 搜索与摘要
// ---------------------------------------------------------------------------

export function filterDiaries(entries: DiaryEntry[], query: string): DiaryEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return entries;
  return entries.filter((entry) => diaryFold(entry).includes(needle));
}

/**
 * 预折叠索引（v0.8.6 需求 1）：标题 + 正文 + 标签的 lowercase 拼接串。
 * **按对象身份缓存**——日记是逐条不可变更新，改一条只产生一个新对象，
 * 其余的折叠串在后续每次搜索里直接复用，不再对全文重新 `toLowerCase()`。
 * 字段之间用 `\n` 分隔（用户查询里几乎没有换行，跨字段命中只在这种离谱输入下才可能）。
 */
const diaryFolds = new WeakMap<DiaryEntry, string>();

export function diaryFold(entry: DiaryEntry): string {
  const cached = diaryFolds.get(entry);
  if (cached !== undefined) return cached;
  const folded = [entry.title, entry.markdown, ...(entry.tags ?? []).map((tag) => tag.text ?? "")]
    .join("\n")
    .toLowerCase();
  diaryFolds.set(entry, folded);
  return folded;
}

/** 去掉正文的第一行非空行（没有标题时它已经被当作标题显示了，摘要不该再重复一遍）。 */
export function withoutFirstLine(markdown: string): string {
  const lines = markdown.split(/\r?\n/);
  const index = lines.findIndex((line) => line.trim().length > 0);
  return index < 0 ? "" : lines.slice(index + 1).join("\n");
}

/**
 * 卡片摘要：把 Markdown 语法剥成一行可读的纯文本。
 * 图片整张丢掉（`![](x)` 在摘要里只是噪音），链接留下文字。
 */
export function diaryExcerpt(markdown: string, limit = 180): string {
  const text = markdown
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^#{1,6}\s*/gm, "")
    .replace(/^\s*>\s?/gm, "")
    .replace(/^\s*([-*+]|\d+\.)\s+/gm, "")
    .replace(/==([^=]+)==/g, "$1")
    .replace(/[*_~]{1,3}/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > limit ? `${text.slice(0, limit)}…` : text;
}

/** 卡片上那张「有没有图」的小标记。 */
export function diaryImageCount(markdown: string): number {
  return (markdown.match(/!\[[^\]]*\]\([^)]+\)/g) ?? []).length;
}

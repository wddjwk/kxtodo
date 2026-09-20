//! 记账的纯逻辑层（零外部依赖）：金额格式化、按天分组、月历热力、统计聚合、余额推导。
//! 视图层只负责画；core 的 ledger.stats 给 CLI/Agent 用，前端图表自己算同一套口径。

import type {
  LedgerAccount,
  LedgerAccountType,
  LedgerBook,
  LedgerCategory,
  LedgerEntry,
  LedgerSide
} from "./types";
import type { MonthCursor } from "./diary";
import { isoOf, leadingBlanks, shiftDays, todayDate, type WeekStart } from "./diary";

/** 记账条目的插图走 markdown 插图同一条通道，伪条目 id = ledger（与日记的 diary 同款）。 */
export const LEDGER_IMAGE_NODE = "ledger";

/** 占比环的兜底配色：分类自己没填颜色时按序号取，保证一屏里片片区得开
 *  （钻取二级分类尤其需要——子分类默认继承大类颜色，不换调色板整个环就一块色）。 */
export const DONUT_PALETTE = [
  "#f0862c", "#3d8bfd", "#2f9e6e", "#e0654f", "#9b59b6", "#e8a33d",
  "#16a5a5", "#d94f70", "#7cb342", "#6b7fd7", "#c0392b", "#5c6470"
];

export function paletteColor(index: number): string {
  return DONUT_PALETTE[((index % DONUT_PALETTE.length) + DONUT_PALETTE.length) % DONUT_PALETTE.length];
}

/** 分 → 两位小数字符串（带千分位，界面读起来不累）。 */
export function formatCents(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(Math.round(cents));
  const whole = Math.floor(abs / 100).toString();
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const frac = (abs % 100).toString().padStart(2, "0");
  return `${sign}${grouped}.${frac}`;
}

/** 元（用户输入）→ 分；非法返回 null。
 *
 * 与 core 的 `parse_cents`（`crates/core/src/ops_ledger.rs`）**逐条同口径**：逗号先去掉；
 * `.5` 与 `5.` 都收；小数第三位四舍五入、第四位起直接丢；只允许一个小数点；
 * 允许负数（账户的期初/当前金额可以是负的，信用卡尤其如此）。
 *
 * 两套解析器曾经三处不一致（`.5`、`5.`、四位以上小数：core 收而前端拒），于是同一串输入
 * CLI 记进去了、GUI 弹「无效金额」——钱的事不该有两种口径。改这里之前先看 core 那一侧，
 * 两边必须一起动（`src/lib/__tests__/ledger.spec.ts` 有黄金用例守着）。
 */
export function parseYuanToCents(raw: string): number | null {
  const text = raw.trim().replace(/,/g, "");
  if (text === "") return null;
  const negative = text.startsWith("-");
  const unsigned = negative || text.startsWith("+") ? text.slice(1) : text;
  const parts = unsigned.split(".");
  if (parts.length > 2) return null;
  const whole = parts[0];
  const frac = parts.length === 2 ? parts[1] : "0";
  if (whole === "" && frac === "") return null;
  if (!/^\d*$/.test(whole) || !/^\d*$/.test(frac)) return null;
  // 前导零去掉再转数（core 是 trim_start_matches('0') 后 parse，空串按 0）；
  // 超出安全整数就拒——core 那边是 i128 checked_mul，同样会拒。
  const wholeCents = Number(whole.replace(/^0+/, "") || "0") * 100;
  if (!Number.isSafeInteger(wholeCents)) return null;
  const fracDigits = `${frac}000`.slice(0, 3);
  const cents = wholeCents + Number(fracDigits.slice(0, 2)) + (Number(fracDigits[2]) >= 5 ? 1 : 0);
  if (!Number.isSafeInteger(cents)) return null;
  return negative && cents !== 0 ? -cents : cents;
}

/** 列表顺序：日期新→旧，同一天时间晚→早（与 core 的规范序一致）。 */
export function sortEntries(entries: LedgerEntry[]): LedgerEntry[] {
  return [...entries].sort((a, b) => {
    if (a.date !== b.date) return a.date < b.date ? 1 : -1;
    if (a.time !== b.time) return a.time < b.time ? 1 : -1;
    if (a.createdAt !== b.createdAt) return a.createdAt < b.createdAt ? 1 : -1;
    return a.id < b.id ? -1 : 1;
  });
}

// ---------------------------------------------------------------------------
// 按 id 查表的索引
// ---------------------------------------------------------------------------

export type LedgerLookup = {
  categoryById: Map<string, LedgerCategory>;
  accountById: Map<string, LedgerAccount>;
  /** 自定义账户类型按名字查（kind 字符串 = 类型名） */
  accountTypeByName: Map<string, LedgerAccountType>;
};

/**
 * 索引按 **book 对象身份**缓存（WeakMap）：一屏几百行条目每行都要查分类与账户，
 * 早先是每行 6 次 `Array.find`（其中 3 次查的还是同一个分类），300 行 × 60 个分类
 * ≈ 十万次比较，而每次记账写入都会重来一遍。
 *
 * 用 WeakMap 而不是让调用方传 prop：`book` 每次快照刷新都是新对象，旧索引随之被回收，
 * 同一份 book 下的所有行共用一份索引——四处调用点（列表/日历/钻取/搜索）一行都不用改。
 */
const lookupCache = new WeakMap<LedgerBook, LedgerLookup>();

export function ledgerLookup(book: LedgerBook): LedgerLookup {
  const cached = lookupCache.get(book);
  if (cached !== undefined) return cached;
  const built: LedgerLookup = {
    categoryById: new Map(book.categories.map((item) => [item.id, item])),
    accountById: new Map(book.accounts.map((item) => [item.id, item])),
    accountTypeByName: new Map((book.accountTypes ?? []).map((item) => [item.name, item]))
  };
  lookupCache.set(book, built);
  return built;
}

/**
 * 记账搜索：分类名（二级命中时把大类名也算上）、备注、金额（含大类的名字对不上时也认）。
 * 匹配规则与日记/任务各自那条同构（各自模块里一份），结果按时间倒序（最新在前）。
 * 金额按「元」比对：查询里的逗号、空格先去掉，再与 12.34 / 1,234.56 / 1234 各种形态比；
 * **带符号也认**（支出 `-12.34`、收入 `+8888`，与屏幕上画的一致，转账不带符号）。
 */
export function filterLedgerEntries(book: LedgerBook, query: string): LedgerEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return sortEntries(book.entries);
  // **先筛后排**：早先对全部流水做一次 sortEntries 再 filter，命中 5 条也要把 3000 笔
  // 拷贝 + 排序一遍（O(n log n)），而这一步每敲一个键就跑一次。
  const hits = book.entries.filter((entry) => ledgerMatches(book, entry, needle));
  return sortEntries(hits);
}

/**
 * 预折叠索引（v0.8.6 需求 1）：一笔流水的「文字串」与「金额形态串」。
 * - `text` = 转账字样 + 分类名 + 大类名 + 备注（lowercase）；
 * - `amounts` = 金额的几种形态（12.34 / 1,234.56 / 1234 / 带符号），`\n` 分隔。
 *
 * 两者分开缓存是有意的：金额串里带逗号（`formatCents` 的千分位），若与文字串合并，
 * 搜一个「,」会把所有四位数的账都列出来（正是 `amountNeedle` 那道门要挡的东西）。
 *
 * 外层按 `book` 身份索引：分类名住在 `book` 里，只有整本换新（任何一次账本写入）
 * 才需要重算；同一本账下每一笔的身份是稳定的。
 */
export type LedgerFold = { text: string; amounts: string };

const foldCache = new WeakMap<LedgerBook, WeakMap<LedgerEntry, LedgerFold>>();

export function ledgerFold(book: LedgerBook, entry: LedgerEntry): LedgerFold {
  let byEntry = foldCache.get(book);
  if (!byEntry) {
    byEntry = new WeakMap();
    foldCache.set(book, byEntry);
  }
  const cached = byEntry.get(entry);
  if (cached !== undefined) return cached;
  const lookup = ledgerLookup(book);
  const category = entry.categoryId ? lookup.categoryById.get(entry.categoryId) : undefined;
  const parent = category?.parentId ? lookup.categoryById.get(category.parentId) : undefined;
  const text = [entry.kind === "transfer" ? "转账" : "", category?.name ?? "", parent?.name ?? "", entry.note]
    .join("\n")
    .toLowerCase();
  const amount = entry.amountCents / 100;
  const forms = [
    amount.toFixed(2),
    formatCents(entry.amountCents),
    String(amount),
    Math.round(amount).toString()
  ];
  // 界面上支出画的是 `-12.34`、收入 `+12.34`（转账不带符号）：用户照着屏幕敲
  // 带符号的查询也得能命中，而 amountCents 恒为正，光靠上面四种形态永远匹配不上。
  const sign = entry.kind === "income" ? "+" : entry.kind === "expense" ? "-" : "";
  if (sign) forms.push(...forms.map((form) => `${sign}${form}`));
  const fold: LedgerFold = { text, amounts: forms.join("\n") };
  byEntry.set(entry, fold);
  return fold;
}

/** 搜索匹配的单一来源：`filterLedgerEntries` 与全局搜索的扫描器共用这一份。 */
export function ledgerMatches(book: LedgerBook, entry: LedgerEntry, needle: string): boolean {
  const fold = ledgerFold(book, entry);
  if (fold.text.includes(needle)) return true;
  // 查询只由逗号/空格组成时 amountNeedle 是空串，而 `x.includes("")` 恒真——
  // 不加这道门的话搜一个「,」会把整本账都列出来。
  const amountNeedle = needle.replaceAll(",", "").replace(/\s+/g, "");
  if (!amountNeedle) return false;
  return fold.amounts.includes(amountNeedle);
}

/** 一组流水（搜索结果）的收/支/结余合计——转账不计入，与统计口径一致。 */
export function entriesTotals(entries: LedgerEntry[]): { income: number; expense: number } {
  let income = 0;
  let expense = 0;
  for (const entry of entries) {
    if (entry.kind === "income") income += entry.amountCents;
    else if (entry.kind === "expense") expense += entry.amountCents;
  }
  return { income, expense };
}

export type LedgerDayGroup = {
  date: string;
  income: number;
  expense: number;
  entries: LedgerEntry[];
};

/** 某个月的按天卡片（新→旧），空天不出现。 */
export function monthDayGroups(entries: LedgerEntry[], cursor: MonthCursor): LedgerDayGroup[] {
  const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
  const map = new Map<string, LedgerDayGroup>();
  for (const entry of entries) {
    if (!entry.date.startsWith(prefix)) continue;
    let group = map.get(entry.date);
    if (!group) {
      group = { date: entry.date, income: 0, expense: 0, entries: [] };
      map.set(entry.date, group);
    }
    if (entry.kind === "income") group.income += entry.amountCents;
    if (entry.kind === "expense") group.expense += entry.amountCents;
    group.entries.push(entry);
  }
  const groups = [...map.values()];
  groups.sort((a, b) => (a.date < b.date ? 1 : -1));
  for (const group of groups) {
    group.entries = sortEntries(group.entries);
  }
  return groups;
}

/** 某一天的一组账（没有就 null）。日历选中日可能是补格里的上/下个月，不能走按月分组。 */
export function dayGroup(entries: LedgerEntry[], date: string): LedgerDayGroup | null {
  const dayEntries = sortEntries(entries.filter((entry) => entry.date === date));
  if (dayEntries.length === 0) return null;
  let income = 0;
  let expense = 0;
  for (const entry of dayEntries) {
    if (entry.kind === "income") income += entry.amountCents;
    if (entry.kind === "expense") expense += entry.amountCents;
  }
  return { date, income, expense, entries: dayEntries };
}

/** 某个月的收/支合计（转账不计）。 */
export function monthTotals(entries: LedgerEntry[], cursor: MonthCursor): { income: number; expense: number } {
  const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
  let income = 0;
  let expense = 0;
  for (const entry of entries) {
    if (!entry.date.startsWith(prefix)) continue;
    if (entry.kind === "income") income += entry.amountCents;
    if (entry.kind === "expense") expense += entry.amountCents;
  }
  return { income, expense };
}

/** 日历格的紧凑金额：整元不带小数（1,200），有零头才带（328.5 / 3.05）。 */
export function compactCents(cents: number): string {
  const abs = Math.abs(Math.round(cents));
  if (abs % 100 === 0) return formatCents(cents).replace(/\.00$/, "");
  return formatCents(cents).replace(/0$/, "");
}

export type LedgerCalendarCell = {
  date: string;
  day: number;
  /** 非本月（补格） */
  otherMonth: boolean;
  income: number;
  expense: number;
  count: number;
};

/**
 * 月历格：每格带上当天的收/支数额（不做热力着色，数额本身就是最直白的信息）。
 * 格子只有 62px 高，金额用 compactCents 省掉无意义的 .00。
 */
export function ledgerCalendarCells(
  cursor: MonthCursor,
  entries: LedgerEntry[],
  weekStart: WeekStart = 1
): LedgerCalendarCell[] {
  const start = shiftDays(isoOf(cursor.year, cursor.month, 1), -leadingBlanks(cursor.year, cursor.month, weekStart));
  const totals = new Map<string, LedgerCalendarCell>();
  for (const entry of entries) {
    const slot = totals.get(entry.date);
    if (slot) {
      if (entry.kind === "income") slot.income += entry.amountCents;
      if (entry.kind === "expense") slot.expense += entry.amountCents;
      slot.count += 1;
    } else {
      totals.set(entry.date, {
        date: entry.date,
        day: Number.parseInt(entry.date.slice(8, 10), 10),
        otherMonth: false,
        income: entry.kind === "income" ? entry.amountCents : 0,
        expense: entry.kind === "expense" ? entry.amountCents : 0,
        count: 1
      });
    }
  }
  const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
  const cells: LedgerCalendarCell[] = [];
  for (let index = 0; index < 42; index += 1) {
    const date = shiftDays(start, index);
    const slot = totals.get(date);
    cells.push({
      date,
      day: Number.parseInt(date.slice(8, 10), 10),
      otherMonth: !date.startsWith(prefix),
      income: slot?.income ?? 0,
      expense: slot?.expense ?? 0,
      count: slot?.count ?? 0
    });
  }
  // 末尾整周全是下个月就收掉，月历不留空行
  while (cells.length > 35 && cells.slice(35).every((cell) => cell.otherMonth)) {
    cells.length = 35;
  }
  return cells;
}

/** 统计周期：周（周一起）/月/年/总（全部流水跨度）/自定义区间。 */
export type StatsMode = "week" | "month" | "year" | "total" | "custom";

export type StatsBounds = { from: string; to: string };

/**
 * 本周起点。默认周一起（国内习惯），跟着设置 `features.weekStart` 走——
 * 日历的第一列换了，统计里的「本周」也必须跟着换，否则同一个界面里有两个周一。
 */
export function weekStartOf(date: string, weekStart: WeekStart = 1): string {
  const weekday = new Date(`${date}T00:00:00`).getDay();
  return shiftDays(date, -((weekday - weekStart + 7) % 7));
}

export function weekRangeOf(date: string, weekStart: WeekStart = 1): StatsBounds {
  const from = weekStartOf(date, weekStart);
  return { from, to: shiftDays(from, 6) };
}

export function shiftWeek(date: string, delta: number): string {
  return shiftDays(date, delta * 7);
}

function monthBounds(cursor: MonthCursor): StatsBounds {
  const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
  const lastDay = new Date(cursor.year, cursor.month + 1, 0).getDate();
  return { from: `${prefix}-01`, to: `${prefix}-${lastDay.toString().padStart(2, "0")}` };
}

/** 周期 → 起止日期（含两端）。总 = 全部流水的首末；自定义直接用给的起止。 */
export function statsBounds(
  entries: LedgerEntry[],
  mode: StatsMode,
  cursor: MonthCursor,
  anchor = "",
  from = "",
  to = "",
  weekStart: WeekStart = 1
): StatsBounds {
  if (mode === "week") {
    return weekRangeOf(anchor || todayIsoLike(cursor), weekStart);
  }
  if (mode === "month") return monthBounds(cursor);
  if (mode === "year") return { from: `${cursor.year}-01-01`, to: `${cursor.year}-12-31` };
  if (mode === "custom") {
    return { from: from || `${cursor.year}-01-01`, to: to || from || `${cursor.year}-12-31` };
  }
  let min = "";
  let max = "";
  for (const entry of entries) {
    if (!min || entry.date < min) min = entry.date;
    if (!max || entry.date > max) max = entry.date;
  }
  return { from: min || `${cursor.year}-01-01`, to: max || `${cursor.year}-12-31` };
}

function todayIsoLike(cursor: MonthCursor): string {
  return `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}-01`;
}

export function inStatsBounds(date: string, bounds: StatsBounds): boolean {
  return date >= bounds.from && date <= bounds.to;
}

/** 统计窗口判定：占比环/排行不能拿全量数据配当期汇总。 */
export function statsEntries(entries: LedgerEntry[], bounds: StatsBounds): LedgerEntry[] {
  return entries.filter((entry) => inStatsBounds(entry.date, bounds));
}

export type LedgerSeriesPoint = { key: string; income: number; expense: number };

/** 分桶门槛：**必须与 core `ops_ledger.rs::DAY_GRAIN_MAX_DAYS` 同一个数**（那边有一条
 *  include_str! 本文件的测试守着，改一边就会被挡住）。门槛不同 = 同一段区间 GUI 画日桶、
 *  CLI 给月桶。 */
function bucketOf(bounds: StatsBounds): "day" | "month" {
  const days =
    (new Date(`${bounds.to}T00:00:00`).getTime() - new Date(`${bounds.from}T00:00:00`).getTime()) / 86_400_000 + 1;
  return days <= 62 ? "day" : "month";
}

/** 统计序列：按起止区间分桶（day 逐天 / month 逐月），空档补齐曲线不断线。 */
export function statsSeries(entries: LedgerEntry[], bounds: StatsBounds): LedgerSeriesPoint[] {
  const bucket = bucketOf(bounds);
  const buckets = new Map<string, LedgerSeriesPoint>();
  for (const entry of entries) {
    if (!inStatsBounds(entry.date, bounds)) continue;
    const key = bucket === "day" ? entry.date : entry.date.slice(0, 7);
    const point = buckets.get(key) ?? { key, income: 0, expense: 0 };
    if (entry.kind === "income") point.income += entry.amountCents;
    if (entry.kind === "expense") point.expense += entry.amountCents;
    buckets.set(key, point);
  }
  const keys: string[] = [];
  if (bucket === "day") {
    for (let date = bounds.from; date <= bounds.to; date = shiftDays(date, 1)) keys.push(date);
  } else {
    let [year, month] = bounds.from.slice(0, 7).split("-").map(Number);
    const [endYear, endMonth] = bounds.to.slice(0, 7).split("-").map(Number);
    while (year < endYear || (year === endYear && month <= endMonth)) {
      keys.push(`${year}-${month.toString().padStart(2, "0")}`);
      month += 1;
      if (month > 12) {
        month = 1;
        year += 1;
      }
    }
  }
  return keys.map((key) => buckets.get(key) ?? { key, income: 0, expense: 0 });
}

/** 周期标签：年 <2026>、月 <2026/09>、周 <2026/09/14-09/20>、总/自定义 起止全写。 */
export function statsPeriodLabel(mode: StatsMode, bounds: StatsBounds, cursor: MonthCursor): string {
  if (mode === "year") return `${cursor.year}`;
  if (mode === "month") return `${cursor.year}/${(cursor.month + 1).toString().padStart(2, "0")}`;
  if (mode === "week") return `${bounds.from.replaceAll("-", "/")}-${bounds.to.slice(5).replaceAll("-", "/")}`;
  return `${bounds.from.replaceAll("-", "/")}-${bounds.to.replaceAll("-", "/")}`;
}

/** 占比环吃的一项：统计视图给大类，钻取面板给二级分类。 */
export type LedgerDonutItem = {
  id: string;
  name: string;
  cents: number;
  count: number;
  color: string;
};

export type LedgerCategoryStat = {
  categoryId: string;
  name: string;
  side: LedgerSide;
  count: number;
  cents: number;
  percent: number;
  children: { categoryId: string; name: string; count: number; cents: number }[];
};

/** 分类占比：归到大类一级（子分类金额并进父类），未分类单独一组。 */
export function categoryStats(book: LedgerBook, entries: LedgerEntry[], side: LedgerSide): LedgerCategoryStat[] {
  const lookup = ledgerLookup(book);
  const stats: LedgerCategoryStat[] = [];
  // 累积一律走 Map：早先在 per-entry 循环里对累积数组做 `stats.find(...)`、
  // 对 children 又做一次 `find`，是 O(账目 × 分类) —— 3000 笔 × 60 分类 = 十几万次比较，
  // 而切一次周期/收支侧就重跑一遍。
  const groupBy = new Map<string, LedgerCategoryStat>();
  const childBy = new Map<string, LedgerCategoryStat["children"][number]>();
  const pick = (key: string, name: string): LedgerCategoryStat => {
    let slot = groupBy.get(key);
    if (!slot) {
      slot = { categoryId: key, name, side, count: 0, cents: 0, percent: 0, children: [] };
      groupBy.set(key, slot);
      stats.push(slot);
    }
    return slot;
  };
  for (const entry of entries) {
    const entrySide: LedgerSide | null =
      entry.kind === "expense" ? "expense" : entry.kind === "income" ? "income" : null;
    if (entrySide !== side) continue;
    const category = entry.categoryId ? lookup.categoryById.get(entry.categoryId) : undefined;
    const parent = category?.parentId ? lookup.categoryById.get(category.parentId) : undefined;
    const groupKey = parent ? parent.id : category ? category.id : "";
    const groupName = parent ? parent.name : category ? category.name : "未分类";
    const slot = pick(groupKey, groupName);
    slot.count += 1;
    slot.cents += entry.amountCents;
    if (category && parent) {
      const childKey = `${slot.categoryId}\u0000${category.id}`;
      let child = childBy.get(childKey);
      if (!child) {
        child = { categoryId: category.id, name: category.name, count: 0, cents: 0 };
        childBy.set(childKey, child);
        slot.children.push(child);
      }
      child.count += 1;
      child.cents += entry.amountCents;
    }
  }
  const total = stats.reduce((sum, item) => sum + item.cents, 0);
  for (const item of stats) {
    item.percent = total > 0 ? Math.round((item.cents / total) * 10000) / 100 : 0;
    item.children.sort((a, b) => b.cents - a.cents);
  }
  stats.sort((a, b) => b.cents - a.cents);
  return stats;
}

/**
 * 一次遍历算出**所有**账户的余额（期初 + 流水推导）。
 * 早先是每个账户各扫一遍全部流水（`accountBalance` × 账户数）：20 个账户 × 3000 笔
 * = 6 万次判断，而资产视图与账户管理器各算一次、每次记账写入都重跑。
 * 口径与 core 的 `account_balance_cents` 一致：收入 +、支出 −、转账转出 − / 转入 +。
 */
export function accountBalances(book: LedgerBook): Map<string, number> {
  const balances = new Map<string, number>();
  for (const account of book.accounts) balances.set(account.id, account.initialCents);
  for (const entry of book.entries) {
    const from = balances.get(entry.accountId) ?? 0;
    if (entry.kind === "income") {
      balances.set(entry.accountId, from + entry.amountCents);
    } else if (entry.kind === "expense") {
      balances.set(entry.accountId, from - entry.amountCents);
    } else {
      balances.set(entry.accountId, from - entry.amountCents);
      if (entry.toAccountId) {
        balances.set(entry.toAccountId, (balances.get(entry.toAccountId) ?? 0) + entry.amountCents);
      }
    }
  }
  return balances;
}

/** 账户余额 = 期初 + 流水推导（与 core 的 account_balance_cents 同口径）。 */
export function accountBalance(book: LedgerBook, accountId: string): number {
  // 走同一份实现：两套算法迟早会漂移，而这是钱。
  return accountBalances(book).get(accountId) ?? 0;
}

export type LedgerAssets = {
  net: number;
  assets: number;
  liabilities: number;
  perAccount: { account: LedgerAccount; balance: number }[];
};

/** 资产总览：净资产 / 总资产 / 总负债（信用卡负余额计入负债）。 */
export function assetsOverview(book: LedgerBook): LedgerAssets {
  const balances = accountBalances(book);
  const perAccount = [...book.accounts]
    .sort((a, b) => a.order - b.order)
    .map((account) => ({ account, balance: balances.get(account.id) ?? 0 }));
  let net = 0;
  let liabilities = 0;
  for (const item of perAccount) {
    net += item.balance;
    if (item.account.kind === "credit" && item.balance < 0) liabilities += -item.balance;
  }
  return { net, assets: net + liabilities, liabilities, perAccount };
}

export type AssetTrendPoint = { date: string; cents: number };

/**
 * 总资产随时间的变化：从最早的一天（首笔流水或首个账户创建日）扫到**最晚的一天**
 * （今天，或账里日期最晚的那一笔——见下）。
 * 横坐标自适应——按跨度取采样步长，点数封顶 maxPoints（标签不会太密，首尾都落点）。
 * 每个点是**当天结束时**的总资产，口径与 assetsOverview 完全一致
 * （净资产 + 信用类账户负余额的负债），不另立第二套算法。
 */
export function assetsTrend(book: LedgerBook, maxPoints = 90): AssetTrendPoint[] {
  if (book.accounts.length === 0) return [];
  // 终点取 max(今天, 最晚一笔)：账里可以有**未来日期**的一笔（预付下月房租、已定的还款日），
  // 只扫到今天的话曲线右端会漏掉它，与旁边 assetsOverview 的数额对不上——用户看到的就是
  // 「曲线末尾和卡片里的数字不一样」。
  let to = todayDate();
  let from = to;
  for (const entry of book.entries) {
    if (entry.date < from) from = entry.date;
    if (entry.date > to) to = entry.date;
  }
  for (const account of book.accounts) {
    const created = (account.createdAt ?? "").slice(0, 10);
    if (/^\d{4}-\d{2}-\d{2}$/.test(created) && created < from) from = created;
  }
  const spanDays =
    Math.round(
      (new Date(`${to}T00:00:00`).getTime() - new Date(`${from}T00:00:00`).getTime()) / 86_400_000
    ) + 1;
  const step = Math.max(1, Math.ceil(spanDays / maxPoints));

  const sorted = [...book.entries].sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
  const balances = new Map<string, number>();
  for (const account of book.accounts) balances.set(account.id, account.initialCents);
  let cursorIndex = 0;

  function applyThrough(date: string): void {
    while (cursorIndex < sorted.length && sorted[cursorIndex].date <= date) {
      const entry = sorted[cursorIndex];
      cursorIndex += 1;
      const fromBalance = balances.get(entry.accountId) ?? 0;
      if (entry.kind === "income") {
        balances.set(entry.accountId, fromBalance + entry.amountCents);
      } else if (entry.kind === "expense") {
        balances.set(entry.accountId, fromBalance - entry.amountCents);
      } else {
        balances.set(entry.accountId, fromBalance - entry.amountCents);
        if (entry.toAccountId) {
          balances.set(entry.toAccountId, (balances.get(entry.toAccountId) ?? 0) + entry.amountCents);
        }
      }
    }
  }

  function totalAt(): number {
    let net = 0;
    let liabilities = 0;
    for (const account of book.accounts) {
      const balance = balances.get(account.id) ?? 0;
      net += balance;
      if (account.kind === "credit" && balance < 0) liabilities += -balance;
    }
    return net + liabilities;
  }

  const points: AssetTrendPoint[] = [];
  // 起点前一格给「只有期初」的水平段：曲线从左缘就有值，不是凭空冒出来
  points.push({ date: shiftDays(from, -step), cents: totalAt() });
  for (let date = from; date < to; date = shiftDays(date, step)) {
    applyThrough(date);
    points.push({ date, cents: totalAt() });
  }
  applyThrough(to);
  points.push({ date: to, cents: totalAt() });
  return points;
}

/** 两级分类树：大类（含子分类），按 order 排。 */
export function categoryTree(book: LedgerBook, side: LedgerSide): { parent: LedgerCategory; children: LedgerCategory[] }[] {
  const parents = book.categories
    .filter((item) => item.side === side && !item.parentId)
    .sort((a, b) => a.order - b.order);
  return parents.map((parent) => ({
    parent,
    children: book.categories
      .filter((item) => item.side === side && item.parentId === parent.id)
      .sort((a, b) => a.order - b.order)
  }));
}

/** 分类颜色：自己没填就继承大类，再退到侧的默认色。 */
export function categoryColor(book: LedgerBook, category: LedgerCategory | undefined): string {
  if (!category) return "#95a5a6";
  if (category.color) return category.color;
  const parent = category.parentId ? ledgerLookup(book).categoryById.get(category.parentId) : undefined;
  return parent?.color || (category.side === "income" ? "#27ae60" : "#f0862c");
}

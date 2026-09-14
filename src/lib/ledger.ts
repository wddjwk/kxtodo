//! 记账的纯逻辑层（零外部依赖）：金额格式化、按天分组、月历热力、统计聚合、余额推导。
//! 视图层只负责画；core 的 ledger.stats 给 CLI/Agent 用，前端图表自己算同一套口径。

import type {
  LedgerAccount,
  LedgerBook,
  LedgerCategory,
  LedgerEntry,
  LedgerSide
} from "./types";
import type { MonthCursor } from "./diary";
import { isoOf, shiftDays, todayDate } from "./diary";

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

/** 元（用户输入）→ 分；非法返回 null。第三位小数四舍五入。
 *  允许负数（账户的期初/当前金额可以是负的，信用卡尤其如此）；
 *  记账金额是否必须为正在调用方另拦（与 core parse_cents 同口径）。 */
export function parseYuanToCents(raw: string): number | null {
  const text = raw.trim().replace(/,/g, "");
  if (text === "" || !/^[+-]?\d+(\.\d{1,4})?$/.test(text)) return null;
  const negative = text.startsWith("-");
  const unsigned = negative || text.startsWith("+") ? text.slice(1) : text;
  const [whole, frac = ""] = unsigned.split(".");
  const padded = `${frac}000`.slice(0, 3);
  const cents = Number.parseInt(whole, 10) * 100 + Number.parseInt(padded.slice(0, 2), 10);
  const third = Number.parseInt(padded.slice(2, 3), 10);
  const value = cents + (third >= 5 ? 1 : 0);
  return negative ? -value : value;
}

/** 一笔账的带符号展示：支出 -、收入 +、转账按转出方向记 -。 */
export function signedCents(entry: LedgerEntry): number {
  return entry.kind === "income" ? entry.amountCents : -entry.amountCents;
}

export function signedLabel(entry: LedgerEntry): string {
  const value = signedCents(entry);
  return `${value > 0 ? "+" : ""}${formatCents(value)}`;
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
export function ledgerCalendarCells(cursor: MonthCursor, entries: LedgerEntry[]): LedgerCalendarCell[] {
  const first = new Date(cursor.year, cursor.month, 1);
  const startWeekday = first.getDay();
  const start = shiftDays(isoOf(cursor.year, cursor.month, 1), -startWeekday);
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

/** 周一是一周的开始（国内习惯）。 */
export function weekStartOf(date: string): string {
  const weekday = new Date(`${date}T00:00:00`).getDay();
  return shiftDays(date, -((weekday + 6) % 7));
}

export function weekRangeOf(date: string): StatsBounds {
  const from = weekStartOf(date);
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
  to = ""
): StatsBounds {
  if (mode === "week") {
    return weekRangeOf(anchor || todayIsoLike(cursor));
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
  const stats: LedgerCategoryStat[] = [];
  const pick = (key: string, name: string): LedgerCategoryStat => {
    let slot = stats.find((item) => item.categoryId === key);
    if (!slot) {
      slot = { categoryId: key, name, side, count: 0, cents: 0, percent: 0, children: [] };
      stats.push(slot);
    }
    return slot;
  };
  for (const entry of entries) {
    const entrySide: LedgerSide | null =
      entry.kind === "expense" ? "expense" : entry.kind === "income" ? "income" : null;
    if (entrySide !== side) continue;
    const category = entry.categoryId ? book.categories.find((item) => item.id === entry.categoryId) : undefined;
    const parent = category?.parentId ? book.categories.find((item) => item.id === category.parentId) : undefined;
    const groupKey = parent ? parent.id : category ? category.id : "";
    const groupName = parent ? parent.name : category ? category.name : "未分类";
    const slot = pick(groupKey, groupName);
    slot.count += 1;
    slot.cents += entry.amountCents;
    if (category && parent) {
      let child = slot.children.find((item) => item.categoryId === category.id);
      if (!child) {
        child = { categoryId: category.id, name: category.name, count: 0, cents: 0 };
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

/** 账户余额 = 期初 + 流水推导（与 core 的 account_balance_cents 同口径）。 */
export function accountBalance(book: LedgerBook, accountId: string): number {
  const account = book.accounts.find((item) => item.id === accountId);
  let cents = account?.initialCents ?? 0;
  for (const entry of book.entries) {
    if (entry.kind === "income") {
      if (entry.accountId === accountId) cents += entry.amountCents;
    } else if (entry.kind === "expense") {
      if (entry.accountId === accountId) cents -= entry.amountCents;
    } else {
      if (entry.accountId === accountId) cents -= entry.amountCents;
      if (entry.toAccountId === accountId) cents += entry.amountCents;
    }
  }
  return cents;
}

export type LedgerAssets = {
  net: number;
  assets: number;
  liabilities: number;
  perAccount: { account: LedgerAccount; balance: number }[];
};

/** 资产总览：净资产 / 总资产 / 总负债（信用卡负余额计入负债）。 */
export function assetsOverview(book: LedgerBook): LedgerAssets {
  const perAccount = [...book.accounts]
    .sort((a, b) => a.order - b.order)
    .map((account) => ({ account, balance: accountBalance(book, account.id) }));
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
 * 总资产随时间的变化：从最早的一天（首笔流水或首个账户创建日）扫到今天。
 * 横坐标自适应——按跨度取采样步长，点数封顶 maxPoints（标签不会太密，首尾都落点）。
 * 每个点是**当天结束时**的总资产，口径与 assetsOverview 完全一致
 * （净资产 + 信用类账户负余额的负债），不另立第二套算法。
 */
export function assetsTrend(book: LedgerBook, maxPoints = 90): AssetTrendPoint[] {
  if (book.accounts.length === 0) return [];
  const to = todayDate();
  let from = to;
  for (const entry of book.entries) {
    if (entry.date < from) from = entry.date;
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
  const parent = category.parentId ? book.categories.find((item) => item.id === category.parentId) : undefined;
  return parent?.color || (category.side === "income" ? "#27ae60" : "#f0862c");
}

export function accountName(book: LedgerBook, accountId: string): string {
  return book.accounts.find((item) => item.id === accountId)?.name ?? "已删除账户";
}

export function categoryName(book: LedgerBook, categoryId?: string): string {
  if (!categoryId) return "";
  return book.categories.find((item) => item.id === categoryId)?.name ?? "";
}

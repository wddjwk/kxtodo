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
import { isoOf, shiftDays } from "./diary";

/** 分 → 两位小数字符串（带千分位，界面读起来不累）。 */
export function formatCents(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(Math.round(cents));
  const whole = Math.floor(abs / 100).toString();
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const frac = (abs % 100).toString().padStart(2, "0");
  return `${sign}${grouped}.${frac}`;
}

/** 元（用户输入）→ 分；非法或负数返回 null。第三位小数四舍五入。 */
export function parseYuanToCents(raw: string): number | null {
  const text = raw.trim().replace(/,/g, "");
  if (text === "" || !/^\d+(\.\d{1,4})?$/.test(text)) return null;
  const [whole, frac = ""] = text.split(".");
  const padded = `${frac}000`.slice(0, 3);
  const cents = Number.parseInt(whole, 10) * 100 + Number.parseInt(padded.slice(0, 2), 10);
  const third = Number.parseInt(padded.slice(2, 3), 10);
  return cents + (third >= 5 ? 1 : 0);
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

/** 统计窗口判定：与 statsSeries 同一口径，占比环/排行不能拿全量数据配当期汇总。 */
export function inStatsRange(date: string, mode: "month" | "year", cursor: MonthCursor): boolean {
  if (mode === "month") {
    const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
    return date.startsWith(prefix);
  }
  return date.startsWith(`${cursor.year}-`);
}

export function statsEntries(
  entries: LedgerEntry[],
  mode: "month" | "year",
  cursor: MonthCursor
): LedgerEntry[] {
  return entries.filter((entry) => inStatsRange(entry.date, mode, cursor));
}

export type LedgerSeriesPoint = { key: string; income: number; expense: number };

/** 统计序列：月视图逐天、年视图逐月、自定义区间按跨度自动选。 */
export function statsSeries(
  entries: LedgerEntry[],
  mode: "month" | "year" | "range",
  cursor: MonthCursor,
  from?: string,
  to?: string
): LedgerSeriesPoint[] {
  const buckets = new Map<string, LedgerSeriesPoint>();
  const keyOf = (date: string): string => (mode === "month" ? date : date.slice(0, 7));
  const inRange = (date: string): boolean => {
    if (mode === "month") {
      const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
      return date.startsWith(prefix);
    }
    if (mode === "year") return date.startsWith(`${cursor.year}-`);
    if (from && date < from) return false;
    if (to && date > to) return false;
    return true;
  };
  for (const entry of entries) {
    if (!inRange(entry.date)) continue;
    const key = keyOf(entry.date);
    const point = buckets.get(key) ?? { key, income: 0, expense: 0 };
    if (entry.kind === "income") point.income += entry.amountCents;
    if (entry.kind === "expense") point.expense += entry.amountCents;
    buckets.set(key, point);
  }
  // 补齐空档，曲线不断线
  const keys: string[] = [];
  if (mode === "month") {
    const days = new Date(cursor.year, cursor.month + 1, 0).getDate();
    const prefix = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}`;
    for (let day = 1; day <= days; day += 1) keys.push(`${prefix}-${day.toString().padStart(2, "0")}`);
  } else if (mode === "year") {
    for (let month = 1; month <= 12; month += 1) keys.push(`${cursor.year}-${month.toString().padStart(2, "0")}`);
  } else {
    keys.push(...[...buckets.keys()].sort());
  }
  return keys.map((key) => buckets.get(key) ?? { key, income: 0, expense: 0 });
}

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

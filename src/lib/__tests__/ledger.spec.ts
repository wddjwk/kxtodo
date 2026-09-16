/**
 * 记账纯逻辑层（src/lib/ledger.ts）的单元测试。
 *
 * **期望值以 Rust core 为权威**：金额解析对 `crates/core/src/ops_ledger.rs::parse_cents`，
 * 余额对 `account_balance_cents`，统计对 `ledger_stats`，资产对 `ledger_balance`，
 * 顺序对 `sort_entries`；下面标着「core 黄金用例」的几条与
 * `crates/core/tests/ledger_money.rs` 里的同一本账算出同一个数（同一份账本两侧收敛，
 * 这是「前端是 core 的第二套实现」唯一能守住的办法）。
 *
 * 测试**只描述现状**，不修代码：与 core 不一致或行为可疑的地方写成用例并在注释里标
 * 「现状/不一致」，同时汇总在给用户的报告里。
 */
import { describe, expect, it } from "vitest";

import type {
  LedgerAccount,
  LedgerAccountType,
  LedgerBook,
  LedgerCategory,
  LedgerEntry,
  LedgerKind,
  LedgerSide
} from "../types";
import type { MonthCursor } from "../diary";
import { shiftDays, todayDate } from "../diary";
import {
  accountBalance,
  accountBalances,
  assetsOverview,
  assetsTrend,
  categoryColor,
  categoryStats,
  compactCents,
  dayGroup,
  entriesTotals,
  filterLedgerEntries,
  formatCents,
  ledgerCalendarCells,
  ledgerLookup,
  monthDayGroups,
  monthTotals,
  parseYuanToCents,
  sortEntries,
  statsBounds,
  statsSeries,
  weekStartOf
} from "../ledger";

// ---------------------------------------------------------------------------
// 数据工厂：默认值填满，用例只写自己关心的字段
// ---------------------------------------------------------------------------

let entrySeq = 0;

function makeEntry(overrides: Partial<LedgerEntry> = {}): LedgerEntry {
  entrySeq += 1;
  return {
    id: `le-${entrySeq.toString().padStart(4, "0")}`,
    kind: "expense" as LedgerKind,
    amountCents: 1000,
    accountId: "lacc-01",
    date: "2026-09-08",
    time: "12:00:00",
    note: "",
    createdAt: "2026-09-08T12:00:00",
    ...overrides
  };
}

function makeAccount(overrides: Partial<LedgerAccount> = {}): LedgerAccount {
  return {
    id: "lacc-01",
    name: "现金",
    icon: "Wallet",
    color: "#e8a33d",
    kind: "cash",
    initialCents: 0,
    note: "",
    order: 1,
    createdAt: "2026-09-01T00:00:00.000Z",
    ...overrides
  };
}

function makeCategory(overrides: Partial<LedgerCategory> = {}): LedgerCategory {
  return {
    id: "lcat-exp-01",
    name: "餐饮",
    side: "expense" as LedgerSide,
    icon: "Utensils",
    color: "#f0862c",
    order: 1,
    createdAt: "2026-09-01T00:00:00.000Z",
    ...overrides
  };
}

function makeBook(overrides: Partial<LedgerBook> = {}): LedgerBook {
  return { accounts: [], categories: [], entries: [], accountTypes: [], ...overrides };
}

/** 2026 年 9 月（MonthCursor.month 是 0 基）。 */
const SEP: MonthCursor = { year: 2026, month: 8 };
const AUG: MonthCursor = { year: 2026, month: 7 };

/**
 * core 种子账本的子集（id / 名字 / 颜色与 `LedgerFile::seed_defaults`、
 * 前端 `seedLedgerBook()` 三方一致），ledger_money.rs 的黄金用例就建在这套 id 上。
 */
function coreSeedBook(): LedgerBook {
  const accounts = [
    makeAccount({ id: "lacc-01", name: "现金", kind: "cash", order: 1 }),
    makeAccount({ id: "lacc-02", name: "微信", kind: "other", icon: "MessageCircle", color: "#2aae67", order: 2 }),
    makeAccount({ id: "lacc-03", name: "支付宝", kind: "other", icon: "Smartphone", color: "#1677ff", order: 3 }),
    makeAccount({ id: "lacc-04", name: "储蓄卡", kind: "debit", icon: "Landmark", color: "#b23a48", order: 4 })
  ];
  const categories = [
    makeCategory({ id: "lcat-exp-01", name: "餐饮", color: "#f0862c", order: 1 }),
    makeCategory({ id: "lcat-exp-01-01", name: "早餐", parentId: "lcat-exp-01", color: "", icon: "Coffee", order: 1 }),
    makeCategory({ id: "lcat-exp-01-02", name: "午餐", parentId: "lcat-exp-01", color: "", order: 2 }),
    makeCategory({ id: "lcat-exp-01-03", name: "晚餐", parentId: "lcat-exp-01", color: "", order: 3 }),
    makeCategory({ id: "lcat-exp-01-05", name: "饮料", parentId: "lcat-exp-01", color: "", order: 5 }),
    makeCategory({ id: "lcat-exp-02", name: "交通", color: "#4a90d9", order: 2 }),
    makeCategory({ id: "lcat-exp-02-02", name: "打车", parentId: "lcat-exp-02", color: "", order: 2 }),
    makeCategory({ id: "lcat-inc-01", name: "工资", side: "income", color: "#27ae60", icon: "Banknote", order: 1 }),
    makeCategory({ id: "lcat-inc-01-01", name: "工资薪金", side: "income", parentId: "lcat-inc-01", color: "", order: 1 })
  ];
  return makeBook({ accounts, categories });
}

// ---------------------------------------------------------------------------
// formatCents
// ---------------------------------------------------------------------------

describe("formatCents（分 → 带千分位的两位小数）", () => {
  it("正常路径：正负、千分位、分位补零", () => {
    expect(formatCents(5)).toBe("0.05"); // 分位补零
    expect(formatCents(0)).toBe("0.00");
    expect(formatCents(100)).toBe("1.00");
    expect(formatCents(1050)).toBe("10.50");
    expect(formatCents(-1234)).toBe("-12.34");
    expect(formatCents(-5)).toBe("-0.05");
    expect(formatCents(123456789)).toBe("1,234,567.89"); // 大额千分位
    expect(formatCents(100000000)).toBe("1,000,000.00");
  });

  it("边界：负零不带负号（界面不该出现 -0.00）", () => {
    expect(formatCents(-0)).toBe("0.00");
    expect(formatCents(-0)).not.toBe("-0.00");
  });

  it("边界（现状）：非整数入参先四舍五入到整分 —— 正负不对称", () => {
    expect(formatCents(10.4)).toBe("0.10");
    expect(formatCents(10.5)).toBe("0.11"); // 正数半值进位
    // Math.round(-10.5) === -10（JS 的半值向 +∞ 取整），于是负数半值是「向零」而不是「远离零」：
    // 与正数不对称。实践中 amountCents 恒为整数（parseYuanToCents 已经取过整），
    // 这是潜在的口径瑕疵而不是现行 bug。详见报告。
    expect(formatCents(-10.5)).toBe("-0.10");
    expect(formatCents(-10.6)).toBe("-0.11");
  });
});

// ---------------------------------------------------------------------------
// parseYuanToCents —— 与 core 的 parse_cents 逐条同口径（重点）
// ---------------------------------------------------------------------------

describe("parseYuanToCents（元 → 分，与 core parse_cents 对齐）", () => {
  it("正常路径：两位小数、整数、千分位逗号", () => {
    expect(parseYuanToCents("12.34")).toBe(1234);
    expect(parseYuanToCents("0.01")).toBe(1);
    expect(parseYuanToCents("100")).toBe(10000);
    expect(parseYuanToCents("1,234.56")).toBe(123456); // 逗号先去掉
    expect(parseYuanToCents("1,2,3,4")).toBe(123400);
    expect(parseYuanToCents("  7  ")).toBe(700); // 前后空白 trim
    expect(parseYuanToCents("007")).toBe(700); // 前导零
    expect(parseYuanToCents("000")).toBe(0);
  });

  it("边界：`.5` 与 `5.` 都收（core 收，前端曾经拒）", () => {
    expect(parseYuanToCents(".5")).toBe(50);
    expect(parseYuanToCents("5.")).toBe(500);
    expect(parseYuanToCents("-.5")).toBe(-50);
    expect(parseYuanToCents("+.5")).toBe(50);
  });

  it("边界：第三位四舍五入，第四位起直接丢（不是四舍五入）", () => {
    expect(parseYuanToCents("1.235")).toBe(124); // 第三位 5 → 进位
    expect(parseYuanToCents("1.234")).toBe(123);
    expect(parseYuanToCents("1.2349")).toBe(123); // 第四位不看
    expect(parseYuanToCents("1.23456")).toBe(123); // ← 不是 124
    expect(parseYuanToCents("0.005")).toBe(1);
    expect(parseYuanToCents("0.004")).toBe(0); // 亚分金额落到 0（调用方自己守 > 0，core 也在写入口拒 0）
  });

  it("正常路径：符号", () => {
    expect(parseYuanToCents("-3.5")).toBe(-350);
    expect(parseYuanToCents("+2")).toBe(200);
    expect(parseYuanToCents("-1000")).toBe(-100000);
  });

  it("边界：`-0` 返回正零（刻意规避 JS 负零）", () => {
    const value = parseYuanToCents("-0");
    expect(value).toBe(0);
    expect(Object.is(value, 0)).toBe(true);
    expect(Object.is(value, -0)).toBe(false);
    // -0.00 同理
    expect(Object.is(parseYuanToCents("-0.00"), 0)).toBe(true);
  });

  it("边界：非法输入一律 null", () => {
    for (const raw of ["", "   ", ".", "abc", "1.2.3", "1e3", "0x10", "--1", "1.-2", "12a", "1２", "NaN", "Infinity", "+-1", "1,2.3.4"]) {
      expect(parseYuanToCents(raw), `parseYuanToCents(${JSON.stringify(raw)})`).toBeNull();
    }
  });

  it("边界（两侧一致）：裸符号 `-` / `+` 解析成 0（不是 null）", () => {
    // 前端：unsigned 变成空串 → whole "" 通过 /^\d*$/ → 0；
    // core：digits_part 为空 → whole "" 且 frac 落到默认 "0" → 同样 Some(0)。
    // 两侧同口径，所以不算不一致；调用方（LedgerEditor 的 cents <= 0、core 的
    // LEDGER_AMOUNT_INVALID 写入门）负责把 0 挡在门外。
    expect(parseYuanToCents("-")).toBe(0);
    expect(parseYuanToCents("+")).toBe(0);
    expect(Object.is(parseYuanToCents("-"), -0)).toBe(false);
  });

  it("边界：超出安全整数拒绝（core 那边是 i64::try_from 失败返回 None）", () => {
    expect(parseYuanToCents("12345678901234567890")).toBeNull(); // 20 位整数
    expect(parseYuanToCents("99999999999999999999999999999999999999.99")).toBeNull();
  });

  it("不一致（现状）：前端按 Number.isSafeInteger 卡上限，core 按 i64 —— 2^53 分这一段 core 收、前端拒", () => {
    // 90071992547409.91 元 = 9007199254740991 分 = Number.MAX_SAFE_INTEGER，两侧都收
    expect(parseYuanToCents("90071992547409.91")).toBe(9007199254740991);
    // 再多一分就超出安全整数：前端 null，而 core 的 i64 上限是 9.22e18 分（≈9.2e16 元），
    // 这一段（2^53 ~ i64::MAX）CLI 收、GUI 弹「无效金额」。详见报告。
    expect(parseYuanToCents("90071992547409.92")).toBeNull();
    expect(parseYuanToCents("9007199254740993")).toBeNull();
  });

  it("不一致（现状）：40 位以上的整数部分 core 会静默当成 0，前端拒", () => {
    // core: whole.trim_start_matches('0').parse::<i128>() 溢出 → unwrap_or(0) → 0*100 = 0
    // → 结果是 Some(0)（记一笔 0.00 的账，虽然后面 amount<=0 的写入门会拦住）；
    // 前端: Number.isSafeInteger 检查 → null。详见报告。
    const huge = "1".repeat(40);
    expect(parseYuanToCents(huge)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// compactCents
// ---------------------------------------------------------------------------

describe("compactCents（日历格的紧凑金额）", () => {
  it("正常路径：整元去掉 .00，有零头保留", () => {
    expect(compactCents(120000)).toBe("1,200");
    expect(compactCents(32850)).toBe("328.5");
    expect(compactCents(305)).toBe("3.05");
    expect(compactCents(1000)).toBe("10");
    expect(compactCents(1000000)).toBe("10,000");
  });

  it("边界：零、负数、亚角、只去掉一个尾零", () => {
    expect(compactCents(0)).toBe("0");
    expect(compactCents(-32850)).toBe("-328.5");
    expect(compactCents(-120000)).toBe("-1,200");
    expect(compactCents(5)).toBe("0.05");
    expect(compactCents(10)).toBe("0.1");
    expect(compactCents(20)).toBe("0.2");
    expect(compactCents(2000)).toBe("20"); // 不是 "2"
  });
});

// ---------------------------------------------------------------------------
// sortEntries
// ---------------------------------------------------------------------------

describe("sortEntries（日期新→旧，同日时间晚→早，再 createdAt，最后 id）", () => {
  const raw = () => [
    makeEntry({ id: "s1", date: "2026-09-01", time: "10:00:00", createdAt: "2026-09-01T10:00:00" }),
    makeEntry({ id: "s3", date: "2026-09-02", time: "09:00:00", createdAt: "2026-09-02T09:00:00" }),
    makeEntry({ id: "s2", date: "2026-09-02", time: "18:00:00", createdAt: "2026-09-02T18:00:00" }),
    makeEntry({ id: "b", date: "2026-09-03", time: "", createdAt: "2026-09-03T01:00:00" }),
    makeEntry({ id: "a", date: "2026-09-03", time: "", createdAt: "2026-09-03T01:00:00" }),
    makeEntry({ id: "d", date: "2026-09-03", time: "", createdAt: "2026-09-03T02:00:00" })
  ];

  it("正常路径：四级比较键（与 core sort_entries 同口径）", () => {
    const sorted = sortEntries(raw());
    expect(sorted.map((entry) => entry.id)).toEqual(["d", "a", "b", "s2", "s3", "s1"]);
  });

  it("边界：不改动入参数组，返回新数组", () => {
    const input = raw();
    const before = input.map((entry) => entry.id);
    const sorted = sortEntries(input);
    expect(sorted).not.toBe(input);
    expect(input.map((entry) => entry.id)).toEqual(before);
  });

  it("边界：空数组与单元素", () => {
    expect(sortEntries([])).toEqual([]);
    const single = [makeEntry({ id: "only" })];
    expect(sortEntries(single).map((entry) => entry.id)).toEqual(["only"]);
  });

  it("边界：time 为空串的排在同一天的最后（字符串降序，与 core 的 b.time.cmp(&a.time) 一致）", () => {
    const sorted = sortEntries([
      makeEntry({ id: "no-time", date: "2026-09-05", time: "" }),
      makeEntry({ id: "early", date: "2026-09-05", time: "08:00:00" }),
      makeEntry({ id: "late", date: "2026-09-05", time: "20:00:00" })
    ]);
    expect(sorted.map((entry) => entry.id)).toEqual(["late", "early", "no-time"]);
  });
});

// ---------------------------------------------------------------------------
// monthDayGroups / monthTotals / dayGroup
// ---------------------------------------------------------------------------

describe("monthDayGroups / monthTotals / dayGroup（按天分组）", () => {
  function monthBook() {
    return makeBook({
      entries: [
        makeEntry({ id: "sep-3-b", amountCents: 3000, date: "2026-09-03", time: "09:00:00", categoryId: "lcat-exp-02-02" }),
        makeEntry({ id: "sep-3-a", amountCents: 700, date: "2026-09-03", time: "18:00:00", categoryId: "lcat-exp-01-05" }),
        makeEntry({ id: "sep-2", amountCents: 2000, date: "2026-09-02" }),
        makeEntry({ id: "sep-1", amountCents: 1050, date: "2026-09-01" }),
        makeEntry({ id: "sep-income", kind: "income", amountCents: 8888, date: "2026-09-01" }),
        makeEntry({ id: "sep-transfer", kind: "transfer", amountCents: 500, date: "2026-09-02", toAccountId: "lacc-02" }),
        makeEntry({ id: "aug-31", amountCents: 111, date: "2026-08-31" }),
        makeEntry({ id: "oct-1", amountCents: 222, date: "2026-10-01" })
      ]
    });
  }

  it("正常路径：跨月数据只取目标月，空天不出现，组按日期降序", () => {
    const groups = monthDayGroups(monthBook().entries, SEP);
    expect(groups.map((group) => group.date)).toEqual(["2026-09-03", "2026-09-02", "2026-09-01"]);
    expect(monthDayGroups(monthBook().entries, AUG).map((group) => group.date)).toEqual(["2026-08-31"]);
    // 9 月没有账的日子不会凭空出现
    expect(groups.some((group) => group.date === "2026-09-04")).toBe(false);
  });

  it("正常路径：组内顺序 = sortEntries（时间晚的在前）", () => {
    const groups = monthDayGroups(monthBook().entries, SEP);
    const third = groups.find((group) => group.date === "2026-09-03");
    expect(third?.entries.map((entry) => entry.id)).toEqual(["sep-3-a", "sep-3-b"]);
  });

  it("正常路径：转账不计入 income/expense，但仍在当天的条目列表里", () => {
    const groups = monthDayGroups(monthBook().entries, SEP);
    const second = groups.find((group) => group.date === "2026-09-02");
    expect(second?.expense).toBe(2000);
    expect(second?.income).toBe(0);
    // 两笔的 time / createdAt 都相同 → 落到第四级比较键 id 升序
    expect(second?.entries.map((entry) => entry.id)).toEqual(["sep-2", "sep-transfer"]);
  });

  it("正常路径：monthTotals 只合计目标月的收/支（转账不计）", () => {
    const entries = monthBook().entries;
    expect(monthTotals(entries, SEP)).toEqual({ income: 8888, expense: 1050 + 2000 + 700 + 3000 });
    expect(monthTotals(entries, AUG)).toEqual({ income: 0, expense: 111 });
    expect(monthTotals(entries, { year: 2026, month: 9 })).toEqual({ income: 0, expense: 222 });
    expect(monthTotals([], SEP)).toEqual({ income: 0, expense: 0 });
  });

  it("边界：dayGroup 对没有账的日期返回 null，对补格里的别月日期照常工作", () => {
    const entries = monthBook().entries;
    expect(dayGroup(entries, "2026-09-30")).toBeNull();
    expect(dayGroup([], "2026-09-01")).toBeNull();
    const august = dayGroup(entries, "2026-08-31");
    expect(august).not.toBeNull();
    expect(august?.expense).toBe(111);
    expect(august?.entries.map((entry) => entry.id)).toEqual(["aug-31"]);
    const first = dayGroup(entries, "2026-09-01");
    expect(first?.income).toBe(8888);
    expect(first?.expense).toBe(1050);
  });
});

// ---------------------------------------------------------------------------
// ledgerCalendarCells
// ---------------------------------------------------------------------------

describe("ledgerCalendarCells（月历格）", () => {
  it("正常路径：2026-09 收成 35 格（末尾整周全是下个月），周一打头", () => {
    const cells = ledgerCalendarCells(SEP, []);
    expect(cells).toHaveLength(35);
    expect(cells[0].date).toBe("2026-08-31"); // 周一
    expect(cells[34].date).toBe("2026-10-04");
    expect(cells.filter((cell) => cell.otherMonth)).toHaveLength(5); // 8/31、10/1~10/4
    expect(cells[0].otherMonth).toBe(true);
    expect(cells[1].otherMonth).toBe(false); // 9/1
    expect(cells[1].day).toBe(1);
  });

  it("正常路径：2026-08 需要第六周 → 42 格", () => {
    const cells = ledgerCalendarCells(AUG, []);
    expect(cells).toHaveLength(42);
    expect(cells[0].date).toBe("2026-07-27"); // 周一
    expect(cells[41].date).toBe("2026-09-06");
  });

  it("设置成周日打头时首格是周日", () => {
    const cells = ledgerCalendarCells(SEP, [], 0);
    expect(cells[0].date).toBe("2026-08-30"); // 周日
    expect(cells[cells.length - 1].date).toBe("2026-10-03");
  });

  it("边界：首格永远是一周的第一天（默认周一），格子连续不重不漏", () => {
    for (const weekStart of [0, 1] as const) {
      for (const cursor of [SEP, AUG, { year: 2026, month: 1 }, { year: 2024, month: 1 }]) {
        const cells = ledgerCalendarCells(cursor, [], weekStart);
        expect(new Date(`${cells[0].date}T00:00:00`).getDay()).toBe(weekStart);
        cells.forEach((cell, index) => {
          if (index === 0) return;
          expect(cell.date).toBe(shiftDays(cells[index - 1].date, 1));
        });
        expect(new Set(cells.map((cell) => cell.date)).size).toBe(cells.length);
      }
    }
  });

  it("正常路径：每格的 income / expense / count；转账两边都不计但算 count", () => {
    const entries = [
      makeEntry({ id: "a", amountCents: 1050, date: "2026-09-01" }),
      makeEntry({ id: "b", kind: "income", amountCents: 20000, date: "2026-09-01" }),
      makeEntry({ id: "c", kind: "transfer", amountCents: 500, date: "2026-09-01", toAccountId: "lacc-02" }),
      makeEntry({ id: "d", amountCents: 300, date: "2026-08-31" }) // 补格里的上个月（周一）
    ];
    const cells = ledgerCalendarCells(SEP, entries);
    const first = cells.find((cell) => cell.date === "2026-09-01");
    expect(first).toEqual({ date: "2026-09-01", day: 1, otherMonth: false, income: 20000, expense: 1050, count: 3 });
    const padding = cells.find((cell) => cell.date === "2026-08-31");
    expect(padding?.otherMonth).toBe(true);
    expect(padding?.expense).toBe(300);
    expect(padding?.count).toBe(1);
    // 没有账的格子是零，不是 undefined
    expect(cells.find((cell) => cell.date === "2026-09-15")).toEqual({
      date: "2026-09-15",
      day: 15,
      otherMonth: false,
      income: 0,
      expense: 0,
      count: 0
    });
  });
});

// ---------------------------------------------------------------------------
// statsBounds / weekStartOf / statsSeries
// ---------------------------------------------------------------------------

describe("statsBounds / weekStartOf（统计周期）", () => {
  it("正常路径：周一起算，month/year/total/custom 各自的起止", () => {
    expect(weekStartOf("2026-09-16")).toBe("2026-09-14"); // 周三 → 周一
    expect(weekStartOf("2026-09-14")).toBe("2026-09-14"); // 周一不动
    expect(weekStartOf("2026-09-20")).toBe("2026-09-14"); // 周日仍属这一周
    // 设置成周日打头：9/20 是周日 → 它就是这一周的第一天
    expect(weekStartOf("2026-09-16", 0)).toBe("2026-09-13");
    expect(weekStartOf("2026-09-20", 0)).toBe("2026-09-20");
    expect(statsBounds([], "week", SEP, "2026-09-16", "", "", 0)).toEqual({ from: "2026-09-13", to: "2026-09-19" });
    expect(statsBounds([], "week", SEP, "2026-09-16")).toEqual({ from: "2026-09-14", to: "2026-09-20" });
    expect(statsBounds([], "month", SEP)).toEqual({ from: "2026-09-01", to: "2026-09-30" });
    expect(statsBounds([], "month", { year: 2024, month: 1 })).toEqual({ from: "2024-02-01", to: "2024-02-29" }); // 闰月
    expect(statsBounds([], "year", SEP)).toEqual({ from: "2026-01-01", to: "2026-12-31" });
    expect(statsBounds([], "custom", SEP, "", "2026-09-05", "")).toEqual({ from: "2026-09-05", to: "2026-09-05" });
    expect(statsBounds([], "custom", SEP, "", "2026-09-05", "2026-10-05")).toEqual({ from: "2026-09-05", to: "2026-10-05" });
    expect(statsBounds([], "custom", SEP)).toEqual({ from: "2026-01-01", to: "2026-12-31" }); // 缺省回落整年
  });

  it("正常路径：total = 全部流水的首末，没有流水时回落整年", () => {
    const entries = [
      makeEntry({ date: "2026-09-03" }),
      makeEntry({ date: "2026-01-05" }),
      makeEntry({ date: "2026-05-20" })
    ];
    expect(statsBounds(entries, "total", SEP)).toEqual({ from: "2026-01-05", to: "2026-09-03" });
    expect(statsBounds([], "total", SEP)).toEqual({ from: "2026-01-01", to: "2026-12-31" });
  });
});

describe("statsSeries（统计序列分桶）", () => {
  const dayEntries = [
    makeEntry({ id: "d1", amountCents: 1050, date: "2026-09-01" }),
    makeEntry({ id: "d2", amountCents: 2000, date: "2026-09-03" }),
    makeEntry({ id: "d3", kind: "income", amountCents: 8888, date: "2026-09-03" }),
    makeEntry({ id: "d4", kind: "transfer", amountCents: 500, date: "2026-09-03", toAccountId: "lacc-02" })
  ];

  it("回归（v0.7.5「NaN 月」）：跨月长区间走 month 桶，key 是 YYYY-MM 且没有 NaN", () => {
    const series = statsSeries(dayEntries, { from: "2026-07-01", to: "2026-10-31" });
    expect(series.map((point) => point.key)).toEqual(["2026-07", "2026-08", "2026-09", "2026-10"]);
    for (const point of series) {
      // 历史 bug：day 键（长度 10）被误判成 month 桶，LedgerStats 拿 Number("09-14") → NaN
      expect(point.key).toMatch(/^\d{4}-\d{2}$/);
      expect(point.key).toHaveLength(7);
      expect(Number.isFinite(point.income)).toBe(true);
      expect(Number.isFinite(point.expense)).toBe(true);
      // 消费侧（LedgerStats.svelte:189/454）就是这么解 key 的，这里替它验一遍
      expect(Number.isNaN(Number(point.key.slice(0, 4)))).toBe(false);
      expect(Number.isNaN(Number(point.key.slice(5, 7)))).toBe(false);
    }
    expect(series[2]).toEqual({ key: "2026-09", income: 8888, expense: 3050 });
    expect(series[0]).toEqual({ key: "2026-07", income: 0, expense: 0 });
  });

  it("正常路径：62 天内走 day 桶，key 是 YYYY-MM-DD，空档补零、曲线不断线、首尾都落点", () => {
    const series = statsSeries(dayEntries, { from: "2026-09-01", to: "2026-09-05" });
    expect(series.map((point) => point.key)).toEqual([
      "2026-09-01",
      "2026-09-02",
      "2026-09-03",
      "2026-09-04",
      "2026-09-05"
    ]);
    for (const point of series) {
      expect(point.key).toHaveLength(10);
      expect(Number.isNaN(Number(point.key.slice(8)))).toBe(false);
    }
    expect(series[1]).toEqual({ key: "2026-09-02", income: 0, expense: 0 }); // 空档补零
    expect(series[2]).toEqual({ key: "2026-09-03", income: 8888, expense: 2000 }); // 转账不计
  });

  it("边界：桶判据的天数门槛是 62（含）", () => {
    // 2026-08-01 ~ 2026-10-01 = 62 天 → day 桶
    const day = statsSeries([], { from: "2026-08-01", to: "2026-10-01" });
    expect(day).toHaveLength(62);
    expect(day[0].key).toHaveLength(10);
    // 多一天（63 天）→ month 桶
    const month = statsSeries([], { from: "2026-08-01", to: "2026-10-02" });
    expect(month.map((point) => point.key)).toEqual(["2026-08", "2026-09", "2026-10"]);
  });

  it("不一致（现状）：core 的 ledger stats 用 31 天门槛切 grain，前端用 62 天", () => {
    // ops_ledger.rs::stats_range: span_days <= 31 → "day"，否则 "month"；
    // ledger.ts::bucketOf: days <= 62 → "day"。于是 32~62 天的区间两侧粒度不同
    // （CLI 给月桶、GUI 画日桶）。这里钉住前端现状。
    const series = statsSeries(dayEntries, { from: "2026-09-01", to: "2026-10-15" }); // 45 天
    expect(series[0].key).toHaveLength(10);
  });

  it("边界：month 桶下越界的账不进桶（半月起止只算区间内的那些天）", () => {
    const entries = [
      makeEntry({ amountCents: 100, date: "2026-08-01" }), // 早于 from（同月）
      makeEntry({ amountCents: 200, date: "2026-08-25" }),
      makeEntry({ amountCents: 300, date: "2026-11-20" }) // 晚于 to（同月）
    ];
    const series = statsSeries(entries, { from: "2026-08-20", to: "2026-11-05" }); // 78 天 → month
    expect(series.map((point) => point.key)).toEqual(["2026-08", "2026-09", "2026-10", "2026-11"]);
    expect(series[0].expense).toBe(200);
    expect(series[3].expense).toBe(0);
  });

  it("边界：from 晚于 to → 空序列", () => {
    expect(statsSeries(dayEntries, { from: "2026-09-10", to: "2026-09-01" })).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// categoryStats
// ---------------------------------------------------------------------------

describe("categoryStats（分类占比）", () => {
  /** core tests/ledger_money.rs::stats_category_totals_equal_entry_sums 的同一本账。 */
  function coreGoldenBook(): LedgerBook {
    const book = coreSeedBook();
    book.entries = [
      makeEntry({ id: "g1", amountCents: 1050, categoryId: "lcat-exp-01-02", date: "2026-09-01" }), // 午餐
      makeEntry({ id: "g2", amountCents: 2000, categoryId: "lcat-exp-01-03", date: "2026-09-02" }), // 晚餐
      makeEntry({ id: "g3", amountCents: 700, categoryId: "lcat-exp-01-05", date: "2026-09-03" }), // 饮料
      makeEntry({ id: "g4", amountCents: 3000, categoryId: "lcat-exp-02-02", date: "2026-09-03" }) // 打车
    ];
    return book;
  }

  it("core 黄金用例：子分类并进大类、percent 两位小数、整数分无 drift", () => {
    const book = coreGoldenBook();
    const stats = categoryStats(book, book.entries, "expense");
    expect(stats.map((item) => [item.categoryId, item.name, item.cents, item.count])).toEqual([
      ["lcat-exp-01", "餐饮", 3750, 3],
      ["lcat-exp-02", "交通", 3000, 1]
    ]);
    expect(stats[0].percent).toBe(55.56); // 与 core 的 55.56 完全一致
    expect(stats[1].percent).toBe(44.44);
    // 分组之和恰好 = 总支出（整数分，无浮点 drift）
    expect(stats.reduce((sum, item) => sum + item.cents, 0)).toBe(6750);
    expect(stats.reduce((sum, item) => sum + item.percent, 0)).toBeCloseTo(100, 6);
    expect(stats.every((item) => item.side === "expense")).toBe(true);
  });

  it("正常路径：children 是二级分类，按 cents 降序；顶层也按 cents 降序", () => {
    const golden = coreGoldenBook();
    const stats = categoryStats(golden, golden.entries, "expense");
    expect(stats[0].children.map((child) => [child.categoryId, child.name, child.cents, child.count])).toEqual([
      ["lcat-exp-01-03", "晚餐", 2000, 1],
      ["lcat-exp-01-02", "午餐", 1050, 1],
      ["lcat-exp-01-05", "饮料", 700, 1]
    ]);
    // 直接记在大类上的账并入大类但不产生 children 行
    const onParent = categoryStats(
      coreSeedBook(),
      [makeEntry({ amountCents: 100, categoryId: "lcat-exp-01" }), makeEntry({ amountCents: 50, categoryId: "lcat-exp-01-02" })],
      "expense"
    );
    expect(onParent[0].cents).toBe(150);
    expect(onParent[0].children).toEqual([
      { categoryId: "lcat-exp-01-02", name: "午餐", count: 1, cents: 50 }
    ]);
  });

  it("边界：percent = Math.round(cents/total*10000)/100（两位小数四舍五入）", () => {
    const book = coreSeedBook();
    const entries = [
      makeEntry({ id: "x", amountCents: 1000, categoryId: "lcat-exp-01-01" }),
      makeEntry({ id: "y", amountCents: 1000, categoryId: "lcat-exp-02-02" }),
      makeEntry({ id: "z", amountCents: 1000, categoryId: "lcat-exp-01-02" })
    ];
    const stats = categoryStats(book, entries, "expense");
    expect(stats.map((item) => [item.name, item.cents, item.percent])).toEqual([
      ["餐饮", 2000, 66.67], // 2000/3000 = 66.666…% → 进位
      ["交通", 1000, 33.33] // 33.333…% → 舍掉
    ]);
    expect(Math.abs(stats.reduce((sum, item) => sum + item.percent, 0) - 100)).toBeLessThan(0.02);
  });

  it("边界：三等分的合计是 99.99（接近 100，不是 100.00）", () => {
    const book = makeBook({
      categories: [
        makeCategory({ id: "p1", name: "一", order: 1 }),
        makeCategory({ id: "p2", name: "二", order: 2 }),
        makeCategory({ id: "p3", name: "三", order: 3 })
      ],
      entries: [
        makeEntry({ id: "x", amountCents: 1000, categoryId: "p1" }),
        makeEntry({ id: "y", amountCents: 1000, categoryId: "p2" }),
        makeEntry({ id: "z", amountCents: 1000, categoryId: "p3" })
      ]
    });
    const stats = categoryStats(book, book.entries, "expense");
    expect(stats.map((item) => item.percent)).toEqual([33.33, 33.33, 33.33]);
    expect(stats.reduce((sum, item) => sum + item.percent, 0)).toBeCloseTo(99.99, 6);
    // cents 相等时 sort 稳定 → 保持插入顺序
    expect(stats.map((item) => item.categoryId)).toEqual(["p1", "p2", "p3"]);
  });

  it("边界：未分类（categoryId 为空 / 指向不存在的分类）归到 categoryId === \"\" 的「未分类」", () => {
    const entries = [
      makeEntry({ id: "none", amountCents: 100, categoryId: undefined }),
      makeEntry({ id: "empty", amountCents: 200, categoryId: "" }),
      makeEntry({ id: "ghost", amountCents: 300, categoryId: "lcat-does-not-exist" }),
      makeEntry({ id: "real", amountCents: 400, categoryId: "lcat-exp-01-02" })
    ];
    const stats = categoryStats(coreSeedBook(), entries, "expense");
    expect(stats.map((item) => [item.categoryId, item.name, item.cents, item.count])).toEqual([
      ["", "未分类", 600, 3],
      ["lcat-exp-01", "餐饮", 400, 1]
    ]);
    expect(stats[0].children).toEqual([]);
  });

  it("边界：parentId 指向不存在的分类时按大类自己算（与 core 的 find_category 失败分支一致）", () => {
    const book = coreSeedBook();
    book.categories.push(makeCategory({ id: "orphan", name: "孤儿", parentId: "lcat-ghost", color: "" }));
    const stats = categoryStats(book, [makeEntry({ amountCents: 900, categoryId: "orphan" })], "expense");
    expect(stats.map((item) => [item.categoryId, item.name, item.cents])).toEqual([["orphan", "孤儿", 900]]);
  });

  it("正常路径：转账被排除，只统计传入的 side", () => {
    const entries = [
      makeEntry({ id: "exp", amountCents: 500, categoryId: "lcat-exp-01-02" }),
      makeEntry({ id: "inc", kind: "income", amountCents: 400, categoryId: "lcat-inc-01-01" }),
      makeEntry({ id: "tr", kind: "transfer", amountCents: 300, categoryId: "lcat-exp-01-02", toAccountId: "lacc-02" })
    ];
    const expense = categoryStats(coreSeedBook(), entries, "expense");
    expect(expense.map((item) => [item.name, item.cents, item.count])).toEqual([["餐饮", 500, 1]]);
    const income = categoryStats(coreSeedBook(), entries, "income");
    expect(income.map((item) => [item.name, item.cents, item.side])).toEqual([["工资", 400, "income"]]);
  });

  it("边界：没有匹配账目 → 空数组；总额为 0 时 percent 是 0（不除零、不出 NaN）", () => {
    expect(categoryStats(coreSeedBook(), [], "expense")).toEqual([]);
    expect(categoryStats(coreSeedBook(), [makeEntry({ kind: "transfer", toAccountId: "lacc-02" })], "expense")).toEqual([]);
    const zero = categoryStats(coreSeedBook(), [makeEntry({ amountCents: 0, categoryId: "lcat-exp-01-02" })], "expense");
    expect(zero).toHaveLength(1);
    expect(zero[0].cents).toBe(0);
    expect(zero[0].percent).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// accountBalances / accountBalance / assetsOverview
// ---------------------------------------------------------------------------

describe("accountBalances / accountBalance / assetsOverview（余额与资产）", () => {
  /** core tests/ledger_money.rs::balance_equals_initial_plus_flows 的同一本账。 */
  function coreGoldenBalanceBook(): LedgerBook {
    const book = coreSeedBook();
    book.accounts = book.accounts.map((account) =>
      account.id === "lacc-02" ? { ...account, initialCents: 8888 } : account
    );
    book.entries = [
      makeEntry({ id: "i", kind: "income", amountCents: 100000, accountId: "lacc-02", date: "2026-09-01", categoryId: "lcat-inc-01-01" }),
      makeEntry({ id: "x", amountCents: 25050, accountId: "lacc-02", date: "2026-09-02", categoryId: "lcat-exp-01-02" }),
      makeEntry({ id: "t", kind: "transfer", amountCents: 10000, accountId: "lacc-02", toAccountId: "lacc-01", date: "2026-09-03" })
    ];
    return book;
  }

  it("core 黄金用例：余额 = 期初 + 收入 − 支出 − 转账转出 + 转账转入", () => {
    const book = coreGoldenBalanceBook();
    expect(accountBalance(book, "lacc-02")).toBe(8888 + 100000 - 25050 - 10000); // 73838
    expect(accountBalance(book, "lacc-01")).toBe(10000);
    expect(accountBalance(book, "lacc-03")).toBe(0);
    const balances = accountBalances(book);
    expect(balances.get("lacc-02")).toBe(73838);
    expect(assetsOverview(book).net).toBe(73838 + 10000);
  });

  it("core 黄金用例：期初 500 − 支出 30.50，非 credit 账户的负余额不算负债", () => {
    const book = coreSeedBook();
    book.accounts = book.accounts.map((account) =>
      account.id === "lacc-01" ? { ...account, initialCents: 50000 } : account
    );
    book.entries = [makeEntry({ amountCents: 3050, accountId: "lacc-01", date: "2026-09-08" })];
    expect(accountBalance(book, "lacc-01")).toBe(46950);
    const assets = assetsOverview(book);
    expect(assets.net).toBe(46950);
    expect(assets.liabilities).toBe(0);
    expect(assets.assets).toBe(46950);

    // 现金账户透支成负数也不进负债（只有 kind === "credit" 才算）
    book.entries.push(makeEntry({ id: "over", amountCents: 100000, accountId: "lacc-01", date: "2026-09-09" }));
    const overdrawn = assetsOverview(book);
    expect(accountBalance(book, "lacc-01")).toBe(-53050);
    expect(overdrawn.liabilities).toBe(0);
    expect(overdrawn.net).toBe(-53050);
    expect(overdrawn.assets).toBe(-53050);
  });

  it("core 黄金用例：credit 负余额计入负债，净资产 = 各账户之和，总资产 = 净资产 + 负债", () => {
    const book = makeBook({
      accounts: [makeAccount({ id: "credit", name: "信用卡", kind: "credit" })],
      entries: [makeEntry({ amountCents: 50000, accountId: "credit", date: "2026-09-08" })]
    });
    const assets = assetsOverview(book);
    expect(accountBalance(book, "credit")).toBe(-50000);
    expect(assets.liabilities).toBe(50000);
    expect(assets.net).toBe(-50000);
    expect(assets.assets).toBe(0); // 总资产 = 净资产 + 负债

    // 从别的账户还款把 credit 拉回 0 → 不再是负债；净资产不变（钱只是换了口袋）
    book.accounts.push(makeAccount({ id: "cash", name: "现金", initialCents: 50000, order: 2 }));
    book.entries.push(makeEntry({ id: "repay", kind: "transfer", amountCents: 50000, accountId: "cash", toAccountId: "credit", date: "2026-09-09" }));
    const repaid = assetsOverview(book);
    expect(accountBalance(book, "credit")).toBe(0);
    expect(accountBalance(book, "cash")).toBe(0);
    expect(repaid.liabilities).toBe(0);
    expect(repaid.net).toBe(0);
    expect(repaid.assets).toBe(0);
  });

  it("正常路径：perAccount 按 account.order 升序", () => {
    const book = makeBook({
      accounts: [
        makeAccount({ id: "c", name: "丙", order: 3 }),
        makeAccount({ id: "a", name: "甲", order: 1 }),
        makeAccount({ id: "b", name: "乙", order: 2 })
      ]
    });
    expect(assetsOverview(book).perAccount.map((item) => item.account.id)).toEqual(["a", "b", "c"]);
    // 不改动入参顺序
    expect(book.accounts.map((account) => account.id)).toEqual(["c", "a", "b"]);
  });

  it("不变式：accountBalance 与 accountBalances 给出同一份结果（两个函数只有一套算法）", () => {
    const book = coreGoldenBalanceBook();
    book.accounts.push(makeAccount({ id: "credit", name: "信用卡", kind: "credit", initialCents: -1234, order: 9 }));
    const balances = accountBalances(book);
    for (const account of book.accounts) {
      expect(accountBalance(book, account.id)).toBe(balances.get(account.id));
    }
    // assetsOverview 的每个账户余额也来自同一份
    const assets = assetsOverview(book);
    for (const item of assets.perAccount) {
      expect(item.balance).toBe(balances.get(item.account.id));
    }
  });

  it("边界：转账没有转入账户时只扣转出方；未知账户不炸", () => {
    const book = coreSeedBook();
    book.entries = [
      makeEntry({ id: "t1", kind: "transfer", amountCents: 5000, accountId: "lacc-01" }), // toAccountId 缺省
      makeEntry({ id: "ghost", amountCents: 700, accountId: "lacc-does-not-exist" })
    ];
    expect(accountBalance(book, "lacc-01")).toBe(-5000);
    expect(accountBalances(book).get("lacc-does-not-exist")).toBe(-700); // 索引里有，但不在 perAccount 里
    expect(accountBalance(book, "lacc-does-not-exist")).toBe(-700);
    expect(assetsOverview(book).perAccount.map((item) => item.account.id)).toEqual([
      "lacc-01",
      "lacc-02",
      "lacc-03",
      "lacc-04"
    ]);
    expect(accountBalance(makeBook(), "nope")).toBe(0);
  });

  it("边界：空账本", () => {
    const assets = assetsOverview(makeBook());
    expect(assets).toEqual({ net: 0, assets: 0, liabilities: 0, perAccount: [] });
    expect(accountBalances(makeBook()).size).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// assetsTrend
// ---------------------------------------------------------------------------

describe("assetsTrend（总资产趋势）", () => {
  function trendBook(spanDays: number, maxPoints?: number) {
    const today = todayDate();
    const from = shiftDays(today, -spanDays);
    return {
      book: makeBook({
        accounts: [
          makeAccount({ id: "a1", initialCents: 100000, createdAt: `${from}T00:00:00` }),
          makeAccount({ id: "a2", initialCents: 20000, order: 2, createdAt: `${from}T09:00:00` })
        ],
        entries: [
          makeEntry({ id: "t-first", amountCents: 1000, accountId: "a1", date: from }),
          makeEntry({ id: "t-mid", kind: "income", amountCents: 5000, accountId: "a2", date: shiftDays(today, -Math.floor(spanDays / 2)) }),
          makeEntry({ id: "t-last", amountCents: 250, accountId: "a1", date: today })
        ]
      }),
      today,
      from,
      maxPoints
    };
  }

  it("正常路径：末点是今天，口径与 assetsOverview 一致", () => {
    const { book, today } = trendBook(40);
    const points = assetsTrend(book);
    expect(points[points.length - 1].date).toBe(today);
    expect(points[points.length - 1].cents).toBe(assetsOverview(book).assets);
    expect(points[points.length - 1].cents).toBe(100000 + 20000 - 1000 + 5000 - 250);
  });

  it("正常路径：起点前一格是「只有期初」的水平段", () => {
    const { book, from } = trendBook(40);
    const points = assetsTrend(book); // span 41 天 ≤ 90 → step 1
    expect(points[0].date).toBe(shiftDays(from, -1));
    expect(points[0].cents).toBe(120000); // 只有期初，还没算任何流水
    expect(points[1].date).toBe(from);
    expect(points[1].cents).toBe(119000); // 当天那笔支出已入账
  });

  it("边界：日期严格递增", () => {
    const { book } = trendBook(400);
    const points = assetsTrend(book);
    points.forEach((point, index) => {
      if (index === 0) return;
      expect(point.date > points[index - 1].date).toBe(true);
    });
    expect(points[points.length - 1].date).toBe(todayDate());
  });

  it("边界：点数不超过 maxPoints + 2（含起点前一格与今天）", () => {
    for (const [span, maxPoints] of [
      [40, 90],
      [400, 90],
      [40, 5],
      [400, 5],
      [1, 90],
      [0, 90]
    ] as [number, number][]) {
      const { book } = trendBook(span);
      const points = assetsTrend(book, maxPoints);
      expect(points.length, `span=${span} maxPoints=${maxPoints}`).toBeLessThanOrEqual(maxPoints + 2);
      expect(points.length).toBeGreaterThanOrEqual(2);
    }
  });

  it("边界：没有账户 → 空数组；只有期初没有流水 → 一条水平线", () => {
    expect(assetsTrend(makeBook())).toEqual([]);
    const today = todayDate();
    const flat = makeBook({
      accounts: [makeAccount({ id: "a1", initialCents: 4242, createdAt: `${shiftDays(today, -3)}T00:00:00` })],
      entries: []
    });
    const points = assetsTrend(flat);
    expect(points.map((point) => point.cents)).toEqual([4242, 4242, 4242, 4242, 4242]);
    expect(points[points.length - 1].date).toBe(today);
    expect(points[0].date).toBe(shiftDays(today, -4));
  });

  it("边界：账户 createdAt 不是合法日期时忽略，起点退回首笔流水", () => {
    const today = todayDate();
    const book = makeBook({
      accounts: [makeAccount({ id: "a1", initialCents: 100, createdAt: "不是日期" })],
      entries: [makeEntry({ id: "e", amountCents: 1, accountId: "a1", date: shiftDays(today, -2) })]
    });
    const points = assetsTrend(book);
    expect(points[0].date).toBe(shiftDays(shiftDays(today, -2), -1));
    expect(points[points.length - 1].date).toBe(today);
  });

  it("回归（v0.8.0）：有未来日期的账时，趋势末点与 assetsOverview 一致", () => {
    // 终点是 max(今天, 最晚一笔)：早先只扫到今天，末点漏掉未来那笔，
    // 曲线右端与旁边卡片里的数字对不上（预付下月房租这类日期是真实场景）。
    const today = todayDate();
    const future = shiftDays(today, 30);
    const book = makeBook({
      accounts: [makeAccount({ id: "a1", initialCents: 100000, createdAt: `${shiftDays(today, -5)}T00:00:00` })],
      entries: [makeEntry({ id: "future", amountCents: 7777, accountId: "a1", date: future })]
    });
    const points = assetsTrend(book);
    expect(points[points.length - 1].date).toBe(future);
    expect(points[points.length - 1].cents).toBe(100000 - 7777);
    expect(points[points.length - 1].cents).toBe(assetsOverview(book).assets);
    // 未来那天之前的点还没卷进这一笔
    const beforeFuture = points.filter((point) => point.date < future);
    expect(beforeFuture.every((point) => point.cents === 100000)).toBe(true);
  });

  it("边界：没有未来的账时终点仍是今天（不会因为这条改动而变）", () => {
    const today = todayDate();
    const book = makeBook({
      accounts: [makeAccount({ id: "a1", initialCents: 100, createdAt: `${shiftDays(today, -5)}T00:00:00` })],
      entries: [makeEntry({ id: "e", amountCents: 1, accountId: "a1", date: shiftDays(today, -2) })]
    });
    expect(assetsTrend(book).at(-1)?.date).toBe(today);
  });
});

// ---------------------------------------------------------------------------
// filterLedgerEntries / entriesTotals
// ---------------------------------------------------------------------------

describe("filterLedgerEntries（记账搜索）", () => {
  function searchBook(): LedgerBook {
    const book = coreSeedBook();
    book.entries = [
      makeEntry({ id: "f-lunch", amountCents: 1050, categoryId: "lcat-exp-01-02", date: "2026-09-01", time: "12:00:00" }),
      makeEntry({ id: "f-taxi", amountCents: 3000, categoryId: "lcat-exp-02-02", date: "2026-09-03", time: "18:00:00", note: "机场打车" }),
      makeEntry({ id: "f-note", amountCents: 1234, note: "Team Taxi 报销", date: "2026-09-05", time: "09:00:00" }),
      makeEntry({ id: "f-big", amountCents: 123456, categoryId: "lcat-exp-01-03", date: "2026-09-06", time: "20:00:00" }),
      makeEntry({ id: "f-salary", kind: "income", amountCents: 888800, categoryId: "lcat-inc-01-01", date: "2026-09-07" }),
      makeEntry({ id: "f-transfer", kind: "transfer", amountCents: 5000, date: "2026-09-08", toAccountId: "lacc-02" })
    ];
    return book;
  }

  it("正常路径：分类名（二级）与大类名都命中", () => {
    const book = searchBook();
    expect(filterLedgerEntries(book, "午餐").map((entry) => entry.id)).toEqual(["f-lunch"]);
    // 命中二级分类时把大类名也算上：搜「餐饮」拿到餐饮下的两笔
    expect(filterLedgerEntries(book, "餐饮").map((entry) => entry.id)).toEqual(["f-big", "f-lunch"]);
    expect(filterLedgerEntries(book, "工资薪金").map((entry) => entry.id)).toEqual(["f-salary"]);
    expect(filterLedgerEntries(book, "工资").map((entry) => entry.id)).toEqual(["f-salary"]);
  });

  it("正常路径：备注命中且大小写不敏感；分类名与备注同时命中只出一次", () => {
    const book = searchBook();
    expect(filterLedgerEntries(book, "报销").map((entry) => entry.id)).toEqual(["f-note"]);
    expect(filterLedgerEntries(book, "机场").map((entry) => entry.id)).toEqual(["f-taxi"]);
    expect(filterLedgerEntries(book, "taxi").map((entry) => entry.id)).toEqual(["f-note"]); // 备注里的英文
    expect(filterLedgerEntries(book, "TAXI").map((entry) => entry.id)).toEqual(["f-note"]);
    // 「打车」既命中二级分类名也命中备注，但一笔账只出现一次
    expect(filterLedgerEntries(book, "打车").map((entry) => entry.id)).toEqual(["f-taxi"]);
  });

  it("正常路径：转账按「转账」命中", () => {
    expect(filterLedgerEntries(searchBook(), "转账").map((entry) => entry.id)).toEqual(["f-transfer"]);
  });

  it("正常路径：金额的各种形态（12.34 / 1,234.56 / 1234 / 取整）", () => {
    const book = searchBook();
    expect(filterLedgerEntries(book, "12.34").map((entry) => entry.id)).toEqual(["f-note"]); // 1234 分
    expect(filterLedgerEntries(book, "1234.56").map((entry) => entry.id)).toEqual(["f-big"]);
    expect(filterLedgerEntries(book, "1,234.56").map((entry) => entry.id)).toEqual(["f-big"]); // 逗号去掉
    expect(filterLedgerEntries(book, "1 234.56").map((entry) => entry.id)).toEqual(["f-big"]); // 空格去掉
    expect(filterLedgerEntries(book, "1234").map((entry) => entry.id)).toEqual(["f-big"]); // 整数部分形态
    expect(filterLedgerEntries(book, "8888").map((entry) => entry.id)).toEqual(["f-salary"]); // 取整形态
  });

  it("边界：金额形态不去掉小数点 —— 搜「1234」拿不到 12.34（形态是逐字符包含，不是数值比较）", () => {
    const book = searchBook();
    // 四种形态是 toFixed(2) / formatCents / String(amount) / Math.round(amount)，都保留小数点
    expect(filterLedgerEntries(book, "1234").some((entry) => entry.id === "f-note")).toBe(false);
  });

  it("回归（v0.8.0）：照着屏幕上看到的带符号金额也能搜到", () => {
    const book = searchBook();
    // amountCents 恒为正（方向由 kind 决定），界面上支出画 `-12.34`、收入 `+8888.00`；
    // 早先只比无符号形态，用户照着屏幕敲进去一无所获。
    expect(filterLedgerEntries(book, "-12.34").map((entry) => entry.id)).toEqual(["f-note"]);
    expect(filterLedgerEntries(book, "-1234.56").map((entry) => entry.id)).toEqual(["f-big"]);
    expect(filterLedgerEntries(book, "-1,234.56").map((entry) => entry.id)).toEqual(["f-big"]);
    expect(filterLedgerEntries(book, "+8888").map((entry) => entry.id)).toEqual(["f-salary"]);
    // 转账界面上不带符号，所以也不该被带符号的查询命中
    expect(filterLedgerEntries(book, "-50")).toEqual([]);
    expect(filterLedgerEntries(book, "50.00").map((entry) => entry.id)).toEqual(["f-transfer"]);
  });

  it("回归（v0.8.0）：只由逗号/空格组成的查询不再命中全部条目", () => {
    const book = searchBook();
    // 早先 needle 非空但 amountNeedle 被清成空串，String.includes("") 恒真 → 整本账都列出来
    expect(filterLedgerEntries(book, ",")).toEqual([]);
    expect(filterLedgerEntries(book, "，")).toEqual([]); // 全角逗号不在剥离名单里，本来也搜不到
  });

  it("边界：空查询返回全量且已按时间倒序；结果一律倒序", () => {
    const book = searchBook();
    const all = filterLedgerEntries(book, "");
    expect(all).toHaveLength(book.entries.length);
    expect(all.map((entry) => entry.id)).toEqual(sortEntries(book.entries).map((entry) => entry.id));
    expect(filterLedgerEntries(book, "   ")).toHaveLength(book.entries.length);
    const byCategory = filterLedgerEntries(book, "餐饮");
    expect(byCategory.map((entry) => entry.date)).toEqual(["2026-09-06", "2026-09-01"]);
    // 不改动入参
    expect(book.entries.map((entry) => entry.id)[0]).toBe("f-lunch");
  });

  it("边界：没有命中 → 空数组；账本为空 → 空数组", () => {
    expect(filterLedgerEntries(searchBook(), "不存在的词")).toEqual([]);
    expect(filterLedgerEntries(makeBook(), "午餐")).toEqual([]);
    expect(filterLedgerEntries(makeBook(), "")).toEqual([]);
  });
});

describe("entriesTotals（一组流水的收/支合计）", () => {
  it("正常路径：收入与支出分列，转账不计入", () => {
    const totals = entriesTotals([
      makeEntry({ kind: "income", amountCents: 1000 }),
      makeEntry({ kind: "income", amountCents: 250 }),
      makeEntry({ kind: "expense", amountCents: 300 }),
      makeEntry({ kind: "transfer", amountCents: 99999, toAccountId: "lacc-02" })
    ]);
    expect(totals).toEqual({ income: 1250, expense: 300 });
  });

  it("边界：空数组 → 双零", () => {
    expect(entriesTotals([])).toEqual({ income: 0, expense: 0 });
  });
});

// ---------------------------------------------------------------------------
// ledgerLookup / categoryColor
// ---------------------------------------------------------------------------

describe("ledgerLookup（WeakMap 索引）", () => {
  it("正常路径：同一个 book 对象两次调用返回同一个索引对象", () => {
    const book = coreSeedBook();
    const first = ledgerLookup(book);
    expect(ledgerLookup(book)).toBe(first);
    expect(ledgerLookup(book)).toBe(first);
  });

  it("正常路径：换一个 book 对象（哪怕内容一样）就重建", () => {
    const book = coreSeedBook();
    const clone = coreSeedBook();
    expect(ledgerLookup(book)).not.toBe(ledgerLookup(clone));
    expect(ledgerLookup({ ...book })).not.toBe(ledgerLookup(book));
  });

  it("正常路径：分类 / 账户 / 自定义账户类型三张表都能查到", () => {
    const type: LedgerAccountType = { id: "latype-01", name: "饭卡", icon: "CreditCard", color: "#123456" };
    const book = coreSeedBook();
    book.accountTypes = [type];
    const lookup = ledgerLookup(book);
    expect(lookup.categoryById.get("lcat-exp-01-02")?.name).toBe("午餐");
    expect(lookup.accountById.get("lacc-02")?.name).toBe("微信");
    expect(lookup.accountTypeByName.get("饭卡")).toBe(type);
    expect(lookup.categoryById.size).toBe(book.categories.length);
    expect(lookup.accountById.size).toBe(book.accounts.length);
    expect(lookup.accountTypeByName.size).toBe(1);
    expect(lookup.categoryById.get("nope")).toBeUndefined();
  });

  it("边界：accountTypes 字段缺失（老快照）不炸", () => {
    const legacy = { accounts: [], categories: [], entries: [] } as unknown as LedgerBook;
    expect(ledgerLookup(legacy).accountTypeByName.size).toBe(0);
  });
});

describe("categoryColor（分类颜色）", () => {
  it("正常路径：自己有颜色用自己的；没有则继承大类", () => {
    const book = coreSeedBook();
    const lookup = ledgerLookup(book);
    expect(categoryColor(book, lookup.categoryById.get("lcat-exp-02"))).toBe("#4a90d9"); // 交通自己的颜色
    expect(categoryColor(book, lookup.categoryById.get("lcat-exp-02-02"))).toBe("#4a90d9"); // 打车继承交通
    expect(categoryColor(book, lookup.categoryById.get("lcat-inc-01"))).toBe("#27ae60");
  });

  it("边界：大类也没颜色 → 按 side 给默认色（income #27ae60 / expense #f0862c）", () => {
    const book = coreSeedBook();
    book.categories = book.categories.map((category) => ({ ...category, color: "" }));
    expect(categoryColor(book, book.categories.find((category) => category.id === "lcat-exp-01"))).toBe("#f0862c");
    expect(categoryColor(book, book.categories.find((category) => category.id === "lcat-inc-01"))).toBe("#27ae60");
  });

  it("边界：category 为 undefined → #95a5a6；parentId 查不到 → side 默认色", () => {
    const book = coreSeedBook();
    expect(categoryColor(book, undefined)).toBe("#95a5a6");
    expect(categoryColor(book, makeCategory({ id: "ghost-child", parentId: "lcat-ghost", color: "" }))).toBe("#f0862c");
    expect(categoryColor(book, makeCategory({ id: "ghost-income", side: "income", color: "" }))).toBe("#27ae60");
  });
});

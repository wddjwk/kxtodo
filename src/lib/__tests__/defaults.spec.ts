/**
 * defaults.ts 的归一化（normalize*）单元测试 —— 挑与资金/时刻/外观相关的重点。
 *
 * 记账那一块以 core 为权威：`crates/core/src/model.rs::{LedgerFile::seed_defaults,
 * LedgerEntry::fold_legacy_image}` 与 `ops_ledger.rs` 的写入门。
 * `normalizeLedgerEntry` / `normalizeLedgerAccount` 这些是模块私有函数，一律经
 * 公开的 `normalizeLedger` 验证。测试**只描述现状**，可疑之处写在注释与报告里。
 */
import { describe, expect, it } from "vitest";

import type { LedgerAccount, LedgerBook, LedgerCategory } from "../types";
import { defaultSettings, normalizeLedger, normalizeSettings, seedLedgerBook } from "../defaults";

// ---------------------------------------------------------------------------
// normalizeLedger → normalizeLedgerEntry
// ---------------------------------------------------------------------------

describe("normalizeLedger（条目）", () => {
  const entry = (raw: Record<string, unknown>) => normalizeLedger({ entries: [raw] }).entries[0];

  it("正常路径：v0.7.4 的单数 image 折叠进 images[]", () => {
    const folded = entry({ id: "n1", amountCents: 1200, accountId: "a1", date: "2026-09-08", image: "md-legacy.png" });
    expect(folded.images).toEqual(["md-legacy.png"]);
    // 多图字段优先，空串被剔掉
    const multi = entry({ id: "n2", amountCents: 500, accountId: "a1", date: "2026-09-08", images: ["b.png", "", "c.png"] });
    expect(multi.images).toEqual(["b.png", "c.png"]);
    // 没有图 → 字段是 undefined（不落一个空数组）
    expect(entry({ id: "n3", amountCents: 1, accountId: "a1", date: "2026-09-08" }).images).toBeUndefined();
    expect(entry({ id: "n4", amountCents: 1, accountId: "a1", date: "2026-09-08", image: "" }).images).toBeUndefined();
    expect(entry({ id: "n5", amountCents: 1, accountId: "a1", date: "2026-09-08", images: [] }).images).toBeUndefined();
  });

  it("边界（与 core 同口径）：images 非空时旧 image 被丢弃，不合并", () => {
    // core 的 fold_legacy_image 同样是「images 非空就把 legacy 置空」。
    const both = entry({ id: "n", amountCents: 1, accountId: "a1", date: "2026-09-08", images: ["new.png"], image: "old.png" });
    expect(both.images).toEqual(["new.png"]);
  });

  it("回归（v0.8.0）：images 是空数组 + 旧 image 有值 → 折进 legacy（与 core 一致）", () => {
    // 早先前端拿 `Array.isArray([])` 当分支条件 → 走 images 分支 → 过滤后仍是空 → 丢图，
    // 而 core 的判据是 `images.is_empty()` → ["old.png"]。同一份 JSON 两侧结论相反。
    const mixed = entry({ id: "n", amountCents: 1, accountId: "a1", date: "2026-09-08", images: [], image: "old.png" });
    expect(mixed.images).toEqual(["old.png"]);
    // legacy 名字两侧都 trim；trim 完是空串就不算有图
    expect(entry({ id: "n", amountCents: 1, accountId: "a1", date: "2026-09-08", image: "  old.png  " }).images).toEqual(["old.png"]);
    expect(entry({ id: "n", amountCents: 1, accountId: "a1", date: "2026-09-08", images: [], image: "   " }).images).toBeUndefined();
  });

  it("正常路径：amountCents <= 0 / accountId 为空 / id 缺失的条目被丢弃", () => {
    expect(entry({ id: "zero", amountCents: 0, accountId: "a1", date: "2026-09-08" })).toBeUndefined();
    expect(entry({ id: "neg", amountCents: -50, accountId: "a1", date: "2026-09-08" })).toBeUndefined();
    expect(entry({ id: "no-account", amountCents: 50, date: "2026-09-08" })).toBeUndefined();
    expect(entry({ id: "", amountCents: 50, accountId: "a1", date: "2026-09-08" })).toBeUndefined();
    expect(entry({ amountCents: 50, accountId: "a1", date: "2026-09-08" })).toBeUndefined();
    expect(normalizeLedger({ entries: [null, 42, "x", []] }).entries).toEqual([]);
    // 正数照常留下
    expect(entry({ id: "ok", amountCents: 1, accountId: "a1", date: "2026-09-08" })?.amountCents).toBe(1);
  });

  it("正常路径：amountCents 容忍数字字符串与小数（四舍五入到整分）", () => {
    expect(entry({ id: "s", amountCents: "1200", accountId: "a1", date: "2026-09-08" })?.amountCents).toBe(1200);
    expect(entry({ id: "f", amountCents: 1200.6, accountId: "a1", date: "2026-09-08" })?.amountCents).toBe(1201);
    expect(entry({ id: "bad", amountCents: "abc", accountId: "a1", date: "2026-09-08" })).toBeUndefined(); // toCents → 0 → 丢弃
  });

  it("正常路径：未知 kind 回落 expense，income/transfer 原样保留", () => {
    expect(entry({ id: "k1", kind: "weird", amountCents: 1, accountId: "a1", date: "2026-09-08" })?.kind).toBe("expense");
    expect(entry({ id: "k2", amountCents: 1, accountId: "a1", date: "2026-09-08" })?.kind).toBe("expense");
    expect(entry({ id: "k3", kind: "income", amountCents: 1, accountId: "a1", date: "2026-09-08" })?.kind).toBe("income");
    expect(entry({ id: "k4", kind: "transfer", amountCents: 1, accountId: "a1", date: "2026-09-08" })?.kind).toBe("transfer");
  });

  it("正常路径：toAccountId 只对转账保留", () => {
    expect(entry({ id: "t", kind: "transfer", amountCents: 1, accountId: "a1", toAccountId: "a2", date: "2026-09-08" })?.toAccountId).toBe("a2");
    expect(entry({ id: "x", kind: "expense", amountCents: 1, accountId: "a1", toAccountId: "a2", date: "2026-09-08" })?.toAccountId).toBeUndefined();
    expect(entry({ id: "y", kind: "transfer", amountCents: 1, accountId: "a1", toAccountId: "", date: "2026-09-08" })?.toAccountId).toBeUndefined();
  });

  it("边界：date 非法时退回 createdAt 的本地日历日；两者都没有 → 空串（现状）", () => {
    // 无时区的时间戳按本地解析，任何时区下都是同一天
    expect(entry({ id: "d1", amountCents: 1, accountId: "a1", date: "bad-date", createdAt: "2026-09-07T18:19:00" })?.date).toBe("2026-09-07");
    expect(entry({ id: "d3", amountCents: 1, accountId: "a1", createdAt: "2026-09-07T18:19:00" })?.date).toBe("2026-09-07");
    // date 与 createdAt 都缺 → date 是空串：这样的条目会掉出所有按月/日历/统计分组
    // （那些都按 date 前缀或闭区间过滤），但余额推导照样算它 —— 见报告。
    const dateless = entry({ id: "d4", amountCents: 1, accountId: "a1" });
    expect(dateless?.date).toBe("");
    expect(typeof dateless?.createdAt).toBe("string");
    expect((dateless?.createdAt ?? "").length).toBeGreaterThan(0); // 补了一个 now()
  });

  it("不一致（现状）：date 只做形状校验，2026-13-45 原样留下（core 的 parse_date 会拒）", () => {
    // /^\d{4}-\d{2}-\d{2}$/ 只看形状，月份 13、日 45 都过；core 侧 ledger add / Excel 导入
    // 都走 chrono 的 parse_date，语义非法直接拒（LEDGER_*_INVALID / 跳过并计入 skipped）。
    // 这种日期永远匹配不上任何 MonthCursor 前缀 → 界面上看不到，却照样进余额与 total 区间的首末。
    expect(entry({ id: "d2", amountCents: 1, accountId: "a1", date: "2026-13-45", createdAt: "2026-09-07T18:19:00" })?.date).toBe("2026-13-45");
    expect(entry({ id: "d5", amountCents: 1, accountId: "a1", date: "2026-02-30" })?.date).toBe("2026-02-30");
  });

  it("边界：缺字段补默认值（time/note/images 空，updatedAt 不落）", () => {
    const minimal = entry({ id: "m", amountCents: 1, accountId: "a1", date: "2026-09-08" });
    expect(minimal).toMatchObject({ id: "m", kind: "expense", amountCents: 1, accountId: "a1", date: "2026-09-08", time: "", note: "" });
    expect(minimal?.updatedAt).toBeUndefined();
    expect(minimal?.categoryId).toBeUndefined();
    expect(entry({ id: "c", amountCents: 1, accountId: "a1", date: "2026-09-08", categoryId: "" })?.categoryId).toBeUndefined();
    expect(entry({ id: "c2", amountCents: 1, accountId: "a1", date: "2026-09-08", categoryId: "lcat-exp-01" })?.categoryId).toBe("lcat-exp-01");
  });
});

describe("normalizeLedger（账户 / 分类 / 账户类型 / 顶层形状）", () => {
  it("边界：raw 不是对象、字段不是数组 → 四个空数组", () => {
    expect(normalizeLedger(undefined)).toEqual({ accounts: [], categories: [], entries: [], accountTypes: [] });
    expect(normalizeLedger(null)).toEqual({ accounts: [], categories: [], entries: [], accountTypes: [] });
    expect(normalizeLedger("账本")).toEqual({ accounts: [], categories: [], entries: [], accountTypes: [] });
    expect(normalizeLedger({ accounts: "x", categories: 3, entries: null })).toEqual({
      accounts: [],
      categories: [],
      entries: [],
      accountTypes: []
    });
  });

  it("正常路径：账户名字 trim、kind 只兜底缺省、initialCents 容忍字符串", () => {
    const { accounts } = normalizeLedger({
      accounts: [
        { id: "a1", name: " 现金 ", kind: "", initialCents: "1200", order: 3 },
        { id: "a2", name: "饭卡", kind: "饭卡" }, // v0.7.4 起类型是自由字符串
        { id: "", name: "无 id" },
        { id: "a4", name: "   " },
        { id: "a5" }
      ]
    });
    expect(accounts.map((account) => [account.id, account.name, account.kind, account.initialCents, account.order])).toEqual([
      ["a1", "现金", "cash", 1200, 3],
      ["a2", "饭卡", "饭卡", 0, 0]
    ]);
    expect(accounts[0].icon).toBe("");
    expect(accounts[0].color).toBe("");
    expect(accounts[0].note).toBe("");
    expect(typeof accounts[0].createdAt).toBe("string");
  });

  it("正常路径：分类 side 非法回落 expense、空 parentId 变 undefined", () => {
    const { categories } = normalizeLedger({
      categories: [
        { id: "c1", name: " 餐饮 ", side: "weird", parentId: "" },
        { id: "c2", name: "工资", side: "income", parentId: "c1", color: "#27ae60" },
        { id: "c3", name: "" },
        { name: "无 id" }
      ]
    });
    expect(categories.map((category) => [category.id, category.name, category.side, category.parentId ?? null])).toEqual([
      ["c1", "餐饮", "expense", null],
      ["c2", "工资", "income", "c1"]
    ]);
    expect(categories[1].color).toBe("#27ae60");
  });

  it("正常路径：自定义账户类型（名字唯一、trim、空名字丢弃）", () => {
    const { accountTypes } = normalizeLedger({
      accountTypes: [
        { id: "latype-01", name: " 饭卡 ", icon: "CreditCard", color: "#123456" },
        { id: "latype-02", name: "" },
        { id: "", name: "无 id" }
      ]
    });
    expect(accountTypes).toEqual([{ id: "latype-01", name: "饭卡", icon: "CreditCard", color: "#123456", createdAt: undefined, updatedAt: undefined }]);
  });

  it("正常路径：归一后的账本可以直接喂给 ledger.ts（与 seedLedgerBook 同形状）", () => {
    const seed = seedLedgerBook();
    const round = normalizeLedger(JSON.parse(JSON.stringify(seed)));
    expect(round.accounts).toHaveLength(seed.accounts.length);
    expect(round.categories).toHaveLength(seed.categories.length);
    expect(round.entries).toEqual([]);
    expect(round.accountTypes).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// seedLedgerBook —— 种子 id 必须确定性（与 core LedgerFile::seed_defaults 同一套）
// ---------------------------------------------------------------------------

describe("seedLedgerBook（浏览器预览首跑的种子账本）", () => {
  it("正常路径：账户 id / 名字 / 类型 / 图标 / 颜色 / order 与 core seed_defaults 完全一致", () => {
    const { accounts } = seedLedgerBook();
    expect(accounts.map((account) => [account.id, account.name, account.kind, account.icon, account.color, account.order])).toEqual([
      ["lacc-01", "现金", "cash", "Wallet", "#e8a33d", 1],
      ["lacc-02", "微信", "other", "MessageCircle", "#2aae67", 2],
      ["lacc-03", "支付宝", "other", "Smartphone", "#1677ff", 3],
      ["lacc-04", "储蓄卡", "debit", "Landmark", "#b23a48", 4]
    ]);
    expect(accounts.every((account) => account.initialCents === 0 && account.note === "")).toBe(true);
  });

  it("正常路径：两级分类的 id 是确定性的（lcat-exp-01 / lcat-exp-01-01 这套）", () => {
    const { categories } = seedLedgerBook();
    const parents = categories.filter((category) => !category.parentId);
    expect(parents.filter((category) => category.side === "expense").map((category) => category.id)).toEqual([
      "lcat-exp-01",
      "lcat-exp-02",
      "lcat-exp-03",
      "lcat-exp-04",
      "lcat-exp-05",
      "lcat-exp-06",
      "lcat-exp-07",
      "lcat-exp-08",
      "lcat-exp-09",
      "lcat-exp-10"
    ]);
    expect(parents.filter((category) => category.side === "income").map((category) => category.id)).toEqual([
      "lcat-inc-01",
      "lcat-inc-02",
      "lcat-inc-03",
      "lcat-inc-04",
      "lcat-inc-05",
      "lcat-inc-06"
    ]);
    // core 黄金 id：餐饮 与它的第一个/第二个子分类
    expect(categories.find((category) => category.id === "lcat-exp-01")?.name).toBe("餐饮");
    expect(categories.find((category) => category.id === "lcat-exp-01-01")?.name).toBe("早餐");
    expect(categories.find((category) => category.id === "lcat-exp-01-02")?.name).toBe("午餐");
    expect(categories.find((category) => category.id === "lcat-inc-01-01")?.name).toBe("工资薪金");
    // ledger_money.rs 的分类占比用例就建在这三个 id 上
    expect(categories.find((category) => category.name === "晚餐")?.id).toBe("lcat-exp-01-03");
    expect(categories.find((category) => category.name === "饮料")?.id).toBe("lcat-exp-01-05");
    expect(categories.find((category) => category.name === "打车")?.id).toBe("lcat-exp-02-02");
  });

  it("正常路径：子分类不存颜色（继承大类）、parentId 指向真实存在的大类", () => {
    const { categories } = seedLedgerBook();
    const ids = new Set(categories.map((category) => category.id));
    for (const category of categories) {
      if (category.parentId === undefined) {
        expect(category.color, `大类 ${category.id} 应该有颜色`).toMatch(/^#[0-9a-f]{6}$/);
        expect(category.side === "expense" ? category.id.startsWith("lcat-exp-") : category.id.startsWith("lcat-inc-")).toBe(true);
      } else {
        expect(ids.has(category.parentId), `子分类 ${category.id} 的 parentId 悬空`).toBe(true);
        expect(category.color).toBe("");
        expect(categories.find((parent) => parent.id === category.parentId)?.side).toBe(category.side);
      }
    }
    expect(categories).toHaveLength(62); // 16 个大类 + 46 个子分类
    expect(ids.size).toBe(62); // id 唯一
  });

  it("边界：两次调用的 id/名字/图标/颜色/order 完全一致（确定性），只有 createdAt 是当下时间", () => {
    const first = seedLedgerBook();
    const second = seedLedgerBook();
    expect(first.accounts.map((account) => account.id)).toEqual(second.accounts.map((account) => account.id));
    expect(first.categories.map((category) => category.id)).toEqual(second.categories.map((category) => category.id));
    // 剥掉 createdAt 之后两本账逐字段相等（createdAt 是 now()，跨毫秒就不等，所以不能直接 deep-equal）
    const strip = (book: LedgerBook) => JSON.parse(JSON.stringify(book, (key, value) => (key === "createdAt" ? "X" : value)));
    expect(strip(first)).toEqual(strip(second));
    expect(first.entries).toEqual([]);
    expect(first.accountTypes).toEqual([]);
    // createdAt 是合法的 ISO 时间戳
    for (const account of first.accounts) expect(account.createdAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });
});

// ---------------------------------------------------------------------------
// normalizeSettings —— 与本次改动相关的字号 clamp 与 uiScale clamp
// ---------------------------------------------------------------------------

describe("normalizeSettings（字号与缩放的夹取）", () => {
  it("正常路径：缺省值 = defaultSettings", () => {
    const settings = normalizeSettings(undefined);
    expect(settings.appearance.uiScale).toBe(defaultSettings.appearance.uiScale);
    expect(settings.appearance.uiFontSize).toBe(defaultSettings.appearance.uiFontSize);
    expect(settings.appearance.markdownFontSize).toBe(defaultSettings.appearance.markdownFontSize);
    expect(settings.appearance.ledgerFontSize).toBe(defaultSettings.appearance.ledgerFontSize);
    expect(settings.appearance.diaryFontSize).toBe(defaultSettings.appearance.diaryFontSize);
    expect(settings.appearance.editorFontSize).toBe(defaultSettings.appearance.editorFontSize);
    expect(settings.appearance.tagFontSize).toBe(defaultSettings.appearance.tagFontSize);
    expect(settings.appearance.editorWidthPercent).toBe(72);
    expect(settings.appearance.editorHeightPercent).toBe(86);
  });

  it("正常路径：六种字号各自的 clamp 区间（UI 14-22、正文/记账/日记/编辑器 14-26、标签 11-30）", () => {
    const big = normalizeSettings({
      appearance: { uiFontSize: 99, markdownFontSize: 99, ledgerFontSize: 99, diaryFontSize: 99, editorFontSize: 99, tagFontSize: 99 }
    }).appearance;
    expect([big.uiFontSize, big.markdownFontSize, big.ledgerFontSize, big.diaryFontSize, big.editorFontSize, big.tagFontSize]).toEqual([
      22, 26, 26, 26, 26, 30
    ]);
    const small = normalizeSettings({
      appearance: { uiFontSize: 1, markdownFontSize: 1, ledgerFontSize: 1, diaryFontSize: 1, editorFontSize: 1, tagFontSize: 1 }
    }).appearance;
    expect([small.uiFontSize, small.markdownFontSize, small.ledgerFontSize, small.diaryFontSize, small.editorFontSize, small.tagFontSize]).toEqual([
      14, 14, 14, 14, 14, 11
    ]);
  });

  it("边界：字号四舍五入取整，编辑器宽高百分比夹在 30-100", () => {
    const appearance = normalizeSettings({
      appearance: { diaryFontSize: 20.6, ledgerFontSize: 18.4, editorWidthPercent: 5, editorHeightPercent: 500 }
    }).appearance;
    expect(appearance.diaryFontSize).toBe(21);
    expect(appearance.ledgerFontSize).toBe(18);
    expect(appearance.editorWidthPercent).toBe(30);
    expect(appearance.editorHeightPercent).toBe(100);
  });

  it("边界：uiScale 夹在 0.5-1.5，非有限数字回落默认（legacy display.uiScale 也认）", () => {
    expect(normalizeSettings({ appearance: { uiScale: 9 } }).appearance.uiScale).toBe(1.5);
    expect(normalizeSettings({ appearance: { uiScale: 0.01 } }).appearance.uiScale).toBe(0.5);
    expect(normalizeSettings({ appearance: { uiScale: 1 } }).appearance.uiScale).toBe(1);
    // Infinity / NaN 都不是有限数 → 不夹取，直接回落默认（字号那套同口径，见下条）
    expect(normalizeSettings({ appearance: { uiScale: Infinity } }).appearance.uiScale).toBe(defaultSettings.appearance.uiScale);
    expect(normalizeSettings({ appearance: { uiScale: "1.2" } }).appearance.uiScale).toBe(defaultSettings.appearance.uiScale);
    expect(normalizeSettings({ display: { uiScale: 0.2 } }).appearance.uiScale).toBe(0.5);
    // appearance 优先于 legacy display
    expect(normalizeSettings({ appearance: { uiScale: 1.1 }, display: { uiScale: 0.6 } }).appearance.uiScale).toBe(1.1);
  });

  it("回归（v0.8.0）：NaN / Infinity 不能从字号的 clamp 里穿过去", () => {
    // 早先 normalizeFontSize 只判 typeof === "number"，Math.round(NaN) 一路 NaN 出来
    // → CSS 变量变成 `--font-ledger: NaNpx`，整页字号失效。现在与 normalizeUiScale 同口径。
    const appearance = normalizeSettings({ appearance: { ledgerFontSize: NaN, uiScale: NaN } }).appearance;
    expect(appearance.ledgerFontSize).toBe(defaultSettings.appearance.ledgerFontSize);
    expect(appearance.uiScale).toBe(defaultSettings.appearance.uiScale);
    for (const key of ["uiFontSize", "markdownFontSize", "editorFontSize", "ledgerFontSize", "diaryFontSize"] as const) {
      for (const bad of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
        expect(Number.isFinite(normalizeSettings({ appearance: { [key]: bad } }).appearance[key])).toBe(true);
      }
    }
  });

  it("正常路径：记账 / 日记的视图选择与外观默认值", () => {
    expect(normalizeSettings({ ledger: { view: "stats" } }).ledger.view).toBe("stats");
    expect(normalizeSettings({ ledger: { view: "assets" } }).ledger.view).toBe("assets");
    expect(normalizeSettings({ ledger: { view: "nope" } }).ledger.view).toBe("list");
    expect(normalizeSettings(undefined).ledger.view).toBe("list");
    expect(normalizeSettings(undefined).ledger.backgroundColor).toBe("#eef3ee");
    expect(normalizeSettings({ diary: { view: "calendar" } }).diary.view).toBe("calendar");
    expect(normalizeSettings({ diary: { view: "nope" } }).diary.view).toBe("list");
    // 非法颜色回落默认（记账/日记的主题色与背景色都进同步共享子集）
    expect(normalizeSettings({ ledger: { accent: "红" } }).ledger.accent).toBe("");
    expect(normalizeSettings({ ledger: { accent: "#abcdef" } }).ledger.accent).toBe("#abcdef");
    expect(normalizeSettings({ ledger: { backgroundColor: "nope" } }).ledger.backgroundColor).toBe("#eef3ee");
  });

  it("边界：normalizeSettings 不改动入参对象", () => {
    const raw = { appearance: { uiScale: 9, uiFontSize: 99 }, ledger: { view: "stats" } };
    const snapshot = JSON.parse(JSON.stringify(raw));
    normalizeSettings(raw);
    expect(raw).toEqual(snapshot);
  });
});

describe("seedLedgerBook 与归一化的接口契约", () => {
  it("正常路径：种子账本的每个账户/分类都能被 ledger.ts 的索引查到", () => {
    const book: LedgerBook = seedLedgerBook();
    const accounts: LedgerAccount[] = book.accounts;
    const categories: LedgerCategory[] = book.categories;
    expect(accounts.every((account) => typeof account.id === "string" && account.id !== "")).toBe(true);
    expect(categories.every((category) => typeof category.id === "string" && category.id !== "")).toBe(true);
    // 归一化不会把种子账本里的任何东西丢掉（幂等）
    const round = normalizeLedger(book);
    expect(round.accounts.map((account) => account.id)).toEqual(accounts.map((account) => account.id));
    expect(round.categories.map((category) => category.id)).toEqual(categories.map((category) => category.id));
  });
});

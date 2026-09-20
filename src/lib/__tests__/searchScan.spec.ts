/**
 * 全局搜索的预折叠索引与分块扫描器（v0.8.6 需求 1）。
 *
 * 钉住三件事：① 折叠串按对象身份缓存（同一对象重复搜不再 lowercase 全文）；
 * ② 匹配语义与领域模块一致（过滤走的就是这份折叠串）；③ 扫描器分批、封顶 200、
 * 完成后按 updatedAt 全局排序，且取消语义正确。
 */
import { describe, expect, it, vi } from "vitest";

import { normalizeLedger, normalizeState } from "../defaults";
import { createSearchScanner, SEARCH_CHUNK, SEARCH_HIT_LIMIT } from "../searchScan";
import { diaryFold, filterDiaries } from "../diary";
import { ledgerFold, ledgerMatches } from "../ledger";
import type { AppState, DiaryEntry, LedgerBook, LedgerEntry, SearchHit, Task } from "../types";

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

function task(id: string, markdown: string, updatedAt: string, nodeId = "n1"): Task {
  return {
    id,
    nodeId,
    markdown,
    completed: false,
    createdAt: updatedAt,
    updatedAt,
    tags: [],
    emojis: [],
    isMyDay: false,
    order: 1
  } as unknown as Task;
}

function diary(id: string, title: string, markdown: string, updatedAt: string, tags: string[] = []): DiaryEntry {
  return {
    id,
    date: "2026-09-01",
    title,
    markdown,
    createdAt: updatedAt,
    updatedAt,
    tags: tags.map((text, index) => ({ id: `t${index}`, text }))
  } as unknown as DiaryEntry;
}

function ledgerEntry(id: string, note: string, amountCents: number, date: string): LedgerEntry {
  return {
    id,
    kind: "expense",
    amountCents,
    accountId: "a1",
    categoryId: "c1",
    date,
    note,
    createdAt: date,
    updatedAt: date
  } as unknown as LedgerEntry;
}

/** 最小可用的 AppState / 账本（只放搜索会读的字段） */
function fixture(...input: Array<{ tasks?: Task[]; diaries?: DiaryEntry[]; entries?: LedgerEntry[] }>) {
  const taskList = input.flatMap((part) => part.tasks ?? []);
  const diaries = input.flatMap((part) => part.diaries ?? []);
  const entries = input.flatMap((part) => part.entries ?? []);
  const state = normalizeState({
    nodes: [
      { id: "n1", name: "工作", kind: "entry", parentId: null, order: 1, cardStyle: "todo" },
      { id: "sys", name: "我的一天", kind: "system", parentId: null, order: 0 }
    ],
    tasks: taskList,
    selectedNodeId: "n1"
  }) as AppState;
  const ledger = normalizeLedger({
    accounts: [{ id: "a1", name: "现金", type: "cash", initialCents: 0, order: 1 }],
    categories: [{ id: "c1", name: "餐饮", kind: "expense", order: 1 }],
    entries
  }) as LedgerBook;
  return { state, diaries, ledger };
}

function collect(query: string, source: () => ReturnType<typeof fixture>) {
  const batches: Array<{ hits: SearchHit[]; done: boolean }> = [];
  const scanner = createSearchScanner({
    source,
    onBatch: (hits, done) => batches.push({ hits, done }),
    schedule: (run) => run()
  });
  scanner.run(query);
  return batches;
}

// ---------------------------------------------------------------------------
// 预折叠索引
// ---------------------------------------------------------------------------

describe("预折叠索引", () => {
  it("按对象身份缓存：同一对象第二次搜索不再 lowercase 全文", () => {
    const entry = diary("d1", "标题", "正文内容", "2026-09-01T10:00:00Z");
    const spy = vi.spyOn(String.prototype, "toLowerCase");
    filterDiaries([entry], "正文");
    const afterFirst = spy.mock.calls.length;
    filterDiaries([entry], "正文");
    const secondRun = spy.mock.calls.length - afterFirst;
    // 只剩搜索词自己那一次；折叠串直接命中缓存
    expect(secondRun).toBe(1);
    spy.mockRestore();
  });

  it("日记：标题 / 正文 / 标签三处都能命中（与旧实现同语义）", () => {
    const entry = diary("d1", "会议纪要", "讨论了发布计划", "2026-09-01T10:00:00Z", ["重要"]);
    expect(filterDiaries([entry], "会议")).toHaveLength(1);
    expect(filterDiaries([entry], "发布计划")).toHaveLength(1);
    expect(filterDiaries([entry], "重要")).toHaveLength(1);
    expect(filterDiaries([entry], "不存在")).toHaveLength(0);
    expect(diaryFold(entry)).toContain("会议纪要");
  });

  it("记账：分类名、大类名、备注、金额形态（含符号）都认", () => {
    const book = fixture({ entries: [ledgerEntry("l1", "午饭", 123456, "2026-09-01")] }).ledger;
    const entry = book.entries[0];
    const fold = ledgerFold(book, entry);
    expect(fold.text).toContain("餐饮");
    for (const needle of ["餐饮", "午饭", "1234.56", "1,234.56", "1235", "-1234.56", "-1,234.56"]) {
      expect(ledgerMatches(book, entry, needle), needle).toBe(true);
    }
    expect(ledgerMatches(book, entry, "晚饭")).toBe(false);
    // 只由逗号/空格组成的查询：金额串里的千分位逗号不能把它变成「列出全部」
    expect(ledgerMatches(book, entry, ",")).toBe(false);
    expect(ledgerMatches(book, entry, " ")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// 分块扫描器
// ---------------------------------------------------------------------------

describe("分块扫描器", () => {
  it("空词同步回空结果（清空立即生效，不走 idle）", () => {
    const { state, diaries, ledger } = fixture({ tasks: [task("t1", "abc", "2026-09-01T00:00:00Z")] });
    const batches = collect("   ", () => ({ state, diaries, ledger }));
    expect(batches).toEqual([{ hits: [], done: true }]);
  });

  it("分块推进：超过一个 chunk 时分多批，最后一批才是最终结果", () => {
    const total = SEARCH_CHUNK * 2 + 37;
    const tasks = Array.from({ length: total }, (_, index) =>
      task(`t${index}`, `命中 ${index}`, `2026-09-01T00:00:${String(index % 60).padStart(2, "0")}Z`)
    );
    const { state, diaries, ledger } = fixture({ tasks });
    const batches = collect("命中", () => ({ state, diaries, ledger }));
    expect(batches.length).toBe(3);
    expect(batches.slice(0, -1).every((batch) => !batch.done)).toBe(true);
    expect(batches.at(-1)?.done).toBe(true);
    // 中间批不排序、只是「扫到哪儿显示到哪儿」；最终批封顶
    expect(batches.at(-1)?.hits.length).toBe(SEARCH_HIT_LIMIT);
  });

  it("最终结果按 updatedAt 全局排序（跨三种命中混排）", () => {
    const { state, diaries, ledger } = fixture(
      { tasks: [task("t1", "关键词", "2026-09-01T00:00:00Z")] },
      { diaries: [diary("d1", "关键词", "", "2026-09-03T00:00:00Z")] },
      { entries: [ledgerEntry("l1", "关键词", 100, "2026-09-02")] }
    );
    const batches = collect("关键词", () => ({ state, diaries, ledger }));
    const hits = batches.at(-1)!.hits;
    expect(hits.map((hit) => hit.kind)).toEqual(["diary", "ledger", "task"]);
  });

  it("cancel 之后不再有批次落地（防抖词变化作废上一轮）", () => {
    const tasks = Array.from({ length: SEARCH_CHUNK * 3 }, (_, index) => task(`t${index}`, "命中", "2026-09-01T00:00:00Z"));
    const { state, diaries, ledger } = fixture({ tasks });
    const batches: SearchHit[][] = [];
    const scanner = createSearchScanner({
      source: () => ({ state, diaries, ledger }),
      onBatch: (hits) => {
        batches.push(hits);
        scanner.cancel();
      },
      schedule: (run) => run()
    });
    scanner.run("命中");
    expect(batches.length).toBe(1);
  });

  it("取消上一轮后开新一轮：只有新词的结果落地", () => {
    const { state, diaries, ledger } = fixture(
      { tasks: [task("t1", "苹果", "2026-09-01T00:00:00Z")] },
      { diaries: [diary("d1", "香蕉", "", "2026-09-02T00:00:00Z")] }
    );
    // 手动泵的分片调度：两次 run 之间不执行，复现「分片还在队列里、词已经变了」
    const queue: Array<() => void> = [];
    const batches: SearchHit[][] = [];
    const scanner = createSearchScanner({
      source: () => ({ state, diaries, ledger }),
      onBatch: (hits, done) => {
        if (done) batches.push(hits);
      },
      schedule: (run) => queue.push(run)
    });
    scanner.run("苹果");
    scanner.run("香蕉");
    while (queue.length > 0) queue.shift()!();
    expect(batches).toHaveLength(1);
    expect(batches[0].map((hit) => hit.key)).toEqual(["diary-d1"]);
  });
});

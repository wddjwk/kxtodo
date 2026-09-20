// 性能基准（v0.8.0 起）：用「大数据量」把渲染与交互路径的真实成本量出来。
//
// 为什么需要它：这一版的主题是性能，而性能改动最容易自欺——改完感觉快了、其实没快，
// 或者快了首屏却慢了交互。这个脚本给出一组可复现的数字，改前改后各跑一次对比。
//
// 数据量：300 条任务（含长 markdown 与代码块）/ 300 篇日记 / 3000 笔账目跨 12 个月。
// 跑在 vite dev 的浏览器预览路径上（localStorage legacy），量到的是**前端渲染与重算**；
// core 的写路径（全量序列化 / 审计 / 快照）由 cargo test 与真机体感覆盖，不在这里。
//
// 计时口径：一律在页面内用 `performance.now()` 量「派发事件 → 连过两帧 rAF」，
// 也就是 Svelte 的更新已经 flush 且浏览器已经画完。**不要用 Playwright 的
// waitForTimeout 兜底**——那个固定等待会把真实耗时整个淹掉（第一版就这么错了）。
//
// 用法：先 `npm run dev`，再 `node scripts/perf-bench.mjs`。
// 关键读数：
//   - renderStats.block：完整 markdown 渲染的调用次数。**首屏全是折叠卡片时必须是 0**
//     （v0.8.0 之前等于卡片数，因为 `$: fullHtml = renderMarkdown(...)` 是急切求值；
//      再叠上图片缓存逐张 update，是「卡片数 × 图片数」量级）。
//   - renderStats.inlineHit / inline：折叠态标题的渲染命中率（记忆化）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
// v0.8.4 需求 6：口径提到「真实会卡的量级」——300/300/3000 测不出「全量挂载」这类问题
// （日记界面卡死、内存 2GB 就是在这个量级下才暴露的）。
const TASK_COUNT = 1000;
const DIARY_COUNT = 5000;
const ENTRY_COUNT = 10000;

const STATE_KEY = "todo-note-state-v3";
const DIARY_KEY = "todo-note-diary-v1";
const LEDGER_KEY = "todo-note-ledger-v1";

/** 造一份大数据量的本地状态：形状与 defaults.ts 的 normalize 输入一致（缺的字段它会补）。 */
function buildSeed() {
  const now = new Date().toISOString();
  const entryId = "bench-entry";
  const nodes = [
    { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now, order: 0 },
    { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now, order: 1 },
    { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now, order: 2 },
    { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now, order: 3 },
    { id: entryId, kind: "entry", name: "基准条目", icon: "inbox", parentId: null, createdAt: now, order: 4 }
  ];
  // 每 10 条混 1 条长的（多行 + 代码块 + 公式 + 高亮），其余是单行超长标题
  // （单行超长要折行 → 卡片会量出「可展开」，顺带覆盖 measure 上报链）
  const tasks = Array.from({ length: TASK_COUNT }, (_, index) => {
    const long = index % 10 === 0;
    const markdown = long
      ? [
          `第 ${index} 条：这是一条很长的基准任务标题，用来把折叠态撑到需要折行的程度，`.repeat(3),
          "",
          "- 列表项一",
          "- 列表项二 **加粗** 与 ==高亮==",
          "",
          "```js",
          "const total = items.reduce((sum, item) => sum + item.cents, 0);",
          "console.log(total);",
          "```",
          "",
          "行内公式 $E = mc^2$ 与 `code`。"
        ].join("\n")
      : `第 ${index} 条：单行但足够长的基准任务标题，用来触发折叠态的折行判定与省略号`.repeat(2);
    return {
      id: `bench-task-${index}`,
      nodeId: entryId,
      markdown,
      completed: index % 7 === 0,
      important: false,
      myDay: false,
      dueDate: index % 5 === 0 ? new Date().toISOString().slice(0, 10) : "",
      tags: [],
      emojis: [],
      expanded: false,
      createdAt: now,
      updatedAt: now
    };
  });
  const diaries = Array.from({ length: DIARY_COUNT }, (_, index) => {
    const date = new Date(Date.now() - index * 86_400_000).toISOString().slice(0, 10);
    return {
      id: `bench-diary-${index}`,
      date,
      title: `基准日记 ${index}`,
      // 5000 篇要塞进 localStorage（约 5MB 配额），正文别太长——这一项测的是
      // 「卡片数量」而不是「单卡正文体积」
      markdown: `今天做了第 ${index} 件事。`.repeat(4),
      tags: [],
      mood: "",
      weather: "",
      expanded: false,
      createdAt: `${date}T09:00:00.000Z`,
      updatedAt: `${date}T09:00:00.000Z`
    };
  });
  const accounts = [
    { id: "bench-acc-1", name: "现金", kind: "cash", initialCents: 100000, order: 0, icon: "", color: "", createdAt: now, updatedAt: now },
    { id: "bench-acc-2", name: "银行卡", kind: "debit", initialCents: 5000000, order: 1, icon: "", color: "", createdAt: now, updatedAt: now }
  ];
  const categories = [
    { id: "bench-cat-1", name: "餐饮", side: "expense", parentId: null, order: 0, icon: "Utensils", color: "#f0862c", createdAt: now, updatedAt: now },
    { id: "bench-cat-2", name: "早餐", side: "expense", parentId: "bench-cat-1", order: 0, icon: "Coffee", color: "", createdAt: now, updatedAt: now },
    { id: "bench-cat-3", name: "交通", side: "expense", parentId: null, order: 1, icon: "Bus", color: "#3d8bfd", createdAt: now, updatedAt: now },
    { id: "bench-cat-4", name: "工资", side: "income", parentId: null, order: 0, icon: "Wallet", color: "#2f9e6e", createdAt: now, updatedAt: now }
  ];
  const entries = Array.from({ length: ENTRY_COUNT }, (_, index) => {
    const date = new Date(Date.now() - (index % 365) * 86_400_000).toISOString().slice(0, 10);
    const income = index % 12 === 0;
    return {
      id: `bench-ledger-${index}`,
      kind: income ? "income" : "expense",
      amountCents: 100 + ((index * 37) % 20000),
      accountId: accounts[index % 2].id,
      categoryId: income ? "bench-cat-4" : index % 3 === 0 ? "bench-cat-2" : "bench-cat-3",
      date,
      time: `${String(8 + (index % 12)).padStart(2, "0")}:${String(index % 60).padStart(2, "0")}`,
      note: index % 4 === 0 ? `基准备注 ${index}` : "",
      images: [],
      createdAt: `${date}T10:00:00.000Z`,
      updatedAt: `${date}T10:00:00.000Z`
    };
  });
  return {
    state: { nodes, tasks, backgrounds: {}, selectedNodeId: entryId },
    diaries,
    ledger: { accounts, categories, entries, accountTypes: [] }
  };
}

const checks = [];
function check(name, ok, note = "") {
  checks.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${note ? ` — ${note}` : ""}`);
}

async function timed(page, label, body, samples = 3) {
  const values = [];
  for (let i = 0; i < samples; i += 1) {
    values.push(await page.evaluate(body, i));
  }
  values.sort((a, b) => a - b);
  const mid = values[Math.floor(values.length / 2)];
  console.log(`${label.padEnd(40)} ${mid.toFixed(1).padStart(8)} ms   (${values.map((v) => v.toFixed(1)).join(" / ")})`);
  return mid;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));

  await page.goto(URL, { waitUntil: "load" });
  const seed = buildSeed();
  await page.evaluate(
    ({ stateKey, diaryKey, ledgerKey, seed }) => {
      localStorage.clear();
      localStorage.setItem(stateKey, JSON.stringify(seed.state));
      localStorage.setItem(diaryKey, JSON.stringify(seed.diaries));
      localStorage.setItem(ledgerKey, JSON.stringify(seed.ledger));
    },
    { stateKey: STATE_KEY, diaryKey: DIARY_KEY, ledgerKey: LEDGER_KEY, seed }
  );

  console.log(`\n数据量：${TASK_COUNT} 任务 / ${DIARY_COUNT} 日记 / ${ENTRY_COUNT} 账目\n`);
  await page.reload({ waitUntil: "load" });
  const t0 = Date.now();
  await page.waitForSelector(".task-card", { timeout: 60000 });
  const firstPaint = Date.now() - t0;
  const cardCount = await page.$$eval(".task-card", (nodes) => nodes.length);
  const stats = await page.evaluate(() => window.__kxtodoRenderStats ?? null);
  console.log(`--- 首屏 ---`);
  console.log(`${"reload → 首张任务卡（Node 侧计时）".padEnd(40)} ${firstPaint.toFixed(1).padStart(8)} ms`);
  console.log(`DOM 里的卡片：${cardCount}   渲染计数：${JSON.stringify(stats)}`);

  check("首屏无 pageerror", errors.length === 0, errors[0] ?? "");
  check(
    "折叠卡片不产生完整 markdown 渲染（block === 0）",
    stats !== null && stats.block === 0,
    `block=${stats?.block}，卡片 ${cardCount} 张全部折叠`
  );


  // --- 日记列表（v0.8.4 需求 3/6）：5000 篇的打开耗时、挂载卡片数与滚动长任务 ---
  const diaryOpen = await page.evaluate(async () => {
    const at = performance.now();
    document.querySelector(".system-nav .nav-row[title='日记']")?.click();
    const deadline = at + 15000;
    while (performance.now() < deadline) {
      if (document.querySelector(".diary-card")) break;
      await new Promise((resolve) => setTimeout(resolve, 8));
    }
    await new Promise((resolve) => setTimeout(resolve, 120));
    return performance.now() - at;
  });
  const diaryStats = await page.evaluate(() => ({
    cards: document.querySelectorAll(".diary-card").length,
    nodes: document.querySelector(".diary-scroll")?.querySelectorAll("*").length ?? 0,
    heap: performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : -1
  }));
  console.log(`--- 日记列表（${DIARY_COUNT} 篇）---`);
  console.log(`${"打开日记界面（首张卡上屏）".padEnd(40)} ${diaryOpen.toFixed(1).padStart(8)} ms`);
  console.log(`挂载卡片 ${diaryStats.cards}   子树节点 ${diaryStats.nodes}   堆 ${diaryStats.heap}MB`);
  check("日记界面打开 < 1000ms（需求 6 验收）", diaryOpen < 1000, `${diaryOpen.toFixed(0)}ms`);
  check("日记只挂视口附近的一段（不是 5000 张）", diaryStats.cards > 0 && diaryStats.cards < 120, `${diaryStats.cards} 张`);
  check("堆占用 < 500MB（需求 6 验收）", diaryStats.heap > 0 && diaryStats.heap < 500, `${diaryStats.heap}MB`);

  // 滚动长任务：PerformanceObserver 抓 >100ms 的 longtask
  const scrollLong = await page.evaluate(async () => {
    const scroller = document.querySelector(".diary-scroll");
    if (!scroller) return { tasks: -1, max: -1, cards: -1 };
    const longTasks = [];
    let observer = null;
    try {
      observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) longTasks.push(entry.duration);
      });
      observer.observe({ entryTypes: ["longtask"] });
    } catch {
      observer = null;
    }
    for (let step = 1; step <= 8; step += 1) {
      scroller.scrollTop = (scroller.scrollHeight / 9) * step;
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
    observer?.disconnect();
    return {
      tasks: longTasks.length,
      max: longTasks.length ? Math.max(...longTasks) : 0,
      cards: document.querySelectorAll(".diary-card").length
    };
  });
  console.log(
    `滚动 8 屏：长任务 ${scrollLong.tasks} 个（最长 ${scrollLong.max.toFixed(0)}ms），当前挂载 ${scrollLong.cards} 张`
  );
  check(
    "滚动过程没有 >100ms 的长任务（需求 6 验收）",
    scrollLong.max < 100,
    `最长 ${scrollLong.max.toFixed(0)}ms（${scrollLong.tasks} 个长任务）`
  );
  check("滚动后仍只挂一段", scrollLong.cards > 0 && scrollLong.cards < 120, `${scrollLong.cards} 张`);

  // --- Entry 任务列表（1000 条，需求 5/6）---
  await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".tree-row")];
    rows.find((el) => (el.textContent ?? "").includes("基准条目"))?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  const entryOpen = await page.evaluate(async () => {
    const at = performance.now();
    const deadline = at + 15000;
    while (performance.now() < deadline) {
      if (document.querySelector(".task-list .task-card")) break;
      await new Promise((resolve) => setTimeout(resolve, 8));
    }
    await new Promise((resolve) => setTimeout(resolve, 120));
    return performance.now() - at;
  });
  const entryStats = await page.evaluate(() => ({
    cards: document.querySelectorAll(".task-list .task-card").length,
    heap: performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : -1
  }));
  console.log(`--- Entry（${TASK_COUNT} 条任务）---`);
  console.log(`${"打开条目（首张卡上屏）".padEnd(40)} ${entryOpen.toFixed(1).padStart(8)} ms`);
  console.log(`挂载卡片 ${entryStats.cards}   堆 ${entryStats.heap}MB`);
  check("1000 卡条目打开 < 2000ms（需求 6 验收）", entryOpen < 2000, `${entryOpen.toFixed(0)}ms`);
  check("任务列表只挂一段（不是 1000 张）", entryStats.cards > 0 && entryStats.cards < 120, `${entryStats.cards} 张`);


  console.log("\n--- 交互（页内计时，各取 3 次中位数）---");
  await timed(page, "勾选一条任务 → 上屏", async (i) => {
    const at = performance.now();
    const buttons = document.querySelectorAll(".task-card .task-check");
    buttons[(i * 7) % buttons.length]?.click();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return performance.now() - at;
  });

  await timed(page, "展开一张长卡片（首次完整渲染）", async () => {
    const card = [...document.querySelectorAll(".task-card")].find((node) => node.querySelector("pre"));
    const target = card ?? document.querySelector(".task-card");
    const at = performance.now();
    target?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const cost = performance.now() - at;
    target?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    return cost;
  });

  await timed(page, "再展开同一张（记忆化命中）", async () => {
    const target = document.querySelector(".task-card");
    const at = performance.now();
    target?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const cost = performance.now() - at;
    target?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    return cost;
  });

  // 搜索：从敲下字符到「标题变成搜索结果 + 命中卡片上屏」，含 180ms 防抖
  // （防抖是刻意的，量的是用户感知的总延迟）。查询词挑一个只命中一条的，
  // 免得量到的是「几百张结果卡的渲染」而不是搜索本身。
  const searchLatency = await page.evaluate(async () => {
    const input = document.querySelector(".search-box input");
    if (!input) return -1;
    const at = performance.now();
    input.value = "第 123 条";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const deadline = at + 3000;
    const entered = () =>
      document.querySelector(".list-header h1")?.textContent?.includes("搜索结果") === true;
    while (performance.now() < deadline) {
      if (entered() && document.querySelector(".task-list .task-card")) break;
      await new Promise((resolve) => setTimeout(resolve, 4));
    }
    const cost = entered() ? performance.now() - at : -1;
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    return cost;
  });
  check("搜索能命中（防抖没有把搜索弄坏）", searchLatency > 0, `${searchLatency.toFixed(0)}ms`);
  console.log(`${"搜索按键 → 结果上屏（含防抖）".padEnd(40)} ${searchLatency.toFixed(1).padStart(8)} ms`);

  // 宽词搜索（v0.8.6 需求 1 验收）：命中 7000+ 条，量「扫完 + 最终结果上屏」的总时长
  // 与扫描期间的长任务；命中数必须封顶 200。窗口化保证只挂视口附近那段 DOM。
  const broadSearch = await page.evaluate(async () => {
    const input = document.querySelector(".search-box input");
    if (!input) return null;
    const longTasks = [];
    let observer = null;
    try {
      observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) longTasks.push(entry.duration);
      });
      observer.observe({ entryTypes: ["longtask"] });
    } catch {
      observer = null;
    }
    const at = performance.now();
    input.value = "基";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const deadline = at + 15000;
    let settled = -1;
    while (performance.now() < deadline) {
      const stats = window.__kxtodoSearch;
      if (stats && !stats.scanning && stats.hits > 0 && document.querySelector(".task-list .task-card")) {
        settled = performance.now() - at;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 8));
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const stats = window.__kxtodoSearch ?? { hits: -1, scanning: true };
    const result = {
      settled,
      hits: stats.hits,
      domCards: document.querySelectorAll(".task-list .task-card").length,
      longTasks: longTasks.length,
      maxTask: longTasks.length ? Math.max(...longTasks) : 0
    };
    observer?.disconnect();
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 60));
    return result;
  });
  if (broadSearch) {
    console.log(`--- 宽词搜索「基」（命中 ${broadSearch.hits} 条）---`);
    console.log(`${"输入 → 扫描完成、结果上屏".padEnd(40)} ${broadSearch.settled.toFixed(1).padStart(8)} ms`);
    console.log(
      `命中 ${broadSearch.hits} 条封顶，DOM 里 ${broadSearch.domCards} 张卡；扫描期长任务 ${broadSearch.longTasks} 个（最长 ${broadSearch.maxTask.toFixed(0)}ms）`
    );
    check("宽词搜索响应 < 1000ms（需求 1 验收）", broadSearch.settled > 0 && broadSearch.settled < 1000, `${broadSearch.settled.toFixed(0)}ms`);
    check("命中数封顶 200 条（需求 1 验收）", broadSearch.hits === 200, `${broadSearch.hits} 条`);
    check("扫描期间没有 >100ms 的长任务（需求 1 验收）", broadSearch.maxTask < 100, `最长 ${broadSearch.maxTask.toFixed(0)}ms`);
  }

  const ledgerRow = await page.$(".system-nav .nav-row:has-text('记账')");
  if (ledgerRow) {
    await ledgerRow.click();
    await page.waitForSelector(".ledger-view", { timeout: 20000 });
    await timed(page, "记账列表换月（3000 笔取一个月）", async () => {
      const scroll = document.querySelector(".ledger-scroll");
      if (!scroll) return -1;
      const at = performance.now();
      scroll.dispatchEvent(
        new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true })
      );
      scroll.scrollTop = scroll.scrollHeight;
      scroll.dispatchEvent(new Event("scroll"));
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return performance.now() - at;
    });
    await timed(page, "切到统计视图（全量聚合 + 画图）", async () => {
      const button = document.querySelector(".ledger-view-switch button[title='统计视图']");
      if (!button) return -1;
      const at = performance.now();
      button.click();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return performance.now() - at;
    });
    await timed(page, "切回列表视图", async () => {
      const button = document.querySelector(".ledger-view-switch button[title='列表视图']");
      if (!button) return -1;
      const at = performance.now();
      button.click();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return performance.now() - at;
    });
  }

  const finalStats = await page.evaluate(() => window.__kxtodoRenderStats ?? null);
  console.log(`\n结束时渲染计数：${JSON.stringify(finalStats)}`);
  if (finalStats) {
    check(
      "完整渲染有记忆化命中（blockHit > 0）",
      finalStats.block > 0 && finalStats.blockHit > 0,
      `block=${finalStats.block} hit=${finalStats.blockHit}`
    );
    check(
      "折叠态标题渲染有记忆化命中",
      finalStats.inlineHit > 0,
      `inline=${finalStats.inline} hit=${finalStats.inlineHit}`
    );
  }
  check("全程无 pageerror", errors.length === 0, errors[0] ?? "");
} finally {
  await browser.close();
}

const failed = checks.filter((item) => !item.ok).length;
console.log(`\n${checks.length - failed}/${checks.length} 项通过`);
process.exit(failed > 0 ? 1 : 0);

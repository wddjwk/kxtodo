// v0.8.5 回归（v0.8.4 review 修复批次）。覆盖：
// P0 三条（转账行裸类名踩踏 / 窗口化跨阈值高度记账 / 发送后仍在线由 cargo 钉）+ 
// UX 偏差（预设色块单击落盘 / 草稿纸字数 / 工具子页标题 / 图例跟随侧段控 / 日记滚动锚点）+
// 拖动两件套（图标模式落点 / 分组树实时让位）+ 传输分区与 relay 菜单 + 深层编号与大字号视觉。
// 用法：node scripts/v085-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";
const LEDGER_KEY = "todo-note-ledger-v1";
const DIARY_KEY = "todo-note-diary-v1";

let failures = 0;
let passes = 0;
function check(name, ok, extra = "") {
  if (ok) passes += 1;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra && !ok ? " — " + extra : ""}`);
  if (!ok) failures += 1;
}
const J = (value) => JSON.stringify(value);

function localToday(offsetDays = 0) {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

async function freshPage(context) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

const SYSTEM_NODES = [
  { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null },
  { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null },
  { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null },
  { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null }
];

async function seedState(page, { nodes = [], tasks = [], settings = null, selected = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, selected, stateKey, settingsKey, system }) => {
      const now = new Date().toISOString();
      const all = [...system.map((node) => ({ ...node, createdAt: now })), ...nodes.map((node) => ({ createdAt: now, ...node }))];
      localStorage.setItem(
        stateKey,
        JSON.stringify({
          schemaVersion: 3,
          nodes: all,
          tasks,
          backgrounds: {},
          selectedNodeId: selected ?? nodes[0]?.id ?? "entry-a"
        })
      );
      if (settings) {
        const existing = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings, schemaVersion: 3 }));
      }
    },
    { nodes, tasks, settings, selected, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY, system: SYSTEM_NODES }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
}

function makeTask(overrides) {
  const now = new Date().toISOString();
  return {
    id: `task-${Math.random().toString(16).slice(2, 10)}`,
    nodeId: "entry-a",
    markdown: "任务",
    completed: false,
    important: false,
    myDay: false,
    tags: [],
    emojis: [],
    expanded: false,
    createdAt: now,
    updatedAt: now,
    order: 1,
    ...overrides
  };
}

/**
 * 逐段拖动：真实指针是连续采样、每段间隔至少一个事件循环；Playwright 的
 * `{ steps: N }` 一次走完，flip 动画还在途中时矩形就是飞在半路的临时值——
 * 那不是用户输入，落点算在动画中间不代表产品有问题。
 */
async function dragBy(page, from, to, steps = 14) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  for (let i = 1; i <= steps; i += 1) {
    await page.mouse.move(from.x + ((to.x - from.x) * i) / steps, from.y + ((to.y - from.y) * i) / steps);
    await page.waitForTimeout(25);
  }
  await page.waitForTimeout(150);
}
const centerOf = (box) => ({ x: box.x + box.width / 2, y: box.y + box.height / 2 });

const browser = await chromium.launch({ channel: "msedge" });
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const mobile = await browser.newContext({
  userAgent: ANDROID_UA,
  viewport: { width: 393, height: 851 },
  isMobile: true,
  hasTouch: true
});

// ===========================================================================
// 1 转账行不被裸类名踩踏（v0.8.4 的 .transfer 撞上记账转账行）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate((key) => {
    const now = new Date().toISOString();
    const today = new Date();
    const iso = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
    localStorage.setItem(
      key,
      JSON.stringify({
        accounts: [
          { id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now },
          { id: "lacc-02", name: "银行", kind: "bank", icon: "CreditCard", color: "#2f9e6e", initialCents: 200000, createdAt: now }
        ],
        categories: [
          { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now }
        ],
        entries: [
          { id: "le-exp", kind: "expense", amountCents: 1200, accountId: "lacc-01", categoryId: "lcat-exp-01", date: iso, note: "午饭", createdAt: now },
          { id: "le-tr", kind: "transfer", amountCents: 30000, accountId: "lacc-01", toAccountId: "lacc-02", categoryId: "", date: iso, note: "周转", createdAt: now }
        ],
        accountTypes: []
      })
    );
  }, LEDGER_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  await page.waitForTimeout(500);
  const layout = await page.evaluate(() => {
    const row = document.querySelector(".ledger-entry-account.transfer");
    const normalRow = document.querySelector(".ledger-entry-account:not(.transfer)");
    const entryHeights = [...document.querySelectorAll(".ledger-entry")].map((el) => Math.round(el.getBoundingClientRect().height));
    if (!row) return null;
    const style = getComputedStyle(row);
    return {
      direction: style.flexDirection,
      display: style.display,
      width: Math.round(row.getBoundingClientRect().width),
      height: Math.round(row.getBoundingClientRect().height),
      normalHeight: normalRow ? Math.round(normalRow.getBoundingClientRect().height) : 0,
      entryHeights
    };
  });
  check("1 转账行存在且横排（不被 .transfer 竖排）", layout !== null && layout.direction === "row", J(layout));
  check(
    "1 转账行高度与普通行一致（没被 gap:14 撑高）",
    layout !== null && layout.normalHeight > 0 && Math.abs(layout.height - layout.normalHeight) <= 4,
    J(layout)
  );
  check("1 记账列表无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 3 窗口化跨 fullBelow 阈值后高度记账仍然有效（99→101 之后占位与实际一致）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const line = "这是一行用来把卡片撑高的正文内容，重复几遍让每张卡的高度明显大于估高。";
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "大条目", icon: "inbox", parentId: null }],
    tasks: Array.from({ length: 100 }, (_, i) =>
      makeTask({ id: `t-${i}`, markdown: `任务 ${i}\n${line}\n${line}`, order: i + 1 })
    )
  });
  await page.locator(".tree-row", { hasText: "大条目" }).click();
  await page.waitForTimeout(900);
  // 100 条 = fullBelow 阈值内：全量直出（也应当已被量高，v0.8.5 起两种模式都记账）
  const full = await page.evaluate(() => document.querySelectorAll(".task-list .task-card").length);
  check("3 100 条全量直出（阈值内）", full === 100, String(full));

  // 跨过阈值：不重载，直接在界面上加两条（这一下从全量切到窗口化）
  const composer = page.locator(".add-task-bar textarea").first();
  for (const text of ["新任务甲", "新任务乙"]) {
    await composer.click();
    await composer.fill(text);
    await composer.press("Enter");
    await page.waitForTimeout(350);
  }
  await page.waitForTimeout(700);
  const measured = await page.evaluate(() => {
    const stack = document.querySelector(".task-list .virtual-stack");
    const items = [...document.querySelectorAll(".task-list .virtual-item")].map((el) => el.getBoundingClientRect().height);
    const sorted = [...items].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
    return {
      cards: document.querySelectorAll(".task-list .task-card").length,
      stackHeight: stack ? Math.round(stack.getBoundingClientRect().height) : 0,
      median: Math.round(median),
      samples: items.slice(0, 4).map((value) => Math.round(value))
    };
  });
  // 窗口化已启动（挂的卡片数 < 总数），且总高 ≈ 102 × 实测高度（若跨阈值的那批行仍停在
  // estimate=84 上，总高会差出一大截——这就是 v0.8.4 的 bug 的可观测形态）
  const expected = 102 * measured.median;
  check("3 跨过阈值后已窗口化", measured.cards > 0 && measured.cards < 100, J(measured));
  check(
    "3 跨阈值后高度记账仍有效（总高与实测一致）",
    measured.median > 0 && Math.abs(measured.stackHeight - expected) < 102 * 6,
    J({ ...measured, expected })
  );
  check("3 窗口化无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 5 预设背景色块单击即落盘（不需要再点保存）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ markdown: "背景色任务" })]
  });
  await page.locator("button[title='列表菜单']").dispatchEvent("mousedown");
  await page.waitForSelector(".context-menu .color-grid", { timeout: 8000 });
  const target = page.locator(".color-grid button:not(.palette-button):not(.reset-bg-button)").nth(1);
  const wanted = await target.evaluate((el) => getComputedStyle(el).getPropertyValue("--swatch").trim());
  await target.click();
  await page.waitForTimeout(600);
  const stored = await page.evaluate(
    ({ stateKey }) => JSON.parse(localStorage.getItem(stateKey) ?? "{}").backgrounds?.["entry-a"]?.color ?? "",
    { stateKey: STATE_KEY }
  );
  const draftBar = await page.locator(".color-draft-actions").count();
  check("5 预设色块单击即落盘（无需保存）", stored.toLowerCase() === wanted.toLowerCase(), `${stored} vs ${wanted}`);
  check("5 单击预设不弹草稿条", draftBar === 0, String(draftBar));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  const persisted = await page.evaluate(
    ({ stateKey }) => JSON.parse(localStorage.getItem(stateKey) ?? "{}").backgrounds?.["entry-a"]?.color ?? "",
    { stateKey: STATE_KEY }
  );
  check("5 重载后背景色还在", persisted.toLowerCase() === wanted.toLowerCase(), persisted);
  check("5 取色无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 6/7 草稿纸字数回来了 + 工具子页有标题
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  const title = (await page.locator(".toolbox-sub-bar-title").textContent()) ?? "";
  check("7 工具子页有「图标 + 名称」标题", title.includes("草稿纸"), title.trim());
  await page.locator(".scratchpad-area").fill("一二三四五");
  await page.waitForTimeout(300);
  const count = (await page.locator(".scratchpad-count").textContent()) ?? "";
  check("6 草稿纸字数统计回来了", count.includes("5") && count.includes("字"), count.trim());
  const savedHint = await page.locator(".scratchpad").textContent();
  check("6 保存提示没有回来", !(savedHint ?? "").includes("已自动保存"), (savedHint ?? "").trim());

  // 传输工具页标题 + 分区卡片
  await page.locator(".toolbox-sub-bar button[title='返回工具箱']").click();
  await page.waitForTimeout(400);
  await page.locator(".toolbox-card", { hasText: "文件传输助手" }).click();
  await page.waitForSelector(".transfer-page", { timeout: 8000 });
  await page.waitForTimeout(400);
  const transfer = await page.evaluate(() => ({
    title: document.querySelector(".toolbox-sub-bar-title")?.textContent?.trim() ?? "",
    cards: document.querySelectorAll(".transfer-page .transfer-card").length,
    workCard: !!document.querySelector(".transfer-card.transfer-work"),
    tabsInCard: !!document.querySelector(".transfer-card.transfer-work .transfer-tabs")
  }));
  check("30 传输页有标题", transfer.title.includes("文件传输助手"), transfer.title);
  check("30 传输页分了卡片区（至少三块）且滑块在收发卡内", transfer.cards >= 3 && transfer.workCard && transfer.tabsInCard, J(transfer));

  // 31 relay：v0.8.6 需求 10 改成「一级菜单项 + 二级钻取」——
  // 一级项叫「relay 服务」，展开后才看得见三单选（v0.8.7 起文案改成
  // 复用同步配置 / 使用默认服务 / 自定义服务，出厂默认「使用默认服务」）
  await page.locator(".toolbox-sub-bar button[title='外观']").click();
  await page.waitForSelector(".context-menu .menu-item-button", { timeout: 5000 });
  const relayEntry = await page.evaluate(() =>
    [...document.querySelectorAll(".context-menu .menu-item-button")].some((el) => (el.textContent ?? "").includes("relay 服务"))
  );
  check("31 ⋯ 菜单里 relay 是一级菜单项", relayEntry);
  await page.locator(".context-menu .menu-item-button", { hasText: "relay 服务" }).click();
  await page.waitForSelector(".submenu-panel .menu-item-button", { timeout: 5000 });
  const relay = await page.evaluate(() => ({
    rows: [...document.querySelectorAll(".submenu-panel .menu-item-button")].map((el) => el.textContent?.trim() ?? ""),
    hint: document.querySelector(".submenu-panel .relay-hint")?.textContent?.trim() ?? ""
  }));
  check(
    "31 展开子菜单后三单选可见：复用同步配置 / 使用默认服务 / 自定义服务",
    relay.rows.length >= 3 &&
      relay.rows[0].includes("复用同步配置") &&
      relay.rows[1].includes("使用默认服务") &&
      relay.rows[2].includes("自定义服务"),
    J(relay)
  );
  // 选「自定义」出输入框；选「使用默认服务」写进设置（浏览器预览也走 settings 持久化）
  await page.locator(".submenu-panel .menu-item-button", { hasText: "自定义" }).click();
  await page.waitForSelector(".submenu-panel .relay-input", { timeout: 3000 });
  await page.locator(".submenu-panel .menu-item-button", { hasText: "使用默认服务" }).click();
  await page.waitForTimeout(500);
  const relayValue = await page.evaluate(
    ({ key }) => JSON.parse(localStorage.getItem(key) ?? "{}").transfer?.relay ?? "",
    { key: SETTINGS_KEY }
  );
  check("31 选「使用默认服务」写进 settings.transfer.relay = default", relayValue === "default", relayValue);

  // 移动端也看一眼（四图标 2×2 仍成立、卡片不超屏）
  await page.close();
  const mobilePage = await freshPage(mobile);
  await mobilePage.page.locator(".nav-row", { hasText: "工具箱" }).click();
  await mobilePage.page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await mobilePage.page.locator(".toolbox-card", { hasText: "文件传输助手" }).click();
  await mobilePage.page.waitForSelector(".transfer-page", { timeout: 8000 });
  await mobilePage.page.waitForTimeout(500);
  const mobileInfo = await mobilePage.page.evaluate(() => {
    const root = document.querySelector(".transfer-page");
    const rect = root?.getBoundingClientRect();
    const actions = document.querySelector(".transfer-actions");
    return {
      columns: actions ? getComputedStyle(actions).gridTemplateColumns.split(" ").length : 0,
      right: Math.round(rect?.right ?? 0),
      viewport: window.innerWidth
    };
  });
  check("30 移动端传输页不超屏且四图标 2×2", mobileInfo.right <= mobileInfo.viewport + 1 && mobileInfo.columns === 2, J(mobileInfo));
  check("30 传输页无脚本报错", errors.length === 0 && mobilePage.errors.length === 0, errors[0] ?? mobilePage.errors[0] ?? "");
  await mobilePage.page.close();
}

// ===========================================================================
// 9 日记列表的滚动锚点：切视图复位 / 搜索回顶 / 日历点天定位
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(
    ({ diaryKey, stateKey, settingsKey }) => {
      const now = new Date().toISOString();
      const entries = [];
      for (let i = 0; i < 60; i += 1) {
        const d = new Date();
        d.setDate(d.getDate() - Math.floor(i / 2));
        const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        entries.push({
          id: `dy-${i}`,
          date,
          title: `第${i}篇`,
          markdown: `正文 ${i}\n${"这是一段比较长的正文，用来把卡片撑出高度差。".repeat(3)}`,
          mood: "",
          weather: "",
          tags: [],
          createdAt: now,
          updatedAt: now
        });
      }
      localStorage.setItem(diaryKey, JSON.stringify({ schemaVersion: 1, entries, meta: { revision: 1 } }));
      const settings = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
      settings.diary = { ...(settings.diary ?? {}), view: "list" };
      localStorage.setItem(settingsKey, JSON.stringify(settings));
    },
    { diaryKey: DIARY_KEY, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await page.locator(".nav-row", { hasText: "日记" }).click();
  await page.waitForSelector(".diary-card", { timeout: 10000 });
  await page.waitForTimeout(500);

  // 切到日历再切回列表：滚动复位
  await page.evaluate(() => {
    document.querySelector(".diary-scroll").scrollTop = 900;
  });
  await page.waitForTimeout(300);
  await page.locator(".diary-view-switch button[title='日历视图']").click();
  await page.waitForTimeout(400);
  await page.locator(".diary-view-switch button[title='列表视图']").click();
  await page.waitForTimeout(600);
  const afterSwitch = await page.evaluate(() => document.querySelector(".diary-scroll").scrollTop);
  check("9 切回列表滚动复位到顶", afterSwitch === 0, String(afterSwitch));

  // 滚到深处再搜索：命中项要在眼前（回顶）
  await page.evaluate(() => {
    document.querySelector(".diary-scroll").scrollTop = 1500;
  });
  await page.waitForTimeout(300);
  await page.locator(".diary-view .header-actions button[title='搜索日记']").click();
  await page.waitForSelector(".diary-search input", { timeout: 5000 });
  await page.locator(".diary-search input").fill("第55篇");
  await page.waitForTimeout(600);
  const search = await page.evaluate(() => {
    const scroller = document.querySelector(".diary-scroll");
    const rect = scroller.getBoundingClientRect();
    const visible = [...document.querySelectorAll(".diary-card")].filter((el) => {
      const box = el.getBoundingClientRect();
      return box.bottom > rect.top && box.top < rect.bottom;
    });
    return { scrollTop: Math.round(scroller.scrollTop), text: visible[0]?.textContent ?? "", cards: document.querySelectorAll(".diary-card").length };
  });
  check("9 搜索后回顶且第一张就是命中项", search.scrollTop === 0 && search.text.includes("第55篇"), J(search));
  await page.locator(".diary-view .header-actions button[title='关闭搜索']").click();
  await page.waitForTimeout(400);

  // 日历点某天 → 切列表定位到该日期组（第 40 篇所在的那天）
  const wantedDate = await page.evaluate(() => {
    const entries = JSON.parse(localStorage.getItem("todo-note-diary-v1")).entries;
    return entries[40].date;
  });
  await page.locator(".diary-view-switch button[title='日历视图']").click();
  await page.waitForTimeout(500);
  // 翻到那个月（日历默认停在今天所在月；第 40 篇在 20 天前，可能跨月）
  const day = Number(wantedDate.slice(8, 10));
  const monthLabel = await page.locator(".diary-calendar-bar strong").textContent();
  const wantedMonthLabel = `${Number(wantedDate.slice(5, 7))}月`;
  if (!(monthLabel ?? "").includes(wantedMonthLabel)) {
    await page.locator(".diary-calendar-bar button[aria-label='上个月']").click();
    await page.waitForTimeout(300);
  }
  await page
    .locator(`.diary-calendar-cell:not(.other-month):has(.diary-cell-day:text-is('${day}'))`)
    .first()
    .click();
  await page.waitForTimeout(400);
  await page.locator(".diary-view-switch button[title='列表视图']").click();
  await page.waitForTimeout(800);
  const jumped = await page.evaluate(() => {
    const scroller = document.querySelector(".diary-scroll");
    const rect = scroller.getBoundingClientRect();
    const visible = [...document.querySelectorAll(".diary-card")].filter((el) => {
      const box = el.getBoundingClientRect();
      return box.bottom > rect.top && box.top < rect.bottom;
    });
    return { scrollTop: Math.round(scroller.scrollTop), text: visible[0]?.textContent ?? "" };
  });
  check("9 日历点天后列表定位到那一天的组", jumped.scrollTop > 0 && /第4[01]篇/.test(jumped.text), J(jumped));
  check("9 日记锚点无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 11 图标模式拖动排序能落到任意位置（不只是前两位）
// ===========================================================================
for (const target of ["first-to-last", "middle-to-fourth"]) {
  const { page } = await freshPage(desktop);
  await page.evaluate(
    ({ key }) => {
      localStorage.setItem(
        key,
        JSON.stringify({
          schemaVersion: 3,
          appearance: {
            navItems: ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"],
            navLayout: "icons"
          }
        })
      );
    },
    { key: SETTINGS_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const box = (name) => page.locator(`.nav-row[title="${name}"]`).first().boundingBox();
  const order = () =>
    page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").appearance?.navItems ?? [], SETTINGS_KEY);

  if (target === "first-to-last") {
    const from = await box("我的一天");
    const last = await box("工具箱");
    await dragBy(page, centerOf(from), { x: last.x + last.width - 2, y: centerOf(last).y });
    await page.mouse.up();
    await page.waitForTimeout(600);
    const after = await order();
    check("11（拖到最右）图标模式能落到末位", after[after.length - 1] === "my-day", J(after));
  } else {
    const from = await box("定时任务");
    const fourth = await box("日记");
    await dragBy(page, centerOf(from), { x: fourth.x + 4, y: centerOf(fourth).y });
    const preview = await page.evaluate(() => [...document.querySelectorAll(".nav-row")].map((el) => el.getAttribute("title")));
    await page.mouse.up();
    await page.waitForTimeout(600);
    const after = await order();
    check("11（拖到第 4 位）拖动中行实时让位", preview[3] === "定时任务", J(preview));
    check("11（拖到第 4 位）松手后落到第 4 位", after[3] === "scheduled", J(after));
  }
  await page.close();
}

// ===========================================================================
// 29 分组树拖动：拖到分组头 = 移入；拖到间隙 = 排序（都带实时让位）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [
      { id: "cat-a", kind: "category", name: "分组甲", icon: "folder", parentId: null, collapsed: false },
      { id: "cat-b", kind: "category", name: "分组乙", icon: "folder", parentId: null, collapsed: false },
      { id: "entry-a", kind: "entry", name: "条目甲", icon: "list", parentId: "cat-a" }
    ],
    selected: "entry-a"
  });
  const treeState = () =>
    page.evaluate((stateKey) => {
      const state = JSON.parse(localStorage.getItem(stateKey) ?? "{}");
      return {
        nodes: state.nodes.map((node) => ({ id: node.id, parent: node.parentId, collapsed: Boolean(node.collapsed) })),
        entryParent: state.nodes.find((node) => node.id === "entry-a")?.parentId ?? null
      };
    }, STATE_KEY);

  // A. 拖条目 -> 分组乙的行中部 = 移入（子节点 +1）
  const entry = await page.locator('.tree-row[data-node-id="entry-a"]').boundingBox();
  const catB = await page.locator('.tree-row[data-node-id="cat-b"]').boundingBox();
  await dragBy(page, centerOf(entry), centerOf(catB));
  const hovering = await page.evaluate(() => ({
    inside: !!document.querySelector(".tree-row.drop-inside"),
    dragged: !!document.querySelector(".tree-row.dragging"),
    previewParent: (() => {
      const rows = [...document.querySelectorAll(".tree-row[data-node-id]")].map((el) => el.dataset.nodeId);
      return rows.indexOf("entry-a") > rows.indexOf("cat-b");
    })()
  }));
  check("29 悬停分组头显示「移入」反馈（虚线框）", hovering.inside && hovering.dragged, J(hovering));
  await page.mouse.up();
  await page.waitForTimeout(700);
  const afterInside = await treeState();
  check("29 松手后条目移入分组乙", afterInside.entryParent === "cat-b", J(afterInside));

  // B. 拖分组乙到分组甲的上沿 = 排序（不改变父子关系）
  const catB2 = await page.locator('.tree-row[data-node-id="cat-b"]').boundingBox();
  const catA = await page.locator('.tree-row[data-node-id="cat-a"]').boundingBox();
  await dragBy(page, centerOf(catB2), { x: centerOf(catA).x, y: catA.y + 3 });
  const gapHint = await page.evaluate(() => ({
    before: !!document.querySelector(".tree-row.drop-before"),
    inside: !!document.querySelector(".tree-row.drop-inside"),
    order: [...document.querySelectorAll(".tree-row[data-node-id]")].map((el) => el.dataset.nodeId)
  }));
  await page.mouse.up();
  await page.waitForTimeout(700);
  const afterSort = await treeState();
  const ids = afterSort.nodes.map((node) => node.id);
  check("29 间隙落点显示插入位（不是移入）", gapHint.before && !gapHint.inside, J(gapHint));
  check("29 松手后顺序变化（分组乙排到分组甲前）", ids.indexOf("cat-b") < ids.indexOf("cat-a"), J(ids));
  check("29 排序不改父子关系", afterSort.entryParent === "cat-b", J(afterSort));
  check("29 分组拖动无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 24 深层 counters 编号（2.1 / 2.1.1）在窄卡片 + 大字号下不被裁
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const nested = [
    "2. 第二章",
    "   1. 小节一",
    "      1. 更深一层",
    "         1. 第四层",
    "",
    "正文段落用来对比左右边界。"
  ].join("\n");
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "编号条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-nested", markdown: nested, expanded: true })],
    settings: { appearance: { uiFontSize: 22, markdownFontSize: 26 } }
  });
  await page.locator(".tree-row", { hasText: "编号条目" }).click();
  await page.waitForTimeout(1200);
  const card = page.locator(".task-card").first();
  const geo = await page.evaluate(() => {
    const body = document.querySelector(".task-card .markdown-body");
    const list = body?.querySelector("ol");
    const deepest = list ? [...list.querySelectorAll("li")].pop() : null;
    if (!body || !list || !deepest) return null;
    const bodyRect = body.getBoundingClientRect();
    const listRect = list.getBoundingClientRect();
    const liRect = deepest.getBoundingClientRect();
    return {
      bodyLeft: Math.round(bodyRect.left),
      listLeft: Math.round(listRect.left),
      liLeft: Math.round(liRect.left),
      overflow: Math.round(list.scrollWidth - list.clientWidth),
      bodyOverflow: Math.round(body.scrollWidth - body.clientWidth),
      content: getComputedStyle(deepest, "::before").content,
      listPadding: getComputedStyle(list).paddingLeft
    };
  });
  check("24 深层编号的 li 与列表左边界都在卡片内", geo !== null && geo.liLeft >= geo.bodyLeft - 1 && geo.listLeft >= geo.bodyLeft - 1, J(geo));
  check("24 列表区没有横向溢出（编号没把内容撑出去）", geo !== null && geo.overflow <= 0 && geo.bodyOverflow <= 0, J(geo));
  await page.screenshot({ path: "test-data/v085-counters-deep.png", clip: await card.boundingBox() });
  check("24 编号渲染无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 27 大字号下的日期面板（编辑器 22px 界面字号 + 最大编辑器字号）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "日期条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-date", markdown: "带日期的任务", dueDate: localToday() })],
    settings: {
      appearance: { uiFontSize: 22, editorFontSize: 26, markdownFontSize: 26 }
    }
  });
  await page.locator(".tree-row", { hasText: "日期条目" }).click();
  await page.waitForTimeout(800);
  await page.locator(".task-card .task-due-date").first().click();
  await page.waitForSelector(".date-reminder-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const panel = await page.evaluate(() => {
    const grid = document.querySelector(".date-reminder-panel .date-picker-grid") ?? document.querySelector(".date-picker-grid");
    const cell = grid?.querySelector(".dp-cell");
    // offsetWidth/Height 是布局像素（不受 shell 的 transform: scale(uiScale) 影响），
    // getBoundingClientRect 会乘上 0.75 的缩放——量尺寸一律用 offset*
    return {
      gridWidth: Math.round(grid?.offsetWidth ?? 0),
      cellWidth: Math.round(cell?.offsetWidth ?? 0),
      cellFont: cell ? getComputedStyle(cell).fontSize : "",
      overflow: grid ? Math.round(grid.scrollWidth - grid.clientWidth) : 0,
      cellText: cell?.textContent?.trim() ?? ""
    };
  });
  check("27 大字号下日历仍是 7 列定宽且不溢出", panel.gridWidth === 228 && panel.overflow <= 0, J(panel));
  check("27 格子放得下字号（格宽 ≥ 字号 ×1.4）", panel.cellWidth >= parseFloat(panel.cellFont || "0") * 1.4, J(panel));
  const panelBox = await page.locator(".date-reminder-panel").boundingBox();
  await page.screenshot({ path: "test-data/v085-date-panel-large-font.png", clip: panelBox ?? undefined });
  check("27 日期面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

await browser.close();
console.log(`\n${passes} passed, ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);

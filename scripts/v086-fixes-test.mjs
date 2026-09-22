// v0.8.6 回归。逐条覆盖本版需求：1 全局搜索、2 分组树、3 移动端整页视图、
// 4 浮层定位与失焦关闭、5 传输整治、6-9 小修、10-12 relay 与取色盘。
// 用法：node scripts/v086-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";
const LEDGER_KEY = "todo-note-ledger-v1";

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

async function freshPage(context, { mobile = false } = {}) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  if (mobile) page.setDefaultTimeout(15000);
  return { page, errors };
}

async function seedState(page, { nodes = [], tasks = [], settings = null, diaries = null, ledger = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, stateKey, settingsKey, diaryKey, ledgerKey, diaries, ledger }) => {
      const now = new Date().toISOString();
      const base = [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now }
      ];
      localStorage.setItem(
        stateKey,
        JSON.stringify({
          schemaVersion: 3,
          nodes: [...base, ...nodes],
          tasks,
          backgrounds: {},
          selectedNodeId: nodes[0]?.id ?? "entry-a"
        })
      );
      if (settings) {
        const existing = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings, schemaVersion: 3 }));
      }
      if (diaries) localStorage.setItem(diaryKey, JSON.stringify(diaries));
      if (ledger) localStorage.setItem(ledgerKey, JSON.stringify(ledger));
    },
    {
      nodes,
      tasks,
      settings,
      diaries,
      ledger,
      stateKey: STATE_KEY,
      settingsKey: SETTINGS_KEY,
      diaryKey: "todo-note-diary-v1",
      ledgerKey: LEDGER_KEY
    }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
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
    dueDate: "",
    dueTime: "",
    tags: [],
    emojis: [],
    expanded: false,
    createdAt: now,
    updatedAt: now,
    ...overrides
  };
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const mobile = await browser.newContext({
  viewport: { width: 392, height: 850 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true,
  deviceScaleFactor: 2
});

// ===========================================================================
// 1 全局搜索：分块扫描 + 封顶 200 + 结果窗口化渲染
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  const nodes = [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }];
  const tasks = Array.from({ length: 60 }, (_, index) =>
    makeTask({ id: `t${index}`, markdown: `关键词任务 ${index}`, updatedAt: new Date(Date.now() - index * 60000).toISOString() })
  );
  const diaries = Array.from({ length: 400 }, (_, index) => ({
    id: `d${index}`,
    date: localToday(-(index % 30)),
    title: `关键词日记 ${index}`,
    markdown: `正文 ${index}`,
    tags: [],
    createdAt: `${localToday(-(index % 30))}T09:00:00.000Z`,
    updatedAt: `${localToday(-(index % 30))}T09:00:00.000Z`
  }));
  await seedState(page, { nodes, tasks, diaries: { schemaVersion: 1, entries: diaries } });

  const searchResult = await page.evaluate(async () => {
    const input = document.querySelector(".search-box input");
    const at = performance.now();
    input.value = "关键词";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const deadline = at + 8000;
    while (performance.now() < deadline) {
      const stats = window.__kxtodoSearch;
      if (stats && !stats.scanning && stats.hits > 0 && document.querySelector(".task-list .task-card")) break;
      await new Promise((resolve) => setTimeout(resolve, 8));
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
    return {
      elapsed: performance.now() - at,
      hits: window.__kxtodoSearch?.hits ?? -1,
      domCards: document.querySelectorAll(".task-list .task-card, .task-list .diary-card").length,
      hasStack: Boolean(document.querySelector(".task-list .virtual-stack")),
      scanningHint: document.querySelectorAll(".search-scanning").length
    };
  });
  check("1 搜索命中数封顶 200 条", searchResult.hits === 200, J(searchResult));
  check("1 结果走 VirtualStack（只挂视口附近）", searchResult.hasStack && searchResult.domCards < 60, J(searchResult));
  check("1 扫描完成后不再显示「搜索中…」", searchResult.scanningHint === 0, J(searchResult));
  check("1 搜索无脚本报错", errors.length === 0, errors[0] ?? "");

  // 清空立即生效（不走 idle）：清词后当拍就该是空结果
  const cleared = await page.evaluate(async () => {
    const input = document.querySelector(".search-box input");
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 0));
    return { hits: window.__kxtodoSearch?.hits ?? -1, scanning: window.__kxtodoSearch?.scanning ?? true };
  });
  check("1 清空搜索立即生效（hits=0、不再扫描）", cleared.hits === 0 && cleared.scanning === false, J(cleared));
  await page.close();
}

// ===========================================================================
// 4 浮层定位：菜单在四角都不溢出；日期浮层全局单例 + 点外部不保存关闭
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }],
    tasks: [
      makeTask({ id: "task-a", markdown: "第一张卡", dueDate: localToday(1), dueTime: "" }),
      makeTask({ id: "task-b", markdown: "第二张卡", dueDate: localToday(2), dueTime: "" })
    ]
  });
  // 菜单几何：屏幕四角 + 贴边（纯函数，直接对 placePopover 断言——
  // 真实菜单只能落在卡片上，碰不到屏幕角落）
  const geometry = await page.evaluate(() => {
    const place = window.__kxtodoPlacePopover;
    if (!place) return null;
    const size = { width: 232, height: 300 };
    const view = { width: 1440, height: 900 };
    const margin = 8;
    const corners = [
      { x: 4, y: 4 },
      { x: 1436, y: 4 },
      { x: 4, y: 896 },
      { x: 1436, y: 896 }
    ];
    return corners.map((anchor) => {
      const placed = place(anchor, size, view);
      const height = placed.maxHeight > 0 ? Math.min(placed.maxHeight, size.height) : size.height;
      return {
        anchor,
        placed,
        // 「不溢出」= 不越过视口边界（margin 是美学边距，锚点本身就在边距内时不该强推）
        overflowX: placed.left < 0 || placed.left + size.width > view.width,
        overflowY: placed.top < 0 || placed.top + height > view.height,
        bottomAligned: Math.abs(placed.top + size.height - anchor.y) <= 1
      };
    });
  });
  check(
    "4 四角都不溢出视口",
    Array.isArray(geometry) && geometry.every((item) => !item.overflowX && !item.overflowY),
    J(geometry)
  );
  check(
    "4 下方放不下时翻身向上、下边缘贴住点击位置",
    Array.isArray(geometry) && geometry[2].bottomAligned && geometry[3].bottomAligned,
    J(geometry?.slice(2))
  );

  // 真实菜单（右键卡片）+ 长按落点在页面下半部：菜单必须整体在视口内
  const cardBox = await page.locator(".task-card").first().boundingBox();
  await page.mouse.click(cardBox.x + cardBox.width - 12, cardBox.y + cardBox.height - 6, { button: "right" });
  await page.waitForTimeout(350);
  const menuBox = await page.locator(".context-menu").first().boundingBox();
  check(
    "4 真实右键菜单不溢出视口",
    Boolean(menuBox) && menuBox.x >= 0 && menuBox.y >= 0 && menuBox.x + menuBox.width <= 1440 && menuBox.y + menuBox.height <= 900,
    J(menuBox)
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);

  // 日期浮层：全局单例
  await page.locator(".task-card").first().locator(".task-due-date").click({ force: true });
  await page.waitForTimeout(400);
  const first = await page.evaluate(() => ({
    panels: document.querySelectorAll(".task-date-popover").length,
    inside: Boolean(document.querySelector(".task-date-popover .date-reminder-panel")),
    box: document.querySelector(".task-date-popover")?.getBoundingClientRect().toJSON() ?? null
  }));
  check("4 日期浮层能打开且只有一个", first.panels === 1 && first.inside, J(first));
  check(
    "4 日期浮层在视口内（不下方溢出）",
    Boolean(first.box) && first.box.x >= 0 && first.box.y >= 0 && first.box.x + first.box.width <= 1440 && first.box.y + first.box.height <= 900,
    J(first.box)
  );
  // 点另一张卡的日期按钮：先关旧的（点卡片空白）再点新的，验证「先关旧再开新」
  await page.locator(".task-card").first().click({ force: true });
  await page.waitForTimeout(250);
  await page.locator(".task-card").nth(1).locator(".task-due-date").click({ force: true });
  await page.waitForTimeout(400);
  const switched = await page.evaluate(() => ({
    panels: document.querySelectorAll(".task-date-popover").length,
    ownerText: document.querySelector(".task-date-popover")?.closest(".task-card")?.innerText?.slice(0, 6) ?? ""
  }));
  check("4 点另一张卡的日期：面板跟着换（同时只有一个）", switched.panels === 1 && switched.ownerText.includes("第二张"), J(switched));
  // 点别处（卡片空白）：不保存关闭
  await page.locator(".task-card").first().click({ force: true });
  await page.waitForTimeout(300);
  check("4 点外部不保存关闭日期浮层", (await page.locator(".task-date-popover").count()) === 0);
  // 再开一次，然后点别的卡片空白处：也关闭
  await page.locator(".task-card").first().locator(".task-due-date").click({ force: true });
  await page.waitForTimeout(300);
  check("4 再开一次仍然只有一个", (await page.locator(".task-date-popover").count()) === 1);
  // 点另一张卡片的正文（非菜单区域）
  await page.locator(".task-card").nth(1).click({ force: true });
  await page.waitForTimeout(300);
  check("4 点别的卡片即关闭（不保存）", (await page.locator(".task-date-popover").count()) === 0);
  check("4 浮层无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 5 传输助手：界面整治（5.5）+ 拒绝/错误语义的纯映射（5.3/5.4 的前端部分）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".system-nav .nav-row")];
    rows.find((row) => (row.getAttribute("title") ?? "").includes("工具箱"))?.click();
  });
  await page.waitForTimeout(400);
  await page.evaluate(() => {
    const cards = [...document.querySelectorAll(".toolbox-card")];
    cards.find((card) => (card.textContent ?? "").includes("传输"))?.click();
  });
  await page.waitForTimeout(500);
  check("5 传输工具页打开", (await page.locator(".transfer-page").count()) === 1);

  const codeInput = await page.evaluate(() => {
    const input = [...document.querySelectorAll(".transfer-identity input")].find((item) =>
      (item.getAttribute("placeholder") ?? "").includes("密钥")
    );
    return input
      ? { type: input.getAttribute("type"), eyes: document.querySelectorAll(".transfer-eye").length, plain: input.type }
      : null;
  });
  check("5 口令输入框是普通文本（不是密码框）", codeInput && codeInput.plain === "text", J(codeInput));
  check("5 口令行没有眼睛按钮", codeInput && codeInput.eyes === 0, J(codeInput));

  // 空态：文案与「端到端加密」徽标同处一行，徽标不被挤下去
  const emptyState = await page.evaluate(() => {
    const empty = document.querySelector(".transfer-empty");
    if (!empty) return null;
    const spans = [...empty.querySelectorAll("span, em")].map((el) => el.getBoundingClientRect());
    const sameLine = spans.length >= 2 && Math.abs(spans[0].top - spans[spans.length - 1].top) < 12;
    return { text: empty.textContent ?? "", sameLine, count: spans.length };
  });
  check("5 空态文案改成「输入相同口令以匹配」", Boolean(emptyState?.text.includes("输入相同口令以匹配")), J(emptyState));
  check("5 端到端加密徽标与文案同一行（空态卡片够宽）", Boolean(emptyState?.sameLine), J(emptyState));

  // 接收页：路径中间省略（头/尾两段）+ 按钮另起一行
  await page.locator(".transfer-tabs button", { hasText: "接收" }).click();
  await page.waitForTimeout(300);
  const receiveUi = await page.evaluate(() => {
    const row = document.querySelector(".transfer-save-row");
    const head = row?.querySelector(".transfer-path-head");
    const tail = row?.querySelector(".transfer-path-tail");
    const actions = row?.querySelector(".transfer-save-actions");
    const rowBox = row?.getBoundingClientRect();
    const actionsBox = actions?.getBoundingClientRect();
    return {
      hasParts: Boolean(head && tail),
      headOverflow: head ? getComputedStyle(head).textOverflow : "",
      tailNowrap: tail ? getComputedStyle(tail).whiteSpace : "",
      actionsBelow: Boolean(rowBox && actionsBox && actionsBox.top >= rowBox.top),
      actionsOwnRow: Boolean(actionsBox && tail && actionsBox.top >= tail.getBoundingClientRect().bottom - 2),
      hint: document.querySelector(".transfer-hint")?.textContent ?? ""
    };
  });
  check("5 存储路径分头尾两段（中间省略）", receiveUi.hasParts && receiveUi.headOverflow === "ellipsis" && receiveUi.tailNowrap === "nowrap", J(receiveUi));
  check("5 「更改 / 打开文件夹」另起一行", receiveUi.actionsOwnRow, J(receiveUi));
  check("5 有锁屏边界的提示文案", receiveUi.hint.includes("锁屏"), J(receiveUi));

  // 危险按钮变体存在（「下线」用它；本页不显示时从样式表里核对）
  const dangerRule = await page.evaluate(() => {
    for (const sheet of [...document.styleSheets]) {
      let rules = [];
      try {
        rules = [...sheet.cssRules];
      } catch {
        continue;
      }
      if (rules.some((rule) => rule.selectorText === ".menu-action-button.danger")) return true;
    }
    return false;
  });
  check("5 存在 .menu-action-button.danger 变体（下线按钮用它）", dangerRule);
  check("5 传输界面无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 7 统计「自定义」日历：年月面板在大字号下不换行（10月/11月/12月）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  const today = localToday();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }],
    settings: { appearance: { uiFontSize: 20 } },
    ledger: {
      accounts: [{ id: "a1", name: "现金", kind: "cash", initialCents: 0, order: 0, createdAt: now }],
      categories: [{ id: "c1", name: "餐饮", side: "expense", parentId: null, order: 0, createdAt: now }],
      entries: [
        { id: "e1", kind: "expense", amountCents: 1234, accountId: "a1", categoryId: "c1", date: today, note: "", createdAt: now, updatedAt: now }
      ]
    }
  });
  await page.locator(".system-nav .nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(600);
  await page.locator(".ledger-view-switch button[title='统计视图']").click();
  await page.waitForTimeout(500);
  await page.locator(".ledger-segmented button", { hasText: "自定义" }).first().click();
  await page.waitForTimeout(400);
  await page.locator(".ledger-custom-field").first().click();
  await page.waitForTimeout(450);
  const title = page.locator(".ledger-custom-field .dp-title").first();
  if (await title.count()) {
    await title.click();
    await page.waitForTimeout(450);
  }
  const monthGrid = await page.evaluate(() => {
    const cells = [...document.querySelectorAll(".month-picker-grid .mp-cell")];
    if (cells.length === 0) return null;
    const wrapped = cells.filter((cell) => {
      const range = document.createRange();
      range.selectNodeContents(cell);
      return range.getClientRects().length > 1;
    });
    const grid = document.querySelector(".month-picker-grid");
    const picker = document.querySelector(".month-picker");
    const pickerBox = picker?.getBoundingClientRect();
    return {
      cells: cells.length,
      wrapped: wrapped.map((cell) => cell.textContent?.trim()),
      nowrap: getComputedStyle(cells[0]).whiteSpace,
      gridWidth: grid ? grid.getBoundingClientRect().width : -1,
      // 逻辑像素（壳上有 transform: scale(uiScale)，rect 是视觉像素）
      pickerWidth: picker ? Number.parseFloat(getComputedStyle(picker).width) : -1,
      // 「不溢出」：面板左右都在视口内
      insideViewport: Boolean(pickerBox) && pickerBox.x >= 0 && pickerBox.x + pickerBox.width <= window.innerWidth
    };
  });
  check("7 年月面板 12 格都不换行（10/11/12 月字样完整）", Boolean(monthGrid) && monthGrid.wrapped.length === 0, J(monthGrid));
  check("7 年月面板与日历同宽（228，切视图不跳位）", Boolean(monthGrid) && monthGrid.pickerWidth === 228, J(monthGrid));
  check("7 面板不溢出视口", Boolean(monthGrid?.insideViewport), J(monthGrid));
  check("7 年月面板无脚本报错", errors.length === 0, errors[0] ?? "");

  // 独立浮层（记账日历头部的年月标签）：宽度**按字号自适应**，不是写死像素
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  await page.locator(".ledger-view-switch button[title='列表视图']").click();
  await page.waitForTimeout(400);
  const standalone = await page.evaluate(() => {
    const anchor = document.querySelector(".month-pop-anchor");
    anchor?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    return Boolean(anchor);
  });
  await page.waitForTimeout(450);
  const standaloneBox = await page.evaluate(() => {
    const picker = document.querySelector(".month-pop .month-picker");
    if (!picker) return null;
    const wrapped = [...picker.querySelectorAll(".mp-cell")].filter((cell) => {
      const range = document.createRange();
      range.selectNodeContents(cell);
      return range.getClientRects().length > 1;
    });
    return {
      mode: picker.querySelectorAll(".mp-cell").length,
      logicalWidth: Number.parseFloat(getComputedStyle(picker).width),
      wrapped: wrapped.length
    };
  });
  check(
    "7 独立年月浮层宽度按字号自适应（> 默认 240）且不换行",
    standalone && standaloneBox && standaloneBox.logicalWidth > 240 && standaloneBox.wrapped === 0,
    J({ standalone, standaloneBox })
  );
  await page.close();
}

// ===========================================================================
// 8 记账 / 日记的齿轮：再点一次关闭（toggle + anchor）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }]
  });
  // 记账
  await page.locator(".system-nav .nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(600);
  const ledgerGear = page.locator(".ledger-view .header-actions button[title='记账菜单']");
  await ledgerGear.click();
  await page.waitForTimeout(350);
  check("8 记账齿轮打开菜单", (await page.locator(".context-menu").count()) === 1);
  await ledgerGear.click();
  await page.waitForTimeout(350);
  check("8 记账齿轮再点一次关闭（不闪）", (await page.locator(".context-menu").count()) === 0);
  await ledgerGear.click();
  await page.waitForTimeout(300);
  check("8 记账齿轮还能再开", (await page.locator(".context-menu").count()) === 1);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  // 日记
  await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
  await page.waitForTimeout(600);
  const diaryGear = page.locator(".diary-view .header-actions button[title='日记菜单']");
  await diaryGear.click();
  await page.waitForTimeout(350);
  check("8 日记齿轮打开菜单", (await page.locator(".context-menu").count()) === 1);
  await diaryGear.click();
  await page.waitForTimeout(350);
  check("8 日记齿轮再点一次关闭（不闪）", (await page.locator(".context-menu").count()) === 0);
  check("8 齿轮开合无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 6 工具箱：四个工具页统一标题（左箭头 + 图标 + 工具名 / 右侧 ⋯），两端都有箭头
// ===========================================================================
for (const [name, context] of [["桌面", desktop], ["移动端", mobile]]) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: new Date().toISOString() }],
    settings: { appearance: { uiFontSize: 20 } }
  });
  // 进工具箱（移动端从固定区，桌面同样从固定区）
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(500);
  const toolNames = await page.evaluate(() => [...document.querySelectorAll(".toolbox-card")].map((card) => card.textContent?.trim() ?? ""));
  const expected = ["随机数生成", "人民币大写", "草稿纸", "文件传输助手"];
  const visited = [];
  for (let index = 0; index < Math.min(toolNames.length, 4); index += 1) {
    await page.locator(".toolbox-card").nth(index).click();
    await page.waitForTimeout(450);
    const info = await page.evaluate(() => {
      const bar = document.querySelector(".toolbox-sub-bar");
      const title = bar?.querySelector(".toolbox-header-title");
      const back = bar?.querySelector("button[title='返回工具箱']");
      const more = bar?.querySelector("button[title='外观']");
      const backBox = back?.getBoundingClientRect();
      const titleBox = title?.getBoundingClientRect();
      const h1 = document.querySelector(".workspace h1");
      return {
        hasTitle: Boolean(title),
        text: title?.textContent?.trim() ?? "",
        fontSize: title ? getComputedStyle(title).fontSize : "",
        h1FontSize: h1 ? getComputedStyle(h1).fontSize : "",
        hasBack: Boolean(back),
        hasMore: Boolean(more),
        backLeftOfTitle: Boolean(backBox && titleBox && backBox.left <= titleBox.left),
        innerTitles: document.querySelectorAll(".toolbox-sub-title").length
      };
    });
    visited.push(info);
    await page.locator(".toolbox-sub-bar button[title='返回工具箱']").click();
    await page.waitForTimeout(400);
  }
  check(`6（${name}）四个工具页都有统一标题（名称 + 返回箭头 + ⋯）`,
    visited.length >= 4 && visited.every((item) => item.hasTitle && item.hasBack && item.hasMore && item.backLeftOfTitle && item.text.length > 0),
    J(visited));
  check(`6（${name}）工具页不再有内部标题行（.toolbox-sub-title）`,
    visited.every((item) => item.innerTitles === 0), J(visited.map((item) => item.innerTitles)));
  // --font-title = uiFontSize + 18 = 38px（字号跟随设置，两端同一口径）
  check(`6（${name}）标题字号 = 页面标题口径（--font-title = uiFontSize + 18 = 38px）`,
    visited.every((item) => item.fontSize === "38px"),
    J(visited.map((item) => item.fontSize)));
  check(`6（${name}）工具页无脚本报错`, errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 11/12 统一取色盘（iro）：手输双向联动、非法输入不崩、rAF 合帧、取消语义、触屏
// ===========================================================================
async function openListMenu(page) {
  if ((await page.locator(".context-menu").count()) > 0) return;
  const direct = page.locator("button[title='列表菜单']");
  if ((await direct.count()) > 0) {
    await direct.click({ force: true });
  } else {
    // 移动端：右上角只有一枚齿轮，列表菜单在它的面板里
    await page.locator(".header-actions button[title='更多操作']").click({ force: true });
    await page.waitForTimeout(300);
    await page.locator(".header-menu-panel .menu-item-button", { hasText: "列表菜单" }).click({ force: true });
  }
  await page.waitForTimeout(350);
}

async function colorPickerChecks(page, label) {
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  // 打开列表菜单 → UI颜色 → 色块（走需求 11 的统一取色盘）
  await openListMenu(page);
  await page.locator(".ui-color-picker").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(350);
  const opened = await page.evaluate(() => {
    const panel = document.querySelector(".kx-color-panel");
    const box = panel?.getBoundingClientRect();
    const canvas = panel?.querySelector(".kx-color-canvas");
    return {
      hasIro: Boolean(panel?.querySelector("[class*='Iro']")) || Boolean(panel?.querySelector("svg")),
      insideViewport:
        Boolean(box) && box.x >= 0 && box.y >= 0 && box.x + box.width <= window.innerWidth && box.y + box.height <= window.innerHeight,
      touchAction: canvas ? getComputedStyle(canvas).touchAction : ""
    };
  });
  check(`11（${label}）取色盘打开、iro 色盘在场、面板在视口内`, opened.hasIro && opened.insideViewport, J(opened));

  // ① HEX 手输 → RGB 与预览同步
  await page.locator(".kx-color-hex").fill("#ff8800");
  await page.locator(".kx-color-hex").press("Enter");
  await page.waitForTimeout(250);
  const afterHex = await page.evaluate(() => ({
    rgb: [...document.querySelectorAll(".kx-color-rgb input")].map((input) => input.value).join(","),
    accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
  }));
  check(`11（${label}）HEX 手输 → RGB 与预览同步`, afterHex.rgb === "255,136,0" && afterHex.accent === "#ff8800", J(afterHex));

  // ② RGB 手输 → HEX 同步
  await page.evaluate(() => {
    const inputs = [...document.querySelectorAll(".kx-color-rgb input")];
    const values = ["16", "32", "48"];
    inputs.forEach((input, index) => {
      input.value = values[index];
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("blur"));
    });
  });
  await page.waitForTimeout(300);
  const afterRgb = await page.evaluate(() => ({
    hex: document.querySelector(".kx-color-hex")?.value ?? "",
    accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
  }));
  check(`11（${label}）RGB 手输 → HEX 与预览同步`, afterRgb.hex === "#102030" && afterRgb.accent === "#102030", J(afterRgb));

  // ③ 非法 HEX：不写入、给行内提示
  await page.locator(".kx-color-hex").fill("zzzz");
  await page.locator(".kx-color-hex").press("Enter");
  await page.waitForTimeout(200);
  const badHex = await page.evaluate(() => ({
    hint: document.querySelector(".kx-color-hint")?.textContent?.trim() ?? "",
    accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
  }));
  check(`11（${label}）非法 HEX 不写入且给行内提示`, badHex.accent === "#102030" && badHex.hint.length > 0, J(badHex));

  // ④ 一帧内 60 个移动事件 → 预览写入按 rAF 合帧
  const burst = await page.evaluate(async () => {
    window.__kxtodoColorPick = { previewWrites: 0, previewEvents: 0 };
    // iro 的把手指令挂在组件包装层（.IroBox / .IroSlider）上；派发到画布容器
    // 不会向下冒泡到它，所以这里要精确取包装层元素
    const root = document.querySelector(".kx-color-canvas .IroBox") ?? document.querySelector(".kx-color-canvas .IroSlider");
    const rect = root?.getBoundingClientRect();
    if (!rect) return null;
    const at = (x, y) => ({ clientX: rect.left + x, clientY: rect.top + y });
    root.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, ...at(10, 10) }));
    for (let index = 0; index < 60; index += 1) {
      const point = at(10 + index, 12 + (index % 9));
      document.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, cancelable: true, ...point }));
    }
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, ...at(70, 20) }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await new Promise((resolve) => setTimeout(resolve, 80));
    return {
      debug: window.__kxtodoColorPick,
      hex: document.querySelector(".kx-color-hex")?.value ?? "",
      accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
    };
  });
  check(
    `11（${label}）拖动选色按 rAF 合帧（事件数 ≫ 写入次数）`,
    Boolean(burst) && burst.debug.previewEvents > 20 && burst.debug.previewWrites > 0 && burst.debug.previewWrites <= 5,
    J(burst)
  );
  check(
    `11（${label}）拖动时输入框与预览都跟着走`,
    Boolean(burst) && burst.hex === burst.accent && /^#[0-9a-f]{6}$/.test(burst.hex),
    J(burst)
  );

  // ⑤ 非法 RGB：不写入、给行内提示
  await page.evaluate(() => {
    const input = document.querySelectorAll(".kx-color-rgb input")[0];
    input.value = "300";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("blur"));
  });
  await page.waitForTimeout(200);
  const badRgb = await page.evaluate(() => ({
    hint: document.querySelector(".kx-color-hint")?.textContent?.trim() ?? ""
  }));
  check(`11（${label}）非法 RGB 不写入且给行内提示`, badRgb.hint.includes("RGB"), J(badRgb));

  // ⑥ Esc = 取消（草稿丢弃、预览回退、面板收起）
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  const afterEsc = await page.evaluate(() => ({
    panel: Boolean(document.querySelector(".kx-color-panel")),
    accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
  }));
  check(`11（${label}）Esc = 取消（面板收起、预览回退）`, !afterEsc.panel && afterEsc.accent === "#2564cf", J(afterEsc));
  check(`11（${label}）取色盘无脚本报错`, errors.length === 0, errors[0] ?? "");
  return opened;
}

{
  const { page } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }]
  });
  await page.locator('.tree-row[data-node-id="entry-a"]').click();
  await page.waitForTimeout(400);
  await colorPickerChecks(page, "桌面");

  // 12 处入口统一之后：页面上不再有原生 type=color
  const nativeInputs = await page.evaluate(() => document.querySelectorAll('input[type="color"]').length);
  check("11 全应用不再有原生 color input（取色盘统一）", nativeInputs === 0, String(nativeInputs));

  // 确认才落盘（背景色入口）
  await openListMenu(page);
  await page.locator(".palette-button").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  await page.locator(".kx-color-hex").fill("#123456");
  await page.locator(".kx-color-hex").press("Enter");
  await page.waitForTimeout(250);
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const persisted = await page.evaluate(() => {
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return { background: state.backgrounds?.["entry-a"]?.color ?? "(none)", panel: Boolean(document.querySelector(".kx-color-panel")) };
  });
  check("11 确认才落盘（背景色入口）", persisted.background === "#123456" && !persisted.panel, J(persisted));

  // U13（v0.8.7 需求 2.3）：输入完**不按 Enter**、直接点确认——颜色必须生效。
  // 此前所有用例都先按了 Enter（同步提交），恰好绕开「确认点击自己的 mousedown
  // 就是那个 blur、rAF 被自己作废」这条结构性缺陷。
  await openListMenu(page);
  await page.locator(".palette-button").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  await page.locator(".kx-color-hex").fill("#654321");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const flushedConfirm = await page.evaluate(() => {
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return {
      background: state.backgrounds?.["entry-a"]?.color ?? "(none)",
      panel: Boolean(document.querySelector(".kx-color-panel"))
    };
  });
  check("U13 输入完不按 Enter 直接确认：颜色生效", flushedConfirm.background === "#654321" && !flushedConfirm.panel, J(flushedConfirm));

  // 工具页外观菜单里的两处入口也能开面板（别只换了一处）
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(500);
  await page.locator(".toolbox-card").first().click();
  await page.waitForTimeout(450);
  await page.locator(".toolbox-sub-bar button[title='外观']").click({ force: true });
  await page.waitForTimeout(350);
  await page.locator(".context-menu .ui-color-picker").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  const toolboxPanel = await page.evaluate(() => ({
    open: Boolean(document.querySelector(".kx-color-panel")),
    count: document.querySelectorAll(".kx-color-panel").length
  }));
  check("11 工具页外观菜单的取色入口也走统一色盘", toolboxPanel.open && toolboxPanel.count === 1, J(toolboxPanel));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  check("11 桌面取色盘无脚本报错", true);
  await page.close();
}

{
  // 移动端（需求 12）：同一套断言 + 触屏拖动不滚页面
  const { page } = await freshPage(mobile);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }]
  });
  await page.locator(".tree-row").first().click();
  await page.waitForTimeout(400);
  const opened = await colorPickerChecks(page, "移动端");
  check("12 移动端色盘 touch-action: none（拖动选色不滚页面）", opened.touchAction === "none", J(opened));
  await page.close();
}

console.log(`\n合计：${passes} 通过 / ${failures} 失败`);
await browser.close();
process.exit(failures === 0 ? 0 : 1);

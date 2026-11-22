// v0.9.4 回归。逐条覆盖本版需求：排序按页独立 / 图标选择器分类间隔 / 习惯多次打卡
// 比例高亮与动效、默认图标、统计月历紧凑 / 标语栏（悬浮层定位、轮播、关闭、移动端只配置）/
// 定时任务去矩形 hover 与主题化开关 / 360 窄屏逐元素扫。
// 用法：node scripts/v094-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";
const HABIT_KEY = "todo-note-habits-v1";

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
  if (mobile) await page.setViewportSize({ width: 360, height: 800 });
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

async function seedState(page, { nodes = [], tasks = [], settings = null, habits = null, slogan = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, habits, slogan, stateKey, settingsKey, habitKey }) => {
      const now = new Date().toISOString();
      const base = [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now }
      ];
      const state = {
        schemaVersion: 3,
        nodes: [...base, ...nodes],
        tasks,
        backgrounds: {},
        selectedNodeId: nodes[0]?.id ?? "entry-a"
      };
      if (slogan) state.slogan = slogan;
      localStorage.setItem(stateKey, JSON.stringify(state));
      if (settings) {
        const existing = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings }));
      }
      if (habits) localStorage.setItem(habitKey, JSON.stringify(habits));
    },
    { nodes, tasks, settings, habits, slogan, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY, habitKey: HABIT_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

function entryNode(id, name, order = 1) {
  return { id, kind: "entry", name, icon: "notebook", parentId: null, order, createdAt: new Date().toISOString() };
}

function task(id, nodeId, name, createdAt) {
  return {
    id, nodeId, markdown: name, completed: false, important: false, myDay: true,
    tags: [], emojis: [], expanded: false, createdAt
  };
}

function habitSeed(id, name, overrides = {}) {
  return {
    id, name, color: "#2f9e6e", icon: "",
    freq: { kind: "daily", weekdays: [] }, time: "", reminders: [],
    status: "active", order: 1, targetCount: 1, checkins: [],
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), ...overrides
  };
}

function sloganSeed(overrides = {}) {
  return {
    items: [
      { id: "sl-1", text: "多喝水，少久坐", icon: "", createdAt: new Date().toISOString() },
      { id: "sl-2", text: "今日事今日毕", icon: "", createdAt: new Date().toISOString() }
    ],
    activeIds: ["sl-1", "sl-2"],
    accent: "",
    textColor: "",
    opacity: 0.94,
    intervalSeconds: 8,
    position: "top-right",
    enabled: true,
    updatedAt: new Date().toISOString(),
    ...overrides
  };
}

const titles = (page) => page.locator(".workspace .task-card .markdown-title-row").allTextContents();

async function listMenu(page) {
  const direct = page.locator("button[title='列表菜单']");
  if (await direct.isVisible()) await direct.click();
  else {
    await page.locator(".workspace .header-actions button[title='更多操作']").click();
    await page.locator(".header-menu-panel .menu-item-button", { hasText: "列表菜单" }).click();
  }
  await page.locator(".context-menu").waitFor({ state: "visible" });
}

async function pickSort(page, label) {
  await listMenu(page);
  await page.getByRole("button", { name: "排序方式" }).click();
  await page.getByRole("button", { name: label, exact: true }).click();
  await page.waitForTimeout(400);
}

async function openHabitTool(page, mobile = false) {
  const nav = mobile ? ".mobile-nav .nav-row, .system-nav .nav-row" : ".system-nav .nav-row";
  await page.locator(nav, { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "习惯打卡" }).click();
  await page.waitForSelector(".habit-tool", { timeout: 5000 });
}

async function openSloganTool(page, mobile = false) {
  const nav = mobile ? ".mobile-nav .nav-row, .system-nav .nav-row" : ".system-nav .nav-row";
  await page.locator(nav, { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "标语栏" }).click();
  await page.waitForSelector(".slogan-tool", { timeout: 5000 });
}

async function sweep(page, label, selector) {
  const bad = await page.evaluate((sel) => {
    const out = [];
    const vw = document.documentElement.clientWidth;
    for (const el of document.querySelectorAll(sel)) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      if (rect.left < -1 || rect.right > vw + 1) out.push(`${el.className}`.slice(0, 60));
    }
    return out;
  }, selector);
  check(label, bad.length === 0, J(bad.slice(0, 6)));
}

// ---------------------------------------------------------------------------
// 1. 排序方式按页独立
// ---------------------------------------------------------------------------
async function section1SortPerPage(context) {
  const { page, errors } = await freshPage(context);
  const t = (i) => new Date(Date.now() - i * 60000).toISOString();
  await seedState(page, {
    nodes: [entryNode("entry-a", "甲列表"), entryNode("entry-b", "乙列表", 2)],
    tasks: [
      task("a1", "entry-a", "C 任务", t(3)), task("a2", "entry-a", "A 任务", t(2)), task("a3", "entry-a", "B 任务", t(1)),
      task("b1", "entry-b", "Z 任务", t(3)), task("b2", "entry-b", "M 任务", t(2)), task("b3", "entry-b", "A 任务", t(0))
    ]
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  // 默认 created-desc：新建在前
  check("1.1 甲页默认创建降序", J((await titles(page)).map((s) => s.trim())) === J(["B 任务", "A 任务", "C 任务"]), J(await titles(page)));
  await pickSort(page, "字母顺序 A → Z");
  check("1.2 甲页改字母升序", J((await titles(page)).map((s) => s.trim())) === J(["A 任务", "B 任务", "C 任务"]), J(await titles(page)));
  // 切到乙页：不受甲页影响，仍默认创建降序
  await page.locator(".tree-row", { hasText: "乙列表" }).click();
  await page.waitForTimeout(400);
  check("1.3 乙页不受甲页排序影响", J((await titles(page)).map((s) => s.trim())) === J(["A 任务", "M 任务", "Z 任务"]), J(await titles(page)));
  // 我的一天：也默认
  await page.locator(".system-nav .nav-row", { hasText: "我的一天" }).first().click();
  await page.waitForTimeout(400);
  const myday = (await titles(page)).map((s) => s.trim());
  check("1.4 我的一天默认创建降序（新建在前）", myday[0] === "A 任务", J(myday));
  // 回甲页：排序还在
  await page.locator(".tree-row", { hasText: "甲列表" }).click();
  await page.waitForTimeout(400);
  check("1.5 回甲页排序保留", J((await titles(page)).map((s) => s.trim())) === J(["A 任务", "B 任务", "C 任务"]), J(await titles(page)));
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), SETTINGS_KEY);
  check("1.6 落盘按页键", stored.appearance.listSortModes["entry-a"] === "alpha-asc" && stored.appearance.listSortModes["entry-b"] === undefined, J(stored.appearance.listSortModes));
  check("1.7 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 2. 图标选择器分类间隔
// ---------------------------------------------------------------------------
async function section2IconPickerGap(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], habits: { habits: [habitSeed("habit-a", "刷牙")] } });
  await openHabitTool(page);
  await page.locator(".habit-card").first().click({ button: "right" });
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  await page.waitForSelector(".habit-sheet", { timeout: 4000 });
  await page.locator(".habit-sheet .cal-field", { hasText: "图标" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".icon-grid", { timeout: 4000 });
  const gapOf = () => page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".icon-grid button")];
    if (buttons.length < 9) return null;
    const first = buttons[0].getBoundingClientRect();
    // 第二行第一个：top 明显大于第一行
    const secondRow = buttons.find((b) => b.getBoundingClientRect().top > first.bottom + 1);
    if (!secondRow) return null;
    return secondRow.getBoundingClientRect().top - first.bottom;
  });
  const allGap = await gapOf();
  await page.locator(".icon-group-chip", { hasText: "娱乐" }).click();
  await page.waitForTimeout(300);
  const entGap = await gapOf();
  check("2.1 全部分类行距正常（≤12px）", allGap !== null && allGap <= 12, J(allGap));
  check("2.2 娱乐分类行距不再被拉伸（≤12px）", entGap !== null && entGap <= 12, J(entGap));
  check("2.3 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 3. 习惯多次打卡：比例高亮 / 动效 / 默认图标 / 月历紧凑
// ---------------------------------------------------------------------------
async function section3HabitMulti(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    habits: { habits: [habitSeed("habit-a", "喝水", { targetCount: 3 }), habitSeed("habit-b", "刷牙", { order: 2 })] }
  });
  await openHabitTool(page);
  // 默认图标不再是 Ellipsis（CalendarCheck 不在白名单的老 bug）
  const iconClass = await page.locator(".habit-card-icon svg").first().getAttribute("class");
  check("3.1 默认图标渲染为日历类（非 Ellipsis 回退）", /calendar/.test(iconClass ?? ""), iconClass);
  await page.locator(".habit-view-switch button", { hasText: "每日" }).click();
  await page.waitForTimeout(300);
  const dot = page.locator(".habit-dot").first();
  await dot.click();
  await page.waitForTimeout(350);
  check("3.2 第一次打卡 = 部分完成（1/3）", (await page.locator(".habit-dot.partial").count()) === 1 && (await page.locator(".habit-dot-count").first().innerText()) === "1/3");
  const fill1 = await page.locator(".habit-dot.partial .habit-dot-circle").first().evaluate((el) => getComputedStyle(el).getPropertyValue("--habit-fill").trim());
  check("3.3 比例环角度 = 120deg", fill1 === "120deg", fill1);
  await dot.click();
  await page.waitForTimeout(350);
  check("3.4 第二次 = 2/3 仍部分", (await page.locator(".habit-dot-count").first().innerText()) === "2/3" && (await page.locator(".habit-dot.partial").count()) === 1);
  await dot.click();
  await page.waitForTimeout(350);
  check("3.5 打满 = checked 态", (await page.locator(".habit-dot.checked").count()) === 1 && (await page.locator(".habit-dot.partial").count()) === 0);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), HABIT_KEY);
  const today = localToday();
  check("3.6 落盘同日期三条（次数 = 出现次数）", stored.habits[0].checkins.filter((d) => d === today).length === 3, J(stored.habits[0].checkins));
  await dot.click();
  await page.waitForTimeout(350);
  check("3.7 打满后再点 = 清空当天", (await page.locator(".habit-dot.checked").count()) === 0 && (await page.locator(".habit-dot.partial").count()) === 0);
  // 统计月历紧凑：圆直径 ≤ 32
  await page.locator(".habit-view-switch button", { hasText: "全部" }).click();
  await page.waitForTimeout(300);
  await page.locator(".habit-card").first().click();
  await page.waitForSelector(".habit-heat-grid", { timeout: 4000 });
  const cellSize = await page.locator(".habit-heat-cell:not(.blank)").first().evaluate((el) => el.offsetWidth);
  check("3.8 月历圆收紧凑（≤32px）", cellSize <= 32, J(cellSize));
  await page.locator(".habit-sheet .editor-icon-button[title='关闭']").click();
  // 编辑器每日次数步进器
  await page.locator(".habit-card").first().click({ button: "right" });
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  await page.waitForSelector(".habit-sheet", { timeout: 4000 });
  const targetInput = page.locator(".habit-target-control input");
  check("3.9 编辑器有每日次数步进器（值 3）", (await targetInput.inputValue()) === "3");
  await targetInput.fill("5");
  await targetInput.press("Enter");
  await page.locator(".habit-sheet .calendar-sheet-foot .settings-button.primary").click();
  await page.waitForTimeout(500);
  const stored2 = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), HABIT_KEY);
  check("3.10 改目标次数落盘 5", stored2.habits[0].targetCount === 5, J(stored2.habits[0].targetCount));
  check("3.11 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 标语栏：悬浮层 / 轮播 / 关闭 / 位置 / 工具页维护
// ---------------------------------------------------------------------------
async function section4SloganDesktop(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [task("a1", "entry-a", "一件事", new Date().toISOString())], slogan: sloganSeed() });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const geo = await page.evaluate(() => {
    const banner = document.querySelector(".slogan-banner").getBoundingClientRect();
    const header = document.querySelector(".workspace .list-header").getBoundingClientRect();
    const shell = document.querySelector(".app-shell");
    const shellRect = shell.getBoundingClientRect();
    const scale = shellRect.width / shell.offsetWidth;
    return {
      rightDelta: Math.abs(banner.right - header.right) / scale,
      above: banner.bottom <= header.top + 1,
      text: document.querySelector(".slogan-banner-text").textContent
    };
  });
  // v0.9.5：左右边界跟卡片区对齐（不再固定距壳右缘）
  check("4.1 右边缘与内容区对齐（≤2px）", geo.rightDelta <= 2, J(geo.rightDelta));
  check("4.2 悬浮在标题上方（不挤占排版）", geo.above, J(geo));
  check("4.3 展示第一条选中标语", geo.text === "多喝水，少久坐", geo.text);
  // v0.9.5：多条轮播不再显示进度条
  check("4.4 轮播无进度条", (await page.locator(".slogan-banner-progress").count()) === 0 && (await page.locator(".slogan-layer").count()) === 1);
  // 轮播：8s 后换第二条
  await page.waitForTimeout(8600);
  const text2 = await page.locator(".slogan-banner-text").innerText();
  check("4.5 轮播切换到第二条", text2 === "今日事今日毕", text2);
  // 位置切到「底部居中」（v0.9.5 六位置网格）
  await openSloganTool(page);
  await page.locator(".slogan-position-grid button", { hasText: "底部居中" }).click();
  await page.waitForTimeout(500);
  await page.locator(".tree-row", { hasText: "记事" }).click();
  await page.waitForTimeout(600);
  const geo2 = await page.evaluate(() => {
    const banner = document.querySelector(".slogan-banner");
    const bar = document.querySelector(".add-task-bar");
    if (!banner || !bar) return null;
    return banner.getBoundingClientRect().top >= bar.getBoundingClientRect().bottom - 1;
  });
  check("4.6 底部位置悬浮在内容区底部（输入框之下）", geo2 === true, J(geo2));
  // v0.9.5：胶囊上不再有 X；点横幅本体直接进工具页
  check("4.7 不再显示关闭叉号", (await page.locator(".slogan-banner-x").count()) === 0);
  await page.locator(".slogan-banner").click();
  await page.waitForSelector(".slogan-tool", { timeout: 5000 });
  check("4.8 点横幅跳转标语工具页", (await page.locator(".slogan-tool").count()) === 1);
  // 工具页：列表 / 添加 / 勾选
  check("4.9 工具页两条标语", (await page.locator(".slogan-row").count()) === 2);
  await page.locator(".slogan-add-input").fill("第三条标语");
  await page.locator(".slogan-add .settings-button.primary").click();
  await page.waitForTimeout(400);
  check("4.10 添加后三条且自动选中", (await page.locator(".slogan-row").count()) === 3 && (await page.locator(".slogan-row.active").count()) === 3);
  // 工具页总开关关闭 → 回工作区横幅消失；再开回来
  await page.locator(".slogan-panel .toggle-switch").click();
  await page.waitForTimeout(400);
  await page.locator(".tree-row", { hasText: "记事" }).click();
  await page.waitForTimeout(600);
  check("4.11 总开关关闭后悬浮层消失", (await page.locator(".slogan-banner").count()) === 0);
  await openSloganTool(page);
  await page.locator(".slogan-panel .toggle-switch").click();
  await page.waitForTimeout(400);
  await page.locator(".tree-row", { hasText: "记事" }).click();
  await page.waitForTimeout(600);
  check("4.12 总开关重开悬浮层回归", (await page.locator(".slogan-banner").count()) === 1);
  check("4.13 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 定时任务：去矩形 hover + 主题化开关
// ---------------------------------------------------------------------------
async function section5SchedulerPolish(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await page.locator(".system-nav .nav-row", { hasText: "定时任务" }).first().click();
  await page.waitForSelector(".scheduler-panel", { timeout: 5000 });
  await page.locator(".scheduler-floating-add").click();
  await page.waitForSelector(".scheduler-editor", { timeout: 5000 });
  // 标题行 hover 不再是灰矩形
  await page.locator(".scheduled-title-row").hover();
  await page.waitForTimeout(200);
  const titleBg = await page.locator(".scheduled-title-row").evaluate((el) => getComputedStyle(el).backgroundColor);
  check("5.1 标题行 hover 无矩形底色", titleBg === "rgba(0, 0, 0, 0)" || titleBg === "transparent", titleBg);
  await page.locator(".scheduler-step-heading").first().hover();
  await page.waitForTimeout(200);
  const stepBg = await page.locator(".scheduler-step-heading").first().evaluate((el) => getComputedStyle(el).backgroundColor);
  check("5.2 步骤头 hover 无矩形底色", stepBg === "rgba(0, 0, 0, 0)" || stepBg === "transparent", stepBg);
  //  footer 启用是主题开关（role=switch）
  check("5.3 启用改主题开关", (await page.locator(".scheduler-footer-enabled .toggle-switch[role='switch']").count()) === 1);
  // 通知跟进里的原生 checkbox 画成拨动开关（宽 36）
  await page.locator(".scheduler-sentence button").nth(1).click();
  await page.waitForTimeout(300);
  await page.locator(".scheduler-disclosure > summary", { hasText: "参数、工作目录、超时与通知" }).click();
  await page.waitForTimeout(300);
  const boxWidth = await page.locator(".notification-followups > .checkbox-line input").first().evaluate((el) => el.offsetWidth);
  check("5.4 勾选行开关化（宽 36）", Math.abs(boxWidth - 36) <= 1, J(boxWidth));
  // 折叠 summary 是柔底圆角行
  const summaryRadius = await page.locator(".scheduler-disclosure > summary").first().evaluate((el) => getComputedStyle(el).borderRadius);
  check("5.5 折叠行圆角主题化", summaryRadius !== "0px", summaryRadius);
  check("5.6 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 移动端 360：标语只配置不展示 + 习惯编辑器新行不溢出
// ---------------------------------------------------------------------------
async function section6Mobile(context) {
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
  const ctx = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(ctx, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    habits: { habits: [habitSeed("habit-a", "喝水", { targetCount: 3 })] },
    slogan: sloganSeed()
  });
  check("6.1 移动端不展示悬浮标语", (await page.locator(".slogan-banner").count()) === 0);
  await openSloganTool(page, true);
  check("6.2 移动端工具页可维护标语", (await page.locator(".slogan-row").count()) === 2);
  await sweep(page, "6.3 标语工具页 360 无溢出", ".slogan-tool *");
  await page.locator(".slogan-panel .toggle-switch").click();
  await page.waitForTimeout(300);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("6.4 移动端开关落盘", stored.slogan.enabled === false, J(stored.slogan.enabled));
  // 回工具箱列表再进习惯打卡（移动端子页里侧栏导航被整页覆盖挡住）
  await page.locator(".toolbox-sub-bar button[title='返回工具箱']").click();
  await page.waitForTimeout(400);
  await page.locator(".toolbox-card", { hasText: "习惯打卡" }).click();
  await page.waitForSelector(".habit-tool", { timeout: 5000 });
  await page.locator(".habit-view-switch button", { hasText: "每日" }).click();
  await page.waitForTimeout(300);
  await page.locator(".habit-dot").first().click();
  await page.waitForTimeout(350);
  check("6.5 移动端多次打卡部分态", (await page.locator(".habit-dot.partial").count()) === 1);
  await sweep(page, "6.6 每日视图 360 无溢出", ".habit-tool *");
  await page.locator(".habit-dot").first().click({ button: "right" });
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  await page.waitForSelector(".habit-sheet", { timeout: 4000 });
  await sweep(page, "6.7 习惯编辑器（含每日次数行）360 无溢出", ".habit-sheet *");
  check("6.8 无页面错误", errors.length === 0, J(errors));
  await browser.close();
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
try {
  await section1SortPerPage(context);
  await section2IconPickerGap(context);
  await section3HabitMulti(context);
  await section4SloganDesktop(context);
  await section5SchedulerPolish(context);
  await section6Mobile(context);
} finally {
  await browser.close();
}
console.log(`\n${passes} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
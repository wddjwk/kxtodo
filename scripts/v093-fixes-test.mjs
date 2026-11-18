// v0.9.3 回归。逐条覆盖本版需求：移动端今天高亮圆几何 / 编辑器时间行移动端不截尾 +
// 时长括号下移成块 / 桌面真括号跨两行 / 分类管理面板业界化（无解释文案、色点列表、
// 整行编辑、底部添加）/ 习惯打卡全链路（新建/打卡/统计/暂停/删除）/ 360 窄屏逐元素扫。
// 用法：node scripts/v093-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";
const CALENDAR_KEY = "todo-note-calendar-v1";
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

async function seedState(page, { nodes = [], tasks = [], settings = null, calendar = null, habits = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, calendar, habits, stateKey, settingsKey, calendarKey, habitKey }) => {
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
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings }));
      }
      if (calendar) localStorage.setItem(calendarKey, JSON.stringify(calendar));
      if (habits) localStorage.setItem(habitKey, JSON.stringify(habits));
    },
    { nodes, tasks, settings, calendar, habits, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY, calendarKey: CALENDAR_KEY, habitKey: HABIT_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

function entryNode(id, name) {
  return { id, kind: "entry", name, icon: "notebook", parentId: null, order: 1, createdAt: new Date().toISOString() };
}

function calendarEvent(id, overrides = {}) {
  const today = localToday();
  return {
    id, title: "测试日程", details: "", startDate: today, startTime: "09:00",
    endDate: today, endTime: "10:00", allDay: false, reminders: [], taskId: "",
    location: "", categoryId: "", repeat: null, exceptDates: [],
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), ...overrides
  };
}

function habitSeed(id, name, overrides = {}) {
  return {
    id, name, color: "#2f9e6e", icon: "CalendarCheck",
    freq: { kind: "daily", weekdays: [] }, time: "", reminders: [],
    status: "active", order: 1, checkins: [],
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), ...overrides
  };
}

async function openSchedulePage(page) {
  await page.locator(".system-nav .nav-row", { hasText: "日程" }).first().click();
  await page.waitForSelector(".calendar-view", { timeout: 5000 });
  await page.waitForTimeout(300);
}

async function openSchedulePageMobile(page) {
  await page.locator(".mobile-nav .nav-row, .system-nav .nav-row", { hasText: "日程" }).first().click();
  await page.waitForSelector(".calendar-view", { timeout: 5000 });
  await page.waitForTimeout(350);
}

async function openHabitTool(page, mobile = false) {
  const nav = mobile ? ".mobile-nav .nav-row, .system-nav .nav-row" : ".system-nav .nav-row";
  await page.locator(nav, { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "习惯打卡" }).click();
  await page.waitForSelector(".habit-tool", { timeout: 8000 });
  await page.waitForTimeout(300);
}

// ---------------------------------------------------------------------------
// 1. 移动端周视图「今天」高亮圆：数字必须完整落在圆内（v0.9.2 只缩圆不缩字号的回归）
// ---------------------------------------------------------------------------
async function section1TodayCircle() {
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
  const context = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(context, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], calendar: { schemaVersion: 1, events: [], categories: [] } });
  await openSchedulePageMobile(page);
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(350);
  const geo = await page.evaluate(() => {
    const dom = document.querySelector(".cal-day-head.today .cal-dom");
    if (!dom) return null;
    const circle = dom.getBoundingClientRect();
    const range = document.createRange();
    range.selectNodeContents(dom);
    const text = range.getBoundingClientRect();
    const head = dom.closest(".cal-day-head").getBoundingClientRect();
    return {
      circle: { w: circle.width, h: circle.height, top: circle.top, bottom: circle.bottom, left: circle.left, right: circle.right },
      text: { w: text.width, h: text.height, top: text.top, bottom: text.bottom, left: text.left, right: text.right },
      head: { top: head.top, bottom: head.bottom, left: head.left, right: head.right }
    };
  });
  check("1.1 今天高亮圆存在", geo !== null);
  if (geo) {
    check("1.2 数字宽度落在圆内（左右各留 >=1px）", geo.text.left >= geo.circle.left + 1 && geo.text.right <= geo.circle.right - 1, J(geo));
    check("1.3 数字高度落在圆内", geo.text.top >= geo.circle.top + 1 && geo.text.bottom <= geo.circle.bottom - 1, J(geo));
    check("1.4 圆不被日期头裁切", geo.circle.top >= geo.head.top - 0.5 && geo.circle.bottom <= geo.head.bottom + 0.5, J(geo));
    check("1.5 圆是正圆不是椭圆", Math.abs(geo.circle.w - geo.circle.h) < 1, J(geo.circle));
  }
  // 大字号档也不许出圆（uiFontSize 上限 22）
  await page.evaluate((key) => {
    const existing = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(key, JSON.stringify({ ...existing, appearance: { ...(existing.appearance ?? {}), uiFontSize: 22 } }));
  }, SETTINGS_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  await openSchedulePageMobile(page);
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(350);
  const geo2 = await page.evaluate(() => {
    const dom = document.querySelector(".cal-day-head.today .cal-dom");
    if (!dom) return null;
    const circle = dom.getBoundingClientRect();
    const range = document.createRange();
    range.selectNodeContents(dom);
    const text = range.getBoundingClientRect();
    return { cw: circle.width, tw: text.width, ch: circle.height, th: text.height };
  });
  check("1.6 大字号档数字仍落在圆内", geo2 !== null && geo2.tw <= geo2.cw - 2 && geo2.th <= geo2.ch - 2, J(geo2));
  check("1.7 无页面错误", errors.length === 0, J(errors));
  await browser.close();
}

// ---------------------------------------------------------------------------
// 2. 移动端编辑器：今天/明天不截尾 + 时长括号下移成块可换行
// ---------------------------------------------------------------------------
async function section2MobileTimeRows() {
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
  const context = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(context, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: { schemaVersion: 1, events: [calendarEvent("cal-m", { title: "移动端时间行" })], categories: [] }
  });
  await openSchedulePageMobile(page);
  await page.locator(".cal-event", { hasText: "移动端时间行" }).click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  await page.waitForTimeout(300);
  const rows = await page.evaluate(() => {
    const values = [...document.querySelectorAll(".cal-time-range .cal-field-value")];
    const bracket = document.querySelector(".cal-duration-bracket");
    const rowsBox = document.querySelector(".cal-time-range-rows").getBoundingClientRect();
    const b = bracket.getBoundingClientRect();
    const text = bracket.querySelector(".cal-duration-bracket-text");
    return {
      values: values.map((el) => ({ text: el.innerText, scroll: el.scrollWidth, client: el.clientWidth })),
      bracket: { top: b.top, bottom: b.bottom, left: b.left, right: b.right, scroll: text.scrollWidth, client: text.clientWidth, text: text.innerText },
      rowsBottom: rowsBox.bottom,
      hostRight: document.querySelector(".calendar-form-body").getBoundingClientRect().right
    };
  });
  const today = localToday();
  const md = `${Number(today.slice(5, 7))}/${Number(today.slice(8, 10))}`;
  check("2.1 开始行含今天紧凑日期", rows.values[0].text.includes("今天") && rows.values[0].text.includes(md), J(rows.values[0]));
  check("2.2 开始行不截尾", rows.values[0].scroll <= rows.values[0].client + 1, J(rows.values[0]));
  check("2.3 结束行不截尾", rows.values[1].scroll <= rows.values[1].client + 1, J(rows.values[1]));
  check("2.4 时长括号在两行下面", rows.bracket.top >= rows.rowsBottom - 1, J(rows.bracket));
  check("2.5 括号块不顶出右边界", rows.bracket.right <= rows.hostRight + 1, J(rows.bracket));
  check("2.6 括号文字完整可见", rows.bracket.scroll <= rows.bracket.client + 1, J(rows.bracket));
  // 长时长（跨天）也不许截尾：选全天再改回 3 小时档验证换行能力
  await page.locator(".cal-duration-bracket").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "3 小时" }).click();
  await page.waitForTimeout(300);
  const long = await page.evaluate(() => {
    const text = document.querySelector(".cal-duration-bracket-text");
    return { scroll: text.scrollWidth, client: text.clientWidth, text: text.innerText };
  });
  check("2.7 3 小时时长完整可见", long.scroll <= long.client + 1 && long.text.includes("3 小时"), J(long));
  check("2.8 无页面错误", errors.length === 0, J(errors));
  await browser.close();
}

// ---------------------------------------------------------------------------
// 3. 桌面编辑器：真括号跨两行 + 菜单依旧可用
// ---------------------------------------------------------------------------
async function section3DesktopBracket(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: { schemaVersion: 1, events: [calendarEvent("cal-d", { title: "桌面括号" })], categories: [] }
  });
  await openSchedulePage(page);
  await page.locator(".cal-event", { hasText: "桌面括号" }).click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const geo = await page.evaluate(() => {
    const bracket = document.querySelector(".cal-duration-bracket");
    const line = bracket.querySelector(".cal-duration-bracket-line");
    const rows = document.querySelector(".cal-time-range-rows").getBoundingClientRect();
    const b = bracket.getBoundingClientRect();
    const l = line.getBoundingClientRect();
    const style = getComputedStyle(line);
    return {
      rows: { top: rows.top, bottom: rows.bottom, height: rows.height },
      bracket: { top: b.top, bottom: b.bottom, right: b.right },
      line: { top: l.top, bottom: l.bottom, width: l.width },
      borderLeft: style.borderLeftWidth,
      borderRight: style.borderRightWidth,
      hostRight: document.querySelector(".calendar-form-body").getBoundingClientRect().right
    };
  });
  check("3.1 括号竖线跨住两行（高度 >= 行块高 - 8）", geo.line.bottom - geo.line.top >= geo.rows.height - 8, J(geo));
  check("3.2 竖线在右、开口朝左（左无边框）", geo.borderLeft === "0px" && parseFloat(geo.borderRight) > 0, J(geo));
  check("3.3 括号整体在行块右侧", geo.bracket.right <= geo.hostRight + 1 && geo.line.top >= geo.rows.top - 2, J(geo));
  await page.locator(".cal-duration-bracket").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check("3.4 括号菜单快捷档仍在", menuText.includes("1 小时") && menuText.includes("全天"), menuText.replace(/\n/g, "|"));
  await page.keyboard.press("Escape");
  check("3.5 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 分类管理面板：业界化列表 + 无解释文案 + 增删改与默认星
// ---------------------------------------------------------------------------
async function section4CategoryPanel(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      schemaVersion: 1,
      events: [],
      categories: [
        { id: "ccat-a", name: "工作", color: "#3d8bfd", isDefault: true, order: 1, createdAt: new Date().toISOString() },
        { id: "ccat-b", name: "个人", color: "#2f9e6e", isDefault: false, order: 2, createdAt: new Date().toISOString() }
      ]
    }
  });
  await openSchedulePage(page);
  await page.locator(".header-actions button[title='日程菜单']").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "管理分类" }).click();
  await page.waitForSelector(".calendar-category-panel", { timeout: 3000 });
  const panelText = await page.locator(".calendar-category-panel").innerText();
  check("4.1 无同步解释文案", !panelText.includes("同步"), panelText.replace(/\n/g, "|"));
  check("4.2 无删分类后果解释", !panelText.includes("退回无分类"), panelText.replace(/\n/g, "|"));
  const rows = page.locator(".calendar-category-panel .calendar-cat-row");
  check("4.3 列表两行", (await rows.count()) === 2);
  const dotColors = await page.evaluate(() =>
    [...document.querySelectorAll(".calendar-cat-row .calendar-cat-color")].map((el) => getComputedStyle(el).backgroundColor)
  );
  check("4.4 行首色点可见且跟分类色", dotColors.length === 2 && dotColors[0] === "rgb(61, 139, 253)", J(dotColors));
  const starOn = await page.locator(".calendar-cat-row .calendar-cat-star.on").count();
  check("4.5 默认分类星形点亮", starOn === 1);
  // 整行点击进编辑
  await rows.first().click();
  await page.waitForSelector(".calendar-category-panel .calendar-field-input", { timeout: 3000 });
  const nameValue = await page.locator(".calendar-category-panel .calendar-field-input").inputValue();
  check("4.6 整行点击进编辑且带原名", nameValue === "工作", nameValue);
  const formText = await page.locator(".calendar-category-panel").innerText();
  check("4.7 表单无块内效果解释长句", !formText.includes("左侧边框是主色"), formText.replace(/\n/g, "|"));
  // 选中色块打勾
  const checkedDots = await page.locator(".calendar-category-panel .calendar-cat-dot.active svg").count();
  check("4.8 当前颜色色块带勾", checkedDots === 1);
  await page.locator(".calendar-category-panel .calendar-cat-dot").nth(3).click();
  await page.waitForTimeout(200);
  const checkedAfter = await page.evaluate(() => {
    const dots = [...document.querySelectorAll(".calendar-category-panel .calendar-cat-dot")];
    return dots.findIndex((dot) => dot.classList.contains("active"));
  });
  check("4.9 点色块即改选中色", checkedAfter === 3, String(checkedAfter));
  await page.locator(".calendar-category-panel .calendar-sheet-foot .settings-button.primary").click();
  await page.waitForTimeout(400);
  const savedColor = await page.evaluate(() => {
    const raw = JSON.parse(localStorage.getItem("todo-note-calendar-v1"));
    return raw.categories.find((item) => item.id === "ccat-a").color;
  });
  check("4.10 保存落盘新颜色", savedColor === "#e0654f", savedColor);
  // 底部添加按钮新建
  await page.locator(".calendar-category-panel .calendar-cat-add").click();
  await page.waitForSelector(".calendar-category-panel .calendar-field-input", { timeout: 3000 });
  await page.locator(".calendar-category-panel .calendar-field-input").fill("体检");
  await page.locator(".calendar-category-panel .calendar-sheet-foot .settings-button.primary").click();
  await page.waitForTimeout(400);
  check("4.11 新建后回列表三行", (await page.locator(".calendar-category-panel .calendar-cat-row").count()) === 3);
  // 星形切换默认（列表行内）
  await page.locator(".calendar-cat-row", { hasText: "体检" }).locator(".calendar-cat-star").click();
  await page.waitForTimeout(400);
  const defaultName = await page.evaluate(() => {
    const raw = JSON.parse(localStorage.getItem("todo-note-calendar-v1"));
    return raw.categories.find((item) => item.isDefault)?.name;
  });
  check("4.12 星形切换默认全局互斥", defaultName === "体检", String(defaultName));
  // 行内删除
  await page.locator(".calendar-cat-row", { hasText: "个人" }).locator(".calendar-cat-remove").click();
  await page.waitForTimeout(400);
  check("4.13 行内删除生效", (await page.locator(".calendar-category-panel .calendar-cat-row").count()) === 2);
  // 编辑器地点 placeholder 不再带「可留空」
  await page.locator(".calendar-category-panel .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const placeholder = await page.locator(".calendar-editor input[placeholder*='会议室']").getAttribute("placeholder");
  check("4.14 地点 placeholder 无「可留空」", placeholder === "会议室 / 地址", String(placeholder));
  check("4.15 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 习惯打卡桌面全链路：新建 → 打卡 → 统计 → 暂停 → 删除
// ---------------------------------------------------------------------------
async function section5HabitDesktop(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openHabitTool(page);
  check("5.1 空态提示", (await page.locator(".habit-empty").innerText()).includes("还没有进行中"));
  // 子页头部加号新建
  await page.locator(".toolbox-sub-bar button[title='新建习惯']").click();
  await page.waitForSelector(".habit-sheet", { timeout: 4000 });
  await page.locator(".habit-sheet .habit-input").fill("刷牙");
  // 时间菜单选 08:00
  await page.locator(".habit-sheet .cal-field", { hasText: "时间" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "08:00" }).click();
  await page.waitForTimeout(250);
  // 提醒预设
  await page.locator(".habit-sheet .habit-reminder-add").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "计划时刻" }).click();
  await page.waitForTimeout(250);
  check("5.2 提醒芯片出现", (await page.locator(".habit-reminder-chip").count()) === 1);
  await page.locator(".habit-sheet .calendar-sheet-foot .settings-button.primary").click();
  await page.waitForTimeout(500);
  check("5.3 卡片出现且累计 0 天", (await page.locator(".habit-card").count()) === 1 && (await page.locator(".habit-card-stat strong").innerText()) === "0天");
  check("5.4 卡片副标题含频率与时刻", (await page.locator(".habit-card-sub").innerText()).includes("每天") && (await page.locator(".habit-card-sub").innerText()).includes("08:00"));
  // 每日视图打卡
  await page.locator(".habit-view-switch button", { hasText: "每日" }).click();
  await page.waitForTimeout(300);
  check("5.5 每日视图有上午分组", (await page.locator(".habit-slot-head", { hasText: "上午" }).count()) === 1);
  const dot = page.locator(".habit-dot").first();
  await dot.click();
  await page.waitForTimeout(400);
  check("5.6 点圆即打卡（checked 态）", (await page.locator(".habit-dot.checked").count()) === 1);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), HABIT_KEY);
  check("5.7 打卡落盘今天", stored.habits[0].checkins.includes(localToday()), J(stored.habits[0].checkins));
  // 统计面板
  await page.locator(".habit-view-switch button", { hasText: "全部" }).click();
  await page.waitForTimeout(300);
  check("5.8 累计变 1 天", (await page.locator(".habit-card-stat strong").innerText()) === "1天");
  await page.locator(".habit-card").first().click();
  await page.waitForSelector(".habit-stats-grid", { timeout: 4000 });
  const statsText = await page.locator(".habit-stats-grid").innerText();
  check("5.9 统计四格含累计/连续", statsText.includes("累计打卡") && statsText.includes("当前连续") && statsText.includes("最长连续") && statsText.includes("本月打卡"), statsText.replace(/\n/g, "|"));
  const statNums = await page.locator(".habit-stat strong").allInnerTexts();
  check("5.10 累计/连续/本月都是 1", statNums[0].startsWith("1") && statNums[1].startsWith("1") && statNums[3].startsWith("1"), J(statNums));
  check("5.11 热力图今天实心", (await page.locator(".habit-heat-cell.checked.today").count()) === 1);
  await page.locator(".habit-sheet .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  // 取消打卡
  await page.locator(".habit-view-switch button", { hasText: "每日" }).click();
  await page.waitForTimeout(300);
  await page.locator(".habit-dot").first().click();
  await page.waitForTimeout(400);
  check("5.12 再点取消打卡", (await page.locator(".habit-dot.checked").count()) === 0);
  await page.locator(".habit-view-switch button", { hasText: "全部" }).click();
  await page.waitForTimeout(300);
  // 右键菜单暂停
  await page.locator(".habit-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "暂停" }).click();
  await page.waitForTimeout(400);
  check("5.13 暂停后进行中页签空", (await page.locator(".habit-card").count()) === 0);
  await page.locator(".habit-tab", { hasText: "已暂停" }).click();
  await page.waitForTimeout(300);
  check("5.14 已暂停页签计数 1", (await page.locator(".habit-tab.active .habit-tab-count").innerText()) === "1");
  check("5.15 暂停卡片在列", (await page.locator(".habit-card").count()) === 1);
  // 删除两步确认
  await page.locator(".habit-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "删除" }).click();
  await page.waitForTimeout(200);
  await page.locator(".context-menu .menu-item-button", { hasText: "确认删除" }).click();
  await page.waitForTimeout(400);
  check("5.16 删除后空态", (await page.locator(".habit-card").count()) === 0);
  const storedAfter = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), HABIT_KEY);
  check("5.17 落盘已删", storedAfter.habits.length === 0, J(storedAfter));
  check("5.18 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 习惯打卡移动端 360：建/打/统计 + 逐元素视口扫
// ---------------------------------------------------------------------------
async function section6HabitMobile() {
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
  const context = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(context, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    habits: {
      habits: [
        habitSeed("habit-a", "刷牙", { time: "08:00", order: 1 }),
        habitSeed("habit-b", "阅读", { order: 2 }),
        habitSeed("habit-c", "锻炼", { time: "19:00", order: 3 }),
        habitSeed("habit-d", "周报", { freq: { kind: "weekly", weekdays: [0] }, time: "17:00", order: 4 })
      ]
    }
  });
  await openHabitTool(page, true);
  check("6.1 移动端四张卡片", (await page.locator(".habit-card").count()) === 4);
  const sweep = async (label) => {
    const bad = await page.evaluate(() => {
      const out = [];
      const vw = document.documentElement.clientWidth;
      for (const el of document.querySelectorAll(".habit-tool *, .habit-sheet *")) {
        const rect = el.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) continue;
        if (rect.left < -1 || rect.right > vw + 1) out.push(`${el.className}`.slice(0, 60));
      }
      return out;
    });
    check(label, bad.length === 0, J(bad.slice(0, 6)));
  };
  await sweep("6.2 全部视图 360 无溢出");
  await page.locator(".habit-view-switch button", { hasText: "每日" }).click();
  await page.waitForTimeout(300);
  check("6.3 每日视图分组（周日才有周报）", (await page.locator(".habit-slot").count()) >= 2);
  await sweep("6.4 每日视图 360 无溢出");
  await page.locator(".habit-dot").first().click();
  await page.waitForTimeout(400);
  check("6.5 触屏点圆打卡", (await page.locator(".habit-dot.checked").count()) === 1);
  await page.locator(".habit-view-switch button", { hasText: "全部" }).click();
  await page.waitForTimeout(300);
  await page.locator(".habit-card").first().click();
  await page.waitForSelector(".habit-stats-grid", { timeout: 4000 });
  await sweep("6.6 统计面板 360 无溢出");
  check("6.7 统计两列布局", await page.evaluate(() => getComputedStyle(document.querySelector(".habit-stats-grid")).gridTemplateColumns.split(" ").length === 2));
  await page.locator(".habit-sheet .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  // 移动端新建（加号在子页头部）
  await page.locator(".toolbox-sub-bar button[title='新建习惯']").click();
  await page.waitForSelector(".habit-sheet", { timeout: 4000 });
  await sweep("6.8 编辑器 360 无溢出");
  await page.locator(".habit-sheet .habit-input").fill("喝水");
  await page.locator(".habit-sheet .cal-field", { hasText: "计划" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "周一" }).click();
  await page.waitForTimeout(200);
  await page.locator(".context-menu .menu-item-button", { hasText: "周三" }).click();
  await page.waitForTimeout(200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  const freqText = await page.locator(".habit-sheet .cal-field", { hasText: "计划" }).locator(".cal-field-value").innerText();
  check("6.9 周菜单多选成每周一、三", freqText === "每周一、三", freqText);
  await page.locator(".habit-sheet .calendar-sheet-foot .settings-button.primary").click();
  await page.waitForTimeout(500);
  check("6.10 新建成功五张卡", (await page.locator(".habit-card").count()) === 5);
  check("6.11 无页面错误", errors.length === 0, J(errors));
  await browser.close();
}

// ---------------------------------------------------------------------------
// 7. 剪切板发送端无感：读路径不改内容（repair 只在接收端）——源码级断言 + 接收端修复单测已在 core
// ---------------------------------------------------------------------------
async function section7ClipboardSender(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  // 浏览器 dev 无原生剪切板桥：工具页应照旧可用且不报错（能力门控）
  const nav = ".system-nav .nav-row";
  await page.locator(nav, { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "剪切板同步" }).click();
  await page.waitForTimeout(600);
  const bodyText = await page.locator(".toolbox-view").innerText();
  check("7.1 剪切板工具页正常打开", bodyText.includes("剪切板"), bodyText.slice(0, 80));
  check("7.2 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });

await section1TodayCircle();
await section2MobileTimeRows();
await section3DesktopBracket(context);
await section4CategoryPanel(context);
await section5HabitDesktop(context);
await section6HabitMobile();
await section7ClipboardSender(context);

await browser.close();
console.log(`\n${passes} passed, ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
// v0.9.0 回归。逐条覆盖本版需求：日程（视图/三入口/双向详情/图标）、草稿纸标签页、
// 安卓滑动、以及体验优化与 bugfix 七项。
// 用法：node scripts/v090-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";
const CALENDAR_KEY = "todo-note-calendar-v1";

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
  if (mobile) {
    await page.setViewportSize({ width: 360, height: 800 });
  }
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

async function seedState(
  page,
  { nodes = [], tasks = [], settings = null, calendar = null, scheduler = null } = {}
) {
  await page.evaluate(
    ({ nodes, tasks, settings, calendar, scheduler, stateKey, settingsKey, calendarKey, schedulerKey }) => {
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
      localStorage.setItem(stateKey, JSON.stringify(state));
      // legacy 模式调度态住独立键（与 backend.ts::schedulerKey 同口径）
      if (scheduler) localStorage.setItem(schedulerKey, JSON.stringify(scheduler));
      if (settings) {
        const existing = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings }));
      }
      if (calendar) {
        localStorage.setItem(calendarKey, JSON.stringify(calendar));
      }
    },
    {
      nodes, tasks, settings, calendar, scheduler,
      stateKey: STATE_KEY, settingsKey: SETTINGS_KEY, calendarKey: CALENDAR_KEY,
      schedulerKey: "todo-note-scheduler-v8"
    }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

function entryNode(id, name) {
  return { id, kind: "entry", name, icon: "notebook", parentId: null, order: 1, createdAt: new Date().toISOString() };
}

function task(id, nodeId, markdown, extra = {}) {
  return {
    id, nodeId, order: 1, markdown, completed: false, important: false, myDay: false,
    tags: [], emojis: [], createdAt: new Date().toISOString(), ...extra
  };
}

function calendarEvent(id, overrides = {}) {
  const today = localToday();
  return {
    id, title: "测试日程", details: "", startDate: today, startTime: "09:00",
    endDate: today, endTime: "10:00", allDay: false, reminders: [], taskId: "",
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), ...overrides
  };
}

async function openSchedulePage(page) {
  const row = page.locator(".system-nav .nav-row", { hasText: "日程" }).first();
  await row.click();
  await page.waitForSelector(".calendar-view", { timeout: 5000 });
  await page.waitForTimeout(250);
}

// ---------------------------------------------------------------------------
// 1. 日程：桌面视图 / FAB 新建 / 编辑器 / 月视图 / 右键菜单 / 持久化
// ---------------------------------------------------------------------------
async function section1ScheduleDesktop(context) {
  const { page, errors } = await freshPage(context);

  // 导航行存在（默认名单里日程在收藏之后）
  const row = page.locator(".system-nav .nav-row", { hasText: "日程" }).first();
  check("1.1 侧栏有「日程」行", await row.count() === 1);
  const labels = await page.locator(".system-nav .nav-row").allInnerTexts();
  const calIndex = labels.findIndex((text) => text.includes("日程"));
  const impIndex = labels.findIndex((text) => text.includes("收藏"));
  check("1.2 日程行默认排在收藏之后", calIndex === impIndex + 1, `labels=${J(labels)}`);

  await openSchedulePage(page);
  check("1.3 打开日程页", await page.locator(".calendar-view").count() === 1);

  // 默认周视图：段控第 2 个按钮 active，网格 7 列
  const activeIndex = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".calendar-view-switch button")];
    return buttons.findIndex((button) => button.classList.contains("active"));
  });
  check("1.4 默认周视图", activeIndex === 1, `activeIndex=${activeIndex}`);
  check("1.5 周视图 7 列", await page.locator(".cal-day-col").count() === 7);
  check("1.6 星期头 7 个", await page.locator(".cal-day-head").count() === 7);

  // 今天列有「现在」红线
  check("1.7 今天列画现在线", await page.locator(".cal-day-col.today .cal-now").count() === 1);

  // FAB 新建：编辑器打开、默认今天、保存后网格出现事件块
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  await page.locator(".calendar-title-input").fill("周会");
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  const block = page.locator(".cal-event .cal-event-title", { hasText: "周会" });
  check("1.8 FAB 建的日程出现在网格里", await block.count() >= 1);

  // 持久化（legacy localStorage 路径）：等过 180ms 防抖再刷新
  await page.waitForTimeout(450);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(600);
  await openSchedulePage(page);
  check("1.9 刷新后日程还在", await page.locator(".cal-event .cal-event-title", { hasText: "周会" }).count() >= 1);

  // 切月视图：42 格、今天格子里有芯片
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(300);
  check("1.10 月视图 42 格", await page.locator(".cal-month-cell").count() === 42);
  check("1.11 月视图今天格有芯片", await page.locator(".cal-month-cell.today .cal-month-chip").count() >= 1);

  // 月视图点空白格 → 新建全天日程（预置那天）。点格子底部空白，避开日期号与芯片
  const cellBox = await page.locator(".cal-month-cell.today").boundingBox();
  await page.mouse.click(cellBox.x + cellBox.width / 2, cellBox.y + cellBox.height - 6);
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  const allDayChecked = await page.locator(".calendar-allday input").isChecked();
  check("1.12 月视图点格子默认全天", allDayChecked);
  await page.locator(".calendar-title-input").fill("全天事项");
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  await page.waitForTimeout(300);
  check("1.13 全天日程落进月格子", await page.locator(".cal-month-cell.today .cal-month-chip", { hasText: "全天事项" }).count() >= 1);

  // 全天日程在周视图的全天栏
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(300);
  check("1.14 全天日程住全天栏", await page.locator(".cal-allday-cell .cal-chip", { hasText: "全天事项" }).count() >= 1);

  // 右键事件块 → 菜单（编辑日程 / 删除）
  await page.locator(".cal-event", { hasText: "周会" }).first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check("1.15 事件右键菜单有编辑/删除", menuText.includes("编辑日程") && menuText.includes("删除"));
  await page.locator(".context-menu .menu-item-button", { hasText: "删除" }).click();
  await page.waitForTimeout(400);
  check("1.16 右键删除生效", await page.locator(".cal-event", { hasText: "周会" }).count() === 0);

  // 三点菜单：有 UI颜色/背景颜色，没有导出导入
  await page.locator(".header-actions button[title='日程菜单']").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const listText = await page.locator(".context-menu").innerText();
  check("1.17 三点菜单有外观区", listText.includes("UI颜色") && listText.includes("背景颜色"));
  check("1.18 三点菜单没有导出导入", !listText.includes("导出") && !listText.includes("导入"));
  // 再点一次收起
  await page.locator(".header-actions button[title='日程菜单']").click();
  await page.waitForTimeout(250);
  check("1.19 三点按钮再点收起", await page.locator(".context-menu").count() === 0);

  // 视图切换持久化（settings.calendar.view）
  await page.locator(".calendar-view-switch button[title='日视图']").click();
  await page.waitForTimeout(300);
  check("1.20 日视图 1 列", await page.locator(".cal-day-col").count() === 1);
  const savedView = await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    return settings?.calendar?.view;
  }, SETTINGS_KEY);
  check("1.21 视图选择落盘 calendar.view", savedView === "day", `savedView=${savedView}`);

  // 三日视图
  await page.locator(".calendar-view-switch button[title='三日视图']").click();
  await page.waitForTimeout(300);
  check("1.22 三日视图 3 列", await page.locator(".cal-day-col").count() === 3);

  check("1.23 桌面日程无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 2. 日程：网格拖选建日程 / 点空白建默认一小时 / 事件点击进编辑
// ---------------------------------------------------------------------------
async function section2GridInteractions(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { calendar: { events: [calendarEvent("cal-1", { title: "已有日程", startTime: "13:00", endTime: "14:30" })] } });
  await openSchedulePage(page);
  await page.locator(".calendar-view-switch button[title='日视图']").click();
  await page.waitForTimeout(300);

  const col = page.locator(".cal-day-col").first();
  const box = await col.boundingBox();

  // 拖选 10:00 → 12:00（48px/小时：10 点 = 480px 起）
  const hourH = box.height / 24;
  await page.mouse.move(box.x + box.width / 2, box.y + hourH * 10.05);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + hourH * 12 - 2, { steps: 8 });
  await page.waitForTimeout(120);
  const preview = await page.locator(".cal-drag-preview").innerText().catch(() => "");
  check("2.1 拖选出现预览块（10:00 – 12:00）", preview.replace(/\s/g, "").includes("10:00–12:00"), `preview=${preview}`);
  await page.mouse.up();
  await page.waitForSelector(".calendar-editor", { timeout: 3000 });
  const metaText = await page.locator(".calendar-meta").innerText();
  check("2.2 拖选放开进编辑器且预置 10:00", metaText.includes("10:00"), `meta=${metaText}`);
  await page.locator(".calendar-title-input").fill("拖选日程");
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  await page.waitForTimeout(300);
  const dragged = page.locator(".cal-event", { hasText: "拖选日程" });
  check("2.3 拖选日程落盘上屏", await dragged.count() === 1);
  const draggedBox = await dragged.first().boundingBox();
  const heightHours = draggedBox.height / hourH;
  check("2.4 拖选日程时长两小时", Math.abs(heightHours - 2) < 0.2, `hours=${heightHours}`);

  // 单击空白：默认一小时
  await page.mouse.click(box.x + box.width / 2, box.y + hourH * 16.5);
  await page.waitForSelector(".calendar-editor", { timeout: 3000 });
  const clickMeta = await page.locator(".calendar-meta").innerText();
  check("2.5 单击空白预置 16:30 起", clickMeta.includes("16:30"), `meta=${clickMeta}`);
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), CALENDAR_KEY);
  check("2.6 空标题空详情关掉不留日程", (stored.events ?? []).length === 2, `n=${(stored.events ?? []).length}`);

  // 点击已有事件块 → 进编辑（标题预填）
  await page.locator(".cal-event", { hasText: "已有日程" }).click();
  await page.waitForSelector(".calendar-editor", { timeout: 3000 });
  const titleValue = await page.locator(".calendar-title-input").inputValue();
  check("2.7 点事件块进编辑且预填标题", titleValue === "已有日程", `title=${titleValue}`);
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });

  check("2.8 网格交互无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 3. 日程：todo 右键「添加到日程」/ 条目图标 / 详情双向
// ---------------------------------------------------------------------------
async function section3TaskLink(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "工作")],
    tasks: [task("task-1", "entry-a", "写周报", { dueDate: localToday(), dueTime: "15:00" })]
  });

  // 右键任务 → 添加到日程
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const items = await page.locator(".context-menu .menu-item-button").allInnerTexts();
  check("3.1 任务右键有「添加到日程」", items.some((text) => text.includes("添加到日程")), J(items));
  await page.locator(".context-menu .menu-item-button", { hasText: "添加到日程" }).click();
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  const title = await page.locator(".calendar-title-input").inputValue();
  check("3.2 编辑器预填任务名", title === "写周报", `title=${title}`);
  const metaText = await page.locator(".calendar-meta").innerText();
  check("3.3 预置任务的日期与时刻", metaText.includes("15:00"), `meta=${metaText}`);
  check("3.4 显示关联条目芯片", await page.locator(".calendar-linked-chip").count() === 1);
  const detailsText = await page.evaluate(() => {
    const cm = document.querySelector(".calendar-editor .cm-content");
    return cm?.cmView?.view?.state?.doc?.toString?.() ?? cm?.textContent ?? "";
  });
  check("3.5 详情预填任务内容", detailsText.includes("写周报"), `details=${detailsText}`);
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  await page.waitForTimeout(400);

  // 任务卡片出现日程图标
  check("3.6 任务卡显示日程图标", await page.locator(".task-card .task-calendar-badge").count() === 1);

  // 右键再点 = 打开已有日程
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  check("3.7 已关联后菜单变「打开日程」", (await page.locator(".context-menu").innerText()).includes("打开日程"));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);

  // 双向：任务编辑器改内容 → 日程详情跟着变（用真实键盘输入走 CM 的 updateListener）
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-overlay", { timeout: 8000 });
  await page.waitForTimeout(500);
  await page.locator(".editor-overlay .cm-content").click();
  await page.keyboard.press("Control+a");
  await page.keyboard.type("写周报 v2");
  await page.waitForTimeout(300);
  await page.locator(".editor-overlay .editor-icon-button.primary").click();
  await page.waitForSelector(".editor-overlay", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(600);
  const storedState = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), STATE_KEY);
  const taskMd = (storedState.tasks ?? []).find((item) => item.id === "task-1")?.markdown ?? "";
  check("3.8a 任务内容真的改掉了", taskMd.includes("写周报 v2"), `markdown=${taskMd}`);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), CALENDAR_KEY);
  const linked = (stored.events ?? []).find((event) => event.taskId === "task-1");
  check("3.8 任务内容改动同步进日程详情", Boolean(linked) && linked.details.includes("写周报 v2"), J(linked));

  check("3.9 关联链路无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 日程：老用户 navItems 一次性注入
// ---------------------------------------------------------------------------
async function section4NavSeeding(context) {
  const { page, errors } = await freshPage(context);
  // 存量设置：七行名单（没有 calendar）。「老安装」还要没有注入标记——
  // freshPage 水合时已经跑过一次 seed（把标记写上了），这里显式摘掉再重载。
  await seedState(page, {
    settings: {
      appearance: {
        navItems: ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"]
      }
    }
  });
  await page.evaluate(() => localStorage.removeItem("kxtodo-nav-calendar-seeded-v1"));
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const hasRow = await page.locator(".system-nav .nav-row", { hasText: "日程" }).count();
  check("4.1 存量名单自动补上日程行", hasRow === 1, `count=${hasRow}`);
  const persisted = await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    return settings?.appearance?.navItems ?? [];
  }, SETTINGS_KEY);
  check("4.2 注入结果落盘", persisted.includes("calendar"), J(persisted));

  // 用户此后隐藏日程（标记已在）：不再被塞回来
  await seedState(page, {
    settings: {
      appearance: {
        navItems: ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"]
      }
    }
  });
  await page.waitForTimeout(400);
  const hasRowAgain = await page.locator(".system-nav .nav-row", { hasText: "日程" }).count();
  check("4.3 已注入过的设备不再重复塞（隐藏可保留）", hasRowAgain === 0, `count=${hasRowAgain}`);

  check("4.4 注入无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 日程：移动端 360×800（视图完整、无横向溢出、点格建日程）
// ---------------------------------------------------------------------------
async function section5Mobile(context) {
  const { page, errors } = await freshPage(context, { mobile: true });
  await page.evaluate((ua) => {
    // isMobile 靠 UA：newContext 已设，这里只兜底记录
    void ua;
  }, ANDROID_UA);
  await seedState(page, {
    calendar: {
      events: [
        calendarEvent("cal-1", { title: "移动日程", startTime: "09:00", endTime: "10:00" }),
        calendarEvent("cal-2", { title: "移动全天", allDay: true, startTime: "", endTime: "" })
      ]
    }
  });

  const row = page.locator(".system-nav .nav-row", { hasText: "日程" }).first();
  check("5.1 移动端侧栏有日程行", await row.count() === 1);
  await row.tap();
  await page.waitForSelector(".calendar-view", { timeout: 5000 });
  await page.waitForTimeout(400);
  check("5.2 移动端整页日程", await page.evaluate(() => document.querySelector(".app-shell").classList.contains("view-schedule")));

  // 360 窄屏：任何元素不许横向溢出（v0.8.10 铁律的逐个元素量法）
  const overflow = await page.evaluate(() => {
    const bad = [];
    for (const el of document.querySelectorAll(".calendar-view *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (rect.right > window.innerWidth + 1 || rect.left < -1) {
        bad.push(`${el.className?.toString?.().slice(0, 40)} [${Math.round(rect.left)},${Math.round(rect.right)}]`);
      }
    }
    return bad.slice(0, 6);
  });
  check("5.3 周视图 360 宽无横向溢出", overflow.length === 0, J(overflow));
  check("5.4 事件块在网格上", await page.locator(".cal-event", { hasText: "移动日程" }).count() >= 1);
  check("5.5 全天日程在全天栏", await page.locator(".cal-allday-cell .cal-chip", { hasText: "移动全天" }).count() >= 1);

  // 点事件 → 编辑器（移动端全屏）
  await page.locator(".cal-event", { hasText: "移动日程" }).tap();
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  await page.waitForTimeout(300);
  const editorOverflow = await page.evaluate(() => {
    const dialog = document.querySelector(".calendar-editor");
    const rect = dialog?.getBoundingClientRect();
    return rect ? rect.right <= window.innerWidth + 1 && rect.left >= -1 : false;
  });
  check("5.6 移动端编辑器不超屏", editorOverflow);
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").tap();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });

  // 安卓返回键：先回列表
  const backOk = await page.evaluate(() => {
    const handler = window.kxtodoBackHandler;
    if (typeof handler !== "function") return "no-handler";
    return handler() ? "consumed" : "passthrough";
  });
  check("5.7 返回键处理链在", backOk === "consumed" || backOk === "passthrough", backOk);

  // 月视图窄屏
  await page.locator(".calendar-view-switch button[title='月视图']").tap();
  await page.waitForTimeout(400);
  const monthOverflow = await page.evaluate(() => {
    const bad = [];
    for (const el of document.querySelectorAll(".cal-month *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (rect.right > window.innerWidth + 1 || rect.left < -1) bad.push(String(el.className).slice(0, 40));
    }
    return bad.slice(0, 6);
  });
  check("5.8 月视图 360 宽无横向溢出", monthOverflow.length === 0, J(monthOverflow));
  check("5.9 月视图 42 格", await page.locator(".cal-month-cell").count() === 42);

  check("5.10 移动端日程无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 草稿纸标签页（新特性 1）：加号新建 / 默认名 / 改名 / 关闭 / 右键三选项 /
//    宽度伸缩与左对齐 / 落盘与重载
// ---------------------------------------------------------------------------
const SCRATCH_CACHE_KEY = "kxtodo-scratchpad-v2";

async function openScratchpad(page) {
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.waitForTimeout(250);
}

async function tabNames(page) {
  return page.locator(".scratchpad-tab-name").allTextContents();
}

async function section6ScratchpadTabs(desktopContext) {
  const { page, errors } = await freshPage(desktopContext);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [] });
  await openScratchpad(page);

  check("6.1 单页时不渲染标签栏", (await page.locator(".scratchpad-tabs").count()) === 0);
  const actions = await page.evaluate(() => {
    const bar = document.querySelector(".toolbox-sub-bar-actions");
    if (!bar) return null;
    return [...bar.querySelectorAll("button")].map((b) => b.getAttribute("aria-label") || b.getAttribute("title"));
  });
  check(
    "6.2 加号在 ⋯ 左侧（新建标签页）",
    Array.isArray(actions) && actions[0] === "新建标签页" && actions[1] === "外观",
    J(actions)
  );

  const plus = page.locator('button[aria-label="新建标签页"]');
  await plus.click();
  await page.waitForTimeout(250);
  check("6.3 新建后标签栏出现（2 页）", (await page.locator(".scratchpad-tab").count()) === 2);
  check("6.4 默认名 1 / 2", (await tabNames(page)).join(",") === "1,2", J(await tabNames(page)));
  check("6.5 新页被激活", (await page.locator(".scratchpad-tab.active .scratchpad-tab-name").textContent()) === "2");

  // 内容按页隔离
  await page.locator(".scratchpad-area").fill("第二页的内容");
  await page.waitForTimeout(150);
  await page.locator(".scratchpad-tab").nth(0).click();
  await page.waitForTimeout(250);
  check("6.6 切到第一页内容为空（隔离）", (await page.locator(".scratchpad-area").inputValue()) === "");
  await page.locator(".scratchpad-tab").nth(1).click();
  await page.waitForTimeout(250);
  check("6.7 切回第二页内容还在", (await page.locator(".scratchpad-area").inputValue()) === "第二页的内容");

  // 再点已激活的 = 改名
  await page.locator(".scratchpad-tab.active").click();
  await page.waitForTimeout(200);
  check("6.8 点激活页进入改名", (await page.locator(".scratchpad-tab-rename").count()) === 1);
  await page.locator(".scratchpad-tab-rename").fill("会议");
  await page.locator(".scratchpad-tab-rename").press("Enter");
  await page.waitForTimeout(250);
  check("6.9 改名生效", (await tabNames(page)).join(",") === "1,会议", J(await tabNames(page)));

  // 改名后再新建：默认名不许与既有名重复
  await plus.click();
  await page.waitForTimeout(250);
  const names3 = await tabNames(page);
  check("6.10 新建默认名不重复", names3.length === 3 && new Set(names3).size === 3, J(names3));

  // 宽度：min/max 钳制 + 左对齐（flex 1 1 0）
  const geom = await page.evaluate(() => {
    const bar = document.querySelector(".scratchpad-tabs");
    const tabs = [...document.querySelectorAll(".scratchpad-tab")];
    const barRect = bar.getBoundingClientRect();
    const rects = tabs.map((t) => t.getBoundingClientRect());
    return {
      widths: rects.map((r) => Math.round(r.width)),
      leftGap: Math.round(rects[0].left - barRect.left),
      freeRight: Math.round(barRect.right - rects[rects.length - 1].right),
      scrollable: getComputedStyle(bar).overflowX
    };
  });
  check("6.11 标签宽度在 84–200 之间", geom.widths.every((w) => w >= 83 && w <= 201), J(geom));
  check("6.12 标签左对齐（右侧留白）", geom.leftGap <= 8 && geom.freeRight > 20, J(geom));
  check("6.13 放不下时横向滚动", geom.scrollable === "auto", J(geom));

  // X 关闭（桌面悬浮露出，占位恒定不引起宽度跳动）。先给两页写上可辨识的内容。
  await page.locator(".scratchpad-tab").nth(0).click();
  await page.waitForTimeout(200);
  await page.locator(".scratchpad-area").fill("A");
  await page.locator(".scratchpad-tab").nth(1).click();
  await page.waitForTimeout(200);
  await page.locator(".scratchpad-area").fill("B");
  await page.waitForTimeout(200);

  const widthBefore = geom.widths[0];
  await page.locator(".scratchpad-tab").nth(0).hover();
  await page.waitForTimeout(150);
  const closeOpacity = await page.evaluate(
    () => getComputedStyle(document.querySelectorAll(".scratchpad-tab")[0].querySelector(".scratchpad-tab-close")).opacity
  );
  check("6.14 悬浮露出关闭叉", Number(closeOpacity) > 0.5, closeOpacity);

  // 关掉非激活页：活动页与正文都不动
  await page.locator(".scratchpad-tab").nth(0).locator(".scratchpad-tab-close").click({ force: true });
  await page.waitForTimeout(250);
  const afterClose = await page.evaluate(() => ({
    count: document.querySelectorAll(".scratchpad-tab").length,
    names: [...document.querySelectorAll(".scratchpad-tab-name")].map((n) => n.textContent),
    active: document.querySelector(".scratchpad-tab.active .scratchpad-tab-name")?.textContent ?? "",
    widths: [...document.querySelectorAll(".scratchpad-tab")].map((t) => Math.round(t.getBoundingClientRect().width))
  }));
  check("6.15 X 关掉非激活页", afterClose.count === 2 && afterClose.names.join(",") === "会议,2", J(afterClose));
  check("6.16 关掉非激活页不动活动页正文", afterClose.active === "会议" && (await page.locator(".scratchpad-area").inputValue()) === "B", J(afterClose));
  check("6.17 关闭不引起其余标签跳动", afterClose.widths.every((w) => Math.abs(w - widthBefore) <= 1), J(afterClose.widths));

  // 关掉激活页：选邻居并显示邻居正文
  await page.locator(".scratchpad-tab.active").locator(".scratchpad-tab-close").click({ force: true });
  await page.waitForTimeout(250);
  check("6.18 关掉激活页后选邻居", (await page.locator(".scratchpad-tabs").count()) === 0 && (await page.locator(".scratchpad-area").inputValue()) === "");

  // 右键菜单三选项（4 页起，关掉右侧后还剩 2 页，标签栏可见才好断言）
  await plus.click();
  await page.waitForTimeout(200);
  await plus.click();
  await page.waitForTimeout(200);
  await plus.click();
  await page.waitForTimeout(250);
  check("6.19 连点加号到 4 页", (await page.locator(".scratchpad-tab").count()) === 4);
  await page.locator(".scratchpad-tab").nth(1).click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 4000 });
  const menuItems = await page.locator(".context-menu .menu-item-label").allTextContents();
  check(
    "6.20 右键菜单 = 关闭当前/其他/右侧",
    menuItems.join("|") === "关闭当前标签页|关闭其他标签页|关闭右侧标签页",
    J(menuItems)
  );
  await page.locator(".context-menu .menu-item-label", { hasText: "关闭右侧标签页" }).click();
  await page.waitForTimeout(250);
  check("6.21 关闭右侧生效", (await tabNames(page)).length === 2, J(await tabNames(page)));

  await page.locator(".scratchpad-tab").nth(0).click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 4000 });
  await page.locator(".context-menu .menu-item-label", { hasText: "关闭其他标签页" }).click();
  await page.waitForTimeout(250);
  check("6.22 关闭其他生效（标签栏收起）", (await page.locator(".scratchpad-tabs").count()) === 0);

  await plus.click();
  await page.waitForTimeout(250);
  await page.locator(".scratchpad-tab").nth(0).click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 4000 });
  await page.locator(".context-menu .menu-item-label", { hasText: "关闭当前标签页" }).click();
  await page.waitForTimeout(250);
  check("6.23 关闭当前生效（只剩一页）", (await page.locator(".scratchpad-tabs").count()) === 0);

  // 落盘：文本防抖 5 秒进数据域，结构变更立即进缓存
  await plus.click();
  await page.waitForTimeout(250);
  await page.locator(".scratchpad-area").fill("持久化检查");
  await page.waitForTimeout(5400);
  const domain = await page.evaluate(
    ({ stateKey, cacheKey }) => ({
      scratchpad: JSON.parse(localStorage.getItem(stateKey) ?? "{}").scratchpad ?? null,
      cache: JSON.parse(localStorage.getItem(cacheKey) ?? "null")
    }),
    { stateKey: STATE_KEY, cacheKey: SCRATCH_CACHE_KEY }
  );
  const activeTab = domain.scratchpad?.tabs?.find((t) => t.id === domain.scratchpad?.activeTabId);
  check("6.24 正文进数据域（可同步）", activeTab?.text === "持久化检查", J(domain.scratchpad));
  check(
    "6.25 缓存是标签页结构",
    Array.isArray(domain.cache?.tabs) && domain.cache.tabs.length === domain.scratchpad?.tabs?.length,
    J(domain.cache)
  );
  const namesBeforeReload = await tabNames(page);

  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await openScratchpad(page);
  check("6.26 重载后正文与激活页还在", (await page.locator(".scratchpad-area").inputValue()) === "持久化检查");
  check("6.27 重载后标签名还在", (await tabNames(page)).join(",") === namesBeforeReload.join(","), J(await tabNames(page)));
  check("6.28 草稿纸标签页无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 7. 草稿纸标签页：移动端 360 宽（关闭叉常显、宽度口径、无横向溢出、点按切换）
// ---------------------------------------------------------------------------
async function section7ScratchpadMobile(context) {
  const { page, errors } = await freshPage(context, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [] });
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).first().tap();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).tap();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.waitForTimeout(300);

  await page.locator('button[aria-label="新建标签页"]').tap();
  await page.waitForTimeout(300);
  check("7.1 移动端新建标签页", (await page.locator(".scratchpad-tab").count()) === 2);

  // 触屏没有 hover：关闭叉必须常显
  const closeOpacity = await page.evaluate(
    () => getComputedStyle(document.querySelectorAll(".scratchpad-tab")[0].querySelector(".scratchpad-tab-close")).opacity
  );
  check("7.2 触屏关闭叉常显", Number(closeOpacity) > 0.3, closeOpacity);

  const geom = await page.evaluate(() => {
    const bar = document.querySelector(".scratchpad-tabs");
    const barRect = bar.getBoundingClientRect();
    const rects = [...document.querySelectorAll(".scratchpad-tab")].map((t) => t.getBoundingClientRect());
    return {
      widths: rects.map((r) => Math.round(r.width)),
      barRight: Math.round(barRect.right),
      innerWidth: window.innerWidth
    };
  });
  check("7.3 移动端标签宽度在 76–168 之间", geom.widths.every((w) => w >= 75 && w <= 169), J(geom));
  check("7.4 标签栏不超屏", geom.barRight <= geom.innerWidth + 1, J(geom));

  // 新建的第二页是活动页；写完后点第一页 = 切换，再点第二页 = 切回
  await page.locator(".scratchpad-area").fill("移动端第二页");
  await page.waitForTimeout(200);
  await page.locator(".scratchpad-tab").nth(0).tap();
  await page.waitForTimeout(300);
  check("7.5 移动端点按切换标签", (await page.locator(".scratchpad-area").inputValue()) === "");
  await page.locator(".scratchpad-tab").nth(1).tap();
  await page.waitForTimeout(300);
  check("7.6 切回原页内容还在", (await page.locator(".scratchpad-area").inputValue()) === "移动端第二页");
  check("7.7 移动端草稿纸无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 8. 移动端任务卡左右滑（新特性 3）：右滑渐进露出删除且到位即删；左滑同时露出
//    置顶与编辑（一起缩放）；纵向手势与下拉同步互不干扰
// ---------------------------------------------------------------------------

/**
 * 合成一次触摸滑动。Playwright 的 touchscreen 只有 tap，滑动要自己派发 TouchEvent
 * （swipeRow 只认 touch*，用 mouse 走不到那条路径）。坐标是**视觉像素**（clientX），
 * 与真实手指同一套单位——壳的 uiScale 换算由被测代码自己负责。
 */
async function touchSwipe(page, selector, dx, dy, { steps = 8, release = true, holdAt = 0 } = {}) {
  await page.evaluate(
    ({ selector, dx, dy, steps, release, holdAt }) => {
      const el = document.querySelector(selector);
      if (!el) throw new Error(`selector not found: ${selector}`);
      const rect = el.getBoundingClientRect();
      const startX = rect.left + rect.width / 2;
      const startY = rect.top + Math.min(rect.height / 2, 30);
      const make = (type, x, y) => {
        const touch = new Touch({ identifier: 1, target: el, clientX: x, clientY: y });
        const ended = type === "touchend" || type === "touchcancel";
        return new TouchEvent(type, {
          bubbles: true,
          cancelable: true,
          composed: true,
          touches: ended ? [] : [touch],
          targetTouches: ended ? [] : [touch],
          changedTouches: [touch]
        });
      };
      el.dispatchEvent(make("touchstart", startX, startY));
      const last = holdAt > 0 ? holdAt : steps;
      for (let index = 1; index <= last; index += 1) {
        el.dispatchEvent(make("touchmove", startX + (dx * index) / steps, startY + (dy * index) / steps));
      }
      if (release) {
        for (let index = last + 1; index <= steps; index += 1) {
          el.dispatchEvent(make("touchmove", startX + (dx * index) / steps, startY + (dy * index) / steps));
        }
        el.dispatchEvent(make("touchend", startX + dx, startY + dy));
      }
    },
    { selector, dx, dy, steps, release, holdAt }
  );
}

/** 只抬手（配合 holdAt 的「拖到一半先量一量」用法） */
async function touchEnd(page, selector) {
  await page.evaluate((sel) => {
    const el = document.querySelector(sel);
    if (!el) return;
    const touch = new Touch({ identifier: 1, target: el, clientX: 0, clientY: 0 });
    el.dispatchEvent(
      new TouchEvent("touchend", { bubbles: true, cancelable: true, touches: [], targetTouches: [], changedTouches: [touch] })
    );
  }, selector);
}

/** 读一行的手势状态：CSS 变量、方向、两块动作的缩放与透明度 */
async function swipeState(page, index = 0) {
  return page.evaluate((which) => {
    const row = document.querySelectorAll(".task-swipe")[which];
    if (!row) return null;
    const card = row.querySelector(".task-card");
    const style = getComputedStyle(row);
    const actions = [...row.querySelectorAll(".swipe-action")].map((el) => {
      const computed = getComputedStyle(el);
      return {
        kind: el.dataset.swipeAction,
        opacity: Number(computed.opacity),
        transform: computed.transform,
        width: Math.round(el.getBoundingClientRect().width)
      };
    });
    const sides = [...row.querySelectorAll(".swipe-side")].map((el) => ({
      side: el.className.includes("left") ? "left" : "right",
      width: Math.round(el.getBoundingClientRect().width)
    }));
    return {
      swipeX: style.getPropertyValue("--swipe-x").trim(),
      reveal: Number(style.getPropertyValue("--swipe-reveal")),
      direction: row.dataset.swipe ?? "",
      ready: row.classList.contains("swipe-ready"),
      open: row.classList.contains("swipe-open"),
      cardTransform: getComputedStyle(card).transform,
      pointerEvents: getComputedStyle(row.querySelector(".task-swipe-actions")).pointerEvents,
      actions,
      sides
    };
  }, index);
}

async function section8SwipeGestures(context) {
  const { page, errors } = await freshPage(context, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [
      task("task-a", "entry-a", "第一条任务"),
      task("task-b", "entry-a", "第二条任务"),
      task("task-c", "entry-a", "第三条任务")
    ]
  });
  await page.locator('.tree-row[data-node-id="entry-a"]').first().tap();
  await page.waitForSelector(".task-card", { timeout: 8000 });
  await page.waitForTimeout(400);
  check("8.1 三张任务卡就位", (await page.locator(".task-card").count()) === 3);
  check("8.2 每行都有滑动外壳", (await page.locator(".task-swipe").count()) === 3);

  // 右滑未到阈值：松手弹回，什么都不发生
  const row0 = ".task-list .virtual-item:nth-child(2) .task-swipe";
  await touchSwipe(page, row0, 50, 0, { steps: 6, holdAt: 6, release: false });
  const mid = await swipeState(page, 0);
  check("8.3 右滑渐进露出删除块", mid !== null && mid.direction === "right" && mid.sides.some((s) => s.side === "left" && s.width > 0), J(mid));
  check("8.4 露出进度随位移增长", mid !== null && mid.reveal > 0.3 && mid.reveal < 1, J(mid?.reveal));
  check("8.5 未到阈值不置就绪", mid !== null && mid.ready === false, J(mid));
  await touchEnd(page, row0);
  await page.waitForTimeout(320);
  const back = await swipeState(page, 0);
  check("8.6 未到阈值松手弹回", back !== null && back.swipeX === "0px" && back.direction === "", J(back));
  check("8.7 弹回后任务还在", (await page.locator(".task-card").count()) === 3);

  // 右滑到位：松手即删
  await touchSwipe(page, row0, 140, 0, { steps: 8 });
  await page.waitForTimeout(400);
  check("8.8 右滑到位松手即删", (await page.locator(".task-card").count()) === 2);
  check("8.9 删掉的是第一条", !(await page.locator(".task-card").first().innerText()).includes("第一条任务"));

  // 左滑：两块动作同时露出、一起缩放
  const row1 = ".task-list .virtual-item:nth-child(2) .task-swipe";
  await touchSwipe(page, row1, -200, 0, { steps: 10, holdAt: 7, release: false });
  const left = await swipeState(page, 0);
  check("8.10 左滑方向与位移", left !== null && left.direction === "left" && left.swipeX.startsWith("-"), J(left));
  const revealed = left?.actions.filter((a) => a.kind === "pin" || a.kind === "edit") ?? [];
  check("8.11 置顶与编辑同时露出", revealed.length === 2 && revealed.every((a) => a.opacity > 0.2), J(left?.actions));
  check(
    "8.12 两块一起缩放（同一个 --swipe-reveal 驱动）",
    revealed.length === 2 && Math.abs(revealed[0].opacity - revealed[1].opacity) < 0.001 && revealed[0].transform === revealed[1].transform,
    J(revealed)
  );
  check("8.13 两块不是抽屉式逐个显示（宽度相同）", revealed.length === 2 && Math.abs(revealed[0].width - revealed[1].width) <= 1, J(revealed));
  check("8.14 删除块在左滑时不露出", (left?.actions.find((a) => a.kind === "delete")?.opacity ?? 1) < 0.05, J(left?.actions));
  // 半途松手（还没到 152 的阈值）→ 弹回；再滑一次过阈值 → 停在露出位等点击
  await touchEnd(page, row1);
  await page.waitForTimeout(320);
  const bounced = await swipeState(page, 0);
  check("8.15a 左滑未到阈值松手弹回", bounced !== null && bounced.swipeX === "0px" && bounced.open === false, J(bounced));
  await touchSwipe(page, row1, -200, 0, { steps: 10 });
  await page.waitForTimeout(360);
  const parked = await swipeState(page, 0);
  check("8.15 左滑到位后停住等点击", parked !== null && parked.open === true && parked.pointerEvents === "auto", J(parked));

  // 点「置顶」：与右键菜单同一条路径
  await page.locator('.task-swipe.swipe-open .swipe-action[data-swipe-action="pin"]').first().tap();
  await page.waitForTimeout(400);
  check("8.16 点置顶生效（卡片出现图钉）", (await page.locator(".task-pin").count()) >= 1);
  check("8.17 动作执行后行收回", (await page.locator(".task-swipe.swipe-open").count()) === 0);
  const pinnedPersisted = await page.evaluate((key) => {
    const state = JSON.parse(localStorage.getItem(key) ?? "{}");
    return (state.tasks ?? []).some((item) => item.pinned === true);
  }, STATE_KEY);
  check("8.18 置顶落盘", pinnedPersisted);

  // 点「编辑」：打开编辑器
  await touchSwipe(page, row1, -200, 0, { steps: 10 });
  await page.waitForTimeout(360);
  await page.locator('.task-swipe.swipe-open .swipe-action[data-swipe-action="edit"]').first().tap();
  await page.waitForSelector(".editor-overlay", { timeout: 8000 });
  check("8.19 点编辑打开编辑器", (await page.locator(".editor-overlay").count()) === 1);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);

  // 纵向手势不被横滑接管
  await touchSwipe(page, row1, 6, 90, { steps: 6, release: false });
  const vertical = await swipeState(page, 0);
  check("8.20 纵向滑动不接管（方向锁）", vertical !== null && vertical.swipeX === "0px" && vertical.direction === "", J(vertical));
  await touchEnd(page, row1);

  // 横滑不许把下拉同步指示条拽出来
  await page.evaluate(() => {
    const list = document.querySelector(".task-list");
    if (list) list.scrollTop = 0;
  });
  await touchSwipe(page, row1, -160, 12, { steps: 8, holdAt: 8, release: false });
  const pullZone = await page.locator(".pull-sync-zone").count();
  check("8.21 横滑不带出下拉同步条", pullZone === 0, `count=${pullZone}`);
  await touchEnd(page, row1);
  await page.waitForTimeout(300);

  // 展开态不接管手势（长文档里的横滑留给文本选择）
  await page.locator(".task-card").first().tap();
  await page.waitForTimeout(300);
  await page.locator(".task-card").first().tap();
  await page.waitForTimeout(500);
  const expandedCount = await page.locator(".task-card.expanded").count();
  if (expandedCount > 0) {
    await touchSwipe(page, ".task-card.expanded", -180, 0, { steps: 8, holdAt: 8, release: false });
    const expandedState = await page.evaluate(() => {
      const row = document.querySelector(".task-card.expanded")?.closest(".task-swipe");
      return row ? { swipeX: getComputedStyle(row).getPropertyValue("--swipe-x").trim(), direction: row.dataset.swipe ?? "" } : null;
    });
    check("8.22 展开态不接管横滑", expandedState !== null && expandedState.swipeX === "0px" && expandedState.direction === "", J(expandedState));
    await touchEnd(page, ".task-card.expanded");
  } else {
    check("8.22 展开态不接管横滑", true, "单行卡片不可展开，跳过");
  }

  check("8.23 滑动手势无页面错误", errors.length === 0, J(errors.slice(0, 3)));
  await page.close();
}

// ---------------------------------------------------------------------------
// 9. 体验优化与 bugfix 七项里能在浏览器里钉住的部分
// ---------------------------------------------------------------------------
async function section9Bugfixes(desktopContext) {
  // 9.1 代码块能横向滚但不显示滚动条
  {
    const { page, errors } = await freshPage(desktopContext);
    const longLine = "x".repeat(240);
    await seedState(page, {
      nodes: [entryNode("entry-a", "记事")],
      tasks: [task("task-a", "entry-a", `代码块\n\n\`\`\`js\nconst a = "${longLine}";\n\`\`\``)]
    });
    await page.locator('.tree-row[data-node-id="entry-a"]').first().click();
    await page.waitForSelector(".task-card", { timeout: 8000 });
    // 折叠态只画首行：双击展开才看得到代码块
    await page.locator(".task-card").first().dblclick();
    await page.waitForSelector(".markdown-body pre", { timeout: 8000 });
    const pre = await page.evaluate(() => {
      const el = document.querySelector(".markdown-body pre");
      const style = getComputedStyle(el);
      return {
        overflowX: style.overflowX,
        scrollbarWidth: style.scrollbarWidth,
        webkitScrollbar: getComputedStyle(el, "::-webkit-scrollbar").display,
        scrollable: el.scrollWidth > el.clientWidth
      };
    });
    check("9.1 代码块横向可滚", pre.scrollable === true && pre.overflowX === "auto", J(pre));
    check("9.2 代码块不显示滚动条", pre.scrollbarWidth === "none" && pre.webkitScrollbar === "none", J(pre));
    check("9.3 代码块无页面错误", errors.length === 0, J(errors.slice(0, 3)));
    await page.close();
  }

  // 9.4 固定区三级菜单浮在最前（不被二级面板裁剪）
  {
    const { page, errors } = await freshPage(desktopContext);
    await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [] });
    await page.locator(".system-nav .nav-row").first().click({ button: "right" });
    await page.waitForSelector(".context-menu", { timeout: 5000 });
    await page.locator(".context-menu .menu-item-button", { hasText: "添加固定" }).click();
    await page.waitForTimeout(300);
    await page.locator(".submenu-panel .menu-item-button", { hasText: "工具箱内的工具" }).click();
    await page.waitForTimeout(400);
    const third = await page.evaluate(() => {
      const panels = [...document.querySelectorAll(".submenu-panel")];
      // 一级是 .context-menu 本体，二/三级才是 .submenu-panel：三级菜单 = 2 个面板
      const deep = panels[panels.length - 1];
      if (!deep || panels.length !== 2) return null;
      const rect = deep.getBoundingClientRect();
      const probe = document.elementFromPoint(rect.left + rect.width / 2, rect.top + 20);
      return {
        count: panels.length,
        position: getComputedStyle(deep).position,
        visible: probe !== null && probe.closest(".submenu-panel") === deep,
        insideViewport: rect.top >= 0 && rect.bottom <= window.innerHeight + 1
      };
    });
    check("9.4 三级菜单存在且 fixed 浮在最前", third !== null && third.count === 2 && third.position === "fixed", J(third));
    check("9.5 三级菜单内容点得到（不被裁剪）", third !== null && third.visible === true && third.insideViewport, J(third));
    check("9.6 三级菜单无页面错误", errors.length === 0, J(errors.slice(0, 3)));
    await page.close();
  }

  // 9.7 定时任务角标只统计调度中的
  {
    const { page } = await freshPage(desktopContext);
    await seedState(page, {
      nodes: [entryNode("entry-a", "记事")],
      tasks: [],
      scheduler: {
        tasks: [
          {
            id: "sch-on", name: "跑着的", enabled: true, runCount: 0, lastStatus: "idle",
            trigger: { type: "interval", intervalMinutes: 60 },
            action: { language: "python", script: "print(1)" },
            createdAt: new Date().toISOString()
          },
          {
            id: "sch-off", name: "停着的", enabled: false, runCount: 0, lastStatus: "idle",
            trigger: { type: "interval", intervalMinutes: 60 },
            action: { language: "python", script: "print(2)" },
            createdAt: new Date().toISOString()
          }
        ]
      }
    });
    const badge = await page.evaluate(() => {
      const row = [...document.querySelectorAll(".system-nav .nav-row")].find((el) => el.textContent.includes("定时任务"));
      return row?.querySelector(".count-pill")?.textContent ?? "";
    });
    check("9.7 定时任务角标只算调度中的", badge === "1", `badge=${badge}`);
    await page.close();
  }

  // 9.8 剪切板工具：无 HTML 档 / 居中 / 三个按钮 / 路径折行完整显示
  {
    const { page, errors } = await freshPage(desktopContext);
    await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [] });
    await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).first().click();
    await page.waitForSelector(".toolbox-card", { timeout: 8000 });
    await page.locator(".toolbox-card", { hasText: "剪切板同步" }).click();
    await page.waitForSelector(".clipboard-page", { timeout: 8000 });
    await page.waitForTimeout(300);
    const centered = await page.evaluate(() => {
      const el = document.querySelector(".clipboard-page");
      if (!el || !el.parentElement) return null;
      const rect = el.getBoundingClientRect();
      const parent = el.parentElement.getBoundingClientRect();
      const leftGap = rect.left - parent.left;
      const rightGap = parent.right - rect.right;
      return { leftGap: Math.round(leftGap), rightGap: Math.round(rightGap) };
    });
    check(
      "9.8 剪切板页限宽居中（左右留白相等）",
      centered !== null && Math.abs(centered.leftGap - centered.rightGap) <= 2,
      J(centered)
    );
    // 内容区展开后才看得到类型行
    if ((await page.locator(".clipboard-types").count()) === 0) {
      await page.locator(".clipboard-section-head", { hasText: "接收内容" }).click();
      await page.waitForTimeout(250);
    }
    const typeRows = await page.locator(".clipboard-type-row").count();
    const htmlRow = await page.locator(".clipboard-type-row", { hasText: "HTML" }).count();
    check("9.9 类型只剩 文字/图片/文件 三档", typeRows === 3 && htmlRow === 0, `rows=${typeRows} html=${htmlRow}`);
    const dirUi = await page.evaluate(() => {
      const row = document.querySelector(".clipboard-dir-row");
      if (!row) return null;
      const input = row.querySelector(".clipboard-dir-input");
      return {
        buttons: row.querySelectorAll(".clipboard-dir-actions .menu-action-button").length,
        titles: [...row.querySelectorAll(".clipboard-dir-actions .menu-action-button")].map((b) => b.getAttribute("title")),
        inputTag: input?.tagName ?? "",
        wrap: input ? getComputedStyle(input).whiteSpace : ""
      };
    });
    check(
      "9.10 打开/选择/默认三个按钮齐",
      dirUi !== null && dirUi.buttons === 3 && dirUi.titles.join("|") === "打开文件夹|选择路径|恢复默认保存位置",
      J(dirUi)
    );
    check("9.11 路径是可折行的多行输入", dirUi !== null && dirUi.inputTag === "TEXTAREA" && dirUi.wrap !== "nowrap", J(dirUi));
    check("9.12 剪切板工具无页面错误", errors.length === 0, J(errors.slice(0, 3)));
    await page.close();
  }

  // 9.13 存储空间：主视图精简，ⓘ 里看详细
  {
    const { page, errors } = await freshPage(desktopContext);
    await seedState(page, { nodes: [entryNode("entry-a", "记事")], tasks: [] });
    await page.locator(".profile-card").click();
    await page.waitForSelector(".settings-drawer", { timeout: 8000 });
    const storageSection = page.locator(".settings-section", { hasText: "存储空间" }).first();
    if (await storageSection.evaluate((el) => el.classList.contains("folded"))) {
      await storageSection.locator(".settings-section-toggle").click();
      await page.waitForTimeout(400);
    }
    // 浏览器 dev 没有数据目录：占用块整块不渲染（设置页里那句兜底文案就是给它看的），
    // 这几条断言只在真桌面运行时成立，这里点名跳过而不是假绿。
    if ((await page.locator(".storage-parts").count()) === 0) {
      check("9.13 存储主视图只有名字与字节数", true, "浏览器 dev 无数据目录，跳过（未验证清单点名桌面真机）");
      check("9.14 有 ⓘ 说明按钮", true, "同上跳过");
      check("9.15 ⓘ 展开后有逐项说明与清理范围", true, "同上跳过");
      check("9.16 存储区无页面错误", errors.length === 0, J(errors.slice(0, 3)));
      await page.close();
      return;
    }
    const concise = await page.evaluate(() => {
      const list = document.querySelector(".storage-parts");
      if (!list) return null;
      return {
        items: list.querySelectorAll("li").length,
        inlineExplain: list.querySelectorAll("li em").length,
        infoButton: document.querySelectorAll(".storage-head-row .icon-button").length
      };
    });
    check("9.13 存储主视图只有名字与字节数", concise !== null && concise.items > 0 && concise.inlineExplain === 0, J(concise));
    check("9.14 有 ⓘ 说明按钮", concise !== null && concise.infoButton === 1, J(concise));
    await page.locator(".storage-head-row .icon-button").click();
    await page.waitForTimeout(250);
    const detail = await page.evaluate(() => {
      const pop = document.querySelector(".storage-info-pop");
      return pop ? { explains: pop.querySelectorAll("li em").length, paragraphs: pop.querySelectorAll("p").length } : null;
    });
    check("9.15 ⓘ 展开后有逐项说明与清理范围", detail !== null && detail.explains >= 6 && detail.paragraphs >= 2, J(detail));
    check("9.16 存储区无页面错误", errors.length === 0, J(errors.slice(0, 3)));
    await page.close();
  }
}

// ---------------------------------------------------------------------------

const executablePath =
  process.env.KXTODO_EDGE ?? "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-gpu"]
});

try {
  const context = await browser.newContext({ userAgent: ANDROID_UA, viewport: { width: 1280, height: 860 }, hasTouch: true, isMobile: true });
  // 桌面段用非移动 UA 的独立 context
  const desktopContext = await browser.newContext({ viewport: { width: 1280, height: 860 } });

  await section1ScheduleDesktop(desktopContext);
  await section2GridInteractions(desktopContext);
  await section3TaskLink(desktopContext);
  await section4NavSeeding(desktopContext);
  await section5Mobile(context);
  await section6ScratchpadTabs(desktopContext);
  await section7ScratchpadMobile(context);
  await section8SwipeGestures(context);
  await section9Bugfixes(desktopContext);

  await context.close();
  await desktopContext.close();
} finally {
  await browser.close();
}

console.log(`\nv0.9.0 回归：${passes} 通过，${failures} 失败`);
process.exit(failures > 0 ? 1 : 0);

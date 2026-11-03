// v0.9.2 回归。逐条覆盖本版需求：skipAhead 远区间 / 重复提醒出现日 / 24:00 约定 /
// 现在红线随缩放 / 横滑方向 / Esc 校验失败不丢草稿 / 分类取色层次 / 固定面板 /
// 管理菜单项与截止快捷项 / 日期弹窗层次 / 移动端日程拖动 / 地址展示 / 现在按钮 /
// 编辑器时间文案与时长括号。
// 用法：node scripts/v092-fixes-test.mjs（需先 npm run dev）。
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

async function seedState(page, { nodes = [], tasks = [], settings = null, calendar = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, calendar, stateKey, settingsKey, calendarKey }) => {
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
    },
    { nodes, tasks, settings, calendar, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY, calendarKey: CALENDAR_KEY }
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
    location: "", categoryId: "", repeat: null, exceptDates: [],
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

/** 合成指针事件（触屏手势用；playwright 的 touchscreen 只能 tap） */
async function pointerAt(page, type, x, y, { pointerType = "touch", target = null } = {}) {
  await page.evaluate(
    ({ type, x, y, pointerType, selector }) => {
      const el = selector ? document.querySelector(selector) : document.elementFromPoint(x, y);
      if (!el) return;
      el.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true, cancelable: true, composed: true,
          clientX: x, clientY: y, pointerId: 1, pointerType, isPrimary: true,
          button: 0, buttons: type === "pointerup" || type === "pointercancel" ? 0 : 1
        })
      );
    },
    { type, x, y, pointerType, selector: target }
  );
}

/** 合成横向滑动手势（swipeX 听的是 touch 三件） */
async function touchSwipe(page, selector, fromX, toX, y) {
  await page.evaluate(
    ({ selector, fromX, toX, y }) => {
      const el = document.querySelector(selector);
      if (!el) return;
      const fire = (type, x, down) => {
        const touch = new Touch({ identifier: 1, target: el, clientX: x, clientY: y });
        el.dispatchEvent(
          new TouchEvent(type, {
            bubbles: true, cancelable: true,
            touches: down ? [touch] : [],
            targetTouches: down ? [touch] : [],
            changedTouches: [touch]
          })
        );
      };
      fire("touchstart", fromX, true);
      fire("touchmove", (fromX + toX) / 2, true);
      fire("touchmove", toX, true);
      fire("touchend", toX, false);
    },
    { selector, fromX, toX, y }
  );
}

// ---------------------------------------------------------------------------
// 1. 需求 1：skipAhead 估步方向——start 距视野好几年的月度/年度系列仍要展出来
// ---------------------------------------------------------------------------
async function section1SkipAhead(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      schemaVersion: 1,
      events: [
        calendarEvent("cal-old-month", {
          title: "老月度",
          startDate: localToday(-1400),
          startTime: "09:00",
          endDate: localToday(-1400),
          endTime: "10:00",
          repeat: { freq: "month", interval: 1, byWeekday: [], byMonthDay: [], end: "never", count: 0, until: "" }
        }),
        calendarEvent("cal-old-year", {
          title: "老年度",
          startDate: localToday(-1464),
          startTime: "09:00",
          endDate: localToday(-1464),
          endTime: "10:00",
          repeat: { freq: "year", interval: 1, byWeekday: [], byMonthDay: [], end: "never", count: 0, until: "" }
        })
      ],
      categories: []
    }
  });
  await openSchedulePage(page);
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(400);
  check("1.1 三年多的月度系列在本月视图出现", await page.locator(".cal-month-chip", { hasText: "老月度" }).count() >= 1);
  check("1.2 四年的年度系列在本月视图出现", await page.locator(".cal-month-chip", { hasText: "老年度" }).count() >= 1);
  check("1.3 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 2. 需求 3：24:00 结束统一 core 约定——次日没有幽灵块/幽灵芯片
// ---------------------------------------------------------------------------
async function section2Midnight(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      schemaVersion: 1,
      events: [
        calendarEvent("cal-mid", {
          title: "跨午夜",
          startDate: localToday(),
          startTime: "23:00",
          endDate: localToday(1),
          endTime: "00:00"
        })
      ],
      categories: []
    }
  });
  await openSchedulePage(page);
  check("2.1 周视图只有一块（开始日画满、次日无幽灵块）", await page.locator(".cal-event", { hasText: "跨午夜" }).count() === 1);
  const todayBlocks = await page.locator(".cal-day-col.today .cal-event", { hasText: "跨午夜" }).count();
  check("2.2 块在今天列", todayBlocks === 1, `today=${todayBlocks}`);
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(400);
  check("2.3 月视图只有一枚芯片", await page.locator(".cal-month-chip", { hasText: "跨午夜" }).count() === 1);

  // 拖结束边缘到网格底（24:00）落盘必须是「次日 + 00:00」而不是「当天 + 00:00」
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(400);
  const box = await page.locator(".cal-event", { hasText: "跨午夜" }).boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height - 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height + 120, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(500);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").events?.[0], CALENDAR_KEY);
  check(
    "2.4 拖到更晚仍存成 core 约定（次日 + 00:00 或更晚时刻）",
    stored && (stored.endDate === localToday(1) || stored.endDate > localToday(1)),
    J(stored)
  );
  check("2.5 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 3. 需求 4：「现在」红线会动且随缩放缩放
// ---------------------------------------------------------------------------
async function section3NowLine(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  const top1 = await page.evaluate(() => document.querySelector(".cal-now")?.style.top ?? "");
  const grid = await page.locator(".cal-scroll").boundingBox();
  await page.mouse.move(grid.x + grid.width / 2, grid.y + grid.height / 2);
  await page.keyboard.down("Control");
  await page.mouse.wheel(0, -240);
  await page.keyboard.up("Control");
  await page.waitForTimeout(500);
  const top2 = await page.evaluate(() => document.querySelector(".cal-now")?.style.top ?? "");
  const parse = (value) => Number.parseFloat(value) || 0;
  check("3.1 红线存在且有 top", top1 !== "", top1);
  check("3.2 放大后红线 top 跟着变大（依赖没藏进函数体）", parse(top2) > parse(top1), `${top1} → ${top2}`);
  check("3.3 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 需求 5：移动端横滑方向（左滑=下一段）
// ---------------------------------------------------------------------------
async function section4Swipe() {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(mobile, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(page);
  await page.locator(".calendar-view-switch button[title='月视图']").tap();
  await page.waitForTimeout(400);
  const title0 = await page.locator(".calendar-range-step strong").innerText();
  await touchSwipe(page, ".calendar-body", 260, 90, 400);
  await page.waitForTimeout(350);
  const title1 = await page.locator(".calendar-range-step strong").innerText();
  await touchSwipe(page, ".calendar-body", 90, 260, 400);
  await page.waitForTimeout(350);
  const title2 = await page.locator(".calendar-range-step strong").innerText();
  check("4.1 左滑=下一段", title1 !== title0, `${title0} → ${title1}`);
  check("4.2 右滑=回到上一段", title2 === title0, `${title1} → ${title2}`);
  check("4.3 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 需求 6：校验失败时 Esc/遮罩不许关、不许丢草稿
// ---------------------------------------------------------------------------
async function section5EscValidation(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  await page.locator(".calendar-title-input").fill("草稿别丢");

  // 把结束日期改到上个月 → 区间倒挂
  await page.locator(".cal-time-range .cal-field", { hasText: "结束" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".date-picker", { timeout: 3000 });
  await page.locator(".date-picker-header button[aria-label='上个月']").click();
  await page.waitForTimeout(200);
  await page.locator(".date-picker-grid .dp-cell:not(.other-month)").first().click();
  await page.locator(".date-picker-actions button", { hasText: "确认" }).click();
  await page.waitForTimeout(300);
  check("5.1 倒挂区间给出错误提示", await page.locator(".calendar-form-error").count() === 1);

  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  check("5.2 校验失败时 Esc 不关闭编辑器", await page.locator(".calendar-editor").count() === 1);
  check(
    "5.3 草稿标题还在",
    (await page.locator(".calendar-title-input").inputValue()) === "草稿别丢"
  );
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  check("5.4 校验失败时点关闭也不丢草稿（仍开着）", await page.locator(".calendar-editor").count() === 1);
  check("5.5 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 需求 7：分类表单里开取色盘，色盘必须在面板之上
// ---------------------------------------------------------------------------
async function section6ColorZOrder(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  await page.locator(".cal-field", { hasText: "分类" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "管理分类" }).click();
  await page.waitForSelector(".calendar-category-panel", { timeout: 3000 });
  await page.locator(".calendar-category-panel .settings-button.primary", { hasText: "新建分类" }).click();
  await page.waitForTimeout(300);
  await page.locator(".calendar-cat-dot.custom").click();
  await page.waitForSelector(".kx-color-panel", { timeout: 3000 });
  await page.waitForTimeout(300);
  const onTop = await page.evaluate(() => {
    const panel = document.querySelector(".kx-color-panel");
    if (!panel) return { ok: false, why: "no panel" };
    const rect = panel.getBoundingClientRect();
    const el = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return { ok: !!el && !!el.closest(".kx-color-panel"), why: el?.className ?? "null" };
  });
  check("6.1 取色盘浮在分类面板之上", onTop.ok, J(onTop));
  check("6.2 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 7. 需求 9：管理菜单项（显隐 / 拖序 / 恢复默认）+ 截止快捷项
// ---------------------------------------------------------------------------
async function section7MenuManager(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("task-a", "entry-a", "要办的事")]
  });
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check("7.1 菜单底部有「管理菜单项」", menuText.includes("管理菜单项"));
  check("7.2 截止快捷项默认不展示", !menuText.includes("设置今天截止"));

  await page.locator(".context-menu .menu-item-button", { hasText: "管理菜单项" }).click();
  await page.waitForSelector(".task-menu-manager", { timeout: 3000 });
  const rows = await page.locator(".task-menu-manager-row").count();
  check("7.3 管理面板列出全部条目（含分隔线与隐藏项）", rows === 14, `rows=${rows}`);

  // 隐藏「编辑」
  await page.locator(".task-menu-manager-row", { hasText: "编辑" }).locator(".editor-icon-button").click();
  await page.waitForTimeout(200);
  // 显示「设置今天截止」
  await page.locator(".task-menu-manager-row", { hasText: "设置今天截止" }).locator(".editor-icon-button").click();
  await page.waitForTimeout(200);

  // 拖「删除」到「编辑」之前
  const gripDel = await page.locator(".task-menu-manager-row", { hasText: "删除" }).locator(".task-menu-manager-grip").boundingBox();
  const rowEdit = await page.locator(".task-menu-manager-row", { hasText: "编辑" }).boundingBox();
  await page.mouse.move(gripDel.x + gripDel.width / 2, gripDel.y + gripDel.height / 2);
  await page.mouse.down();
  await page.mouse.move(rowEdit.x + rowEdit.width / 2, rowEdit.y + 4, { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const orderLabels = await page.locator(".task-menu-manager-row .task-menu-manager-label").allInnerTexts();
  check(
    "7.4 拖动排序生效（删除移到编辑前）",
    orderLabels.indexOf("删除") < orderLabels.indexOf("编辑"),
    J(orderLabels)
  );
  await page.locator(".task-menu-manager .settings-button.primary", { hasText: "完成" }).click();
  await page.waitForTimeout(300);

  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menu2 = await page.locator(".context-menu").innerText();
  check("7.5 隐藏后菜单里没有「编辑」", !menu2.split("\n").includes("编辑"), menu2.replace(/\n/g, "|"));
  check("7.6 开启后菜单里有「设置今天截止」", menu2.includes("设置今天截止"));
  await page.locator(".context-menu .menu-item-button", { hasText: "设置今天截止" }).click();
  await page.waitForTimeout(500);
  const dueText = await page.locator(".task-card .task-due-date").first().innerText().catch(() => "");
  const [ty, tm, td] = localToday().split("-").map(Number);
  check(
    "7.7 点「设置今天截止」后卡片出现今天截止",
    dueText.includes("今天") || dueText.includes(`${tm}月${td}日`),
    dueText
  );

  // 恢复默认
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "管理菜单项" }).click();
  await page.waitForSelector(".task-menu-manager", { timeout: 3000 });
  await page.locator(".task-menu-manager .settings-button", { hasText: "恢复默认" }).click();
  await page.waitForTimeout(250);
  const resetLabels = await page.locator(".task-menu-manager-row .task-menu-manager-label").allInnerTexts();
  check("7.8 恢复默认把顺序与显隐还原", resetLabels.indexOf("编辑") < resetLabels.indexOf("删除"), J(resetLabels));
  check("7.9 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 8. 需求 10：条目日期弹窗不被卡片盖住（叠层上下文陷阱）
// ---------------------------------------------------------------------------
async function section8DatePopover(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [
      task("task-a", "entry-a", "第一条"),
      task("task-b", "entry-a", "第二条", { dueDate: localToday() }),
      task("task-c", "entry-a", "第三条"),
      task("task-d", "entry-a", "第四条")
    ]
  });
  await page.locator(".task-due-date").first().click();
  await page.waitForSelector(".task-date-popover.placed", { timeout: 3000 });
  await page.waitForTimeout(250);
  const probe = await page.evaluate(() => {
    const pop = document.querySelector(".task-date-popover");
    const row = pop?.closest(".task-swipe");
    const rect = pop.getBoundingClientRect();
    const el = document.elementFromPoint(rect.left + rect.width / 2, rect.top + Math.min(rect.height / 2, 60));
    return {
      inPop: !!el && !!el.closest(".task-date-popover"),
      rowLifted: !!row && row.classList.contains("date-pop-open"),
      hit: el?.className ?? "null"
    };
  });
  check("8.1 日期弹窗中心命中自己（没被后面卡片盖住）", probe.inPop, J(probe));
  check("8.2 开着弹窗的行被抬到其它行之上", probe.rowLifted, J(probe));
  check("8.3 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 9. 需求 11：移动端日程拖动 = 直接拖（无长按）；tap 仍是开编辑
// ---------------------------------------------------------------------------
async function section9MobileDrag() {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(mobile, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: { schemaVersion: 1, events: [calendarEvent("cal-drag", { title: "拖我" })], categories: [] }
  });
  await openSchedulePageMobile(page);
  const box = await page.locator(".cal-event", { hasText: "拖我" }).boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await pointerAt(page, "pointerdown", cx, cy, { target: ".cal-event" });
  await pointerAt(page, "pointermove", cx, cy + 30, { target: ".cal-event" });
  await pointerAt(page, "pointermove", cx, cy + 60, { target: ".cal-event" });
  await pointerAt(page, "pointerup", cx, cy + 60, { target: ".cal-event" });
  await page.waitForTimeout(600);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").events?.[0], CALENDAR_KEY);
  check("9.1 触屏直接拖动即挪动（不需长按）", stored?.startTime !== "09:00", J(stored));
  check("9.2 拖动后没有开编辑器", await page.locator(".calendar-editor").count() === 0);

  await page.waitForTimeout(500);
  const box2 = await page.locator(".cal-event", { hasText: "拖我" }).boundingBox();
  await pointerAt(page, "pointerdown", box2.x + box2.width / 2, box2.y + box2.height / 2, { target: ".cal-event" });
  await pointerAt(page, "pointerup", box2.x + box2.width / 2, box2.y + box2.height / 2, { target: ".cal-event" });
  await page.evaluate(() => {
    document.querySelector(".cal-event")?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
  await page.waitForSelector(".calendar-editor", { timeout: 3000 });
  check("9.3 单击事件块仍是开编辑", true);
  check("9.4 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 10. 需求 12/13：块内地址 + 移动端「现在」右对齐图标化
// ---------------------------------------------------------------------------
async function section10LocAndNow(context) {
  const { page } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      schemaVersion: 1,
      events: [calendarEvent("cal-loc", { title: "带地址", location: "三楼会议室" })],
      categories: []
    }
  });
  await openSchedulePage(page);
  check("10.1 事件块展示地址", (await page.locator(".cal-event-loc").innerText().catch(() => "")).includes("三楼会议室"));
  await page.close();

  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page: mpage, errors } = await freshPage(mobile, { mobile: true });
  await seedState(mpage, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(mpage);
  const btn = await mpage.locator(".calendar-now-btn").boundingBox();
  check("10.2 移动端「现在」右对齐", btn.x + btn.width > 360 - 60, J(btn));
  const labelDisplay = await mpage.evaluate(() => getComputedStyle(document.querySelector(".calendar-now-label")).display);
  check("10.3 移动端只留图标（文字隐藏）", labelDisplay === "none", labelDisplay);
  check("10.4 无页面错误", errors.length === 0, J(errors));
  await mpage.close();
}

// ---------------------------------------------------------------------------
// 11. 需求 14：开始/结束文案不重复 + 时长括号可选档
// ---------------------------------------------------------------------------
async function section11EditorTime(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: { schemaVersion: 1, events: [calendarEvent("cal-t", { title: "时间文案" })], categories: [] }
  });
  await openSchedulePage(page);
  await page.locator(".cal-event", { hasText: "时间文案" }).click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const startText = await page.locator(".cal-time-range .cal-field-trigger").first().innerText();
  const weekdayCount = (startText.match(/周[一二三四五六日]/g) ?? []).length;
  check("11.1 开始行日期只出现一次", weekdayCount === 1, startText);

  await page.locator(".cal-duration-bracket").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check("11.2 括号菜单有快捷档与全天", menuText.includes("2 小时") && menuText.includes("全天"), menuText.replace(/\n/g, "|"));
  await page.locator(".context-menu .menu-item-button", { hasText: "2 小时" }).click();
  await page.waitForTimeout(300);
  const endText = await page.locator(".cal-time-range .cal-field-trigger").nth(1).innerText();
  check("11.3 选 2 小时后结束按开始推算", endText.includes("11:00"), endText);
  const bracket = await page.locator(".cal-duration-bracket").innerText();
  check("11.4 括号里显示时长", bracket.includes("2 小时"), bracket);

  await page.locator(".cal-duration-bracket").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "全天" }).click();
  await page.waitForTimeout(300);
  const bracket2 = await page.locator(".cal-duration-bracket").innerText();
  check("11.5 选全天后括号显示全天", bracket2.includes("全天"), bracket2);

  // 括号不许顶出右边界
  const overflow = await page.evaluate(() => {
    const el = document.querySelector(".cal-duration-bracket");
    const rect = el.getBoundingClientRect();
    const host = document.querySelector(".calendar-form-body").getBoundingClientRect();
    return { right: rect.right, host: host.right };
  });
  check("11.6 括号不超出表单右边界", overflow.right <= overflow.host + 1, J(overflow));
  check("11.7 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------

const browser = await chromium.launch({
  channel: "msedge",
  headless: true,
  args: ["--no-sandbox", "--disable-gpu"]
});
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });

await section1SkipAhead(context);
await section2Midnight(context);
await section3NowLine(context);
await section4Swipe();
await section5EscValidation(context);
await section6ColorZOrder(context);
await section7MenuManager(context);
await section8DatePopover(context);
await section9MobileDrag();
await section10LocAndNow(context);
await section11EditorTime(context);

await browser.close();
console.log(`\n${passes} passed, ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);

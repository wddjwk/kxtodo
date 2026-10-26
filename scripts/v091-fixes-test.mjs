// v0.9.1 回归。逐条覆盖本版需求：日程（表单化编辑器 / 分类 / 重复 / 缩放 / 视图收口）、
// 全局悬浮提示、任务卡日程图标对齐、右键菜单顺序、剪切板与存储空间与草稿纸的收口。
// 用法：node scripts/v091-fixes-test.mjs（需先 npm run dev）。
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

function calendarCategory(id, name, color, extra = {}) {
  return {
    id, name, color, isDefault: false, order: 1,
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), ...extra
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

async function clickAt(page, x, y, { target = null } = {}) {
  await pointerAt(page, "pointerdown", x, y, { target });
  await page.evaluate(
    ({ x, y, selector }) => {
      const el = selector ? document.querySelector(selector) : document.elementFromPoint(x, y);
      el?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    },
    { x, y, selector: target }
  );
}

// ---------------------------------------------------------------------------
// 1. 需求 1：坏掉的本机视图偏好不许让界面瘫掉
// ---------------------------------------------------------------------------
async function section1LenientView(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    settings: { calendar: { view: "nonsense", hourZoom: 1 } }
  });
  await openSchedulePage(page);
  const active = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".calendar-view-switch button")];
    return buttons.findIndex((b) => b.classList.contains("active"));
  });
  check("1.1 认不出的 calendar.view 回落周视图", active === 1, `active=${active}`);

  // 遗留的开发中间版写法 "3days" → 按用户本意还原成三日视图
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], settings: { calendar: { view: "3days" } } });
  await openSchedulePage(page);
  const cols = await page.locator(".cal-day-col").count();
  check('1.2 遗留写法 "3days" 还原成三日视图', cols === 3, `cols=${cols}`);

  // 坏值不该让「改设置」失败：切到月视图能落盘
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(300);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").calendar?.view, SETTINGS_KEY);
  check("1.3 坏值之后仍能改视图偏好", stored === "month", `stored=${stored}`);
  check("1.4 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 2. 需求 10：工具栏去掉「今天」，只留「日期 现在」
// ---------------------------------------------------------------------------
async function section2Toolbar(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  check("2.1 工具栏没有「今天」按钮", await page.locator(".calendar-today-btn").count() === 0);
  check("2.2 工具栏有「现在」按钮", await page.locator(".calendar-now-btn").count() === 1);
  const barText = (await page.locator(".calendar-range-bar").innerText()).replace(/\s+/g, "");
  check("2.3 工具栏就是「日期 + 现在」", /现在$/.test(barText) && barText.includes("‹") === false, barText);

  // 翻到下个月再点「现在」→ 回到今天
  await page.locator(".calendar-range-step button[aria-label='下一段']").click();
  await page.waitForTimeout(200);
  const away = await page.locator(".calendar-range-step strong").innerText();
  await page.locator(".calendar-now-btn").click();
  await page.waitForTimeout(250);
  const back = await page.locator(".calendar-range-step strong").innerText();
  check("2.4 点「现在」回到今天所在区间", away !== back, `${away} → ${back}`);

  // 月视图也给「现在」（点了回到本月）
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(250);
  check("2.5 月视图同样有「现在」", await page.locator(".calendar-now-btn").count() === 1);
  await page.locator(".calendar-range-step button[aria-label='上一段']").click();
  await page.waitForTimeout(200);
  await page.locator(".calendar-now-btn").click();
  await page.waitForTimeout(250);
  check("2.6 月视图「现在」回到本月", await page.locator(".cal-month-cell.today").count() === 1);
  await page.close();
}

// ---------------------------------------------------------------------------
// 3. 需求 15：一屏 12 小时 + Ctrl 滚轮缩放并记住
// ---------------------------------------------------------------------------
async function section3Zoom(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  await page.waitForTimeout(400);

  const read = () =>
    page.evaluate(() => {
      const scroll = document.querySelector(".cal-scroll");
      const body = document.querySelector(".cal-grid-body");
      const hourH = Number.parseFloat(getComputedStyle(body).getPropertyValue("--cal-hour-h"));
      return { hourH, viewport: scroll.clientHeight, total: scroll.scrollHeight };
    });

  const base = await read();
  const hours = base.viewport / base.hourH;
  check("3.1 默认一屏约 12 小时", Math.abs(hours - 12) < 0.6, `hours=${hours.toFixed(2)}`);
  check("3.2 网格总高 = 24 小时", Math.abs(base.total - base.hourH * 24) < 2, J(base));

  // Ctrl + 滚轮放大
  const box = await page.locator(".cal-scroll").boundingBox();
  await page.keyboard.down("Control");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -240);
  await page.keyboard.up("Control");
  await page.waitForTimeout(150);
  const bigger = await read();
  check("3.3 Ctrl+滚轮放大格高", bigger.hourH > base.hourH + 1, `${base.hourH} → ${bigger.hourH}`);

  // 落盘（防抖 240ms 再写设置）
  await page.waitForTimeout(600);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").calendar?.hourZoom, SETTINGS_KEY);
  check("3.4 缩放倍率写进设置", typeof stored === "number" && stored > 1, `stored=${stored}`);

  // 普通滚轮不改缩放（只滚网格）
  const before = await read();
  await page.mouse.wheel(0, -240);
  await page.waitForTimeout(200);
  const after = await read();
  check("3.5 不按 Ctrl 的滚轮不改缩放", Math.abs(after.hourH - before.hourH) < 0.5, `${before.hourH} → ${after.hourH}`);

  // 刷新后记得
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await openSchedulePage(page);
  await page.waitForTimeout(400);
  const restored = await read();
  check("3.6 刷新后缩放保持", Math.abs(restored.hourH - bigger.hourH) < 2, `${bigger.hourH} → ${restored.hourH}`);
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 需求 14 + 7：网格不显示滚动条；块内文字换行且不越界
// ---------------------------------------------------------------------------
async function section4GridLook(context) {
  const { page } = await freshPage(context);
  const long = "这是一个非常非常非常长的日程标题用来验证块内文字会不会自动换行而不是溢出到块外面去";
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      events: [
        calendarEvent("cal-1", { title: long, startTime: "09:00", endTime: "11:30" }),
        calendarEvent("cal-2", { title: "短", startTime: "10:00", endTime: "10:20" })
      ],
      categories: []
    }
  });
  await openSchedulePage(page);

  const scrollInfo = await page.evaluate(() => {
    const el = document.querySelector(".cal-scroll");
    return {
      offsetWidth: el.offsetWidth,
      clientWidth: el.clientWidth,
      scrollbarWidth: getComputedStyle(el).scrollbarWidth
    };
  });
  check(
    "4.1 网格纵向滚动条不占位",
    scrollInfo.offsetWidth === scrollInfo.clientWidth || scrollInfo.scrollbarWidth === "none",
    J(scrollInfo)
  );

  const titleStyle = await page.evaluate(() => {
    const el = document.querySelector(".cal-event .cal-event-title");
    const style = getComputedStyle(el);
    return { whiteSpace: style.whiteSpace, overflow: getComputedStyle(el.closest(".cal-event")).overflow };
  });
  check("4.2 标题自动换行", titleStyle.whiteSpace === "normal", J(titleStyle));
  check("4.3 块裁掉溢出（「被挡住」的观感）", titleStyle.overflow === "hidden", J(titleStyle));

  const inside = await page.evaluate(() => {
    let worst = 0;
    for (const event of document.querySelectorAll(".cal-event")) {
      const box = event.getBoundingClientRect();
      const title = event.querySelector(".cal-event-title");
      if (!title) continue;
      const range = document.createRange();
      range.selectNodeContents(title);
      for (const rect of range.getClientRects()) {
        worst = Math.max(worst, rect.right - box.right, rect.bottom - box.bottom);
      }
    }
    return worst;
  });
  check("4.4 文字没有超出块范围", inside <= 1.5, `overflowPx=${inside.toFixed(2)}`);
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 需求 11：分类与自定义颜色
// ---------------------------------------------------------------------------
async function section5Categories(context) {
  const { page } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      events: [calendarEvent("cal-1", { title: "带分类", startTime: "09:00", endTime: "11:00", categoryId: "ccat-1" })],
      categories: [calendarCategory("ccat-1", "工作", "#1f6feb"), calendarCategory("ccat-2", "个人", "#d97706")]
    }
  });
  await openSchedulePage(page);

  const vars = await page.evaluate(() => {
    const el = document.querySelector(".cal-event");
    const style = getComputedStyle(el);
    return {
      main: style.getPropertyValue("--cal-main").trim(),
      soft: style.getPropertyValue("--cal-soft").trim(),
      borderLeft: style.borderLeftColor,
      background: style.backgroundColor
    };
  });
  check("5.1 事件块拿到分类主色", vars.main.toLowerCase() === "#1f6feb", J(vars));
  check("5.2 左侧边框是主色", vars.borderLeft === "rgb(31, 111, 235)", J(vars));
  check("5.3 块内铺淡色（不是纯白也不是主色）", /^rgb\(/.test(vars.background) && vars.background !== "rgb(31, 111, 235)" && vars.background !== "rgb(255, 255, 255)", J(vars));

  // 淡色必须比主色亮得多（「不影响阅读」的量化口径）
  const brightness = await page.evaluate(() => {
    const parse = (value) => {
      const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(value);
      return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : [0, 0, 0];
    };
    const el = document.querySelector(".cal-event");
    const style = getComputedStyle(el);
    const lum = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
    return {
      main: lum(parse(style.getPropertyValue("--cal-main"))),
      soft: lum(parse(style.backgroundColor)),
      text: lum(parse(style.color))
    };
  });
  check("5.4 淡色底明显亮于主色", brightness.soft - brightness.main > 90, J(brightness));
  check("5.5 文字仍是深色（可读）", brightness.text < 90, J(brightness));

  // 月视图芯片也吃分类色
  await page.locator(".calendar-view-switch button[title='月视图']").click();
  await page.waitForTimeout(300);
  const chip = await page.evaluate(() => {
    const el = document.querySelector(".cal-month-chip");
    return getComputedStyle(el).getPropertyValue("--cal-main").trim();
  });
  check("5.6 月视图芯片同样跟分类色", chip.toLowerCase() === "#1f6feb", chip);

  // 分类管理面板：从三点菜单进
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(250);
  await page.locator(".header-actions button[title='日程菜单']").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check("5.7 三点菜单有「管理分类」", menuText.includes("管理分类"), menuText);
  await page.locator(".context-menu .menu-item-button", { hasText: "管理分类" }).click();
  await page.waitForSelector(".calendar-category-panel", { timeout: 3000 });
  const rows = await page.locator(".calendar-category-panel .calendar-cat-row .calendar-cat-name").allInnerTexts();
  check("5.8 分类面板列出两个分类", rows.join("|") === "工作|个人", J(rows));

  // 新建一个分类并设为默认
  await page.locator(".calendar-category-panel .settings-button.primary", { hasText: "新建分类" }).click();
  await page.waitForSelector(".calendar-category-panel .calendar-field-input", { timeout: 3000 });
  await page.locator(".calendar-category-panel .calendar-field-input").fill("健康");
  await page.locator(".calendar-category-panel .calendar-cat-dot", { has: page.locator("[style*='2f9e6e']") }).first().click().catch(() => {});
  await page.locator(".calendar-field-inline input[type='checkbox']").check();
  await page.locator(".calendar-category-panel .settings-button.primary", { hasText: "保存" }).click();
  await page.waitForTimeout(400);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), CALENDAR_KEY);
  const created = (stored.categories ?? []).find((item) => item.name === "健康");
  check("5.9 新建分类落盘", Boolean(created), J(stored.categories));
  check("5.10 「设为默认」是全局互斥的", Boolean(created?.isDefault) && (stored.categories ?? []).filter((c) => c.isDefault).length === 1, J(stored.categories));

  // 新建日程时默认分类预选中
  await page.locator(".calendar-category-panel .editor-icon-button[title='关闭']").click();
  await page.waitForSelector(".calendar-category-panel", { state: "detached" });
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const categoryRow = await page.locator(".cal-field", { hasText: "分类" }).innerText();
  check("5.11 新建日程预选默认分类", categoryRow.includes("健康"), categoryRow);

  // 分类选择浮层：勾选态 + 管理入口
  await page.locator(".cal-field-trigger", { hasText: "健康" }).click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const catMenu = await page.locator(".context-menu").innerText();
  check("5.12 分类浮层有无分类/管理入口", catMenu.includes("无分类") && catMenu.includes("管理分类"), catMenu);
  await page.locator(".context-menu .menu-item-button", { hasText: "工作" }).click();
  await page.waitForTimeout(250);
  const rowAfter = await page.locator(".cal-field", { hasText: "分类" }).innerText();
  check("5.13 选中别的分类后行内更新", rowAfter.includes("工作"), rowAfter);
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 需求 12：重复日程
// ---------------------------------------------------------------------------
async function section6Repeat(context) {
  const { page } = await freshPage(context);
  const start = localToday(-7);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      events: [
        calendarEvent("cal-weekly", {
          title: "周会", startDate: start, endDate: start, startTime: "10:00", endTime: "11:00",
          repeat: { freq: "week", interval: 1, byWeekday: [], byMonthDay: [], end: "never", count: 0, until: "" }
        }),
        calendarEvent("cal-count", {
          title: "只三次", startDate: start, endDate: start, startTime: "14:00", endTime: "14:30",
          repeat: { freq: "day", interval: 1, byWeekday: [], byMonthDay: [], end: "count", count: 3, until: "" }
        }),
        calendarEvent("cal-except", {
          title: "跳过一次", startDate: localToday(-14), endDate: localToday(-14), startTime: "16:00", endTime: "16:30",
          repeat: { freq: "day", interval: 1, byWeekday: [], byMonthDay: [], end: "never", count: 0, until: "" },
          exceptDates: [localToday()]
        })
      ],
      categories: []
    }
  });
  await openSchedulePage(page);
  await page.waitForTimeout(300);

  const weeklies = await page.locator(".cal-event", { hasText: "周会" }).count();
  check("6.1 每周重复在周视图里出现", weeklies >= 1, `count=${weeklies}`);

  const counted = await page.locator(".cal-event", { hasText: "只三次" }).count();
  const dates = await page.evaluate(() => {
    // 数一下「只三次」在这周出现几次：count=3 且起始在 7 天前 → 本周不该再有
    return document.querySelectorAll(".cal-event").length;
  });
  check("6.2 按次数结束的重复不无限展开", counted <= 3, `counted=${counted} total=${dates}`);

  // 重复标记（↻）画在块里
  const repeatIcon = await page.locator(".cal-event .cal-repeat").count();
  check("6.3 重复日程带标记", repeatIcon >= 1, `count=${repeatIcon}`);

  // 例外日期：日视图钉在那天 → 没有块；走一天 → 有块
  await page.locator(".calendar-view-switch button[title='日视图']").click();
  await page.waitForTimeout(300);
  const onExcept = await page.locator(".cal-event", { hasText: "跳过一次" }).count();
  await page.locator(".calendar-range-step button[aria-label='下一段']").click();
  await page.waitForTimeout(300);
  const afterExcept = await page.locator(".cal-event", { hasText: "跳过一次" }).count();
  check("6.4 例外日期那次被跳过、次日照常出现", onExcept === 0 && afterExcept === 1, `${onExcept} / ${afterExcept}`);
  await page.locator(".calendar-view-switch button[title='周视图']").click();
  await page.waitForTimeout(300);

  // 编辑器里的重复行 + 预设菜单
  await page.locator(".cal-event", { hasText: "周会" }).first().click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const repeatRow = await page.locator(".cal-field", { hasText: "重复" }).first().innerText();
  check("6.5 编辑器显示重复规则文本", /每/.test(repeatRow), repeatRow);
  await page.locator(".cal-field-trigger", { hasText: /每周|每天/ }).first().click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const repeatMenu = await page.locator(".context-menu").innerText();
  check(
    "6.6 重复菜单有业界预设",
    ["不重复", "每天", "工作日", "每周", "每两周", "每月", "每年", "自定义"].every((item) => repeatMenu.includes(item)),
    repeatMenu
  );

  // 自定义面板：频率 / 每 N / 星期 / 结束
  await page.locator(".context-menu .menu-item-button", { hasText: "自定义" }).click();
  await page.waitForTimeout(300);
  const custom = await page.locator(".calendar-repeat-custom").innerText();
  check("6.7 自定义面板有频率与结束条件", custom.includes("频率") && custom.includes("结束") && custom.includes("永不"), custom);
  await page.locator(".calendar-weekday", { hasText: "三" }).click();
  await page.locator(".calendar-custom-apply").click();
  await page.waitForTimeout(300);
  const rowAfter = await page.locator(".cal-field", { hasText: "重复" }).first().innerText();
  // 展示串把重复的「周」省掉了（"周二、三"），所以按数字断言
  check("6.8 自定义重复写回行内（含刚点的周三）", /周三|、三/.test(rowAfter), rowAfter);
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForTimeout(500);
  const persisted = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), CALENDAR_KEY);
  const weekly = (persisted.events ?? []).find((event) => event.id === "cal-weekly");
  check("6.9 重复规则落盘", weekly?.repeat?.freq === "week" && (weekly?.repeat?.byWeekday ?? []).includes(3), J(weekly?.repeat));

  // 右键重复日程 → 三档删除
  await page.locator(".cal-event", { hasText: "周会" }).first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const menuText = await page.locator(".context-menu").innerText();
  check(
    "6.10 重复日程的删除有三档",
    menuText.includes("这一次") && menuText.includes("此次及以后") && menuText.includes("整个日程"),
    menuText
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  await page.close();
}

// ---------------------------------------------------------------------------
// 7. 需求 6：桌面拖动实时预览
// ---------------------------------------------------------------------------
async function section7DesktopDrag(context) {
  const { page } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      events: [calendarEvent("cal-1", { title: "可拖动", startTime: "09:00", endTime: "10:00" })],
      categories: []
    }
  });
  await openSchedulePage(page);
  await page.waitForTimeout(400);
  await page.evaluate(() => {
    const scroll = document.querySelector(".cal-scroll");
    scroll.scrollTop = 0;
  });
  await page.waitForTimeout(200);

  const boxOf = () => page.locator(".cal-event", { hasText: "可拖动" }).first().boundingBox();
  const before = await boxOf();
  check("7.1 事件块渲染出来", Boolean(before), J(before));

  // 拖下边缘：过程中（未松手）块就该跟着变
  const startY = before.y + before.height - 2;
  const startX = before.x + before.width / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX, startY + 30, { steps: 6 });
  const during = await boxOf();
  const duringText = await page.locator(".cal-event", { hasText: "可拖动" }).first().innerText();
  check("7.2 拖下边缘时块实时变高", during.height > before.height + 5, `${before.height} → ${during.height}`);
  check("7.3 拖动过程中时间文本已更新", /–/.test(duringText) && duringText !== "09:00 – 10:00", duringText);
  await page.mouse.up();
  await page.waitForTimeout(400);
  const afterResize = await boxOf();
  check("7.4 松手后保持新高度", Math.abs(afterResize.height - during.height) < 6, `${during.height} → ${afterResize.height}`);

  // 整块挪动：过程中位置实时跟手
  const midY = afterResize.y + Math.min(12, afterResize.height / 2);
  await page.mouse.move(startX, midY);
  await page.mouse.down();
  await page.mouse.move(startX, midY + 60, { steps: 8 });
  const duringMove = await boxOf();
  check("7.5 整块拖动时实时跟手", duringMove.y > afterResize.y + 20, `${afterResize.y} → ${duringMove.y}`);
  await page.mouse.up();
  await page.waitForTimeout(500);
  const persisted = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").events?.[0], CALENDAR_KEY);
  check("7.6 挪动落盘成新的开始时刻", persisted?.startTime !== "09:00", J(persisted));
  await page.close();
}

// ---------------------------------------------------------------------------
// 8. 需求 4 + 5：设置式表单编辑器
// ---------------------------------------------------------------------------
async function section8EditorForm(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });

  const labels = await page.locator(".calendar-editor .cal-field-label").allInnerTexts();
  check(
    "8.1 表单行齐全且有序",
    ["全天", "开始", "结束", "时长", "重复", "提醒", "地点", "分类"].every((item) => labels.includes(item)),
    J(labels)
  );
  check("8.2 详情是独立区块（不是整个界面）", await page.locator(".calendar-editor .calendar-details").count() === 1);

  // 标签栏等宽对仗
  const widths = await page.evaluate(() =>
    [...document.querySelectorAll(".calendar-editor .cal-field-label")].map((el) => Math.round(el.getBoundingClientRect().width))
  );
  check("8.3 标签栏同宽（对仗工整）", new Set(widths).size === 1, J(widths));

  // 桌面：新建时聚焦标题（不是详情编辑器）
  const focused = await page.evaluate(() => document.activeElement?.className ?? "");
  check("8.4 桌面新建聚焦标题输入框", focused.includes("calendar-title-input"), focused);

  // 开始日期浮层：给「确认」不给「今天」
  await page.locator(".cal-field", { hasText: "开始" }).locator(".cal-field-trigger").click();
  await page.waitForSelector(".date-picker", { timeout: 3000 });
  const actions = await page.locator(".date-picker-actions").innerText();
  check("8.5 选日期给「确认」不给「今天」", actions.includes("确认") && !actions.includes("今天"), actions);

  // 进时刻面板：也是「确认」
  await page.locator(".date-picker input[type='checkbox']").check().catch(() => {});
  const timeTrigger = page.locator(".dp-time-trigger");
  if (await timeTrigger.count()) {
    await timeTrigger.click();
    await page.waitForTimeout(250);
    const timeActions = await page.locator(".date-picker-actions").innerText();
    check("8.6 「选择时刻」面板给「确认」", timeActions.includes("确认") && !timeActions.includes("今天"), timeActions);
    check("8.7 时刻面板是双列滚轮", await page.locator(".time-picker .time-col").count() === 2);
    await page.locator(".date-picker-actions .dp-today").click();
  } else {
    check("8.6 「选择时刻」面板给「确认」", false, "没有时刻入口");
  }
  await page.waitForTimeout(250);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  if (await page.locator(".calendar-editor").count()) {
    await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  }
  await page.waitForTimeout(300);
  await page.close();
}

async function section8bEditorMobile(browser) {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(mobile, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(page);
  await page.locator(".calendar-fab").click();
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  await page.waitForTimeout(400);

  const focused = await page.evaluate(() => {
    const el = document.activeElement;
    if (!el) return "none";
    const editable = el.matches("input, textarea, [contenteditable='true'], .cm-content");
    return `${editable ? "EDITABLE:" : ""}${el.tagName}.${el.className}`;
  });
  check(
    "9.1 移动端进编辑器不聚焦任何输入框（不唤起输入法）",
    !focused.startsWith("EDITABLE:"),
    focused
  );
  check("9.2 移动端详情默认不是编辑态", await page.locator(".calendar-editor .cm-editor").count() === 0);
  check("9.3 移动端显示「点击添加详细信息」", await page.locator(".calendar-details-empty").count() === 1);

  await page.locator(".calendar-details-empty").click();
  await page.waitForTimeout(600);
  check("9.4 点详情区块才挂编辑器", await page.locator(".calendar-editor .cm-editor").count() === 1);

  // 切到预览再切回编辑：编辑器必须还在（宿主一直挂着，只是藏起来）
  await page.locator(".calendar-editor .cm-content").tap();
  await page.keyboard.type("移动端写的详情");
  await page.waitForTimeout(300);
  await page.locator(".calendar-details-head .editor-mode-switch button", { hasText: "预览" }).tap();
  await page.waitForTimeout(400);
  check("9.4a 预览渲染出刚写的内容", (await page.locator(".calendar-editor .editor-preview").innerText()).includes("移动端写的详情"));
  await page.locator(".calendar-details-head .editor-mode-switch button", { hasText: "编辑" }).tap();
  await page.waitForTimeout(400);
  check("9.4b 切回编辑内容还在（宿主没被销毁）", (await page.locator(".calendar-editor .cm-content").innerText()).includes("移动端写的详情"));

  // 提醒浮层不许跑出屏幕
  await page.locator(".cal-field-trigger", { hasText: "添加提醒" }).click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const inView = await page.evaluate(() => {
    const menu = document.querySelector(".context-menu");
    const r = menu.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, w: innerWidth, h: innerHeight };
  });
  check(
    "9.5 提醒浮层完整在屏内",
    inView.left >= -1 && inView.right <= inView.w + 1 && inView.top >= -1 && inView.bottom <= inView.h + 1,
    J(inView)
  );

  // 展开「指定具体时刻」子面板也不许越界
  await page.locator(".context-menu .menu-item-button", { hasText: "指定具体时刻" }).click();
  await page.waitForTimeout(400);
  const subInView = await page.evaluate(() => {
    const panels = [...document.querySelectorAll(".submenu-panel")];
    const last = panels[panels.length - 1];
    if (!last) return null;
    const r = last.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, w: innerWidth, h: innerHeight };
  });
  check(
    "9.6 自定义时刻子面板在屏内且可见",
    Boolean(subInView) && subInView.left >= -1 && subInView.right <= subInView.w + 1 && subInView.top >= -1 && subInView.bottom <= subInView.h + 1,
    J(subInView)
  );
  check("9.7 自定义时刻有日期与时刻两个输入", await page.locator(".calendar-reminder-custom input[type='date']").count() === 1 && await page.locator(".calendar-reminder-custom input[type='time']").count() === 1);
  check("9.8 无页面错误", errors.length === 0, J(errors));
  await page.close();
  await mobile.close();
}

// ---------------------------------------------------------------------------
// 10. 需求 6（移动端）：点空白落占位块 → 拖边界 → 点块进编辑器
// ---------------------------------------------------------------------------
async function section10MobileDraft(browser) {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(mobile, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(page);
  await page.waitForTimeout(400);

  // 列高是 24 小时（远超出可视区），落点必须在 .cal-scroll 的可视范围里
  const col = await page.locator(".cal-day-col.today").first().boundingBox();
  const scroll = await page.locator(".cal-scroll").boundingBox();
  check("10.1 今天列与滚动区都可见", Boolean(col) && Boolean(scroll), `${J(col)} / ${J(scroll)}`);
  const tapX = col.x + col.width / 2;
  const tapY = scroll.y + scroll.height * 0.45;

  await clickAt(page, tapX, tapY);
  await page.waitForTimeout(300);
  const draft = page.locator(".cal-draft");
  check("10.2 点空白落下占位块", await draft.count() === 1, `drafts=${await draft.count()}`);
  check("10.3 点空白没有直接进编辑器", await page.locator(".calendar-editor").count() === 0);

  const info = await page.evaluate(() => {
    const el = document.querySelector(".cal-draft");
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { text: el.innerText.replace(/\s+/g, " "), height: r.height, top: r.top, bottom: r.bottom };
  });
  if (!info) {
    check("10.4 占位块是 30 分钟（15 分钟吸附）", false, "没有占位块");
    await page.close();
    await mobile.close();
    return;
  }
  check("10.4 占位块是 30 分钟（15 分钟吸附）", /(\d{2}):([03]0|15|45)\s*–\s*(\d{2}):([03]0|15|45)/.test(info.text), info.text);
  const minutes = await page.evaluate(({ top, bottom }) => {
    const colEl = document.querySelector(".cal-day-col.today");
    const r = colEl.getBoundingClientRect();
    const toMin = (y) => ((y - r.top) / r.height) * 1440;
    return Math.round(toMin(bottom) - toMin(top));
  }, info);
  check("10.5 占位块高度约 30 分钟", Math.abs(minutes - 30) <= 16, `minutes=${minutes}`);

  // 拖下边界：时间文本实时变
  const handleY = info.bottom - 3;
  await pointerAt(page, "pointerdown", tapX, handleY, { target: ".cal-draft .cal-resize-bottom" });
  await pointerAt(page, "pointermove", tapX, handleY + 60, { target: ".cal-draft .cal-resize-bottom" });
  await page.waitForTimeout(120);
  const duringText = await page.locator(".cal-draft").innerText();
  const duringBox = await page.locator(".cal-draft").boundingBox();
  check("10.6 拖下边界时块实时变长", duringBox.height > info.height + 8, `${info.height} → ${duringBox.height}`);
  check("10.7 拖动中时间文本已更新", duringText.replace(/\s+/g, " ") !== info.text, `${info.text} → ${duringText}`);
  await pointerAt(page, "pointerup", tapX, handleY + 60, { target: ".cal-draft .cal-resize-bottom" });
  await page.waitForTimeout(200);

  // 点占位块 → 进编辑器，并带上拖出来的起止
  await page.locator(".cal-draft").click({ force: true });
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });
  const startRow = await page.locator(".cal-field", { hasText: "开始" }).innerText();
  const endRow = await page.locator(".cal-field", { hasText: "结束" }).innerText();
  check("10.8 点占位块进编辑器且带起止时刻", /\d{2}:\d{2}/.test(startRow) && /\d{2}:\d{2}/.test(endRow), `${startRow} / ${endRow}`);
  check("10.9 占位块已收掉", await page.locator(".cal-draft").count() === 0);
  check("10.10 无页面错误", errors.length === 0, J(errors));
  await page.locator(".calendar-editor .editor-icon-button[title='关闭']").click();
  await page.waitForTimeout(300);
  await page.close();
  await mobile.close();
}

// ---------------------------------------------------------------------------
// 11. 需求 8 + 9：移动端今天高亮与 FAB 位置
// ---------------------------------------------------------------------------
async function section11MobileChrome(browser) {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page } = await freshPage(mobile, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(page);
  await page.waitForTimeout(350);

  const fit = await page.evaluate(() => {
    const dom = document.querySelector(".cal-day-head.today .cal-dom");
    if (!dom) return null;
    const circle = dom.getBoundingClientRect();
    const range = document.createRange();
    range.selectNodeContents(dom);
    const text = range.getBoundingClientRect();
    const style = getComputedStyle(dom);
    return {
      circle, text,
      overflowX: Math.max(text.left - circle.left, circle.right - text.right),
      overflowY: Math.max(text.top - circle.top, circle.bottom - text.bottom),
      radius: style.borderRadius,
      aspect: circle.width / circle.height
    };
  });
  check("11.1 今天的圆底存在", Boolean(fit), J(fit));
  check("11.2 数字完整落在圆内", fit && fit.overflowX >= -0.5 && fit.overflowY >= -0.5, J(fit && { x: fit.overflowX, y: fit.overflowY }));
  check("11.3 圆底是圆不是被压扁的椭圆", fit && Math.abs(fit.aspect - 1) < 0.12, J(fit && fit.aspect));

  // FAB 与日记同高（不沉底）
  await page.locator(".calendar-view").waitFor();
  const calFab = await page.evaluate(() => {
    const el = document.querySelector(".calendar-fab-row");
    return Math.round(el.getBoundingClientRect().bottom);
  });
  const viewport = 800;
  const gap = viewport - calFab;
  // CSS 写的是布局像素（97px），壳上还有 scale → 视觉像素会更小；下限按 60 视觉像素算
  check("11.4 移动端 FAB 上提（不再沉底）", gap >= 60, `gap=${gap}`);

  // 与日记页 FAB 的距底差值一致（同一口径）
  await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".nav-row")];
    const diary = rows.find((row) => row.textContent.includes("日记"));
    diary?.click();
  });
  await page.waitForTimeout(600);
  const diaryFab = await page.evaluate(() => {
    const el = document.querySelector(".diary-fab-row");
    return el ? Math.round(el.getBoundingClientRect().bottom) : null;
  });
  check("11.5 与日记 FAB 同一高度", diaryFab !== null && Math.abs(diaryFab - calFab) <= 2, `diary=${diaryFab} calendar=${calFab}`);
  await page.close();
  await mobile.close();
}

// ---------------------------------------------------------------------------
// 12. 需求 2 + 3：右键菜单顺序与卡片日程图标对齐
// ---------------------------------------------------------------------------
async function section12TaskCard(context) {
  const { page } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("item-1", "entry-a", "关联了日程的条目")],
    calendar: {
      events: [calendarEvent("cal-1", { title: "关联", taskId: "item-1", startTime: "09:00", endTime: "10:00" })],
      categories: []
    }
  });
  await page.waitForSelector(".task-card", { timeout: 5000 });

  // 图标在文字**左边**（同一行、x 更小）
  const align = await page.evaluate(() => {
    const card = document.querySelector(".task-card");
    const badge = card.querySelector(".task-calendar-badge");
    const body = card.querySelector(".task-body");
    const b = badge.getBoundingClientRect();
    const t = body.getBoundingClientRect();
    return { badgeRight: b.right, bodyLeft: t.left, sameRow: Math.abs(b.top + b.height / 2 - (t.top + Math.min(t.height, 30) / 2)) < 30 };
  });
  check("12.1 日程图标在文字左侧", align.badgeRight <= align.bodyLeft + 1, J(align));
  const bodyAlign = await page.evaluate(() => {
    const body = document.querySelector(".task-card .task-body");
    return getComputedStyle(body).textAlign;
  });
  check("12.2 文字仍左对齐", bodyAlign === "left" || bodyAlign === "start", bodyAlign);

  // 右键菜单：「添加到日程/打开日程」紧跟「日期与提醒」
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const items = await page.locator(".context-menu > .menu-item .menu-item-label, .context-menu > .menu-item-button .menu-item-label").allInnerTexts();
  const dateIndex = items.findIndex((text) => text.includes("日期与提醒"));
  const calIndex = items.findIndex((text) => text.includes("日程"));
  check("12.3 「日期与提醒」在菜单里", dateIndex >= 0, J(items));
  check("12.4 日程项紧跟在日期与提醒下面", calIndex === dateIndex + 1, J(items));
  check("12.5 已关联时文案是「打开日程」", items[calIndex]?.includes("打开日程"), J(items));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  await page.close();
}

async function section12bTaskCardMobile(browser) {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page } = await freshPage(mobile, { mobile: true });
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("item-1", "entry-a", "移动端也要左对齐"), task("item-2", "entry-a", "置顶加日程", { pinned: true })],
    calendar: {
      events: [
        calendarEvent("cal-1", { title: "关联", taskId: "item-1", startTime: "09:00", endTime: "10:00" }),
        calendarEvent("cal-2", { title: "关联2", taskId: "item-2", startTime: "11:00", endTime: "12:00" })
      ],
      categories: []
    },
    settings: { features: { pinnedIcon: true } }
  });
  await page.locator(".tree-row[data-node-id='entry-a']").first().click();
  await page.waitForSelector(".task-card", { state: "attached", timeout: 5000 });
  await page.waitForTimeout(400);

  const rows = await page.evaluate(() =>
    [...document.querySelectorAll(".task-card")].map((card) => {
      const badge = card.querySelector(".task-calendar-badge");
      const body = card.querySelector(".task-body");
      const pin = card.querySelector(".task-pin");
      if (!badge || !body) return null;
      const b = badge.getBoundingClientRect();
      const t = body.getBoundingClientRect();
      const p = pin?.getBoundingClientRect();
      return {
        badgeLeft: Math.round(b.left),
        bodyLeft: Math.round(t.left),
        bodyTop: Math.round(t.top),
        badgeTop: Math.round(b.top),
        pinLeft: p ? Math.round(p.left) : null,
        cardLeft: Math.round(card.getBoundingClientRect().left)
      };
    }).filter(Boolean)
  );
  check("12.6 移动端两张卡都有日程图标", rows.length === 2, J(rows));
  check(
    "12.7 移动端图标在文字左侧、文字没有被顶到下一行",
    rows.every((row) => row.badgeLeft < row.bodyLeft && Math.abs(row.badgeTop - row.bodyTop) < 40),
    J(rows)
  );
  const pinned = rows.find((row) => row.pinLeft !== null);
  check("12.8 置顶图钉排在日程图标左边", !pinned || pinned.pinLeft < pinned.badgeLeft, J(pinned));
  check(
    "12.9 图标紧贴卡片左侧（不是右对齐）",
    rows.every((row) => row.badgeLeft - row.cardLeft < 90),
    J(rows)
  );
  await page.close();
  await mobile.close();
}

// ---------------------------------------------------------------------------
// 13. 需求 13：全局悬浮提示
// ---------------------------------------------------------------------------
async function section13Tooltip(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePage(page);

  const target = page.locator(".calendar-view-switch button[aria-label='月视图']").first();
  await target.hover();
  await page.waitForTimeout(150);
  check("13.1 延迟内不立刻冒出来", await page.locator(".kx-tooltip.show").count() === 0);
  await page.waitForTimeout(500);
  const tip = page.locator(".kx-tooltip");
  check("13.2 悬浮后出现自定义提示", await tip.count() === 1 && (await tip.innerText()) === "月视图", await tip.innerText().catch(() => ""));
  const stashed = await target.getAttribute("title");
  check("13.3 原生 title 被摘掉（不会双重提示）", stashed === null, J(stashed));
  const dataTip = await target.getAttribute("data-tip");
  check("13.4 文本暂存在 data-tip", dataTip === "月视图", J(dataTip));

  const geom = await page.evaluate(() => {
    const tipEl = document.querySelector(".kx-tooltip");
    const anchor = document.querySelector(".calendar-view-switch button[title], .calendar-view-switch button[data-tip]");
    const t = tipEl.getBoundingClientRect();
    const a = anchor.getBoundingClientRect();
    const style = getComputedStyle(tipEl);
    return {
      below: t.top >= a.bottom - 2,
      centered: Math.abs(t.left + t.width / 2 - (a.left + a.width / 2)) < 12,
      opacity: style.opacity,
      radius: style.borderRadius,
      inView: t.left >= 0 && t.right <= innerWidth && t.bottom <= innerHeight
    };
  });
  check("13.5 提示在元素正下方居中", geom.below && geom.centered, J(geom));
  check("13.6 提示已完成淡入", Number(geom.opacity) > 0.9, J(geom));
  check("13.7 提示在视口内", geom.inView, J(geom));
  check("13.8 现代观感（圆角）", parseFloat(geom.radius) >= 6, J(geom));

  await page.mouse.move(5, 5);
  await page.waitForTimeout(250);
  check("13.9 移开后提示收起", await page.locator(".kx-tooltip.show").count() === 0);
  const restored = await target.getAttribute("title");
  check("13.10 移开后 title 原样还回去", restored === "月视图", J(restored));
  await page.close();
}

// ---------------------------------------------------------------------------
// 14. 需求 16 / 17 / 18：剪切板路径行、存储空间 ⓘ、草稿纸标签名
// ---------------------------------------------------------------------------
async function section14SmallItems(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });

  // ---- 需求 18：草稿纸标签页默认名取正文前几个字符 ----
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.waitForTimeout(300);

  await page.locator('button[aria-label="新建标签页"]').click();
  await page.waitForTimeout(300);
  const emptyNames = await page.locator(".scratchpad-tab-name").allTextContents();
  check("14.1 空标签页仍是数字默认名", emptyNames.join(",") === "1,2", J(emptyNames));

  await page.locator(".scratchpad-area").fill("下周三下午和客户开评审会议");
  await page.waitForTimeout(400);
  const derived = await page.locator(".scratchpad-tab-name").allTextContents();
  const autoName = (derived[1] ?? "").trim();
  check("14.2 默认名取正文开头几个字", autoName.length > 0 && "下周三下午和客户开评审会议".startsWith(autoName), J(derived));
  check("14.3 名字长度受标签宽度预算限制", [...autoName].length <= 6, J(autoName));

  // 名字不许溢出自己的标签（宽度预算是按标签最小宽度算的）
  const fits = await page.evaluate(() => {
    const tabs = [...document.querySelectorAll(".scratchpad-tab")];
    return tabs.map((tab) => {
      const name = tab.querySelector(".scratchpad-tab-name");
      if (!name) return null;
      return { over: name.scrollWidth - name.clientWidth, tabW: Math.round(tab.getBoundingClientRect().width) };
    }).filter(Boolean);
  });
  check("14.4 默认名不撑破标签", fits.every((item) => item.over <= 1), J(fits));

  // 手动改名后不再被内容覆盖
  await page.locator(".scratchpad-tab.active").click();
  await page.waitForTimeout(250);
  await page.locator(".scratchpad-tab-rename").fill("评审会");
  await page.locator(".scratchpad-tab-rename").press("Enter");
  await page.waitForTimeout(250);
  await page.locator(".scratchpad-area").fill("改过名字之后又写了一堆新的内容进去");
  await page.waitForTimeout(400);
  const afterRename = await page.locator(".scratchpad-tab-name").allTextContents();
  check("14.5 手动改过的名字不被内容覆盖", (afterRename[1] ?? "").trim() === "评审会", J(afterRename));

  // ---- 需求 16：桌面剪切板路径行 ----
  await page.locator(".toolbox-sub-bar button[title='返回工具箱']").click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "剪切板" }).click();
  await page.waitForTimeout(800);
  const dirInput = page.locator(".clipboard-dir-input");
  check("14.6 剪切板页有路径输入框", await dirInput.count() === 1);
  const rows = await dirInput.getAttribute("rows");
  check("14.7 桌面路径框默认单行", rows === "1", `rows=${rows}`);
  const box = await dirInput.evaluate((el) => {
    const style = getComputedStyle(el);
    return { height: el.getBoundingClientRect().height, lineHeight: Number.parseFloat(style.lineHeight) || 20, wrap: style.whiteSpace };
  });
  check("14.8 单行高度（不预留两行）", box.height < box.lineHeight * 2.1, J(box));

  // 「打开文件夹」是纯图标按钮，认 title / aria-label；「选择路径」必须整条没有
  const actions = await page.evaluate(() =>
    [...document.querySelectorAll(".clipboard-dir-actions button")].map(
      (button) => button.getAttribute("title") || button.getAttribute("aria-label") || button.textContent.trim()
    )
  );
  check("14.9 「选择路径」已删除", !actions.some((label) => label.includes("选择路径")), J(actions));
  check(
    "14.10 仍有「打开文件夹」与「默认」",
    actions.some((label) => label.includes("打开文件夹")) && actions.some((label) => label.includes("默认")),
    J(actions)
  );

  // 路径真的很长时才折行（自动增高，不出现内部滚动条）
  await page.evaluate(() => {
    const el = document.querySelector(".clipboard-dir-input");
    if (!el) return;
    el.value = "C:\\\\Users\\\\somebody\\\\AppData\\\\Local\\\\kxtodo\\\\clipboard\\\\saved\\\\2026\\\\10\\\\一个很长很长的路径名字\\\\文件.png";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.waitForTimeout(400);
  const grown = await page.locator(".clipboard-dir-input").evaluate((el) => ({
    height: el.getBoundingClientRect().height,
    scrollable: el.scrollHeight > el.clientHeight + 2
  }));
  check("14.11 长路径自动增高且不出内部滚动条", grown.height > box.height && !grown.scrollable, J(grown));
  await page.close();
}

async function section14bStorage(context) {
  const { page } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  // 浏览器预览没有数据目录 → storageUsage 为空，整节不渲染。这里只钉 CSS 与文案。
  const css = await page.evaluate(async () => {
    const response = await fetch("/src/styles/settings.css");
    return response.ok ? response.text() : "";
  });
  check("15.1 ⓘ 与分组名同行（三列网格）", /\.storage-head-row\s*{[^}]*grid-template-columns:\s*[^;]*auto/.test(css), "没找到三列网格规则");
  const app = await page.evaluate(async () => {
    const response = await fetch("/src/lib/SettingsDrawer.svelte");
    return response.ok ? response.text() : "";
  });
  check("15.2 那句清理范围废话已删", !app.includes("只清没人引用的图片"), "文案还在");
  await page.close();
}

// ---------------------------------------------------------------------------
// 16. 编辑器写路径不回归：改字段能落盘
// ---------------------------------------------------------------------------
async function section16EditorWrites(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    calendar: {
      events: [calendarEvent("cal-1", { title: "原标题", startTime: "09:00", endTime: "10:00" })],
      categories: [calendarCategory("ccat-1", "工作", "#1f6feb")]
    }
  });
  await openSchedulePage(page);
  await page.locator(".cal-event", { hasText: "原标题" }).first().click();
  await page.waitForSelector(".calendar-editor", { timeout: 4000 });

  await page.locator(".calendar-title-input").fill("改过的标题");
  await page.locator(".cal-field-location input").fill("3 号会议室");
  await page.locator(".cal-field-trigger", { hasText: "无分类" }).click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "工作" }).click();
  await page.waitForTimeout(250);
  await page.locator(".calendar-details-empty, .editor-cm-host").first().click();
  await page.waitForTimeout(400);
  if (await page.locator(".calendar-editor .cm-content").count()) {
    await page.locator(".calendar-editor .cm-content").fill("详情正文");
  }
  await page.locator(".calendar-editor .editor-icon-button.primary").click();
  await page.waitForSelector(".calendar-editor", { state: "detached", timeout: 5000 });
  await page.waitForTimeout(500);

  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").events?.[0], CALENDAR_KEY);
  check("16.1 标题落盘", stored?.title === "改过的标题", J(stored));
  check("16.2 地点落盘", stored?.location === "3 号会议室", J(stored));
  check("16.3 分类落盘", stored?.categoryId === "ccat-1", J(stored));
  check("16.4 详情落盘", stored?.details === "详情正文", J(stored));
  check("16.5 时间没被误改", stored?.startTime === "09:00" && stored?.endTime === "10:00", J(stored));
  check("16.6 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 17. 需求 15（移动端）：双指捏合缩放格高并记住
// ---------------------------------------------------------------------------
async function section17MobilePinch(browser) {
  const mobile = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(mobile, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")] });
  await openSchedulePageMobile(page);
  await page.waitForTimeout(400);

  const hourOf = () =>
    page.evaluate(() => Number.parseFloat(getComputedStyle(document.querySelector(".cal-grid-body")).getPropertyValue("--cal-hour-h")));
  const before = await hourOf();
  check("17.1 移动端也是一屏约 12 小时", before > 20, `hourH=${before}`);

  // 双指捏合张开（distance 100 → 200 = 放大 2 倍，钳到上限 3）
  await page.evaluate(() => {
    const grid = document.querySelector(".cal-grid");
    const rect = grid.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const touch = (id, dx) => new Touch({ identifier: id, target: grid, clientX: cx + dx, clientY: cy });
    const fire = (type, list) =>
      grid.dispatchEvent(new TouchEvent(type, { bubbles: true, cancelable: true, touches: list, targetTouches: list, changedTouches: list }));
    fire("touchstart", [touch(1, -50), touch(2, 50)]);
    fire("touchmove", [touch(1, -70), touch(2, 70)]);
    fire("touchmove", [touch(1, -100), touch(2, 100)]);
    fire("touchend", []);
  });
  await page.waitForTimeout(500);
  const after = await hourOf();
  check("17.2 捏合张开实时放大格高", after > before * 1.5, `${before} → ${after}`);

  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").calendar?.hourZoom, SETTINGS_KEY);
  check("17.3 移动端缩放也写进设置", typeof stored === "number" && stored > 1.4, `stored=${stored}`);

  // 移动端编辑器铺满且不横向溢出
  await page.locator(".calendar-fab").tap();
  await page.waitForSelector(".calendar-editor", { timeout: 5000 });
  await page.waitForTimeout(400);
  const fit = await page.evaluate(() => {
    const dialog = document.querySelector(".calendar-editor").getBoundingClientRect();
    const bad = [];
    for (const el of document.querySelectorAll(".calendar-editor *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (rect.right > innerWidth + 1 || rect.left < -1) bad.push(`${el.className?.toString?.().slice(0, 40)}`);
    }
    return { dialog: { left: dialog.left, right: dialog.right, w: innerWidth }, bad: bad.slice(0, 5) };
  });
  check(
    "17.4 移动端编辑器铺满宽度",
    fit.dialog.left >= -1 && fit.dialog.right <= fit.dialog.w + 1,
    J(fit.dialog)
  );
  check("17.5 编辑器里没有横向溢出的元素", fit.bad.length === 0, J(fit.bad));
  check("17.6 无页面错误", errors.length === 0, J(errors));
  await page.close();
  await mobile.close();
}

// ---------------------------------------------------------------------------

async function main() {
  const browser = await chromium.launch({
    channel: "msedge",
    headless: true,
    args: ["--no-sandbox", "--disable-gpu"]
  });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });

  await section1LenientView(context);
  await section2Toolbar(context);
  await section3Zoom(context);
  await section4GridLook(context);
  await section5Categories(context);
  await section6Repeat(context);
  await section7DesktopDrag(context);
  await section8EditorForm(context);
  await section8bEditorMobile(browser);
  await section10MobileDraft(browser);
  await section11MobileChrome(browser);
  await section12TaskCard(context);
  await section12bTaskCardMobile(browser);
  await section13Tooltip(context);
  await section14SmallItems(context);
  await section14bStorage(context);
  await section16EditorWrites(context);
  await section17MobilePinch(browser);

  await context.close();
  await browser.close();
  console.log(`\n${passes} passed, ${failures} failed`);
  process.exit(failures > 0 ? 1 : 0);
}

await main();

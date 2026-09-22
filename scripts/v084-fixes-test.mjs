// v0.8.4 回归。本版分四批：
// A 批（BugFix 16–21）、B 批（编辑器 / 取色器 / 日历 UI 7–15）、
// C 批（性能 3–6）、D 批（草稿纸同步 2 / 文件传输助手 1）。
// 用法：node scripts/v084-fixes-test.mjs（需先 npm run dev）。
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

/** 本地今天的 YYYY-MM-DD（`toISOString()` 是 UTC，凌晨跑会差一天）。 */
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
  await page.waitForTimeout(500);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

/** 种状态：nodes/tasks/backgrounds 的形状与 normalize 的输入一致。 */
async function seedState(page, { nodes = [], tasks = [], settings = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, stateKey, settingsKey }) => {
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
    },
    { nodes, tasks, settings, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY }
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

const browser = await chromium.launch({ channel: "msedge" });
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const mobile = await browser.newContext({
  userAgent: ANDROID_UA,
  viewport: { width: 393, height: 851 },
  isMobile: true,
  hasTouch: true
});

// ===========================================================================
// 16 固定区工具图标点击跳转（固定行直达 / 工具箱行回列表 / 移动端返回键回列表）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const toolboxState = async () =>
    page.evaluate(() => ({
      list: !!document.querySelector(".toolbox-list"),
      sub: !!document.querySelector(".toolbox-sub-bar"),
      selected: [...document.querySelectorAll(".nav-row.selected")].map((el) => el.textContent?.trim() ?? "")
    }));

  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  const firstCard = page.locator(".toolbox-card").first();
  const firstName = (await firstCard.textContent())?.trim().split(/\s+/)[0] ?? "";
  await firstCard.click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "固定此工具" }).click();
  await page.waitForTimeout(500);

  await page.locator(".nav-row", { hasText: "日记" }).click();
  await page.waitForTimeout(400);
  await page.locator(".nav-row", { hasText: firstName }).first().click();
  await page.waitForTimeout(700);
  let state = await toolboxState();
  check("16 固定区图标直达工具子界面", state.sub && !state.list, J(state));

  await page.locator(".toolbox-sub-bar button").first().click();
  await page.waitForTimeout(400);
  state = await toolboxState();
  check("16 子界面返回落在工具箱主界面", state.list && !state.sub, J(state));

  const cards = page.locator(".toolbox-card");
  // 点第二个工具要进第二个工具（原来会跳回被固定那个）
  await cards.nth(1).click();
  await page.waitForTimeout(700);
  const secondTitle = await page.evaluate(() => document.querySelector(".toolbox-view")?.textContent?.slice(0, 40) ?? "");
  check("16 从列表点第二个工具不被固定项抢走", !secondTitle.includes(firstName), secondTitle);

  // 侧栏「工具箱」行：在子界面时也要回主界面
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(400);
  state = await toolboxState();
  check("16 子界面点侧栏工具箱行回主界面", state.list && !state.sub, J(state));
  check("16 全程无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// 16b 移动端：子工具里按返回键回工具箱主界面，再按一次才退出工具箱
{
  const { page } = await freshPage(mobile);
  await page.evaluate(
    ({ key }) => {
      const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
      settings.appearance = {
        ...(settings.appearance ?? {}),
        navItems: ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox", "tool:rmb"]
      };
      localStorage.setItem(key, JSON.stringify(settings));
    },
    { key: SETTINGS_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const pressBack = async () => {
    const consumed = await page.evaluate(() => {
      const handler = window.kxtodoBackHandler;
      return typeof handler === "function" ? handler() : false;
    });
    if (!consumed) await page.goBack();
    await page.waitForTimeout(500);
    return consumed;
  };
  await page.locator(".nav-row", { hasText: "人民币大小写" }).first().click();
  await page.waitForTimeout(700);
  let info = await page.evaluate(() => ({
    view: document.querySelector(".app-shell")?.className.includes("view-toolbox") ?? false,
    list: !!document.querySelector(".toolbox-list"),
    sub: !!document.querySelector(".toolbox-sub-bar")
  }));
  check("16 移动端固定行直达子界面", info.view && info.sub && !info.list, J(info));

  await pressBack();
  info = await page.evaluate(() => ({
    view: document.querySelector(".app-shell")?.className.includes("view-toolbox") ?? false,
    list: !!document.querySelector(".toolbox-list"),
    sub: !!document.querySelector(".toolbox-sub-bar")
  }));
  check("16 移动端返回键先回工具箱主界面", info.view && info.list && !info.sub, J(info));

  await pressBack();
  info = await page.evaluate(() => ({
    view: document.querySelector(".app-shell")?.className.includes("view-toolbox") ?? false
  }));
  check("16 移动端再按返回键退出工具箱", !info.view, J(info));
  await page.close();
}

// ===========================================================================
// 17 移动端蓝色遮罩：全局一行，散落的逐元素补丁全部收编
// ===========================================================================
{
  const { page } = await freshPage(mobile);
  const tapColor = await page.evaluate(() => {
    const probes = [".app-shell", ".nav-row", ".header-actions button", ".toolbox-card"];
    const found = {};
    for (const selector of probes) {
      const el = document.querySelector(selector);
      if (el) found[selector] = getComputedStyle(el).webkitTapHighlightColor;
    }
    return found;
  });
  const values = Object.values(tapColor);
  check("17 根上全局关掉了点按高亮", values.length > 0 && values.every((value) => value === "rgba(0, 0, 0, 0)"), J(tapColor));
  await page.close();
}

// ===========================================================================
// 18 「计划内」折叠块换行：折叠一个分区，下一个标题不能被吸到右边
// ===========================================================================
{
  const { page } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [
      makeTask({ id: "t-overdue", nodeId: "entry-a", markdown: "逾期任务", dueDate: localToday(-2), order: 1 }),
      makeTask({ id: "t-today", nodeId: "entry-a", markdown: "今天任务", dueDate: localToday(0), order: 2 }),
      makeTask({ id: "t-three", nodeId: "entry-a", markdown: "近三天任务", dueDate: localToday(1), order: 3 })
    ]
  });
  await page.locator(".nav-row", { hasText: "计划内" }).click();
  await page.waitForTimeout(700);
  const labels = page.locator(".task-section-label");
  const count = await labels.count();
  check("18 计划内出现多个分区", count >= 3, `count=${count}`);
  const rectOf = async (index) => labels.nth(index).boundingBox();
  const todayIndex = await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".task-section-label")];
    return rows.findIndex((el) => (el.textContent ?? "").includes("今天") && !(el.textContent ?? "").includes("近三"));
  });
  check("18 找到「今天」分区标题", todayIndex >= 0, `index=${todayIndex}`);
  const before = await rectOf(todayIndex);
  await labels.nth(todayIndex).click();
  await page.waitForTimeout(400);
  const todayBox = await rectOf(todayIndex);
  const nextBox = await rectOf(todayIndex + 1);
  check(
    "18 折叠后下一个分区另起一行",
    Boolean(todayBox && nextBox) && Math.abs((nextBox?.x ?? -1) - (todayBox?.x ?? 0)) < 4,
    J({ todayBox, nextBox, before })
  );
  const firstBox = await rectOf(0);
  check(
    "18 折叠后标题宽度仍是胶囊（没有撑满整行）",
    Boolean(firstBox) && (firstBox?.width ?? 0) < 420,
    J(firstBox)
  );
  await page.close();
}

// ===========================================================================
// 19 记账统计「自定义」：日历年视图与月视图不再叠在一起
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(700);
  await page.click(".ledger-view-switch button[title='统计视图']");
  await page.waitForTimeout(500);
  await page.locator(".ledger-segmented button", { hasText: "自定义" }).first().click();
  await page.waitForTimeout(400);
  await page.locator(".ledger-custom-field strong").first().click();
  await page.waitForSelector(".ledger-pop.date .dp-title", { timeout: 8000 });

  await page.locator(".ledger-pop.date .dp-title").click();
  await page.waitForTimeout(400);
  const yearView = await page.evaluate(() => {
    const pop = document.querySelector(".ledger-pop.date");
    return {
      headers: pop?.querySelectorAll(".date-picker-header").length ?? 0,
      dayGrid: !!pop?.querySelector(".date-picker-grid"),
      monthGrid: !!pop?.querySelector(".month-picker")
    };
  });
  check(
    "19 年视图只显示年/月网格（没有日月历头、没有日网格）",
    yearView.monthGrid && !yearView.dayGrid && yearView.headers === 0,
    J(yearView)
  );

  await page.locator(".ledger-pop.date .mp-cell", { hasText: "3月" }).click();
  await page.waitForTimeout(400);
  const backToDays = await page.evaluate(() => {
    const pop = document.querySelector(".ledger-pop.date");
    return {
      header: pop?.querySelector(".dp-title")?.textContent?.trim() ?? "",
      dayGrid: !!pop?.querySelector(".date-picker-grid")
    };
  });
  check(
    "19 选完月份跳到那个月的月视图",
    backToDays.dayGrid && backToDays.header === "2026年3月",
    J(backToDays)
  );
  check("19 记账统计全程无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// 19b 「日期与提醒」面板的日历共用同一份 CalendarGrid：同样只显示年视图
{
  const { page } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-date", nodeId: "entry-a", markdown: "带日期的任务", dueDate: localToday(0) })]
  });
  await page.locator(".nav-row", { hasText: "计划内" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".date-reminder-panel", { timeout: 8000 });
  await page.locator(".date-reminder-panel .dp-title").click();
  await page.waitForTimeout(400);
  const state = await page.evaluate(() => {
    const panel = document.querySelector(".date-reminder-panel");
    return {
      headers: panel?.querySelectorAll(".date-picker-header").length ?? 0,
      dayGrid: !!panel?.querySelector(".date-picker-grid"),
      monthGrid: !!panel?.querySelector(".month-picker")
    };
  });
  check("19 日期与提醒面板同样只显示年视图", state.monthGrid && !state.dayGrid && state.headers === 0, J(state));
  await page.locator(".date-reminder-panel .mp-cell", { hasText: "5月" }).click();
  await page.waitForTimeout(400);
  const after = await page.evaluate(() => {
    const panel = document.querySelector(".date-reminder-panel");
    return {
      header: panel?.querySelector(".dp-title")?.textContent?.trim() ?? "",
      dayGrid: !!panel?.querySelector(".date-picker-grid")
    };
  });
  check("19 选完月份回到该月日网格", after.dayGrid && after.header.includes("5月"), J(after));
  await page.close();
}

// ===========================================================================
// 20 趋势图折线：大跨度（真实路径长度超过 dash 基准）也要铺满整条路径
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  // 30 天里大小交替的巨额流水 → 日桶折线一条比一条陡，真实长度远超 2400
  await page.evaluate(
    ({ key }) => {
      const now = new Date().toISOString();
      const iso = (offset) => new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10);
      localStorage.setItem(
        key,
        JSON.stringify({
          accounts: [{ id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now }],
          categories: [
            { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now },
            { id: "lcat-inc-01", name: "工资", side: "income", icon: "Wallet", color: "#2f9e6e", parentId: "", createdAt: now }
          ],
          entries: Array.from({ length: 30 }, (_, i) => ({
            id: `le-${i}`,
            kind: "expense",
            amountCents: i % 2 === 0 ? 100 : 3000000,
            accountId: "lacc-01",
            categoryId: "lcat-exp-01",
            date: iso(i),
            note: `第 ${i} 笔`,
            createdAt: now
          })),
          accountTypes: []
        })
      );
    },
    { key: "todo-note-ledger-v1" }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await page.locator(".nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(600);
  await page.click(".ledger-view-switch button[title='统计视图']");
  await page.waitForTimeout(400);
  await page.locator(".ledger-segmented button", { hasText: "自定义" }).first().click();
  await page.waitForTimeout(1200);
  // 图例默认收入+支出两条：收入那条是平的，陡的是支出线（.ledger-line.out）
  const line = await page.evaluate(() => {
    const path = document.querySelector(".ledger-line.out") ?? document.querySelector(".ledger-line");
    if (!path) return null;
    return {
      totalLength: Math.round(path.getTotalLength()),
      dash: parseFloat(getComputedStyle(path).strokeDasharray),
      pathLength: path.getAttribute("pathLength"),
      dashoffset: parseFloat(getComputedStyle(path).strokeDashoffset)
    };
  });
  check(
    "20 折线路径比 dash 基准长（旧代码必断的情形）",
    line !== null && line.totalLength > (line?.dash ?? 0),
    J(line)
  );
  check(
    "20 用 pathLength 归一化，整条折线画满（虚线图案不重复）",
    line?.pathLength === String(line?.dash) && line?.dashoffset === 0,
    J(line)
  );
  check("20 记账统计无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 21 标签面板：字号调大后元素也不能伸出面板边界
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(() => {
    const key = "todo-note-settings-v3";
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.appearance = {
      ...(settings.appearance ?? {}),
      uiFontSize: 22,
      tagPresets: [
        { id: "p1", color: "red", text: "工作" },
        { id: "p2", color: "blue", text: "这是一条特别特别长的预置标签名字用来撑宽度" },
        { id: "p3", color: "green", text: "生活" }
      ],
      diaryTagPresets: [{ id: "d1", color: "purple", text: "日记预置" }]
    };
    localStorage.setItem(key, JSON.stringify(settings));
  });
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-tag", nodeId: "entry-a", markdown: "标签面板测试" })]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card .edit-button").first().click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.waitForTimeout(800);
  await page.locator(".editor-tag-add").first().click();
  await page.waitForSelector(".editor-tag-pop .tag-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const probe = await page.evaluate(() => {
    const pop = document.querySelector(".editor-tag-pop");
    const pr = pop.getBoundingClientRect();
    const cs = getComputedStyle(pop);
    const scale = pr.width / pop.offsetWidth;
    const content = { left: pr.left + parseFloat(cs.paddingLeft) * scale, right: pr.right - parseFloat(cs.paddingRight) * scale };
    const over = [];
    for (const el of pop.querySelectorAll("*")) {
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0) continue;
      if (r.right > content.right + 0.6 || r.left < content.left - 0.6) {
        over.push({ cls: (el.className || el.tagName).toString().slice(0, 36), overRight: Math.round(r.right - content.right) });
      }
    }
    return over;
  });
  check("21 大字号下标签面板元素不伸出边界", probe.length === 0, J(probe));
  check("21 编辑器无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// 21b 同一份面板在右键菜单里（含长预置）也不越界
{
  const { page } = await freshPage(desktop);
  await page.evaluate(() => {
    const key = "todo-note-settings-v3";
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.appearance = {
      ...(settings.appearance ?? {}),
      uiFontSize: 22,
      tagPresets: [{ id: "p2", color: "blue", text: "这是一条特别特别长的预置标签名字用来撑宽度" }]
    };
    localStorage.setItem(key, JSON.stringify(settings));
  });
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-tag", nodeId: "entry-a", markdown: "标签菜单测试" })]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "标签" }).first().click();
  await page.waitForSelector(".tag-editor-panel .tag-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const probe = await page.evaluate(() => {
    const panel = document.querySelector(".tag-editor-panel");
    const pr = panel.getBoundingClientRect();
    const cs = getComputedStyle(panel);
    const scale = pr.width / panel.offsetWidth;
    const right = pr.right - parseFloat(cs.paddingRight) * scale;
    const over = [];
    for (const el of panel.querySelectorAll(".tag-preset, .tag-preset-list, .tag-editor-input-row, .tag-color-grid, .tag-color-row")) {
      const r = el.getBoundingClientRect();
      if (r.right > right + 0.6) over.push({ cls: (el.className || "").slice(0, 30), overRight: Math.round(r.right - right) });
    }
    return over;
  });
  check("21 右键菜单里的标签面板不越界", probe.length === 0, J(probe));
  await page.close();
}

// ===========================================================================
// 7 工具箱子页：右上角两枚按钮（返回 + ⋯ 外观菜单），背景/主题色可换
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card").first().click();
  await page.waitForTimeout(700);
  const head = await page.evaluate(() => ({
    buttons: document.querySelectorAll(".toolbox-sub-bar button").length,
    oldBackRow: !!document.querySelector(".toolbox-sub-back"),
    tool: !!document.querySelector(".toolbox-sub, .random-tool, .scratchpad-area, .rmb-tool, .transfer-tool")
  }));
  check("7 子页头部只有两枚按钮、旧返回行没了", head.buttons === 2 && !head.oldBackRow && head.tool, J(head));

  await page.locator(".toolbox-sub-bar button").nth(1).click();
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  await page.waitForTimeout(300);
  const menuText = ((await page.locator(".context-menu").textContent()) ?? "").replace(/\s+/g, " ");
  check("7 ⋯ 菜单提供 UI颜色与背景颜色", menuText.includes("UI颜色") && menuText.includes("背景颜色"), menuText.slice(0, 60));

  const readSettings = () => page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), SETTINGS_KEY);
  const toolboxBg = () =>
    page.evaluate(() => getComputedStyle(document.querySelector(".toolbox-view")).backgroundColor);
  const toolboxAccent = () =>
    page.evaluate(() => getComputedStyle(document.querySelector(".toolbox-view")).getPropertyValue("--accent").trim());

  const beforeSave = JSON.stringify((await readSettings()).toolbox ?? null);
  // v0.8.5 需求 5：预设色块单击即落盘（不再进草稿等保存）
  await page.locator(".context-menu .color-grid button").nth(2).click();
  await page.waitForTimeout(600);
  const afterPreset = (await readSettings()).toolbox ?? null;
  check(
    "7 预设背景色单击即落盘（不弹草稿条）",
    JSON.stringify(afterPreset) !== beforeSave &&
      Boolean(afterPreset?.backgroundColor) &&
      (await page.locator(".context-menu .color-draft-actions").count()) === 0,
    J(afterPreset)
  );

  // 自定义取色（统一取色盘）仍走「草稿 → 保存」
  await page.locator(".context-menu .palette-button").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  const pickColor = async (hex) => {
    const input = page.locator(".kx-color-hex");
    await input.fill(hex);
    await input.press("Enter");
    await page.waitForTimeout(200);
  };
  await pickColor("#123456");
  const previewBg = await toolboxBg();
  check(
    "7 自定义取色只预览不落盘",
    JSON.stringify((await readSettings()).toolbox ?? null) === JSON.stringify(afterPreset) &&
      previewBg !== "rgb(240, 240, 240)",
    `preview=${previewBg}`
  );
  // 取色盘的「确认」= 落盘（v0.8.6 需求 11：确认/取消就做在色盘上）
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const saved = (await readSettings()).toolbox ?? null;
  // v0.8.7 起工具子页调的是**它自己那一份**（toolbox.toolBackgrounds[工具id]），主界面那层不动
  check("7 保存后落盘到该工具自己那一份", saved?.toolBackgrounds?.random === "#123456", J(saved));

  // 取消：预览回退、盘里不动
  const savedBg = await toolboxBg();
  await page.locator(".context-menu .palette-button").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  await pickColor("#777777");
  const previewBg2 = await toolboxBg();
  await page.locator(".kx-color-panel .menu-action-button", { hasText: "取消" }).click();
  await page.waitForTimeout(400);
  check(
    "7 取消回退预览且盘里不变",
    previewBg2 !== savedBg && (await toolboxBg()) === savedBg && JSON.stringify((await readSettings()).toolbox) === JSON.stringify(saved),
    `${savedBg} / ${previewBg2}`
  );

  // 主题色：改 --accent，保存才写
  await page.locator(".context-menu .ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  await pickColor("#b64a30");
  check(
    "7 主题色取色时整页立刻变色",
    (await toolboxAccent()) === "#b64a30" && ((await readSettings()).toolbox?.accent ?? "") === "",
    await toolboxAccent()
  );
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  check("7 主题色保存后落盘到该工具自己那一份", ((await readSettings()).toolbox?.toolAccents?.random ?? "") === "#b64a30");

  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  check("7 关菜单后界面仍是保存值", (await toolboxBg()) === savedBg && (await toolboxAccent()) === "#b64a30");

  await page.locator(".toolbox-sub-bar button").first().click();
  await page.waitForTimeout(500);
  check("7 右上角返回按钮回工具箱主界面", await page.evaluate(() => !!document.querySelector(".toolbox-list")));
  check("7 工具箱无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 9 取色器统一：主题色 / 背景色 / 临期配色都是「取色只预览、保存才落盘」
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-due", nodeId: "entry-a", markdown: "任务", dueDate: localToday(0) })],
    // 临期高亮默认 off：这一节要验四档配色，先把档位打开
    settings: { features: { dueHighlight: "solid" } }
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator("button[title='列表菜单']").click();
  await page.waitForSelector(".context-menu .color-grid", { timeout: 8000 });
  await page.waitForTimeout(300);

  const readSettings = () => page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), SETTINGS_KEY);
  const readState = () => page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), STATE_KEY);
  const accentOf = () => page.evaluate(() => getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim());

  // 主题色：统一取色盘里改色 → 立即变色、不落盘
  const uiColorsBefore = JSON.stringify((await readSettings()).appearance?.uiColors ?? null);
  await page.locator(".context-menu .ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  {
    const hex = page.locator(".kx-color-hex");
    await hex.fill("#7a4fd0");
    await hex.press("Enter");
    await page.waitForTimeout(250);
  }
  check("9 主题色取色时界面立刻变色", (await accentOf()) === "#7a4fd0", await accentOf());
  check("9 主题色取色时不落盘", JSON.stringify((await readSettings()).appearance?.uiColors ?? null) === uiColorsBefore);
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  check(
    "9 主题色保存后才落盘",
    ((await readSettings()).appearance?.uiColors ?? {})["entry-a"] === "#7a4fd0",
    J((await readSettings()).appearance?.uiColors ?? null)
  );

  // 临期配色：取色 → 本页卡片底色跟着变，保存才写 appearance.dueColors
  const dueBefore = JSON.stringify((await readSettings()).appearance?.dueColors ?? null);
  const cardBg = () =>
    page.evaluate(() => {
      const el = document.querySelector(".task-card.due-soon");
      return el ? getComputedStyle(el).backgroundColor : "";
    });
  await page.locator(".context-menu .due-color-row .ui-color-picker").nth(1).click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  {
    const hex = page.locator(".kx-color-hex");
    await hex.fill("#00a000");
    await hex.press("Enter");
    await page.waitForTimeout(300);
  }
  check("9 临期色取色时卡片跟着变", (await cardBg()) !== "", await cardBg());
  check("9 临期色取色时不落盘", JSON.stringify((await readSettings()).appearance?.dueColors ?? null) === dueBefore);
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const dueSaved = ((await readSettings()).appearance?.dueColors ?? {})["entry-a"];
  check("9 临期色保存后落盘（四档一次写完）", Array.isArray(dueSaved) && dueSaved.length === 4 && dueSaved[0] === "#808080", J(dueSaved));

  // 背景色：预设色块单击即落盘（v0.8.5 需求 5）；自定义色盘才走草稿 → 保存
  const bgBefore = JSON.stringify((await readState()).backgrounds?.["entry-a"] ?? null);
  const pageBg = () =>
    page.evaluate(() => getComputedStyle(document.querySelector(".workspace")).backgroundColor);
  const bgOriginal = await pageBg();
  await page.locator(".context-menu .color-grid button").nth(3).click();
  await page.waitForTimeout(600);
  const bgAfterPreset = await pageBg();
  const presetColor = ((await readSettings()).appearance?.themePresets ?? [])[3]?.color ?? "";
  check(
    "9 预设背景色单击即落盘（不再要求保存）",
    bgAfterPreset !== bgOriginal &&
      ((await readState()).backgrounds?.["entry-a"]?.color ?? "") === presetColor &&
      (await page.locator(".context-menu .color-draft-actions").count()) === 0,
    `${bgOriginal} → ${bgAfterPreset} / ${presetColor}`
  );
  const presetSaved = JSON.stringify((await readState()).backgrounds?.["entry-a"] ?? null);

  // 自定义色盘：草稿预览 + 保存
  await page.locator(".context-menu .palette-button").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(300);
  {
    const hex = page.locator(".kx-color-hex");
    await hex.fill("#3355aa");
    await hex.press("Enter");
    await page.waitForTimeout(250);
  }
  await page.waitForTimeout(400);
  const bgPreview = await pageBg();
  check(
    "9 自定义取色只预览不落盘",
    bgPreview !== bgAfterPreset && JSON.stringify((await readState()).backgrounds?.["entry-a"] ?? null) === presetSaved,
    `${bgAfterPreset} → ${bgPreview}`
  );
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  check(
    "9 背景色保存后才落盘",
    ((await readState()).backgrounds?.["entry-a"]?.color ?? "") === "#3355aa",
    J((await readState()).backgrounds?.["entry-a"] ?? null)
  );
  check("9 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 8 固定区拖动排序：单列 / 双列 / 图标三种布局都要能用（二维落点）
// ===========================================================================
for (const layout of ["list", "grid", "icons"]) {
  const { page } = await freshPage(desktop);
  await page.evaluate(
    ({ key, layout }) => {
      localStorage.setItem(
        key,
        JSON.stringify({
          schemaVersion: 3,
          appearance: {
            navItems: ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox"],
            navLayout: layout
          }
        })
      );
    },
    { key: SETTINGS_KEY, layout }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const rowBox = (name) => page.locator(`.nav-row[title="${name}"]`).first().boundingBox();
  const order = () =>
    page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").appearance?.navItems ?? [], SETTINGS_KEY);
  const from = await rowBox("定时任务");
  const to = await rowBox("我的一天");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + 6, to.y + 2, { steps: 12 });
  await page.waitForTimeout(250);
  const dragging = await page.evaluate(() => !!document.querySelector(".nav-row.nav-drag-source"));
  const previewOrder = await page.evaluate(() =>
    [...document.querySelectorAll(".nav-row")].map((el) => el.getAttribute("title"))
  );
  await page.mouse.up();
  await page.waitForTimeout(600);
  const after = await order();
  check(
    `8（${layout}）拖动有抬起态且行实时让位`,
    dragging && previewOrder[0] === "定时任务",
    J({ dragging, previewOrder })
  );
  check(`8（${layout}）松手后顺序落盘`, after[0] === "scheduled", J(after));
  await page.close();
}

// ===========================================================================
// 8b 分组树拖动：拖到第 4 位 + 展开是高度动画（v0.8.6 需求 2 的拖动回归补做）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [
      { id: "group", kind: "category", name: "分组", icon: "folder", parentId: null },
      { id: "entry-1", kind: "entry", name: "条目一", icon: "inbox", parentId: "group" },
      { id: "entry-2", kind: "entry", name: "条目二", icon: "inbox", parentId: "group" },
      { id: "entry-3", kind: "entry", name: "条目三", icon: "inbox", parentId: "group" },
      { id: "entry-4", kind: "entry", name: "条目四", icon: "inbox", parentId: "group" }
    ]
  });
  const treeNames = () =>
    page.evaluate(() => [...document.querySelectorAll(".custom-nav .tree-row .list-name")].map((el) => el.textContent));
  const rowBox = (name) =>
    page.locator(`.custom-nav .tree-row:has(.list-name:text-is("${name}"))`).first().boundingBox();
  const from = await rowBox("条目一");
  const to = await rowBox("条目四");
  // 从「条目一」拖到「条目四」的下半段（落点 = 第 4 位）。
  // 横向落点取行宽的 62%：行首有图标按钮（落在它上面按下不会起拖）
  await page.mouse.move(from.x + from.width * 0.62, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + from.width * 0.62, from.y + from.height / 2 + 24, { steps: 6 });
  await page.mouse.move(to.x + to.width * 0.62, to.y + to.height * 0.8, { steps: 14 });
  await page.waitForTimeout(300);
  const dragPreview = await treeNames();
  await page.waitForTimeout(250);
  const settledPreview = await treeNames();
  await page.mouse.up();
  await page.waitForTimeout(700);
  const treeOrder = await page.evaluate(
    (key) => (JSON.parse(localStorage.getItem(key) ?? "{}").nodes ?? []).map((node) => node.id),
    STATE_KEY
  );
  check(
    "8b 拖动中行实时让位（条目一移到第 4 位）",
    dragPreview.join(",") === "分组,条目二,条目三,条目四,条目一",
    J(dragPreview)
  );
  check("8b 指针停住后预览不再抖（迟滞带生效）", settledPreview.join(",") === dragPreview.join(","), J(settledPreview));
  check(
    "8b 松手后落盘为第 4 位",
    treeOrder.join(",") === "my-day,planned,important,scheduled,group,entry-2,entry-3,entry-4,entry-1",
    J(treeOrder)
  );
  check("8b 拖动无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 8c 分组树展开/折叠：走高度动画（grid-template-rows 0fr → 1fr），不是瞬间占位
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [
      { id: "group", kind: "category", name: "分组", icon: "folder", parentId: null, collapsed: true },
      { id: "entry-1", kind: "entry", name: "条目一", icon: "inbox", parentId: "group" },
      { id: "entry-2", kind: "entry", name: "条目二", icon: "inbox", parentId: "group" }
    ]
  });
  const collapsed = await page.evaluate(() => {
    const wrap = document.querySelector(".tree-children");
    const row = document.querySelector(".tree-children .tree-row");
    return {
      wrapHeight: wrap ? wrap.getBoundingClientRect().height : -1,
      transition: wrap ? getComputedStyle(wrap).transitionProperty : "",
      // 收起态子树仍然挂载（高度动画的代价），但行被裁掉、不可命中
      rowMounted: Boolean(row),
      rowHit: row ? document.elementFromPoint(row.getBoundingClientRect().left + 20, row.getBoundingClientRect().top + 20)?.closest(".tree-row")?.dataset.nodeId ?? "" : "none"
    };
  });
  check("8c 收起态子树高度为 0（不占位）", collapsed.wrapHeight === 0, J(collapsed));
  check("8c 收起态的行不可命中（被裁切）", collapsed.rowHit !== "entry-1", J(collapsed));
  const anim = await page.evaluate(async () => {
    const wrap = document.querySelector(".tree-children");
    const button = document.querySelector(".tree-row .collapse-button");
    button.click();
    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const mid = wrap.getBoundingClientRect().height;
    await new Promise((resolve) => setTimeout(resolve, 300));
    return {
      mid,
      settled: wrap.getBoundingClientRect().height,
      transition: getComputedStyle(wrap).transitionProperty + " " + getComputedStyle(wrap).transitionDuration
    };
  });
  check(
    "8c 展开是高度动画（中途高度 < 最终高度）",
    anim.settled > 40 && anim.mid >= 0 && anim.mid < anim.settled,
    J(anim)
  );
  check("8c 动画时长与兄弟行 flip 对齐（150ms）", anim.transition.includes("grid-template-rows") && anim.transition.includes("0.15s"), J(anim));
  check("8c 展开无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 10 记账统计：段控只留支出|收入（移动端加回「周」），图例三枚胶囊切曲线
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate((key) => {
    const now = new Date().toISOString();
    const iso = (offset) => new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10);
    localStorage.setItem(
      key,
      JSON.stringify({
        accounts: [{ id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now }],
        categories: [
          { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now },
          { id: "lcat-inc-01", name: "工资", side: "income", icon: "Wallet", color: "#2f9e6e", parentId: "", createdAt: now }
        ],
        entries: Array.from({ length: 40 }, (_, i) => ({
          id: `le-${i}`,
          kind: i % 4 === 0 ? "income" : "expense",
          amountCents: 1000 + i * 700,
          accountId: "lacc-01",
          categoryId: i % 4 === 0 ? "lcat-inc-01" : "lcat-exp-01",
          date: iso(i % 25),
          note: `第 ${i} 笔`,
          createdAt: now
        })),
        accountTypes: []
      })
    );
  }, LEDGER_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await page.locator(".nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(600);
  await page.click(".ledger-view-switch button[title='统计视图']");
  await page.waitForTimeout(500);

  const stats = () =>
    page.evaluate(() => ({
      segments: [...document.querySelectorAll(".ledger-stats-bar .ledger-segmented button")].map((el) => el.textContent?.trim()),
      title: document.querySelector(".ledger-panel-head h2")?.textContent?.trim() ?? "",
      legend: [...document.querySelectorAll(".ledger-legend button")].map((el) => ({
        text: el.textContent?.trim(),
        on: el.classList.contains("on"),
        dot: getComputedStyle(el.querySelector("i")).backgroundColor
      })),
      lines: [...document.querySelectorAll(".ledger-line")].map((el) => el.getAttribute("class"))
    }));

  let state = await stats();
  check("10 侧段控只剩支出|收入（结余已去掉）", !state.segments.includes("结余") && state.segments.includes("支出") && state.segments.includes("收入"), J(state.segments));
  check("10 折线图标题固定「收支趋势」", state.title === "收支趋势", state.title);
  // v0.8.5 需求 8：图例默认跟随侧段控——支出侧只有支出一条
  check("10 默认图例跟随侧段控（支出侧只亮支出）", state.lines.length === 1 && state.legend[1].on && !state.legend[0].on && !state.legend[2].on, J(state));
  check("10 灰态色块是中性的（不是透明）", state.legend[2].dot === "rgb(200, 204, 210)", state.legend[2].dot);

  await page.locator(".ledger-legend button", { hasText: "收入" }).click();
  await page.waitForTimeout(700);
  state = await stats();
  check("10 手动点亮收入后画两条线", state.lines.length === 2 && state.legend[0].on, J(state.lines));

  await page.locator(".ledger-legend button", { hasText: "结余" }).click();
  await page.waitForTimeout(700);
  state = await stats();
  check("10 再点结余后画三条线", state.lines.length === 3 && state.legend[2].on, J(state.lines));

  // 切侧把图例重置为该侧单条亮（手动叠出来的三条线不该留到下一侧）
  await page.locator(".ledger-side-switch button", { hasText: "收入" }).click();
  await page.waitForTimeout(800);
  state = await stats();
  check(
    "10 切到收入侧图例重置为单条（收入亮）",
    state.lines.length === 1 && state.legend[0].on && !state.legend[1].on && !state.legend[2].on,
    J(state)
  );

  await page.locator(".ledger-side-switch button", { hasText: "支出" }).click();
  await page.waitForTimeout(800);
  state = await stats();
  check(
    "10 切回支出侧同样单条（支出亮）",
    state.lines.length === 1 && state.legend[1].on && !state.legend[0].on,
    J(state)
  );
  check("10 记账统计无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// 10b 移动端也有「周」，文案收短
{
  const { page } = await freshPage(mobile);
  await page.locator(".nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(600);
  await page.click(".ledger-view-switch button[title='统计视图']");
  await page.waitForTimeout(500);
  const segments = await page.evaluate(() =>
    [...document.querySelectorAll(".ledger-stats-bar .ledger-segmented button")].map((el) => el.textContent?.trim())
  );
  check("10 移动端把「周」加回来了", segments.includes("周") && segments.includes("支") && segments.includes("收"), J(segments));
  const barBox = await page.locator(".ledger-stats-bar").boundingBox();
  check("10 移动端段控没超出屏宽", Boolean(barBox) && barBox.x >= 0 && barBox.x + barBox.width <= 394, J(barBox));
  await page.close();
}

// ===========================================================================
// 11 编辑器日期工具：共用 CalendarGrid，选中那天必须看得见（不是白字透明底）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-date", nodeId: "entry-a", markdown: "带日期的任务", dueDate: localToday(0) })]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.waitForTimeout(700);
  await page.locator(".editor-meta-trigger").first().click();
  await page.waitForSelector(".editor-meta-pop .date-reminder-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const cell = await page.evaluate(() => {
    const el = document.querySelector(".editor-meta-pop .dp-cell.selected");
    if (!el) return null;
    const cs = getComputedStyle(el);
    return { text: el.textContent, color: cs.color, bg: cs.backgroundColor };
  });
  check("11 编辑器日历的选中日有实底、数字看得见", Boolean(cell) && cell.bg !== "rgba(0, 0, 0, 0)" && cell.color !== cell.bg, J(cell));
  check("11 编辑器无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 12 日期与提醒面板尺寸稳定：点哪天都一样宽；自定义的日期↔时间标签页等高
// ===========================================================================
{
  const { page } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-date", nodeId: "entry-a", markdown: "带日期的任务", dueDate: localToday(0), dueTime: "18:00" })]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".date-reminder-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const panelBox = () => page.locator(".date-reminder-panel").first().boundingBox();

  const before = await panelBox();
  await page.locator(".date-reminder-panel .dp-cell.today").first().click();
  await page.waitForTimeout(400);
  const afterToday = await panelBox();
  await page.locator(".date-reminder-panel .dp-cell").nth(20).click();
  await page.waitForTimeout(400);
  const afterOther = await panelBox();
  const sameWidth = (a, b) => Math.abs((a?.width ?? 0) - (b?.width ?? 0)) < 1.5;
  check("12 点今天与点别的日子宽度不变", sameWidth(before, afterToday) && sameWidth(afterToday, afterOther), J([before?.width, afterToday?.width, afterOther?.width]));

  // 自定义提醒：日期 ↔ 时间 标签页等高（上下两行不动）
  await page.locator(".date-reminder-panel .dr-reminder-empty").click();
  await page.waitForSelector(".date-reminder-panel .dr-add-menu", { timeout: 5000 });
  await page.locator(".date-reminder-panel .dr-add-menu button").nth(4).click();
  await page.waitForTimeout(500);
  const dateTab = await panelBox();
  await page.locator(".date-reminder-panel .dr-tab", { hasText: "时间" }).click();
  await page.waitForTimeout(500);
  const timeTab = await panelBox();
  check(
    "12 自定义的日期/时间标签页尺寸一致（按钮不跳位）",
    sameWidth(dateTab, timeTab) && Math.abs((dateTab?.height ?? 0) - (timeTab?.height ?? 0)) < 1.5,
    J([dateTab, timeTab])
  );
  const gridWidth = await page.evaluate(() => {
    const el = document.querySelector(".date-reminder-panel .dp-time-wheel");
    return el ? Math.round(el.getBoundingClientRect().width / 0.75) : 0;
  });
  check("12 双轨与日历同宽（228）", gridWidth === 228, `width=${gridWidth}`);
  await page.close();
}

// ===========================================================================
// 13 日记编辑器的日期按钮：只有日历，没有时刻（与卡片右键「修改日期」同款）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "日记" }).click();
  await page.waitForTimeout(700);
  await page.locator(".diary-fab").click();
  await page.waitForSelector(".editor-dialog.diary-editor", { timeout: 10000 });
  await page.waitForTimeout(700);
  await page.locator(".editor-meta-trigger").first().click();
  await page.waitForSelector(".editor-meta-pop .date-picker", { timeout: 8000 });
  await page.waitForTimeout(400);
  const pop = await page.evaluate(() => {
    const el = document.querySelector(".editor-meta-pop");
    return {
      timeSwitch: !!el?.querySelector(".dp-time-switch"),
      timeTrigger: !!el?.querySelector(".dp-time-trigger"),
      calendar: !!el?.querySelector(".date-picker-grid"),
      actions: [...(el?.querySelectorAll(".date-picker-actions button") ?? [])].map((b) => b.textContent?.trim())
    };
  });
  check("13 日记编辑器日期浮层没有「勾选框 + 时刻」那一行", !pop.timeSwitch && !pop.timeTrigger, J(pop));
  check("13 只剩日历与清除/今天", pop.calendar && pop.actions.join("/") === "清除/今天", J(pop.actions));
  check("13 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 14 日记/记账右上角：搜索独立成按钮，齿轮直弹菜单；记账把两个管理器收进菜单
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const header = (cls) =>
    page.evaluate((cls) => {
      const el = document.querySelector(`.${cls} .header-actions`);
      return [...(el?.children ?? [])].map((child) => child.getAttribute("title") ?? child.className);
    }, cls);

  await page.locator(".nav-row", { hasText: "日记" }).click();
  await page.waitForTimeout(700);
  let titles = await header("diary-view");
  check(
    "14 日记头部依次是 分视图 / 搜索 / 日记菜单",
    titles.length === 3 && titles[1] === "搜索日记" && titles[2] === "日记菜单",
    J(titles)
  );
  await page.locator(".diary-view .header-actions button[title='搜索日记']").click();
  await page.waitForTimeout(400);
  check("14 日记点搜索直接出输入框", await page.evaluate(() => !!document.querySelector(".diary-search")));
  await page.locator(".diary-view .header-actions button[title='关闭搜索']").click();
  await page.waitForTimeout(300);
  await page.locator(".diary-view .header-actions button[title='日记菜单']").click();
  await page.waitForTimeout(500);
  const diaryMenu = await page.evaluate(() =>
    [...document.querySelectorAll(".context-menu .menu-item-button")].map((el) => el.textContent?.trim())
  );
  check("14 日记齿轮直弹日记菜单（不再有中间面板）", diaryMenu.some((item) => item?.includes("导出全部日记")), J(diaryMenu));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  await page.locator(".nav-row", { hasText: "记账" }).click();
  await page.waitForTimeout(700);
  titles = await header("ledger-view");
  check(
    "14 记账头部依次是 分视图 / 搜索 / 记账菜单",
    titles.length === 3 && titles[1] === "搜索记账" && titles[2] === "记账菜单",
    J(titles)
  );
  await page.locator(".ledger-view .header-actions button[title='搜索记账']").click();
  await page.waitForTimeout(400);
  check("14 记账点搜索直接出输入框", await page.evaluate(() => !!document.querySelector(".ledger-search")));
  await page.locator(".ledger-view .header-actions button[title='关闭搜索']").click();
  await page.waitForTimeout(300);
  await page.locator(".ledger-view .header-actions button[title='记账菜单']").click();
  await page.waitForTimeout(600);
  const ledgerMenu = await page.evaluate(() =>
    [...document.querySelectorAll(".context-menu .menu-item-button")].map((el) => el.textContent?.trim())
  );
  check(
    "14 记账菜单里有分类管理与账户与转账",
    ledgerMenu.includes("分类管理") && ledgerMenu.includes("账户与转账"),
    J(ledgerMenu)
  );
  check("14 记账菜单去掉了排序方式", !ledgerMenu.some((item) => item?.includes("排序方式")), J(ledgerMenu));
  check("14 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 15 编辑器编辑态：字体与草稿纸同源；列表不染蓝；有序列表三级编号与渲染层级
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(() => {
    const key = "todo-note-settings-v3";
    localStorage.setItem(key, JSON.stringify({ schemaVersion: 3, appearance: { editorFontSize: 24 } }));
  });
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-list", nodeId: "entry-a", markdown: "正文行\n1. 甲\n2. 乙\n- 无序项" })]
  });

  // 15.1 编辑器字体与字号（跟草稿纸同源：--font-body + --editor-font-size）
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.waitForTimeout(900);
  const font = await page.evaluate(() => {
    const scroller = document.querySelector(".editor-cm-host .cm-scroller");
    const cs = getComputedStyle(scroller);
    return {
      family: cs.fontFamily.replace(/\s+/g, ""),
      size: cs.fontSize,
      rootFamily: getComputedStyle(document.documentElement).fontFamily.replace(/\s+/g, "")
    };
  });
  check("15.1 编辑器正文用基础字体（不是等宽栈）", font.family === font.rootFamily && !font.family.includes("monospace"), J(font));
  check("15.1 编辑器字号跟着设置（24px）", font.size === "24px", font.size);

  // 草稿纸同源
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  const padSize = await page.evaluate(() => getComputedStyle(document.querySelector(".scratchpad-area")).fontSize);
  check("15.1 草稿纸字号与编辑器同一个设置", padSize === "24px", padSize);
  await page.locator(".toolbox-sub-bar button").first().click();
  await page.waitForTimeout(400);

  // 15.2 列表内容与正文同色（不是蓝色）
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.waitForTimeout(900);
  const colors = await page.evaluate(() => {
    const lines = [...document.querySelectorAll(".editor-cm-host .cm-line")];
    return lines.map((line) => ({
      text: line.textContent ?? "",
      color: getComputedStyle(line).color
    }));
  });
  const normal = colors.find((item) => item.text.startsWith("-"))?.color;
  const ordered = colors.find((item) => item.text.startsWith("1."))?.color;
  const body = colors.find((item) => item.text.startsWith("正文行"))?.color;
  // 与正文同色比较，别写死色值（v0.8.5 需求 23）：主题一换就误报的断言等于没有
  check(
    "15.2 有序/无序列表与正文同色（不单独染蓝）",
    Boolean(body) && normal === body && ordered === body,
    J(colors)
  );

  // 15.3.2 / 15.3.1 三级缩进与续号
  await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.focus());
  await page.keyboard.press("Control+A");
  await page.keyboard.insertText("1. 甲\n2. 乙\n1. 子一");
  await page.waitForTimeout(300);
  await page.keyboard.press("Control+End");
  await page.keyboard.press("Home");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await page.waitForTimeout(300);
  let text = await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.textContent ?? "");
  check("15.3.2 Tab 每级缩进 3 空格（有序父项要 ≥3 才不被摊平）", text.includes("      1. 子一"), JSON.stringify(text));
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.keyboard.insertText("子二");
  await page.waitForTimeout(400);
  text = await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.textContent ?? "");
  // 第三级（6 空格）回车继续递增：**缩进层级也要断言**，只比 "1. …2. …" 二级文本也满足（v0.8.5 需求 22）
  // 注意 textContent 不含换行（每行是独立的 .cm-line），所以用 \s* 而不是 \s+
  check(
    "15.3.1 第三级回车继续递增编号（同级缩进不变）",
    /      1\. 子一\s*      2\. 子二/.test(text.replace(/\u00a0/g, " ")),
    JSON.stringify(text)
  );

  // 真四级嵌套（v0.8.5 需求 22）：层次写全（跨级缩进在 CommonMark 里不算新层级），
  // 光标放第四级行尾回车，应继续第四级的编号
  await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.focus());
  await page.keyboard.press("Control+A");
  await page.keyboard.insertText("1. 甲\n   1. 乙\n      1. 丙\n         1. 丁");
  await page.waitForTimeout(300);
  await page.keyboard.press("Control+End");
  await page.waitForTimeout(200);
  await page.keyboard.press("Enter");
  await page.keyboard.insertText("继续");
  await page.waitForTimeout(400);
  text = await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.textContent ?? "");
  check(
    "15.3.1 四级嵌套回车继续同级递增（9 空格 = 第四级）",
    /         1\. 丁\s*         2\. 继续/.test(text.replace(/\u00a0/g, " ")),
    JSON.stringify(text)
  );
  check("15 编辑器无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await page.close();
}

// 15.3.3 渲染态：层级编号 2.1 / 2.1.1（源码仍是标准 markdown）
{
  const { page } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [
      makeTask({
        id: "t-nest",
        nodeId: "entry-a",
        markdown: ["1. 甲", "2. 乙", "   1. 子一", "   2. 子二", "3. 丙"].join("\n")
      })
    ]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(700);
  await page.locator(".task-card .expand-button, .task-card .edit-button").first().waitFor({ timeout: 5000 }).catch(() => {});
  // 展开卡片让正文渲染出来
  const expanded = await page.evaluate(() => document.querySelectorAll(".markdown-body ol").length);
  if (expanded === 0) {
    await page.locator(".task-card").first().dblclick();
    await page.waitForTimeout(700);
  }
  const markers = await page.evaluate(() => {
    const items = [...document.querySelectorAll(".markdown-body ol > li")];
    return items.map((el) => ({
      text: (el.firstChild?.textContent ?? "").trim().slice(0, 8),
      counter: getComputedStyle(el, "::before").content
    }));
  });
  check("15.3.3 渲染态用 CSS counters 拼层级编号", markers.length > 0 && markers.every((m) => m.counter.includes("kx-ol")), J(markers));
  const screenshot = await page.locator(".markdown-body").first().boundingBox();
  if (screenshot) {
    await page.screenshot({ path: "test-data/v084-nested-ol.png", clip: screenshot });
  }
  await page.close();
}


// ===========================================================================
// 3/4/5 长列表窗口化：日记 2000 篇与任务 300 条都只挂视口附近的一段
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const seeded = await page.evaluate(
    ({ stateKey, diaryKey, diaryCount, taskCount }) => {
      const now = new Date().toISOString();
      const iso = (offset) => new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10);
      const entries = Array.from({ length: diaryCount }, (_, i) => {
        const date = iso(Math.floor(i / 3));
        return {
          id: `big-diary-${i}`,
          date,
          title: `基准日记 ${i}`,
          markdown: `第 ${i} 篇的正文。`.repeat(4),
          tags: [], mood: "", weather: "", expanded: false,
          createdAt: `${date}T09:00:00.000Z`,
          updatedAt: `${date}T09:00:00.000Z`
        };
      });
      const tasks = Array.from({ length: taskCount }, (_, i) => ({
        id: `big-task-${i}`,
        nodeId: "big-entry",
        markdown: `第 ${i} 条基准任务标题，带一点长度用来触发折行判定。`,
        completed: false, important: false, myDay: false,
        tags: [], emojis: [], expanded: false,
        createdAt: now, updatedAt: now, order: i + 1
      }));
      try {
        localStorage.setItem(diaryKey, JSON.stringify({ entries }));
        localStorage.setItem(stateKey, JSON.stringify({
          schemaVersion: 3,
          nodes: [
            { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
            { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
            { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
            { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now },
            { id: "big-entry", kind: "entry", name: "大条目", icon: "inbox", parentId: null, createdAt: now }
          ],
          tasks,
          backgrounds: {},
          selectedNodeId: "big-entry"
        }));
      } catch (error) {
        return String(error);
      }
      return true;
    },
    { stateKey: STATE_KEY, diaryKey: "todo-note-diary-v1", diaryCount: 2000, taskCount: 300 }
  );
  check("3/5 大数据种子写入成功（localStorage 配额内）", seeded === true, String(seeded));
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(1000);

  // 任务列表（300 条 > 100 阈值 → 窗口化）
  await page.locator(".tree-row", { hasText: "大条目" }).click();
  await page.waitForTimeout(900);
  const taskView = await page.evaluate(() => ({
    cards: document.querySelectorAll(".task-list .task-card").length,
    spacers: [...document.querySelectorAll(".task-list .virtual-spacer")].map((el) => el.style.height)
  }));
  check("5 300 条任务只挂一段（>100 阈值触发窗口化）", taskView.cards > 0 && taskView.cards < 100, J(taskView));

  // ≤100 条：全量直出（不为小列表付虚拟化的代价）
  await page.evaluate(({ stateKey }) => {
    const state = JSON.parse(localStorage.getItem(stateKey) ?? "{}");
    state.tasks = state.tasks.slice(0, 40);
    localStorage.setItem(stateKey, JSON.stringify(state));
  }, { stateKey: STATE_KEY });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await page.locator(".tree-row", { hasText: "大条目" }).click();
  await page.waitForTimeout(900);
  const small = await page.evaluate(() => document.querySelectorAll(".task-list .task-card").length);
  check("5 40 条任务全量直出", small === 40, `${small} 张`);

  // 日记列表（2000 篇）
  await page.evaluate(() => {
    const key = "todo-note-settings-v3";
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.diary = { ...(settings.diary ?? {}), view: "list" };
    localStorage.setItem(key, JSON.stringify(settings));
  });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(1000);
  const diaryOpenStart = Date.now();
  await page.locator(".nav-row", { hasText: "日记" }).click();
  await page.waitForSelector(".diary-card", { timeout: 20000 });
  const diaryOpen = Date.now() - diaryOpenStart;
  const diaryView = await page.evaluate(() => ({
    cards: document.querySelectorAll(".diary-card").length,
    height: document.querySelector(".diary-scroll")?.scrollHeight ?? 0
  }));
  check("3 2000 篇日记只挂视口附近的一段", diaryView.cards > 0 && diaryView.cards < 100, J(diaryView));
  check("3 日记打开够快（本地预览路径 < 3s）", diaryOpen < 3000, `${diaryOpen}ms`);
  check("3 滚动条长度仍是全量（占位撑住了）", diaryView.height > 100000, `${diaryView.height}px`);

  // 滚到深处：窗口跟着走
  await page.evaluate(() => {
    document.querySelector(".diary-scroll").scrollTop = 60000;
  });
  await page.waitForTimeout(800);
  const deep = await page.evaluate(() => ({
    cards: document.querySelectorAll(".diary-card").length,
    first: document.querySelector(".diary-card")?.textContent?.trim().slice(0, 16) ?? "",
    spacer: [...document.querySelectorAll(".diary-scroll .virtual-spacer")].map((el) => el.style.height)
  }));
  check("3 滚到深处窗口跟着走（不是停在开头）", deep.cards > 0 && deep.cards < 100 && deep.spacer[0] !== "0px", J(deep));

  // 分组视图：默认全折叠，展开一组才挂条目（需求 4）
  await page.locator(".diary-view-switch button[title='分组视图']").click();
  await page.waitForTimeout(900);
  const grouped = await page.evaluate(() => ({
    cards: document.querySelectorAll(".diary-card").length,
    heads: document.querySelectorAll(".diary-group-head").length
  }));
  check("4 分组视图默认全折叠（一张卡都不挂）", grouped.cards === 0 && grouped.heads > 0, J(grouped));
  await page.locator(".diary-group-head").first().click();
  await page.waitForTimeout(600);
  const afterYear = await page.evaluate(() => ({
    cards: document.querySelectorAll(".diary-card").length,
    subheads: document.querySelectorAll(".diary-group-subhead").length
  }));
  check("4 展开一个年只出月份小标题（不出条目）", afterYear.cards === 0 && afterYear.subheads > 0, J(afterYear));
  await page.locator(".diary-group-subhead").first().click();
  await page.waitForTimeout(700);
  const afterMonth = await page.evaluate(() => document.querySelectorAll(".diary-card").length);
  check("4 再展开一个月才挂该月条目", afterMonth > 0 && afterMonth < 200, `${afterMonth} 张`);
  check("3/4/5 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}


// ===========================================================================
// 2 草稿纸：提示行没了、防抖降频、正文进数据域（跟着「同步数据」走）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.waitForTimeout(400);
  check("2 「正在输入&已自动保存」提示行已隐藏", !(await page.evaluate(() => !!document.querySelector(".scratchpad-status"))));

  await page.locator(".scratchpad-area").fill("第一行草稿");
  await page.waitForTimeout(700);
  const early = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").scratchpad ?? null, STATE_KEY);
  check("2 防抖期内不写盘（降频到秒级）", early === null, J(early));
  await page.waitForTimeout(5200);
  const late = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").scratchpad ?? null, STATE_KEY);
  check("2 停手后落进数据域（带 updatedAt，可同步）", late?.text === "第一行草稿" && Boolean(late?.updatedAt), J(late));
  check("2 localStorage 仍留一份首帧缓存", (await page.evaluate(() => localStorage.getItem("kxtodo-scratchpad-v1"))) === "第一行草稿");

  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.waitForTimeout(500);
  check("2 重载后正文还在", (await page.locator(".scratchpad-area").inputValue()) === "第一行草稿");

  // 退出当前页面必保存（不等防抖）
  await page.locator(".scratchpad-area").fill("退出前最后一笔");
  await page.locator(".toolbox-sub-bar button").first().click();
  await page.waitForTimeout(700);
  const afterLeave = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").scratchpad ?? null, STATE_KEY);
  check("2 离开页面立即保存", afterLeave?.text === "退出前最后一笔", J(afterLeave));
  check("2 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}


// ===========================================================================
// 1 文件传输助手：身份区 / 在线 / 设备 / 接收待命 / 历史（浏览器预览下的界面形态）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "文件传输助手" }).click();
  await page.waitForSelector(".transfer-page", { timeout: 8000 });
  await page.waitForTimeout(400);

  const shell = await page.evaluate(() => {
    const root = document.querySelector(".transfer-page");
    const width = Math.round(root?.getBoundingClientRect().width ?? 0);
    return {
      width,
      overflow: document.body.scrollWidth - window.innerWidth,
      tabs: [...document.querySelectorAll(".transfer-tabs button")].map((el) => el.textContent?.trim()),
      actions: [...document.querySelectorAll(".transfer-action-button")].map((el) => el.textContent?.trim()),
      namePlaceholder: document.querySelector(".transfer-field input")?.getAttribute("placeholder") ?? "",
      // v0.8.6 需求 5.5：口令行改成普通文本输入框（去掉密码框样式与眼睛按钮）
      codePlaceholder:
        [...document.querySelectorAll(".transfer-identity input")].find((input) =>
          (input.getAttribute("placeholder") ?? "").includes("密钥")
        )?.getAttribute("placeholder") ?? "",
      codeTypes: [...document.querySelectorAll(".transfer-identity input")].map((input) => input.type).join(","),
      hasEye: !!document.querySelector(".transfer-eye"),
      empty: document.querySelector(".transfer-empty")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
      hasSendCapsule: !!document.querySelector(".transfer-send-button"),
      sendDisabled: document.querySelector(".transfer-send-button")?.hasAttribute("disabled") ?? null,
      history: !!document.querySelector(".transfer-history")
    };
  });
  check("1.1 顶部有设备名输入（带提示词）", shell.namePlaceholder.includes("输入设备名"), shell.namePlaceholder);
  check(
    "1.2 配对口令是普通文本输入框（v0.8.6 需求 5.5 去掉了眼睛按钮）",
    shell.codePlaceholder.includes("输入和对方约定的密钥") &&
      !shell.hasEye &&
      shell.codeTypes.includes("text") &&
      !shell.codeTypes.includes("password"),
    J(shell)
  );
  check("1.3 发送/接收滑块与四个大图标", shell.tabs.join("/") === "发送/接收" && shell.actions.join("/") === "文件/文件夹/文本/剪贴板", J(shell));
  check("1.3 空状态文案与端到端加密标识", shell.empty.includes("输入相同口令以匹配") && shell.empty.includes("端到端加密"), shell.empty);
  check("1.3 发送按钮是胶囊且未上线时禁用", shell.hasSendCapsule && shell.sendDisabled === true, J(shell));
  check("1.4 有传输历史折叠区", shell.history);
  check("1.5 内容列限宽（桌面 720px 内、不超屏）", shell.width <= 720 && shell.overflow === 0, J(shell));

  // 未上线时点发送：不该有任何反应（按钮禁用）
  await page.locator(".transfer-tabs button", { hasText: "接收" }).click();
  await page.waitForTimeout(300);
  const receive = await page.evaluate(() => ({
    text: document.querySelector(".transfer-receive-head")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
    auto: !!document.querySelector(".transfer-auto input"),
    hasOpen: [...document.querySelectorAll(".transfer-save-row button")].some((el) => (el.textContent ?? "").includes("打开文件夹"))
  }));
  check("1.3 接收页有保存位置与打开文件夹", receive.text.includes("打开文件夹") && receive.hasOpen, receive.text);
  check("1.3 接收页有「自动接收」开关", receive.auto);
  check("1 传输界面无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// 1b 移动端：单列、四宫格、不超屏（需求 1.6）
{
  const { page } = await freshPage(mobile);
  await page.locator(".nav-row", { hasText: "工具箱" }).click();
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "文件传输助手" }).click();
  await page.waitForSelector(".transfer-page", { timeout: 8000 });
  await page.waitForTimeout(500);
  const mobileInfo = await page.evaluate(() => {
    const actions = document.querySelector(".transfer-actions");
    const root = document.querySelector(".transfer-page");
    const rect = root?.getBoundingClientRect();
    const columns = actions ? getComputedStyle(actions).gridTemplateColumns.split(" ").length : 0;
    return {
      columns,
      right: Math.round(rect?.right ?? 0),
      viewport: window.innerWidth,
      overflow: document.body.scrollWidth - window.innerWidth
    };
  });
  check("1.6 移动端四个大图标是 2×2", mobileInfo.columns === 2, J(mobileInfo));
  check("1.6 移动端不超屏", mobileInfo.overflow === 0 && mobileInfo.right <= mobileInfo.viewport + 1, J(mobileInfo));
  await page.close();
}

// ===========================================================================
// 33 渲染态勾选的手术式更新（v0.8.5）：长文档点勾选不再整篇重渲
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const body = [
    "## 长文档勾选",
    "",
    "- [ ] 第一项",
    "- [ ] 第二项",
    "",
    "```js",
    "const a = 1;",
    "function hello(name) {",
    "  return `你好 ${name}`;",
    "}",
    "```",
    "",
    "正文段落若干。" + "这是一段很长的说明文字，用来把文档推过 1000 字的两阶段渲染阈值。".repeat(30),
    "",
    "- [x] 已完成项"
  ].join("\n");
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null }],
    tasks: [makeTask({ id: "t-tick", markdown: body, expanded: true })]
  });
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(800);
  // 等完整版渲染落地（代码块被 highlight.js 上色）
  await page.waitForSelector(".markdown-content code.hljs", { timeout: 10000 });
  await page.waitForTimeout(300);
  const prepared = await page.evaluate(() => {
    const code = document.querySelector(".markdown-content code.hljs");
    window.__codeRef = code;
    window.__fastBefore = window.__kxtodoRenderStats.fast;
    return {
      fast: window.__kxtodoRenderStats.fast,
      boxes: document.querySelectorAll(".markdown-content input.md-task-box").length,
      highlighted: Boolean(code)
    };
  });
  check("33 长文档完整渲染落地（代码块已高亮、任务框在）", prepared.boxes === 3 && prepared.highlighted, J(prepared));

  // 点第一个勾选框：同一帧内 checked 已翻、代码块节点身份未变、快速版没有重跑
  await page.locator(".markdown-content input.md-task-box").first().click();
  const afterTick = await page.evaluate(() => {
    const box = document.querySelector(".markdown-content input.md-task-box");
    const code = document.querySelector(".markdown-content code.hljs");
    const firstLi = document.querySelector(".markdown-content li");
    return {
      checked: box?.checked ?? null,
      done: Boolean(firstLi?.classList.contains("md-task-done")),
      sameCode: window.__codeRef === code && code?.isConnected === true,
      fast: window.__kxtodoRenderStats.fast,
      fastBefore: window.__fastBefore,
      label: firstLi?.querySelector(".md-task-label")?.textContent?.trim() ?? ""
    };
  });
  check("33 点击帧内勾选框已翻转且打上删除线标记", afterTick.checked === true && afterTick.done && afterTick.label.includes("第一项"), J(afterTick));
  check("33 代码块高亮节点没有被替换（未整篇重渲）", afterTick.sameCode, J(afterTick));
  check("33 快速版渲染没有重跑（renderStats.fast 不增长）", afterTick.fast === afterTick.fastBefore, J(afterTick));

  // 再点一次（勾回去）：拆 span 也要等价，且同样不重渲
  await page.locator(".markdown-content input.md-task-box").first().click();
  const afterUntick = await page.evaluate(() => {
    const box = document.querySelector(".markdown-content input.md-task-box");
    const code = document.querySelector(".markdown-content code.hljs");
    const firstLi = document.querySelector(".markdown-content li");
    return {
      checked: box?.checked ?? null,
      done: Boolean(firstLi?.classList.contains("md-task-done")),
      sameCode: window.__codeRef === code && code?.isConnected === true,
      fast: window.__kxtodoRenderStats.fast,
      fastBefore: window.__fastBefore
    };
  });
  check("33 再点一次勾选框回原态且仍未重渲", afterUntick.checked === false && !afterUntick.done && afterUntick.sameCode && afterUntick.fast === afterUntick.fastBefore, J(afterUntick));

  // 文本对不上（写失败回滚 / 远端同步 / 编辑器改写都走这条路）→ 落回正常重渲。
  // 浏览器预览里没有写失败路径，这里用编辑器把正文写回**原文**来模拟回滚后的那一次源变更：
  // 卡片应当重渲，勾选框回到与源码一致的状态。
  await page.locator(".markdown-content input.md-task-box").first().click();
  await page.waitForTimeout(400);
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.waitForTimeout(900);
  await page.evaluate(() => document.querySelector(".editor-cm-host .cm-content")?.focus());
  await page.keyboard.press("Control+A");
  await page.keyboard.insertText(body);
  await page.keyboard.press("Control+s");
  await page.waitForSelector(".editor-dialog", { state: "detached", timeout: 10000 });
  await page.waitForTimeout(700);
  const reverted = await page.evaluate(() => {
    const firstLi = document.querySelector(".markdown-content li");
    return {
      checked: document.querySelector(".markdown-content input.md-task-box")?.checked ?? null,
      done: Boolean(firstLi?.classList.contains("md-task-done")),
      source: JSON.parse(localStorage.getItem("todo-note-state-v3")).tasks.find((t) => t.id === "t-tick")?.markdown?.slice(0, 12) ?? ""
    };
  });
  check("33 文本被改回原样时重渲纠正（勾选框回到与源码一致）", reverted.checked === false && !reverted.done, J(reverted));
  check("33 勾选手术式更新无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

console.log(`\n合计：${passes} 通过 / ${failures} 失败`);
await browser.close();
process.exit(failures === 0 ? 0 : 1);

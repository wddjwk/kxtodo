// v0.8.9 定向回归：剪切板工具页、应用内 iframe、移动端选择菜单、定时任务、
// 取色盘窄屏、侧栏右键菜单、记账报表统计。
// 需要先 npm run dev（vite 1420）。
import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const url = process.env.KXTODO_TEST_URL || "http://127.0.0.1:1420/";
const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
let passes = 0;
let failures = 0;
function check(name, ok, detail = "") {
  if (ok) {
    passes += 1;
    console.log(`  PASS ${name}`);
  } else {
    failures += 1;
    console.log(`  FAIL ${name}${detail ? ` :: ${detail}` : ""}`);
  }
}
const J = (value) => JSON.stringify(value);

async function freshPage(mobile = false, { uiScale = 1, linkRender = "off" } = {}) {
  const context = await browser.newContext(mobile ? {
    viewport: { width: 392, height: 850 }, isMobile: true, hasTouch: true,
    userAgent: "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
  } : { viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();
  page.setDefaultNavigationTimeout(120000);
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.addInitScript(({ uiScale, linkRender }) => {
    const now = new Date().toISOString();
    const nodes = [
      { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
      { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
      { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
      { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now },
      { id: "cat-a", kind: "category", name: "分组甲", icon: "folder", parentId: null, createdAt: now, expanded: true },
      { id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: "cat-a", createdAt: now }
    ];
    localStorage.setItem("todo-note-state-v3", JSON.stringify({
      nodes,
      tasks: [{
        id: "task-a", nodeId: "entry-a", markdown: "第一行文字，看 [示例](https://example.com)\n第二行文字",
        completed: false, important: false, pinned: false, myDay: false, tags: [], emojis: [],
        expanded: true, createdAt: now
      }],
      backgrounds: {}, selectedNodeId: "entry-a"
    }));
    localStorage.setItem("todo-note-settings-v3", JSON.stringify({
      appearance: { uiScale, linkOpenMode: "app" },
      features: { sync: false, linkRender },
      updates: { autoCheck: false }
    }));
    localStorage.setItem("todo-note-diary-v1", JSON.stringify({
      entries: [{
        id: "diary-a", date: new Date().toISOString().slice(0, 10), title: "日记甲",
        markdown: "日记里的 [链接](https://example.com)", mood: "", weather: "", tags: [],
        createdAt: now, updatedAt: now
      }]
    }));
  }, { uiScale, linkRender });
  await page.goto(url, { waitUntil: "load" });
  await page.waitForTimeout(700);
  return { context, page };
}

async function openListMenu(page) {
  if ((await page.locator(".context-menu").count()) > 0) return;
  const direct = page.locator("button[title='列表菜单']");
  if (await direct.count()) {
    await direct.click({ force: true });
  } else {
    await page.locator(".header-actions button[title='更多操作']").click({ force: true });
    await page.waitForTimeout(300);
    await page.locator(".header-menu-panel .menu-item-button", { hasText: "列表菜单" }).click({ force: true });
  }
  await page.waitForTimeout(350);
}

// ---------------------------------------------------------------------------
// 1. 应用内打开：同窗口 iframe，桌面与移动端都不新开窗口
// ---------------------------------------------------------------------------
for (const mobile of [false, true]) {
  const { context, page } = await freshPage(mobile);
  const popups = [];
  page.on("popup", (popup) => popups.push(popup));
  if (mobile) await page.locator('.tree-row', { hasText: "条目甲" }).click();
  await page.locator(".task-card .markdown-content a").first().click();
  await page.waitForTimeout(400);
  check(`1（${mobile ? "移动" : "桌面"}）应用内打开走同窗口 iframe`,
    (await page.locator(".link-preview-overlay iframe").count()) === 1 && popups.length === 0,
    J({ frames: await page.locator(".link-preview-overlay iframe").count(), popups: popups.length }));
  check(`1（${mobile ? "移动" : "桌面"}）预览标题可用`, (await page.locator(".link-preview-title").innerText()).trim() === "示例");
  await page.locator(".link-preview-close").last().click();
  await page.waitForTimeout(250);
  check(`1（${mobile ? "移动" : "桌面"}）关闭按钮退出预览`, (await page.locator(".link-preview-overlay").count()) === 0);
  await context.close();
}

// ---------------------------------------------------------------------------
// 2. 移动端编辑器长按选择菜单不被吞；桌面仍由 CodeMirror 自绘选区
// ---------------------------------------------------------------------------
for (const mobile of [false, true]) {
  const { context, page } = await freshPage(mobile);
  if (mobile) await page.locator('.tree-row', { hasText: "条目甲" }).click();
  // 走真实 UI 入口打开编辑器（右键卡片 → 编辑），不依赖 dev server 的模块实例：
  // HMR 之后动态 import("/src/lib/stores.ts") 可能拿到另一份模块，写 store 不生效。
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu");
  await page.locator(".context-menu .menu-item-button", { hasText: "编辑" }).first().click();
  await page.waitForSelector(".editor-cm-host .cm-content", { timeout: 8000 });
  const selection = await page.evaluate(() => {
    const content = document.querySelector(".editor-cm-host .cm-content");
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    content.dispatchEvent(event);
    return {
      layers: document.querySelectorAll(".cm-selectionLayer").length,
      prevented: event.defaultPrevented,
      userSelect: getComputedStyle(content).userSelect
    };
  });
  check(`2（${mobile ? "移动" : "桌面"}）编辑器可选文本`, selection.userSelect === "text", J(selection));
  if (mobile) {
    check("2 移动端不拦原生 contextmenu", selection.prevented === false && selection.layers === 0, J(selection));
  } else {
    check("2 桌面仍用自绘选区并拦原生菜单", selection.prevented === true && selection.layers === 1, J(selection));
  }
  await context.close();
}

// ---------------------------------------------------------------------------
// 3. 定时任务：删除图标居中、步骤可折叠、动作方块、按钮同宽、下次触发口径
// ---------------------------------------------------------------------------
for (const mobile of [false, true]) {
  const { context, page } = await freshPage(mobile);
  await page.locator('.sidebar .nav-row[title="定时任务"]').click();
  await page.waitForTimeout(300);
  await page.locator(".scheduler-floating-add").click();
  await page.waitForSelector(".scheduler-step-content");
  const del = await page.locator(".scheduler-delete").evaluate((el) => {
    const box = el.getBoundingClientRect();
    const icon = el.querySelector("svg").getBoundingClientRect();
    return {
      dx: Math.abs((box.left + box.right) / 2 - (icon.left + icon.right) / 2),
      dy: Math.abs((box.top + box.bottom) / 2 - (icon.top + icon.bottom) / 2)
    };
  });
  check(`3（${mobile ? "移动" : "桌面"}）删除按钮图标居中`, del.dx < 1 && del.dy < 1, J(del));
  const next = await page.locator(".scheduler-next").innerText();
  check(`3（${mobile ? "移动" : "桌面"}）未启用时下次触发不虚构时间`, next.includes("未启用"), next);
  await page.locator(".scheduler-step-heading").first().click();
  await page.waitForTimeout(200);
  check(`3（${mobile ? "移动" : "桌面"}）再次点击当前步骤折叠`, (await page.locator(".scheduler-step-content").count()) === 0);
  await page.locator(".scheduler-step-heading").first().click();
  await page.waitForTimeout(200);
  check(`3（${mobile ? "移动" : "桌面"}）再点一次重新展开`, (await page.locator(".scheduler-step-content").count()) === 1);
  await page.locator(".scheduler-sentence button").nth(1).click();
  await page.waitForSelector(".action-editor");
  const tiles = await page.locator(".scheduler-action-tiles .scheduler-trigger-tile").evaluateAll((list) => list.map((tile) => {
    const box = tile.getBoundingClientRect();
    return { text: tile.textContent.trim(), top: box.top, right: box.right };
  }));
  check(`3（${mobile ? "移动" : "桌面"}）执行动作是带图标的方块`, tiles.length === (mobile ? 1 : 3), J(tiles));
  check(`3（${mobile ? "移动" : "桌面"}）动作方块同一行`, new Set(tiles.map((tile) => Math.round(tile.top))).size === 1, J(tiles));
  const footer = await page.locator(".scheduler-footer-actions button").evaluateAll((list) => list.map((button) => button.getBoundingClientRect().width));
  check(`3（${mobile ? "移动" : "桌面"}）试运行/取消/保存同宽`, Math.max(...footer) - Math.min(...footer) < 1, J(footer));
  await context.close();
}

// ---------------------------------------------------------------------------
// 4. 取色盘在 uiScale=1.5 的窄屏上不溢出
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage(true, { uiScale: 1.5 });
  await page.locator('.tree-row', { hasText: "条目甲" }).click();
  await openListMenu(page);
  await page.locator(".ui-color-picker").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel");
  await page.waitForTimeout(500);
  const geometry = await page.evaluate(() => {
    const panel = document.querySelector(".kx-color-panel");
    const box = panel.getBoundingClientRect();
    const children = [...panel.querySelectorAll(".kx-color-canvas, .kx-color-hex, .kx-color-rgb input, .kx-color-actions button")];
    return {
      viewport: window.innerWidth,
      panel: { left: box.left, right: box.right, top: box.top, bottom: box.bottom },
      scrollWidth: panel.scrollWidth,
      clientWidth: panel.clientWidth,
      children: children.map((child) => {
        const r = child.getBoundingClientRect();
        return { cls: child.className || child.tagName, left: r.left, right: r.right };
      })
    };
  });
  check("4 取色盘面板在视口内", geometry.panel.left >= 0 && geometry.panel.right <= geometry.viewport && geometry.panel.top >= 0 && geometry.panel.bottom <= 850, J(geometry.panel));
  check("4 取色盘子元素不超出面板", geometry.children.every((child) => child.left >= geometry.panel.left - 1 && child.right <= geometry.panel.right + 1), J(geometry.children));
  check("4 取色盘自身无横向溢出", geometry.scrollWidth <= geometry.clientWidth + 1, J({ scrollWidth: geometry.scrollWidth, clientWidth: geometry.clientWidth }));
  await page.keyboard.press("Escape");
  await context.close();
}

// ---------------------------------------------------------------------------
// 5. 侧栏所有区域右键菜单 + 固定区布局/添加固定/取消固定
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage(false);
  const menuTexts = async () => (await page.locator(".context-menu > .menu-item").allTextContents()).map((text) => text.trim());
  await page.locator('.system-nav .nav-row[title="我的一天"]').click({ button: "right" });
  await page.waitForSelector(".context-menu");
  const navMenu = await menuTexts();
  check("5 固定区右键有布局/添加固定/取消固定", ["选择布局", "添加固定", "取消固定"].every((label) => navMenu.includes(label)), J(navMenu));
  check("5 固定区右键有独立展开/收起区", navMenu.includes("展开全部") && navMenu.includes("收起全部"));
  await page.getByRole("button", { name: "选择布局", exact: true }).click();
  await page.waitForTimeout(250);
  check("5 布局二级菜单三项齐全", (await page.locator(".submenu-panel .menu-item").allTextContents()).map((x) => x.trim()).join("|").includes("只图标"));
  await page.locator(".submenu-panel .menu-item").filter({ hasText: "双列" }).click();
  await page.waitForTimeout(500);
  check("5 布局选择落盘", (await page.locator(".system-nav").getAttribute("class")).includes("nav-grid"));

  // 添加固定：勾掉日记 → 确认；再打开 → 取消 → 不变
  await page.locator('.system-nav .nav-row[title="我的一天"]').click({ button: "right" });
  await page.getByRole("button", { name: "添加固定", exact: true }).click();
  await page.waitForTimeout(250);
  const diaryItem = page.locator(".submenu-panel .menu-item").filter({ hasText: "日记" }).first();
  await diaryItem.click();
  await page.waitForTimeout(150);
  await page.getByRole("button", { name: "确认", exact: true }).click();
  await page.waitForTimeout(500);
  check("5 添加固定取消勾选后生效", (await page.locator('.system-nav .nav-row[title="日记"]').count()) === 0);
  await page.locator('.system-nav .nav-row[title="我的一天"]').click({ button: "right" });
  await page.getByRole("button", { name: "添加固定", exact: true }).click();
  await page.waitForTimeout(250);
  await page.locator(".submenu-panel .menu-item").filter({ hasText: "日记" }).first().click();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.waitForTimeout(400);
  check("5 添加固定的取消不落盘", (await page.locator('.system-nav .nav-row[title="日记"]').count()) === 0);

  // 工具箱二级菜单添加子工具
  await page.locator('.system-nav .nav-row[title="我的一天"]').click({ button: "right" });
  await page.getByRole("button", { name: "添加固定", exact: true }).click();
  await page.waitForTimeout(250);
  await page.getByRole("button", { name: "工具箱内的工具", exact: true }).click();
  await page.waitForTimeout(250);
  const toolItem = page.locator(".submenu-panel .submenu-panel .menu-item").filter({ hasText: "剪切板同步" }).first();
  await toolItem.click();
  await page.getByRole("button", { name: "确认", exact: true }).click({ force: true });
  await page.waitForTimeout(600);
  check("5 工具箱子工具可单独固定", (await page.locator('.system-nav .nav-row[title="剪切板同步"]').count()) === 1);
  check("5 工具箱本身仍可见（与子工具区分）", (await page.locator('.system-nav .nav-row[title="工具箱"]').count()) === 1);
  await page.locator('.system-nav .nav-row[title="剪切板同步"]').click({ button: "right" });
  await page.getByRole("button", { name: "取消固定", exact: true }).click();
  await page.waitForTimeout(500);
  check("5 取消固定摘掉工具行", (await page.locator('.system-nav .nav-row[title="剪切板同步"]').count()) === 0);

  // 树行菜单与资料卡/分隔线空白区域也有展开/收起
  await page.locator('.tree-row', { hasText: "分组甲" }).click({ button: "right" });
  await page.waitForTimeout(250);
  const treeMenu = await menuTexts();
  check("5 树行右键有展开/收起", treeMenu.includes("展开全部") && treeMenu.includes("收起全部"), J(treeMenu));
  await page.locator(".context-menu .menu-item-button", { hasText: "收起全部" }).click();
  await page.waitForTimeout(500);
  const collapsed = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-state-v3")).nodes.find((node) => node.id === "cat-a")?.collapsed);
  check("5 收起全部落盘", collapsed === true, String(collapsed));
  await page.locator(".profile-card").click({ button: "right" });
  await page.waitForTimeout(250);
  const profileMenu = await menuTexts();
  check("5 资料卡空白区也有展开/收起", profileMenu.includes("展开全部") && profileMenu.includes("收起全部"), J(profileMenu));
  await page.locator(".context-menu .menu-item-button", { hasText: "展开全部" }).click();
  await page.waitForTimeout(400);
  const expanded = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-state-v3")).nodes.find((node) => node.id === "cat-a")?.collapsed);
  check("5 展开全部落盘", expanded === false, String(expanded));
  await context.close();
}


// ---------------------------------------------------------------------------
// 5b. 移动端固定区菜单：钻入式下仍能看到确认/取消
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage(true);
  await page.locator('.system-nav .nav-row[title="我的一天"]').click({ button: "right" });
  await page.waitForSelector(".context-menu");
  await page.locator(".context-menu .menu-item-button", { hasText: "添加固定" }).click();
  await page.waitForTimeout(350);
  const confirmVisible = await page.locator(".nav-fixed-actions button", { hasText: "确认" }).isVisible();
  check("5b 移动端添加固定菜单可见", confirmVisible);
  await page.locator(".submenu-panel .menu-item-button", { hasText: "工具箱内的工具" }).click();
  await page.waitForTimeout(350);
  const nested = await page.locator(".submenu-panel .submenu-panel .menu-item").count();
  check("5b 移动端工具箱子菜单可展开", nested >= 5, String(nested));
  const box = await page.locator(".context-menu").boundingBox();
  check("5b 移动端菜单不越界", Boolean(box) && box.x >= 0 && box.y >= 0 && box.x + box.width <= 392 && box.y + box.height <= 850, J(box));
  await context.close();
}

// ---------------------------------------------------------------------------
// 6. 记账报表统计
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage(false);
  await page.evaluate(() => {
    localStorage.setItem("todo-note-ledger-v1", JSON.stringify({
      accounts: [{ id: "acc", name: "现金", kind: "cash", initialCents: 0 }],
      categories: [
        { id: "cat-e", name: "餐饮", side: "expense", parentId: null, order: 0 },
        { id: "cat-i", name: "工资", side: "income", parentId: null, order: 1 }
      ],
      entries: [
        { id: "l1", date: "2026-03-01", time: "12:00:00", kind: "expense", amountCents: 3000, accountId: "acc", categoryId: "cat-e", note: "", createdAt: "2026-03-01T12:00:00" },
        { id: "l2", date: "2026-03-02", time: "09:00:00", kind: "income", amountCents: 10000, accountId: "acc", categoryId: "cat-i", note: "", createdAt: "2026-03-02T09:00:00" },
        { id: "l3", date: "2026-04-02", time: "09:00:00", kind: "expense", amountCents: 2500, accountId: "acc", categoryId: "cat-e", note: "", createdAt: "2026-04-02T09:00:00" }
      ]
    }));
  });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  await page.locator('.nav-row[title="记账"]').click();
  await page.waitForTimeout(700);
  await page.locator(".ledger-view button[title='统计视图']").click();
  await page.waitForTimeout(500);
  check("6 报表统计区块存在", (await page.locator(".ledger-report-panel").count()) === 1);
  check("6 分类占比下是报表统计", (await page.locator(".ledger-report-panel h2").innerText()).trim() === "报表统计");
  await page.getByRole("tablist", { name: "统计范围" }).getByRole("tab", { name: "年", exact: true }).click();
  await page.waitForTimeout(500);
  check("6 年周期默认月粒度", (await page.locator(".ledger-report-grains button.active").innerText()).trim() === "月");
  const headers = (await page.locator(".ledger-report th").allTextContents()).map((x) => x.trim());
  check("6 表头为时间/收入/支出/结余", headers.join("|") === "时间|收入|支出|结余", J(headers));
  const firstRow = (await page.locator(".ledger-report tbody tr").first().innerText()).split(/\s+/);
  check("6 报表行显示收入支出结余", firstRow.includes("100.00") && firstRow.includes("30.00") && firstRow.includes("70.00"), J(firstRow));
  await page.getByRole("tablist", { name: "统计范围" }).getByRole("tab", { name: "总", exact: true }).click();
  await page.waitForTimeout(400);
  check("6 总周期默认年粒度", (await page.locator(".ledger-report-grains button.active").innerText()).trim() === "年");
  await page.getByRole("tablist", { name: "统计范围" }).getByRole("tab", { name: "周", exact: true }).click();
  await page.waitForTimeout(400);
  check("6 周周期默认日粒度", (await page.locator(".ledger-report-grains button.active").innerText()).trim() === "日");
  await context.close();
}

// ---------------------------------------------------------------------------
// 7. 剪切板同步工具页
// ---------------------------------------------------------------------------
for (const mobile of [false, true]) {
  const { context, page } = await freshPage(mobile);
  if (mobile) await page.locator('.sidebar .nav-row[title="工具箱"]').click();
  else await page.locator('.nav-row[title="工具箱"]').click();
  await page.waitForTimeout(300);
  await page.locator(".toolbox-card", { hasText: "剪切板同步" }).click();
  await page.waitForTimeout(400);
  check(`7（${mobile ? "移动" : "桌面"}）剪切板工具页可打开`, (await page.locator(".clipboard-page").count()) === 1);
  await page.locator(".clipboard-code-row input").first().fill("12345678");
  await page.locator(".clipboard-master input").check();
  await page.waitForTimeout(500);
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).clipboard);
  check(`7（${mobile ? "移动" : "桌面"}）总开关一键落盘`, stored.enabled === true && stored.code === "12345678", J(stored));
  check(`7（${mobile ? "移动" : "桌面"}）两类来源默认值正确`, stored.lanEnabled === true && stored.p2pEnabled === false && stored.directEnabled === false, J(stored));
  await page.locator(".clipboard-code-row button", { hasText: "生成" }).click();
  const generated = await page.locator(".clipboard-code-row input").first().inputValue();
  check(`7（${mobile ? "移动" : "桌面"}）生成随机口令`, generated.length === 12 && generated !== "12345678", generated);
  await page.locator(".clipboard-text-input").nth(1).fill("测试设备");
  await page.locator(".clipboard-limit").first().fill("3");
  await page.locator(".clipboard-card").filter({ hasText: "接收类型与上限" }).locator('input[type="checkbox"]').nth(2).uncheck();
  await page.locator(".clipboard-actions button").click();
  await page.waitForTimeout(500);
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).clipboard);
  check(`7（${mobile ? "移动" : "桌面"}）设备名/上限/类型保存`, saved.deviceName === "测试设备" && saved.limits.textMb === 3 && saved.types.image === false, J(saved));
  const noOverflow = await page.locator(".clipboard-page").evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
  check(`7（${mobile ? "移动" : "桌面"}）工具页无横向溢出`, noOverflow);
  await context.close();
}

console.log(`\n合计：${passes} 通过 / ${failures} 失败`);
if (errors.length) {
  console.log("脚本错误：");
  for (const error of errors) console.log(" -", error);
}
await browser.close();
process.exit(failures === 0 && errors.length === 0 ? 0 : 1);

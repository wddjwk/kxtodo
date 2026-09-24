import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
const now = new Date().toISOString();
const entry = { id: "entry-a", kind: "entry", name: "验收列表", parentId: null, icon: "list", createdAt: now };
const tasks = ["C", "A", "B"].map((name, i) => ({
  id: name, nodeId: entry.id, markdown: `${name} 任务${name === "A" ? "\n第二行正文" : ""}`, completed: false, important: false,
  pinned: name !== "B", myDay: true, tags: [], emojis: [], expanded: false,
  createdAt: new Date(Date.now() + i * 1000).toISOString()
}));
const pages = [];
async function fresh(mobile = false, plain = false) {
  const context = await browser.newContext(mobile ? {
    viewport: { width: 360, height: 800 }, hasTouch: true, isMobile: true,
    userAgent: "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
  } : { viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  page.setDefaultNavigationTimeout(120000);
  pages.push(page);
  page.on("pageerror", error => errors.push(String(error)));
  await page.addInitScript(({ entry, tasks, plain }) => {
    if (sessionStorage.getItem("v088-seeded")) return;
    sessionStorage.setItem("v088-seeded", "1");
    localStorage.setItem("todo-note-state-v3", JSON.stringify({
      nodes: [{ ...entry, cardStyle: plain ? "card" : "todo" }], tasks, backgrounds: {}, selectedNodeId: entry.id
    }));
    const date = new Date();
    const month = date.getMonth() + 1;
    const day = m => `${date.getFullYear()}-${String(m).padStart(2, "0")}-05`;
    localStorage.setItem("todo-note-ledger-v1", JSON.stringify({
      accounts: [{ id: "acc", name: "现金", kind: "cash", initialCents: 0 }],
      categories: [{ id: "cat", name: "餐饮", side: "expense", parentId: null, order: 0 }],
      entries: [month, month === 1 ? 2 : 1].map((m, i) => ({
        id: `ledger-${i}`, date: day(m), kind: "expense", amountCents: (i + 1) * 100,
        accountId: "acc", categoryId: "cat", note: "验收", createdAt: date.toISOString()
      }))
    }));
    localStorage.setItem("todo-note-settings-v3", JSON.stringify({
      appearance: { uiScale: 0.75, listSortMode: "alpha-asc" },
      features: { sync: false, pinnedIcon: true, pinnedSection: false, linkRender: "off" },
      updates: { autoCheck: false }
    }));
  }, { entry, tasks, plain });
  await page.goto(process.env.KXTODO_TEST_URL || "http://127.0.0.1:1420/", { waitUntil: "load" });
  if (mobile) await page.locator(".tree-row", { hasText: entry.name }).click();
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  return page;
}
const titles = page => page.locator(".workspace .task-card .markdown-title-row").allTextContents();
async function listMenu(page) {
  const direct = page.locator("button[title='列表菜单']");
  if (await direct.isVisible()) await direct.click();
  else {
    await page.locator(".workspace .header-actions button[title='更多操作']").click();
    await page.locator(".header-menu-panel .menu-item-button", { hasText: "列表菜单" }).click();
  }
  await page.locator(".context-menu").waitFor({ state: "visible" });
}
async function pinMenu(page, title, label) {
  await page.locator(".task-card", { hasText: title }).click({ button: "right" });
  await page.getByRole("button", { name: label, exact: true }).click();
}
async function pinFeatures(page, icon, section) {
  await page.locator(".profile-card").click();
  const row = page.locator(".settings-drawer .toggle-row", { hasText: "置顶条目样式" });
  await row.waitFor({ state: "visible" });
  await row.getByRole("checkbox").nth(0).setChecked(icon);
  await row.getByRole("checkbox").nth(1).setChecked(section);
  await page.getByRole("button", { name: "关闭设置", exact: true }).click({ position: { x: 10, y: 10 } });
}
try {
  const page = await fresh();
  assert.deepEqual((await titles(page)).map(s => s.trim()), ["A 任务", "C 任务", "B 任务"]);
  await pinMenu(page, "B 任务", "置顶");
  assert.deepEqual((await titles(page)).map(s => s.trim()), ["A 任务", "B 任务", "C 任务"]);
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("todo-note-state-v3")).tasks.find(t => t.id === "B").pinned);
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await page.locator(".task-pin").count(), 3);
  await pinMenu(page, "B 任务", "取消置顶");
  await pinFeatures(page, true, true);
  const pinnedLabel = page.getByRole("button", { name: /^置顶 2$/ });
  await pinnedLabel.click();
  assert.deepEqual((await titles(page)).map(s => s.trim()), ["B 任务"]);
  await pinnedLabel.click();
  assert.equal(await page.locator(".task-pin").count(), 2);
  await pinFeatures(page, false, true);
  assert.equal(await page.locator(".task-pin").count(), 0);
  await pinFeatures(page, true, false);
  await listMenu(page);
  await page.getByRole("button", { name: "排序方式" }).click();
  await page.getByRole("button", { name: "字母顺序 Z → A" }).click();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).appearance.listSortMode === "alpha-desc");
  await page.reload({ waitUntil: "networkidle" });
  assert.deepEqual((await titles(page)).map(s => s.trim()), ["C 任务", "A 任务", "B 任务"]);
  console.log("PASS pinned menu, section/icon combinations, sorting and reload persistence");

  await page.locator(".nav-row[title='工具箱']").click();
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  const pad = page.locator(".scratchpad-area");
  await pad.fill("ab");
  await pad.press("ArrowLeft");
  await pad.press("Tab");
  assert.equal(await pad.inputValue(), "a\tb");
  await pad.press("Control+z");
  assert.equal(await pad.inputValue(), "ab");
  await pad.press("Control+a");
  await pad.press("Tab");
  assert.equal(await pad.inputValue(), "\t");
  await pad.press("Shift+Tab");
  assert.equal(await pad.inputValue(), "\t");
  console.log("PASS scratchpad Tab selection, undo and Shift+Tab navigation");

  await page.locator(".nav-row[title='记账']").click();
  await page.locator(".ledger-view button[title='统计视图']").click();
  await page.locator(".ledger-donut-ring").waitFor();
  await page.locator(".ledger-donut-ring").evaluate(el => { el.dataset.original = "1"; });
  await page.getByRole("tablist", { name: "统计范围" }).getByRole("tab", { name: "年", exact: true }).click();
  assert.equal(await page.locator(".ledger-donut-ring[data-original]").count(), 0);
  await page.locator(".ledger-donut-ring").evaluate(el => { el.dataset.original = "2"; });
  await page.getByRole("tablist", { name: "统计范围" }).getByRole("tab", { name: "年", exact: true }).click();
  assert.equal(await page.locator(".ledger-donut-ring[data-original='2']").count(), 1);
  await page.waitForTimeout(1000);
  const slice = await page.locator(".ledger-donut-slice circle").first().boundingBox();
  await page.mouse.click(slice.x + slice.width / 2, slice.y + 2);
  assert.equal(await page.locator(".ledger-donut-slice.focus").count(), 1);
  assert.equal(await page.locator(".ledger-donut-ring[data-original='2']").count(), 1);
  console.log("PASS donut remounts for changed monthly/yearly data but not focus or same period");

  for (const [nav, prefix] of [["记账", "ledger"], ["日记", "diary"]]) {
    await page.locator(`.nav-row[title='${nav}']`).click();
    await page.locator(`.${prefix}-view button[title='日历视图']`).click();
    await page.locator(`.${prefix}-view button[aria-label='上个月']`).click();
    const delta = await page.locator(`.${prefix}-fab-row`).evaluate(el => {
      const buttons = [...el.querySelectorAll("button")].map(b => b.getBoundingClientRect());
      return Math.abs(buttons[0].left + buttons[0].width / 2 - buttons[1].left - buttons[1].width / 2);
    });
    assert.ok(delta < 1);
  }
  console.log("PASS diary and ledger today/add button centers align");

  for (const plain of [false, true]) {
    const mobile = await fresh(true, plain);
    const geometry = await mobile.locator(".task-card").first().evaluate(card => {
      const pin = card.querySelector(".task-pin").getBoundingClientRect();
      const body = card.querySelector(".task-body").getBoundingClientRect();
      const title = card.querySelector(".markdown-title-row").getBoundingClientRect();
      const box = card.getBoundingClientRect();
      return { gap: body.left - pin.right, width: title.width, right: title.right, cardRight: box.right };
    });
    assert.ok(geometry.gap >= 0 && geometry.width > 20 && geometry.right <= geometry.cardRight);
    await mobile.locator(".task-card").first().click();
    await mobile.locator(".task-card.expanded").waitFor({ state: "visible" });
    assert.equal(await mobile.locator(".task-card.expanded .task-pin").count(), 0);
    await mobile.waitForTimeout(250);
    await mobile.locator(".task-card.expanded").click();
    await mobile.locator(".task-card.expanded").waitFor({ state: "detached" });
    console.log(`PASS mobile ${plain ? "plain" : "todo"} pin/title geometry and expanded hiding`);
    await listMenu(mobile);
    await mobile.locator(".ui-color-picker").first().click();
    const panel = mobile.locator(".kx-color-panel.placed");
    await panel.waitFor({ state: "visible" });
    await mobile.evaluate(() => {
      const shell = document.querySelector(".app-shell");
      shell.style.setProperty("--app-height", `${420 / 0.75}px`);
      shell.style.transform = "translateY(60px) scale(0.75)";
      window.visualViewport.dispatchEvent(new Event("resize"));
    });
    await mobile.waitForTimeout(150);
    const bounds = await panel.evaluate(el => {
      const panel = el.getBoundingClientRect(), shell = el.closest(".app-shell").getBoundingClientRect();
      return { left: panel.left >= shell.left, right: panel.right <= shell.right + 1,
        top: panel.top >= shell.top, bottom: panel.bottom <= shell.bottom + 1,
        overflow: el.scrollWidth - el.clientWidth };
    });
    assert.deepEqual(bounds, { left: true, right: true, top: true, bottom: true, overflow: 0 });
    await panel.getByRole("button", { name: "取消", exact: true }).click();
    console.log("PASS mobile color panel remains in translated, shortened IME shell");
  }
  assert.deepEqual(errors, []);
} finally {
  await browser.close();
}

// 日记功能冒烟（v0.6.5）：桌面三视图 + 编辑器 + 菜单 + 视图持久化，移动端整页层与手势差异。
// 浏览器 dev 模式走 localStorage legacy 路径，足够验证纯前端逻辑（core 命令层由 cargo test 覆盖）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

function todayIso() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------- desktop ----------
const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
const page = await ctx.newPage();
page.on("pageerror", (e) => check("no pageerror", false, String(e)));
await page.goto(URL, { waitUntil: "networkidle" });
await page.waitForTimeout(700);
// 干净起点：清掉上一次跑留下的 localStorage
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(700);

const navRows = await page.locator(".system-nav .nav-row").allInnerTexts();
const diaryIndex = navRows.findIndex((t) => t.includes("日记"));
const importantIndex = navRows.findIndex((t) => t.includes("收藏"));
check("sidebar has a 日记 row", diaryIndex >= 0, JSON.stringify(navRows));
check("日记 sits right below 收藏", diaryIndex === importantIndex + 1, `收藏=${importantIndex} 日记=${diaryIndex}`);

await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
check("diary pane opens", (await page.locator(".app-shell.diary-open").count()) === 1);
check("diary view rendered", await page.locator(".diary-view").isVisible());
check("tasks workspace hidden while diary is open", !(await page.locator(".workspace").isVisible()));
check("view switcher has three modes", (await page.locator(".diary-view-switch button").count()) === 3);
check("empty state shown", (await page.locator(".diary-scroll .empty-state").count()) === 1);
check("no entries yet", (await page.locator(".diary-card").count()) === 0);

// --- write the first entry through the editor ---
await page.locator(".diary-fab").click();
await page.waitForTimeout(700);
check("editor opened", await page.locator(".editor-dialog.diary-editor").isVisible());
check("editor meta bar present", (await page.locator(".diary-meta-trigger").count()) >= 3);
await page.locator(".diary-editor-title").fill("第一篇日记");
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.type("今天把日记功能跑通了。\n第二行内容，用来验证展开。");
await page.waitForTimeout(200);
// 选个心情
await page.locator(".diary-meta-trigger", { hasText: "心情" }).click();
await page.waitForTimeout(200);
await page.locator(".diary-emoji-grid-pop .diary-emoji-cell").first().click();
await page.waitForTimeout(200);
check("mood filled into the trigger", (await page.locator(".diary-meta-trigger.filled").count()) >= 1);
await page.keyboard.press("Escape");
await page.waitForTimeout(600);
check("editor closed after save", (await page.locator(".editor-dialog.diary-editor").count()) === 0);
check("one diary card rendered", (await page.locator(".diary-card").count()) === 1);
const cardText = await page.locator(".diary-card").first().innerText();
check("card shows the title", cardText.includes("第一篇日记"), cardText.replace(/\n/g, " | "));
check("card shows an excerpt", cardText.includes("今天把日记功能跑通了"));
check("card shows the day number", cardText.trim().startsWith(String(new Date().getDate()).padStart(2, "0")) || cardText.includes(String(new Date().getDate())));
check("year divider rendered", (await page.locator(".diary-year-divider").count()) === 1);
check("stats line shows 1 篇", (await page.locator(".diary-subtitle").innerText()).includes("共 1 篇"));

// --- cancel-on-empty must not create a stray entry ---
await page.locator(".diary-fab").click();
await page.waitForTimeout(600);
await page.keyboard.press("Escape");
await page.waitForTimeout(400);
check("empty draft leaves nothing behind", (await page.locator(".diary-card").count()) === 1);

// --- expand / collapse ---
await page.locator(".diary-card").first().dblclick();
await page.waitForTimeout(350);
check("card expands on double click", (await page.locator(".diary-card.expanded").count()) === 1);
check("expanded card renders markdown", (await page.locator(".diary-card .diary-card-content").count()) === 1);
await page.locator(".diary-card").first().dblclick();
await page.waitForTimeout(350);
check("card collapses again", (await page.locator(".diary-card.expanded").count()) === 0);

// --- context menu: weather ---
await page.locator(".diary-card").first().click({ button: "right" });
await page.waitForTimeout(350);
const menuText = await page.locator(".context-menu").first().innerText();
check("card menu has edit/date/mood/weather/delete", ["编辑", "修改日期", "心情", "天气", "删除"].every((t) => menuText.includes(t)), menuText.replace(/\n/g, " | "));
await page.locator(".context-menu .menu-item-button", { hasText: "天气" }).click();
await page.waitForTimeout(350);
await page.locator(".diary-emoji-menu .diary-emoji-cell").first().click();
await page.waitForTimeout(450);
check("weather set from the menu", (await page.locator(".diary-card .diary-meta-chip").count()) >= 2);

// --- search ---
await page.locator(".header-actions > button").last().click();
await page.waitForTimeout(250);
await page.locator(".diary-search input").fill("跑通");
await page.waitForTimeout(300);
check("search keeps the match", (await page.locator(".diary-card").count()) === 1);
await page.locator(".diary-search input").fill("查不到的词");
await page.waitForTimeout(300);
check("search filters everything out", (await page.locator(".diary-card").count()) === 0);
await page.locator(".header-actions > button").last().click();
await page.waitForTimeout(300);
check("closing search restores the list", (await page.locator(".diary-card").count()) === 1);

// --- calendar view + persistence ---
await page.locator(".diary-view-switch button").nth(1).click();
await page.waitForTimeout(350);
check("calendar grid rendered", (await page.locator(".diary-calendar-cell").count()) >= 28);
check("calendar marks the day with an entry", (await page.locator(".diary-calendar-cell.has-entry").count()) === 1);
check("selected day shows its cards", (await page.locator(".diary-day-head").count()) === 1);
check("no 今 button while today is selected", (await page.locator(".diary-fab-today").count()) === 0);

// pick another day -> the add button files under it and 今 shows up
await page.locator(".diary-calendar-cell.today").click();
await page.waitForTimeout(200);
const prevMonth = page.locator(".diary-calendar-bar > button").first();
await prevMonth.click();
await page.waitForTimeout(250);
await page.locator(".diary-calendar-cell:not(.other-month)").nth(4).click();
await page.waitForTimeout(300);
check("今 button appears for a non-today selection", (await page.locator(".diary-fab-today").count()) === 1);
await page.locator(".diary-fab").click();
await page.waitForTimeout(650);
await page.locator(".diary-editor-title").fill("上个月的一篇");
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.type("补写的日记。");
await page.keyboard.press("Escape");
await page.waitForTimeout(500);
check("back-filled entry shows under its own day", (await page.locator(".diary-card").count()) >= 1);
await page.locator(".diary-fab-today").click();
await page.waitForTimeout(300);
check("今 jumps back to today", (await page.locator(".diary-calendar-cell.today.selected").count()) === 1);
check("backfilled entry is not on today", (await page.locator(".diary-day-head + .diary-card").count()) >= 1);

// view choice must survive a reload
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(700);
check("diary pane state is not persisted across reload (opens on tasks)", (await page.locator(".diary-view").count()) === 0);
await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
check("view choice persisted as calendar", (await page.locator(".diary-calendar").count()) === 1);
check("calendar is the active switch button", await page.locator(".diary-view-switch button").nth(1).evaluate((el) => el.classList.contains("active")));

// --- group view ---
await page.locator(".diary-view-switch button").nth(2).click();
await page.waitForTimeout(350);
check("group view renders a year header", (await page.locator(".diary-group-head").count()) >= 1);
check("group view renders a month header", (await page.locator(".diary-group-subhead").count()) >= 1);
const beforeCollapse = await page.locator(".diary-card").count();
await page.locator(".diary-group-head").first().click();
await page.waitForTimeout(300);
check("collapsing a year hides its cards", (await page.locator(".diary-card").count()) < beforeCollapse);
await page.locator(".diary-group-head").first().click();
await page.waitForTimeout(300);
check("expanding restores them", (await page.locator(".diary-card").count()) === beforeCollapse);

// --- back to the list view, then leave the diary ---
await page.locator(".diary-view-switch button").nth(0).click();
await page.waitForTimeout(300);
await page.locator(".tree-row").first().click();
await page.waitForTimeout(400);
check("selecting a node leaves the diary", (await page.locator(".app-shell.diary-open").count()) === 0);
check("tasks workspace is back", await page.locator(".workspace").isVisible());

// --- delete ---
await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
const countBefore = await page.locator(".diary-card").count();
await page.locator(".diary-card").first().click({ button: "right" });
await page.waitForTimeout(300);
await page.locator(".context-menu .menu-item-button", { hasText: "删除" }).click();
await page.waitForTimeout(450);
check("delete removes the card", (await page.locator(".diary-card").count()) === countBefore - 1);
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(600);
await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
check("delete persisted", (await page.locator(".diary-card").count()) === countBefore - 1);

await ctx.close();

// ---------- mobile ----------
const mctx = await browser.newContext({
  viewport: { width: 390, height: 844 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true
});
const mpage = await mctx.newPage();
mpage.on("pageerror", (e) => check("mobile: no pageerror", false, String(e)));
await mpage.goto(URL, { waitUntil: "networkidle" });
await mpage.waitForTimeout(800);

const mNav = await mpage.locator(".system-nav .nav-row").allInnerTexts();
const mDiary = mNav.findIndex((t) => t.includes("日记"));
const mToolbox = mNav.findIndex((t) => t.includes("工具箱"));
check("mobile: 日记 row present", mDiary >= 0, JSON.stringify(mNav));
check("mobile: 日记 sits right above 工具箱", mDiary >= 0 && mToolbox === mDiary + 1, `日记=${mDiary} 工具箱=${mToolbox}`);
check("mobile: 定时任务 hidden", !mNav.some((t) => t.includes("定时任务")));

await mpage.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await mpage.waitForTimeout(400);
check("mobile: diary layer active", (await mpage.locator(".app-shell.mobile.view-diary").count()) === 1);
check("mobile: sidebar hidden", !(await mpage.locator(".sidebar").isVisible()));
check("mobile: tasks workspace hidden", !(await mpage.locator(".workspace").isVisible()));
check("mobile: diary view visible", await mpage.locator(".diary-view").isVisible());
check("mobile: back button visible", await mpage.locator(".diary-view .mobile-back").isVisible());

await mpage.locator(".diary-fab").click();
await mpage.waitForTimeout(800);
check("mobile: editor opens full screen", await mpage.locator(".editor-dialog.diary-editor").isVisible());
await mpage.locator(".diary-editor-title").fill("移动端日记");
await mpage.locator(".editor-cm-host .cm-content").click();
await mpage.keyboard.type("在手机上写的一篇。");
await mpage.keyboard.press("Escape");
await mpage.waitForTimeout(700);
check("mobile: card created", (await mpage.locator(".diary-card").count()) >= 1);
check("mobile: pen button hidden on cards", !(await mpage.locator(".diary-edit-button").first().isVisible().catch(() => false)));

// single tap expands, does not open the editor
await mpage.locator(".diary-card").first().click();
await mpage.waitForTimeout(500);
check("mobile: single tap does not open the editor", (await mpage.locator(".editor-dialog.diary-editor").count()) === 0);

// hardware back returns to the category list
await mpage.goBack();
await mpage.waitForTimeout(500);
check("mobile: history back returns to the list", (await mpage.locator(".app-shell.mobile.view-list").count()) === 1);

await mctx.close();
await browser.close();

console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);

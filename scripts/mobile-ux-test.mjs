// 移动端 UX 冒烟：Android UA + 触摸模拟，验证 v0.2.0 导航栈/设置页/长按/定时任务隐藏。
// 浏览器 dev 模式走 localStorage legacy 路径，足够验证纯前端 UX 逻辑。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------- mobile context ----------
const mobileCtx = await browser.newContext({
  viewport: { width: 390, height: 844 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true
});
const page = await mobileCtx.newPage();
page.on("pageerror", (e) => check("no pageerror", false, String(e)));
await page.goto(URL, { waitUntil: "networkidle" });
await page.waitForTimeout(800);

check("app shell mounted (no TDZ white screen)", (await page.locator(".app-shell").count()) === 1);
check("mobile class applied", (await page.locator(".app-shell.mobile").count()) === 1);
check("starts on list view", (await page.locator(".app-shell.mobile.view-list").count()) === 1);
check("sidebar visible on list", await page.locator(".sidebar").isVisible());
check("workspace hidden on list", !(await page.locator(".workspace").isVisible()));

const navText = await page.locator(".system-nav").innerText();
check("system nav has my-day/planned/important", navText.includes("我的一天") && navText.includes("计划内") && navText.includes("收藏"));
check("system nav hides scheduled on mobile", !navText.includes("定时任务"));

// list -> content
await page.locator(".tree-row").first().click();
await page.waitForTimeout(400);
check("entry tap enters content view", (await page.locator(".app-shell.mobile.view-content").count()) === 1);
check("workspace visible on content", await page.locator(".workspace").isVisible());
check("back button visible", await page.locator(".mobile-back").first().isVisible());

// add a task through the composer
await page.locator(".add-task-bar textarea").fill("移动端冒烟任务");
await page.locator(".add-task-bar textarea").press("Enter");
await page.waitForTimeout(500);
check("task card rendered", (await page.locator(".task-card").count()) >= 1);

// hardware/history back -> list
await page.goBack();
await page.waitForTimeout(400);
check("history back returns to list", (await page.locator(".app-shell.mobile.view-list").count()) === 1);

// forward again then settings page
await page.locator(".tree-row").first().click();
await page.waitForTimeout(300);
await page.goBack();
await page.waitForTimeout(300);
await page.locator(".profile-card").click();
await page.waitForTimeout(400);
check("settings opens as page (view-settings)", (await page.locator(".app-shell.mobile.view-settings").count()) === 1);
check("settings drawer visible", await page.locator(".settings-drawer").isVisible());
const drawerText = await page.locator(".settings-drawer").innerText();
check("settings shows update section on mobile", drawerText.includes("关于与更新"));
check("settings hides desktop-only sections", !drawerText.includes("窗口与系统") && !drawerText.includes("快捷键") && !drawerText.includes("云同步预留"));
check("settings hides popup geometry on mobile", !drawerText.includes("弹窗位置"));

// history back closes settings -> list
await page.goBack();
await page.waitForTimeout(400);
check("back closes settings to list", (await page.locator(".app-shell.mobile.view-list").count()) === 1 && (await page.locator(".settings-drawer").count()) === 0);

// long-press on task card opens context menu
await page.locator(".tree-row").first().click();
await page.waitForTimeout(300);
await page.evaluate(() => {
  const card = document.querySelector(".task-card");
  const r = card.getBoundingClientRect();
  card.dispatchEvent(new PointerEvent("pointerdown", {
    bubbles: true, cancelable: true, composed: true,
    pointerType: "touch", isPrimary: true, pointerId: 7,
    button: 0, buttons: 1, clientX: r.left + r.width / 2, clientY: r.top + 12
  }));
});
await page.waitForTimeout(700);
await page.evaluate(() => {
  window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch", pointerId: 7 }));
});
await page.waitForTimeout(300);
check("long-press opens task context menu", (await page.locator(".context-menu").count()) === 1);

// editor layer: open editor, back saves+closes
await page.locator(".context-menu").locator("text=编辑").first().click({ timeout: 5000 }).catch((e) => {
  console.log("edit click err:", String(e).split("\n")[0]);
});
await page.waitForTimeout(1500);
const editorOpen = (await page.locator(".editor-overlay").count()) === 1;
check("editor opens from menu", editorOpen);
if (editorOpen) {
  await page.goBack();
  await page.waitForTimeout(500);
  check("back closes editor back to content", (await page.locator(".editor-overlay").count()) === 0 && (await page.locator(".app-shell.mobile.view-content").count()) === 1);
}

await mobileCtx.close();

// ---------- desktop context regression ----------
const desktopCtx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const dpage = await desktopCtx.newPage();
dpage.on("pageerror", (e) => check("desktop no pageerror", false, String(e)));
await dpage.goto(URL, { waitUntil: "networkidle" });
await dpage.waitForTimeout(800);
check("desktop shell not mobile", (await dpage.locator(".app-shell.mobile").count()) === 0);
check("desktop shows both panes", (await dpage.locator(".sidebar").isVisible()) && (await dpage.locator(".workspace").isVisible()));
const dnav = await dpage.locator(".system-nav").innerText();
check("desktop nav keeps scheduled", dnav.includes("定时任务"));
// desktop right-click on a task card still opens menu
await dpage.locator(".add-task-bar textarea").fill("桌面回归任务");
await dpage.locator(".add-task-bar textarea").press("Enter");
await dpage.waitForTimeout(400);
if ((await dpage.locator(".task-card").count()) >= 1) {
  await dpage.locator(".task-card").first().click({ button: "right" });
  await dpage.waitForTimeout(300);
  check("desktop right-click opens task menu", (await dpage.locator(".context-menu").count()) === 1);
}
await desktopCtx.close();
await browser.close();

console.log(failures === 0 ? "ALL MOBILE UX CHECKS PASSED" : `${failures} CHECK(S) FAILED`);
process.exit(failures === 0 ? 0 : 1);

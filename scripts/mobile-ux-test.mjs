// 移动端 UX 冒烟：Android UA + 触摸模拟，验证 v0.2.0 导航栈/设置页/长按/定时任务隐藏，
// 以及 v0.2.1 新增：界面缩放移动端生效、齿轮下拉、长按转拖拽、工具箱页、计划内分组、
// 背景菜单项存在性。浏览器 dev 模式走 localStorage legacy 路径，足够验证纯前端 UX 逻辑。
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
check("system nav shows toolbox row on mobile", navText.includes("工具箱"));

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

// item 1: uiScale row now visible on mobile and drives the shell transform
const scaleInput = page.locator('.settings-drawer input[aria-label="界面缩放"]');
check("settings shows uiScale row on mobile", await scaleInput.isVisible());
await scaleInput.fill("80");
await page.waitForTimeout(400);
const shellStyle = (await page.locator(".app-shell").getAttribute("style")) ?? "";
check("mobile shell honors uiScale", shellStyle.includes("--ui-scale: 0.8"), shellStyle.slice(0, 120));
check("mobile shell emits safe-area inverse", /--safe-inv: 1\.2/.test(shellStyle), shellStyle.slice(0, 120));
await scaleInput.fill("100");
await page.waitForTimeout(400);
const shellStyleReset = (await page.locator(".app-shell").getAttribute("style")) ?? "";
check("uiScale resets to 1", shellStyleReset.includes("--ui-scale: 1"), shellStyleReset.slice(0, 120));

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

// item 2.1: menu must stay inside the viewport (clamped / flipped / height-capped)
const menuBox = await page.locator(".context-menu").boundingBox();
check(
  "context menu clamped inside viewport",
  Boolean(menuBox) && menuBox.x >= 0 && menuBox.y >= 0 && menuBox.x + menuBox.width <= 391 && menuBox.y + menuBox.height <= 845,
  JSON.stringify(menuBox)
);

// item 2.1 edge case: long-press anchored at the bottom-right corner → flip/clamp into view
await page.keyboard.press("Escape");
await page.waitForTimeout(250);
check("escape closes task menu", (await page.locator(".context-menu").count()) === 0);
await page.evaluate(() => {
  const card = document.querySelector(".task-card");
  card.dispatchEvent(new PointerEvent("pointerdown", {
    bubbles: true, cancelable: true, composed: true,
    pointerType: "touch", isPrimary: true, pointerId: 8,
    button: 0, buttons: 1, clientX: 385, clientY: 838
  }));
});
await page.waitForTimeout(700);
await page.evaluate(() => {
  window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch", pointerId: 8 }));
});
await page.waitForTimeout(300);
check("corner long-press reopens task menu", (await page.locator(".context-menu").count()) === 1);
const flipBox = await page.locator(".context-menu").boundingBox();
check(
  "bottom-right anchor keeps menu on screen",
  Boolean(flipBox) && flipBox.x >= 0 && flipBox.y >= 0 && flipBox.x + flipBox.width <= 391 && flipBox.y + flipBox.height <= 845,
  JSON.stringify(flipBox)
);
check("menu flips above a bottom-edge anchor", Boolean(flipBox) && flipBox.y + flipBox.height <= 839, JSON.stringify(flipBox));

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

// item 4: header gear dropdown replaces title-tap reveal
const gear = page.locator('.header-actions button[aria-label="更多操作"]');
check("mobile header shows gear button", await gear.isVisible());
check("mobile header renders only the gear", (await page.locator(".header-actions > button").count()) === 1);
check("title-tap mechanism removed", (await page.locator(".mobile-title-tap").count()) === 0);
await gear.click();
await page.waitForTimeout(250);
check("gear opens dropdown panel", await page.locator(".header-menu-panel").isVisible());
const gearPanelText = await page.locator(".header-menu-panel").innerText();
check("gear panel has 展开全部 + 列表菜单", gearPanelText.includes("展开全部") && gearPanelText.includes("列表菜单"));
await page.locator(".header-menu-panel").locator("text=列表菜单").first().click();
await page.waitForTimeout(300);
check("gear 列表菜单 opens ListMenu and closes panel",
  (await page.locator(".context-menu").count()) === 1 && (await page.locator(".header-menu-panel").count()) === 0);
const listMenuText = await page.locator(".context-menu").innerText();
check("ListMenu keeps background actions", listMenuText.includes("上传图片") && listMenuText.includes("清除背景"));
await page.locator(".list-header h1").click({ force: true });
await page.waitForTimeout(250);
check("outside tap closes ListMenu", (await page.locator(".context-menu").count()) === 0);
await gear.click();
await page.waitForTimeout(200);
await gear.click();
await page.waitForTimeout(200);
check("gear re-tap closes panel", (await page.locator(".header-menu-panel").count()) === 0);

// item 7: planned regrouping (mobile pass)
await page.goBack();
await page.waitForTimeout(300);
await page.locator(".system-nav button", { hasText: "计划内" }).click();
await page.waitForTimeout(300);
check("planned chip visible on planned view", await page.locator(".planned-group-chip").isVisible());
await page.locator(".planned-group-chip").click();
await page.waitForTimeout(250);
const groupTexts = await page.locator(".planned-group-panel").innerText();
check("planned panel lists 6 groups",
  ["今天", "明天", "近三天", "本周", "稍后", "全部"].every((t) => groupTexts.includes(t)),
  groupTexts.replace(/\n/g, "|"));
await page.locator(".planned-group-panel").locator("text=今天").first().click();
await page.waitForTimeout(250);
const chipText = await page.locator(".planned-group-chip").innerText();
check("chip label switches to selected group", chipText.includes("今天"));
check("planned panel closes after select", (await page.locator(".planned-group-panel").count()) === 0);
// ListMenu gains the show-completed toggle on the planned node
await gear.click();
await page.waitForTimeout(200);
await page.locator(".header-menu-panel").locator("text=列表菜单").first().click();
await page.waitForTimeout(300);
const plannedMenuText = await page.locator(".context-menu").innerText();
check("planned ListMenu offers 显示已完成", plannedMenuText.includes("显示已完成"));
await page.locator(".context-menu").locator("text=显示已完成").first().click();
await page.waitForTimeout(300);
check("toggle closes ListMenu", (await page.locator(".context-menu").count()) === 0);

// item 3: toolbox page
await page.goBack();
await page.waitForTimeout(300);
check("back returns to list from planned", (await page.locator(".app-shell.mobile.view-list").count()) === 1);
await page.locator(".system-nav button", { hasText: "工具箱" }).click();
await page.waitForTimeout(300);
check("toolbox view entered",
  (await page.locator(".app-shell.mobile.view-toolbox").count()) === 1 && (await page.locator(".toolbox-view").isVisible()) === true);
check("toolbox hides sidebar and workspace",
  !(await page.locator(".sidebar").isVisible()) && !(await page.locator(".workspace").isVisible()));
check("random tool card listed", (await page.locator(".toolbox-card").count()) >= 1);
await page.locator(".toolbox-card").first().click();
await page.waitForTimeout(250);
check("random sub-view opened", (await page.locator(".toolbox-sub").count()) === 1);
await page.locator(".toolbox-sub button.settings-button.primary").click();
await page.waitForTimeout(250);
const chipCount = await page.locator(".toolbox-result-chip").count();
const chipValue = chipCount > 0 ? Number(await page.locator(".toolbox-result-chip").first().innerText()) : NaN;
check("random tool generates an in-range integer", chipCount >= 1 && Number.isInteger(chipValue) && chipValue >= 1 && chipValue <= 100, String(chipValue));
await page.locator(".toolbox-sub-back").click();
await page.waitForTimeout(250);
check("sub-view back returns to tool list",
  (await page.locator(".toolbox-card").count()) >= 1 && (await page.locator(".toolbox-sub").count()) === 0);
await page.locator(".toolbox-header .mobile-back").click();
await page.waitForTimeout(350);
check("toolbox back returns to list view", (await page.locator(".app-shell.mobile.view-list").count()) === 1);

// item 2.2: long-press a tree row, keep holding and move → menu closes, drag starts
{
  const rowBox = await page.locator(".tree-row").first().boundingBox();
  const rx = Math.round(rowBox.x + rowBox.width / 2);
  const ry = Math.round(rowBox.y + rowBox.height / 2);
  await page.evaluate(({ x, y }) => {
    const el = document.elementFromPoint(x, y) ?? document.body;
    el.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true, cancelable: true, composed: true,
      pointerType: "touch", isPrimary: true, pointerId: 11,
      button: 0, buttons: 1, clientX: x, clientY: y
    }));
  }, { x: rx, y: ry });
  await page.waitForTimeout(700);
  check("tree row long-press opens menu", (await page.locator(".context-menu").count()) === 1);
  await page.evaluate(({ x, y }) => {
    window.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true, composed: true,
      pointerType: "touch", isPrimary: true, pointerId: 11,
      button: 0, buttons: 1, clientX: x, clientY: y
    }));
  }, { x: rx, y: ry + 24 });
  await page.waitForTimeout(250);
  check("hold+move closes the menu", (await page.locator(".context-menu").count()) === 0);
  check("hold+move starts row drag", (await page.locator(".tree-row.dragging").count()) >= 1);
  // move back to origin so no actual reorder happens, then release
  await page.evaluate(({ x, y }) => {
    window.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true, composed: true,
      pointerType: "touch", isPrimary: true, pointerId: 11,
      button: 0, buttons: 1, clientX: x, clientY: y
    }));
  }, { x: rx, y: ry });
  await page.waitForTimeout(100);
  await page.evaluate(() => {
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch", pointerId: 11 }));
  });
  await page.waitForTimeout(250);
  check("drag cleans up after release", (await page.locator(".tree-row.dragging").count()) === 0);
  check("release without drop keeps tree menu closed", (await page.locator(".context-menu").count()) === 0);
}

// capped menu: submenus must stay usable (inline accordion, not clipped by scroll)
await page.setViewportSize({ width: 390, height: 420 });
await page.waitForTimeout(300);
if ((await page.locator(".app-shell.mobile.view-content").count()) === 0) {
  await page.locator(".tree-row").first().click();
  await page.waitForTimeout(300);
}
await page.locator(".header-actions button").first().click();
await page.waitForTimeout(300);
await page.locator(".header-menu-panel").locator("text=列表菜单").first().click();
await page.waitForTimeout(400);
const capped = (await page.locator(".context-menu.capped").count()) === 1;
check("short viewport caps ListMenu height", capped);
if (capped) {
  await page.locator(".context-menu.capped").locator("text=排序方式").first().click();
  await page.waitForTimeout(300);
  const sub = page.locator(".context-menu.capped .submenu-panel").first();
  const subVisible = (await sub.count()) === 1 && (await sub.isVisible());
  check("capped menu submenu renders", subVisible);
  if (subVisible) {
    const item = sub.locator(".menu-item-button, .menu-item").first();
    const box = await item.boundingBox();
    const hit = box
      ? await page.evaluate(([cx, cy]) => {
          const el = document.elementFromPoint(cx, cy);
          return Boolean(el?.closest(".submenu-panel"));
        }, [box.x + box.width / 2, box.y + box.height / 2])
      : false;
    check("capped menu submenu item hit-testable", hit);
  }
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
}
await page.setViewportSize({ width: 390, height: 844 });

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
check("desktop nav has no toolbox row", !dnav.includes("工具箱"));
check("desktop header has no gear button", (await dpage.locator('.header-actions button[aria-label="更多操作"]').count()) === 0);
check("desktop header keeps list menu button", (await dpage.locator('.header-actions button[title="列表菜单"]').count()) === 1);
// desktop right-click on a task card still opens menu
await dpage.locator(".add-task-bar textarea").fill("桌面回归任务");
await dpage.locator(".add-task-bar textarea").press("Enter");
await dpage.waitForTimeout(400);
if ((await dpage.locator(".task-card").count()) >= 1) {
  await dpage.locator(".task-card").first().click({ button: "right" });
  await dpage.waitForTimeout(300);
  check("desktop right-click opens task menu", (await dpage.locator(".context-menu").count()) === 1);
  const dMenuBox = await dpage.locator(".context-menu").boundingBox();
  check("desktop menu stays inside viewport",
    Boolean(dMenuBox) && dMenuBox.x >= 0 && dMenuBox.y >= 0 && dMenuBox.x + dMenuBox.width <= 1281 && dMenuBox.y + dMenuBox.height <= 801,
    JSON.stringify(dMenuBox));
  await dpage.keyboard.press("Escape");
  await dpage.waitForTimeout(200);
}
// item 7 on desktop: planned chip works there too
await dpage.locator(".system-nav button", { hasText: "计划内" }).click();
await dpage.waitForTimeout(300);
check("desktop planned chip visible", await dpage.locator(".planned-group-chip").isVisible());
await dpage.locator(".planned-group-chip").click();
await dpage.waitForTimeout(200);
check("desktop planned panel lists 6 groups",
  (await dpage.locator(".planned-group-panel .menu-item-button").count()) === 6);
await dpage.keyboard.press("Escape");
await dpage.locator(".list-header h1").click({ force: true });
await dpage.waitForTimeout(200);
check("desktop planned panel closes on outside click", (await dpage.locator(".planned-group-panel").count()) === 0);
await desktopCtx.close();
await browser.close();

console.log(failures === 0 ? "ALL MOBILE UX CHECKS PASSED" : `${failures} CHECK(S) FAILED`);
process.exit(failures === 0 ? 0 : 1);

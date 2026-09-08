// v0.6.8 修复项回归：移动端标签红叉不再常驻（点按露出）、日记透明度条跟手、
// 完成/删除/新增其它任务不会让别的卡片自己展开。用法：node scripts/v068-fixes-test.mjs（需先 npm run dev）。
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

// ---------- 移动端：标签红叉默认隐藏，点标签露出，点别处收回 ----------
const mob = await browser.newContext({
  viewport: { width: 390, height: 844 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true,
  deviceScaleFactor: 2
});
const mp = await mob.newPage();
await mp.goto(URL, { waitUntil: "load", timeout: 90000 });
await mp.waitForTimeout(700);
await mp.evaluate(() => localStorage.clear());
await mp.reload({ waitUntil: "load", timeout: 90000 });
await mp.waitForTimeout(700);
await mp.locator(".custom-nav .tree-row", { hasText: "收集箱" }).first().click();
await mp.waitForTimeout(500);
// 造一条带标签的任务：编辑器里加标签
await mp.locator(".task-card").first().dblclick().catch(() => {});
await mp.locator(".add-task-bar textarea").fill("带标签的任务");
await mp.locator(".add-task-bar textarea").press("Enter");
await mp.waitForTimeout(400);
await mp.locator(".task-card").first().dblclick();
await mp.waitForSelector(".editor-meta", { timeout: 20000 });
await mp.locator(".editor-meta-trigger", { hasText: "标签" }).first().click();
await mp.waitForTimeout(300);
await mp.locator(".tag-editor-input-row input").fill("工作");
await mp.locator(".tag-add-btn").click();
await mp.waitForTimeout(400);
const hiddenOpacity = await mp.locator(".editor-meta-tags .task-tag .tag-delete").first().evaluate((el) => getComputedStyle(el).opacity);
check("移动端编辑器标签红叉默认隐藏", hiddenOpacity === "0", hiddenOpacity);
await mp.locator(".editor-meta-tags .task-tag").first().click();
await mp.waitForTimeout(250);
const revealedOpacity = await mp.locator(".editor-meta-tags .task-tag .tag-delete").first().evaluate((el) => getComputedStyle(el).opacity);
check("点标签露出红叉", revealedOpacity === "1", revealedOpacity);
await mp.locator(".editor-meta-trigger").first().click();
await mp.waitForTimeout(250);
const hiddenAgain = await mp.locator(".editor-meta-tags .task-tag .tag-delete").first().evaluate((el) => getComputedStyle(el).opacity);
check("点别处收回红叉", hiddenAgain === "0", hiddenAgain);
await mob.close();

// ---------- 桌面：透明度条跟手 + 展开态不被其它任务操作带跑 ----------
const desk = await browser.newContext({ viewport: { width: 1280, height: 860 } });
const dp = await desk.newPage();
const errors = [];
dp.on("pageerror", (e) => errors.push(String(e)));
await dp.goto(URL, { waitUntil: "load", timeout: 90000 });
await dp.waitForTimeout(700);
await dp.evaluate(() => localStorage.clear());
await dp.reload({ waitUntil: "load", timeout: 90000 });
await dp.waitForTimeout(700);

// 日记菜单的透明度条：连续 input 期间值不被滞后回渲拽回
await dp.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await dp.waitForTimeout(500);
await dp.locator(".diary-view .header-actions > button").first().click();
await dp.waitForTimeout(300);
await dp.locator(".diary-gear-panel .menu-item-button", { hasText: "日记菜单" }).click();
await dp.waitForTimeout(400);
const slider = dp.locator(".opacity-row input");
for (const v of ["10", "30", "55", "70"]) {
  await slider.evaluate((el, value) => {
    el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, v);
  await dp.waitForTimeout(120);
  const current = await slider.inputValue();
  check(`透明度条跟手 (${v})`, current === v, `input=${current}`);
}
await dp.keyboard.press("Escape");
await dp.waitForTimeout(300);

// 展开态稳定：展开第一条（多行），再完成第二条、新增第四条，展开集合不变
await dp.locator(".custom-nav .tree-row", { hasText: "收集箱" }).first().click();
await dp.waitForTimeout(400);
for (const t of ["第一行内容\n第二行内容\n第三行内容", "待完成的一条", "再一条"]) {
  await dp.locator(".add-task-bar textarea").fill(t);
  await dp.locator(".add-task-bar textarea").press("Enter");
  await dp.waitForTimeout(300);
}
await dp.locator(".task-card", { hasText: "第一行内容" }).first().dblclick();
await dp.waitForTimeout(400);
const expandedBefore = await dp.locator(".task-card.expanded").count();
await dp.locator(".task-card .task-check").nth(1).click();
await dp.waitForTimeout(500);
await dp.locator(".add-task-bar textarea").fill("新插入的一条任务");
await dp.locator(".add-task-bar textarea").press("Enter");
await dp.waitForTimeout(500);
const expandedAfter = await dp.locator(".task-card.expanded").count();
check("完成/新增其它任务不改变展开集合", expandedBefore === 1 && expandedAfter === 1, `${expandedBefore} -> ${expandedAfter}`);
check("桌面全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await desk.close();

await browser.close();
console.log(failures === 0 ? "V068 FIXES ALL PASS" : `V068 FIXES FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

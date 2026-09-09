// v0.6.10 回归：桌面编辑器工具栏（含特性开关）、标题按钮光标落在「# 」之后、
// 同步总开关关掉后配置隐藏、一般卡片条目菜单出现 Markdown 导出/导入两项、移动端工具栏常显。
// 用法：node scripts/v0610-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];

// ---------------------------------------------------------------- 桌面
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
page.on("pageerror", (e) => errors.push("desktop: " + e));
await page.goto(URL, { waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);

// 桌面编辑器工具栏：默认显示，17 个按钮，在元数据行下方
await page.locator(".composer-plus").click();
await page.waitForSelector(".editor-dialog", { timeout: 30000 });
check("桌面编辑器默认有工具栏", (await page.locator(".editor-dialog .editor-md-toolbar button").count()) === 16);
const orderOk = await page.evaluate(() => {
  const toolbar = document.querySelector(".editor-dialog .editor-md-toolbar");
  const meta = document.querySelector(".editor-dialog .editor-meta");
  const body = document.querySelector(".editor-dialog .editor-body");
  if (!toolbar || !meta || !body) return false;
  return meta.getBoundingClientRect().bottom <= toolbar.getBoundingClientRect().top + 1 &&
    toolbar.getBoundingClientRect().bottom <= body.getBoundingClientRect().top + 1;
});
check("工具栏位于元数据行与正文之间", orderOk);

// 标题按钮：光标落在「# 」之后（接着打字应接在标记后面）
await page.locator(".editor-cm-host .cm-content").click();
await page.locator(".editor-md-toolbar button[title='一级标题']").click();
await page.waitForTimeout(200);
await page.keyboard.insertText("标题");
await page.waitForTimeout(200);
const line1 = await page.locator(".editor-cm-host .cm-content").innerText();
check("h1 后光标在标记之后", line1.startsWith("# 标题"), JSON.stringify(line1.slice(0, 12)));
await page.locator(".editor-md-toolbar button[title='六级标题']").click();
await page.waitForTimeout(200);
check("h6 可换级", (await page.locator(".editor-cm-host .cm-content").innerText()).startsWith("###### 标题"));
await page.keyboard.press("Escape");
await page.waitForTimeout(500);
await page.keyboard.press("Escape");
await page.waitForTimeout(500);

// 特性开关：编辑器工具栏
await page.keyboard.press("Control+,");
await page.waitForSelector(".settings-drawer, aside", { timeout: 30000 });
const toolbarToggle = page.locator("label.toggle-row", { hasText: "编辑器工具栏" }).locator("input");
await toolbarToggle.setChecked(false);
await page.waitForTimeout(500);
await page.locator("button.settings-backdrop").click();
await page.waitForTimeout(400);
await page.locator(".composer-plus").click();
await page.waitForSelector(".editor-dialog", { timeout: 30000 });
check("关掉开关后桌面工具栏消失", (await page.locator(".editor-dialog .editor-md-toolbar button").count()) === 0);
await page.keyboard.press("Escape");
await page.waitForTimeout(400);
await page.keyboard.press("Control+,");
await page.waitForTimeout(500);
await toolbarToggle.setChecked(true);
await page.waitForTimeout(400);

// 特性开关：启动同步功能
const syncToggle = page.locator("label.toggle-row", { hasText: "启动同步功能" }).locator("input");
await syncToggle.setChecked(false);
await page.waitForTimeout(600);
const drawerText = await page.locator("aside.settings-drawer").innerText();
check("关掉同步总开关后同步配置隐藏", drawerText.includes("同步功能已在特性开关里停用") && !drawerText.includes("通信方式"));
await syncToggle.setChecked(true);
await page.waitForTimeout(600);
const drawerText2 = await page.locator("aside.settings-drawer").innerText();
check("勾回后同步配置恢复", drawerText2.includes("通信方式"));
await page.locator("button.settings-backdrop").click();
await page.waitForTimeout(400);

// 一般卡片条目：三点菜单出现 Markdown 导出/导入
await page.locator(".list-header button[title='列表菜单']").click();
await page.waitForTimeout(400);
await page.locator(".context-menu button", { hasText: "卡片类型" }).first().click();
await page.waitForTimeout(300);
await page.locator(".context-menu button", { hasText: "一般卡片" }).first().click();
await page.waitForTimeout(500);
await page.locator(".list-header button[title='列表菜单']").click();
await page.waitForTimeout(400);
check("一般卡片菜单有导出为 Markdown", (await page.locator(".context-menu button", { hasText: "导出为 Markdown" }).count()) === 1);
check("一般卡片菜单有导入 Markdown 压缩包", (await page.locator(".context-menu button", { hasText: "导入 Markdown 压缩包" }).count()) === 1);
await page.keyboard.press("Escape");
await page.waitForTimeout(300);
await page.close();

// ---------------------------------------------------------------- 移动端
const ctx = await browser.newContext({
  viewport: { width: 390, height: 844 },
  hasTouch: true,
  isMobile: true,
  userAgent: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36"
});
const m = await ctx.newPage();
m.on("pageerror", (e) => errors.push("mobile: " + e));
await m.goto(URL, { waitUntil: "load", timeout: 90000 });
await m.waitForTimeout(700);
await m.evaluate(() => localStorage.clear());
await m.reload({ waitUntil: "load", timeout: 90000 });
await m.waitForTimeout(900);
await m.locator(".custom-nav .tree-row").first().click();
await m.waitForTimeout(700);
await m.locator(".composer-plus").click();
await m.waitForSelector(".editor-dialog", { timeout: 30000 });
check("移动端编辑器有工具栏", (await m.locator(".editor-dialog .editor-md-toolbar button").count()) === 16);
await m.close();

check("全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failures === 0 ? "V070 FIXES ALL PASS" : `V070 FIXES FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

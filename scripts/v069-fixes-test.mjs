// v0.6.9 回归：日记长单行可展开（桌面双击/移动单击）+ 展开态稳定、移动端编辑器 markdown
// 工具栏（不抢焦点）、桌面编辑器宽高比例设置、一般卡片移动端正文占满、标签点按两段式
// （第一下露红叉不删除）、任务编辑器标签加号按钮可见。
// 用法：node scripts/v069-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const LONG_LINE = "这是一行特别长的日记正文，它没有任何换行，但是长度远远超过两行，" +
  "于是折叠态的摘要两行根本放不下，按照统一口径这张卡片应当可以被展开看到全文，" +
  "而不是把后面的内容永远藏在省略号后面。";

const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];

/** 编辑器是两段式 Esc（先收浮层再关窗），关干净为止 */
async function closeOverlays(page) {
  for (let i = 0; i < 4; i++) {
    if ((await page.locator(".editor-overlay").count()) === 0) return;
    await page.keyboard.press("Escape");
    await page.waitForTimeout(400);
  }
}

// ---------------------------------------------------------------- 桌面
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
page.on("pageerror", (e) => errors.push("desktop: " + e));
await page.goto(URL, { waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);

// 编辑器尺寸比例：默认 72% / 86%
await page.locator(".composer-plus").click();
await page.waitForSelector(".editor-dialog", { timeout: 30000 });
const sizeRatio = await page.evaluate(() => {
  const dialog = document.querySelector(".editor-dialog");
  const shell = document.querySelector(".app-shell");
  return { w: dialog.getBoundingClientRect().width / shell.getBoundingClientRect().width, h: dialog.getBoundingClientRect().height / shell.getBoundingClientRect().height };
});
check("桌面编辑器宽度≈72% 窗口宽", Math.abs(sizeRatio.w - 0.72) < 0.02, sizeRatio.w.toFixed(3));
check("桌面编辑器高度≈86% 窗口高", Math.abs(sizeRatio.h - 0.86) < 0.02, sizeRatio.h.toFixed(3));

// 标签加号按钮可见（任务编辑器浮层没有 --accent，曾白底白图）
await page.locator(".editor-meta-trigger.editor-tag-add").click();
await page.waitForTimeout(300);
const addBtn = await page.evaluate(() => {
  const btn = document.querySelector(".editor-dialog .tag-add-btn");
  const cs = getComputedStyle(btn);
  return { bg: cs.backgroundColor, w: btn.offsetWidth, h: btn.offsetHeight };
});
check("任务编辑器标签加号按钮有底色可见", addBtn.bg !== "rgba(0, 0, 0, 0)" && addBtn.w >= 20, JSON.stringify(addBtn));
await closeOverlays(page);

// 日记长单行：桌面双击展开
await page.locator(".system-nav .nav-row").filter({ hasText: "日记" }).first().click();
await page.waitForTimeout(700);
await page.locator(".diary-fab").click();
await page.waitForSelector(".editor-dialog.diary-editor .cm-content", { timeout: 30000 });
await page.locator(".editor-dialog.diary-editor .cm-content").click();
await page.keyboard.insertText(LONG_LINE);
await page.waitForTimeout(300);
await closeOverlays(page);
await page.waitForTimeout(400);

const card = page.locator(".diary-card").first();
check("长单行日记卡片算可展开", (await card.getAttribute("class"))?.includes("expandable") === true);
await card.dblclick();
await page.waitForTimeout(600);
check("桌面双击展开长单行日记", (await card.getAttribute("class"))?.includes("expanded") === true);
// 展开态稳定：动别的卡片不该让它自己收起/弹开
await page.locator(".diary-fab").click();
await page.waitForSelector(".editor-dialog.diary-editor .cm-content", { timeout: 30000 });
await page.locator(".editor-dialog.diary-editor .cm-content").click();
await page.keyboard.insertText("第二篇日记");
await page.waitForTimeout(300);
await closeOverlays(page);
await page.waitForTimeout(400);
check("新增别的日记后展开态不变", (await card.getAttribute("class"))?.includes("expanded") === true);
await card.dblclick();
await page.waitForTimeout(500);
check("桌面双击可收起", (await card.getAttribute("class"))?.includes("expanded") === false);
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

// 工具栏：13 个按钮、不抢焦点、包裹选区（移动端首屏是列表视图，先进条目内容页）
await m.locator(".custom-nav .tree-row").first().click();
await m.waitForTimeout(700);
await m.locator(".composer-plus").click();
await m.waitForSelector(".editor-cm-host .cm-content", { timeout: 30000 });
check("移动端编辑器有 markdown 工具栏", (await m.locator(".editor-md-toolbar button").count()) === 13);
await m.locator(".editor-cm-host .cm-content").click();
await m.keyboard.insertText("工具栏测试文字");
await m.waitForTimeout(200);
await m.evaluate(() => {
  const view = document.querySelector(".editor-cm-host .cm-content");
  const sel = window.getSelection();
  sel?.selectAllChildren(view);
});
await m.waitForTimeout(200);
await m.locator(".editor-md-toolbar button[title='加粗']").click();
await m.waitForTimeout(300);
const cmText = await m.locator(".editor-cm-host .cm-content").innerText();
check("加粗按钮包裹选区", cmText.includes("**工具栏测试文字**"), cmText.slice(0, 40));
const focusOk = await m.evaluate(() => document.activeElement?.closest(".cm-editor") !== null);
check("工具栏不抢编辑器焦点", focusOk);
await m.locator(".editor-md-toolbar button[title='待办项']").click();
await m.waitForTimeout(300);
check("待办项按钮加行前缀", (await m.locator(".editor-cm-host .cm-content").innerText()).includes("- [ ] "));
await m.locator(".editor-md-toolbar button[title='二级标题']").click();
await m.waitForTimeout(300);
check("标题按钮加 # 前缀", (await m.locator(".editor-cm-host .cm-content").innerText()).includes("## "));
await closeOverlays(m);
await m.waitForTimeout(400);

// 一般卡片移动端正文占满（隐藏勾选框 + 隐藏编辑列后不留空列）
await m.locator(".list-header button[title='更多操作']").click();
await m.waitForTimeout(400);
await m.locator(".header-menu-panel button", { hasText: "列表菜单" }).first().click();
await m.waitForTimeout(500);
await m.locator(".context-menu button", { hasText: "卡片类型" }).first().click();
await m.waitForTimeout(400);
await m.locator(".context-menu button", { hasText: "一般卡片" }).first().click();
await m.waitForTimeout(600);
const widths = await m.evaluate(() => {
  const card = document.querySelector(".task-card");
  const body = card?.querySelector(".task-body");
  if (!card || !body) return null;
  return { card: card.clientWidth, body: body.clientWidth, plain: card.classList.contains("plain") };
});
check("列表已切到一般卡片", widths !== null && widths.plain, JSON.stringify(widths));
check(
  "一般卡片移动端正文占满卡片宽",
  widths !== null && widths.plain && widths.body >= widths.card - 40,
  JSON.stringify(widths)
);

// 标签两段式点按：第一下露红叉不删除
await m.locator(".composer-plus").click();
await m.waitForSelector(".editor-cm-host .cm-content", { timeout: 30000 });
await m.locator(".editor-meta-trigger.editor-tag-add").click();
await m.waitForTimeout(300);
await m.locator(".editor-tag-pop input").fill("点按标签");
await m.locator(".tag-add-btn").click();
await m.waitForTimeout(300);
await m.locator(".editor-cm-host .cm-content").click();
await m.waitForTimeout(300);
const tagBefore = await m.locator(".editor-dialog .task-tag").count();
await m.locator(".editor-dialog .task-tag").first().click();
await m.waitForTimeout(400);
const tagAfter = await m.locator(".editor-dialog .task-tag").count();
const revealed = await m.locator(".editor-dialog .task-tag.reveal-delete").count();
check("移动端第一下点标签只露红叉不删除", tagBefore === 1 && tagAfter === 1 && revealed === 1, `before=${tagBefore} after=${tagAfter} revealed=${revealed}`);
await m.locator(".editor-dialog .task-tag .tag-delete").click();
await m.waitForTimeout(400);
check("露出后点红叉才删除", (await m.locator(".editor-dialog .task-tag").count()) === 0);
await closeOverlays(m);

check("全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failures === 0 ? "V069 FIXES ALL PASS" : `V069 FIXES FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

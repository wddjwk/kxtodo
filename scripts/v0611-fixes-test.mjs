// v0.6.11 回归：列表前缀光标位置、工具栏顺序（checkbox 在超链接与 h1 之间）、
// 移动端元数据触发器只留图标、特性开关灰色卡片无行下注释、「已完成」默认折叠且按视图记忆、
// 分割线细虚线、工具栏受特性开关控制（移动端同）。
// 用法：node scripts/v0611-fixes-test.mjs（需先 npm run dev）。
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

await page.locator(".composer-plus").click();
await page.waitForSelector(".editor-dialog", { timeout: 30000 });

// 工具栏顺序：第 8 个按钮是待办项（超链接之后、h1 之前）
const titles = await page.$$eval(".editor-dialog .editor-md-toolbar button", (els) =>
  els.map((el) => el.getAttribute("title"))
);
check("工具栏 16 个按钮", titles.length === 16, String(titles.length));
check("checkbox 在超链接与 h1 之间", titles[6] === "超链接" && titles[7] === "待办项" && titles[8] === "一级标题", titles.slice(6, 9).join("/"));

// 列表前缀光标：点待办项后接着打字应接在「- [ ] 」后面
await page.locator(".editor-cm-host .cm-content").click();
await page.locator(".editor-md-toolbar button[title='待办项']").click();
await page.waitForTimeout(200);
await page.keyboard.insertText("事项");
await page.waitForTimeout(200);
const line1 = await page.locator(".editor-cm-host .cm-content").innerText();
check("待办项前缀后光标正确", line1.startsWith("- [ ] 事项"), JSON.stringify(line1.slice(0, 14)));
await page.locator(".editor-md-toolbar button[title='有序列表']").click();
await page.waitForTimeout(200);
await page.keyboard.insertText("x");
await page.waitForTimeout(200);
const line2 = await page.locator(".editor-cm-host .cm-content").innerText();
check("有序列表前缀后光标正确", line2.includes("1. x"), JSON.stringify(line2.slice(0, 14)));

// 分割线：细虚线
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.press("Control+a");
await page.keyboard.insertText("上段\n\n---\n\n下段");
await page.waitForTimeout(200);
await page.locator(".editor-mode-switch button", { hasText: "预览" }).click();
await page.waitForTimeout(400);
const hrStyle = await page.locator(".editor-preview hr").first().evaluate((el) => {
  const cs = getComputedStyle(el);
  return { h: el.getBoundingClientRect().height, style: cs.borderTopStyle, color: cs.borderTopColor };
});
check("分割线是 1px 细虚线", hrStyle.h <= 2 && hrStyle.style === "dashed", JSON.stringify(hrStyle));
await page.keyboard.press("Escape");
await page.waitForTimeout(400);
await page.keyboard.press("Escape");
await page.waitForTimeout(400);

// 特性开关：一整块灰色卡片、三行开关、无行下注释
await page.keyboard.press("Control+,");
await page.waitForTimeout(600);
const cardInfo = await page.evaluate(() => {
  const card = document.querySelector("aside.settings-drawer .settings-card");
  if (!card) return null;
  return {
    rows: card.querySelectorAll(".toggle-row").length,
    muted: card.querySelectorAll(".muted").length,
    bg: getComputedStyle(card).backgroundColor,
    titled: [...card.querySelectorAll(".toggle-row")].every((row) => Boolean(row.getAttribute("title")))
  };
});
check("特性开关是一整块灰卡三行开关", cardInfo !== null && cardInfo.rows === 3 && cardInfo.muted === 0, JSON.stringify(cardInfo));
check("开关说明在 title 悬浮提示里", cardInfo !== null && cardInfo.titled);
check("灰卡有底色", cardInfo !== null && cardInfo.bg !== "rgba(0, 0, 0, 0)", cardInfo?.bg);
await page.locator("button.settings-backdrop").click();
await page.waitForTimeout(400);

// 「已完成」默认折叠 + 按视图记忆
await page.locator(".add-task-bar textarea").fill("待完成的事");
await page.keyboard.press("Enter");
await page.waitForTimeout(500);
await page.locator(".task-card .task-check").first().click();
await page.waitForTimeout(600);
check("已完成区默认折叠", (await page.locator(".completed-section .task-card").count()) === 0);
await page.locator(".completed-toggle").click();
await page.waitForTimeout(400);
check("展开后显示已完成卡片", (await page.locator(".completed-section .task-card").count()) === 1);
await page.reload({ waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(900);
check("刷新后记住展开偏好", (await page.locator(".completed-section .task-card").count()) === 1);
await page.locator(".completed-toggle").click();
await page.waitForTimeout(400);
await page.reload({ waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(900);
check("收起后也记住", (await page.locator(".completed-section .task-card").count()) === 0);
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

// 日记编辑器：心情/天气/标签只留图标
await m.locator(".system-nav .nav-row").filter({ hasText: "日记" }).first().click();
await m.waitForTimeout(700);
await m.locator(".diary-fab").click();
await m.waitForSelector(".editor-dialog.diary-editor", { timeout: 30000 });
const labelHidden = await m.evaluate(() => {
  const labels = [...document.querySelectorAll(".editor-dialog .editor-meta-trigger .trigger-label")];
  return labels.length >= 3 && labels.every((el) => getComputedStyle(el).display === "none");
});
check("移动端心情/天气/标签只留图标", labelHidden);
// 工具栏在移动端同样受特性开关控制（默认开 → 显示）
check("移动端工具栏默认显示", (await m.locator(".editor-dialog .editor-md-toolbar button").count()) === 16);
await m.close();

check("全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failures === 0 ? "V0611 FIXES ALL PASS" : `V0611 FIXES FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

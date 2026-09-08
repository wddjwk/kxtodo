// markdown 扩展渲染回归：callout / 数学公式 / mermaid / markmap / 代码折叠 /
// front-matter / 链接图标 / 图的全屏与源码切换。用法：node scripts/markdown-ext-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));

await page.goto(URL, { waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "load", timeout: 90000 });
await page.waitForTimeout(700);

const MD = [
  "---",
  "title: 测试日记",
  "date: 2026-09-08",
  "mood: 🙂",
  "---",
  "",
  "正文里 inline math $E = mc^2$ 与链接 https://example.com 看看。",
  "",
  "> [!tip] 小贴士",
  "> 这是一段 callout 内容。",
  "",
  "> [!warning] 注意",
  "> 警告内容。",
  "",
  "$$\\int_0^1 x^2 dx = \\frac{1}{3}$$",
  "",
  "```mermaid",
  "graph TD",
  "  A[开始] --> B{判断}",
  "```",
  "",
  "```markmap",
  "# 主题",
  "## 分支一",
  "- 叶子 1",
  "```",
  "",
  "```rust",
  Array.from({ length: 24 }, (_, i) => `let line_${i} = ${i};`).join("\n"),
  "```",
  "",
  "```rust",
  "let short = 1;",
  "```"
].join("\n");

await page.locator(".composer-plus").click();
await page.waitForSelector(".editor-cm-host .cm-content", { timeout: 30000 });
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.insertText(MD);
await page.waitForTimeout(300);
await page.locator(".editor-mode-switch button", { hasText: "预览" }).click();
await page.waitForSelector(".editor-preview .diagram-canvas[data-diagram='mermaid'] svg", { timeout: 30000 });
await page.waitForTimeout(800);

check("front-matter 渲染成键值表", (await page.locator(".editor-preview .front-matter .fm-row").count()) === 3);
check("front-matter 不再露出 ---", !(await page.locator(".editor-preview").innerText()).includes("---"));
check("callout 两块", (await page.locator(".editor-preview .admonition").count()) === 2);
check("callout 类型着色(tip/warning)", (await page.locator(".editor-preview .admonition.tip").count()) === 1 && (await page.locator(".editor-preview .admonition.warning").count()) === 1);
check("callout 头部有图标", (await page.locator(".editor-preview .admonition-header svg").count()) === 2);
check("内联公式渲染", (await page.locator(".editor-preview .katex").count()) >= 1);
check("块级公式渲染", (await page.locator(".editor-preview .kx-math-block").count()) === 1);
check("mermaid 渲染成 svg", (await page.locator(".editor-preview .diagram-canvas[data-diagram='mermaid'] svg").count()) === 1);
check("markmap 渲染成 svg", (await page.locator(".editor-preview .diagram-canvas[data-diagram='markmap'] svg").count()) === 1);
check("代码块带语言条", (await page.locator(".editor-preview .code-block").count()) === 2);
check("长代码默认折叠", (await page.locator(".editor-preview .code-collapsible.collapsed").count()) === 1);
check(
  "链接带图标",
  await page.locator(".editor-preview p a").first().evaluate((el) => getComputedStyle(el, "::before").backgroundImage.includes("svg"))
);

await page.locator(".editor-preview .code-toggle").first().click();
await page.waitForTimeout(250);
check("代码折叠可展开", (await page.locator(".editor-preview .code-collapsible.collapsed").count()) === 0);

await page.locator(".editor-preview .diagram-source-toggle").first().click();
await page.waitForTimeout(250);
check("图源码可切换", (await page.locator(".editor-preview pre.diagram-source:not([hidden])").count()) === 1);
await page.locator(".editor-preview .diagram-source-toggle").first().click();
await page.waitForTimeout(200);

await page.locator(".editor-preview .diagram-fullscreen").first().click();
await page.waitForTimeout(400);
check("图全屏打开", (await page.locator(".md-fullscreen-overlay.active").count()) === 1);
await page.locator(".md-fullscreen-close").click();
await page.waitForTimeout(250);
check("图全屏可退出", (await page.locator(".md-fullscreen-overlay.active").count()) === 0);

check("全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failures === 0 ? "MARKDOWN EXT ALL PASS" : `MARKDOWN EXT FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

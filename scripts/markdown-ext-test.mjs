// markdown 扩展渲染回归：callout / 数学公式（含 $ $、\( \)、\[ \] 与货币反例）/ mermaid /
// markmap+mindmap / 代码折叠（含「折叠不旋转」回归）/ front-matter / 链接图标 / 任务列表 /
// :emoji: / 图的全屏（Esc 与按钮退出、不再双击退出、markmap 全屏重建）与源码切换。
// 用法：node scripts/markdown-ext-test.mjs（需先 npm run dev）。
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
  "带空格的行内公式 $ x^2 + 1 $ 也要渲染。",
  "",
  "括号行内 \\(a^2 + b^2\\) 与块级：",
  "",
  "\\[\\int_0^1 x^2 dx = \\frac{1}{3}\\]",
  "",
  "货币不是公式：$5 和 $10 之间。",
  "",
  "表情短码 :smile: 与 :tada: 应替换。",
  "",
  "> [!tip] 小贴士",
  "> 这是一段 callout 内容。",
  "",
  "> [!warning] 注意",
  "> 警告内容。",
  "",
  "$$\\sum_{i=1}^{n} i$$",
  "",
  "- [ ] 未完成的事",
  "- [x] 已完成的事",
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
  "```mindmap",
  "# 另一个主题",
  "## 分支二",
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
await page.waitForTimeout(900);

check("front-matter 渲染成键值表", (await page.locator(".editor-preview .front-matter .fm-row").count()) === 3);
check("front-matter 不再露出 ---", !(await page.locator(".editor-preview").innerText()).includes("---"));
check("callout 两块", (await page.locator(".editor-preview .admonition").count()) === 2);
check("callout 头部有图标", (await page.locator(".editor-preview .admonition-header svg").count()) === 2);

// 数学：4 处行内（$..$、$ .. $、\(..\)）+ 2 处块级（\[..\]、$$..$$）
const mathInfo = await page.evaluate(() => {
  const root = document.querySelector(".editor-preview");
  const paras = [...root.querySelectorAll("p")].map((p) => ({
    text: (p.textContent || "").slice(0, 40),
    katex: p.querySelectorAll(".katex").length
  }));
  return {
    paras,
    blocks: root.querySelectorAll(".kx-math-block").length,
    blockTag: root.querySelector(".kx-math-block")?.tagName ?? "",
    currency: [...root.querySelectorAll("p")].find((p) => (p.textContent || "").includes("货币"))?.textContent || ""
  };
});
check("行内 $..$ 渲染", mathInfo.paras.some((p) => p.text.includes("inline math") && p.katex === 1));
check("行内 $ .. $ （带空格）渲染", mathInfo.paras.some((p) => p.text.includes("带空格") && p.katex === 1));
check("行内 \\(..\\) 渲染", mathInfo.paras.some((p) => p.text.includes("括号行内") && p.katex === 1));
check("块级 \\[..\\] 与 $$..$$ 渲染", mathInfo.blocks === 2, `blocks=${mathInfo.blocks}`);
check("块级公式是 span（不截断段落）", mathInfo.blockTag === "SPAN", mathInfo.blockTag);
check("货币 $5 和 $10 不被当公式", mathInfo.currency.includes("$5") && mathInfo.currency.includes("$10"));

check(":emoji: 短码替换", (await page.locator(".editor-preview").innerText()).includes("😄"));

// 任务列表：checkbox 在、列表圆点没了
const taskList = await page.evaluate(() => {
  const li = [...document.querySelectorAll(".editor-preview li")].filter((el) => el.querySelector(":scope > input[type=checkbox]"));
  return li.map((el) => ({ listStyle: getComputedStyle(el).listStyleType, boxes: el.querySelectorAll("input[type=checkbox]").length }));
});
check("任务列表两项都有 checkbox", taskList.length === 2 && taskList.every((t) => t.boxes === 1));
check("任务列表去掉列表圆点", taskList.length === 2 && taskList.every((t) => t.listStyle === "none"));

check("mermaid 渲染成 svg", (await page.locator(".editor-preview .diagram-canvas[data-diagram='mermaid'] svg").count()) === 1);
check("markmap 渲染成 svg", (await page.locator(".editor-preview .diagram-canvas[data-diagram='markmap'] svg").count()) === 1);
check("mindmap 围栏也渲染成 markmap", (await page.locator(".editor-preview .diagram-canvas[data-diagram='mindmap'] svg").count()) === 1);
check("代码块带语言条", (await page.locator(".editor-preview .code-block").count()) === 2);
check("长代码默认折叠", (await page.locator(".editor-preview .code-collapsible.collapsed").count()) === 1);

// 4.1 回归：折叠态不许旋转（base.css 的通用 .collapsed 曾把整块代码转 90°）
const collapseTransform = await page.evaluate(() => {
  const block = document.querySelector(".editor-preview .code-collapsible.collapsed");
  const pre = block.querySelector("pre");
  return {
    block: getComputedStyle(block).transform,
    pre: getComputedStyle(pre).transform,
    preW: pre.offsetWidth,
    preH: pre.offsetHeight
  };
});
check(
  "折叠态代码块不旋转、不竖起来",
  collapseTransform.block === "none" && collapseTransform.pre === "none" && collapseTransform.preW > collapseTransform.preH,
  JSON.stringify(collapseTransform)
);

check(
  "链接带图标",
  await page.locator(".editor-preview p a").first().evaluate((el) => getComputedStyle(el, "::before").backgroundImage.includes("svg"))
);

await page.locator(".editor-preview .code-toggle").first().click();
await page.waitForTimeout(250);
check("代码折叠可展开", (await page.locator(".editor-preview .code-collapsible.collapsed").count()) === 0);
await page.locator(".editor-preview .code-toggle").first().click();
await page.waitForTimeout(250);
check("代码可再收起", (await page.locator(".editor-preview .code-collapsible.collapsed").count()) === 1);

await page.locator(".editor-preview .diagram-source-toggle").first().click();
await page.waitForTimeout(250);
check("图源码可切换", (await page.locator(".editor-preview pre.diagram-source:not([hidden])").count()) === 1);
await page.locator(".editor-preview .diagram-source-toggle").first().click();
await page.waitForTimeout(200);

// 全屏：markmap 全屏要重建（克隆不带 d3 监听，会拖不动缩不了）
await page.locator(".editor-preview .diagram-frame[data-diagram]")
  .filter({ has: page.locator(".diagram-lang", { hasText: "markmap" }) })
  .locator(".diagram-fullscreen")
  .click();
await page.waitForTimeout(700);
check("markmap 全屏打开", (await page.locator(".md-fullscreen-overlay.active").count()) === 1);
check("markmap 全屏里重建出 svg（可拖拽缩放）", (await page.locator(".md-fullscreen-stage .diagram-canvas svg").count()) >= 1);
const hint = await page.locator(".md-fullscreen-bar span").first().innerText();
check("全屏提示不再提双击", !hint.includes("双击"), hint);
await page.locator(".md-fullscreen-stage").click({ position: { x: 40, y: 300 } });
await page.locator(".md-fullscreen-stage").dblclick({ position: { x: 40, y: 320 } });
await page.waitForTimeout(300);
check("双击不再退出全屏", (await page.locator(".md-fullscreen-overlay.active").count()) === 1);
await page.keyboard.press("Escape");
await page.waitForTimeout(300);
check("Esc 退出全屏", (await page.locator(".md-fullscreen-overlay.active").count()) === 0);

await page.locator(".editor-preview .diagram-fullscreen").first().click();
await page.waitForTimeout(400);
await page.locator(".md-fullscreen-close").click();
await page.waitForTimeout(250);
check("按钮退出全屏", (await page.locator(".md-fullscreen-overlay.active").count()) === 0);

check("全程无 pageerror", errors.length === 0, errors.slice(0, 3).join(" | "));
await browser.close();
console.log(failures === 0 ? "MARKDOWN EXT ALL PASS" : `MARKDOWN EXT FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

// v0.7.7 回归：
// 1 移动端齿轮按钮点开又收起后不留底色（触屏没有 hover）；
// 2 编辑器 ESC：标签输入框不再吃掉 Escape（先收浮层、再关编辑器，等价 ctrl+S）；
// 3 桌面记账编辑框：高度 = 宽度 − 一个分类图标，三排（备注+金额/日期账户图片/底栏）贴底，开搁板不跳（v076 覆盖）；
// 4 总资产趋势：点图直接读数（不拉浮窗），只有全屏按钮进全屏；坐标标签互不覆盖；
// 5 选择图标：「常用图标」（最近使用、表情与简笔画混排、最多两行）置顶，简笔画区固定五行自滚不显滚动条；
// 6 移动端总资产三块靠右对齐（桌面靠左）；
// 7 超链接自动解析标题：裸链接换成网页标题（31 字截断），手写的 [文字](链接) 不动；
// 8 超链接渲染为卡片（站点 + 复制按钮 / 标题 / 正文预览）。
// v0.7.8 起：标题上限 60 字、卡片默认开、复制按钮只有图标，「标题/卡片」合成一组
// 「超链接渲染样式」开关（本套只做最小适配，细则由 v078-fixes-test.mjs 覆盖）。
// 用法：node scripts/v077-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const PAGE_HTML = `<!doctype html><html><head>
<meta charset="utf-8">
<title>示例站点的一篇很长很长的文章标题超过了三十个字所以应该被截断显示省略号才对哦</title>
<meta property="og:description" content="这是一段正文预览摘要，用来占位看看两行截断的效果是否正常。">
<meta property="og:site_name" content="示例站">
</head><body>正文</body></html>`;

/** 首行不带链接（否则双击展开会点到链接上），两张链接分别落在裸链接与手写链接 */
const MARKDOWN = [
  "链接检查",
  "",
  "裸链接 https://example.com/page 在这里",
  "",
  "手写的链接 [我的文字](https://example.com/other) 不动"
].join("\n");

async function stubLinks(page) {
  const handler = (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html; charset=utf-8",
      headers: { "access-control-allow-origin": "*" },
      body: PAGE_HTML
    });
  await page.route("**/example.com/**", handler);
  await page.route("**/example.com", handler);
}

async function freshPage(context) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(700);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(800);
  return { page, errors };
}

/** 改浏览器预览里的设置（localStorage legacy 路径）后重载 */
async function setFeatures(page, features) {
  await page.evaluate((patch) => {
    const key = "todo-note-settings-v3";
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.features = { ...(settings.features ?? {}), ...patch };
    localStorage.setItem(key, JSON.stringify(settings));
  }, features);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
}

/** 种一条展开的链接任务（走 localStorage 数据面，不依赖编辑器） */
async function seedLinkTask(page, markdown) {
  await page.evaluate((text) => {
    const now = new Date().toISOString();
    const state = {
      schemaVersion: 3,
      nodes: [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "entry-link", kind: "entry", name: "链接测试", icon: "list", parentId: null, createdAt: now }
      ],
      tasks: [
        {
          id: "task-link",
          nodeId: "entry-link",
          markdown: text,
          completed: false,
          important: false,
          myDay: false,
          tags: [],
          emojis: [],
          expanded: true,
          createdAt: now,
          updatedAt: now
        }
      ],
      selectedNodeId: "entry-link",
      backgrounds: []
    };
    localStorage.setItem("todo-note-state-v3", JSON.stringify(state));
  }, markdown);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(1200);
}

async function openLedger(page) {
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  await page.waitForTimeout(300);
}

async function switchView(page, label) {
  await page.click(`.ledger-view-switch button[title='${label}']`);
  await page.waitForTimeout(300);
}

/** 种一本带流水的账（趋势图才有多个采样点） */
async function seedBook(page) {
  await page.evaluate(() => {
    const day = (offset) => {
      const d = new Date();
      d.setDate(d.getDate() - offset);
      return d.toISOString().slice(0, 10);
    };
    const now = new Date().toISOString();
    localStorage.setItem(
      "kxtodo-ledger-book",
      JSON.stringify({
        accounts: [
          { id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now },
          { id: "lacc-02", name: "储蓄卡", kind: "储蓄卡", icon: "Landmark", color: "#4a90d9", initialCents: 500000, createdAt: now }
        ],
        categories: [
          { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now },
          { id: "lcat-inc-01", name: "工资", side: "income", icon: "Wallet", color: "#2f9e6e", parentId: "", createdAt: now }
        ],
        entries: [0, 3, 6, 9, 12].flatMap((offset, i) => [
          { id: `le-e${i}`, kind: "expense", amountCents: 2000 + i * 700, accountId: "lacc-01", categoryId: "lcat-exp-01", date: day(offset), note: "支出", createdAt: now },
          { id: `le-i${i}`, kind: "income", amountCents: 40000 + i * 3000, accountId: "lacc-02", categoryId: "lcat-inc-01", date: day(offset - 1), note: "收入", createdAt: now }
        ]),
        accountTypes: []
      })
    );
  });
}

/** 坐标标签两两不许重叠（「纵轴起点值和横轴起点值不要互相打架覆盖」） */
async function axisOverlaps(page, rootSelector) {
  return page.evaluate((selector) => {
    const rects = [...document.querySelectorAll(`${selector} .ledger-chart-axis`)].map((el) => {
      const r = el.getBoundingClientRect();
      return { text: el.textContent ?? "", x: r.x, y: r.y, w: r.width, h: r.height };
    });
    const hits = [];
    for (let i = 0; i < rects.length; i += 1) {
      for (let j = i + 1; j < rects.length; j += 1) {
        const a = rects[i];
        const b = rects[j];
        if (a.x < b.x + b.w - 1 && b.x < a.x + a.w - 1 && a.y < b.y + b.h - 1 && b.y < a.y + a.h - 1) {
          hits.push([a.text, b.text]);
        }
      }
    }
    return hits;
  }, rootSelector);
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------------------------------------------------------------- 桌面
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);
  await stubLinks(page);

  // 2 编辑器 ESC：标签输入框里按 Esc（旧版这里无条件 stopPropagation，Escape 被整个吃掉）
  await page.locator(".composer-plus").click();
  await page.waitForSelector(".editor-dialog .cm-content", { timeout: 30000 });
  await page.locator(".editor-dialog .cm-content").click();
  await page.keyboard.insertText("ESC 测试");
  await page.locator(".editor-meta-trigger.editor-tag-add").click();
  await page.waitForSelector(".editor-tag-pop .tag-editor-input-row input", { timeout: 5000 });
  await page.fill(".editor-tag-pop .tag-editor-input-row input", "标签甲");
  await page.waitForTimeout(200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  check("标签输入框里 Esc 先收浮层（2）", (await page.$$(".editor-tag-pop")).length === 0);
  check("第一下 Esc 不关编辑器（2）", (await page.$$(".editor-overlay")).length === 1);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(600);
  check("再一下 Esc 关掉编辑器（2）", (await page.$$(".editor-overlay")).length === 0);
  check("ESC 等于保存（内容落盘）（2）", (await page.$$(".task-card")).length === 1);

  // 2b 内联标签编辑输入框里按 Esc 同样关编辑器
  await page.locator(".task-card .edit-button").first().click();
  await page.waitForSelector(".editor-dialog .cm-content", { timeout: 8000 });
  await page.locator(".editor-meta-trigger.editor-tag-add").click();
  await page.waitForSelector(".editor-tag-pop .tag-editor-input-row input", { timeout: 5000 });
  await page.fill(".editor-tag-pop .tag-editor-input-row input", "标签乙");
  await page.locator(".editor-tag-pop .tag-add-btn").click();
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await page.locator(".editor-dialog .task-tag").first().click();
  await page.waitForSelector(".editor-dialog .tag-edit-input", { timeout: 5000 });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(600);
  check("内联标签编辑里 Esc 关编辑器（2）", (await page.$$(".editor-overlay")).length === 0);

  // 7 + 8 超链接：先只看「标题」档（显式关掉卡片，v0.7.8 起卡片是默认档）
  await setFeatures(page, { linkRender: "title" });
  await seedLinkTask(page, MARKDOWN);
  const links = await page.$$eval(".task-card .markdown-content a", (els) =>
    els.map((el) => ({ text: el.textContent.trim(), href: el.getAttribute("href") ?? "" }))
  );
  const bare = links.find((item) => item.href.endsWith("/page"));
  const written = links.find((item) => item.href.endsWith("/other"));
  check(
    "裸链接自动换成网页标题（7）",
    bare !== undefined && bare.text.startsWith("示例站点的一篇很长很长的文章标题") && [...bare.text].length <= 61,
    bare?.text
  );
  check("手写的 [我的文字](链接) 不动（7）", written?.text === "我的文字", written?.text);
  check("关掉卡片档就不出卡片（8）", (await page.$$(".task-card .kx-link-card")).length === 0);

  // 8 打开「渲染超链接为卡片」
  await setFeatures(page, { linkRender: "card" });
  await stubLinks(page);
  await page.waitForTimeout(1500);
  const cards = await page.$$eval(".task-card .kx-link-card", (els) =>
    els.map((el) => ({
      site: el.querySelector(".kx-link-card-site")?.textContent ?? "",
      title: el.querySelector(".kx-link-card-title")?.textContent ?? "",
      desc: el.querySelector(".kx-link-card-desc")?.textContent ?? "",
      copyIcon: Boolean(el.querySelector(".kx-link-card-copy svg")),
      copyText: (el.querySelector(".kx-link-card-copy")?.textContent ?? "").trim(),
      icon: Boolean(el.querySelector(".kx-link-card-head svg, .kx-link-card-favicon img, img.kx-link-card-favicon")),
      href: el.querySelector(".kx-link-card-main")?.getAttribute("href") ?? ""
    }))
  );
  check("卡片模式：两种链接都成卡片（8）", cards.length === 2, String(cards.length));
  check(
    "卡片有 站点/标题/摘要/图标/复制按钮（8）",
    cards.every(
      (item) =>
        item.site === "示例站" &&
        item.title.length > 0 &&
        item.desc.length > 0 &&
        item.copyIcon &&
        item.copyText === "" &&
        item.icon
    ),
    JSON.stringify(cards[0] ?? {})
  );
  check(
    "卡片链接指向原地址（8）",
    cards.some((item) => item.href.endsWith("/page")) && cards.some((item) => item.href.endsWith("/other"))
  );
  check("用户原文没有被改动（8）", ((await page.textContent(".task-card .markdown-content")) ?? "").includes("手写的链接"));

  // 7 关掉自动标题：裸链接保持原样
  await setFeatures(page, { linkRender: "off" });
  await stubLinks(page);
  await page.waitForTimeout(800);
  const plainText = await page.$$eval(".task-card .markdown-content a", (els) => els.map((el) => el.textContent.trim()));
  check("关掉开关后裸链接保持原样（7）", plainText.includes("https://example.com/page"), JSON.stringify(plainText));

  // 4 趋势：点图读数 / 全屏按钮才开浮窗 / 坐标标签不互相覆盖
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(600);
  await seedBook(page);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await openLedger(page);
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-trend-card", { timeout: 8000 });
  const chartBox = await page.locator(".ledger-trend-card .ledger-chart-box").boundingBox();
  await page.mouse.move(chartBox.x + chartBox.width * 0.5, chartBox.y + chartBox.height * 0.5);
  await page.waitForSelector(".ledger-trend-card .ledger-chart-tip", { timeout: 3000 });
  check(
    "桌面悬浮读数字写在卡片里（4）",
    ((await page.textContent(".ledger-trend-card .ledger-chart-tip")) ?? "").includes("总资产")
  );
  await page.mouse.click(chartBox.x + chartBox.width * 0.3, chartBox.y + chartBox.height * 0.6);
  await page.waitForTimeout(300);
  check("点卡片不拉浮窗（4）", (await page.$$(".ledger-trend-dialog")).length === 0);
  const cardOverlaps = await axisOverlaps(page, ".ledger-trend-card");
  check("卡片上坐标标签互不覆盖（4）", cardOverlaps.length === 0, JSON.stringify(cardOverlaps));
  await page.click(".ledger-trend-card .ledger-trend-zoom");
  await page.waitForSelector(".ledger-trend-dialog", { timeout: 8000 });
  check("全屏按钮才开放大浮窗（4）", (await page.$$(".ledger-trend-dialog")).length === 1);
  const dialogOverlaps = await axisOverlaps(page, ".ledger-trend-dialog");
  check("浮窗里坐标标签互不覆盖（4）", dialogOverlaps.length === 0, JSON.stringify(dialogOverlaps));
  await page.click(".ledger-trend-dialog .ledger-icon-button[title='关闭']");
  await page.waitForSelector(".ledger-trend-dialog", { state: "detached", timeout: 8000 });

  // 6 桌面总资产三块保持靠左
  const deskSplit = await page.evaluate(() => {
    const split = document.querySelector(".ledger-net-split");
    const card = document.querySelector(".ledger-net-card");
    const cells = [...split.children];
    const last = cells[cells.length - 1].getBoundingClientRect();
    const first = cells[0].getBoundingClientRect();
    const cardRect = card.getBoundingClientRect();
    return { gapRight: Math.round(cardRect.right - last.right), gapLeft: Math.round(first.left - cardRect.left) };
  });
  check("桌面总资产三块保持靠左（6）", deskSplit.gapLeft < 30 && deskSplit.gapRight > 60, JSON.stringify(deskSplit));

  // 3 记账编辑框：展开最后一个大类，二级搁板要被带进可视区（只滚分类区自己）
  await switchView(page, "列表视图");
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  await page.waitForTimeout(400);
  const catCells = page.locator(".ledger-cat-zone .ledger-cat-grid > .ledger-cat-cell:not(.add)");
  const cellCount = await catCells.count();
  let shelfVisible = false;
  for (let index = cellCount - 1; index >= 0; index -= 1) {
    await catCells.nth(index).click();
    await page.waitForTimeout(350);
    if ((await page.$$(".ledger-cat-sub")).length > 0) {
      shelfVisible = await page.evaluate(() => {
        const zone = document.querySelector(".ledger-cat-zone");
        const shelf = document.querySelector(".ledger-cat-sub");
        if (!zone || !shelf) return false;
        const zoneRect = zone.getBoundingClientRect();
        const shelfRect = shelf.getBoundingClientRect();
        return shelfRect.bottom <= zoneRect.bottom + 1 && shelfRect.top >= zoneRect.top - 1;
      });
      break;
    }
  }
  check("展开大类后二级搁板被带进可视区（3）", shelfVisible);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });

  // 5 选择图标：常用图标置顶 + 简笔画区五行自滚
  await page.locator(".tree-row .tree-icon").first().click();
  await page.waitForSelector(".icon-picker", { timeout: 8000 });
  await page.waitForTimeout(300);
  const labels = await page.$$eval(".icon-picker .picker-section-label", (els) => els.map((el) => el.textContent.trim()));
  check("旧「常用表情」一节没了（5）", !labels.includes("常用表情"), JSON.stringify(labels));
  check("没选过图标时常用图标不外显（5）", (await page.$$(".icon-picker .emoji-grid")).length === 0);
  const gridInfo = await page.$eval(".icon-picker .icon-grid", (el) => ({
    h: el.clientHeight,
    scrollH: el.scrollHeight,
    overflow: getComputedStyle(el).overflowY,
    scrollbar: getComputedStyle(el).scrollbarWidth
  }));
  check(
    "简笔画区固定五行高、自己滚、不画滚动条（5）",
    gridInfo.h === 238 && gridInfo.scrollH > gridInfo.h + 100 && gridInfo.overflow === "auto" && gridInfo.scrollbar === "none",
    JSON.stringify(gridInfo)
  );
  await page.locator(".icon-picker .icon-grid button").nth(2).click();
  await page.waitForTimeout(400);
  await page.locator(".tree-row .tree-icon").first().click();
  await page.waitForSelector(".icon-picker", { timeout: 8000 });
  await page.waitForTimeout(300);
  const recentLabels = await page.$$eval(".icon-picker .picker-section-label", (els) => els.map((el) => el.textContent.trim()));
  check("「常用图标」排在第一个（5）", recentLabels[0] === "常用图标", JSON.stringify(recentLabels));
  const recents = await page.$$eval(".icon-picker .emoji-grid button", (els) => els.map((el) => el.title || el.textContent.trim()));
  check("选过的图标进「常用图标」（5）", recents.length === 1, JSON.stringify(recents));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  // 最近 16 个：两行封顶、表情与简笔画混排
  await page.evaluate(() => {
    localStorage.setItem(
      "kxtodo-recent-icons",
      JSON.stringify(["🚀", "CupSoda", "📚", "Heart", "🎯", "Star", "💡", "Landmark", "🔥", "Plane", "📌", "Music", "🏆", "Camera", "⚡", "Rocket"])
    );
  });
  await page.locator(".tree-row .tree-icon").first().click();
  await page.waitForSelector(".icon-picker", { timeout: 8000 });
  await page.waitForTimeout(300);
  const recentBox = await page.$eval(".icon-picker .emoji-grid", (el) => ({
    count: el.children.length,
    h: el.clientHeight
  }));
  check("常用图标最多两行（5）", recentBox.count === 16 && recentBox.h <= 42 * 2 + 12, JSON.stringify(recentBox));
  const mixedIcons = await page.$$eval(".icon-picker .emoji-grid button", (els) =>
    els.filter((el) => el.querySelector("svg")).length
  );
  check("常用图标里表情与简笔画混排（5）", mixedIcons > 0 && mixedIcons < recentBox.count, String(mixedIcons));
  await page.keyboard.press("Escape");

  check("桌面没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------------------------------------------------------------- 移动端
{
  const context = await browser.newContext({
    viewport: { width: 400, height: 860 },
    userAgent:
      "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36",
    hasTouch: true,
    isMobile: true
  });
  const { page, errors } = await freshPage(context);
  await stubLinks(page);

  // 1 齿轮按钮：点开又收起后不留底色
  await openLedger(page);
  const gear = page.locator(".ledger-view .header-actions > button").last();
  await gear.tap();
  await page.waitForTimeout(300);
  await gear.tap();
  await page.waitForTimeout(300);
  const gearBg = await gear.evaluate((el) => getComputedStyle(el).backgroundColor);
  check("齿轮收起后不留底色（1）", /rgba\(0, 0, 0, 0\)|transparent/.test(gearBg), gearBg);

  // 6 移动端总资产三块靠右
  await seedBook(page);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await openLedger(page);
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-net-split", { timeout: 8000 });
  const mobileSplit = await page.evaluate(() => {
    const split = document.querySelector(".ledger-net-split");
    const card = document.querySelector(".ledger-net-card");
    const cells = [...split.children];
    const last = cells[cells.length - 1].getBoundingClientRect();
    const first = cells[0].getBoundingClientRect();
    const cardRect = card.getBoundingClientRect();
    return { gapRight: Math.round(cardRect.right - last.right), gapLeft: Math.round(first.left - cardRect.left) };
  });
  check("移动端总资产三块靠右（6）", mobileSplit.gapRight < 30 && mobileSplit.gapLeft > 60, JSON.stringify(mobileSplit));

  // 5 移动端选择图标：简笔画区同样自己滚（不显滚动条）
  // 记账是整页层（侧栏被藏起来），先回列表视图
  await page.goBack();
  await page.waitForTimeout(500);
  await page.locator(".tree-row .tree-icon").first().tap();
  await page.waitForSelector(".icon-picker", { timeout: 8000 });
  await page.waitForTimeout(300);
  const mGrid = await page.$eval(".icon-picker .icon-grid", (el) => ({
    h: el.clientHeight,
    scrollH: el.scrollHeight,
    scrollbar: getComputedStyle(el).scrollbarWidth
  }));
  check(
    "移动端简笔画区自己滚（不显滚动条）（5）",
    mGrid.h === 238 && mGrid.scrollH > mGrid.h + 100 && mGrid.scrollbar === "none",
    JSON.stringify(mGrid)
  );
  await page.locator(".icon-picker .icon-grid button").first().tap();
  await page.waitForTimeout(400);

  // 7 + 8 移动端：超链接同样渲染成卡片
  await setFeatures(page, { linkRender: "card" });
  await stubLinks(page);
  await seedLinkTask(page, MARKDOWN);
  await page.locator(".tree-row:has-text('链接测试')").first().click();
  await page.waitForTimeout(1500);
  const mobileCards = await page.$$(".task-card .kx-link-card");
  check("移动端超链接也渲染成卡片（8）", mobileCards.length === 2, String(mobileCards.length));
  const mobileSite = await page.$eval(".task-card .kx-link-card-site", (el) => el.textContent.trim()).catch(() => "");
  check("移动端卡片带站点信息（8）", mobileSite === "示例站", mobileSite);

  check("移动端没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();

console.log(failures === 0 ? "\n全部通过" : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);

// 日记 + v0.6.6 相关改动的回归（playwright-core + 系统 Edge）。
// 浏览器 dev 走 localStorage legacy 路径，足够验证纯前端逻辑；
// core 命令层、zip 打包与同步由 cargo test 覆盖（cli_diary / diary_archive / sync::merge）。
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

// ---------- desktop ----------
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(String(e)));
await page.goto(URL, { waitUntil: "networkidle" });
await page.waitForTimeout(700);
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(700);

const navRows = await page.locator(".system-nav .nav-row").allInnerTexts();
const diaryIndex = navRows.findIndex((t) => t.includes("日记"));
check("侧栏有日记行，且在收藏下面", diaryIndex === navRows.findIndex((t) => t.includes("收藏")) + 1, JSON.stringify(navRows));

await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
check("日记界面打开", await page.locator(".diary-view").isVisible());
check("任务工作区被隐藏", !(await page.locator(".workspace").isVisible()));
check("三种视图切换按钮", (await page.locator(".diary-view-switch button").count()) === 3);

// --- 齿轮取代了原来的搜索按钮（需求 1）---
check("头部有齿轮按钮", (await page.locator(".diary-view .header-actions > button").count()) === 1);
await page.locator(".diary-view .header-actions > button").click();
await page.waitForTimeout(250);
const gearText = await page.locator(".diary-gear-panel").innerText();
check("齿轮面板里有搜索与三点菜单", gearText.includes("搜索日记") && gearText.includes("日记菜单"), gearText.replace(/\n/g, " | "));

// 齿轮 → 搜索
await page.locator(".diary-gear-panel .menu-item-button", { hasText: "搜索日记" }).click();
await page.waitForTimeout(300);
check("点搜索后面板收起、搜索框出现", (await page.locator(".diary-gear-panel").count()) === 0 && (await page.locator(".diary-search input").count()) === 1);

// --- 齿轮 → 日记菜单（UI 主题色 / 背景 / 导入导出）---
await page.locator(".diary-view .header-actions > button").click();
await page.waitForTimeout(250);
await page.locator(".diary-gear-panel .menu-item-button", { hasText: "日记菜单" }).click();
await page.waitForTimeout(350);
const menuText = await page.locator(".context-menu").first().innerText();
for (const item of ["导出全部日记", "按日期范围导出", "导入日记压缩包", "UI颜色", "背景颜色", "背景图片链接", "图片透明度"]) {
  check(`日记菜单含「${item}」`, menuText.includes(item));
}
for (const absent of ["重命名", "排序方式", "删除当前条目", "导出当前", "导入 JSON"]) {
  check(`日记菜单不含「${absent}」`, !menuText.includes(absent), menuText.replace(/\n/g, " | "));
}

// 主题色与背景真的作用到界面上
const accentBefore = await page.locator(".diary-view").evaluate((el) => getComputedStyle(el).getPropertyValue("--accent").trim());
await page.locator(".ui-color-picker input").fill("#b64a30");
await page.locator(".ui-color-picker input").dispatchEvent("input");
await page.waitForTimeout(400);
const accentAfter = await page.locator(".diary-view").evaluate((el) => getComputedStyle(el).getPropertyValue("--accent").trim());
check("改 UI 颜色会换日记主题色", accentAfter === "#b64a30", `${accentBefore} -> ${accentAfter}`);

await page.locator(".color-grid button").first().click();
await page.waitForTimeout(400);
const bgAfter = await page.locator(".diary-view").evaluate((el) => el.style.background);
check("改背景色会换日记背景", bgAfter.includes("rgb"), bgAfter);

// 关掉菜单
await page.keyboard.press("Escape");
await page.waitForTimeout(250);
await page.locator(".diary-view").click({ position: { x: 5, y: 300 } });
await page.waitForTimeout(300);

// --- 写一篇日记（心情/天气/标签/标题/正文）---
await page.locator(".diary-fab").click();
await page.waitForTimeout(800);
check("编辑器打开", await page.locator(".editor-dialog.diary-editor").isVisible());
await page.locator(".editor-title-input").fill("第一篇日记");
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.type("今天把日记拆成独立文件了。\n第二行用来验证展开。");
await page.locator(".editor-meta-trigger", { hasText: "心情" }).click();
await page.waitForTimeout(250);
await page.locator(".editor-emoji-grid-pop .emoji-pick-cell").first().click();
await page.waitForTimeout(250);
check("选完心情浮层收起", (await page.locator(".editor-emoji-grid-pop").count()) === 0);

// 需求 6：浮层在点别处时自动隐藏
await page.locator(".editor-meta-trigger", { hasText: "天气" }).click();
await page.waitForTimeout(250);
check("天气浮层打开", (await page.locator(".editor-emoji-grid-pop").count()) === 1);
await page.locator(".editor-header .editor-title").click();
await page.waitForTimeout(300);
check("点浮层以外的地方后天气浮层自动隐藏", (await page.locator(".editor-emoji-grid-pop").count()) === 0);
await page.locator(".editor-meta-trigger", { hasText: "天气" }).click();
await page.waitForTimeout(250);
await page.locator(".editor-emoji-grid-pop .emoji-pick-cell").first().click();
await page.waitForTimeout(200);

// 标签
await page.locator(".editor-tag-add").click();
await page.waitForTimeout(250);
await page.locator(".editor-tag-pop input").fill("工作");
await page.locator(".editor-tag-pop .tag-add-btn").click();
await page.waitForTimeout(250);
check("标签加进元数据行", (await page.locator(".editor-meta-tags .task-tag").count()) === 1);
// 点浮层外面 → 标签浮层也收起
await page.locator(".editor-cm-host .cm-content").click();
await page.waitForTimeout(300);
check("点正文后标签浮层自动隐藏", (await page.locator(".editor-tag-pop").count()) === 0);

await page.keyboard.press("Escape");
await page.waitForTimeout(700);
check("保存后编辑器关闭", (await page.locator(".editor-dialog.diary-editor").count()) === 0);
check("日记卡片出现", (await page.locator(".diary-card").count()) === 1);
const cardText = await page.locator(".diary-card").first().innerText();
check("卡片显示标题", cardText.includes("第一篇日记"), cardText.replace(/\n/g, " | "));
check("卡片显示心情与天气", (await page.locator(".diary-card .diary-meta-chip").count()) >= 2);
check("卡片显示标签", (await page.locator(".diary-card .task-tag").count()) === 1);

// 空草稿不落盘
await page.locator(".diary-fab").click();
await page.waitForTimeout(700);
await page.keyboard.press("Escape");
await page.waitForTimeout(400);
check("空草稿不留垃圾", (await page.locator(".diary-card").count()) === 1);

// --- 搜索（齿轮里）---
await page.locator(".diary-view .header-actions > button").click();
await page.waitForTimeout(250);
// 前面测齿轮时已经开过一次搜索，这里先关掉，保证走的是「打开」这条路径
if (await page.locator(".diary-gear-panel .menu-item-button", { hasText: "关闭搜索" }).count()) {
  await page.locator(".diary-gear-panel .menu-item-button", { hasText: "关闭搜索" }).click();
  await page.waitForTimeout(300);
  check("关闭搜索后搜索框消失", (await page.locator(".diary-search").count()) === 0);
  await page.locator(".diary-view .header-actions > button").click();
  await page.waitForTimeout(250);
}
await page.locator(".diary-gear-panel .menu-item-button", { hasText: "搜索日记" }).click();
await page.waitForTimeout(300);
await page.locator(".diary-search input").fill("独立文件");
await page.waitForTimeout(350);
check("日记内搜索命中", (await page.locator(".diary-card").count()) === 1);
await page.locator(".diary-search input").fill("找不到的词");
await page.waitForTimeout(350);
check("日记内搜索能筛空", (await page.locator(".diary-card").count()) === 0);
await page.locator(".diary-view .header-actions > button").click();
await page.waitForTimeout(250);
await page.locator(".diary-gear-panel .menu-item-button", { hasText: "关闭搜索" }).click();
await page.waitForTimeout(350);
check("关闭搜索后列表恢复", (await page.locator(".diary-card").count()) === 1);

// --- 日历 / 分组视图与持久化 ---
await page.locator(".diary-view-switch button").nth(1).click();
await page.waitForTimeout(350);
check("日历视图渲染", (await page.locator(".diary-calendar-cell").count()) >= 28);
check("有日记的那天被标记", (await page.locator(".diary-calendar-cell.has-entry").count()) === 1);
check("日历格里显示心情", (await page.locator(".diary-cell-mood").count()) === 1);
await page.locator(".diary-calendar-bar > button").first().click();
await page.waitForTimeout(300);
check("翻到上个月后出现「今」按钮", (await page.locator(".diary-fab-today").count()) === 1);
await page.locator(".diary-fab-today").click();
await page.waitForTimeout(300);
check("「今」跳回今天", (await page.locator(".diary-calendar-cell.today.selected").count()) === 1);

await page.locator(".diary-view-switch button").nth(2).click();
await page.waitForTimeout(350);
check("分组视图有年/月标题", (await page.locator(".diary-group-head").count()) >= 1 && (await page.locator(".diary-group-subhead").count()) >= 1);
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(700);
await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await page.waitForTimeout(350);
check("视图选择持久化（仍是分组）", (await page.locator(".diary-group-head").count()) >= 1);
const accentKept = await page.locator(".diary-view").evaluate((el) => getComputedStyle(el).getPropertyValue("--accent").trim());
check("主题色持久化", accentKept === "#b64a30", accentKept);
await page.locator(".diary-view-switch button").nth(0).click();
await page.waitForTimeout(300);

// --- 全局搜索混排（需求 5）---
await page.locator(".tree-row").first().click();
await page.waitForTimeout(400);
await page.locator(".add-task-bar textarea").fill("全局搜索用的日记任务");
await page.locator(".add-task-bar textarea").press("Enter");
await page.waitForTimeout(500);
check("任务已添加", (await page.locator(".task-card").count()) >= 1);

await page.locator(".search-box input").fill("日记");
await page.waitForTimeout(500);
const hitKinds = await page.evaluate(() => ({
  tasks: document.querySelectorAll(".workspace .task-card").length,
  diaries: document.querySelectorAll(".workspace .diary-card").length
}));
check("全局搜索同时命中任务与日记", hitKinds.tasks >= 1 && hitKinds.diaries >= 1, JSON.stringify(hitKinds));
check("搜索结果里两种卡片混排在同一个列表", (await page.locator(".workspace .task-list > *").count()) >= 2);
await page.locator(".search-box input").fill("");
await page.waitForTimeout(400);

// --- 输入框加号 = 用编辑器新建（需求 10）---
const tasksBefore = await page.locator(".task-card").count();
await page.locator(".composer-plus").click();
await page.waitForTimeout(900);
check("加号唤起任务编辑器", await page.locator(".editor-dialog").first().isVisible());
check("任务编辑器有元数据行", (await page.locator(".editor-meta .editor-meta-trigger").count()) >= 2);
check("日期默认是空的（不设默认值）", (await page.locator(".editor-meta-trigger", { hasText: "添加日期" }).count()) === 1);
// 设个日期 + 表情 + 标签
await page.locator(".editor-meta-trigger", { hasText: "添加日期" }).click();
await page.waitForTimeout(300);
await page.locator(".date-picker .dp-cell.today").click();
await page.waitForTimeout(300);
check("选完日期浮层收起", (await page.locator(".editor-meta .date-picker").count()) === 0);
await page.locator(".editor-cm-host .cm-content").click();
await page.keyboard.type("用编辑器新建的事项");
await page.keyboard.press("Escape");
await page.waitForTimeout(800);
check("新建的任务落进列表", (await page.locator(".task-card").count()) === tasksBefore + 1);
const createdCard = page.locator(".task-card", { hasText: "用编辑器新建的事项" }).first();
check("新建的事项能按内容找到", (await createdCard.count()) === 1);
const newCard = await createdCard.innerText();
check("新建时设的日期显示在卡片上", /\d+月\d+日/.test(newCard), newCard.replace(/\n/g, " | "));

// 空正文关掉 → 不创建
const tasksAfter = await page.locator(".task-card").count();
await page.locator(".composer-plus").click();
await page.waitForTimeout(800);
await page.keyboard.press("Escape");
await page.waitForTimeout(500);
check("空草稿新建不留空任务", (await page.locator(".task-card").count()) === tasksAfter);

// --- 已有任务的编辑器也带元数据行（需求 9.4）---
const targetCard = page.locator(".task-card", { hasText: "用编辑器新建的事项" }).first();
await targetCard.hover();
await targetCard.locator(".edit-button").click({ force: true });
await page.waitForTimeout(900);
check("编辑已有任务时也有元数据行", (await page.locator(".editor-meta .editor-meta-trigger").count()) >= 2);
await page.locator(".editor-tag-add").click();
await page.waitForTimeout(250);
await page.locator(".editor-tag-pop input").fill("编辑器加的标签");
await page.locator(".editor-tag-pop .tag-add-btn").click();
await page.waitForTimeout(200);
check("标签浮层留着不关（可以连着加几个）", (await page.locator(".editor-tag-pop").count()) === 1);
// 第一次 Esc 只收浮层（两段式关闭），第二次才保存并关编辑器
await page.keyboard.press("Escape");
await page.waitForTimeout(250);
check("第一次 Esc 只收起标签浮层", (await page.locator(".editor-tag-pop").count()) === 0 && (await page.locator(".editor-dialog").count()) === 1);
await page.keyboard.press("Escape");
await page.waitForTimeout(700);
check("编辑器里加的标签落到了卡片上", (await targetCard.innerText()).includes("编辑器加的标签"));

check("桌面全程无 pageerror", pageErrors.length === 0, pageErrors.join(" / "));
await ctx.close();

// ---------- mobile ----------
const mctx = await browser.newContext({
  viewport: { width: 390, height: 844 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true
});
const mpage = await mctx.newPage();
const mErrors = [];
mpage.on("pageerror", (e) => mErrors.push(String(e)));
await mpage.goto(URL, { waitUntil: "networkidle" });
await mpage.waitForTimeout(800);

const mNav = await mpage.locator(".system-nav .nav-row").allInnerTexts();
check(
  "移动端导航顺序：收藏 < 日记 < 记账 < 工具箱",
  mNav.findIndex((t) => t.includes("收藏")) < mNav.findIndex((t) => t.includes("日记")) &&
    mNav.findIndex((t) => t.includes("日记")) === mNav.findIndex((t) => t.includes("记账")) - 1 &&
    mNav.findIndex((t) => t.includes("记账")) === mNav.findIndex((t) => t.includes("工具箱")) - 1,
  JSON.stringify(mNav)
);

// 需求 8：移动端全局搜索要有结果面板
await mpage.locator(".add-task-bar").waitFor({ state: "detached" }).catch(() => {});
await mpage.locator(".tree-row").first().click();
await mpage.waitForTimeout(500);
await mpage.locator(".add-task-bar textarea").fill("手机上搜得到的任务");
await mpage.locator(".add-task-bar textarea").press("Enter");
await mpage.waitForTimeout(600);
await mpage.goBack();
await mpage.waitForTimeout(500);
check("回到列表视图", (await mpage.locator(".app-shell.mobile.view-list").count()) === 1);

await mpage.locator(".search-box input").fill("搜得到");
await mpage.waitForTimeout(600);
check("移动端出现搜索结果面板", await mpage.locator(".sidebar .search-results").isVisible());
check("移动端搜索面板里有卡片", (await mpage.locator(".sidebar .search-results .task-card").count()) >= 1);
const panelBox = await mpage.locator(".sidebar .search-results").boundingBox();
check("结果面板占了大半屏", panelBox && panelBox.height > 400, JSON.stringify(panelBox));
check("搜索态下导航与树被收起", !(await mpage.locator(".sidebar.searching .custom-nav").isVisible()));

// 结果卡片可以点开编辑器
await mpage.locator(".sidebar .search-results .task-card").first().click();
await mpage.waitForTimeout(600);
check("移动端结果卡片单击不打开编辑器", (await mpage.locator(".editor-dialog").count()) === 0);
await mpage.locator(".sidebar .search-results .task-card").first().dblclick();
await mpage.waitForTimeout(900);
check("移动端结果卡片双击进编辑器", (await mpage.locator(".editor-dialog").count()) === 1);
await mpage.keyboard.press("Escape");
await mpage.waitForTimeout(600);

// 搜索也能搜到日记
await mpage.locator(".sidebar .search-box input").fill("日记");
await mpage.waitForTimeout(400);
await mpage.locator(".system-nav .nav-row", { hasText: "日记" }).click({ force: true }).catch(() => {});
await mpage.waitForTimeout(400);
await mpage.locator(".search-box input").fill("");
await mpage.waitForTimeout(400);

// 日记层
await mpage.locator(".system-nav .nav-row", { hasText: "日记" }).click();
await mpage.waitForTimeout(500);
check("移动端日记整页层", (await mpage.locator(".app-shell.mobile.view-diary").count()) === 1);
check("移动端日记也有齿轮", (await mpage.locator(".diary-view .header-actions > button").count()) === 1);
await mpage.locator(".diary-fab").click();
await mpage.waitForTimeout(900);
await mpage.locator(".editor-title-input").fill("移动端的一篇");
await mpage.locator(".editor-cm-host .cm-content").click();
await mpage.keyboard.type("在手机上写的。");
await mpage.keyboard.press("Escape");
await mpage.waitForTimeout(800);
check("移动端能写日记", (await mpage.locator(".diary-card").count()) >= 1);
check("移动端卡片没有笔形按钮", (await mpage.locator(".diary-edit-button").count()) === 0 || !(await mpage.locator(".diary-edit-button").first().isVisible()));
const highlight = await mpage.locator(".diary-card").first().evaluate((el) => getComputedStyle(el).webkitTapHighlightColor);
check("移动端日记卡片去掉了点按蓝罩", highlight === "rgba(0, 0, 0, 0)" || highlight === "transparent", highlight);
await mpage.goBack();
await mpage.waitForTimeout(500);
check("移动端返回键回列表", (await mpage.locator(".app-shell.mobile.view-list").count()) === 1);
check("移动端全程无 pageerror", mErrors.length === 0, mErrors.join(" / "));

await mctx.close();
await browser.close();

console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);

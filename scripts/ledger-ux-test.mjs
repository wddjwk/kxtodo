// 记账回归（v0.7.1 重写版）：桌面 + 移动模拟。覆盖侧栏入口、四视图、按天卡片、
// 记一笔（桌面输入框 / 移动端数字键盘）、齿轮面板与「记账菜单」三点菜单（唤不出的回归）、
// 日历数额格、统计图表、资产与账户/分类管理、转账、列表视图的无限月份翻页、
// 设置页同步五勾选框、移动端底部抽屉与返回键。
// 需先 npm run dev（浏览器预览走 localStorage legacy 路径；core 命令层与 Excel 导入导出
// 由 cargo test 的 cli_ledger 覆盖）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const LEDGER_KEY = "todo-note-ledger-v1";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${ok || !extra ? "" : ` — ${extra}`}`);
  if (!ok) failures += 1;
}

/** 打开应用并清干净本地数据（首屏会种下默认的账户与分类）。 */
async function freshPage(context) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load" });
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });
  return { page, errors };
}

async function openLedger(page) {
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
}

async function switchView(page, label) {
  await page.click(`.ledger-view-switch button[title='${label}']`);
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------- 桌面 ----------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);

  // 侧栏入口：记账紧跟在日记下面
  const navLabels = await page.$$eval(".system-nav .nav-row .list-name", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  const diaryIndex = navLabels.indexOf("日记");
  const ledgerIndex = navLabels.indexOf("记账");
  check("侧栏含日记与记账行", diaryIndex >= 0 && ledgerIndex >= 0, navLabels.join(","));
  check("记账排在日记下面", ledgerIndex === diaryIndex + 1, `${diaryIndex}/${ledgerIndex}`);

  await openLedger(page);
  check("记账视图打开", (await page.$$(".ledger-view")).length === 1);
  check("工作区被隐藏", await page.$eval(".workspace", (el) => getComputedStyle(el).display === "none"));
  const ledgerTitleSize = await page.$eval(".ledger-view h1", (el) => getComputedStyle(el).fontSize);
  const workspaceTitleSize = await page.$eval(".workspace h1", (el) => getComputedStyle(el).fontSize);
  check("标题字号与其它页面一致", ledgerTitleSize === workspaceTitleSize, `${ledgerTitleSize} vs ${workspaceTitleSize}`);

  // 记一笔：桌面走输入框（键盘只在触屏渲染）
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  check("记账面板打开", (await page.$$(".ledger-sheet")).length === 1);
  check("桌面无数字键盘", (await page.$$(".ledger-keypad")).length === 0);
  check("大类 chips 渲染", (await page.$$(".ledger-parent-chip")).length >= 5);
  await page.click(".ledger-parent-chip >> nth=0");
  await page.waitForSelector(".ledger-cat-tile", { timeout: 5000 });
  const tileCount = (await page.$$(".ledger-cat-tile")).length;
  check("子分类图标格子渲染", tileCount >= 2, String(tileCount));
  await page.click(".ledger-cat-tile >> nth=0");
  const picked = await page.textContent(".ledger-amount-cat em");
  check("选中分类回显在金额行", Boolean(picked && picked !== "选择分类"), picked ?? "");
  await page.fill(".ledger-amount-field input", "30.50");
  await page.click(".ledger-sheet-foot button:has-text('记一笔')");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });

  // 列表视图：一天一张卡片，卡片里是当天每一笔
  await page.waitForSelector(".ledger-card", { timeout: 8000 });
  check("按天卡片出现", (await page.$$(".ledger-card")).length === 1);
  check("卡片里有日期栏", (await page.$$(".ledger-card .ledger-date-day")).length === 1);
  const cardTitleSize = await page.$eval(".ledger-card-title", (el) => getComputedStyle(el).fontSize);
  const entryAmountSize = await page.$eval(".ledger-entry-amount", (el) => getComputedStyle(el).fontSize);
  check(
    "卡片字号跟全局变量走（18px 基准上下）",
    Math.abs(parseFloat(cardTitleSize) - 19) < 0.6 && Math.abs(parseFloat(entryAmountSize) - 19) < 0.6,
    `${cardTitleSize}/${entryAmountSize}`
  );
  const entryAmount = await page.textContent(".ledger-entry-amount");
  check("支出金额带负号", entryAmount?.trim() === "-30.50", entryAmount ?? "");
  const daySum = await page.textContent(".ledger-card-sums");
  check("卡片头给出当天合计", daySum?.includes("30.5") ?? false, daySum ?? "");

  // 齿轮面板 → 记账菜单（三点菜单唤不出就是这条链断在冒泡上）
  await page.click(".ledger-view .header-actions > button[title='更多操作']");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  check("齿轮面板打开", (await page.$$(".ledger-gear-panel .menu-item-button")).length === 3);
  await page.click(".ledger-gear-panel .menu-item-button:has-text('记账菜单')");
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  const listMenuText = await page.textContent(".context-menu");
  check("记账菜单唤得出", (listMenuText ?? "").includes("导出全部账本"), (listMenuText ?? "").slice(0, 60));
  check("记账菜单带背景配色区", (listMenuText ?? "").includes("背景颜色"));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 5000 });

  // 齿轮面板 → 分类管理
  await page.click(".ledger-view .header-actions > button[title='更多操作']");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.click(".ledger-gear-panel .menu-item-button:has-text('分类管理')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("分类管理打开", (await page.$$(".ledger-cat-group")).length >= 8);
  await page.click(".ledger-manager .ledger-cat-chip.add >> nth=0");
  await page.waitForSelector(".ledger-icon-grid", { timeout: 5000 });
  check("添加分类表单带图标网格", (await page.$$(".ledger-icon-cell")).length >= 40);
  check("添加分类表单带颜色圆点", (await page.$$(".ledger-color-dot")).length >= 8);
  await page.fill(".ledger-field-row input[placeholder='例如 早餐']", "夜宵");
  await page.click(".ledger-icon-cell >> nth=3");
  await page.click(".ledger-sheet-foot button:has-text('保存')");
  await page.waitForSelector(".ledger-icon-grid", { state: "detached", timeout: 8000 });
  const catText = await page.textContent(".ledger-manager .ledger-sheet-body");
  check("新分类进列表", (catText ?? "").includes("夜宵"), (catText ?? "").slice(-60));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 卡片里的一笔：右键菜单
  await page.click(".ledger-entry", { button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  check("单笔右键出菜单", (await page.$$(".context-menu .menu-item-button")).length >= 3);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 5000 });

  // 日历视图：格子里是数额，不是热力块
  await switchView(page, "日历视图");
  await page.waitForSelector(".ledger-calendar-grid", { timeout: 8000 });
  const cells = await page.$$(".ledger-calendar-cell");
  check("日历 42 或 35 格", cells.length === 42 || cells.length === 35, String(cells.length));
  const amountCells = await page.$$(".ledger-cell-amounts em");
  check("日历格显示数额", amountCells.length >= 1, String(amountCells.length));
  const cellAmount = await page.textContent(".ledger-cell-amounts em");
  check("日历数额是当天支出", cellAmount?.trim() === "30.5", cellAmount ?? "");
  check("日历下方列出当天卡片", (await page.$$(".ledger-day-head + .ledger-card")).length === 1);

  // 统计视图
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-line-chart", { timeout: 8000 });
  check("曲线渲染", (await page.$$(".ledger-line.out")).length === 1);
  check("占比环渲染", (await page.$$(".ledger-donut circle")).length >= 2);
  check("排行渲染", (await page.$$(".ledger-rank > li")).length >= 1);
  check("排行带比例条", (await page.$$(".ledger-rank-track i")).length >= 1);
  const summary = await page.textContent(".ledger-summary");
  check("汇总卡给出本期支出", summary?.includes("30.50") ?? false, (summary ?? "").replace(/\s+/g, " ").slice(0, 60));

  // 资产视图 + 账户管理
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-net-card", { timeout: 8000 });
  const net = await page.textContent(".ledger-net-value");
  check("净资产反映这笔支出", net?.trim() === "-30.50", net ?? "");
  check("种子账户在列", (await page.$$(".ledger-account-row")).length === 4);

  await page.click(".ledger-panel-head .ledger-chip-button:has-text('添加')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  const managerTitle = await page.textContent(".ledger-sheet-title");
  check("添加账户表单打开", managerTitle?.trim() === "添加账户", managerTitle ?? "");
  await page.fill(".ledger-field-row input[placeholder='例如 招商银行']", "零钱罐");
  await page.click(".ledger-choice:has-text('投资')");
  await page.fill(".ledger-field-row input[placeholder='0.00']", "200");
  await page.click(".ledger-sheet-foot button:has-text('保存')");
  await page.waitForSelector(".ledger-field-row", { state: "detached", timeout: 8000 });
  const accountListText = await page.textContent(".ledger-manager .ledger-sheet-body");
  check("保存后回到账户列表并含新账户", (accountListText ?? "").includes("零钱罐"), (accountListText ?? "").slice(0, 60));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  const assetText = await page.textContent(".ledger-view");
  check("新账户进资产视图", assetText?.includes("零钱罐") ?? false);
  check("净资产加上新账户期初", (await page.textContent(".ledger-net-value"))?.trim() === "169.50");

  // 转账
  await page.click(".ledger-panel-head .ledger-chip-button:has-text('转账')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  const transferTitle = await page.textContent(".ledger-sheet-title");
  check("转账面板打开", transferTitle?.trim() === "账户转账", transferTitle ?? "");
  await page.fill(".ledger-manager input[placeholder='0.00']", "12.34");
  await page.click(".ledger-sheet-foot button:has-text('确认转账')");
  await page.waitForSelector(".ledger-field-row", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  await switchView(page, "列表视图");
  await page.waitForSelector(".ledger-entry:has-text('转账')", { timeout: 8000 });
  check("转账进列表且标成转账", true);
  const transferAmount = await page.textContent(".ledger-entry:has-text('转账') .ledger-entry-amount");
  check("转账金额不带正负号", transferAmount?.trim() === "12.34", transferAmount ?? "");

  // 设置页同步内容：五个勾选框（数据/设置/任务/日记/账本）
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  const scopes = await page.$$eval(".sync-scope", (els) => els.map((el) => el.textContent?.trim() ?? ""));
  check(
    "同步内容含日记与账本勾选框",
    scopes.length === 5 && scopes.includes("日记") && scopes.includes("账本"),
    scopes.join(",")
  );
  await page.locator("button.settings-backdrop").click();
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });

  check("桌面无 pageerror", errors.length === 0, errors.join(" | "));
  await context.close();
}

// ---------- 桌面：列表视图的无限月份 ----------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 620 } });
  const { page, errors } = await freshPage(context);

  // 直接往 legacy 存储里塞 70 天的账，滚到底应当接上上个月（UI 逐笔记太慢）
  await page.evaluate((key) => {
    const book = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!book || !book.accounts?.length) return;
    const account = book.accounts[0].id;
    const category = book.categories.find((item) => item.side === "expense" && item.parentId)?.id ?? "";
    const entries = [];
    for (let index = 1; index <= 70; index += 1) {
      const stamp = new Date(Date.now() - index * 86_400_000);
      const date = `${stamp.getFullYear()}-${String(stamp.getMonth() + 1).padStart(2, "0")}-${String(stamp.getDate()).padStart(2, "0")}`;
      entries.push({
        id: `ledger-test-${index}`,
        kind: "expense",
        amountCents: 1000 + index,
        accountId: account,
        categoryId: category,
        date,
        time: "",
        note: `测试 ${index}`,
        createdAt: stamp.toISOString()
      });
    }
    book.entries = entries;
    localStorage.setItem(key, JSON.stringify(book));
  }, LEDGER_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  const cardsBefore = (await page.$$(".ledger-card")).length;
  check("首屏只渲染当前月", cardsBefore >= 1 && cardsBefore <= 31, String(cardsBefore));
  check("首屏不画月份分隔行", (await page.$$(".ledger-month-divider")).length === 0);

  await page.evaluate(() => {
    const el = document.querySelector(".ledger-scroll");
    if (el) el.scrollTop = el.scrollHeight;
  });
  await page.waitForSelector(".ledger-month-divider", { timeout: 8000 });
  const cardsAfter = (await page.$$(".ledger-card")).length;
  check("滚到底接上上个月", cardsAfter > cardsBefore, `${cardsBefore} → ${cardsAfter}`);
  const divider = await page.textContent(".ledger-month-divider");
  check("月份分隔行带收支持", divider?.includes("支") ?? false, (divider ?? "").replace(/\s+/g, " "));

  // 月份导航：跳到上个月，卡片跟着换
  const monthBefore = await page.textContent(".ledger-month-step strong");
  await page.click(".ledger-month-step button >> nth=0");
  await page.waitForTimeout(200);
  const monthAfter = await page.textContent(".ledger-month-step strong");
  check("月份导航换月", monthBefore !== monthAfter, `${monthBefore} → ${monthAfter}`);
  check("换月后分隔行归零", (await page.$$(".ledger-month-divider")).length === 0);

  check("翻页无 pageerror", errors.length === 0, errors.join(" | "));
  await context.close();
}

// ---------- 移动 ----------
{
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const { page, errors } = await freshPage(context);

  await openLedger(page);
  check("移动端记账整页", await page.$eval(".app-shell", (el) => el.classList.contains("view-ledger")));
  check("移动端侧栏隐藏", await page.$eval(".sidebar", (el) => getComputedStyle(el).display === "none"));

  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  const sheet = await page.$eval(".ledger-sheet", (el) => {
    const rect = el.getBoundingClientRect();
    return { top: rect.top, bottom: rect.bottom, width: rect.width };
  });
  const viewport = page.viewportSize();
  check("移动端抽屉贴底", viewport !== null && viewport.height - sheet.bottom < 4, JSON.stringify(sheet));
  check("移动端抽屉铺满宽度", viewport !== null && Math.abs(sheet.width - viewport.width) < 2, String(sheet.width));
  check("移动端有数字键盘", (await page.$$(".ledger-keypad .ledger-key")).length === 14);
  check("移动端金额只读展示", (await page.$$(".ledger-amount-value")).length === 1);

  await page.click(".ledger-parent-chip >> nth=0");
  await page.waitForSelector(".ledger-cat-tile", { timeout: 5000 });
  await page.click(".ledger-cat-tile >> nth=0");
  for (const key of ["1", "2", ".", "3", "4"]) {
    await page.click(`.ledger-key:text-is("${key}")`);
  }
  const shown = await page.textContent(".ledger-amount-value");
  check("键盘累计金额正确", shown?.includes("12.34"), shown ?? "");
  await page.click(".ledger-key.save");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });
  await page.waitForSelector(".ledger-entry", { timeout: 8000 });
  check("移动端记一笔进卡片", (await page.textContent(".ledger-entry-amount"))?.trim() === "-12.34");

  // 分类管理在窄屏上也要能用（底部抽屉 + 表单子层）
  await page.click(".ledger-view .header-actions > button[title='更多操作']");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.click(".ledger-gear-panel .menu-item-button:has-text('账户与转账')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("移动端账户管理打开", (await page.$$(".ledger-account-row")).length >= 4);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 返回键回列表
  await page.goBack();
  await page.waitForSelector(".app-shell.view-list", { timeout: 8000 });
  check("移动端返回键回列表", (await page.$$(".app-shell.view-list")).length === 1);

  check("移动端无 pageerror", errors.length === 0, errors.join(" | "));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "LEDGER UX ALL PASS" : `${failures} FAILURE(S)`);
process.exit(failures ? 1 : 0);

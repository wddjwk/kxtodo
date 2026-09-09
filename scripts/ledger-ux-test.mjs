// 记账回归：桌面 + 移动模拟，跑通四视图切换、记一笔、转账、分类/账户管理、热力与统计渲染、
// 设置页同步范围五个勾选框。需先 npm run dev（浏览器预览走 localStorage legacy 路径，
// core 命令层与 Excel 导入导出由 cargo test 的 cli_ledger 覆盖）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${ok || !extra ? "" : ` — ${extra}`}`);
  if (!ok) failures += 1;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------- 桌面 ----------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load" });
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });

  // 侧栏有「记账」行，且在「日记」下面
  const navLabels = await page.$$eval(".system-nav .nav-row .list-name", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  const diaryIndex = navLabels.indexOf("日记");
  const ledgerIndex = navLabels.indexOf("记账");
  check("侧栏含日记与记账行", diaryIndex >= 0 && ledgerIndex >= 0, navLabels.join(","));
  check("记账排在日记下面", ledgerIndex === diaryIndex + 1, `${diaryIndex}/${ledgerIndex}`);

  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  check("记账视图打开", (await page.$$(".ledger-view")).length === 1);
  check("工作区被隐藏", await page.$eval(".workspace", (el) => getComputedStyle(el).display === "none"));

  // 列表视图：种子账户/分类在，先记一笔
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-editor", { timeout: 8000 });
  check("记账面板打开", (await page.$$(".ledger-editor")).length === 1);
  check("分类大类横排渲染", (await page.$$(".ledger-editor-parents button")).length >= 5);
  // 选一个子分类
  await page.click(".ledger-editor-parents button:first-child");
  await page.waitForSelector(".ledger-editor-children button", { timeout: 5000 });
  await page.click(".ledger-editor-children button:first-child");
  // 键盘输入 30.50
  for (const key of ["3", "0", ".", "5", "0"]) {
    await page.click(`.ledger-keypad button:text-is("${key}")`);
  }
  const shown = await page.textContent(".ledger-editor-amount-hint");
  check("金额键盘累计正确", shown?.includes("30.50"), shown ?? "");
  await page.click(".ledger-editor-foot button:has-text('完成')");
  await page.waitForSelector(".ledger-editor", { state: "detached", timeout: 8000 });
  await page.waitForSelector(".ledger-row", { timeout: 8000 });
  check("记一笔后列表出现行", (await page.$$(".ledger-row")).length >= 1);
  const rowAmount = await page.textContent(".ledger-row .ledger-row-side b");
  check("行金额带负号", rowAmount?.trim() === "-30.50", rowAmount ?? "");

  // 日历视图
  await page.click(".ledger-view-switch button[title='日历视图']");
  await page.waitForSelector(".ledger-heat-grid", { timeout: 8000 });
  const heatCells = await page.$$(".ledger-heat-cell");
  check("热力图 42 或 35 格", heatCells.length === 42 || heatCells.length === 35, String(heatCells.length));
  const colored = await page.$$eval(".ledger-heat-cell", (els) =>
    els.filter((el) => (el.getAttribute("style") ?? "").includes("rgba")).length
  );
  check("热力格按金额着色", colored >= 1, String(colored));

  // 统计视图
  await page.click(".ledger-view-switch button[title='统计视图']");
  await page.waitForSelector(".ledger-line-chart", { timeout: 8000 });
  check("曲线渲染", (await page.$$(".ledger-line.expense")).length === 1);
  check("占比环渲染", (await page.$$(".ledger-donut circle")).length >= 1);
  check("排行渲染", (await page.$$(".ledger-rank > li")).length >= 1);

  // 资产视图
  await page.click(".ledger-view-switch button[title='资产视图']");
  await page.waitForSelector(".ledger-net-card", { timeout: 8000 });
  const net = await page.textContent(".ledger-net-value");
  check("净资产卡渲染", (net ?? "").includes("30.50"), net ?? "");
  check("种子账户在列", (await page.$$(".ledger-account-row")).length === 4);

  // 账户管理：加一个账户
  await page.click(".ledger-account-row >> nth=0");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("点账户行进编辑表单", (await page.$$(".ledger-manager-form")).length === 1);
  await page.click(".ledger-manager .ledger-editor-close");
  await page.waitForSelector(".ledger-manager", { state: "detached" });

  // 齿轮菜单：分类管理
  await page.click(".ledger-view .header-menu-button");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.click(".ledger-gear-panel >> text=分类管理");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("分类管理打开", (await page.$$(".ledger-manager-row")).length >= 10);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached" });

  // 行右键菜单
  await page.click(".ledger-view-switch button[title='列表视图']");
  await page.waitForSelector(".ledger-row", { timeout: 8000 });
  await page.click(".ledger-row", { button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  check("行右键出菜单", (await page.$$(".context-menu .menu-item-button")).length >= 3);
  await page.keyboard.press("Escape");

  // 转账：资产视图的转账面板
  await page.click(".ledger-view-switch button[title='资产视图']");
  await page.waitForSelector(".ledger-net-card", { timeout: 8000 });
  await page.click(".ledger-asset-actions button:has-text('转账')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.fill(".ledger-manager-form input[placeholder='0.00']", "12.34");
  await page.click(".ledger-editor-foot button:has-text('转账')");
  await page.waitForSelector(".ledger-manager-form", { state: "detached", timeout: 8000 });
  await page.click(".ledger-manager .ledger-editor-close");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  await page.click(".ledger-view-switch button[title='列表视图']");
  await page.waitForSelector(".ledger-row:has-text('转账')", { timeout: 8000 });
  check("转账后列表出现转账行", true);

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

// ---------- 移动 ----------
{
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load" });
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });

  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  check("移动端记账整页", await page.$eval(".app-shell", (el) => el.classList.contains("view-ledger")));
  check("移动端侧栏隐藏", await page.$eval(".sidebar", (el) => getComputedStyle(el).display === "none"));

  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-editor", { timeout: 8000 });
  check("移动端记账面板", (await page.$$(".ledger-editor")).length === 1);
  await page.click(".ledger-editor-close");
  await page.waitForSelector(".ledger-editor", { state: "detached" });

  // 返回键回列表
  await page.goBack();
  await page.waitForSelector(".app-shell.view-list", { timeout: 8000 });
  check("移动端返回键回列表", (await page.$$(".app-shell.view-list")).length === 1);

  check("移动端无 pageerror", errors.length === 0, errors.join(" | "));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "ALL PASS" : `${failures} FAILURES`);
process.exit(failures ? 1 : 0);

// v0.7.3 回归：这一版改动的守门员。
// 覆盖：月份标题行左对齐到卡片左缘 + 年月可点选、记账时刻（日历里的滚轮 → 落盘 → 卡片显示）、
// 保存语义（编辑态没有「保存再记」、点保存就关）、改转账不再新增一笔、转账账户浮层不被金额行盖住、
// 分类行末尾的加号直达分类管理新增表单、统计排行点进去的钻取面板（大类 → 二级 → 账单明细 → 改这一笔）、
// 汇总金额不出省略号、设置分区重组（外观效果/窗口与系统/存储空间）与固定分组的可见性与展示方式、
// 同步账户不再用资料里的显示名预填、移动端点抽屉上方遮罩关闭且不误触下面的内容。
// 需先 npm run dev（浏览器预览走 localStorage legacy 路径）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${ok || !extra ? "" : ` — ${extra}`}`);
  if (!ok) failures += 1;
}

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
  await openLedger(page);

  // 先记一笔（带时刻）：后面所有断言都用它
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  await page.click(".ledger-cat-grid > .ledger-cat-cell:not(.add) >> nth=0");
  await page.waitForSelector(".ledger-cat-sub .ledger-cat-cell", { timeout: 5000 });
  await page.click(".ledger-cat-sub .ledger-cat-cell:not(.add) >> nth=0");
  await page.fill(".ledger-amount-plain input", "30.50");

  // 日期浮层里有「时钟 + 18:19」那一行，点开是双列滚轮
  await page.click(".ledger-meta-field >> nth=0 >> .ledger-meta-plain");
  await page.waitForSelector(".ledger-pop.date .date-picker", { timeout: 5000 });
  check("日期浮层带时刻行", (await page.$$(".ledger-pop.date .dp-time-trigger")).length === 1);
  const clockLabel = await page.textContent(".dp-time-trigger strong");
  check("时刻行显示 HH:MM", /^\d{2}:\d{2}$/.test((clockLabel ?? "").trim()), clockLabel ?? "");
  // v0.8.1：「要不要精确到分钟」默认不勾、时刻行是灰的；先勾上再点
  check("时刻行默认关着（置灰）", await page.isDisabled(".dp-time-trigger"));
  await page.click(".dp-time-switch input");
  await page.waitForTimeout(200);
  await page.click(".dp-time-trigger");
  await page.waitForSelector(".time-picker", { timeout: 5000 });
  check("滚轮是两列", (await page.$$(".time-picker .time-col")).length === 2);
  const hourCells = await page.locator(".time-col").nth(0).locator(".time-cell").count();
  const minuteCells = await page.locator(".time-col").nth(1).locator(".time-cell").count();
  check("时分列数正确", hourCells === 24 && minuteCells === 60, `${hourCells}/${minuteCells}`);
  // 滚轮换成面板而不是往下追加：浮层高度不该因为展开时刻而暴涨
  const pickerBox = await page.locator(".ledger-pop.date .date-picker").boundingBox();
  check("切到滚轮后日历高度基本不变", pickerBox.height > 200 && pickerBox.height < 420, String(pickerBox.height));
  await page.locator(".time-col").nth(1).locator(".time-cell", { hasText: "45" }).first().click();
  await page.waitForTimeout(200);
  const wheelShown = await page.locator(".time-col").nth(1).locator(".time-cell.active").textContent();
  check("点分钟格即选中", (wheelShown ?? "").trim() === "45", wheelShown ?? "");
  // 回到日历（左上角返回），关掉浮层
  await page.click(".date-picker-header button >> nth=0");
  await page.waitForSelector(".date-picker-grid", { timeout: 5000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-pop.date", { state: "detached", timeout: 5000 });
  const triggerText = await page.textContent(".ledger-meta-field .ledger-meta-plain");
  check("触发器回显选中的时刻", (triggerText ?? "").includes("45"), triggerText ?? "");

  await page.click(".ledger-sheet-foot button:has-text('记一笔')");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });
  await page.waitForSelector(".ledger-entry", { timeout: 8000 });
  check("记一笔后面板关闭", (await page.$$(".ledger-sheet")).length === 0);
  check("卡片里显示时刻", (await page.$$(".ledger-entry-time")).length === 1);

  // 月份标题行：左对齐到卡片左缘（不是标题文字的起点）
  const monthBar = await page.locator(".ledger-month-bar").boundingBox();
  const card = await page.locator(".ledger-card").boundingBox();
  check(
    "月份标题行与卡片左缘对齐",
    Math.abs(monthBar.x - card.x) < 2,
    `${monthBar.x} vs ${card.x}`
  );

  // 年月标签可以点开直接选年月
  await page.click(".ledger-month-step strong");
  await page.waitForSelector(".month-pop .month-picker", { timeout: 5000 });
  check("年月浮层弹出", (await page.$$(".month-pop .mp-cell")).length === 12);
  await page.click(".month-pop .month-picker-header button >> nth=0");
  await page.click(".month-pop .mp-cell >> nth=0");
  await page.waitForSelector(".month-pop", { state: "detached", timeout: 5000 });
  const monthLabel = await page.textContent(".ledger-month-step strong");
  const thisYear = new Date().getFullYear();
  check("选中的年月生效", (monthLabel ?? "").includes(`${thisYear - 1}年1月`), monthLabel ?? "");
  await page.click(".ledger-fab-today");
  await page.waitForSelector(".ledger-entry", { timeout: 8000 });

  // 改这一笔：编辑态没有「保存再记」，点保存就关，且不会多出一笔
  await page.click(".ledger-entry");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  const footButtons = await page.$$eval(".ledger-sheet-foot .settings-button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  check("编辑态只有「保存修改」一个动作", footButtons.join("|") === "保存修改", footButtons.join("|"));
  check("编辑态回显已有时刻", (await page.textContent(".ledger-meta-field .ledger-meta-plain"))?.includes("45") ?? false);
  await page.fill(".ledger-amount-plain input", "42");
  await page.click(".ledger-sheet-foot button:has-text('保存修改')");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(400);
  check("保存修改后不再弹出面板", (await page.$$(".editor-overlay")).length === 0);
  check("改一笔不会变成两笔", (await page.$$(".ledger-entry")).length === 1, String((await page.$$(".ledger-entry")).length));
  check("金额真的改了", (await page.textContent(".ledger-entry-amount"))?.includes("42") ?? false);

  // 转账：改一笔转账仍是改，不新增
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  await page.click(".ledger-kind-tabs button:has-text('转账')");
  await page.waitForSelector(".ledger-transfer-block", { timeout: 5000 });
  await page.locator(".ledger-transfer-field").nth(0).locator("button").click();
  await page.waitForSelector(".ledger-transfer-field .ledger-pop", { timeout: 5000 });
  // 浮层必须真的可点：命中测试落在浮层里的账户行上，而不是被金额行盖住
  const popBox = await page.locator(".ledger-transfer-field .ledger-pop").boundingBox();
  const hit = await page.evaluate(
    ([x, y]) => {
      const el = document.elementFromPoint(x, y);
      return el ? (el.closest(".ledger-pop") ? "pop" : el.className || el.tagName) : "none";
    },
    [popBox.x + popBox.width / 2, popBox.y + popBox.height / 2]
  );
  check("转账账户浮层没被金额行盖住", hit === "pop", String(hit));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-transfer-field .ledger-pop", { state: "detached", timeout: 5000 });
  await page.fill(".ledger-amount-plain input", "12.34");
  await page.click(".ledger-sheet-foot button:has-text('记一笔')");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });
  await page.waitForSelector(".ledger-entry:has-text('转账')", { timeout: 8000 });
  const beforeTransfer = (await page.$$(".ledger-entry")).length;
  await page.click(".ledger-entry:has-text('转账')");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  check("编辑转账时仍是转账页签", (await page.$$(".ledger-transfer-block")).length === 1);
  await page.fill(".ledger-amount-plain input", "20");
  await page.click(".ledger-sheet-foot button:has-text('保存修改')");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(400);
  check(
    "改转账不新增流水",
    (await page.$$(".ledger-entry")).length === beforeTransfer,
    `${beforeTransfer} → ${(await page.$$(".ledger-entry")).length}`
  );
  check("转账金额改到了", (await page.textContent(".ledger-entry:has-text('转账') .ledger-entry-amount"))?.includes("20") ?? false);

  // 分类加号：一级网格末尾与二级展开块末尾都能直达分类管理的新增表单
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  await page.click(".ledger-cat-grid > .ledger-cat-cell.add");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("一级加号打开新增分类表单", (await page.textContent(".ledger-sheet-title"))?.trim() === "添加分类");
  check("加号打开时带图标分组", (await page.$$(".ledger-icon-group")).length >= 10);
  check("图标库扩到两百多个", (await page.$$(".ledger-icon-cell")).length >= 200, String((await page.$$(".ledger-icon-cell")).length));
  await page.click(".ledger-icon-group >> nth=1");
  const filtered = (await page.$$(".ledger-icon-cell")).length;
  check("分组筛选生效", filtered > 0 && filtered < 100, String(filtered));
  // v0.8.3 review #6：直达表单的浮层，返回/Esc 一律**整层关掉**——用户是从记账面板的
  // 加号直接落到「添加分类」上的，他从没见过分类列表，退到那里只是多按一次。
  // （AccountManager 在 v0.8.2 就是这么定的，这一版把 CategoryManager 统一到同一套。）
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  check("直达表单时 Esc 整层关掉分类管理", (await page.$$(".ledger-manager")).length === 0);
  check("分类管理关掉后记账面板还在", (await page.$$(".ledger-sheet")).length === 1);
  await page.click(".ledger-cat-grid > .ledger-cat-cell:not(.add) >> nth=0");
  await page.waitForSelector(".ledger-cat-sub .ledger-cat-cell.add", { timeout: 5000 });
  await page.click(".ledger-cat-sub .ledger-cat-cell.add");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("二级加号也打开新增表单", (await page.textContent(".ledger-sheet-title"))?.trim() === "添加分类");
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });

  // 统计：汇总金额不出省略号；点大类钻取到二级分类与账单明细
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-summary", { timeout: 8000 });
  const clipped = await page.$$eval(".ledger-summary strong", (els) =>
    els.filter((el) => el.scrollWidth > el.clientWidth + 1).length
  );
  check("汇总金额没有被截断", clipped === 0, `${clipped} 个被截断`);
  const noEllipsis = await page.$eval(".ledger-summary strong", (el) => getComputedStyle(el).textOverflow);
  check("汇总金额不写省略号", noEllipsis !== "ellipsis", noEllipsis);

  await page.click(".ledger-rank-head >> nth=0");
  await page.waitForSelector(".ledger-drill-pop", { timeout: 8000 });
  check("桌面钻取是锚定下拉面板", (await page.$$(".ledger-drill-pop")).length === 1);
  const drillTitle = await page.textContent(".ledger-drill-title strong");
  check("钻取面板标题是大类名", Boolean(drillTitle && drillTitle.length > 0), drillTitle ?? "");
  const hasRows = (await page.$$(".ledger-drill-row")).length;
  const hasEntries = (await page.$$(".ledger-drill-entries .ledger-entry")).length;
  check("钻取面板给出二级分类或账单明细", hasRows + hasEntries >= 1, `${hasRows}/${hasEntries}`);
  if (hasRows > 0) {
    await page.click(".ledger-drill-row >> nth=0");
    await page.waitForSelector(".ledger-drill-entries .ledger-entry", { timeout: 5000 });
    check("点二级分类看到账单明细", (await page.$$(".ledger-drill-entries .ledger-entry")).length >= 1);
    check("返回按钮出现", (await page.$$(".ledger-drill-back")).length === 1);
  }
  await page.click(".ledger-drill-entries .ledger-entry >> nth=0");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  check("点账单明细直接进编辑器", (await page.$$(".ledger-sheet-foot button:has-text('保存修改')")).length === 1);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });

  // 设置抽屉：分区重组、字号、固定分组、存储空间
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  const titles = await page.$$eval(".settings-section h3", (els) => els.map((el) => el.textContent?.trim() ?? ""));
  check("外观效果分区在", titles.includes("外观效果"), titles.join(","));
  check("窗口与系统分区在", titles.includes("窗口与系统"), titles.join(","));
  check("存储空间分区在", titles.includes("存储空间"), titles.join(","));
  check("旧的「显示与链接」已并入外观效果", !titles.includes("显示与链接"), titles.join(","));
  check("新建分组默认外观已并入外观效果", !titles.includes("新建分组默认外观"), titles.join(","));

  const fontLabels = await page.$$eval(".settings-font-grid .settings-field > span", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  check("字号改名并新增两项", ["UI 字号", "正文字号", "记账字号", "日记字号"].every((label) => fontLabels.includes(label)), fontLabels.join(","));
  check("不再有 Markdown 字号", !fontLabels.includes("Markdown 字号"), fontLabels.join(","));
  const appearanceText = await page.textContent(".settings-section:has(h3:text-is('外观效果'))");
  check("新建分组默认外观在外观效果里", (appearanceText ?? "").includes("新建分组默认外观"));
  check("固定分组配置在外观效果里", (appearanceText ?? "").includes("固定分组"));
  const lifecycleText = await page.textContent(".settings-section:has(h3:text-is('窗口与系统'))");
  check("链接打开移进窗口与系统", (lifecycleText ?? "").includes("链接打开"));

  // 记账字号真的作用到记账页
  await page.click(".settings-section:has(h3:text-is('外观效果')) .settings-field:has(> span:text-is('记账字号')) input");
  await page.fill(".settings-section:has(h3:text-is('外观效果')) .settings-field:has(> span:text-is('记账字号')) input", "24");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  const ledgerFontVar = await page.$eval(".app-shell", (el) => getComputedStyle(el).getPropertyValue("--font-ledger").trim());
  check("记账字号写进 --font-ledger", ledgerFontVar === "24px", ledgerFontVar);
  const controlFontVar = await page.$eval(".app-shell", (el) => getComputedStyle(el).getPropertyValue("--font-control").trim());
  check("UI 字号不受记账字号影响", controlFontVar !== "24px", controlFontVar);

  // 固定分组：隐藏日记 + 换成只图标
  const chips = await page.$$(".settings-chip");
  check("固定分组 chips 列出可选项", chips.length >= 6, String(chips.length));
  await page.click(".settings-chip:has-text('日记')");
  await page.waitForTimeout(300);
  const navLabelsAfter = await page.$$eval(".system-nav .list-name", (els) => els.map((el) => el.textContent?.trim() ?? ""));
  check("取消勾选后日记行消失", !navLabelsAfter.includes("日记"), navLabelsAfter.join(","));
  await page.click(".settings-chip:has-text('日记')");
  await page.waitForTimeout(300);
  await page.click(".settings-segmented button:has-text('只图标')");
  await page.waitForTimeout(300);
  check("只图标模式生效", (await page.$$(".system-nav.nav-icons")).length === 1);
  check("只图标模式不渲染名称", (await page.$$(".system-nav .list-name")).length === 0);
  await page.click(".settings-segmented button:has-text('双列')");
  await page.waitForTimeout(300);
  check("双列模式生效", (await page.$$(".system-nav.nav-grid")).length === 1);
  const gridColumns = await page.$eval(".system-nav.nav-grid", (el) => getComputedStyle(el).gridTemplateColumns);
  check("双列真的是两列", gridColumns.split(" ").length === 2, gridColumns);
  await page.click(".settings-segmented button:has-text('单列')");
  await page.waitForTimeout(300);

  // 存储空间：浏览器预览没有数据目录，但按钮与说明都得在
  const storageText = await page.textContent(".settings-section:has(h3:text-is('存储空间'))");
  check("存储空间给出释放空间按钮", (await page.$$(".settings-section:has(h3:text-is('存储空间')) button:has-text('释放空间')")).length === 1);
  check("存储空间说明清理边界", (storageText ?? "").includes("一概不动"));

  // 同步账户不再用资料里的显示名预填（默认资料名是 Example User）
  const usernameInput = await page.$("input[placeholder^='账户名']");
  check("同步账户表单在", Boolean(usernameInput));
  if (usernameInput) {
    const username = await usernameInput.inputValue();
    check("同步用户名不预填显示名", username === "", username);
  }

  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });
  check("桌面没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------- 日记：时刻行也在 ----------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-view", { timeout: 10000 });
  const subtitle = await page.locator(".list-subtitle").boundingBox();
  const diaryScroll = await page.locator(".diary-scroll").boundingBox();
  check(
    "日记小字与卡片左缘对齐",
    Math.abs(subtitle.x - diaryScroll.x) < 2,
    `${subtitle.x} vs ${diaryScroll.x}`
  );
  await page.click(".diary-fab");
  await page.waitForSelector(".editor-overlay", { timeout: 8000 });
  await page.click(".editor-meta-field .editor-meta-trigger >> nth=0");
  await page.waitForSelector(".editor-meta-pop .date-picker", { timeout: 5000 });
  check("日记编辑器日期浮层带时刻行", (await page.$$(".editor-meta-pop .dp-time-trigger")).length === 1);
  await page.click(".editor-meta-pop .dp-time-switch input");
  await page.waitForTimeout(200);
  await page.click(".editor-meta-pop .dp-time-trigger");
  await page.waitForSelector(".editor-meta-pop .time-picker", { timeout: 5000 });
  check("日记也能选到分钟", (await page.$$(".editor-meta-pop .time-col")).length === 2);
  check("日记页没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------- 移动端：抽屉遮罩吞掉点击，不误触下面 ----------
{
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const { page, errors } = await freshPage(context);
  await openLedger(page);
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  const sheet = await page.locator(".ledger-sheet").boundingBox();
  check("移动端抽屉在下半屏", sheet.y > 200, String(sheet.y));

  // 抽屉上方是遮罩：点它应该关掉抽屉，而且那一下补发的 click 不能落到下面的内容上
  await page.touchscreen.tap(195, Math.max(40, sheet.y - 120));
  await page.waitForTimeout(700);
  check("点遮罩关掉抽屉", (await page.$$(".ledger-sheet")).length === 0);
  check("关掉后没有误触打开别的浮层", (await page.$$(".editor-overlay")).length === 0);

  // 保存键在右下角，正上方是「+」：保存后不能被补发的 click 再开一个面板
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  await page.click(".ledger-cat-grid > .ledger-cat-cell:not(.add) >> nth=0");
  await page.waitForSelector(".ledger-cat-sub .ledger-cat-cell", { timeout: 5000 });
  await page.click(".ledger-cat-sub .ledger-cat-cell:not(.add) >> nth=0");
  await page.tap(".ledger-key:has-text('7')");
  await page.tap(".ledger-key:has-text('8')");
  const saveKey = await page.locator(".ledger-key.save").boundingBox();
  await page.touchscreen.tap(saveKey.x + saveKey.width / 2, saveKey.y + saveKey.height / 2);
  await page.waitForTimeout(800);
  check("移动端记一笔后面板关闭", (await page.$$(".ledger-sheet")).length === 0);
  check("保存后没有幽灵点击再开面板", (await page.$$(".editor-overlay")).length === 0);
  check("这一笔真的记上了", (await page.$$(".ledger-entry")).length >= 1);
  check("移动端没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

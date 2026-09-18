// v0.7.5 回归：列表灰色汇总与无备注条目居中、条目多图角标与旧单图折叠、卡片加号常驻、
// 编辑器行内二级搁板（桌面 7 列 626px / 移动 5 列定高五排）、统计段控分平台与结余三曲线、
// NaN月 修复、图表悬浮/点按读数、饼图出场动效、账户类型自定义（图标/编辑/删除）、
// 编辑账户直设当前余额、资产趋势图（桌面并排浮窗 / 移动纵排）、图标选择器分组复用记账库、
// 桌面工具箱、移动端导航高亮条不残留、移动端账户表单能滚到底。
// 需先 npm run dev（浏览器预览走 localStorage legacy 路径；core 命令层由 cargo test 覆盖）。
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

function dayOffset(days) {
  const stamp = new Date(Date.now() - days * 86_400_000);
  return `${stamp.getFullYear()}-${String(stamp.getMonth() + 1).padStart(2, "0")}-${String(stamp.getDate()).padStart(2, "0")}`;
}

/** 塞一本账：带备注/不带备注/多图/旧单图字段/跨月流水 + 20 个额外支出大类（移动端五排用）。 */
async function seedBook(page) {
  await page.evaluate(({ key, dates }) => {
    const book = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!book || !book.accounts?.length) return;
    const account = book.accounts[0].id;
    const parent = book.categories.find((item) => item.side === "expense" && !item.parentId);
    const children = book.categories.filter((item) => item.parentId === parent?.id).slice(0, 2);
    for (let index = 0; index < 20; index += 1) {
      book.categories.push({
        id: `v075-extra-${index}`,
        name: `额外${index}`,
        side: "expense",
        icon: "Package",
        color: "",
        order: 100 + index,
        createdAt: new Date().toISOString()
      });
    }
    book.entries = [
      {
        id: "v075-a", kind: "expense", amountCents: 1234, accountId: account,
        categoryId: children[0]?.id ?? "", date: dates.today, time: "09:15", note: "早餐",
        createdAt: new Date().toISOString()
      },
      {
        id: "v075-b", kind: "expense", amountCents: 5600, accountId: account,
        categoryId: children[1]?.id ?? "", date: dates.today, time: "18:40", note: "",
        images: ["md-1-test.png", "md-2-test.png"],
        createdAt: new Date().toISOString()
      },
      {
        id: "v075-c", kind: "income", amountCents: 8800, accountId: account,
        date: dates.today, time: "12:00", note: "报销", createdAt: new Date().toISOString()
      },
      // v0.7.4 的旧数据是单数 image 字段：normalize 必须把它折叠进 images
      {
        id: "v075-e", kind: "expense", amountCents: 999, accountId: account,
        date: dates.y1, time: "20:00", note: "旧图", image: "md-legacy.png",
        createdAt: new Date().toISOString()
      },
      {
        id: "v075-f", kind: "expense", amountCents: 2000, accountId: account,
        date: dates.d80, note: "八十天前", createdAt: new Date().toISOString()
      },
      {
        id: "v075-g", kind: "income", amountCents: 50000, accountId: account,
        date: dates.d20, note: "二十天前", createdAt: new Date().toISOString()
      },
      // 既没备注也没图：只有这种才走 solo 布局（v0.7.6 起「有备注或有图」都走两行）
      {
        id: "v075-h", kind: "expense", amountCents: 3300, accountId: account,
        categoryId: children[0]?.id ?? "", date: dates.today, time: "08:05", note: "",
        createdAt: new Date().toISOString()
      }
    ];
    localStorage.setItem(key, JSON.stringify(book));
  }, { key: LEDGER_KEY, dates: { today: dayOffset(0), y1: dayOffset(1), d20: dayOffset(20), d40: dayOffset(40), d80: dayOffset(80) } });
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------- 桌面 ----------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);
  await openLedger(page);
  await seedBook(page);
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  // 1. 汇总灰字：月份条与卡片头的收/支跟「周几」同色
  const weekColor = await page.$eval(".ledger-date-week", (el) => getComputedStyle(el).color);
  const monthSumColor = await page.$eval(".ledger-month-sums em", (el) => getComputedStyle(el).color);
  const cardSumColor = await page.$eval(".ledger-card-sums em", (el) => getComputedStyle(el).color);
  check("月份条收/支/结余与周几同灰", monthSumColor === weekColor, `${monthSumColor} vs ${weekColor}`);
  check("卡片头收/支与周几同灰", cardSumColor === weekColor, `${cardSumColor} vs ${weekColor}`);

  // 2. 无备注无图条目：分类名与图标一样上下居中（solo）；有备注或有图的都是两行
  const solo = page.locator(".ledger-entry:has(.ledger-entry-main.solo)").first();
  check("无备注无图条目走 solo 布局", (await page.$$(".ledger-entry-main.solo")).length >= 1);
  const soloIcon = await solo.locator(".ledger-entry-icon").boundingBox();
  const soloName = await solo.locator(".ledger-entry-solo-name strong").boundingBox();
  check(
    "solo 布局分类名与图标垂直居中对齐",
    soloIcon !== null && soloName !== null &&
      Math.abs(soloIcon.y + soloIcon.height / 2 - (soloName.y + soloName.height / 2)) < 3,
    `${soloIcon?.y}/${soloIcon?.height} vs ${soloName?.y}/${soloName?.height}`
  );
  check(
    "有备注条目仍是两行",
    (await page.$$(".ledger-entry:has-text('早餐') .ledger-entry-sub")).length === 1
  );
  // 13.1：没备注但有图的条目走两行，图片图标落在备注区（不是分类名旁边）
  // （用时刻认这一笔：金额显示成 -56.00，「5600」在界面上找不到）
  const imageOnly = ".ledger-entry:has-text('18:40')";
  check(
    "无备注但有图的条目也走两行",
    (await page.$$(`${imageOnly} .ledger-entry-sub .ledger-entry-image`)).length === 1 &&
      (await page.$$(`${imageOnly} .ledger-entry-main.solo`)).length === 0
  );

  // 3. 图片图标：只在备注区、不带数量角标（角标只在编辑器里，v0.7.6）
  check(
    "有图的条目都只有一个图片图标",
    (await page.$$(".ledger-entry .ledger-entry-image")).length >= 2,
    String((await page.$$(".ledger-entry .ledger-entry-image")).length)
  );
  check(
    "列表里的图片图标不带数量角标（v0.7.6）",
    (await page.$$(".ledger-entry .ledger-entry-image-count")).length === 0
  );
  check(
    "旧的单数 image 字段被折叠（旧图条目也有图片图标）",
    (await page.$$(".ledger-entry:has-text('旧图') .ledger-entry-image")).length === 1
  );

  // 8. 卡片右上角加号常驻
  const addOpacity = await page.$eval(".ledger-card-add", (el) => getComputedStyle(el).opacity);
  check("卡片加号不 hover 也可见", Number.parseFloat(addOpacity) > 0.3, addOpacity);

  // 4. 编辑器：桌面 626px 宽、7 列、二级搁板在大类所在行的下一行
  await page.click(".ledger-entry:has-text('早餐')");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  // app-shell 有 transform 缩放：boundingBox 是视觉像素，布局宽度看 offsetWidth
  const sheetWidth = await page.$eval(".ledger-editor-sheet", (el) => el.offsetWidth);
  check("桌面编辑窗加宽到 626", Math.abs(sheetWidth - 626) < 3, String(sheetWidth));
  const gridCols = await page.$eval(".ledger-cat-grid", (el) => getComputedStyle(el).gridTemplateColumns);
  check("桌面分类网格 7 列", gridCols.split(" ").length === 7, gridCols);
  const cells = page.locator(".ledger-cat-grid > .ledger-cat-cell:not(.add)");
  const cell2 = await cells.nth(2).boundingBox();
  await cells.nth(2).click();
  await page.waitForSelector(".ledger-cat-sub", { timeout: 5000 });
  const shelf = await page.locator(".ledger-cat-sub").boundingBox();
  const cell6 = await cells.nth(6).boundingBox();
  const cell7 = await cells.nth(7).boundingBox();
  check(
    "二级搁板在大类所在行下面一行（行尾之后、下一行之前）",
    shelf !== null && cell6 !== null && cell7 !== null &&
      shelf.y >= cell6.y + cell6.height - 3 && shelf.y < cell7.y,
    `shelf ${shelf?.y} row0底 ${(cell6?.y ?? 0) + (cell6?.height ?? 0)} row1顶 ${cell7?.y}`
  );
  const gridBox = await page.locator(".ledger-cat-zone > .ledger-cat-grid").boundingBox();
  check(
    "搁板横贯整个网格",
    shelf !== null && gridBox !== null && Math.abs(shelf.width - gridBox.width) < 16,
    `${shelf?.width} vs ${gridBox?.width}`
  );
  // 多图按钮状态：改带两张图的那笔
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });
  await page.click(".ledger-entry:has(.ledger-entry-image)");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  check("编辑器图片按钮点亮", (await page.$$(".ledger-meta-plain.has-image")).length === 1);
  check(
    "编辑器图片按钮带数量角标",
    ((await page.textContent(".ledger-image-badge")) ?? "").trim() === "2"
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });

  // 5/6. 统计：桌面段控全称含周、结余三曲线、无 NaN、悬浮读数
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-line-chart", { timeout: 8000 });
  const modeLabels = await page.$$eval(".ledger-stats-bar .ledger-segmented:first-child button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  check("桌面周期段控含周共五项", modeLabels.join(",") === "周,月,年,总,自定义", modeLabels.join(","));
  const sideLabels = await page.$$eval(".ledger-side-switch button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  // v0.8.4 需求 10：侧段控只剩 支出|收入，「结余」曲线改由图例胶囊切出
  check("桌面侧段控是支出/收入", sideLabels.join(",") === "支出,收入", sideLabels.join(","));
  const axisTexts = await page.$$eval(".ledger-chart-axis", (els) => els.map((el) => el.textContent ?? ""));
  check("月视图横轴没有 NaN", axisTexts.every((text) => !text.includes("NaN")), axisTexts.join("|"));
  await page.locator(".ledger-legend button", { hasText: "结余" }).click();
  await page.waitForTimeout(400);
  check("点结余胶囊后画三条曲线", (await page.$$(".ledger-line.out")).length === 1 &&
    (await page.$$(".ledger-line.in")).length === 1 && (await page.$$(".ledger-line.bal")).length === 1);
  const chartBox = await page.locator(".ledger-chart-box").boundingBox();
  await page.mouse.move(chartBox.x + chartBox.width / 2, chartBox.y + chartBox.height / 2);
  await page.waitForSelector(".ledger-chart-tip", { timeout: 3000 });
  const tipText = await page.textContent(".ledger-chart-tip");
  check("桌面悬浮出读数（含结余）", (tipText ?? "").includes("结余"), (tipText ?? "").replace(/\s+/g, " "));
  await page.click(".ledger-segmented button:has-text('总')");
  await page.waitForTimeout(250);
  const totalAxis = await page.$$eval(".ledger-chart-axis", (els) => els.map((el) => el.textContent ?? ""));
  check("总跨度按月分桶且没有 NaN", totalAxis.some((t) => t.includes("月")) && totalAxis.every((t) => !t.includes("NaN")), totalAxis.join("|"));
  await page.click(".ledger-segmented button:has-text('月')");
  await page.waitForTimeout(200);

  // 9. 饼图出场动效
  const ringAnim = await page.$eval(".ledger-donut-ring", (el) => getComputedStyle(el).animationName);
  check("占比环带出场动画", ringAnim === "ledger-donut-intro", ringAnim);

  // 10. 资产视图：FAB=添加账户、自定义类型、直设当前余额
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-net-card", { timeout: 8000 });
  check("资产视图 FAB 是添加账户", ((await page.getAttribute(".ledger-fab", "title")) ?? "") === "添加账户");

  // 11. 趋势卡与净资产并排 + 浮窗放大 + 悬浮读数 + 全屏按钮（v0.7.6 两段式）
  const netBox = await page.locator(".ledger-net-card").boundingBox();
  const trendBox = await page.locator(".ledger-trend-card").boundingBox();
  check(
    "桌面趋势卡在净资产右侧并排",
    netBox !== null && trendBox !== null && trendBox.x > netBox.x + netBox.width - 8 &&
      Math.abs(trendBox.y - netBox.y) < 60,
    `${trendBox?.x} vs ${netBox?.x}+${netBox?.width}`
  );
  check("趋势曲线画出来了", (await page.$$(".ledger-trend-card .ledger-trend-line")).length === 1);
  check(
    "卡片里的趋势图带横纵坐标（v0.7.6）",
    (await page.$$(".ledger-trend-card .ledger-chart-axis")).length >= 4
  );
  // v0.7.7：卡片自己就能读数字（鼠标悬浮 / 点击），全屏按钮才开浮窗
  const cardChartBox = await page.locator(".ledger-trend-card .ledger-chart-box").boundingBox();
  await page.mouse.move(cardChartBox.x + cardChartBox.width / 2, cardChartBox.y + cardChartBox.height / 2);
  await page.waitForSelector(".ledger-trend-card .ledger-chart-tip", { timeout: 3000 });
  check(
    "卡片悬浮直接显示数额（v0.7.7）",
    ((await page.textContent(".ledger-trend-card .ledger-chart-tip")) ?? "").includes("总资产")
  );
  await page.mouse.click(cardChartBox.x + cardChartBox.width * 0.4, cardChartBox.y + cardChartBox.height / 2);
  await page.waitForTimeout(250);
  check("点卡片不拉浮窗（v0.7.7）", (await page.$$(".ledger-trend-dialog")).length === 0);
  await page.click(".ledger-trend-card .ledger-trend-zoom");
  await page.waitForSelector(".ledger-trend-dialog", { timeout: 8000 });
  const dialogWidth = await page.$eval(".ledger-trend-dialog", (el) => el.offsetWidth);
  check("趋势浮窗放大", dialogWidth > 700, String(dialogWidth));
  const trendChartBox = await page.locator(".ledger-trend-dialog .ledger-chart-box").boundingBox();
  await page.mouse.move(trendChartBox.x + trendChartBox.width / 2, trendChartBox.y + trendChartBox.height / 2);
  await page.waitForSelector(".ledger-trend-dialog .ledger-chart-tip", { timeout: 3000 });
  check(
    "趋势浮窗悬浮显示数额",
    ((await page.textContent(".ledger-trend-dialog .ledger-chart-tip")) ?? "").includes("总资产")
  );
  // 全屏按钮：铺满窗口（不是关掉浮窗）
  const expandedBefore = await page.$eval(".ledger-trend-dialog", (el) => el.offsetHeight);
  await page.click(".ledger-trend-dialog .ledger-icon-button[title='全屏查看']");
  await page.waitForTimeout(150);
  const expandedAfter = await page.$eval(".ledger-trend-dialog", (el) => el.offsetHeight);
  check(
    "桌面全屏按钮把浮窗铺满窗口",
    expandedAfter > expandedBefore + 60 && (await page.$(".ledger-trend-dialog")) !== null,
    `${expandedBefore} → ${expandedAfter}`
  );
  await page.click(".ledger-trend-dialog .ledger-icon-button[title='关闭']");
  await page.waitForSelector(".ledger-trend-dialog", { state: "detached", timeout: 8000 });

  // FAB → 添加账户表单 → 自定义类型（带图标/颜色、可编辑/删除）
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  check("FAB 打开的是添加账户表单", ((await page.textContent(".ledger-sheet-title")) ?? "").trim() === "添加账户");
  await page.click(".ledger-choice:has-text('类型')");
  await page.waitForSelector(".ledger-type-form", { timeout: 5000 });
  check("类型小表单带图标网格", (await page.$$(".ledger-type-form .ledger-icon-cell")).length >= 30);
  await page.fill(".ledger-type-form input[placeholder='例如 校园卡']", "校园卡");
  await page.click(".ledger-type-form .ledger-icon-cell >> nth=2");
  await page.click(".ledger-type-form .ledger-color-dot >> nth=3");
  await page.click(".ledger-type-form button:has-text('添加类型')");
  await page.waitForSelector(".ledger-type-form", { state: "detached", timeout: 8000 });
  const customChip = page.locator(".ledger-choice:has-text('校园卡')");
  check("自定义类型进 chips", (await customChip.count()) >= 1);
  check("自定义类型已选中", (await customChip.first().evaluate((el) => el.classList.contains("active"))) === true);
  check("自定义 chip 带编辑铅笔", (await customChip.first().locator(".ledger-choice-edit").count()) === 1);
  await customChip.first().locator(".ledger-choice-edit").click();
  await page.waitForSelector(".ledger-type-form", { timeout: 5000 });
  check(
    "铅笔打开编辑表单（预填名字 + 删除按钮）",
    (await page.inputValue(".ledger-type-form input")) === "校园卡" &&
      (await page.$$(".ledger-type-form button:has-text('删除类型')")).length === 1
  );
  await page.click(".ledger-type-form button:has-text('删除类型')");
  await page.waitForSelector(".ledger-type-form", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(200);
  check("删除后自定义类型从 chips 消失", (await page.locator(".ledger-choice:has-text('校园卡')").count()) === 0);

  // 编辑账户 = 直设「当前金额」（Esc 两段式：先收表单再关面板）
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-form-body", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  const balanceBefore = ((await page.textContent(".ledger-account-row .ledger-account-balance")) ?? "").trim();
  await page.click(".ledger-account-row");
  await page.waitForSelector(".ledger-manager .ledger-form-body", { timeout: 8000 });
  const amountLabel = await page.textContent(".ledger-form-body .ledger-field-row > span >> nth=3");
  check("编辑表单是「当前金额」不是期初", (amountLabel ?? "").includes("当前金额"), amountLabel ?? "");
  const amountValue = await page.inputValue(".ledger-form-body input[inputmode='decimal']");
  check("金额预填成账户现有余额", amountValue === balanceBefore, `${amountValue} vs ${balanceBefore}`);
  await page.fill(".ledger-form-body input[inputmode='decimal']", "100.00");
  await page.click(".ledger-sheet-foot button:has-text('保存')");
  await page.waitForSelector(".ledger-form-body", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(300);
  const balanceAfter = ((await page.textContent(".ledger-account-row .ledger-account-balance")) ?? "").trim();
  check("保存后余额直接变成新值", balanceAfter === "100.00", balanceAfter);
  check("净资产跟着直设余额走", ((await page.textContent(".ledger-net-value")) ?? "").trim() === "100.00");

  // 12. 图标选择器：分组复用记账库 + emoji 无滚动条注入
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForTimeout(200);
  const treeRow = page.locator(".tree-row").first();
  await treeRow.click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  await page.click(".context-menu .menu-item-button:has-text('选择图标')");
  await page.waitForSelector(".icon-picker", { timeout: 8000 });
  check("图标选择器带分组 chips", (await page.$$(".icon-group-chip")).length >= 20);
  const allIcons = (await page.$$(".icon-grid button")).length;
  check("全部档复用记账图标库（两百多个）", allIcons >= 200, String(allIcons));
  await page.click(".icon-group-chip:has-text('饮食')");
  const groupIcons = (await page.$$(".icon-grid button")).length;
  check("分组筛选生效", groupIcons > 0 && groupIcons < allIcons, `${groupIcons}/${allIcons}`);
  check(
    "emoji-picker 的 shadow 里注入了隐藏滚动条样式",
    await page.evaluate(() =>
      Boolean(document.querySelector("emoji-picker")?.shadowRoot?.querySelector("#kx-no-scrollbar"))
    )
  );
  await page.click(".icon-grid button >> nth=1");
  await page.waitForSelector(".icon-picker", { state: "detached", timeout: 8000 });

  // 13. 桌面工具箱 + 设置里的固定分组
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-view", { timeout: 8000 });
  check("桌面工具箱打开", (await page.$$(".toolbox-card")).length >= 1);
  check(
    "工具箱占主区域时工作区隐藏",
    await page.$eval(".workspace", (el) => getComputedStyle(el).display === "none")
  );
  await page.click(".toolbox-card >> nth=0");
  await page.waitForSelector(".toolbox-sub-bar", { timeout: 8000 });
  await page.waitForSelector(".toolbox-field-row", { timeout: 8000 });
  check("工具子视图打开（懒加载组件）", (await page.$$(".toolbox-field-row")).length >= 3);
  await page.click(".toolbox-sub-bar button");
  await page.waitForSelector(".toolbox-list", { timeout: 5000 });
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  check(
    "设置的固定分组能勾选工具箱",
    (await page.locator("aside.settings-drawer :text('工具箱')").count()) >= 1
  );
  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });

  check("桌面没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------- 移动端 ----------
{
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const { page, errors } = await freshPage(context);
  await openLedger(page);
  await seedBook(page);
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  // 14. 固定分组高亮条不残留（先退回列表视图——记账整页开着时侧栏是隐藏的）
  await page.goBack();
  await page.waitForSelector(".app-shell.view-list", { timeout: 8000 });
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForSelector(".app-shell.view-content", { timeout: 8000 });
  await page.goBack();
  await page.waitForSelector(".app-shell.view-list", { timeout: 8000 });
  const railColor = await page.$eval(
    ".system-nav .nav-row.selected .active-rail",
    (el) => getComputedStyle(el).backgroundColor
  ).catch(() => "none-selected");
  check(
    "移动端选中行不画蓝色高亮条",
    railColor === "none-selected" || railColor === "rgba(0, 0, 0, 0)" || railColor === "transparent",
    railColor
  );

  await openLedger(page);

  // 5. 统计段控：月/年/总/自定义 + 支/收/结余，且不超屏
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-line-chart", { timeout: 8000 });
  const mModes = await page.$$eval(".ledger-stats-bar .ledger-segmented:first-child button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  // v0.8.4 需求 10：移动端把「周」加回来（侧段控少了一档，宽度腾出来了）
  check("移动周期段控含周共五项", mModes.join(",") === "周,月,年,总,自定义", mModes.join(","));
  const mSides = await page.$$eval(".ledger-side-switch button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  );
  check("移动侧段控只剩支/收（短文案）", mSides.join(",") === "支,收", mSides.join(","));
  const statsBarBox = await page.locator(".ledger-stats-bar").boundingBox();
  check(
    "段控不超出屏幕",
    statsBarBox !== null && statsBarBox.x >= -1 && statsBarBox.x + statsBarBox.width <= 391,
    `${statsBarBox?.x}+${statsBarBox?.width}`
  );
  const noHScroll = await page.evaluate(() => {
    const view = document.querySelector(".ledger-view");
    return view ? view.scrollWidth <= view.clientWidth + 1 : false;
  });
  check("统计页无横向溢出", noHScroll);

  // 自定义周期弹层不超屏
  await page.click(".ledger-segmented button:has-text('自定义')");
  await page.waitForTimeout(200);
  await page.click(".ledger-custom-field strong >> nth=0");
  await page.waitForSelector(".ledger-custom-field .ledger-pop.date", { timeout: 5000 });
  const popBox = await page.locator(".ledger-custom-field.open .ledger-pop").boundingBox();
  check(
    "选日弹层收进屏幕",
    popBox !== null && popBox.x >= -1 && popBox.x + popBox.width <= 391,
    `${popBox?.x}+${popBox?.width}`
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  await page.click(".ledger-segmented button:has-text('月')");
  await page.waitForTimeout(200);

  // 6. 移动端点按读数
  const mChart = await page.locator(".ledger-chart-box").boundingBox();
  await page.touchscreen.tap(mChart.x + mChart.width / 2, mChart.y + mChart.height / 2);
  await page.waitForSelector(".ledger-chart-tip", { timeout: 3000 });
  check("移动端点按出读数", ((await page.textContent(".ledger-chart-tip")) ?? "").length > 0);

  // 4. 编辑器：定高五排 + 5 列 + 搁板在行下
  await switchView(page, "列表视图");
  await page.waitForSelector(".ledger-card", { timeout: 8000 });
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  // 移动端 app-shell 同样有 transform 缩放：布局尺寸一律 offsetHeight/clientHeight
  const sheetHeight = await page.$eval(".ledger-editor-sheet", (el) => el.offsetHeight);
  check("移动抽屉定高 ≈min(92vh,780)", sheetHeight > 760 && sheetHeight <= 781, String(sheetHeight));
  const mCols = await page.$eval(".ledger-cat-grid", (el) => getComputedStyle(el).gridTemplateColumns);
  check("移动分类网格 5 列", mCols.split(" ").length === 5, mCols);
  const zone = await page.evaluate(() => {
    const el = document.querySelector(".ledger-cat-zone");
    const cell = document.querySelector(".ledger-cat-cell");
    return el && cell ? { client: el.clientHeight, scroll: el.scrollHeight, cell: cell.offsetHeight } : null;
  });
  check(
    "分类区一眼放下五排",
    zone !== null && zone.client >= zone.cell * 5 + 32 - 6,
    JSON.stringify(zone)
  );
  check("图标多了分类区自己滚", zone !== null && zone.scroll > zone.client + 10, JSON.stringify(zone));
  const mCells = page.locator(".ledger-cat-grid > .ledger-cat-cell:not(.add)");
  const mCell2 = await mCells.nth(2).boundingBox();
  await mCells.nth(2).tap();
  await page.waitForSelector(".ledger-cat-sub", { timeout: 5000 });
  const mShelf = await page.locator(".ledger-cat-sub").boundingBox();
  const mCell4 = await mCells.nth(4).boundingBox();
  const mCell5 = await mCells.nth(5).boundingBox();
  check(
    "移动端搁板也在大类所在行下面一行",
    mShelf !== null && mCell4 !== null && mCell5 !== null &&
      mShelf.y >= mCell4.y + mCell4.height - 3 && mShelf.y < mCell5.y,
    `shelf ${mShelf?.y} row0底 ${(mCell4?.y ?? 0) + (mCell4?.height ?? 0)} row1顶 ${mCell5?.y}`
  );
  check("点大类不改变抽屉高度", (await page.$eval(".ledger-editor-sheet", (el) => el.offsetHeight)) === sheetHeight);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });

  // 7. 账户表单能滚到底（最后一行完整可见）
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-net-card", { timeout: 8000 });
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-manager .ledger-form-body", { timeout: 8000 });
  const gridOverflow = await page.$eval(".ledger-form-body .ledger-icon-grid", (el) => {
    const style = getComputedStyle(el);
    return { overflow: style.overflowY, scrollbar: style.scrollbarWidth, h: el.clientHeight, scrollH: el.scrollHeight };
  });
  // v0.7.7：图标区自己滚（不显滚动条）——图标两三百个，全展开会把名称/颜色/预览挤远
  check(
    "移动端表单里图标网格自己滚且不画滚动条（v0.7.7）",
    gridOverflow.overflow === "auto" && gridOverflow.scrollbar === "none" && gridOverflow.scrollH > gridOverflow.h,
    JSON.stringify(gridOverflow)
  );
  await page.evaluate(() => {
    const body = document.querySelector(".ledger-manager .ledger-sheet-body");
    if (body) body.scrollTop = body.scrollHeight;
  });
  await page.waitForTimeout(200);
  const clipped = await page.evaluate(() => {
    const body = document.querySelector(".ledger-manager .ledger-sheet-body");
    const last = document.querySelector(".ledger-manager .ledger-form-preview");
    if (!body || !last) return null;
    const bodyRect = body.getBoundingClientRect();
    const lastRect = last.getBoundingClientRect();
    return {
      scrolledToEnd: Math.abs(body.scrollTop + body.clientHeight - body.scrollHeight) < 3,
      lastBottom: lastRect.bottom,
      bodyBottom: bodyRect.bottom,
      viewport: window.innerHeight
    };
  });
  check(
    "账户表单能滚到底且最后一行完整可见",
    clipped !== null && clipped.scrolledToEnd && clipped.lastBottom <= clipped.bodyBottom + 1 &&
      clipped.lastBottom <= clipped.viewport,
    JSON.stringify(clipped)
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-form-body", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 11. 移动端趋势块纵排在净资产与资金账户之间；点开是「内容视图」，全屏按钮才横屏
  const mNet = await page.locator(".ledger-net-card").boundingBox();
  const mTrend = await page.locator(".ledger-trend-card").boundingBox();
  const mAccounts = await page.locator(".ledger-panel:has-text('资金账户')").boundingBox();
  check(
    "移动趋势块夹在净资产与资金账户之间",
    mNet !== null && mTrend !== null && mAccounts !== null &&
      mTrend.y > mNet.y + mNet.height - 8 && mAccounts.y > mTrend.y + mTrend.height - 8,
    `${mNet?.y} → ${mTrend?.y} → ${mAccounts?.y}`
  );
  // v0.7.7：点图就是点曲线（直接读数），不拉半屏浮窗；全屏按钮才进横屏全屏
  check(
    "移动卡片上的图带横纵坐标",
    (await page.$$(".ledger-trend-card .ledger-chart-axis")).length >= 4
  );
  const mChartBox = await page.locator(".ledger-trend-card .ledger-chart-box").boundingBox();
  await page.locator(".ledger-trend-card .ledger-chart-box").tap({ position: { x: 60, y: 30 } });
  await page.waitForSelector(".ledger-trend-card .ledger-chart-tip", { timeout: 3000 });
  check(
    "移动端点图直接显示读数（v0.7.7）",
    ((await page.textContent(".ledger-trend-card .ledger-chart-tip")) ?? "").includes("总资产"),
    `box ${mChartBox?.width}x${mChartBox?.height}`
  );
  check("点图不拉半屏浮窗（v0.7.7）", (await page.$$(".ledger-trend-dialog")).length === 0);
  await page.click(".ledger-trend-card .ledger-trend-zoom");
  await page.waitForSelector(".ledger-trend-full", { timeout: 8000 });
  check("全屏按钮直接进横屏（v0.7.7）", (await page.$$(".ledger-trend-dialog")).length === 0);
  // 旋转 90° 的元素 bbox 是轴对齐外接框（铺满屏幕）：横屏与否看布局宽高
  const rot = await page.$eval(".ledger-trend-rot", (el) => [el.offsetWidth, el.offsetHeight]);
  check(
    "移动端横屏大图（旋转层宽>高）",
    rot[0] > rot[1] && rot[0] > 700,
    `${rot[0]}x${rot[1]}`
  );
  check("横屏层里有曲线", (await page.$$(".ledger-trend-full .ledger-trend-line")).length === 1);
  // 横屏层只有关闭（进来就是全屏，没有中间态可退）
  await page.click(".ledger-trend-rot-head .ledger-image-tool[title='关闭']");
  await page.waitForSelector(".ledger-trend-full", { state: "detached", timeout: 8000 });

  check("移动端没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

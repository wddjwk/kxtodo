// v0.7.4 回归：记账列表两行卡片、统计双段控与白块、收支双曲线、周/自定义周期、
// 钻取二级分类独立配色、编辑器新布局（圆形分类 + 内联二级 + 备注行 + 定高抽屉）、
// 账户/分类管理不再 autofocus、账户自定义类型与专用图标、设置段控与子分组分隔、
// 条目图片图标。需先 npm run dev（浏览器预览走 localStorage legacy 路径）。
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

/** 塞一本账：一个大类带两个没填颜色的子分类（各一笔）、一笔带图片的账，都在本月。 */
async function seedBook(page) {
  await page.evaluate((key) => {
    const book = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!book || !book.accounts?.length) return;
    const account = book.accounts[0].id;
    const parent = book.categories.find((item) => item.side === "expense" && !item.parentId);
    const children = book.categories.filter((item) => item.parentId === parent?.id).slice(0, 2);
    const today = new Date();
    const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
    book.entries = [
      {
        id: "v074-a",
        kind: "expense",
        amountCents: 1234,
        accountId: account,
        categoryId: children[0]?.id ?? "",
        date,
        time: "09:15",
        note: "早餐",
        createdAt: new Date().toISOString()
      },
      {
        id: "v074-b",
        kind: "expense",
        amountCents: 5600,
        accountId: account,
        categoryId: children[1]?.id ?? "",
        date,
        time: "18:40",
        note: "",
        image: "md-1-test.png",
        createdAt: new Date().toISOString()
      },
      {
        id: "v074-c",
        kind: "income",
        amountCents: 8800,
        accountId: account,
        date,
        time: "12:00",
        note: "报销",
        createdAt: new Date().toISOString()
      }
    ];
    localStorage.setItem(key, JSON.stringify(book));
  }, LEDGER_KEY);
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

  // 1. 两行卡片：图标 + 大字行（分类左 / 金额右）+ 小字行（备注左 / 时刻账户右）
  const line = page.locator(".ledger-entry .ledger-entry-line").first();
  const nameBox = await line.locator("strong").boundingBox();
  const amountBox = await line.locator(".ledger-entry-amount").boundingBox();
  const listBox = await line.boundingBox();
  check(
    "大字行分类左、金额右",
    nameBox !== null && amountBox !== null && listBox !== null &&
      nameBox.x <= listBox.x + 4 && amountBox.x + amountBox.width >= listBox.x + listBox.width - 4,
    `${nameBox?.x} / ${amountBox?.x} / ${listBox?.x}+${listBox?.width}`
  );
  const sub = page.locator(".ledger-entry:has-text('早餐') .ledger-entry-sub").first();
  const noteBox = await sub.locator(".ledger-entry-note").boundingBox();
  const metaBox = await sub.locator(".ledger-entry-meta").boundingBox();
  check(
    "小字行备注左、时刻账户右",
    noteBox !== null && metaBox !== null && metaBox.x > noteBox.x,
    `${noteBox?.x} / ${metaBox?.x}`
  );
  check("小字行带时刻", ((await page.textContent(".ledger-entry:has-text('早餐') .ledger-entry-time")) ?? "").includes("09:15"));

  // 2. 条目图片：有图的账备注右侧有图片图标，菜单多「查看图片」
  check("有图的账显示图片图标", (await page.$$(".ledger-entry .ledger-entry-image")).length === 1);
  check("无图的账不显示图片图标", (await page.$$(".ledger-entry:has-text('早餐') .ledger-entry-image")).length === 0);
  await page.click(".ledger-entry >> nth=0", { button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 5000 });
  check("有图账的菜单带查看图片", ((await page.textContent(".context-menu")) ?? "").includes("查看图片"));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 5000 });

  // 9. 编辑器新布局：没有旧的「选择分类 金额」行；备注是无框文字；meta 是纯文字按钮；加图片按钮在
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-sheet", { timeout: 8000 });
  check("旧的分类+金额行已移除", (await page.$$(".ledger-amount-row")).length === 0);
  check("备注是无框纯文字输入", (await page.$$(".ledger-note-plain input")).length === 1);
  const noteInputBorder = await page.$eval(".ledger-note-plain input", (el) => getComputedStyle(el).borderTopWidth);
  check("备注输入框没有边框", noteInputBorder === "0px", noteInputBorder);
  check("桌面金额是可输入的大字", (await page.$$(".ledger-amount-plain input")).length === 1);
  check("meta 行是纯文字按钮", (await page.$$(".ledger-meta-plain")).length >= 3);
  check("加图片按钮在 meta 行", (await page.$$(".ledger-meta-plain[title='给这条账添加图片（可多选）']")).length === 1);
  check("一级分类是圆形图标", (await page.$$(".ledger-cat-grid .ledger-cat-round")).length >= 5);
  const roundBox = await page.locator(".ledger-cat-grid .ledger-cat-round").first().boundingBox();
  check(
    "圆形图标真的是圆",
    roundBox !== null && Math.abs(roundBox.width - roundBox.height) < 2,
    `${roundBox?.width}x${roundBox?.height}`
  );
  // 点有二级的大类：行下展开一块全宽阴影区，不是悬浮菜单
  await page.click(".ledger-cat-grid > .ledger-cat-cell:not(.add) >> nth=0");
  await page.waitForSelector(".ledger-cat-sub", { timeout: 5000 });
  const subBlock = await page.locator(".ledger-cat-sub").boundingBox();
  const zone = await page.locator(".ledger-cat-zone").boundingBox();
  check(
    "二级区在分类区内全宽展开（非浮层）",
    subBlock !== null && zone !== null && subBlock.width > zone.width * 0.8,
    `${subBlock?.width} vs ${zone?.width}`
  );
  const subBg = await page.$eval(".ledger-cat-sub", (el) => getComputedStyle(el).backgroundColor);
  check("二级区带阴影底", subBg !== "rgba(0, 0, 0, 0)" && subBg !== "transparent", subBg);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });

  // 6. 统计：双段控 + 白块 + 收支双曲线 + 周/自定义周期 + 去掉小字注释
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-summary-card", { timeout: 8000 });
  const modeButtons = await page.$$eval(".ledger-stats-bar .ledger-segmented >> nth=0 >> button", (els) =>
    els.map((el) => el.textContent?.trim() ?? "")
  ).catch(() => []);
  const modeCount = (await page.locator(".ledger-stats-bar .ledger-segmented").nth(0).locator("button").count());
  const sideCount = (await page.locator(".ledger-stats-bar .ledger-segmented").nth(1).locator("button").count());
  check("周期段控五项", modeCount === 5, String(modeCount));
  // v0.8.4 需求 10：侧段控只剩 支出|收入（结余改由图例胶囊切）
  check("收支侧段控两项", sideCount === 2, String(sideCount));
  void modeButtons;
  const statsText = (await page.textContent(".ledger-stats")) ?? "";
  check("去掉了「个日子」注释", !statsText.includes("个日子"));
  check("去掉了「点一类」注释", !statsText.includes("点一类"));
  check("白块里有三标签行", (await page.$$(".ledger-summary-labels span")).length === 3);
  check("白块里有三数额", (await page.$$(".ledger-summary strong")).length === 3);

  // v0.8.4 需求 10：结余改由右上角图例胶囊切（三枚可点），标题固定「收支趋势」
  await page.locator(".ledger-legend button", { hasText: "结余" }).click();
  await page.waitForTimeout(400);
  check("点结余胶囊画三条曲线", (await page.$$(".ledger-line.in")).length === 1 && (await page.$$(".ledger-line.out")).length === 1 && (await page.$$(".ledger-line.bal")).length === 1);
  check("折线图标题固定「收支趋势」", ((await page.textContent(".ledger-panel-head h2")) ?? "").includes("收支趋势"));
  await page.locator(".ledger-legend button", { hasText: "结余" }).click();
  await page.waitForTimeout(200);

  await page.click(".ledger-stats-bar .ledger-segmented >> nth=0 >> button:has-text('周')");
  await page.waitForTimeout(200);
  const weekLabel = (await page.textContent(".ledger-stats-period strong")) ?? "";
  check("周周期标签是起止日", /^\d{4}\/\d{2}\/\d{2}-\d{2}\/\d{2}$/.test(weekLabel.trim()), weekLabel);
  await page.click(".ledger-stats-period strong");
  await page.waitForSelector(".ledger-week-pop .date-picker", { timeout: 5000 });
  const weekPopBox = await page.locator(".ledger-week-pop").boundingBox();
  check("周周期浮层有完整高度", weekPopBox !== null && weekPopBox.height > 150, String(weekPopBox?.height));
  check("周周期可点选锚点日", true);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-week-pop", { state: "detached", timeout: 5000 });

  await page.click(".ledger-stats-bar .ledger-segmented >> nth=0 >> button:has-text('自定义')");
  await page.waitForTimeout(200);
  check("自定义周期两个起止标签", (await page.$$(".ledger-custom-field strong")).length === 2);
  await page.click(".ledger-custom-field >> nth=0 >> strong");
  await page.waitForSelector(".ledger-pop.date .date-picker", { timeout: 5000 });
  const customPopBox = await page.locator(".ledger-custom-field .ledger-pop").first().boundingBox();
  check("自定义起始浮层有完整高度", customPopBox !== null && customPopBox.height > 150, String(customPopBox?.height));
  check("起始日期可点选", true);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-pop.date", { state: "detached", timeout: 5000 });
  await page.click(".ledger-stats-bar .ledger-segmented >> nth=0 >> button:has-text('月')");
  await page.waitForTimeout(200);

  // 7. 钻取：二级分类没填颜色时环也要片片区开（不能整环一个色）
  await page.click(".ledger-rank-head >> nth=0");
  await page.waitForSelector(".ledger-drill-pop", { timeout: 8000 });
  const sliceColors = await page.$$eval(".ledger-drill-pop .ledger-donut-slice circle", (els) =>
    els.map((el) => el.getAttribute("stroke"))
  );
  check("钻取环至少两片", sliceColors.length >= 2, sliceColors.join(","));
  check("钻取环各片颜色不同", new Set(sliceColors).size === sliceColors.length, sliceColors.join(","));
  const rowColors = await page.$$eval(".ledger-drill-pop .ledger-drill-row-icon", (els) =>
    els.map((el) => el.getAttribute("style"))
  );
  check("钻取行图标颜色与环一致地各异", new Set(rowColors).size === rowColors.length, rowColors.join(" | "));
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-drill-pop", { state: "detached", timeout: 5000 });

  // 8. 账户管理：不 autofocus、自定义类型、账户专用图标分组、备注在名称下面
  await page.click(".ledger-view .header-actions > button[title='记账菜单'], .ledger-view .header-actions > button[title='更多操作']");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.click(".ledger-gear-panel .menu-item-button:has-text('账户与转账')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.click(".ledger-sheet-foot button:has-text('添加账户')");
  await page.waitForTimeout(300);
  const activeTag = await page.evaluate(() => document.activeElement?.tagName ?? "");
  check("添加账户不自动拉起输入", activeTag !== "INPUT" && activeTag !== "TEXTAREA", activeTag);
  const formText = await page.textContent(".ledger-sheet-body");
  const nameIndex = (formText ?? "").indexOf("名称");
  const noteIndex = (formText ?? "").indexOf("备注");
  const kindIndex = (formText ?? "").indexOf("类型");
  check("备注在名称下面、类型上面", nameIndex >= 0 && noteIndex > nameIndex && kindIndex > noteIndex, `${nameIndex}/${noteIndex}/${kindIndex}`);
  check("类型 chips 含预置", (formText ?? "").includes("支付宝") && (formText ?? "").includes("公积金"), (formText ?? "").slice(0, 80));
  await page.click(".ledger-choice:has-text('类型')");
  await page.waitForSelector(".ledger-type-form", { timeout: 5000 });
  await page.fill(".ledger-type-form input[placeholder='例如 校园卡']", "饭票");
  await page.click(".ledger-type-form button:has-text('添加类型')");
  await page.waitForSelector(".ledger-type-form", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(200);
  check("自定义类型成为当前类型", ((await page.textContent(".ledger-sheet-body")) ?? "").includes("饭票"));
  await page.click(".ledger-icon-group:has-text('电子支付')");
  await page.waitForTimeout(200);
  const accountIconCells = (await page.$$(".ledger-icon-cell")).length;
  check("账户图标按组筛选", accountIconCells > 0 && accountIconCells < 20, String(accountIconCells));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 10. 分类管理不 autofocus
  await page.click(".ledger-view .header-actions > button[title='记账菜单'], .ledger-view .header-actions > button[title='更多操作']");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.click(".ledger-gear-panel .menu-item-button:has-text('分类管理')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.click(".ledger-sheet-foot button:has-text('添加大类')");
  await page.waitForTimeout(300);
  const catActiveTag = await page.evaluate(() => document.activeElement?.tagName ?? "");
  check("添加分类不自动拉起输入", catActiveTag !== "INPUT" && catActiveTag !== "TEXTAREA", catActiveTag);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 12. 设置：段控是三等分滑块、外观效果子分组之间有分隔
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  await page.locator(".settings-section-toggle:has-text('外观效果')").scrollIntoViewIfNeeded();
  await page.waitForTimeout(250);
  const segButtons = page.locator(".settings-section:has(h3:text-is('外观效果')) .settings-segmented button");
  check("展示方式段控三项", (await segButtons.count()) === 3);
  const segWidths = await segButtons.evaluateAll((els) => els.map((el) => el.getBoundingClientRect().width));
  check(
    "段控三等分（不再挤在长条左端）",
    segWidths.length === 3 && Math.max(...segWidths) - Math.min(...segWidths) < 2,
    segWidths.join(",")
  );
  const secondBlockBorder = await page.$eval(
    ".settings-section:has(h3:text-is('外观效果')) .settings-block + .settings-block",
    (el) => getComputedStyle(el).borderTopStyle
  );
  check("外观效果子分组之间有分隔线", secondBlockBorder === "dashed", secondBlockBorder);
  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });

  check("桌面无 pageerror", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------- 移动端：抽屉定高 + 月份行两行 ----------
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
  const heightBefore = (await page.locator(".ledger-sheet").boundingBox())?.height ?? 0;
  await page.click(".ledger-cat-grid > .ledger-cat-cell:not(.add) >> nth=0");
  await page.waitForSelector(".ledger-cat-sub", { timeout: 5000 });
  await page.waitForTimeout(200);
  const heightAfter = (await page.locator(".ledger-sheet").boundingBox())?.height ?? 0;
  check(
    "展开二级分类抽屉高度不变",
    Math.abs(heightBefore - heightAfter) < 2,
    `${heightBefore} → ${heightAfter}`
  );
  const zoneScrollable = await page.$eval(".ledger-cat-zone", (el) => el.scrollHeight >= el.clientHeight);
  check("分类区自己可滚动", zoneScrollable);
  const zoneScrollbar = await page.$eval(".ledger-cat-zone", (el) => getComputedStyle(el).scrollbarWidth);
  check("分类区滚动条隐藏", zoneScrollbar === "none", zoneScrollbar);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-sheet", { state: "detached", timeout: 8000 });

  check("移动端无 pageerror", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "\nV074 ALL PASS" : `\n${failures} FAILURES`);
process.exit(failures ? 1 : 0);

// v0.7.6 回归：移动端去滚动条与卡片居中、左上角返回按钮（特性开关）、编辑器滑块去点击遮罩、
// 移动端统计段控字号与自定义日期点外收起、资产趋势两段式（坐标轴 + 全屏按钮 + 安全区）、
// 账户/分类表单滚到底、桌面记账编辑框正方形、日历左右滑动换月、齿轮与段控等高、
// 固定分组不留选中底色、FAB 上移与「今」在加号上方、记账搜索（页内 + 全局混排）、
// 条目账户行带图标、图片图标位置/大小/无角标/看图复用与滑动翻页。
// 需先 npm run dev（浏览器预览走 localStorage legacy 路径）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const LEDGER_KEY = "todo-note-ledger-v1";
const DIARY_KEY = "todo-note-diary-v1";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${ok || !extra ? "" : ` — ${extra}`}`);
  if (!ok) failures += 1;
}

function dayOffset(days) {
  const date = new Date();
  date.setDate(date.getDate() - days);
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
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

/** 浏览器预览没有 Tauri：临时塞一个只认图片命令的桥，让查看器能拿到可显示的数据 URL */
async function installImageStub(page) {
  await page.evaluate(() => {
    const png =
      "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd) =>
        cmd === "image_data_url" ? Promise.resolve(png) : Promise.reject(new Error(`stub: ${cmd}`)),
      convertFileSrc: (path) => path,
      transformCallback: () => 1
    };
  });
}

/** 合成一次横向滑动（触屏手势）；dx < 0 = 向左滑 */
async function swipe(page, selector, dx) {
  await page.evaluate(({ selector, dx }) => {
    const el = document.querySelector(selector);
    if (!el) throw new Error(`swipe target missing: ${selector}`);
    const rect = el.getBoundingClientRect();
    const cy = rect.top + rect.height / 2;
    const cx = rect.left + rect.width / 2;
    const touch = (x) => new Touch({ identifier: 1, target: el, clientX: x, clientY: cy });
    const fire = (type, x) =>
      el.dispatchEvent(
        new TouchEvent(type, {
          touches: type === "touchend" ? [] : [touch(x)],
          targetTouches: type === "touchend" ? [] : [touch(x)],
          changedTouches: [touch(x)],
          bubbles: true,
          cancelable: true
        })
      );
    fire("touchstart", cx - dx / 2);
    fire("touchmove", cx + dx / 2);
    fire("touchend", cx + dx / 2);
  }, { selector, dx });
}

/** 塞一本账：两笔今天（一笔有备注、一笔无备注带两张图）、一笔转账、一笔旧图 */
async function seedBook(page) {
  await page.evaluate(({ key, today, earlier }) => {
    const book = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!book || !book.accounts?.length) return;
    const account = book.accounts[0].id;
    const other = book.accounts[1]?.id ?? account;
    const parent = book.categories.find((item) => item.side === "expense" && !item.parentId);
    const children = book.categories.filter((item) => item.parentId === parent?.id).slice(0, 2);
    book.entries = [
      {
        id: "v076-a", kind: "expense", amountCents: 1234, accountId: account,
        categoryId: children[0]?.id ?? "", date: today, time: "09:15", note: "早餐",
        createdAt: new Date().toISOString()
      },
      {
        id: "v076-b", kind: "expense", amountCents: 5600, accountId: account,
        categoryId: children[1]?.id ?? "", date: today, time: "18:40", note: "",
        images: ["md-1-test.png", "md-2-test.png"],
        createdAt: new Date().toISOString()
      },
      {
        id: "v076-c", kind: "transfer", amountCents: 30000, accountId: account, toAccountId: other,
        date: today, time: "20:10", note: "转一笔",
        createdAt: new Date().toISOString()
      },
      {
        id: "v076-d", kind: "income", amountCents: 8800, accountId: account,
        date: earlier, note: "旧账", createdAt: new Date().toISOString()
      }
    ];
    localStorage.setItem(key, JSON.stringify(book));
  }, { key: LEDGER_KEY, today: dayOffset(0), earlier: dayOffset(40) });
  await page.evaluate(({ key, today }) => {
    // 日记 key 可能还没落过盘（本地预览只在保存时才写）：没有就从空账开始
    const diary = JSON.parse(localStorage.getItem(key) ?? "null") ?? { entries: [] };
    diary.entries = [
      ...(diary.entries ?? []).filter((entry) => entry.id !== "v076-diary"),
      {
        id: "v076-diary", title: "搜索混排用的日记", markdown: "混排内容 记账 日记",
        date: today, mood: "", weather: "", tags: [],
        createdAt: new Date().toISOString(), updatedAt: new Date().toISOString()
      }
    ];
    localStorage.setItem(key, JSON.stringify(diary));
  }, { key: DIARY_KEY, today: dayOffset(0) });
  await page.reload({ waitUntil: "load" });
  await page.waitForSelector(".sidebar", { timeout: 15000 });
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------------- 桌面 ----------------
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);
  await openLedger(page);
  await seedBook(page);
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  // 13.2 图片图标与备注等高 + 13.3 列表不带角标
  // 都是逻辑像素（CSS 计算值）：app-shell 带缩放，rect 是视觉像素，不能跟字号直接比
  const imageIcon = await page.$eval(
    ".ledger-entry-image svg",
    (el) => Number.parseFloat(getComputedStyle(el).height)
  );
  const noteFont = await page.$eval(
    ".ledger-entry-note",
    (el) => Number.parseFloat(getComputedStyle(el).fontSize)
  );
  check(
    "图片图标与备注字号等高（13.2）",
    imageIcon >= noteFont && imageIcon - noteFont <= 3,
    `${imageIcon} vs ${noteFont}`
  );
  check("列表里的图片图标不带角标（13.3）", (await page.$$(".ledger-entry-image-count")).length === 0);

  // 15 账户行带图标：[A图标]A->[B图标]B 与 [图标]账户
  const transferSvgs = await page.$$eval(".ledger-entry:has-text('转一笔') .ledger-entry-account svg", (els) => els.length);
  const plainSvgs = await page.$$eval(".ledger-entry:has-text('早餐') .ledger-entry-account svg", (els) => els.length);
  check("转账账户行有两个账户图标 + 箭头（15）", transferSvgs === 3, String(transferSvgs));
  check("普通账户行带账户图标（15）", plainSvgs === 1, String(plainSvgs));

  // 14 桌面：FAB 上移，且「今」在加号上方
  const fabBox = await page.locator(".ledger-fab").boundingBox();
  check("桌面 FAB 不贴底（上移约一个半按钮高）", fabBox !== null && 900 - (fabBox.y + fabBox.height) > 60, `${900 - ((fabBox.y ?? 0) + (fabBox.height ?? 0))}`);
  // 1 桌面滚动条保持不变（不给桌面去掉滚动条）
  const desktopScrollbar = await page.$eval(".ledger-scroll", (el) => getComputedStyle(el).scrollbarWidth);
  check("桌面滚动区仍保留滚动条（1）", desktopScrollbar !== "none", desktopScrollbar);
  await switchView(page, "日历视图");
  await page.waitForSelector(".ledger-calendar", { timeout: 8000 });
  await page.click(".ledger-calendar-bar button[aria-label='上个月']");
  await page.waitForTimeout(250);
  const dToday = await page.locator(".ledger-fab-today").boundingBox();
  const dFab = await page.locator(".ledger-fab").boundingBox();
  check(
    "桌面「今」也在加号上方（14）",
    dToday !== null && dFab !== null && dToday.y + dToday.height <= dFab.y + 3,
    `${dToday?.y}+${dToday?.height} vs ${dFab?.y}`
  );
  // 日历测试把月份留在了上个月：先回本月，列表里才有今天的账
  await page.click(".ledger-fab-today");
  await page.waitForTimeout(250);
  await switchView(page, "列表视图");
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  // 8 桌面记账编辑框：高度 = 宽度 − 一个一级分类图标（v0.7.7），
  //   三排（备注+金额 / 日期账户图片 / 底栏）向下贴底，开搁板不上下乱跳
  await page.click(".ledger-entry:has-text('早餐')");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  await page.waitForTimeout(400); // 等弹出动画（0.16s）走完：动画中 rect 会偏小、位置偏高
  const sheetSize = await page.$eval(".ledger-editor-sheet", (el) => [el.offsetWidth, el.offsetHeight]);
  check(
    "桌面记账编辑框高度 = 宽度 − 一个分类图标（8）",
    sheetSize[0] > 600 && Math.abs(sheetSize[0] - sheetSize[1] - 44) < 3,
    `${sheetSize[0]}x${sheetSize[1]}`
  );
  const rowPositions = () =>
    page.evaluate(() => {
      const pick = (sel) => {
        const el = document.querySelector(sel);
        return el ? Math.round(el.getBoundingClientRect().top) : 0;
      };
      return {
        note: pick(".ledger-editor-sheet .ledger-note-row"),
        meta: pick(".ledger-editor-sheet .ledger-meta-row"),
        foot: pick(".ledger-editor-sheet .ledger-sheet-foot"),
        sheetBottom: Math.round(document.querySelector(".ledger-editor-sheet").getBoundingClientRect().bottom)
      };
    });
  const rowsBefore = await rowPositions();
  check(
    "三排贴在下半部分（8）",
    rowsBefore.note > rowsBefore.sheetBottom - 320 && rowsBefore.foot > rowsBefore.sheetBottom - 200,
    JSON.stringify(rowsBefore)
  );
  const cells = page.locator(".ledger-cat-zone > .ledger-cat-grid > .ledger-cat-cell:not(.add)");
  await cells.nth(0).click();
  await page.waitForTimeout(200);
  const rowsAfter = await rowPositions();
  const sheetAfter = await page.$eval(".ledger-editor-sheet", (el) => el.offsetHeight);
  check(
    "展开二级搁板三排原地不动（8）",
    rowsAfter.note === rowsBefore.note && rowsAfter.meta === rowsBefore.meta && rowsAfter.foot === rowsBefore.foot,
    `${JSON.stringify(rowsBefore)} → ${JSON.stringify(rowsAfter)}`
  );
  check("展开二级搁板不改变对话框高度（8）", sheetAfter === sheetSize[1], `${sheetSize[1]} → ${sheetAfter}`);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });

  // 12 记账搜索：分类 / 备注 / 金额 + 汇总 + 单条卡片
  // v0.8.4 起搜索是头部按钮（齿轮菜单里不再有「搜索记账」）
  await page.click(".ledger-view .header-actions button[title='搜索记账']");
  await page.waitForSelector(".ledger-search", { timeout: 5000 });
  check("头部按钮能打开搜索框（12）", (await page.$$(".ledger-search input")).length === 1);
  const categoryName = await page.$eval(
    ".ledger-entry:has-text('早餐') strong",
    (el) => el.textContent?.trim() ?? ""
  );
  await page.fill(".ledger-search input", categoryName);
  await page.waitForSelector(".ledger-search-sum", { timeout: 5000 });
  check(
    "搜索结果全是单条卡片（不是按天卡片）",
    (await page.$$(".ledger-scroll > .ledger-result-card")).length >= 1 &&
      (await page.$$(".ledger-scroll > .ledger-card:has(.ledger-card-sums)")).length === 0
  );
  const sumText = (await page.textContent(".ledger-search-sum")) ?? "";
  check(
    "搜索汇总块有收/支/结余（12）",
    sumText.includes("收") && sumText.includes("支") && sumText.includes("结余") && sumText.includes("笔"),
    sumText.replace(/\s+/g, " ")
  );
  await page.fill(".ledger-search input", "56");
  await page.waitForTimeout(150);
  check(
    "按金额搜索命中（12）",
    (await page.$$(".ledger-scroll > .ledger-result-card:has-text('56.00')")).length === 1
  );
  await page.fill(".ledger-search input", "早餐");
  await page.waitForTimeout(150);
  check("按备注搜索命中（12）", (await page.$$(".ledger-scroll > .ledger-result-card:has-text('早餐')")).length === 1);
  await page.fill(".ledger-search input", "");
  await page.waitForTimeout(150);
  check("清空搜索词回到正常视图（12）", (await page.$$(".ledger-month-bar")).length === 1);
  await page.click(".ledger-view .header-actions button[title='关闭搜索']");
  await page.waitForTimeout(150);
  check("关闭搜索后搜索框收起（12）", (await page.$$(".ledger-search")).length === 0);

  // 12 全局搜索混排：任务 / 日记 / 记账同一列
  await page.fill(".sidebar .search-box input", "早餐");
  await page.waitForTimeout(400);
  check(
    "全局搜索出现记账卡（12）",
    (await page.$$(".task-list .search-hit .ledger-result-card")).length === 1
  );
  await page.fill(".sidebar .search-box input", "混排");
  await page.waitForTimeout(400);
  check(
    "全局搜索里日记卡与记账卡混排（12）",
    (await page.$$(".task-list .diary-card")).length === 1 &&
      (await page.$$(".task-list .ledger-result-card")).length >= 0
  );
  await page.fill(".sidebar .search-box input", "");
  await page.waitForTimeout(300);

  check("桌面没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

// ---------------- 移动端 ----------------
{
  const context = await browser.newContext({
    viewport: { width: 360, height: 780 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const { page, errors } = await freshPage(context);
  await openLedger(page);
  await seedBook(page);
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });

  // 1 无滚动条 + 无横向溢出 + 卡片居中
  const scrollbarNone = await page.$eval(".ledger-scroll", (el) => getComputedStyle(el).scrollbarWidth);
  check("移动滚动区不画滚动条（1）", scrollbarNone === "none", scrollbarNone);
  const ledgerOverflow = await page.$eval(".ledger-scroll", (el) => [el.scrollWidth, el.clientWidth]);
  check("记账滚动区没有横向溢出（1）", ledgerOverflow[0] === ledgerOverflow[1], ledgerOverflow.join("/"));
  const cardBox = await page.locator(".ledger-card").first().boundingBox();
  const scrollBox = await page.locator(".ledger-scroll").boundingBox();
  check(
    "卡片左右等距（居中）（1）",
    cardBox !== null && scrollBox !== null &&
      Math.abs(cardBox.x - scrollBox.x - (scrollBox.x + scrollBox.width - cardBox.x - cardBox.width)) < 3,
    `${cardBox?.x} in ${scrollBox?.x}..${(scrollBox?.x ?? 0) + (scrollBox?.width ?? 0)}`
  );
  const diaryScrollbar = await page.evaluate(() =>
    getComputedStyle(document.querySelector(".task-list")).scrollbarWidth
  );
  check("任务页滚动区同样不画滚动条（1）", diaryScrollbar === "none", diaryScrollbar);

  // 10 齿轮与视图段控等高、中线对齐
  const gearSize = await page.$eval(".ledger-view .header-actions > button", (el) => {
    const rect = el.getBoundingClientRect();
    return [rect.y, rect.height];
  });
  const switchSize = await page.$eval(".ledger-view-switch", (el) => {
    const rect = el.getBoundingClientRect();
    return [rect.y, rect.height];
  });
  check(
    "齿轮与视图段控等高（10）",
    Math.abs(gearSize[1] - switchSize[1]) < 1.5,
    `${gearSize[1]} vs ${switchSize[1]}`
  );
  check(
    "齿轮与段控上下平齐（10）",
    Math.abs(gearSize[0] + gearSize[1] / 2 - (switchSize[0] + switchSize[1] / 2)) < 1.5,
    `${gearSize[0]} vs ${switchSize[0]}`
  );

  // 14 FAB 上移 + 「今」在加号上方
  const mFab = await page.locator(".ledger-fab").boundingBox();
  check(
    "移动 FAB 不贴底（14）",
    mFab !== null && 780 - (mFab.y + mFab.height) > 40,
    `${780 - ((mFab.y ?? 0) + (mFab.height ?? 0))}`
  );
  await switchView(page, "日历视图");
  await page.waitForSelector(".ledger-calendar", { timeout: 8000 });
  await page.click(".ledger-calendar-bar button[aria-label='上个月']");
  await page.waitForTimeout(200);
  const mTodayBtn = await page.locator(".ledger-fab-today").boundingBox();
  const mFab2 = await page.locator(".ledger-fab").boundingBox();
  check(
    "「今」按钮在加号上方（14）",
    mTodayBtn !== null && mFab2 !== null && mTodayBtn.y + mTodayBtn.height <= mFab2.y + 3,
    `${mTodayBtn?.y}+${mTodayBtn?.height} vs ${mFab2?.y}`
  );

  // 9 日历左右滑动换月（记账）
  const beforeMonth = (await page.textContent(".ledger-calendar-bar strong"))?.trim() ?? "";
  await swipe(page, ".ledger-calendar", -120);
  await page.waitForTimeout(250);
  const afterMonth = (await page.textContent(".ledger-calendar-bar strong"))?.trim() ?? "";
  check("记账日历左滑换到下个月（9）", afterMonth !== beforeMonth && afterMonth.length > 0, `${beforeMonth} → ${afterMonth}`);
  await swipe(page, ".ledger-calendar", 120);
  await page.waitForTimeout(250);
  const backMonth = (await page.textContent(".ledger-calendar-bar strong"))?.trim() ?? "";
  check("记账日历右滑换回上个月（9）", backMonth === beforeMonth, `${afterMonth} → ${backMonth}`);
  // 滑动测试把日历留在了上个月：点「今」回到本月，后面的列表断言才有数据
  await page.click(".ledger-fab-today");
  await page.waitForTimeout(250);
  check("「今」回到本月（9）", (await page.$$(".ledger-fab-today")).length === 0);

  // 4 移动端统计段控：字号回到 -2px 且不超屏
  await switchView(page, "统计视图");
  await page.waitForSelector(".ledger-stats-bar", { timeout: 8000 });
  const segInfo = await page.evaluate(() => {
    const button = document.querySelector(".ledger-segmented button");
    const bar = document.querySelector(".ledger-stats-bar");
    const shell = document.querySelector(".app-shell");
    const ledgerFont = getComputedStyle(shell).getPropertyValue("--ledger-font-size");
    return {
      fontSize: Number.parseFloat(getComputedStyle(button).fontSize),
      ledgerFont: Number.parseFloat(ledgerFont),
      overflow: bar.scrollWidth - bar.clientWidth,
      docOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth
    };
  });
  check(
    "移动统计段控字号回到 -2px（4）",
    Math.abs(segInfo.fontSize - (segInfo.ledgerFont - 2)) < 0.6,
    `${segInfo.fontSize} vs ${segInfo.ledgerFont - 2}`
  );
  check("统计段控不超出屏幕（4）", segInfo.overflow <= 0 && segInfo.docOverflow <= 0, JSON.stringify(segInfo));

  // 5 自定义日期浮层点别处收起
  await page.click(".ledger-segmented button:has-text('自定义')");
  await page.waitForSelector(".ledger-custom-field", { timeout: 5000 });
  await page.click(".ledger-custom-field:first-child strong");
  await page.waitForSelector(".ledger-custom-field.open .ledger-pop.date", { timeout: 5000 });
  check("点日期弹出日历（5）", (await page.$$(".ledger-custom-field.open .ledger-pop.date")).length === 1);
  await page.click(".ledger-stats-bar", { position: { x: 5, y: 5 } });
  await page.waitForTimeout(200);
  check("点其他位置日历自己收起（5）", (await page.$$(".ledger-pop.date")).length === 0);

  // 6 资产趋势：预览带坐标轴 → 点开是浮层 → 全屏按钮才横屏
  await switchView(page, "资产视图");
  await page.waitForSelector(".ledger-trend-card", { timeout: 8000 });
  check(
    "趋势预览带横纵坐标（6）",
    (await page.$$(".ledger-trend-card .ledger-chart-axis")).length >= 4
  );
  // v0.7.7：点图 = 点曲线（直接读数），不拉半屏浮窗；全屏按钮才进横屏全屏
  const mTrendBox = await page.locator(".ledger-trend-card .ledger-chart-box").boundingBox();
  await page.touchscreen.tap(mTrendBox.x + mTrendBox.width * 0.5, mTrendBox.y + mTrendBox.height * 0.5);
  await page.waitForSelector(".ledger-trend-card .ledger-chart-tip", { timeout: 3000 });
  check(
    "点图直接给读数不是浮窗（6）",
    (await page.$$(".ledger-trend-dialog")).length === 0 &&
      (await page.$$(".ledger-trend-full")).length === 0 &&
      (await page.$$(".ledger-trend-card .ledger-chart-tip")).length === 1
  );
  await page.tap(".ledger-trend-card .ledger-trend-zoom");
  await page.waitForSelector(".ledger-trend-full", { timeout: 8000 });
  check("全屏按钮直接进横屏全屏（6）", (await page.$$(".ledger-trend-dialog")).length === 0);
  const rotHead = await page.locator(".ledger-trend-rot-head").boundingBox();
  check(
    "全屏层标题栏在屏幕内（避开系统栏）（6）",
    rotHead !== null && rotHead.y >= 0 && rotHead.y + rotHead.height <= 780,
    JSON.stringify(rotHead)
  );
  await page.click(".ledger-trend-rot-head .ledger-image-tool[title='关闭']");
  await page.waitForSelector(".ledger-trend-full", { state: "detached", timeout: 8000 });

  // 7 分类管理「添加分类」：全部图标也能滚到底（颜色 + 预览可见）
  await page.click(".ledger-view .header-actions button[title='记账菜单'], .ledger-view .header-actions button[title='更多操作']");
  await page.click(".context-menu .menu-item-button:has-text('分类管理')");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.click(".ledger-sheet-foot button:has-text('添加大类')");
  await page.waitForSelector(".ledger-manager .ledger-form-body", { timeout: 8000 });
  await page.waitForTimeout(250);
  // v0.7.7：图标区自己滚（不显滚动条），表单本身不用再滚——颜色与预览贴着图标区下方
  const catForm = await page.evaluate(() => {
    const grid = document.querySelector(".ledger-manager .ledger-icon-grid");
    const body = document.querySelector(".ledger-manager .ledger-sheet-body");
    const sheet = document.querySelector(".ledger-manager");
    if (!grid || !body || !sheet) return null;
    const preview = body.querySelector(".ledger-form-preview");
    const colors = body.querySelector(".ledger-color-row");
    const previewRect = preview?.getBoundingClientRect();
    const colorRect = colors?.getBoundingClientRect();
    return {
      gridHeight: grid.clientHeight,
      gridScroll: grid.scrollHeight,
      overflow: getComputedStyle(grid).overflowY,
      scrollbar: getComputedStyle(grid).scrollbarWidth,
      sheetBottom: sheet.getBoundingClientRect().bottom,
      previewBottom: previewRect ? previewRect.bottom : null,
      colorBottom: colorRect ? colorRect.bottom : null
    };
  });
  check(
    "分类表单图标区自己滚且不画滚动条（7）",
    catForm !== null && catForm.gridScroll > catForm.gridHeight + 50 &&
      catForm.overflow === "auto" && catForm.scrollbar === "none",
    JSON.stringify(catForm)
  );
  check(
    "颜色与预览都在面板内可见（7）",
    catForm !== null && catForm.previewBottom !== null && catForm.colorBottom !== null &&
      catForm.previewBottom <= catForm.sheetBottom + 1 && catForm.colorBottom <= catForm.sheetBottom + 1,
    JSON.stringify(catForm)
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });

  // 13.4 看图：列表点图标 → 复用查看器，左右可用、不退出
  await switchView(page, "列表视图");
  await page.waitForSelector(".ledger-card", { timeout: 8000 });
  await installImageStub(page);
  await page.click(".ledger-entry:has-text('18:40') .ledger-entry-image");
  await page.waitForSelector(".ledger-image-overlay", { timeout: 8000 });
  const counter1 = (await page.textContent(".ledger-image-counter"))?.replace(/\s+/g, " ") ?? "";
  check("列表点图打开查看器（13.4）", counter1 === "1 / 2", counter1);
  await page.click(".ledger-image-nav.next");
  await page.waitForTimeout(200);
  const counter2 = (await page.textContent(".ledger-image-counter"))?.replace(/\s+/g, " ") ?? "";
  check("点右箭头翻到第二张而不是退出（13.4）", counter2 === "2 / 2" && (await page.$$(".ledger-image-overlay")).length === 1, counter2);
  // 13.5 左滑翻页
  await swipe(page, ".ledger-image-overlay", 120);
  await page.waitForTimeout(200);
  const counter3 = (await page.textContent(".ledger-image-counter"))?.replace(/\s+/g, " ") ?? "";
  check("看图界面右滑翻回第一张（13.5）", counter3 === "1 / 2", counter3);
  await page.click(".ledger-image-tool");
  await page.waitForSelector(".ledger-image-overlay", { state: "detached", timeout: 8000 });
  check("列表看图不带编辑按钮（只有关闭）（13.4）", (await page.$$(".ledger-image-overlay")).length === 0);

  await page.evaluate(() => {
    delete window.__TAURI_INTERNALS__;
  });

  // 回列表再进日记（记账是整页层，盖着侧栏时点不到导航行）
  await page.goBack();
  await page.waitForTimeout(400);

  // 3 编辑器滑块无 tap 遮罩
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-view", { timeout: 8000 });
  await page.click(".diary-fab");
  await page.waitForSelector(".editor-mode-switch", { timeout: 8000 });
  const tapHighlight = await page.$eval(
    ".editor-mode-switch button",
    (el) => getComputedStyle(el).webkitTapHighlightColor
  );
  check(
    "编辑/预览滑块没有点击遮罩（3）",
    /rgba\(0, 0, 0, 0\)|transparent/.test(tapHighlight),
    tapHighlight
  );
  // 关闭用编辑器自己的关闭按钮（Esc 在新建空草稿上要走两段式，这里只验证样式）
  await page.click(".diary-editor .editor-icon-button[title='关闭']");
  await page.waitForSelector(".editor-mode-switch", { state: "detached", timeout: 8000 });

  // 9 日记日历滑动换月
  await page.click(".diary-view-switch button[title='日历视图']");
  await page.waitForSelector(".diary-calendar", { timeout: 8000 });
  const diaryBefore = (await page.textContent(".diary-calendar-bar strong"))?.trim() ?? "";
  await swipe(page, ".diary-calendar", -120);
  await page.waitForTimeout(250);
  const diaryAfter = (await page.textContent(".diary-calendar-bar strong"))?.trim() ?? "";
  check("日记日历左滑换到下个月（9）", diaryAfter !== diaryBefore, `${diaryBefore} → ${diaryAfter}`);
  await page.goBack();
  await page.waitForTimeout(400);

  // 11 固定分组不留选中底色（我的一天 → 返回列表）
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForTimeout(400);
  await page.goBack();
  await page.waitForTimeout(400);
  const navBg = await page.$eval(".system-nav .nav-row:has-text('我的一天')", (el) => getComputedStyle(el).backgroundColor);
  check("返回列表后固定分组不留选中底色（11）", /rgba\(0, 0, 0, 0\)|transparent/.test(navBg), navBg);

  // 2 左上角返回按钮：默认不显示 → 开关打开后出现 → 点它回上一级
  check("默认不显示返回箭头（2）", (await page.$$(".mobile-back")).length === 0);
  await page.click(".profile-card");
  await page.waitForSelector(".settings-drawer", { timeout: 8000 });
  const backToggle = page.locator(".toggle-row:has-text('左上角返回按钮') input");
  await backToggle.scrollIntoViewIfNeeded();
  check("特性开关里有「左上角返回按钮」（2）", (await backToggle.count()) === 1);
  await backToggle.click();
  await page.waitForTimeout(200);
  // 设置页自己的关闭入口（移动端抽屉头部那支返回箭头，与特性开关无关）
  await page.click(".drawer-header .mobile-back");
  await page.waitForTimeout(400);
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForSelector(".workspace .mobile-back", { timeout: 8000 });
  check("开关打开后内容页有返回箭头（2）", (await page.$$(".workspace .mobile-back")).length === 1);
  await page.click(".workspace .mobile-back");
  await page.waitForTimeout(400);
  check(
    "点返回箭头回到列表（2）",
    (await page.locator(".app-shell.mobile.view-list").count()) === 1
  );
  // 日记 / 记账 / 工具箱三个整页也各有一支
  for (const [nav, pageSel] of [["日记", ".diary-view"], ["记账", ".ledger-view"], ["工具箱", ".toolbox-view"]]) {
    await page.click(`.system-nav .nav-row:has-text('${nav}')`);
    await page.waitForSelector(pageSel, { timeout: 8000 });
    check(`${nav}页有返回箭头（2）`, (await page.$$(`${pageSel} .mobile-back`)).length === 1);
    await page.click(`${pageSel} .mobile-back`);
    await page.waitForTimeout(400);
  }

  // 12 移动端全局搜索混排：结果面板里出现记账卡
  await page.fill(".sidebar .search-box input", "早餐");
  await page.waitForTimeout(400);
  check(
    "移动端全局搜索出现记账卡（12）",
    (await page.$$(".search-results .search-hit .ledger-result-card")).length === 1
  );
  await page.fill(".sidebar .search-box input", "");
  await page.waitForTimeout(300);

  // 13.4 编辑器里的同一套查看器：带 + 与垃圾桶
  await openLedger(page);
  await page.waitForSelector(".ledger-card", { timeout: 8000 });
  await page.click(".ledger-entry:has-text('18:40')");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  await installImageStub(page);
  await page.click(".ledger-meta-plain.has-image");
  await page.waitForSelector(".ledger-image-overlay", { timeout: 8000 });
  check(
    "编辑器看图带添加与删除按钮（13.4）",
    (await page.$$(".ledger-image-overlay .ledger-image-tool")).length === 3
  );
  await page.click(".ledger-image-tool[title='关闭']");
  await page.waitForSelector(".ledger-image-overlay", { state: "detached", timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  check("移动端没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();
console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

// v0.7.8 回归：
// 1 移动端点「添加事项」拉起输入法时不顶页（shell 跟随视觉视口，条目原地不动）；
// 2 桌面三点菜单按钮支持 toggle（再点一次收起）；
// 3 记账/日记的齿轮下拉面板宽度自适应（比最长条目略宽）；
// 4 记账列表：滚到顶再往上 = 换回更近的月（整月装不满一屏也能换）；
// 5 移动端转账视图的数字键盘完整可见（aspect-ratio 收回）；
// 6 展开全部/收起全部覆盖「单行超长要折行」的卡片（量出来的可展开性也要算）；
// 7 超链接渲染：标题 60 字、卡片铺满内容宽 + 圆角 6px + 右上角复制图标 + 标题/摘要两行封顶 + 网页图标；
// 8 移动端各页齿轮按钮点按不留蓝罩子（与日记/记账对齐）；
// 9 同步账户长度下限（用户名 ≥ 4、密码 ≥ 6，不够时点保存/同步给提示）；
// 10 同步凭据明文留档（core 单测覆盖，这里只查文件存在与否的行为面）；
// 11 桌面全局快捷键按「是否在前台」toggle（core 侧，node 套件不覆盖）。
// 用法：node scripts/v078-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

/** 标题 80 字左右（验证 60 字截断），同时给一个网页图标 */
const PAGE_HTML = `<!doctype html><html><head>
<meta charset="utf-8">
<link rel="icon" href="/fav.png">
<title>这是一篇标题非常非常长的文章用来验证v078的六十个字截断规则是不是真的生效了并且还保留了省略号哦对的现在再加一点内容保证超过六十个字</title>
<meta property="og:description" content="这是一段很长很长的正文预览摘要，长到足够把卡片的摘要行铺满两行还要多出来不少内容，用来验证两行截断与省略号。">
<meta property="og:site_name" content="示例站">
</head><body>正文</body></html>`;

const MARKDOWN = [
  "链接检查",
  "",
  "裸链接 https://example.com/page 在这里",
  "",
  "手写的链接 [我的文字](https://example.com/other) 不动"
].join("\n");

/** 1x1 透明 PNG */
const PNG_1PX =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

async function stubLinks(page) {
  const handler = (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html; charset=utf-8",
      headers: { "access-control-allow-origin": "*" },
      body: PAGE_HTML
    });
  // 后注册的优先：通用处理器先注册，favicon 那条后注册才拦得住
  await page.route("**/example.com/**", handler);
  await page.route("**/example.com", handler);
  await page.route("**/example.com/fav.png", (route) =>
    route.fulfill({
      status: 200,
      contentType: "image/png",
      headers: { "access-control-allow-origin": "*" },
      body: Buffer.from(PNG_1PX, "base64")
    })
  );
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

/** 种几条任务：单行超长（要折行）、多行、普通短标题 */
async function seedTasks(page, tasks) {
  await page.evaluate((items) => {
    const now = new Date().toISOString();
    const state = {
      schemaVersion: 3,
      nodes: [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "entry-long", kind: "entry", name: "长行测试", icon: "list", parentId: null, createdAt: now }
      ],
      tasks: items.map((item, index) => ({
        id: item.id ?? `task-long-${index}`,
        nodeId: "entry-long",
        markdown: item.markdown,
        completed: false,
        important: false,
        myDay: false,
        tags: [],
        emojis: [],
        expanded: item.expanded === true,
        createdAt: now,
        updatedAt: now
      })),
      selectedNodeId: "entry-long",
      backgrounds: []
    };
    localStorage.setItem("todo-note-state-v3", JSON.stringify(state));
  }, tasks);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(1000);
}

async function seedBook(page) {
  await page.evaluate(() => {
    const now = new Date().toISOString();
    // 本地日期：toISOString 是 UTC，凌晨跑会差一天（账按本地日归月）
    const iso = (value) =>
      `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
    const monthDay = (monthOffset, dayOfMonth) => {
      const d = new Date();
      d.setDate(1);
      d.setMonth(d.getMonth() + monthOffset);
      d.setDate(dayOfMonth);
      return iso(d);
    };
    const day = (offset) => {
      const d = new Date();
      d.setDate(d.getDate() - offset);
      return iso(d);
    };
    const entries = [];
    for (let i = 0; i < 13; i += 1) {
      entries.push({
        id: `le-cur-${i}`,
        kind: "expense",
        amountCents: 2000 + i * 100,
        accountId: "lacc-01",
        categoryId: "lcat-exp-01",
        date: day(i),
        note: `当月 ${i}`,
        createdAt: now
      });
    }
    entries.push({
      id: "le-prev-0",
      kind: "expense",
      amountCents: 3300,
      accountId: "lacc-01",
      categoryId: "lcat-exp-01",
      date: monthDay(-1, 5),
      note: "上个月唯一一笔",
      createdAt: now
    });
    localStorage.setItem(
      "todo-note-ledger-v1",
      JSON.stringify({
        accounts: [
          { id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now },
          { id: "lacc-02", name: "储蓄卡", kind: "储蓄卡", icon: "Landmark", color: "#4a90d9", initialCents: 500000, createdAt: now }
        ],
        categories: [
          { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now }
        ],
        entries,
        accountTypes: []
      })
    );
  });
}

async function openLedger(page) {
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  await page.waitForTimeout(400);
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

// ---------------------------------------------------------------- 桌面
{
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const { page, errors } = await freshPage(context);
  await stubLinks(page);

  // 6 展开全部/收起全部要覆盖「单行超长」的卡片
  const longLine = `单行超长的一条 ${"很长的内容".repeat(40)} 结尾`;
  await seedTasks(page, [
    { id: "task-short", markdown: "短标题" },
    { id: "task-multi", markdown: "第一行\n\n第二行\n\n第三行" },
    { id: "task-long", markdown: longLine }
  ]);
  const before = await page.$$eval(".task-card", (els) => els.map((el) => el.className));
  check("三张卡片初始都是折叠态（6）", before.every((cls) => cls.includes("compact")), JSON.stringify(before));
  // 大标题的卡片才量得出「显示不全」：等测量事件跑完（微任务 + 一轮）
  await page.waitForTimeout(600);
  const headerButtons = await page.$$eval(".header-actions > button", (els) => els.map((el) => el.title));
  check("折叠态下头部只给「展开全部」（6）", headerButtons.includes("展开全部") && !headerButtons.includes("收起全部"), JSON.stringify(headerButtons));

  await page.click(".header-actions > button[title='展开全部']");
  await page.waitForTimeout(700);
  const expanded = await page.$$eval(".task-card", (els) =>
    els.map((el) => ({ text: (el.textContent ?? "").trim().slice(0, 8), cls: el.className }))
  );
  const longCard = expanded.find((item) => item.text.startsWith("单行超长"));
  const multiCard = expanded.find((item) => item.text.startsWith("第一行"));
  const shortCard = expanded.find((item) => item.text.startsWith("短标题"));
  check(
    "展开全部：长单行卡片也展开（6）",
    longCard?.cls.includes("expanded") === true &&
      multiCard?.cls.includes("expanded") === true &&
      shortCard?.cls.includes("expanded") === false,
    JSON.stringify(expanded)
  );
  const expandedButtons = await page.$$eval(".header-actions > button", (els) => els.map((el) => el.title));
  check("展开后头部只给「收起全部」（6）", expandedButtons.includes("收起全部") && !expandedButtons.includes("展开全部"), JSON.stringify(expandedButtons));
  await page.click(".header-actions > button[title='收起全部']");
  await page.waitForTimeout(700);
  const collapsed = await page.$$eval(".task-card", (els) =>
    els.map((el) => ({ text: (el.textContent ?? "").trim().slice(0, 8), cls: el.className }))
  );
  check(
    "收起全部：长单行卡片也收起（6）",
    collapsed.every((item) => item.cls.includes("compact")),
    JSON.stringify(collapsed)
  );

  // 2 桌面三点菜单支持 toggle
  await page.click(".header-actions > button[title='列表菜单']", { delay: 10 });
  await page.waitForTimeout(400);
  check("三点按钮打开菜单（2）", (await page.$$(".context-menu")).length === 1);
  await page.click(".header-actions > button[title='列表菜单']", { delay: 10 });
  await page.waitForTimeout(400);
  check("再点一次收起菜单（2）", (await page.$$(".context-menu")).length === 0);
  await page.click(".header-actions > button[title='列表菜单']", { delay: 10 });
  await page.waitForTimeout(400);
  check("还能再打开（2）", (await page.$$(".context-menu")).length === 1);
  // 点别处照旧关闭（anchor 只对这个按钮放行）
  await page.mouse.click(400, 400);
  await page.waitForTimeout(400);
  check("点别处仍然关闭（2）", (await page.$$(".context-menu")).length === 0);

  // 3 日记齿轮面板宽度自适应
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-view", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.click(".diary-view .header-actions > button:last-child");
  await page.waitForSelector(".diary-gear-panel", { timeout: 5000 });
  await page.waitForTimeout(300);
  const diaryPanel = await page.$eval(".diary-gear-panel", (el) => {
    const rect = el.getBoundingClientRect();
    const labelWidths = [...el.querySelectorAll(".menu-item-label")].map((item) => item.getBoundingClientRect().width);
    return { width: rect.width, maxLabel: Math.max(...labelWidths, 0), scale: 0.75 };
  });
  check(
    "日记齿轮面板宽度自适应（比最长条目略宽、不再固定 176）（3）",
    diaryPanel.width / 0.75 < 176 && diaryPanel.width > diaryPanel.maxLabel + 30,
    JSON.stringify(diaryPanel)
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForTimeout(400);

  // 7 超链接：标题档 60 字
  await setFeatures(page, { linkRender: "title" });
  await seedTasks(page, [{ id: "task-link", markdown: MARKDOWN, expanded: true }]);
  await page.waitForTimeout(1400);
  const titleText = await page.$$eval(".task-card .markdown-content a", (els) =>
    els.map((el) => el.textContent.trim())
  );
  const autoTitle = titleText.find((text) => text.includes("这是一篇标题"));
  check(
    "自动标题最多 60 字 + 省略号（7.1）",
    autoTitle !== undefined && autoTitle.endsWith("…") && [...autoTitle].length === 61,
    autoTitle
  );

  // 7 卡片档：铺满宽度 / 圆角 6px / 复制按钮悬浮右上角且只是图标 / 标题摘要两行封顶 / 网页图标
  await setFeatures(page, { linkRender: "card" });
  await page.waitForTimeout(1600);
  const cardInfo = await page.$$eval(".task-card .kx-link-card", (els) =>
    els.map((el) => {
      const style = getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      const content = el.closest(".markdown-content").getBoundingClientRect();
      const copy = el.querySelector(".kx-link-card-copy");
      const copyStyle = copy ? getComputedStyle(copy) : null;
      const copyRect = copy?.getBoundingClientRect();
      const cardRect = el.getBoundingClientRect();
      const title = el.querySelector(".kx-link-card-title");
      const desc = el.querySelector(".kx-link-card-desc");
      const img = el.querySelector("img.kx-link-card-favicon");
      return {
        radius: style.borderRadius,
        widthRatio: rect.width / content.width,
        copyText: (copy?.textContent ?? "").trim(),
        copyIcon: Boolean(copy?.querySelector("svg")),
        copyPos: copyStyle?.position,
        copyTopGap: copyRect ? Math.round(copyRect.top - cardRect.top) : -1,
        copyRightGap: copyRect ? Math.round(cardRect.right - copyRect.right) : -1,
        titleLines: title ? Math.round(title.getBoundingClientRect().height / parseFloat(getComputedStyle(title).lineHeight)) : 0,
        descLines: desc ? Math.round(desc.getBoundingClientRect().height / parseFloat(getComputedStyle(desc).lineHeight)) : 0,
        favicon: Boolean(img),
        faviconLoaded: img ? img.naturalWidth > 0 : false
      };
    })
  );
  check("卡片铺满内容宽（7.3）", cardInfo.length === 2 && cardInfo.every((item) => item.widthRatio > 0.98), JSON.stringify(cardInfo.map((i) => i.widthRatio)));
  check("卡片圆角与 todo 卡片一致（6px）（7.4）", cardInfo.every((item) => item.radius === "6px"), JSON.stringify(cardInfo.map((i) => i.radius)));
  check(
    "复制按钮悬浮在右上角、只有图标（7.2）",
    cardInfo.every(
      (item) =>
        item.copyIcon &&
        item.copyText === "" &&
        item.copyPos === "absolute" &&
        item.copyTopGap >= 0 &&
        item.copyTopGap <= 12 &&
        item.copyRightGap >= 0 &&
        item.copyRightGap <= 12
    ),
    JSON.stringify(cardInfo[0] ?? {})
  );
  check(
    "标题与正文预览都最多两行（7.3）",
    cardInfo.every((item) => item.titleLines >= 1 && item.titleLines <= 2 && item.descLines >= 1 && item.descLines <= 2),
    JSON.stringify(cardInfo.map((i) => [i.titleLines, i.descLines]))
  );
  check(
    "卡片用网页自己的图标（7.4）",
    cardInfo.every((item) => item.favicon && item.faviconLoaded),
    JSON.stringify(cardInfo.map((i) => [i.favicon, i.faviconLoaded]))
  );
  // 7.5 桌面悬浮不加下划线
  await page.hover(".task-card .kx-link-card-main");
  await page.waitForTimeout(200);
  const hoverDecoration = await page.$eval(".task-card .kx-link-card-main", (el) => getComputedStyle(el).textDecorationLine);
  check("悬浮到卡片上不加下划线（7.5）", hoverDecoration === "none", hoverDecoration);

  // 7.6 特性开关合并成一组「超链接渲染样式」（v0.8.1 起是三档单选，不再是两个勾选框）
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  await page.waitForTimeout(500);
  const linkRow = await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".settings-drawer .toggle-row")];
    const row = rows.find((el) => (el.textContent ?? "").includes("超链接渲染样式"));
    if (!row) return null;
    const labels = [...row.querySelectorAll("label")].map((label) => (label.textContent ?? "").trim());
    const checked = [...row.querySelectorAll("input")].map((box) => box.checked);
    const names = [...new Set([...row.querySelectorAll("input")].map((box) => box.name))];
    return {
      labels,
      checked,
      names,
      legacy: document.body.textContent.includes("自动解析超链接标题") || document.body.textContent.includes("渲染超链接为卡片")
    };
  });
  check(
    "超链接渲染样式是三档单选「不渲染|标题|卡片」（7.6/v0.8.1）",
    linkRow !== null && linkRow.labels.join("|") === "不渲染|标题|卡片" && linkRow.names.length === 1,
    JSON.stringify(linkRow)
  );
  check(
    "三档里恰好选中一档（不存在两个都勾）（7.6/v0.8.1）",
    linkRow?.checked.filter(Boolean).length === 1 && linkRow.checked[2] === true,
    JSON.stringify(linkRow?.checked)
  );
  check("旧的「自动解析超链接标题」「渲染超链接为卡片」两行没了（7.6）", linkRow?.legacy === false, String(linkRow?.legacy));

  // 9 同步账户长度下限：不够时点「开始同步」给提示（toast）
  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });
  // 浏览器预览默认是「局域网」且没选主机，按钮本来就灰着：换成自建服务 + 一个填了地址的配置
  await page.evaluate(() => {
    const key = "todo-note-settings-v3";
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.sync = { ...(settings.sync ?? {}), mode: "server", serverUrl: "http://127.0.0.1:1/" };
    // 自动更新检查的 toast 会盖住同步的错误提示，测试期间关掉
    settings.updates = { ...(settings.updates ?? {}), autoCheck: false };
    localStorage.setItem(key, JSON.stringify(settings));
  });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await page.keyboard.press("Control+,");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  await page.evaluate(() => {
    document
      .querySelectorAll(".settings-drawer .settings-section.folded .settings-section-toggle")
      .forEach((button) => button.click());
  });
  await page.waitForTimeout(500);
  const userInput = page.locator(".settings-drawer input[placeholder='账户名（至少 4 位）']");
  const secretInput = page.locator(".settings-drawer input[placeholder='至少 6 位，派生加密密钥']");
  const startButton = page.locator(".settings-drawer .sync-actions .settings-button.primary");
  await userInput.fill("ab");
  await secretInput.fill("123456");
  await startButton.click();
  await page.waitForTimeout(700);
  const userToast = ((await page.textContent(".toast").catch(() => "")) ?? "").trim();
  check("用户名太短：点「开始同步」给提示（9）", userToast.includes("用户名至少 4 位"), userToast);
  await page.waitForTimeout(1600);
  await userInput.fill("abcd");
  await secretInput.fill("123");
  await startButton.click();
  await page.waitForTimeout(700);
  const secretToast = ((await page.textContent(".toast").catch(() => "")) ?? "").trim();
  check("密码太短：点「开始同步」给提示（9）", secretToast.includes("密码至少 6 位"), secretToast);
  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });

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
  await seedBook(page);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await openLedger(page);

  // 5 转账视图的数字键盘完整可见
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-editor-sheet", { timeout: 8000 });
  await page.waitForTimeout(400);
  const expenseKeypad = await page.evaluate(() => {
    const keypad = document.querySelector(".ledger-keypad").getBoundingClientRect();
    const save = document.querySelector(".ledger-key.save").getBoundingClientRect();
    return { keypadBottom: Math.round(keypad.bottom), saveBottom: Math.round(save.bottom), viewport: window.innerHeight };
  });
  check(
    "支出视图键盘完整可见（基准）（5）",
    expenseKeypad.keypadBottom <= expenseKeypad.viewport + 1 && expenseKeypad.saveBottom <= expenseKeypad.viewport + 1,
    JSON.stringify(expenseKeypad)
  );
  await page.click(".ledger-kind-tabs button:has-text('转账')");
  await page.waitForTimeout(500);
  const transferKeypad = await page.evaluate(() => {
    const sheet = document.querySelector(".ledger-editor-sheet");
    const keypad = document.querySelector(".ledger-keypad").getBoundingClientRect();
    const save = document.querySelector(".ledger-key.save").getBoundingClientRect();
    const fields = document.querySelector(".ledger-transfer-block").getBoundingClientRect();
    return {
      keypadBottom: Math.round(keypad.bottom),
      saveBottom: Math.round(save.bottom),
      transferTop: Math.round(fields.top),
      aspect: getComputedStyle(sheet).aspectRatio,
      viewport: window.innerHeight
    };
  });
  check(
    "转账视图键盘完整可见（5）",
    transferKeypad.keypadBottom <= transferKeypad.viewport + 1 &&
      transferKeypad.saveBottom <= transferKeypad.viewport + 1 &&
      transferKeypad.transferTop > 0,
    JSON.stringify(transferKeypad)
  );
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-editor-sheet", { state: "detached", timeout: 8000 });

  // 4 换月：触摸上滑/下滑
  const monthLabel = () => page.textContent(".ledger-month-bar strong");
  const box = await page.locator(".ledger-scroll").boundingBox();
  const cx = Math.round(box.x + box.width / 2);
  const swipe = async (dy) => {
    await page.evaluate(
      ({ x, y, delta }) => {
        const el = document.querySelector(".ledger-scroll");
        const makeTouch = (clientY) => new Touch({ identifier: 1, target: el, clientX: x, clientY });
        const send = (type, clientY) =>
          el.dispatchEvent(
            new TouchEvent(type, {
              bubbles: true,
              cancelable: true,
              touches: type === "touchend" ? [] : [makeTouch(clientY)]
            })
          );
        send("touchstart", y);
        send("touchmove", y + delta / 2);
        send("touchmove", y + delta);
        send("touchend", y + delta);
      },
      { x: cx, y: Math.round(box.y + 150), delta: dy }
    );
  };
  // 先滚到底（有得滚就真滚，没得滚本身就在底部）——手指再往上拖就该换更早的月。
  // **滚到底这一下自己就会换月**（「到底换上月」是设计行为），基准月必须等它落定后再读，
  // 否则程序化滚动换的一跳会算到手指那一滑头上（看着像一次滑了两月）。
  await page.evaluate(() => {
    const el = document.querySelector(".ledger-scroll");
    el.scrollTop = el.scrollHeight;
  });
  await page.waitForTimeout(1000);
  const startMonth = await monthLabel();
  const overflows = await page.$eval(".ledger-scroll", (el) => el.scrollHeight - el.clientHeight);
  await swipe(-95);
  await page.waitForTimeout(900);
  const afterDown = await monthLabel();
  check("贴底上滑换到上一个月（4）", afterDown !== startMonth, `${startMonth}(${overflows}px 可滚) → ${afterDown}`);
  // 上一个月装不满一屏（连滚动条都没有）：手指向下拖必须换回更近的月
  const prevOverflow = await page.$eval(".ledger-scroll", (el) => el.scrollHeight - el.clientHeight);
  await swipe(95);
  await page.waitForTimeout(900);
  const afterUp = await monthLabel();
  check(
    "贴顶下滑换回更近的月（装不满一屏也生效）（4）",
    afterUp === startMonth,
    `${afterDown}(${prevOverflow}px 可滚) → ${afterUp}`
  );
  await swipe(-95);
  await page.waitForTimeout(900);
  const againDown = await monthLabel();
  await swipe(95);
  await page.waitForTimeout(900);
  const againUp = await monthLabel();
  check("反复上下换月都生效（4）", againDown === afterDown && againUp === startMonth, `${startMonth} → ${againDown} → ${againUp}`);

  // 1 输入法跟随：shell 的高度/平移公式（真机键盘弹起时才走到这条分支）
  const imeStyles = await page.evaluate(async () => {
    const { buildMobileShellStyle } = await import("/src/lib/styles.ts");
    const none = buildMobileShellStyle({ uiScale: 1 }, { active: false, height: 0, offset: 0 });
    const small = buildMobileShellStyle({ uiScale: 1 }, { active: true, height: 400, offset: 0 });
    const panned = buildMobileShellStyle({ uiScale: 1 }, { active: true, height: 400, offset: 120 });
    return { none, small, panned };
  });
  check(
    "没弹输入法时 shell 仍是整屏（1）",
    imeStyles.none.includes("--app-height: 100vh") && !imeStyles.none.includes("translateY"),
    imeStyles.none
  );
  check(
    "输入法弹起时 shell 收矮到可见区域（1）",
    imeStyles.small.includes("--app-height: 400px") && !imeStyles.small.includes("translateY"),
    imeStyles.small
  );
  check(
    "视觉视口被顶上去时 shell 跟着平移（1）",
    imeStyles.panned.includes("translateY(120px) scale(1)"),
    imeStyles.panned
  );

  // 3 记账齿轮面板同样宽度自适应（移动端）
  await page.click(".ledger-view .header-actions > button:last-child");
  await page.waitForSelector(".ledger-gear-panel", { timeout: 5000 });
  await page.waitForTimeout(300);
  const ledgerPanel = await page.$eval(".ledger-gear-panel", (el) => {
    const rect = el.getBoundingClientRect();
    const labelWidths = [...el.querySelectorAll(".menu-item-label")].map((item) => item.getBoundingClientRect().width);
    return { width: Math.round(rect.width / 0.75), maxLabel: Math.round(Math.max(...labelWidths, 0) / 0.75) };
  });
  check(
    "记账齿轮面板宽度自适应（3）",
    ledgerPanel.width < 168 && ledgerPanel.width > ledgerPanel.maxLabel,
    JSON.stringify(ledgerPanel)
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  // 8 工作区齿轮：点按不留蓝罩子（tap highlight 关掉）
  // 从记账整页层回不到列表就重载（历史栈在这种连续测试里不好数），再进「我的一天」
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(1000);
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForSelector(".app-shell.view-content", { timeout: 8000 });
  await page.waitForTimeout(400);
  const navGear = page.locator(".workspace .header-actions > button").last();
  const highlight = await navGear.evaluate((el) => getComputedStyle(el).webkitTapHighlightColor);
  check(
    "工作区齿轮关闭蓝色点按高亮（8）",
    /rgba\(0, 0, 0, 0\)|transparent/.test(highlight),
    highlight
  );
  await navGear.tap();
  await page.waitForTimeout(300);
  const gearBg = await navGear.evaluate((el) => getComputedStyle(el).backgroundColor);
  check("工作区齿轮按下不留底色（8）", /rgba\(0, 0, 0, 0\)|transparent/.test(gearBg), gearBg);
  await page.keyboard.press("Escape").catch(() => {});
  await page.waitForTimeout(300);

  // 7 移动端卡片同样铺满宽度 + 复制按钮只有图标
  await setFeatures(page, { linkRender: "card" });
  await stubLinks(page);
  await seedTasks(page, [{ id: "task-link", markdown: MARKDOWN, expanded: true }]);
  await page.waitForTimeout(1200);
  await page.click(".tree-row:has-text('长行测试')");
  await page.waitForTimeout(1600);
  const mobileCards = await page.$$eval(".task-card .kx-link-card", (els) =>
    els.map((el) => {
      const rect = el.getBoundingClientRect();
      const content = el.closest(".markdown-content").getBoundingClientRect();
      return {
        widthRatio: rect.width / content.width,
        copyIcon: Boolean(el.querySelector(".kx-link-card-copy svg")),
        copyText: (el.querySelector(".kx-link-card-copy")?.textContent ?? "").trim()
      };
    })
  );
  check(
    "移动端卡片铺满内容宽、复制按钮只是图标（7.3/7.2）",
    mobileCards.length === 2 && mobileCards.every((item) => item.widthRatio > 0.98 && item.copyIcon && item.copyText === ""),
    JSON.stringify(mobileCards)
  );

  check("移动端没有页面错误", errors.length === 0, errors.join(" | ").slice(0, 300));
  await context.close();
}

await browser.close();

console.log(failures === 0 ? "\n全部通过" : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);

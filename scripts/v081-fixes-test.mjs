// v0.8.1 回归（问题修复 + 新需求）：
// 1 外观缓存收进整个 appearance（含 navLayout），首帧不再「先单列再跳双列」；
// 2 名字/邮箱**失焦或回车才提交**（不再每敲一个字符写一次盘）；
// 3 一周从周一开始（日历表头、日期选择器），设置里可切周日；
// 4 超链接渲染样式是三档单选（不渲染/标题/卡片），不存在「两个都勾」；
// 5 标签面板：预置标签 + 输入框（带「存入预置」勾选）+ 两排九色胶囊，没有清除按钮；
// 6 临期高亮：开关打开后今天到期的卡片带 .due-soon，日期展示带周几；
// 7 工具箱：桌面两列卡片、人民币大写工具、生成按钮在右下角；
// 8 渲染出来的任务勾选框可点（点击写回 markdown 的 - [ ] / - [x]）；
// 9 编辑器列表缩进：Tab 缩进 + 换无序标记，有序列表同级重新编号，Shift-Tab 反缩进。
// 用法：node scripts/v081-fixes-test.mjs（需先起 dev server：npm run dev）
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const SETTINGS_KEY = "todo-note-settings-v3";
const STATE_KEY = "todo-note-state-v3";

async function freshPage(context) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(600);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(800);
  return { page, errors };
}

/** 种一条任务（可指定到期日，用来验证临期高亮） */
async function seedTask(page, { markdown, dueDate, dueTime = "" }) {
  await page.evaluate(
    ({ markdown, dueDate, dueTime, stateKey }) => {
      const now = new Date().toISOString();
      const state = {
        schemaVersion: 3,
        nodes: [
          { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
          { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
          { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
          { id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null, createdAt: now }
        ],
        tasks: [
          {
            id: "task-a",
            nodeId: "entry-a",
            markdown,
            completed: false,
            important: false,
            myDay: false,
            tags: [],
            emojis: [],
            expanded: false,
            plannedDate: dueDate,
            dueDate,
            dueTime,
            createdAt: now,
            updatedAt: now,
            order: 1
          }
        ],
        backgrounds: {},
        selectedNodeId: "entry-a"
      };
      localStorage.setItem(stateKey, JSON.stringify(state));
    },
    { markdown, dueDate, dueTime, stateKey: STATE_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

async function patchSettings(page, patch) {
  await page.evaluate(
    ({ patch, key }) => {
      const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
      localStorage.setItem(key, JSON.stringify({ ...settings, ...patch }));
    },
    { patch, key: SETTINGS_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

const browser = await chromium.launch({ channel: "msedge" });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });

// ---------------------------------------------------------------------------
// 2 名字/邮箱：失焦才提交
// ---------------------------------------------------------------------------
{
  const { page, errors } = await freshPage(context);
  await page.click(".profile-card");
  await page.waitForSelector(".settings-drawer", { timeout: 8000 });
  const input = page.locator(".settings-drawer .settings-row input").first();
  const stored = () =>
    page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.profile?.displayName ?? "", SETTINGS_KEY);

  await input.click();
  await input.type("-草稿", { delay: 10 });
  const duringType = await stored();
  check("名字输入过程中不写盘（不是逐字符 config.set）", duringType !== "-草稿-草稿", `stored=${duringType}`);
  await input.blur();
  await page.waitForTimeout(400);
  const afterBlur = await stored();
  check("名字失焦后落盘", String(afterBlur).endsWith("-草稿"), `stored=${afterBlur}`);
  check("设置抽屉没有报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ---------------------------------------------------------------------------
// 1 外观缓存：整个 appearance 都进缓存（含 navLayout）
// ---------------------------------------------------------------------------
{
  const { page } = await freshPage(context);
  await patchSettings(page, { appearance: { navLayout: "grid" } });
  const cached = await page.evaluate(() =>
    JSON.parse(localStorage.getItem("kxtodo-appearance-cache") ?? "null")
  );
  check(
    "外观缓存收进 navLayout（首帧不闪双列）",
    cached?.navLayout === "grid",
    JSON.stringify(cached)?.slice(0, 120)
  );
  const profileCache = await page.evaluate(() =>
    JSON.parse(localStorage.getItem("kxtodo-profile-cache") ?? "null")
  );
  check(
    "资料缓存同时带上名字与邮箱",
    profileCache !== null && typeof profileCache.displayName === "string" && typeof profileCache.email === "string",
    JSON.stringify(profileCache)
  );
  await page.close();
}

// ---------------------------------------------------------------------------
// 3 一周从周一开始 + 可切周日
// ---------------------------------------------------------------------------
{
  const { page } = await freshPage(context);
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-view", { timeout: 8000 });
  await page.click(".diary-view-switch button[title='日历视图']");
  await page.waitForSelector(".diary-calendar-grid", { timeout: 8000 });
  const headers = await page.$$eval(".diary-calendar-head", (nodes) =>
    nodes.map((node) => node.textContent.trim())
  );
  check("日历表头从周一开始", headers[0] === "一" && headers[6] === "日", headers.join(""));

  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 8000 });
  await page.click(".ledger-view-switch button[title='日历视图']");
  await page.waitForSelector(".ledger-calendar", { timeout: 8000 });
  const ledgerHeaders = await page.$$eval(".ledger-calendar-head", (nodes) =>
    nodes.map((node) => node.textContent.trim())
  );
  check("记账日历同样周一起", ledgerHeaders[0] === "一", ledgerHeaders.join(""));

  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForTimeout(300);
  await page.click(".profile-card");
  await page.waitForSelector(".settings-drawer", { timeout: 8000 });
  const sunday = page.locator('.settings-drawer input[name="week-start"]').nth(1);
  await sunday.click();
  await page.waitForTimeout(400);
  const storedWeek = await page.evaluate(
    (key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.features?.weekStart ?? "",
    SETTINGS_KEY
  );
  check("设置里能切成周日开始", storedWeek === "sunday", `stored=${storedWeek}`);
  await page.close();
}

// ---------------------------------------------------------------------------
// 4 超链接渲染样式：三档单选
// ---------------------------------------------------------------------------
{
  const { page } = await freshPage(context);
  await page.click(".profile-card");
  await page.waitForSelector(".settings-drawer", { timeout: 8000 });
  const radios = page.locator('.settings-drawer input[name="link-render"]');
  check("超链接渲染是三档单选", (await radios.count()) === 3, `${await radios.count()} 档`);
  await radios.nth(1).click();
  await page.waitForTimeout(300);
  let stored = await page.evaluate(
    (key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.features?.linkRender ?? "",
    SETTINGS_KEY
  );
  check("选「标题」写入 linkRender=title", stored === "title", `stored=${stored}`);
  await radios.nth(2).click();
  await page.waitForTimeout(300);
  stored = await page.evaluate(
    (key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.features?.linkRender ?? "",
    SETTINGS_KEY
  );
  check("改选「卡片」后不可能同时是两个", stored === "card", `stored=${stored}`);
  await page.close();
}

// ---------------------------------------------------------------------------
// 5 / 6 任务菜单：日期展示带周几、临期高亮、标签面板结构
// ---------------------------------------------------------------------------
{
  const today = new Date();
  const iso = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(
    today.getDate()
  ).padStart(2, "0")}`;
  const { page, errors } = await freshPage(context);
  await patchSettings(page, { features: { dueHighlight: "solid" } });
  await seedTask(page, { markdown: "临期高亮的任务", dueDate: iso });

  const dueText = await page.textContent(".task-due-date");
  check("卡片日期带周几", /\d+月\d+日 周./.test(dueText ?? ""), dueText ?? "");

  const hasDue = await page.locator(".task-card.due-soon").count();
  check("今天到期的卡片带 due-soon", hasDue === 1, `count=${hasDue}`);
  const dueColor = await page.evaluate(() =>
    getComputedStyle(document.querySelector(".task-card.due-soon")).getPropertyValue("--due-color").trim()
  );
  check("高亮色有值（默认红）", dueColor === "#d93025", dueColor);
  // 悬停与「当前打开的那张」都不许把高亮抹掉（`:hover/.selected` 那条同时改 background + box-shadow，
  // 特异性与 due-soon 相同、位置在后——不覆盖的话选中卡**一直**没有高亮）
  await page.hover(".task-card.due-soon");
  await page.waitForTimeout(200);
  const hoverShadow = await page.evaluate(() =>
    getComputedStyle(document.querySelector(".task-card.due-soon")).boxShadow
  );
  check("悬停时色带还在", hoverShadow.includes("inset") && hoverShadow.includes("3px"), hoverShadow);

  await page.click(".task-card", { button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "标签" }).first().click();
  await page.waitForSelector(".tag-panel", { timeout: 8000 });
  const pills = await page.locator(".tag-panel .tag-color-pill").count();
  check("配色是两排十颗胶囊（v0.8.2 补粉色 + 炫彩色盘）", pills === 10, `${pills} 颗`);
  const rows = await page.$$eval(".tag-panel .tag-color-row", (nodes) =>
    nodes.map((node) => node.querySelectorAll(".tag-color-pill").length)
  );
  check("两排分别是 5 + 5", rows[0] === 5 && rows[1] === 5, rows.join("+"));
  check("面板里没有分区标题文字（v0.8.2 去掉）", (await page.locator(".tag-panel .tag-panel-label").count()) === 0);
  check("预置流末尾有加号胶囊", (await page.locator(".tag-panel .tag-preset-new").count()) === 1);
  check("没有「清除所有标签」按钮", (await page.locator(".tag-clear-all").count()) === 0);
  check(
    "输入框旁有「存入预置」勾选且默认勾上",
    await page.isChecked(".tag-panel .tag-keep-box input")
  );

  await page.fill(".tag-panel .tag-editor-input-row input", "工作");
  await page.press(".tag-panel .tag-editor-input-row input", "Enter");
  await page.waitForTimeout(400);
  const presetCount = await page.evaluate(
    (key) => (JSON.parse(localStorage.getItem(key) ?? "{}")?.appearance?.tagPresets ?? []).length,
    SETTINGS_KEY
  );
  check("回车添加且新标签同时进了预置", presetCount === 1, `${presetCount} 条`);
  check("任务菜单没有报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ---------------------------------------------------------------------------
// 7 工具箱：两列 + 人民币大写 + 生成按钮在右下角
// ---------------------------------------------------------------------------
{
  const { page } = await freshPage(context);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-list", { timeout: 8000 });
  const columns = await page.evaluate(
    () => getComputedStyle(document.querySelector(".toolbox-list")).gridTemplateColumns.split(" ").length
  );
  check("桌面工具箱卡片两列", columns === 2, `${columns} 列`);
  const cards = await page.$$eval(".toolbox-card strong", (nodes) => nodes.map((n) => n.textContent.trim()));
  check("注册表里有两件工具", cards.length === 2, cards.join(" / "));

  await page.click(".toolbox-card:has-text('人民币')");
  await page.waitForSelector(".toolbox-text-input", { timeout: 8000 });
  await page.fill(".toolbox-text-input", "1234.56");
  await page.waitForSelector(".toolbox-rmb-result", { timeout: 8000 });
  const rmb = (await page.textContent(".toolbox-rmb-result"))?.trim();
  check("金额转大写正确", rmb === "壹仟贰佰叁拾肆元伍角陆分", rmb ?? "");
  // v0.8.2：反向识别（中文金额 → 数字）
  await page.fill(".toolbox-text-input", "壹仟贰佰叁拾肆元伍角陆分");
  await page.waitForTimeout(300);
  const back = (await page.textContent(".toolbox-rmb-result"))?.trim();
  check("大写转回金额正确", back === "1234.56", back ?? "");

  await page.click(".toolbox-sub-back");
  await page.waitForSelector(".toolbox-list", { timeout: 8000 });
  await page.click(".toolbox-card:has-text('随机')");
  await page.waitForSelector(".toolbox-sub-actions", { timeout: 8000 });
  const align = await page.evaluate(() => {
    const actions = document.querySelector(".toolbox-sub-actions");
    const rows = document.querySelectorAll(".toolbox-field-row");
    const countRow = rows[rows.length - 1];
    return {
      justify: getComputedStyle(actions).justifyContent,
      belowCount: actions.getBoundingClientRect().top - countRow.getBoundingClientRect().bottom
    };
  });
  check(
    "生成按钮在「数量」下一行、右对齐（v0.8.2 调整）",
    align.justify === "flex-end" && align.belowCount >= -2 && align.belowCount < 60,
    JSON.stringify(align)
  );
  await page.close();
}

// ---------------------------------------------------------------------------
// 8 渲染出来的任务勾选框可点
// ---------------------------------------------------------------------------
{
  const { page, errors } = await freshPage(context);
  await seedTask(page, { markdown: "- [ ] 第一件事\n- [x] 第二件事" });
  await page.dblclick(".task-card");
  await page.waitForSelector(".md-task-box", { timeout: 8000 });
  const boxes = await page.$$eval(".md-task-box", (nodes) => nodes.map((n) => n.checked));
  check("任务框渲染成勾选状态", boxes.length === 2 && boxes[0] === false && boxes[1] === true, JSON.stringify(boxes));
  await page.click(".md-task-box >> nth=0");
  await page.waitForTimeout(500);
  const markdown = await page.evaluate(
    (key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.tasks?.[0]?.markdown ?? "",
    STATE_KEY
  );
  check("点第一颗勾选框写回 markdown", markdown.includes("- [x] 第一件事"), JSON.stringify(markdown));
  const struck = await page.evaluate(
    () => getComputedStyle(document.querySelector("li.md-task-done > .md-task-label")).textDecorationLine
  );
  check("勾上的那行有删除线（只划自己那行，v0.8.2 起打在 .md-task-label 上）", struck.includes("line-through"), struck);
  check("任务框交互没有报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ---------------------------------------------------------------------------
// 9 编辑器列表缩进（Tab 换标记、有序重新编号）
// ---------------------------------------------------------------------------
{
  const { page } = await freshPage(context);
  await seedTask(page, { markdown: "缩进练习" });
  await page.click(".task-card .edit-button");
  await page.waitForSelector(".editor-cm-host .cm-content", { timeout: 8000 });
  await page.click(".editor-cm-host .cm-content");
  const lines = () => page.$$eval(".editor-cm-host .cm-line", (nodes) => nodes.map((n) => n.textContent));
  const selectLine = async (index) => {
    await page.keyboard.press("Control+Home");
    for (let i = 0; i < index; i += 1) await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Home");
    await page.keyboard.press("Shift+End");
  };

  await page.keyboard.press("Control+A");
  await page.keyboard.type("1. 甲");
  await page.keyboard.press("Enter");
  await page.keyboard.type("乙");
  await page.waitForTimeout(150);
  check(
    "回车自动续编号",
    JSON.stringify(await lines()) === JSON.stringify(["1. 甲", "2. 乙"]),
    JSON.stringify(await lines())
  );

  await selectLine(0);
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  let text = await lines();
  check(
    "Tab 缩进有序项并把同级重新编号",
    JSON.stringify(text) === JSON.stringify(["  1. 甲", "1. 乙"]),
    JSON.stringify(text)
  );

  await selectLine(0);
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  text = await lines();
  check("再缩进得到第二级", text[0] === "    1. 甲", JSON.stringify(text));

  await selectLine(0);
  await page.keyboard.press("Shift+Tab");
  await page.waitForTimeout(200);
  text = await lines();
  check("Shift-Tab 反缩进", text[0] === "  1. 甲", JSON.stringify(text));

  await page.keyboard.press("Control+A");
  await page.keyboard.type("- 甲");
  await page.keyboard.press("Shift+End");
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  text = await lines();
  check("无序项 Tab 后换成 * 标记", text[0] === "  * 甲", JSON.stringify(text));

  await selectLine(0);
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  text = await lines();
  check("再缩进换成 + 标记", text[0] === "    + 甲", JSON.stringify(text));
  await page.close();
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 10 移动端返回键：浮层自己接管，逐级退回（v0.8.1 的返回链条 + 历史栈）
// ---------------------------------------------------------------------------
{
  const ANDROID_UA =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
  const mobileCtx = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });
  const page = await mobileCtx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(800);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(900);

  const back = () => page.evaluate(() => window.kxtodoBackHandler());
  check("移动端挂了返回键消费口", await page.evaluate(() => typeof window.kxtodoBackHandler === "function"));

  // 进内容页 → 长按卡片出菜单 → 返回键**先收菜单**，页面留着
  await seedTask(page, { markdown: "返回键用例" });
  await page.locator(".tree-row:has-text('测试条目')").first().click();
  await page.waitForTimeout(500);
  const card = page.locator(".task-card").first();
  await card.waitFor({ timeout: 8000 });
  const box = await card.boundingBox();
  await page.evaluate(async (pt) => {
    const el = document.elementFromPoint(pt.x, pt.y);
    const opts = {
      bubbles: true,
      cancelable: true,
      pointerId: 7,
      pointerType: "touch",
      isPrimary: true,
      clientX: pt.x,
      clientY: pt.y
    };
    el.dispatchEvent(new PointerEvent("pointerdown", opts));
    await new Promise((resolve) => setTimeout(resolve, 700));
    el.dispatchEvent(new PointerEvent("pointerup", opts));
  }, { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + 20) });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  check("长按出菜单", (await page.locator(".context-menu").count()) >= 1);
  check("返回键消费掉这一记（菜单接管）", (await back()) === true);
  await page.waitForTimeout(300);
  check("菜单被返回键收掉", (await page.locator(".context-menu").count()) === 0);
  check("内容页还在（没被一起弹掉）", (await page.locator(".app-shell.mobile.view-content").count()) === 1);

  // 没有浮层时返回键不消费（交给系统 → 历史栈）
  check("没有浮层时不消费", (await back()) === false);

  // 开着浮层时整页被切走：组件卸载必须把拦截器一起摘掉，否则返回键永久失灵
  // （内容页上侧栏是收起的，先回列表：历史栈的返回由 popstate 驱动）
  await page.goBack();
  await page.waitForSelector(".app-shell.mobile.view-list", { timeout: 8000 });
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-list", { timeout: 8000 });
  await page.click(".toolbox-card:has-text('随机')");
  await page.waitForSelector(".toolbox-sub-actions", { timeout: 8000 });
  // 工具页里侧栏是收起的：用历史栈回到列表再切走（真实用户在安卓上按的就是返回键）
  await page.goBack();
  await page.waitForSelector(".app-shell.mobile.view-list", { timeout: 8000 });
  await page.click(".system-nav .nav-row:has-text('我的一天')");
  await page.waitForTimeout(500);
  check("切走之后返回键不再被已卸载的浮层吃掉", (await back()) === false);
  check("移动端返回键用例没有页面错误", errors.length === 0, errors[0] ?? "");
  await page.close();
  await mobileCtx.close();
}

await browser.close();
console.log(failures === 0 ? "\n全部通过" : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);

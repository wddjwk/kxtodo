// v0.8.2 回归。这一版没有新需求，全是 v0.8.1 没做完的事与它新引入的 bug：
// 1  长 markdown 卡片点开**立刻**出完整可读正文（快速版同步上屏，代码高亮与公式随后异步补），
//    短卡片仍走同步完整渲染，不为长文档付代价；
// 2  折行卡片以展开态挂载也认得出「可折叠」（勾选圈画加号、双击能收起）；
// 3  日期与时刻（v0.8.3 起这一套在「日期与提醒」面板里，勾选框换成了时刻行 + 双轨）：
//    时刻能设进去、卡片上显示、重载后仍在（normalizeTask 不再丢 dueTime）、
//    再开面板显示原时刻（不被 now 覆盖）；
// 4  markdown 任务项：删除线只划当前行、子项缩进对齐父项文字、勾选框与文字同高居中、
//    列表行距不超正文、点击勾选框写回源码；
// 5  编辑器 Tab/Enter：整行缩进（含松散列表续行）、光标落在行尾、空项回车退一级、
//    顶级空项回车清标识、有序列表按缩进层级重排编号；
// 6  临期高亮设置：三档圆单选点得中、色盘四个色块（v0.8.3 加了「已过期」）+ 默认按钮
//    同一行排完、默认配色灰红黄蓝；
// 7  首帧缓存：状态缓存写入并被模块初始化读回（首帧种子），特性开关同样进首帧缓存；
// 8  记账：换视图一律把滚动位置归零，日历 ↔ 统计 来回切时切换段控不跳位；
// 9  超链接档位可逆：卡片 → 不渲染 → 标题 → 卡片，拨完就地生效（不必等 markdown 重渲）；
// 10 移动端返回键逐级退：账户表单 → 账户列表 → 关浮层；直达表单则整层关掉；
//    表情选择器先关自己，不把底下的任务列表弹掉；
// 11 移动端点头像进设置：抽屉立刻可见（深度分区随后两帧补上）。
// 用法：node scripts/v082-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const LEDGER_KEY = "todo-note-ledger-v1";
const STATE_CACHE_KEY = "kxtodo-state-cache-v1";
const FEATURES_CACHE_KEY = "kxtodo-features-cache";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures += 1;
}
const J = (value) => JSON.stringify(value);

/** 本地今天的 YYYY-MM-DD。`toISOString()` 给的是 **UTC** 日期，凌晨跑测试会差一天——
 *  临期高亮按本地日分档、记账按本地日归月，种子必须与它们同口径。 */
function localToday() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

async function freshPage(context) {
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(500);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

/** 种一条任务（形状与 normalize 的输入一致，缺的字段它会补）。 */
async function seedTask(page, task) {
  await page.evaluate(
    ({ task, stateKey }) => {
      const now = new Date().toISOString();
      localStorage.setItem(
        stateKey,
        JSON.stringify({
          schemaVersion: 3,
          nodes: [
            { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
            { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
            { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
            { id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null, createdAt: now }
          ],
          tasks: [
            {
              id: "task-a", nodeId: "entry-a", completed: false, important: false, myDay: false,
              tags: [], emojis: [], expanded: false, createdAt: now, updatedAt: now, order: 1,
              ...task
            }
          ],
          backgrounds: {},
          selectedNodeId: "entry-a"
        })
      );
    },
    { task, stateKey: STATE_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
}

const readTask = (page) =>
  page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.tasks?.[0] ?? null, STATE_KEY);

const renderStats = (page) => page.evaluate(() => window.__kxtodoRenderStats ?? null);

/** 种一本有流水的账：全部落在**本地今天**（列表长到能滚，统计与日历当月都有数据）。 */
async function seedBook(page) {
  await page.evaluate(
    ({ key, today }) => {
      const now = new Date().toISOString();
      localStorage.setItem(
        key,
        JSON.stringify({
          accounts: [
            { id: "lacc-01", name: "现金", kind: "cash", icon: "Wallet", color: "#f0862c", initialCents: 100000, createdAt: now },
            { id: "lacc-02", name: "储蓄卡", kind: "储蓄卡", icon: "Landmark", color: "#4a90d9", initialCents: 500000, createdAt: now }
          ],
          categories: [
            { id: "lcat-exp-01", name: "餐饮", side: "expense", icon: "Utensils", color: "#e0654f", parentId: "", createdAt: now },
            { id: "lcat-inc-01", name: "工资", side: "income", icon: "Wallet", color: "#2f9e6e", parentId: "", createdAt: now }
          ],
          entries: Array.from({ length: 80 }, (_, i) => ({
            id: `le-${i}`,
            kind: i % 3 === 0 ? "income" : "expense",
            amountCents: 1000 + i * 137,
            accountId: i % 2 === 0 ? "lacc-01" : "lacc-02",
            categoryId: i % 3 === 0 ? "lcat-inc-01" : "lcat-exp-01",
            date: today,
            note: `第 ${i} 笔`,
            createdAt: now
          })),
          accountTypes: []
        })
      );
    },
    { key: LEDGER_KEY, today: localToday() }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
}

const openLedger = async (page) => {
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });
  await page.waitForTimeout(300);
};

/** 当前整页视图（view-list / view-ledger / …）。$eval 在元素不存在时会抛，
 *  而「返回键把整页弹掉了」正是要抓的失败态，所以这里一律回字符串。 */
const shellView = (page) => page.evaluate(() => document.querySelector(".app-shell")?.className ?? "");

const switchLedgerView = async (page, label) => {
  await page.click(`.ledger-view-switch button[title='${label}']`);
  await page.waitForTimeout(400);
};

/** 触屏长按（移动端唤出卡片菜单只有这一条路） */
async function longPress(page, selector) {
  await page.evaluate((target) => {
    const el = document.querySelector(target);
    if (!el) return;
    const r = el.getBoundingClientRect();
    el.dispatchEvent(
      new PointerEvent("pointerdown", {
        bubbles: true, cancelable: true, composed: true,
        pointerType: "touch", isPrimary: true, pointerId: 7,
        button: 0, buttons: 1, clientX: r.left + r.width / 2, clientY: r.top + 12
      })
    );
  }, selector);
  await page.waitForTimeout(750);
  await page.evaluate(() => {
    window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch", pointerId: 7 }));
  });
  await page.waitForTimeout(350);
}

/** 安卓硬件返回键。MainActivity 的 OnBackPressedCallback 先 evaluateJavascript 问
 *  `window.kxtodoBackHandler`，前端返回 true 就吃掉这一记，返回 false 才让 WebView
 *  退历史 / finish。测试必须走同一条链——直接 `page.goBack()` 量到的是历史栈，
 *  压根问不到浮层拦截器（也就测不出「返回只关最上层浮层」这件事）。 */
async function pressBack(page) {
  const consumed = await page.evaluate(() => {
    const handler = window.kxtodoBackHandler;
    return typeof handler === "function" ? handler() : false;
  });
  if (!consumed) await page.goBack();
  await page.waitForTimeout(500);
  return consumed;
}

const browser = await chromium.launch({ channel: "msedge" });

// ===========================================================================
// 桌面
// ===========================================================================
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });

// ------------------------------------------------------------- 1 两阶段渲染
{
  const { page, errors } = await freshPage(desktop);
  const long = [
    "# 大文档",
    ...Array.from({ length: 40 }, (_, i) => `第 ${i} 段：${"内容很长".repeat(20)}`),
    "```js",
    ...Array.from({ length: 30 }, (_, i) => `console.log("line ${i}");`),
    "```",
    "$$\\int_0^1 x^2 dx = \\frac{1}{3}$$"
  ].join("\n");
  await seedTask(page, { markdown: long });
  const before = await renderStats(page);
  await page.locator(".task-card .markdown-title-row").dblclick();
  await page.waitForTimeout(60);
  const immediate = (await page.locator(".task-card .markdown-content").textContent().catch(() => "")) ?? "";
  check(
    "长卡片点开 60ms 内正文已完整可读（不是空白块）",
    immediate.includes("第 39 段") && immediate.length > 2000,
    `len=${immediate.length}`
  );
  const after = await renderStats(page);
  check(
    "长文档走了两阶段（快速版计数 +1）",
    after !== null && before !== null && after.fast === before.fast + 1,
    `fast ${before?.fast} → ${after?.fast}`
  );
  await page.waitForTimeout(500);
  check(
    "装饰异步跟上（代码高亮 + 公式）",
    (await page.locator(".task-card .markdown-content .hljs").count()) >= 1 &&
      (await page.locator(".task-card .markdown-content .katex").count()) >= 1
  );
  await page.locator(".task-card .markdown-content").dblclick();
  await page.waitForTimeout(300);
  await page.locator(".task-card .markdown-title-row").dblclick();
  await page.waitForTimeout(60);
  check(
    "收起再展开：记忆化命中即刻完整版",
    (await page.locator(".task-card .markdown-content .hljs").count()) >= 1
  );

  // 短卡片（绝大多数）不受两阶段影响：仍是同步一次完整渲染。
  // 计数在每次重载后归零，所以基准要在 seedTask（含重载）**之后**取。
  // 用两行的短文档：单行不折行的卡片根本不可展开，点了也不会渲染。
  await seedTask(page, { markdown: "短任务第一行\n短任务第二行" });
  const shortBefore = await renderStats(page);
  await page.locator(".task-card .markdown-title-row").dblclick();
  await page.waitForTimeout(200);
  const shortAfter = await renderStats(page);
  check(
    "短卡片不付两阶段的代价（fast 不增、block 增 1）",
    shortBefore !== null && shortAfter !== null &&
      shortAfter.fast === shortBefore.fast && shortAfter.block === shortBefore.block + 1,
    `block ${shortBefore?.block}→${shortAfter?.block} fast ${shortBefore?.fast}→${shortAfter?.fast}`
  );
  check("渲染无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------------- 2 折行卡片可折叠
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "这是一条非常非常长的单行任务标题".repeat(8), expanded: true });
  check(
    "展开态挂载的折行卡片：勾选圈画加号（认得出可折叠）",
    await page.locator(".task-card .task-check svg.lucide-plus").isVisible().catch(() => false)
  );
  await page.locator(".task-card .markdown-content").dblclick();
  await page.waitForTimeout(400);
  check(
    "双击能收起",
    !(await page.locator(".task-card .markdown-content").isVisible().catch(() => false))
  );
  check("折行卡片无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------------------ 3 日期与时刻
// v0.8.3 起这一套搬进了「日期与提醒」面板：没有「精确到分钟」勾选框了，时刻是面板里的
// 一行（占位「添加时间」→ 点开双轨 → 确认）。这一节守住的是**当年那个 bug 的后果**：
// 时刻必须真的落盘、卡片上要显示、重载后不能被 normalizeTask 抹掉、再开面板显示原时刻。
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "带时刻任务" });
  await page.locator(".task-card").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".task-menu-date .date-reminder-panel", { timeout: 8000 });
  check(
    "没设时刻时是灰色占位「添加时间」",
    ((await page.locator(".task-menu-date .dr-placeholder").first().textContent().catch(() => "")) ?? "").includes(
      "添加时间"
    )
  );
  await page.locator(".task-menu-date .dp-cell.today").click();
  await page.waitForTimeout(400);
  check("日历还在（没缩小/闪掉）", (await page.locator(".task-menu-date .dp-cell").count()) >= 28);
  // 时刻：进双轨挑 09:30 再确认（`.dp-today` 是这一档的主按钮 = 确认 / 保存）
  await page.locator(".task-menu-date .dr-row .dp-time-trigger").first().click();
  await page.waitForSelector(".task-menu-date .dp-time-wheel", { timeout: 8000 });
  await page.locator(".task-menu-date .time-col").first().locator(".time-cell", { hasText: /^09$/ }).click();
  await page.locator(".task-menu-date .time-col").nth(1).locator(".time-cell", { hasText: /^30$/ }).click();
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForSelector(".task-menu-date .dr-row", { timeout: 8000 });
  const clock = ((await page.locator(".task-menu-date .dr-row strong").first().textContent().catch(() => "")) ?? "").trim();
  check("时刻行显示选定的时刻", clock === "09:30", clock);
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(500);
  const stored = await readTask(page);
  const dueTime = String(stored?.dueTime ?? "");
  check(
    "选到分钟写进任务（dueDate + dueTime）",
    Boolean(stored?.dueDate) && dueTime === "09:30",
    `dueDate=${stored?.dueDate} dueTime=${stored?.dueTime}`
  );
  const label = (await page.locator(".task-due-date").textContent().catch(() => "")) ?? "";
  check("卡片日期带时刻", label.includes(dueTime), `label=${label.trim()} dueTime=${dueTime}`);

  // 重载后时刻仍在（v0.7.3 起 normalizeTask 把 dueTime 丢了，任何一次快照刷新都抹掉时刻）
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  const reloaded = (await page.locator(".task-due-date").textContent().catch(() => "")) ?? "";
  check("重载后时刻不被抹掉", reloaded.includes(dueTime), `label=${reloaded.trim()} dueTime=${dueTime}`);
  check("存下来的时刻没被 now 覆盖", String((await readTask(page))?.dueTime ?? "") === dueTime);

  // 指定过时刻的任务：打开面板显示原时刻，不是「现在」
  await page.locator(".task-card").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".task-menu-date .date-reminder-panel", { timeout: 8000 });
  const shown = ((await page.locator(".task-menu-date .dr-row strong").first().textContent().catch(() => "")) ?? "").trim();
  check("已存时刻：面板显示原时刻", shown === dueTime, `shown=${shown} stored=${dueTime}`);
  check("已存时刻：时刻行有清除叉", (await page.locator(".task-menu-date .dr-row-clear").count()) === 1);
  check("日期与时刻无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------------- 4 markdown 任务项
{
  const { page, errors } = await freshPage(desktop);
  const md = [
    "正文段落一行，用来量正文的行距与字号。",
    "",
    "- [x] 父项甲",
    "  - [ ] 子项未完成",
    "- [x] 父项乙",
    "  - [x] 子项乙",
    "- [ ] 平级未完成"
  ].join("\n");
  await seedTask(page, { markdown: md, expanded: true });
  check(
    "勾选行有 md-task-done + md-task-label 标记（未勾的不打）",
    (await page.locator(".markdown-content li.md-task-done").count()) === 3 &&
      (await page.locator(".markdown-content .md-task-label").count()) === 3,
    `done=${await page.locator(".markdown-content li.md-task-done").count()} label=${await page.locator(".markdown-content .md-task-label").count()}`
  );
  const geometry = await page.evaluate(() => {
    const body = document.querySelector(".markdown-content");
    if (!body) return null;
    const items = [...body.querySelectorAll("li")];
    const ownLabel = (li) => li.querySelector(":scope > .md-task-label");
    const ownBox = (li) => li.querySelector(":scope > input.md-task-box");
    const byLabel = (text) => items.find((li) => (ownLabel(li)?.textContent ?? "").includes(text));
    const uncheckedChild = items.find(
      (li) => ownBox(li) && !ownBox(li).checked && li.textContent.includes("子项未完成")
    );
    const parentDone = byLabel("父项甲");
    const alignParent = byLabel("父项乙");
    const alignChild = byLabel("子项乙");
    const paragraph = body.querySelector("p");
    const rect = (el) => (el ? el.getBoundingClientRect() : null);
    const style = (el) => (el ? getComputedStyle(el) : null);
    const parentLabel = ownLabel(alignParent);
    return {
      childDecoration: uncheckedChild ? style(uncheckedChild).textDecorationLine : "missing",
      parentLabelDecoration: parentDone ? style(ownLabel(parentDone)).textDecorationLine : "missing",
      // 下一级缩到哪：子级的勾选框 / 文字分别相对上级文字左缘偏多少
      childBoxDelta: parentLabel && ownBox(alignChild) ? rect(ownBox(alignChild)).left - rect(parentLabel).left : null,
      childLabelDelta: parentLabel && ownLabel(alignChild) ? rect(ownLabel(alignChild)).left - rect(parentLabel).left : null,
      listItemLineHeight: alignParent ? style(alignParent).lineHeight : "",
      paragraphLineHeight: paragraph ? style(paragraph).lineHeight : "",
      boxHeight: ownBox(alignParent) ? rect(ownBox(alignParent)).height : 0,
      parentFontSize: parentLabel ? style(parentLabel).fontSize : "",
      boxCenterOffset:
        ownBox(alignParent) && parentLabel
          ? rect(ownBox(alignParent)).top + rect(ownBox(alignParent)).height / 2 -
            (rect(parentLabel).top + rect(parentLabel).height / 2)
          : null
    };
  });
  check("子项没有被划掉（删除线不传播）", geometry?.childDecoration === "none", `decoration=${geometry?.childDecoration}`);
  check(
    "删除线打在勾选行自己的文字上",
    geometry?.parentLabelDecoration === "line-through",
    `decoration=${geometry?.parentLabelDecoration}`
  );
  const px = (value) => (typeof value === "string" ? Number.parseFloat(value) : Number(value));
  console.log(`      （缩进实测：子级勾选框 ${geometry?.childBoxDelta}px / 子级文字 ${geometry?.childLabelDelta}px）`);
  check(
    "子级缩到上级文字下方（图标右侧一格的量级）",
    geometry?.childBoxDelta !== null && geometry.childBoxDelta >= -2 && geometry.childBoxDelta <= 12,
    `childBoxDelta=${geometry?.childBoxDelta} childLabelDelta=${geometry?.childLabelDelta}`
  );
  check(
    "列表行距不超过正文行距",
    px(geometry?.listItemLineHeight) <= px(geometry?.paragraphLineHeight) + 0.5,
    `li=${geometry?.listItemLineHeight} p=${geometry?.paragraphLineHeight}`
  );
  check(
    "勾选框不比文字大（同高或略小）",
    geometry?.boxHeight <= px(geometry?.parentFontSize) * 1.2,
    `box=${geometry?.boxHeight} font=${geometry?.parentFontSize}`
  );
  check(
    "勾选框与文字行内垂直居中",
    geometry?.boxCenterOffset !== null && Math.abs(geometry.boxCenterOffset) <= 2,
    `offset=${geometry?.boxCenterOffset}`
  );
  await page.locator(".markdown-content .md-task-box").nth(1).click();
  await page.waitForTimeout(500);
  check(
    "点击勾选框写回源码",
    String((await readTask(page))?.markdown ?? "").includes("- [x] 子项未完成"),
    `md=${J((await readTask(page))?.markdown)}`
  );
  check("任务项无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// --------------------------------------------------------- 5 编辑器 Tab/Enter
{
  const { page, errors } = await freshPage(desktop);
  const openEditor = async () => {
    await page.click(".task-card .edit-button");
    await page.waitForSelector(".editor-cm-host .cm-content", { timeout: 8000 });
    await page.click(".editor-cm-host .cm-content");
    await page.waitForTimeout(200);
  };
  const lines = () => page.$$eval(".editor-cm-host .cm-line", (nodes) => nodes.map((n) => n.textContent));
  /** 重写文档；光标留在**文末**（续列表/回车语义都从行尾出发，与真实使用一致） */
  const setDoc = async (text) => {
    await page.keyboard.press("Control+A");
    await page.keyboard.press("Backspace");
    await page.keyboard.type(text, { delay: 2 });
    await page.waitForTimeout(150);
  };

  await seedTask(page, { markdown: "init" });
  await openEditor();

  await setDoc("- [ ] 待办项");
  await page.keyboard.press("Tab");
  await page.keyboard.type("X", { delay: 2 });
  await page.waitForTimeout(200);
  check("Tab：todo 项整行缩进且光标落在行尾", J(await lines()) === J(["   * [ ] 待办项X"]), J(await lines()));

  await setDoc("1. 甲");
  await page.keyboard.press("Enter");
  await page.keyboard.type("乙");
  await page.waitForTimeout(150);
  check("Enter：有序列表自动续编号", J(await lines()) === J(["1. 甲", "2. 乙"]), J(await lines()));
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  check("Tab：第二项缩进（嵌套层编号从 1 起）", J(await lines()) === J(["1. 甲", "   1. 乙"]), J(await lines()));
  await page.keyboard.press("Shift+Tab");
  await page.waitForTimeout(200);
  check("Shift+Tab：退回外层并接续编号", J(await lines()) === J(["1. 甲", "2. 乙"]), J(await lines()));

  await setDoc("- 甲");
  await page.keyboard.press("Tab");
  await page.waitForTimeout(200);
  check("Tab：无序标记按层级轮换（- → *）", J(await lines()) === J(["   * 甲"]), J(await lines()));

  await setDoc("- 一级");
  await page.keyboard.press("Enter");
  await page.keyboard.type("二级");
  await page.keyboard.press("Tab");
  await page.waitForTimeout(150);
  check("准备：二级项已缩进", J(await lines()) === J(["- 一级", "   * 二级"]), J(await lines()));
  await page.keyboard.press("Enter");
  await page.waitForTimeout(200);
  check("Enter：空二级项生成（带标记）", J(await lines()) === J(["- 一级", "   * 二级", "   * "]), J(await lines()));
  await page.keyboard.press("Enter");
  await page.waitForTimeout(200);
  check("Enter：空项回车退一级（不新增行）", J(await lines()) === J(["- 一级", "   * 二级", "- "]), J(await lines()));
  await page.keyboard.press("Enter");
  await page.waitForTimeout(200);
  check("Enter：顶级空项回车清掉标识", J(await lines()) === J(["- 一级", "   * 二级", ""]), J(await lines()));

  await setDoc("- [ ] 甲");
  await page.keyboard.press("Enter");
  await page.keyboard.type("乙");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(250);
  check(
    "Enter：缩进的空 todo 项退一级（勾选框保留）",
    J(await lines()) === J(["- [ ] 甲", "   * [ ] 乙", "- [ ] "]),
    J(await lines())
  );

  await setDoc("1. 甲");
  await page.keyboard.press("Enter");
  await page.keyboard.type("乙");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  check(
    "Enter：有序空项退级并接外层编号",
    J(await lines()) === J(["1. 甲", "   1. 乙", "2. "]),
    J(await lines())
  );

  // 松散列表：光标在列表项上按 Tab，续行要跟着一起动
  await setDoc("1. 甲");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.keyboard.insertText("   续行说明");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.type("2. 乙");
  await page.waitForTimeout(250);
  check("准备：松散列表已构造", J(await lines()) === J(["1. 甲", "   续行说明", "2. 乙"]), J(await lines()));
  await page.keyboard.press("Control+Home");
  await page.keyboard.press("Tab");
  await page.waitForTimeout(250);
  // 甲缩进成嵌套项后，乙 是块里唯一的顶层有序项，编号重排为 1 是正确语义
  check(
    "Tab：松散列表的续行一起缩进",
    J(await lines()) === J(["   1. 甲", "      续行说明", "1. 乙"]),
    J(await lines())
  );
  check("编辑器无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ----------------------------------------------------------- 6 临期高亮设置
{
  const { page, errors } = await freshPage(desktop);
  const today = localToday();
  await seedTask(page, { markdown: "临期任务", dueDate: today, plannedDate: today });
  await page.click(".profile-card");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  await page.waitForTimeout(600);
  const radios = page.locator('.settings-drawer input[name="due-highlight"]');
  check("临期高亮是三档单选", (await radios.count()) === 3, `count=${await radios.count()}`);
  check(
    "三档是圆的（radio，不是方的勾选框）",
    (await page.$$eval('.settings-drawer input[name="due-highlight"]', (nodes) =>
      nodes.map((node) => node.type)
    )).every((type) => type === "radio")
  );
  await page
    .locator('.settings-drawer label')
    .filter({ has: page.locator('input[name="due-highlight"]') })
    .nth(1)
    .click();
  await page.waitForTimeout(400);
  check("点「配色」能选中", await radios.nth(1).isChecked().catch(() => false));
  await page.click("button.settings-backdrop");
  await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(300);
  check(
    "配色档下今天到期的卡片带高亮",
    (await page.locator(".task-card.due-soon").count()) >= 1 ||
      (await page.locator(".task-card[style*='--due']").count()) >= 1
  );

  await page.locator(".workspace .header-actions button[title='列表菜单']").click();
  await page.waitForSelector(".due-color-row", { timeout: 8000 });
  const row = await page.evaluate(() => {
    const r = document.querySelector(".due-color-row");
    if (!r) return null;
    const kids = [...r.children].map((k) => k.getBoundingClientRect());
    const centers = kids.map((k) => k.top + k.height / 2);
    return {
      count: kids.length,
      colors: [...r.querySelectorAll("input[type=color]")].map((input) => input.value),
      centered: centers.every((c) => Math.abs(c - centers[0]) < 4),
      ordered: kids.every((k, i) => i === 0 || k.left >= kids[i - 1].right - 1),
      inRow: kids.length > 0 && kids[kids.length - 1].right <= r.getBoundingClientRect().right + 1
    };
  });
  check("色盘是四个色块 + 一个默认按钮（v0.8.3 加了「已过期」档）", row?.count === 5, `count=${row?.count}`);
  check("色盘一行排完（不换行、不超出）", Boolean(row?.centered && row?.ordered && row?.inRow), J(row));
  check(
    "默认配色是灰红黄蓝（已过期灰 / 今天红 / 明天黄 / 后天蓝）",
    J(row?.colors) === J(["#808080", "#d93025", "#eab308", "#3b82f6"]),
    J(row?.colors)
  );
  check("临期高亮无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------------------- 7 首帧缓存
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "缓存测试任务" });
  await page.waitForTimeout(1600);
  const cached = await page.evaluate((key) => localStorage.getItem(key), STATE_CACHE_KEY);
  check(
    "状态缓存已写入（含任务正文）",
    Boolean(cached) && String(cached).includes("缓存测试任务"),
    `len=${cached?.length ?? 0}`
  );
  const featuresCache = await page.evaluate(
    ({ featuresKey, stateKey }) => ({
      cached: JSON.parse(localStorage.getItem(featuresKey) ?? "null"),
      live: JSON.parse(localStorage.getItem(stateKey) ?? "{}")?.features ?? null
    }),
    { featuresKey: FEATURES_CACHE_KEY, stateKey: "todo-note-settings-v3" }
  );
  check(
    "特性开关也进首帧缓存且与设置一致",
    featuresCache.cached !== null &&
      (featuresCache.live === null || J(featuresCache.cached) === J(featuresCache.live)),
    J(featuresCache)
  );
  // 浏览器 legacy 模式的水合全程在微任务里跑完，DOM 采样抓不到「水合前的首帧」；
  // 真实 core 模式下有 IPC 宏任务间隔，种子会画出来。这里改用 getItem 钩子证明
  // **模块初始化时确实读了缓存**（种子进了 store），再看水合后的最终界面。
  await page.context().addInitScript(() => {
    window.__cacheReads = [];
    const origGet = Storage.prototype.getItem;
    Storage.prototype.getItem = function patched(key) {
      const value = origGet.call(this, key);
      if (String(key).includes("state-cache")) window.__cacheReads.push(value ? value.length : 0);
      return value;
    };
  });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(500);
  const reads = await page.evaluate(() => window.__cacheReads ?? []);
  check("模块初始化读了状态缓存（首帧种子）", reads.length > 0 && reads[0] > 300, J(reads));
  const title = (await page.locator(".workspace h1").textContent().catch(() => "")) ?? "";
  const cardText = (await page.locator(".task-card").first().textContent().catch(() => "")) ?? "";
  check(
    "水合后界面画出条目与任务",
    title.includes("测试条目") && cardText.includes("缓存测试任务"),
    `title=${title.trim()}`
  );
  check("首帧缓存无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------------- 9 超链接档位可逆
{
  const { page, errors } = await freshPage(desktop);
  const PAGE_HTML = `<!doctype html><html><head><meta charset="utf-8">
<title>示例站点的一篇很长很长的文章标题超过了三十个字所以应该被截断显示省略号才对哦</title>
<meta property="og:description" content="这是一段正文预览摘要。">
<meta property="og:site_name" content="示例站">
</head><body>正文</body></html>`;
  await page.route("**/example.com/**", (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html; charset=utf-8",
      headers: { "access-control-allow-origin": "*" },
      body: PAGE_HTML
    })
  );
  await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.features = { ...(settings.features ?? {}), linkRender: "card" };
    localStorage.setItem(key, JSON.stringify(settings));
  }, "todo-note-settings-v3");
  await seedTask(page, {
    markdown: ["链接检查", "", "裸链接 https://example.com/page 在这里", "", "手写的链接 [我的文字](https://example.com/other) 不动"].join("\n"),
    expanded: true
  });
  await page.waitForTimeout(900);
  const snapshot = () =>
    page.evaluate(() => ({
      cards: document.querySelectorAll(".task-card .kx-link-card").length,
      anchors: [...document.querySelectorAll(".task-card .markdown-content a")].map((a) => a.textContent.trim().slice(0, 40))
    }));
  check("卡片档：两条链接都成卡片", (await snapshot()).cards === 2, J(await snapshot()));

  /** 走真实 UI 拨档（不重载）：增强必须是可逆的 */
  const pick = async (label) => {
    await page.keyboard.press("Control+,");
    await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
    await page.waitForTimeout(600);
    await page.evaluate((want) => {
      const rows = [...document.querySelectorAll(".settings-drawer .toggle-row")];
      const row = rows.find((el) => (el.textContent ?? "").includes("超链接渲染样式"));
      const target = row ? [...row.querySelectorAll("label")].find((l) => (l.textContent ?? "").trim() === want) : null;
      target?.click();
    }, label);
    await page.waitForTimeout(900);
    await page.click("button.settings-backdrop");
    await page.waitForSelector("aside.settings-drawer", { state: "detached", timeout: 8000 });
    await page.waitForTimeout(400);
  };

  await pick("不渲染");
  const off = await snapshot();
  check(
    "拨到「不渲染」：卡片就地退回原样链接（不必重渲 markdown）",
    off.cards === 0 && off.anchors.includes("https://example.com/page"),
    J(off)
  );
  check("退回后手写链接的文字原样回来", off.anchors.includes("我的文字"), J(off.anchors));
  await pick("标题");
  const titled = await snapshot();
  check(
    "拨到「标题」：裸链接换成网页标题、手写链接不动",
    titled.cards === 0 && titled.anchors.some((t) => t.startsWith("示例站点的一篇")) && titled.anchors.includes("我的文字"),
    J(titled)
  );
  await pick("卡片");
  check("拨回「卡片」：又出两张卡", (await snapshot()).cards === 2, J(await snapshot()));
  check("超链接档位无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// --------------------------------------------------------- 8 记账视图切换
{
  const { page, errors } = await freshPage(desktop);
  await seedBook(page);
  await openLedger(page);
  const switchBox = () => page.locator(".ledger-view-switch").boundingBox();
  // 别滚到底：列表到底会触发「换成上个月」，随后自己 resetScroll，量不到跳变
  const scrollDown = () =>
    page.evaluate(() => {
      const el = document.querySelector(".ledger-scroll");
      if (el) el.scrollTop = 300;
    });
  await scrollDown();
  await page.waitForTimeout(300);
  const scrolled = await page.evaluate(() => document.querySelector(".ledger-scroll")?.scrollTop ?? -1);
  check("准备：列表已滚出一段", scrolled > 100, `scrollTop=${scrolled}`);
  const listBox = await switchBox();
  await switchLedgerView(page, "日历视图");
  await scrollDown();
  await switchLedgerView(page, "统计视图");
  const statsScroll = await page.evaluate(() => document.querySelector(".ledger-scroll")?.scrollTop ?? -1);
  check("换到统计视图滚动位置归零（不带着上一视图的滚动量跳）", statsScroll === 0, `scrollTop=${statsScroll}`);
  check("统计视图占比环渲染", (await page.locator(".ledger-donut circle").count()) >= 2);
  const statsBox = await switchBox();
  await switchLedgerView(page, "日历视图");
  const calBox = await switchBox();
  await switchLedgerView(page, "统计视图");
  const statsBox2 = await switchBox();
  const same = (a, b) =>
    a !== null && b !== null && Math.abs(a.x - b.x) < 1.5 && Math.abs(a.y - b.y) < 1.5 && Math.abs(a.width - b.width) < 1.5;
  check(
    "日历 ↔ 统计 来回切：切换段控不跳位",
    same(listBox, statsBox) && same(statsBox, calBox) && same(calBox, statsBox2),
    J({ listBox, statsBox, calBox, statsBox2 })
  );
  check("记账切换无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// --------------------------------------------------- 11 设置抽屉分两段挂载
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "设置测试" });
  const started = Date.now();
  await page.click(".profile-card");
  await page.waitForSelector("aside.settings-drawer", { timeout: 8000 });
  const opened = Date.now() - started;
  check("点头像后设置抽屉立刻可见", opened < 1500, `${opened}ms`);
  const headText = (await page.locator("aside.settings-drawer").innerText().catch(() => "")) ?? "";
  check("前两节（个人资料 / 外观效果）当场就在", headText.includes("个人资料") && headText.includes("外观"), headText.slice(0, 40));
  await page.waitForTimeout(600);
  const fullText = (await page.locator("aside.settings-drawer").innerText().catch(() => "")) ?? "";
  check(
    "深度分区随后补上（特性开关 / 关于与更新）",
    fullText.includes("特性开关") && fullText.includes("关于"),
    fullText.slice(-40)
  );
  check("设置抽屉无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

await desktop.close();

// ===========================================================================
// 移动端（Android UA + 触摸）：返回键逐级退
// ===========================================================================
{
  const mobile = await browser.newContext({
    viewport: { width: 390, height: 844 },
    userAgent: ANDROID_UA,
    hasTouch: true,
    isMobile: true
  });

  // ------------------------------------------- 10a 账户管理：表单 → 列表 → 关
  {
    const { page, errors } = await freshPage(mobile);
    await openLedger(page);
    check("移动端记账整页", (await shellView(page)).includes("view-ledger"), await shellView(page));
    await page.click(".ledger-view .header-actions > button[title='记账菜单'], .ledger-view .header-actions > button[title='更多操作']");
    await page.waitForSelector(".ledger-gear-panel", { timeout: 8000 });
    await page.click(".ledger-gear-panel .menu-item-button:has-text('账户与转账')");
    await page.waitForSelector(".ledger-manager", { timeout: 8000 });
    check("账户管理打开在列表层", (await page.locator(".ledger-account-row").count()) >= 2);
    await page.click(".ledger-manager .ledger-sheet-foot button:has-text('添加账户')");
    await page.waitForSelector(".ledger-field-row", { timeout: 8000 });
    check("添加账户表单打开", ((await page.textContent(".ledger-sheet-title")) ?? "").trim() === "添加账户");
    check("这一记返回被表单层消费（不退历史）", (await pressBack(page)) === true);
    check(
      "返回键先关表单、回到账户列表（不是直接弹掉整层）",
      (await page.locator(".ledger-manager").count()) === 1 &&
        (await page.locator(".ledger-field-row").count()) === 0 &&
        (await page.locator(".ledger-account-row").count()) >= 2,
      `manager=${await page.locator(".ledger-manager").count()} rows=${await page.locator(".ledger-account-row").count()}`
    );
    check("返回后仍在记账页", (await shellView(page)).includes("view-ledger"), await shellView(page));
    check("这一记返回被账户管理层消费", (await pressBack(page)) === true);
    check("再一下返回键关掉账户管理", (await page.locator(".ledger-manager").count()) === 0);
    check("仍在记账页（没有被弹回任务列表）", (await shellView(page)).includes("view-ledger"), await shellView(page));
    // 拦截器必须跟着注销：浮层关完之后再按，就该退「记账整页」这一层历史了
    check("浮层关完后返回键不再被消费（拦截器已注销）", (await pressBack(page)) === false);
    await page.waitForSelector(".app-shell.view-list", { timeout: 8000 });
    check("退回任务列表", (await shellView(page)).includes("view-list"), await shellView(page));

    // 从资产页直接点「添加」进来：用户没见过列表面板，返回就该整层关掉
    await openLedger(page);
    await switchLedgerView(page, "资产视图");
    await page.click(".ledger-panel-head .ledger-chip-button:has-text('添加')");
    await page.waitForSelector(".ledger-field-row", { timeout: 8000 });
    check("直达添加账户表单", ((await page.textContent(".ledger-sheet-title")) ?? "").trim() === "添加账户");
    check("直达表单：这一记返回被消费", (await pressBack(page)) === true);
    check(
      "直达表单时返回键整层关掉（不退到没见过的列表）",
      (await page.locator(".ledger-manager").count()) === 0,
      `manager=${await page.locator(".ledger-manager").count()}`
    );
    check("关掉后仍在记账页", (await shellView(page)).includes("view-ledger"), await shellView(page));
    check("账户管理无脚本报错", errors.length === 0, errors[0] ?? "");
    await page.close();
  }

  // ------------------------------------- 10b 表情选择器（懒加载 chunk 在途也算）
  {
    const { page, errors } = await freshPage(mobile);
    await seedTask(page, { markdown: "表情测试" });
    // 移动端要先点树行进入内容页（长按手势只在卡片列表上有效）
    await page.locator(".tree-row", { hasText: "测试条目" }).click();
    await page.waitForTimeout(600);
    check("任务卡在内容页", (await page.locator(".task-card").count()) === 1, await shellView(page));
    await longPress(page, ".task-card");
    await page.waitForSelector(".context-menu", { timeout: 8000 });
    await page.locator(".context-menu .menu-item-button", { hasText: "添加表情" }).first().click();
    await page.waitForSelector(".icon-picker-backdrop", { timeout: 10000 });
    check("表情选择器打开", (await page.locator(".icon-picker").count()) === 1);
    check("这一记返回被选择器消费", (await pressBack(page)) === true);
    check("返回键只关选择器", (await page.locator(".icon-picker-backdrop").count()) === 0);
    check(
      "底下的任务列表还在（返回只关了选择器，没弹掉整页）",
      (await page.locator(".task-card").count()) === 1 && (await shellView(page)).includes("view-content"),
      `cards=${await page.locator(".task-card").count()} shell=${await shellView(page)}`
    );
    check("表情选择器无脚本报错", errors.length === 0, errors[0] ?? "");
    await page.close();
  }

  await mobile.close();
}

await browser.close();
console.log(failures === 0 ? "\n全部通过" : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);

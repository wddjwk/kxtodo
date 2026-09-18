// v0.8.3 回归。这一版的两个重点是「日期与提醒」与「文件传输助手」，其余是短需求与
// 前两版 review 的甄别修复：
// 1  「日期与提醒」面板：右键菜单更名与换界面、点日期不关闭、时刻行（占位/值/清除叉）、
//    双轨时刻（清除 / 确认都回上级）、提醒行（占位/胶囊/加号/悬浮叉）、添加提醒菜单
//    五项与「截止前」的可用条件、自定义的日期/时间两页、底部清除 / 保存才关闭；
// 2  同一个面板的三个入口：右键菜单、卡片上点日期或时刻、编辑器工具栏的日期按钮；
// 3  日记右键「修改日期」只有日历（与任务条目区分开）；
// 4  人民币大小写：输入框够宽、支持的汉字常显（繁体带括号）；
// 5  工具箱右键「固定此工具」→ 固定区出现图钉条目；固定区可拖动排序、右键取消固定；
// 6  草稿纸：纯文本、自动保存、重载后还在；
// 7  编辑器与日记的标签面板与右键菜单同一套（预置可见、文案「勾选后自动添加到预置」），
//    日记预置与工作事项预置分开存；
// 8  截止时间排序：同一天里有时刻的与没时刻的按方向分列两端；
// 9  临期高亮第四档（已过期，默认灰）与色盘实时预览 + 确认后才落盘；
// 10 markdown 行首空白（`1. xx \t 1.1 xx`）渲染后仍有缩进；
// 11 截止时间排序（UI 口径）：同一天里带时刻的与只到天的按方向分列两端；
// 12 前两版 review 的修复：账户/分类管理器的 Esc 与返回键走同一套逐级退（直达表单则整层关），
//    且在浮层里点一下不会把自己关掉。
// 用法：node scripts/v083-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures += 1;
}
const J = (value) => JSON.stringify(value);

/** 本地今天的 YYYY-MM-DD（`toISOString()` 是 UTC，凌晨跑会差一天）。 */
function localToday(offsetDays = 0) {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
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
            { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now },
            { id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null, createdAt: now },
            { id: "entry-b", kind: "entry", name: "另一个条目", icon: "list", parentId: null, createdAt: now }
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

/** 右键任务卡片打开菜单，再点进「日期与提醒」子面板。 */
async function openSchedulePanel(page) {
  await page.locator(".task-card").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".task-menu-date .date-reminder-panel", { timeout: 8000 });
}

const panelText = (page, selector) =>
  page.locator(`.task-menu-date .date-reminder-panel ${selector}`).first().textContent().catch(() => "");

const browser = await chromium.launch({ channel: "msedge" });

// ===========================================================================
// 桌面
// ===========================================================================
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });

// ------------------------------------------------- 1 面板结构与「点日期不关闭」
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "带提醒的任务" });
  await page.locator(".task-card").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  const labels = await page.locator(".context-menu > .menu-item .menu-item-label, .context-menu > button .menu-item-label").allTextContents();
  check("菜单里是「日期与提醒」（不再叫「添加日期」）", labels.some((text) => text.trim() === "日期与提醒"), J(labels));
  check("菜单里没有残留的「添加日期」", !labels.some((text) => text.trim() === "添加日期"));

  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".task-menu-date .date-reminder-panel", { timeout: 8000 });
  check("面板里有日历", (await page.locator(".task-menu-date .dp-cell").count()) >= 28);
  check("时间行默认是灰色占位「添加时间」", (await panelText(page, ".dr-placeholder")).includes("添加时间"));
  check(
    "提醒行默认是灰色占位「添加提醒」",
    (await page.locator(".task-menu-date .dr-reminder-empty").textContent() ?? "").includes("添加提醒")
  );
  const footer = await page.locator(".task-menu-date .date-picker-actions button").allTextContents();
  check("底部是「清除 / 保存」两个按钮", J(footer.map((t) => t.trim())) === J(["清除", "保存"]), J(footer));

  // 10.2：点日期之后面板不关闭
  await page.locator(".task-menu-date .dp-cell.today").click();
  await page.waitForTimeout(400);
  check(
    "点了日期面板不关闭（需求 10.2）",
    (await page.locator(".task-menu-date .date-reminder-panel").count()) === 1
  );
  check("选中的日期高亮", (await page.locator(".task-menu-date .dp-cell.selected").count()) === 1);
  check("面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 2 时刻行与双轨滚轮
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "时刻任务" });
  await openSchedulePanel(page);

  // 没有时刻 → 「截止前」两项禁用（需求 10.4.1）
  await page.locator(".task-menu-date .dr-reminder-empty").click();
  await page.waitForSelector(".task-menu-date .dr-add-menu", { timeout: 5000 });
  const options = page.locator(".task-menu-date .dr-add-menu button");
  const texts = (await options.allTextContents()).map((text) => text.trim());
  check(
    "添加提醒菜单五项齐全且顺序对",
    J(texts) === J(["一小时后", "两小时后", "截止前一小时", "截止前五分钟", "自定义"]),
    J(texts)
  );
  check("没有分钟时刻时「截止前一小时」禁用", await options.nth(2).isDisabled());
  check("没有分钟时刻时「截止前五分钟」禁用", await options.nth(3).isDisabled());
  check("「一小时后」始终可用", !(await options.nth(0).isDisabled()));
  // 点面板别处只收小菜单，不收整个面板
  await page.locator(".task-menu-date .date-picker-header").click();
  await page.waitForTimeout(300);
  check("点小菜单外面只收小菜单", (await page.locator(".task-menu-date .dr-add-menu").count()) === 0);
  check("面板还开着", (await page.locator(".task-menu-date .date-reminder-panel").count()) === 1);

  // 进双轨：默认停在此刻
  await page.locator(".task-menu-date .dr-row .dp-time-trigger").first().click();
  await page.waitForSelector(".task-menu-date .dp-time-wheel", { timeout: 5000 });
  const wheelTitle = (await page.locator(".task-menu-date .dp-title").textContent() ?? "").trim();
  check("进入双轨选时刻视图", wheelTitle === "选择时刻", wheelTitle);
  const wheelButtons = await page.locator(".task-menu-date .date-picker-actions button").allTextContents();
  check("双轨下方是「清除 / 确认」", J(wheelButtons.map((t) => t.trim())) === J(["清除", "确认"]), J(wheelButtons));
  const activeHour = await page.locator(".task-menu-date .time-col").first().locator(".time-cell.active").textContent();
  check("双轨默认停在当前小时", activeHour?.trim() === String(new Date().getHours()).padStart(2, "0"), activeHour);

  // 选一个固定时刻再确认
  await page.locator(".task-menu-date .time-col").first().locator(".time-cell", { hasText: /^09$/ }).click();
  await page.locator(".task-menu-date .time-col").nth(1).locator(".time-cell", { hasText: /^30$/ }).click();
  await page.waitForTimeout(200);
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForSelector(".task-menu-date .dr-row", { timeout: 5000 });
  check("确认后回到上一级配置界面", (await page.locator(".task-menu-date .dp-time-wheel").count()) === 0);
  check("时刻行显示刚设的值", (await panelText(page, ".dr-row strong")).trim() === "09:30");
  check("时刻行出现清除叉", (await page.locator(".task-menu-date .dr-row-clear").count()) === 1);

  // 设了分钟时刻 → 「截止前」可用了
  await page.locator(".task-menu-date .dr-row .dp-time-trigger").first().click();
  await page.waitForSelector(".task-menu-date .dp-time-wheel", { timeout: 5000 });
  await page.locator(".task-menu-date .date-picker-actions .dp-clear").click();
  await page.waitForTimeout(300);
  check("双轨「清除」也回上一级且不保存时刻", (await panelText(page, ".dr-placeholder")).includes("添加时间"));

  await page.locator(".task-menu-date .dr-row .dp-time-trigger").first().click();
  await page.locator(".task-menu-date .time-col").first().locator(".time-cell", { hasText: /^18$/ }).click();
  await page.locator(".task-menu-date .time-col").nth(1).locator(".time-cell", { hasText: /^00$/ }).click();
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForTimeout(200);
  check("设了时刻后日期自动落在今天（时刻必须有日期依附）", (await page.locator(".task-menu-date .dp-cell.selected").count()) === 1);

  // 保存
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 8000 });
  // legacy 路径的 localStorage 写入有 180ms 防抖（stores.commit）
  await page.waitForTimeout(500);
  const stored = await readTask(page);
  check("保存后 dueTime 落盘", stored?.dueTime === "18:00", `dueTime=${stored?.dueTime}`);
  check("保存后 dueDate 落盘", stored?.dueDate === localToday(), `dueDate=${stored?.dueDate}`);
  check("保存后 plannedDate 同进同出", stored?.plannedDate === localToday(), `plannedDate=${stored?.plannedDate}`);
  check("面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 3 提醒：预设、自定义、移除、持久化
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "提醒任务", dueDate: localToday(1), dueTime: "18:00" });
  await openSchedulePanel(page);

  await page.locator(".task-menu-date .dr-reminder-empty").click();
  await page.waitForSelector(".task-menu-date .dr-add-menu", { timeout: 5000 });
  const options = page.locator(".task-menu-date .dr-add-menu button");
  check("有分钟时刻时「截止前一小时」可用", !(await options.nth(2).isDisabled()));
  await options.nth(2).click();
  await page.waitForTimeout(300);
  check("加了一条提醒后出现胶囊", (await page.locator(".task-menu-date .dr-reminder-chip").count()) === 1);
  check("胶囊文字是「截止前1小时」", (await panelText(page, ".dr-reminder-chip")).includes("截止前1小时"));
  check("加了提醒后占位字消失", (await page.locator(".task-menu-date .dr-reminder-empty").count()) === 0);
  check("末尾有加号（可以加多个）", (await page.locator(".task-menu-date .dr-reminder-plus").count()) === 1);

  // 加号 → 自定义
  await page.locator(".task-menu-date .dr-reminder-plus").click();
  await page.waitForSelector(".task-menu-date .dr-add-menu", { timeout: 5000 });
  await page.locator(".task-menu-date .dr-add-menu button", { hasText: "自定义" }).click();
  await page.waitForSelector(".task-menu-date .dr-tabs", { timeout: 5000 });
  const tabs = (await page.locator(".task-menu-date .dr-tab").allTextContents()).map((t) => t.trim());
  check("自定义页顶部是「日期 / 时间」两页", J(tabs) === J(["日期", "时间"]), J(tabs));
  check("自定义页默认在日期页（有日历）", (await page.locator(".task-menu-date .dp-cell").count()) >= 28);
  // 翻到下个月再挑一天：自定义提醒必须是**未来**的时刻，过去的会被面板当场拒掉
  await page.locator(".task-menu-date .calendar-grid-panel button[aria-label='下个月']").click();
  await page.waitForTimeout(250);
  await page.locator(".task-menu-date .calendar-grid-panel .dp-cell:not(.other-month)", { hasText: /^10$/ }).first().click();
  await page.waitForTimeout(200);
  await page.locator(".task-menu-date .dr-tab", { hasText: "时间" }).click();
  await page.waitForSelector(".task-menu-date .dp-time-wheel", { timeout: 5000 });
  check("时间页是双轨视图", (await page.locator(".task-menu-date .time-col").count()) === 2);
  await page.locator(".task-menu-date .time-col").first().locator(".time-cell", { hasText: /^07$/ }).click();
  await page.locator(".task-menu-date .time-col").nth(1).locator(".time-cell", { hasText: /^45$/ }).click();
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForTimeout(300);
  const chips = await page.locator(".task-menu-date .dr-reminder-chip").allTextContents();
  check("自定义提醒加进去了（共两条）", chips.length === 2, J(chips));

  // 同一瞬时不重复添加
  await page.locator(".task-menu-date .dr-reminder-plus").click();
  await page.locator(".task-menu-date .dr-add-menu button", { hasText: "截止前一小时" }).click();
  await page.waitForTimeout(200);
  check(
    "同一瞬时的重复规则不再加一条",
    (await page.locator(".task-menu-date .dr-reminder-chip").count()) === 2
  );

  // 移除：悬浮出叉
  const chip = page.locator(".task-menu-date .dr-reminder-chip").first();
  await chip.hover();
  await page.waitForTimeout(200);
  check("悬浮胶囊露出删除叉", await chip.locator(".tag-delete").isVisible().catch(() => false));
  await chip.locator(".tag-delete").click();
  await page.waitForTimeout(200);
  check("点叉移除一条提醒", (await page.locator(".task-menu-date .dr-reminder-chip").count()) === 1);

  // 保存并核对落盘
  await page.locator(".task-menu-date .date-picker-actions .dp-today").click();
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(500);
  const stored = await readTask(page);
  check("提醒落盘（1 条）", Array.isArray(stored?.reminders) && stored.reminders.length === 1, J(stored?.reminders));

  // 重开面板：值都还在
  await openSchedulePanel(page);
  check("重开面板时刻还在", (await panelText(page, ".dr-row strong")).trim() === "18:00");
  check("重开面板提醒还在", (await page.locator(".task-menu-date .dr-reminder-chip").count()) === 1);

  // 清除：三样一起回默认并退出
  await page.locator(".task-menu-date .date-picker-actions .dp-clear").click();
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 8000 });
  await page.waitForTimeout(500);
  const cleared = await readTask(page);
  check("清除后日期没了", !cleared?.dueDate, `dueDate=${cleared?.dueDate}`);
  check("清除后时刻没了", !cleared?.dueTime, `dueTime=${cleared?.dueTime}`);
  check("清除后提醒没了", (cleared?.reminders ?? []).length === 0, J(cleared?.reminders));
  check("面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 4 三个入口是同一个面板
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "入口复用", dueDate: localToday(2), dueTime: "09:15" });

  // 卡片上点日期/时刻（需求 13）
  await page.waitForSelector(".task-due-date", { timeout: 8000 });
  const cardLabel = (await page.locator(".task-due-date").textContent() ?? "").trim();
  check("卡片日期带时刻", cardLabel.includes("09:15"), cardLabel);
  await page.locator(".task-due-date").click();
  await page.waitForSelector(".task-date-popover .date-reminder-panel", { timeout: 8000 });
  check("点卡片上的时刻弹出同一个「日期与提醒」面板", true);
  check("卡片浮层里时刻是原值", (await page.locator(".task-date-popover .dr-row strong").textContent() ?? "").trim() === "09:15");
  await page.locator(".task-date-popover .date-picker-actions .dp-today").click();
  await page.waitForSelector(".task-date-popover", { state: "detached", timeout: 8000 });

  // 编辑器工具栏的日期按钮（需求 10.8）
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.locator(".editor-meta-trigger", { hasText: "9月" }).first().click();
  await page.waitForSelector(".editor-meta-pop .date-reminder-panel", { timeout: 8000 });
  check("编辑器工具栏的日期按钮复用同一个面板", true);
  check("编辑器面板里带出了原时刻", (await page.locator(".editor-meta-pop .dr-row strong").textContent() ?? "").trim() === "09:15");

  // 在编辑器里加一条提醒，保存后落盘
  await page.locator(".editor-meta-pop .dr-reminder-empty").click();
  await page.waitForSelector(".editor-meta-pop .dr-add-menu", { timeout: 5000 });
  const editorOptions = page.locator(".editor-meta-pop .dr-add-menu button");
  check("编辑器里「截止前五分钟」可用（有分钟时刻）", !(await editorOptions.nth(3).isDisabled()));
  await editorOptions.nth(3).click();
  await page.waitForTimeout(200);
  check("编辑器里胶囊出现", (await page.locator(".editor-meta-pop .dr-reminder-chip").count()) === 1);
  await page.locator(".editor-meta-pop .date-picker-actions .dp-today").click();
  await page.waitForTimeout(300);
  check("面板保存后浮层收起", (await page.locator(".editor-meta-pop").count()) === 0);
  await page.locator(".editor-dialog button", { hasText: "保存" }).first().click().catch(() => {});
  await page.keyboard.press("Control+s");
  await page.waitForSelector(".editor-dialog", { state: "detached", timeout: 10000 });
  await page.waitForTimeout(500);
  const stored = await readTask(page);
  check("编辑器里加的提醒落盘", (stored?.reminders ?? []).length === 1, J(stored?.reminders));
  check("入口复用无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 5 日记「修改日期」只有日历
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(
    ({ key, today }) => {
      const now = new Date().toISOString();
      localStorage.setItem(
        key,
        JSON.stringify({
          schemaVersion: 1,
          entries: [
            { id: "diary-a", date: today, title: "今天的日记", markdown: "正文", mood: "", weather: "", tags: [], createdAt: now, updatedAt: now }
          ],
          meta: { revision: 1 }
        })
      );
    },
    { key: "todo-note-diary-v1", today: localToday() }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(900);
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-card", { timeout: 10000 });
  await page.locator(".diary-card").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  const labels = (await page.locator(".context-menu .menu-item-label").allTextContents()).map((t) => t.trim());
  check("日记菜单是「修改日期」", labels.includes("修改日期"), J(labels));
  await page.locator(".context-menu .menu-item-button", { hasText: "修改日期" }).first().click();
  await page.waitForSelector(".task-menu-date .date-picker", { timeout: 8000 });
  check("日记只有日历", (await page.locator(".task-menu-date .dp-cell").count()) >= 28);
  check("日记不显示时刻行（需求 10.6）", (await page.locator(".task-menu-date .date-picker-time").count()) === 0);
  check("日记不显示提醒行", (await page.locator(".task-menu-date .dr-reminders").count()) === 0);
  check("日记日期菜单无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 4 人民币：输入框够宽 + 汉字常显
{
  const { page, errors } = await freshPage(desktop);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "人民币大小写" }).click();
  await page.waitForSelector(".transfer-tool, .toolbox-sub", { timeout: 8000 });
  const box = await page.locator(".toolbox-text-input-wide").first().boundingBox();
  check("人民币输入框够宽（>320px，放得下占位字）", box && box.width > 320, J(box));
  const hint = (await page.locator(".toolbox-han-hint").textContent()) ?? "";
  check("支持的汉字常显（不等失败才列）", hint.includes("壹") && hint.includes("貳") && hint.includes("陸"), hint.slice(0, 40));
  await page.locator(".toolbox-text-input-wide").first().fill("1234.56");
  await page.waitForTimeout(200);
  check("正向转换仍正常", ((await page.locator(".toolbox-rmb-result").textContent()) ?? "").includes("壹仟贰佰叁拾肆元伍角陆分"));
  check("人民币工具无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 5 固定此工具 / 取消固定
{
  const { page, errors } = await freshPage(desktop);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "人民币大小写" }).click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "固定此工具" }).click();
  await page.waitForTimeout(600);
  check("固定后侧栏多出工具行", (await page.locator(".system-nav .nav-row:has-text('人民币大小写')").count()) === 1);
  check("卡片上出现图钉标记", (await page.locator(".toolbox-card-pin").count()) === 1);
  const settings = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}"), SETTINGS_KEY);
  check("navItems 里落了 tool:rmb", (settings?.appearance?.navItems ?? []).includes("tool:rmb"), J(settings?.appearance?.navItems));

  // 点钉住的行：直达子视图（v0.8.4 起子页头部是右上角两枚按钮，返回一律回工具箱主界面）
  await page.locator(".system-nav .nav-row:has-text('人民币大小写')").click();
  await page.waitForTimeout(600);
  check("钉住的行直达工具子视图", (await page.locator(".toolbox-sub-bar").count()) === 1);
  await page.locator(".toolbox-sub-bar button").first().click();
  await page.waitForTimeout(500);
  check("子页返回落在工具箱主界面", (await page.locator(".toolbox-list").count()) === 1);

  // 右键侧栏工具行 → 取消固定
  await page.locator(".system-nav .nav-row:has-text('人民币大小写')").click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "取消固定" }).click();
  await page.waitForTimeout(600);
  check("取消固定后行消失", (await page.locator(".system-nav .nav-row:has-text('人民币大小写')").count()) === 0);
  check("固定/取消固定无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 6 固定区拖动排序
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.appearance = settings.appearance ?? {};
    settings.appearance.navItems = ["my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox", "tool:rmb"];
    localStorage.setItem(key, JSON.stringify(settings));
  }, SETTINGS_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const source = page.locator(".system-nav .nav-row", { hasText: "人民币大小写" });
  const target = page.locator(".system-nav .nav-row", { hasText: "我的一天" });
  const a = await source.boundingBox();
  const b = await target.boundingBox();
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + a.width / 2, a.y - 10, { steps: 6 });
  await page.mouse.move(b.x + b.width / 2, b.y + 4, { steps: 10 });
  await page.waitForTimeout(150);
  // v0.8.4：落点线换成「行实时让位」——拖动中那一行有抬起态，顺序已就地预览
  check(
    "拖动中有抬起态且行已让位",
    (await page.locator(".nav-row.nav-drag-source").count()) === 1 &&
      (await page.evaluate(() => document.querySelector(".system-nav .nav-row")?.getAttribute("title"))) === "人民币大小写"
  );
  await page.mouse.up();
  await page.waitForTimeout(600);
  const after = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}")?.appearance?.navItems ?? [], SETTINGS_KEY);
  check("拖动后顺序落盘（工具行到了最前）", after[0] === "tool:rmb", J(after));
  check("拖动排序无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 7 草稿纸：纯文本 + 自动保存
{
  const { page, errors } = await freshPage(desktop);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  await page.locator(".scratchpad-area").fill("第一行\n# 这不是标题，只是文本");
  // v0.8.4：防抖拉到 5 秒（每次落盘 = 一条审计 + 一次 revision），状态提示行撤掉了
  await page.waitForTimeout(5400);
  const stored = await page.evaluate(() => localStorage.getItem("kxtodo-scratchpad-v1"));
  check("草稿自动保存进 localStorage", stored === "第一行\n# 这不是标题，只是文本", J(stored));
  const scratch = await page.evaluate((key) => JSON.parse(localStorage.getItem(key) ?? "{}").scratchpad ?? null, STATE_KEY);
  check("草稿正文进数据域（可同步）", scratch?.text === "第一行\n# 这不是标题，只是文本", J(scratch));
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.locator(".toolbox-card", { hasText: "草稿纸" }).click();
  await page.waitForSelector(".scratchpad-area", { timeout: 8000 });
  check("重载后草稿还在", ((await page.locator(".scratchpad-area").inputValue()) ?? "").startsWith("第一行"));
  check("草稿纸无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 8 传输助手界面（LocalSend 式两栏）
{
  const { page, errors } = await freshPage(desktop);
  await page.click(".system-nav .nav-row:has-text('工具箱')");
  await page.waitForSelector(".toolbox-card", { timeout: 8000 });
  check("工具箱里有文件传输助手", (await page.locator(".toolbox-card", { hasText: "文件传输助手" }).count()) === 1);
  await page.locator(".toolbox-card", { hasText: "文件传输助手" }).click();
  // v0.8.4：两栏换成「发送 / 接收」标签滑块（需求 1.3）
  await page.waitForSelector(".transfer", { timeout: 8000 });
  check("发送 / 接收两栏（滑块）", (await page.locator(".transfer-tabs button").count()) === 2);
  const sendDisabled = await page.locator(".transfer-send-button").isDisabled();
  check("未上线时发送按钮禁用", sendDisabled);
  await page.locator(".transfer-code-row input").fill("same-code-123");
  await page.waitForTimeout(200);
  check("浏览器预览（无壳）下发送按钮仍禁用（没上线）", await page.locator(".transfer-send-button").isDisabled());
  check("传输界面无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 9 编辑器标签面板 = 右键菜单那一套 + 日记预置分开
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.appearance = settings.appearance ?? {};
    settings.appearance.tagPresets = [{ id: "p-work", color: "blue", text: "工作预置" }];
    settings.appearance.diaryTagPresets = [{ id: "p-diary", color: "green", text: "日记预置" }];
    localStorage.setItem(key, JSON.stringify(settings));
  }, SETTINGS_KEY);
  await seedTask(page, { markdown: "标签面板测试" });
  await page.locator(".task-card .edit-button").click();
  await page.waitForSelector(".editor-dialog", { timeout: 10000 });
  await page.locator(".editor-tag-add").click();
  await page.waitForSelector(".editor-tag-pop .tag-panel", { timeout: 8000 });
  check("编辑器标签面板显示预置胶囊", ((await page.locator(".editor-tag-pop .tag-preset-main").allTextContents()).includes("工作预置")));
  check("编辑器不串日记预置", !((await page.locator(".editor-tag-pop .tag-preset-main").allTextContents()).includes("日记预置")));
  const ph = await page.locator('.editor-tag-pop .tag-editor-input-row input:not([type="checkbox"])').getAttribute("placeholder");
  check("占位字统一为「勾选后自动添加到预置」", ph === "勾选后自动添加到预置", ph);
  // 点预置胶囊 → 加到草稿标签上
  await page.locator(".editor-tag-pop .tag-preset-main", { hasText: "工作预置" }).click();
  await page.waitForTimeout(200);
  check("点预置加到条目草稿", (await page.locator(".editor-meta-tags .task-tag").count()) >= 1);
  await page.keyboard.press("Control+s");
  await page.waitForSelector(".editor-dialog", { state: "detached", timeout: 10000 });

  // 日记编辑器：只看到日记预置
  await page.evaluate(({ today }) => {
    const now = new Date().toISOString();
    localStorage.setItem(
      "todo-note-diary-v1",
      JSON.stringify({
        schemaVersion: 1,
        entries: [{ id: "diary-a", date: today, title: "标签", markdown: "正文", mood: "", weather: "", tags: [], createdAt: now, updatedAt: now }],
        meta: { revision: 1 }
      })
    );
  }, { today: localToday() });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  await page.click(".system-nav .nav-row:has-text('日记')");
  await page.waitForSelector(".diary-card", { timeout: 10000 });
  await page.locator(".diary-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "标签" }).click();
  await page.waitForSelector(".tag-panel", { timeout: 8000 });
  const diaryPresets = await page.locator(".context-menu .tag-preset-main").allTextContents();
  check("日记右键菜单用日记预置", diaryPresets.includes("日记预置") && !diaryPresets.includes("工作预置"), J(diaryPresets));
  check("标签面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 10 临期高亮第四档（已过期灰）+ 四个色块
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "过期的任务", dueDate: localToday(-1) });
  // 打开临期高亮（默认 off）
  await page.evaluate((key) => {
    const settings = JSON.parse(localStorage.getItem(key) ?? "{}");
    settings.features = settings.features ?? {};
    settings.features.dueHighlight = "solid";
    localStorage.setItem(key, JSON.stringify(settings));
  }, SETTINGS_KEY);
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);
  const style = await page.locator(".task-card").first().getAttribute("style");
  check("过期任务画了高亮", (style ?? "").includes("--due-color"), style);
  check("过期默认灰 #808080", (style ?? "").includes("#808080"), style);
  // 列表三点菜单里四个色块，顺序是 过期/今天/明天/后天
  await page.locator("button[title='列表菜单']").dispatchEvent("mousedown");
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.waitForSelector(".context-menu .due-color-row", { timeout: 8000 });
  const swatches = await page.locator(".due-color-row .ui-color-picker").count();
  check("临期高亮色四个色块（过期在最左）", swatches === 4, String(swatches));
  check("临期高亮无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 11 截止时间排序：同一天里带时刻的与只到天的按方向分列两端
{
  const { page, errors } = await freshPage(desktop);
  await page.evaluate(
    ({ today, tomorrow, stateKey }) => {
      const now = new Date().toISOString();
      const base = {
        completed: false, important: false, myDay: false, tags: [], emojis: [],
        expanded: false, createdAt: now, updatedAt: now, nodeId: "entry-a"
      };
      localStorage.setItem(
        stateKey,
        JSON.stringify({
          schemaVersion: 3,
          nodes: [
            { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
            { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
            { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
            { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now },
            { id: "entry-a", kind: "entry", name: "测试条目", icon: "list", parentId: null, createdAt: now }
          ],
          tasks: [
            { ...base, id: "t-allday", markdown: "只到天", dueDate: today, order: 1 },
            { ...base, id: "t-morning", markdown: "今天九点", dueDate: today, dueTime: "09:00", order: 2 },
            { ...base, id: "t-tomorrow", markdown: "明天", dueDate: tomorrow, order: 3 }
          ],
          backgrounds: {},
          selectedNodeId: "entry-a"
        })
      );
    },
    { today: localToday(), tomorrow: localToday(1), stateKey: STATE_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(800);

  const titles = async () =>
    (await page.locator(".task-card .markdown-body").allTextContents()).map((text) => text.trim().split("\n")[0].trim());

  const pickSort = async (label) => {
    await page.locator("button[title='列表菜单']").dispatchEvent("mousedown");
    await page.waitForSelector(".context-menu", { timeout: 8000 });
    await page.locator(".context-menu .menu-item-button", { hasText: "排序方式" }).click();
    await page.waitForSelector(".context-menu .submenu-panel", { timeout: 8000 });
    await page.locator(".context-menu .submenu-panel .menu-item-button", { hasText: label }).click();
    await page.waitForTimeout(400);
  };

  await pickSort("截止时间 ↑ 最近");
  check("升序：今天九点 → 只到天 → 明天", J(await titles()) === J(["今天九点", "只到天", "明天"]), J(await titles()));
  await pickSort("截止时间 ↓ 最远");
  check("降序：明天 → 只到天 → 今天九点", J(await titles()) === J(["明天", "只到天", "今天九点"]), J(await titles()));
  check("排序无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 12 markdown 行首空白（`1. xx ⇥ 1.1 xx`）
{
  const { page, errors } = await freshPage(desktop);
  await seedTask(page, { markdown: "1. xx\n\t1.1 xx\n\t\t1.1.1 xxx", expanded: true });
  await page.waitForSelector(".task-card .markdown-content", { timeout: 8000 });
  const html = await page.locator(".task-card .markdown-content").innerHTML();
  const NB = "&nbsp;";
  check("缩进行渲染成不间断空格（一层 4 个）", html.includes(NB.repeat(4) + "1.1 xx"), html.slice(0, 200));
  check("第二层缩进更深（8 个）", html.includes(NB.repeat(8) + "1.1.1 xxx"), html.slice(0, 200));
  check("缩进行没掉出列表（仍是同一个 li）", (await page.locator(".task-card .markdown-content li").count()) === 1, html.slice(0, 200));
  check("没有变成缩进代码块", (await page.locator(".task-card .markdown-content pre").count()) === 0);
  // 三行各占一行：innerText 里逐行取，不间断空格当普通空格看
  const lines = (await page.locator(".task-card .markdown-content").innerText())
    .split("\n")
    .map((line) => line.split("\u00A0").join(" ").trim())
    .filter(Boolean);
  check("三行分开渲染", J(lines.slice(-3)) === J(["xx", "1.1 xx", "1.1.1 xxx"]), J(lines));
  check("缩进渲染无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ------------------------------------------------- 13 记账浮层：管理器逐级退（Esc 与返回键同一套）
// review #6：Esc 与返回键统一成一个 stepBack（此前 Esc 会跳过「账户类型小表单」那一档，
// 同一个浮层两条退路行为不一致）。AccountManager 与 CategoryManager 现在同一套语义：
// 从列表进去的逐级退，直达表单的整层关。
// （review #7 的「图片预览泄漏 window keydown」没法在这里跑：浏览器预览下记账插图的
//   src 解析要走 Tauri 的 image_data_url，预览罩子根本打不开——那处修的是 onMount 的
//   cleanup，与同目录 CategoryDrilldown 逐字对齐，靠代码审查守。）
{
  const { page, errors } = await freshPage(desktop);
  await page.click(".system-nav .nav-row:has-text('记账')");
  await page.waitForSelector(".ledger-view", { timeout: 10000 });

  // 账户管理：列表 → 表单 → 类型小表单，Escape 一档一档退
  // v0.8.4 需求 14：齿轮直接弹记账菜单，两个管理器都收进菜单里
  await page.click(".ledger-view button[title='记账菜单']");
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "账户与转账" }).click();
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.locator(".ledger-manager button", { hasText: "添加账户" }).first().click();
  await page.waitForTimeout(400);
  await page.locator(".ledger-choice[title^='自定义账户类型']").click();
  await page.waitForSelector(".ledger-type-form", { timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  check("Escape 先退类型小表单（不跳过它直接退到列表）", (await page.locator(".ledger-type-form").count()) === 0);
  check("账户表单还在", (await page.locator(".ledger-manager").count()) === 1);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  check("再一下 Escape 回到账户列表", (await page.locator(".ledger-manager").count()) === 1);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  check("第三下 Escape 关掉整个浮层", true);

  // 直达表单（资产视图的「添加账户」）：用户没见过列表，Escape 直接整层关
  await page.click(".ledger-view-switch button[title='资产视图']");
  await page.waitForTimeout(500);
  await page.click(".ledger-fab");
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  check("直达表单时 Escape 整层关掉（不退到没见过的列表）", true);

  // 在浮层里点一下不该把自己关掉（LedgerView.closeOverlays 现在也收这两个管理器，
  // 靠浮层根上的 click|stopPropagation 挡住 App 的「点空白关所有浮层」）
  await page.click(".ledger-view button[title='记账菜单']");
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "分类管理" }).click();
  await page.waitForSelector(".ledger-manager", { timeout: 8000 });
  await page.locator(".ledger-manager .ledger-sheet-title").click();
  await page.waitForTimeout(300);
  check("在管理器里点一下不会把自己关掉", (await page.locator(".ledger-manager").count()) === 1);
  await page.keyboard.press("Escape");
  await page.waitForSelector(".ledger-manager", { state: "detached", timeout: 8000 });
  check("记账浮层无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 移动端
// ===========================================================================
const mobile = await browser.newContext({
  userAgent: ANDROID_UA,
  viewport: { width: 393, height: 851 },
  isMobile: true,
  hasTouch: true
});

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

/** 安卓硬件返回键的真实链路：MainActivity → window.kxtodoBackHandler()。
 *  直接 page.goBack() 量到的是历史栈，问不到浮层拦截器。 */
async function pressBack(page) {
  const consumed = await page.evaluate(() => {
    const handler = window.kxtodoBackHandler;
    return typeof handler === "function" ? handler() : false;
  });
  if (!consumed) await page.goBack();
  await page.waitForTimeout(500);
  return consumed;
}

// ------------------------------------------------- 6 移动端面板与返回键逐级退
{
  const { page, errors } = await freshPage(mobile);
  await seedTask(page, { markdown: "移动端提醒", dueDate: localToday(1), dueTime: "18:00" });
  // 移动端先点树行进入内容页（长按手势只在卡片列表上有效）
  await page.locator(".tree-row", { hasText: "测试条目" }).click();
  await page.waitForTimeout(600);
  check("移动端进了内容页", (await page.locator(".task-card").count()) === 1);
  await longPress(page, ".task-card");
  await page.waitForSelector(".context-menu", { timeout: 8000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "日期与提醒" }).first().click();
  await page.waitForSelector(".date-reminder-panel", { timeout: 8000 });
  const box = await page.locator(".date-reminder-panel").boundingBox();
  check("移动端面板在视口内", box && box.x >= -1 && box.x + box.width <= 394, J(box));
  check("移动端面板带出了原时刻", (await page.locator(".date-reminder-panel .dr-row strong").textContent() ?? "").trim() === "18:00");

  // 进双轨再返回：先退内部视图，不收整个菜单
  await page.locator(".date-reminder-panel .dr-row .dp-time-trigger").first().click();
  await page.waitForSelector(".date-reminder-panel .dp-time-wheel", { timeout: 5000 });
  check("返回键先退双轨视图", (await pressBack(page)) === true);
  check("退回主视图（日历还在）", (await page.locator(".date-reminder-panel .dp-cell").count()) >= 28);
  check("菜单没被关掉", (await page.locator(".context-menu").count()) === 1);

  // 添加提醒的小菜单也要先退
  await page.locator(".date-reminder-panel .dr-reminder-plus, .date-reminder-panel .dr-reminder-empty").first().click();
  await page.waitForSelector(".date-reminder-panel .dr-add-menu", { timeout: 5000 });
  check("返回键先收添加提醒小菜单", (await pressBack(page)) === true);
  check("小菜单收了、面板还在", (await page.locator(".date-reminder-panel .dr-add-menu").count()) === 0);

  // 移动端菜单是钻入式：一记返回退出二级面板（回到一级菜单），再一记才收整个菜单
  check("返回键退出二级面板（回到一级菜单）", (await pressBack(page)) === true);
  check("一级菜单还在", (await page.locator(".context-menu").count()) === 1);
  check("二级面板收了", (await page.locator(".date-reminder-panel").count()) === 0);
  check("再按一次返回收掉整个菜单", (await pressBack(page)) === true);
  await page.waitForSelector(".context-menu", { state: "detached", timeout: 8000 });

  // 卡片上点日期：移动端同样是这个面板
  await page.locator(".task-due-date").click();
  await page.waitForSelector(".task-date-popover .date-reminder-panel", { timeout: 8000 });
  const popBox = await page.locator(".task-date-popover .date-reminder-panel").boundingBox();
  check("移动端卡片浮层的面板也在视口内", popBox && popBox.x >= -1 && popBox.x + popBox.width <= 394, J(popBox));
  check("卡片浮层的返回键关掉浮层", (await pressBack(page)) === true);
  await page.waitForSelector(".task-date-popover", { state: "detached", timeout: 8000 });
  check("移动端面板无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

await desktop.close();
await mobile.close();
await browser.close();
console.log(failures === 0 ? "\n全部通过" : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);

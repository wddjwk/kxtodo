// v0.9.5 回归。标语栏重做逐条覆盖：六位置（顶部/底部对齐内容区、左/中/右）、
// 左右边界跟卡片区对齐、无输入框页面底部也正常、每项独立图标、30 字跑马灯、
// 胶囊主题色/字体色/透明度、轮播间隔配置且无进度条、条目卡片行内编辑保存、
// 注释从 UI 移除、360 窄屏扫。
// 用法：node scripts/v095-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";

let failures = 0;
let passes = 0;
function check(name, ok, extra = "") {
  if (ok) passes += 1;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra && !ok ? " — " + extra : ""}`);
  if (!ok) failures += 1;
}
const J = (value) => JSON.stringify(value);

async function freshPage(context, { mobile = false } = {}) {
  const page = await context.newPage();
  if (mobile) await page.setViewportSize({ width: 360, height: 800 });
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

function entryNode(id, name) {
  return { id, kind: "entry", name, icon: "notebook", parentId: null, order: 1, createdAt: new Date().toISOString() };
}

function task(id, nodeId, name) {
  return {
    id, nodeId, markdown: name, completed: false, important: false, myDay: false,
    tags: [], emojis: [], expanded: false, createdAt: new Date().toISOString()
  };
}

function sloganSeed(overrides = {}) {
  return {
    items: [
      { id: "sl-1", text: "多喝水，少久坐", icon: "", createdAt: new Date().toISOString() },
      { id: "sl-2", text: "今日事今日毕", icon: "", createdAt: new Date().toISOString() }
    ],
    activeIds: ["sl-1", "sl-2"],
    accent: "",
    textColor: "",
    opacity: 0.94,
    intervalSeconds: 8,
    position: "top-right",
    enabled: true,
    updatedAt: new Date().toISOString(),
    ...overrides
  };
}

async function seedState(page, { nodes = [], tasks = [], slogan = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, slogan, stateKey }) => {
      const now = new Date().toISOString();
      const base = [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now }
      ];
      const state = { schemaVersion: 3, nodes: [...base, ...nodes], tasks, backgrounds: {}, selectedNodeId: nodes[0]?.id ?? "my-day" };
      if (slogan) state.slogan = slogan;
      localStorage.setItem(stateKey, JSON.stringify(state));
    },
    { nodes, tasks, slogan, stateKey: STATE_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

/** 直接改 localStorage 的 slogan（浏览器 dev 走 legacy 快照）并刷新。 */
async function patchSlogan(page, patch) {
  await page.evaluate(({ stateKey, patch }) => {
    const state = JSON.parse(localStorage.getItem(stateKey));
    state.slogan = { ...state.slogan, ...patch };
    localStorage.setItem(stateKey, JSON.stringify(state));
  }, { stateKey: STATE_KEY, patch });
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

async function openSloganTool(page) {
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "标语栏" }).click();
  await page.waitForSelector(".slogan-tool", { timeout: 5000 });
}

async function backToWorkspace(page) {
  await page.locator(".tree-row", { hasText: "记事" }).click();
  await page.waitForTimeout(500);
}

async function openDiary(page) {
  await page.locator(".system-nav .nav-row", { hasText: "日记" }).first().click();
  await page.waitForSelector(".diary-view", { timeout: 5000 });
  await page.waitForTimeout(400);
}

/** 量横幅与内容区的对齐几何（rect 同为视觉像素，直接比差值）。 */
async function measure(page) {
  return page.evaluate(() => {
    const banner = document.querySelector(".slogan-banner");
    const header = [...document.querySelectorAll(".list-header")].find((el) => el.getClientRects().length && getComputedStyle(el).visibility !== "hidden");
    const bar = document.querySelector(".add-task-bar");
    const fab = document.querySelector(".diary-fab-row, .ledger-fab-row, .calendar-fab-row, .scheduler-floating-add");
    const rect = (el) => (el ? el.getBoundingClientRect() : null);
    return {
      banner: rect(banner),
      header: rect(header),
      bar: rect(bar),
      fab: rect(fab),
      vh: window.innerHeight
    };
  });
}

// ---------------------------------------------------------------------------
// 1. 六个位置的几何：顶部/底部对齐内容区，左/右/居中水平对齐
// ---------------------------------------------------------------------------
async function section1Positions(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("a1", "entry-a", "一件事")],
    slogan: sloganSeed()
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });

  const cases = [
    { pos: "top-left", label: "左上" },
    { pos: "top-right", label: "右上" },
    { pos: "bottom-left", label: "左下" },
    { pos: "bottom-right", label: "右下" },
    { pos: "top-center", label: "顶部居中" },
    { pos: "bottom-center", label: "底部居中" }
  ];
  for (const { pos, label } of cases) {
    await patchSlogan(page, { position: pos });
    await page.waitForSelector(".slogan-banner", { timeout: 4000 });
    const m = await measure(page);
    const top = pos.startsWith("top");
    const horizontal = pos.endsWith("left") ? "left" : pos.endsWith("right") ? "right" : "center";
    const hOk = horizontal === "left"
      ? Math.abs(m.banner.left - m.header.left) <= 3
      : horizontal === "right"
        ? Math.abs(m.banner.right - m.header.right) <= 3
        : Math.abs((m.banner.left + m.banner.right) / 2 - (m.header.left + m.header.right) / 2) <= 3;
    const vOk = top
      ? m.banner.bottom <= m.header.top + 1
      : m.banner.top >= (m.bar ? m.bar.bottom - 1 : 0) && m.banner.top > m.vh * 0.3;
    check(`1 ${label}：水平对齐内容区（${horizontal}）`, hOk, J({ banner: m.banner, header: m.header }));
    check(`1 ${label}：${top ? "顶部在标题上方" : "底部在内容区底部"}`, vOk, J({ banner: m.banner, bar: m.bar, vh: m.vh }));
  }
  check("1.13 六个位置按钮齐全", (await (async () => { await openSloganTool(page); return page.locator(".slogan-position-grid button").count(); })()) === 6);
  check("1.14 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 2. 没有输入框的页面（日记）：底部位置也正常，且不遮挡加号
// ---------------------------------------------------------------------------
async function section2NoInputPage(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], slogan: sloganSeed({ position: "bottom-right" }) });
  await openDiary(page);
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const m = await measure(page);
  check("2.1 日记页底部位置在页面下方（不是卡在顶部）", m.banner.top > m.vh * 0.55, J({ banner: m.banner, vh: m.vh }));
  check("2.2 日记页不遮挡写日记加号", m.fab ? m.banner.bottom <= m.fab.top + 1 : true, J({ banner: m.banner, fab: m.fab }));
  check("2.3 日记页左右与内容区对齐", Math.abs(m.banner.right - m.header.right) <= 3, J({ banner: m.banner, header: m.header }));
  await patchSlogan(page, { position: "bottom-center" });
  await openDiary(page);
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const m2 = await measure(page);
  check("2.4 日记页底部居中也正常", m2.banner.top > m2.vh * 0.55 && (m2.fab ? m2.banner.bottom <= m2.fab.top + 1 : true), J({ banner: m2.banner, fab: m2.fab }));
  check("2.5 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 3. 每项独立图标 + 卡片行内编辑保存 + 注释移除
// ---------------------------------------------------------------------------
async function section3ItemsAndEdit(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("a1", "entry-a", "一件事")],
    slogan: sloganSeed({
      items: [
        { id: "sl-1", text: "带火焰图标", icon: "Flame", createdAt: new Date().toISOString() },
        { id: "sl-2", text: "不加图标", icon: "none", createdAt: new Date().toISOString() }
      ],
      activeIds: ["sl-1"]
    })
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const iconClass = await page.locator(".slogan-banner-body svg").first().getAttribute("class").catch(() => "");
  check("3.1 条目自带图标生效（Flame）", /flame/.test(iconClass ?? ""), iconClass);
  await patchSlogan(page, { activeIds: ["sl-2"] });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  check("3.2 条目 icon=none 不渲染图标", (await page.locator(".slogan-banner-body svg").count()) === 0);

  await openSloganTool(page);
  check("3.3 卡片各有图标按钮", (await page.locator(".slogan-row .slogan-item-icon").count()) === 2);
  const noteText = await page.locator(".slogan-tool").innerText();
  check("3.4 UI 不再有说明性傻叼注释", !noteText.includes("不挤占排版") && !noteText.includes("移动端只在这里维护内容"));

  // 点文字直接编辑 → 保存按钮落盘
  await page.locator(".slogan-text-btn").first().click();
  await page.waitForSelector(".slogan-edit-input", { timeout: 3000 });
  await page.locator(".slogan-edit-input").fill("改过的标语");
  await page.locator(".slogan-row-btn.save").click();
  await page.waitForTimeout(500);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("3.5 行内编辑保存落盘", stored.slogan.items[0].text === "改过的标语", J(stored.slogan.items[0].text));
  check("3.6 卡片文字跟着更新", (await page.locator(".slogan-text-btn").first().innerText()) === "改过的标语");
  // Esc 取消不落盘
  await page.locator(".slogan-text-btn").first().click();
  await page.locator(".slogan-edit-input").fill("不该保存");
  await page.locator(".slogan-edit-input").press("Escape");
  await page.waitForTimeout(300);
  const stored2 = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("3.7 Esc 取消不落盘", stored2.slogan.items[0].text === "改过的标语", J(stored2.slogan.items[0].text));
  // 条目图标菜单
  await page.locator(".slogan-item-icon").first().click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "不加图标" }).click();
  await page.waitForTimeout(400);
  const stored3 = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("3.8 条目图标可单独改", stored3.slogan.items[0].icon === "none", J(stored3.slogan.items[0].icon));
  check("3.9 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 4. 30 字跑马灯 + 主题色/字体色/透明度 + 间隔配置 + 无进度条
// ---------------------------------------------------------------------------
async function section4MarqueeTheme(context) {
  const { page, errors } = await freshPage(context);
  const longText = "这是一条特别特别长的标语用于验证三十字跑马灯滚动效果展示超过上限之后的横向滚动播放";
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("a1", "entry-a", "一件事")],
    slogan: sloganSeed({
      items: [{ id: "sl-1", text: longText, icon: "", createdAt: new Date().toISOString() }],
      activeIds: ["sl-1"],
      accent: "#d93025",
      textColor: "#112233",
      opacity: 0.5,
      intervalSeconds: 2
    })
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const style = await page.locator(".slogan-banner").evaluate((el) => {
    const banner = getComputedStyle(el);
    const text = el.querySelector(".slogan-banner-text");
    const textStyle = getComputedStyle(text);
    const body = getComputedStyle(el.querySelector(".slogan-banner-body"));
    return {
      marquee: text.classList.contains("marquee"),
      animation: textStyle.animationName,
      bg: banner.backgroundColor,
      textColor: textStyle.color,
      iconColor: body.color,
      maxWidth: banner.maxWidth
    };
  });
  check("4.1 超 30 字走跑马灯动画", style.marquee && style.animation === "slogan-marquee", J(style));
  const maxWidthPx = /([\d.]+)px/.exec(style.maxWidth);
  check("4.2 显示宽度有上限（≈30 字，≤560px）", maxWidthPx !== null && Number(maxWidthPx[1]) <= 560, style.maxWidth);
  check("4.3 背景透明度 0.5", /rgba\(255,\s*255,\s*255,\s*0\.5\)/.test(style.bg), style.bg);
  check("4.4 字体颜色生效", style.textColor === "rgb(17, 34, 51)", style.textColor);
  check("4.5 主题色含图标颜色", style.iconColor === "rgb(217, 48, 37)", style.iconColor);
  check("4.6 轮播无进度条", (await page.locator(".slogan-banner-progress").count()) === 0);

  // 短文本不滚
  await patchSlogan(page, { items: [{ id: "sl-1", text: "短标语", icon: "", createdAt: new Date().toISOString() }], activeIds: ["sl-1"] });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  check("4.7 短文本不滚", (await page.locator(".slogan-banner-text.marquee").count()) === 0);

  // 间隔配置：2 秒换一条
  await patchSlogan(page, {
    items: [
      { id: "sl-1", text: "第一帧", icon: "", createdAt: new Date().toISOString() },
      { id: "sl-2", text: "第二帧", icon: "", createdAt: new Date().toISOString() }
    ],
    activeIds: ["sl-1", "sl-2"],
    intervalSeconds: 2
  });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const first = await page.locator(".slogan-banner-text").innerText();
  await page.waitForTimeout(2600);
  const second = await page.locator(".slogan-banner-text").innerText();
  check("4.8 按配置间隔 2s 轮播", first === "第一帧" && second === "第二帧", J({ first, second }));

  // 工具页里改间隔落到状态
  await openSloganTool(page);
  const intervalInput = page.locator(".slogan-param-control input").nth(1);
  await intervalInput.fill("5");
  await intervalInput.press("Enter");
  await page.waitForTimeout(500);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("4.9 间隔配置落盘 5 秒", stored.slogan.intervalSeconds === 5, J(stored.slogan.intervalSeconds));
  check("4.10 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 6. 左右边界基准可配置（输入框边界 / 程序边界，默认输入框）
// ---------------------------------------------------------------------------
async function section6Bounds(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("a1", "entry-a", "一件事")],
    slogan: sloganSeed({ position: "top-left", bounds: "field" })
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const fieldLeft = await page.evaluate(() => {
    const banner = document.querySelector(".slogan-banner").getBoundingClientRect();
    const header = document.querySelector(".workspace .list-header").getBoundingClientRect();
    return { delta: Math.abs(banner.left - header.left), headerLeft: header.left };
  });
  check("6.1 默认跟输入框/卡片区边界对齐", fieldLeft.delta <= 3, J(fieldLeft));
  await openSloganTool(page);
  check("6.2 边界是二选一段控", (await page.locator(".slogan-bounds-switch button").count()) === 2);
  await page.locator(".slogan-bounds-switch button", { hasText: "程序边界" }).click();
  await page.waitForTimeout(400);
  await backToWorkspace(page);
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const appLeft = await page.evaluate(() => {
    const banner = document.querySelector(".slogan-banner").getBoundingClientRect();
    const root = document.querySelector(".workspace").getBoundingClientRect();
    const header = document.querySelector(".workspace .list-header").getBoundingClientRect();
    return { appDelta: Math.abs(banner.left - root.left), narrower: banner.left < header.left - 2 };
  });
  check("6.3 程序边界对齐到右半区左缘（比内容区更靠左）", appLeft.appDelta <= 3 && appLeft.narrower, J(appLeft));
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("6.4 边界模式落盘", stored.slogan.bounds === "app", J(stored.slogan.bounds));
  check("6.5 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 7. 无关闭叉号 + 悬浮全文只在超长（跑马灯）时给
// ---------------------------------------------------------------------------
async function section7NoXAndTitle(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [task("a1", "entry-a", "一件事")],
    slogan: sloganSeed({
      items: [{ id: "sl-1", text: "短标语", icon: "", createdAt: new Date().toISOString() }],
      activeIds: ["sl-1"]
    })
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  check("7.1 胶囊上没有关闭叉号", (await page.locator(".slogan-banner-x").count()) === 0);
  check("7.2 短标语不带悬浮全文", (await page.locator(".slogan-banner").getAttribute("title")) === null);
  const longText = "这是一条特别特别长的标语用于验证三十字跑马灯滚动效果展示超过上限之后的横向滚动播放";
  await patchSlogan(page, { items: [{ id: "sl-1", text: longText, icon: "", createdAt: new Date().toISOString() }], activeIds: ["sl-1"] });
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  await page.waitForTimeout(300);
  check("7.3 超长标语悬浮才显示全文", (await page.locator(".slogan-banner").getAttribute("title")) === longText);
  check("7.4 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 8. 任务右键「设置为标语」+ 默认图标（含 emoji）
// ---------------------------------------------------------------------------
async function section8TaskToSlogan(context) {
  const { page, errors } = await freshPage(context);
  await seedState(page, {
    nodes: [entryNode("entry-a", "记事")],
    tasks: [
      { id: "a1", nodeId: "entry-a", markdown: "## 多喝水\n第二行正文", completed: false, important: false, myDay: false, tags: [], emojis: [], expanded: false, createdAt: new Date().toISOString() }
    ],
    slogan: sloganSeed({ items: [], activeIds: [], defaultIcon: "" })
  });
  await page.locator(".task-card").first().waitFor({ state: "visible" });
  // 默认图标改成 emoji（图标菜单 → 选择 Emoji… → 点第一个）
  await openSloganTool(page);
  await page.locator(".slogan-icon-trigger").click();
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  await page.locator(".context-menu .menu-item-button", { hasText: "选择 Emoji…" }).click();
  await page.waitForSelector(".emoji-grid", { timeout: 5000 });
  await page.locator(".emoji-grid button").first().click();
  await page.waitForTimeout(500);
  const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  const defaultIcon = stored.slogan.defaultIcon;
  check("8.1 新标语默认图标可配（emoji）", typeof defaultIcon === "string" && defaultIcon.length > 0 && defaultIcon !== "none", J(defaultIcon));
  // 回到工作区，任务右键「设置为标语」
  await backToWorkspace(page);
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  check("8.2 任务菜单有「设置为标语」", (await page.locator(".context-menu .menu-item-button", { hasText: "设置为标语" }).count()) === 1);
  await page.locator(".context-menu .menu-item-button", { hasText: "设置为标语" }).click();
  await page.waitForTimeout(500);
  const stored2 = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  const added = stored2.slogan.items[stored2.slogan.items.length - 1];
  check("8.3 一键入列取第一行文字", added.text === "多喝水", J(added.text));
  check("8.4 新标语用默认图标", added.icon === defaultIcon, J({ icon: added.icon, defaultIcon }));
  check("8.5 新标语自动选中展示", stored2.slogan.activeIds.includes(added.id));
  // 悬浮层用 emoji 渲染
  await page.waitForSelector(".slogan-banner", { timeout: 4000 });
  const emojiText = await page.locator(".slogan-banner .slogan-emoji").first().innerText().catch(() => "");
  check("8.6 悬浮层渲染 emoji 图标", emojiText === defaultIcon, J(emojiText));
  // 重复添加给提示、不重复入列
  await page.locator(".task-card").first().click({ button: "right" });
  await page.waitForSelector(".context-menu", { timeout: 3000 });
  const before = stored2.slogan.items.length;
  await page.locator(".context-menu .menu-item-button", { hasText: "设置为标语" }).click();
  await page.waitForTimeout(400);
  const stored3 = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)), STATE_KEY);
  check("8.7 重复内容不重复入列", stored3.slogan.items.length === before, J({ before, after: stored3.slogan.items.length }));
  check("8.8 无页面错误", errors.length === 0, J(errors));
  await page.close();
}

// ---------------------------------------------------------------------------
// 5. 移动端：只维护不展示 + 360 扫
// ---------------------------------------------------------------------------
async function section5Mobile(context) {
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--no-sandbox", "--disable-gpu"] });
  const ctx = await browser.newContext({ userAgent: ANDROID_UA, hasTouch: true, isMobile: true, viewport: { width: 360, height: 800 } });
  const { page, errors } = await freshPage(ctx, { mobile: true });
  await seedState(page, { nodes: [entryNode("entry-a", "记事")], slogan: sloganSeed() });
  check("5.1 移动端不展示悬浮层", (await page.locator(".slogan-banner").count()) === 0);
  const nav = ".mobile-nav .nav-row, .system-nav .nav-row";
  await page.locator(nav, { hasText: "工具箱" }).first().click();
  await page.waitForSelector(".toolbox-view", { timeout: 5000 });
  await page.locator(".toolbox-card", { hasText: "标语栏" }).click();
  await page.waitForSelector(".slogan-tool", { timeout: 5000 });
  check("5.2 移动端六位置按钮", (await page.locator(".slogan-position-grid button").count()) === 6);
  const bad = await page.evaluate(() => {
    const out = [];
    const vw = document.documentElement.clientWidth;
    for (const el of document.querySelectorAll(".slogan-tool *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      if (rect.left < -1 || rect.right > vw + 1) out.push(`${el.className}`.slice(0, 60));
    }
    return out;
  });
  check("5.3 标语工具页 360 无溢出", bad.length === 0, J(bad.slice(0, 6)));
  check("5.4 无页面错误", errors.length === 0, J(errors));
  await browser.close();
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
try {
  await section1Positions(context);
  await section2NoInputPage(context);
  await section3ItemsAndEdit(context);
  await section4MarqueeTheme(context);
  await section6Bounds(context);
  await section7NoXAndTitle(context);
  await section8TaskToSlogan(context);
  await section5Mobile(context);
} finally {
  await browser.close();
}
console.log(`\n${passes} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
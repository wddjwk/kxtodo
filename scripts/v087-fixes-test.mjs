// v0.8.7 回归。逐条覆盖本版需求：1 浮层定位契约、2 取色体系、3 搜索、
// 4 移动端时刻双轨、6 杂项、追加需求 1（工具箱图标）/ 2（传输菜单与 relay）。
// 用法：node scripts/v087-fixes-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
const STATE_KEY = "todo-note-state-v3";
const SETTINGS_KEY = "todo-note-settings-v3";

let failures = 0;
let passes = 0;
function check(name, ok, extra = "") {
  if (ok) passes += 1;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra && !ok ? " — " + extra : ""}`);
  if (!ok) failures += 1;
}
const J = (value) => JSON.stringify(value);

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
  await page.waitForTimeout(400);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
  return { page, errors };
}

async function seedState(page, { nodes = [], tasks = [], settings = null } = {}) {
  await page.evaluate(
    ({ nodes, tasks, settings, stateKey, settingsKey }) => {
      const now = new Date().toISOString();
      const base = [
        { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
        { id: "planned", kind: "system", name: "计划内", icon: "calendar", parentId: null, createdAt: now },
        { id: "important", kind: "system", name: "收藏", icon: "star", parentId: null, createdAt: now },
        { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now }
      ];
      localStorage.setItem(
        stateKey,
        JSON.stringify({
          schemaVersion: 3,
          nodes: [...base, ...nodes],
          tasks,
          backgrounds: {},
          selectedNodeId: nodes[0]?.id ?? "entry-a"
        })
      );
      if (settings) {
        const existing = JSON.parse(localStorage.getItem(settingsKey) ?? "{}");
        localStorage.setItem(settingsKey, JSON.stringify({ ...existing, ...settings, schemaVersion: 3 }));
      }
    },
    { nodes, tasks, settings, stateKey: STATE_KEY, settingsKey: SETTINGS_KEY }
  );
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(700);
}

function makeTask(overrides) {
  const now = new Date().toISOString();
  return {
    id: `task-${Math.random().toString(16).slice(2, 10)}`,
    nodeId: "entry-a",
    markdown: "任务",
    completed: false,
    important: false,
    myDay: false,
    dueDate: "",
    dueTime: "",
    tags: [],
    emojis: [],
    expanded: false,
    createdAt: now,
    updatedAt: now,
    ...overrides
  };
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const mobile = await browser.newContext({
  viewport: { width: 392, height: 850 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true,
  deviceScaleFactor: 2
});

/** 桌面：打开列表菜单（右上角「列表菜单」按钮；移动端在「更多操作」面板里） */
async function openListMenu(page) {
  if ((await page.locator(".context-menu").count()) > 0) return;
  const direct = page.locator("button[title='列表菜单']");
  if ((await direct.count()) > 0) {
    await direct.click({ force: true });
  } else {
    await page.locator(".header-actions button[title='更多操作']").click({ force: true });
    await page.waitForTimeout(300);
    await page.locator(".header-menu-panel .menu-item-button", { hasText: "列表菜单" }).click({ force: true });
  }
  await page.waitForTimeout(350);
}

// ===========================================================================
// 需求 1：点锚定浮层的定位契约（下方贴鼠标 / 翻上底边贴锚点 / 无内部滚动）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  const tasks = Array.from({ length: 24 }, (_, index) => makeTask({ id: `t${index}`, markdown: `第 ${index} 张卡` }));
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }],
    tasks
  });

  // 上部右键：菜单在下方、左上角贴鼠标；且**不被镜像**
  await page.mouse.click(700, 150, { button: "right" });
  await page.waitForTimeout(400);
  const topCase = await page.evaluate(() => {
    const menu = document.querySelector(".context-menu");
    const box = menu?.getBoundingClientRect();
    return box
      ? {
          x: box.x,
          y: box.y,
          scrollable: menu.scrollHeight - menu.clientHeight,
          maxHeight: getComputedStyle(menu).maxHeight
        }
      : null;
  });
  check(
    "1 上部右键：菜单在下方、左上角贴鼠标（不镜像）",
    Boolean(topCase) && Math.abs(topCase.x - 700) <= 2 && Math.abs(topCase.y - 150) <= 2,
    J(topCase)
  );
  check("1 菜单内部没有滚动条（下方放得下）", Boolean(topCase) && topCase.scrollable <= 1, J(topCase));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);

  // 低位右键：菜单整体翻上、**底边精确贴光标**
  const lowBox = await page.evaluate(() => {
    const cards = [...document.querySelectorAll(".task-card")];
    const target = cards.find((card) => card.getBoundingClientRect().bottom > 700);
    if (!target) return null;
    const box = target.getBoundingClientRect();
    return { x: box.x + 60, y: box.y + box.height - 4 };
  });
  check("1 低位目标卡片在场（列表够长）", Boolean(lowBox), J(lowBox));
  if (lowBox) {
    await page.mouse.click(lowBox.x, lowBox.y, { button: "right" });
    await page.waitForTimeout(400);
    const lowCase = await page.evaluate(() => {
      const menu = document.querySelector(".context-menu");
      const box = menu?.getBoundingClientRect();
      return box
        ? {
            x: box.x,
            y: box.y,
            bottom: box.bottom,
            right: box.right,
            scrollable: menu.scrollHeight - menu.clientHeight,
            width: box.width
          }
        : null;
    });
    check(
      "1 低位右键：菜单翻上且底边精确贴光标",
      Boolean(lowCase) && Math.abs(lowCase.bottom - lowBox.y) <= 2,
      J({ lowCase, lowBox })
    );
    check("1 翻上后仍无内部滚动条", Boolean(lowCase) && lowCase.scrollable <= 1, J(lowCase));
    check("1 菜单不越过视口左右/上边", Boolean(lowCase) && lowCase.x >= 0 && lowCase.y >= 0 && lowCase.right <= 1440, J(lowCase));
    await page.keyboard.press("Escape");
    await page.waitForTimeout(250);
  }
  check("1 浮层契约无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 需求 2.2/2.3/2.5：取色盘本体（视觉口径、把手指贴光标、确认冲刷、吸管）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }]
  });
  await page.locator('.tree-row[data-node-id="entry-a"]').click();
  await page.waitForTimeout(400);

  await openListMenu(page);
  await page.locator(".palette-button").first().click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(450);

  const geometry = await page.evaluate(() => {
    const panel = document.querySelector(".kx-color-panel");
    const host = panel.querySelector(".kx-color-canvas");
    // iro 的指针靶子是 SV 盒（.IroBox）：它的**视觉尺寸 == 布局尺寸**才是「把手指不脱靶」的充要条件
    const box = panel.querySelector(".kx-color-canvas .IroBox");
    if (!panel || !host || !box) return null;
    const boxBox = box.getBoundingClientRect();
    const hostBox = host.getBoundingClientRect();
    const fields = panel.querySelector(".kx-color-fields")?.getBoundingClientRect();
    const panelBox = panel.getBoundingClientRect();
    return {
      boxVisual: [Math.round(boxBox.width), Math.round(boxBox.height)],
      boxLayout: [box.offsetWidth, box.offsetHeight],
      hostVisual: [Math.round(hostBox.width), Math.round(hostBox.height)],
      hostLayoutH: host.offsetHeight,
      fieldsTop: fields ? Math.round(fields.top) : -1,
      hostBottom: Math.round(hostBox.bottom),
      panelOverflow: panel.scrollHeight - panel.clientHeight,
      insideViewport: panelBox.x >= 0 && panelBox.y >= 0 && panelBox.right <= window.innerWidth && panelBox.bottom <= window.innerHeight,
      scale: getComputedStyle(document.querySelector(".app-shell")).getPropertyValue("--ui-scale").trim()
    };
  });
  check(
    "2.2 色盘视觉尺寸 == 布局尺寸（反缩放层生效，拖动不脱靶的前提）",
    Boolean(geometry) && Math.abs(geometry.boxVisual[0] - geometry.boxLayout[0]) <= 1 && Math.abs(geometry.boxVisual[1] - geometry.boxLayout[1]) <= 1,
    J(geometry)
  );
  check(
    "2.2 宿主视觉宽 == 色盘视觉宽（裁剪边精确落在色盘边上）",
    Boolean(geometry) && Math.abs(geometry.boxVisual[0] - geometry.hostVisual[0]) <= 1,
    J(geometry)
  );
  check(
    "2.2 宿主高度显式补偿（字段区在色盘下方，不被压住）",
    Boolean(geometry) && geometry.fieldsTop >= geometry.hostBottom - 1,
    J(geometry)
  );
  check("2.2 面板完整显示、无内部滚动条", Boolean(geometry) && geometry.insideViewport && geometry.panelOverflow <= 1, J(geometry));

  // 把手指贴光标：mousedown 到 SV 盒 80%/70% 处，读把手指圆心，几何差 < 3px
  const finger = await page.evaluate(async () => {
    const box = document.querySelector(".kx-color-canvas .IroBox");
    const rect = box?.getBoundingClientRect();
    if (!rect) return null;
    const targetX = rect.left + rect.width * 0.8;
    const targetY = rect.top + rect.height * 0.7;
    box.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, clientX: targetX, clientY: targetY }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const handle = document.querySelector(".kx-color-canvas .IroHandle");
    const circle = handle?.querySelector("circle") ?? handle;
    const handleBox = circle?.getBoundingClientRect();
    const result = handleBox
      ? {
          dx: Math.abs(handleBox.left + handleBox.width / 2 - targetX),
          dy: Math.abs(handleBox.top + handleBox.height / 2 - targetY),
          dxRaw: handleBox.left + handleBox.width / 2 - targetX
        }
      : null;
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, clientX: targetX, clientY: targetY }));
    return result;
  });
  check(
    "2.2 拖动时把手指精确贴光标（几何差 < 3px）",
    Boolean(finger) && finger.dx < 3 && finger.dy < 3,
    J(finger)
  );

  // 吸管：Chromium 系才渲染
  const eyedropper = await page.evaluate(() => {
    const button = [...document.querySelectorAll(".kx-color-actions .menu-action-button")].find((el) =>
      (el.textContent ?? "").includes("吸管")
    );
    return { has: Boolean(button), api: "EyeDropper" in window };
  });
  check("2.5 吸管按钮在场（有 EyeDropper API 的平台）", eyedropper.has === eyedropper.api, J(eyedropper));

  // 2.3 确认前冲刷：fill HEX → **不按 Enter** → 立刻点确认 → 颜色必须生效
  await page.locator(".kx-color-hex").fill("");
  await page.locator(".kx-color-hex").fill("#123456");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const flushed = await page.evaluate(() => {
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return { background: state.backgrounds?.["entry-a"]?.color ?? "(none)", panel: Boolean(document.querySelector(".kx-color-panel")) };
  });
  check("2.3 输入完立刻确认：颜色生效（不靠 blur/Enter 兜底）", flushed.background === "#123456" && !flushed.panel, J(flushed));

  check("2 取色盘无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 需求 2.4 / 2.6 / 2.7：同入口重开、三个颜色入口、预设编辑器即输即生效
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }]
  });
  await page.locator('.tree-row[data-node-id="entry-a"]').click();
  await page.waitForTimeout(400);

  // ---- 2.4 同入口重开：旧草稿不带进新会话（面板重挂、输入框回初始色）----
  await openListMenu(page);
  await page.locator(".ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#abcdef");
  await page.waitForTimeout(150);
  await page.locator(".ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForTimeout(500);
  const reopened = await page.evaluate(() => {
    const panel = document.querySelector(".kx-color-panel");
    return { hex: panel?.querySelector(".kx-color-hex")?.value ?? "", count: document.querySelectorAll(".kx-color-panel").length };
  });
  check("2.4 同入口重开：面板重挂、旧草稿不带进新会话", reopened.count === 1 && reopened.hex !== "#abcdef" && /^#[0-9a-f]{6}$/.test(reopened.hex), J(reopened));

  // ---- 2.6 主题色：拖动 = 预览，取消回退，确认才落盘 ----
  const dragged = await page.evaluate(async () => {
    const box = document.querySelector(".kx-color-canvas .IroBox");
    const rect = box.getBoundingClientRect();
    const x = rect.left + rect.width * 0.25;
    const y = rect.top + rect.height * 0.5;
    box.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    await new Promise((resolve) => setTimeout(resolve, 120));
    return {
      hex: document.querySelector(".kx-color-hex")?.value ?? "",
      accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
    };
  });
  check("2.6 主题色拖动：界面实时预览（--accent 跟着走）", /^#[0-9a-f]{6}$/.test(dragged.hex) && dragged.accent === dragged.hex, J(dragged));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  const reverted = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return {
      accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim(),
      stored: settings.appearance?.uiColors?.["entry-a"] ?? "(none)",
      panel: Boolean(document.querySelector(".kx-color-panel"))
    };
  });
  check("2.6 取消：预览回退且不落盘", !reverted.panel && reverted.accent !== dragged.hex && reverted.stored === "(none)", J({ reverted, dragged }));
  // Esc 会连菜单一起收掉（取消语义），要再开一次菜单才能点色块
  await openListMenu(page);
  await page.locator(".ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#336699");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const committed = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return {
      stored: settings.appearance?.uiColors?.["entry-a"] ?? "(none)",
      accent: getComputedStyle(document.querySelector(".workspace")).getPropertyValue("--accent").trim()
    };
  });
  check("2.6 确认才落盘（主题色）", committed.stored === "#336699" && committed.accent === "#336699", J(committed));

  // ---- 2.7 预设编辑器：无保存/取消、名称停输即写、颜色框即输即用、色盘预览/取消/确认 ----
  await openListMenu(page);
  const presetBefore = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return settings.appearance?.themePresets ?? null;
  });
  await page.locator(".color-grid button[title*='右键编辑']").first().click({ button: "right" });
  await page.waitForSelector(".preset-editor", { timeout: 5000 });
  const editorUi = await page.evaluate(() => ({
    actions: document.querySelectorAll(".preset-editor-actions").length,
    draftBars: document.querySelectorAll(".color-draft-actions").length,
    title: document.querySelector(".preset-editor-title")?.textContent?.trim() ?? ""
  }));
  check("2.7 编辑器没有保存/取消按钮（旧草稿条也全没了）", editorUi.actions === 0 && editorUi.draftBars === 0, J(editorUi));

  await page.locator(".preset-editor input").first().fill("我的底色");
  await page.waitForTimeout(500);
  const afterName = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return settings.appearance?.themePresets?.[0]?.name ?? "(none)";
  });
  check("2.7 名称停输 300ms 自动落盘（不逐键写）", afterName === "我的底色", J(afterName));

  await page.locator(".preset-color-line input").fill("#123321");
  await page.waitForTimeout(400);
  const afterColorInput = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return {
      preset: settings.appearance?.themePresets?.[0]?.color ?? "(none)",
      background: state.backgrounds?.["entry-a"]?.color ?? "(none)"
    };
  });
  check("2.7 颜色框合法即写（themePresets + 应用为当前背景）", afterColorInput.preset === "#123321" && afterColorInput.background === "#123321", J(afterColorInput));

  // 编辑器色块 → 色盘：拖动时**工作区背景实时变**
  await page.locator(".preset-color-line .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  const presetDrag = await page.evaluate(async () => {
    const box = document.querySelector(".kx-color-canvas .IroBox");
    const rect = box.getBoundingClientRect();
    const x = rect.left + rect.width * 0.7;
    const y = rect.top + rect.height * 0.3;
    box.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    await new Promise((resolve) => setTimeout(resolve, 150));
    const hex = document.querySelector(".kx-color-hex")?.value ?? "";
    // 工作区背景的内联样式会被序列化成 rgb(...)，把 hex 也换算过去比
    const rgb = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(hex);
    const expected = rgb ? `rgb(${Number.parseInt(rgb[1], 16)}, ${Number.parseInt(rgb[2], 16)}, ${Number.parseInt(rgb[3], 16)})` : "";
    return {
      hex,
      expected,
      // --swatch 声明在内层 span 上（需求 2.8）
      swatch: getComputedStyle(document.querySelector(".preset-color-line .ui-color-picker span")).getPropertyValue("--swatch").trim(),
      background: document.querySelector(".workspace")?.getAttribute("style") ?? ""
    };
  });
  check(
    "2.7 编辑器色盘拖动：编辑器草稿 + 背景预览双写",
    /^#[0-9a-f]{6}$/.test(presetDrag.hex) &&
      presetDrag.hex !== "#123321" &&
      presetDrag.swatch === presetDrag.hex &&
      presetDrag.background.includes(presetDrag.expected),
    J(presetDrag)
  );
  // 取消走「点浮层外」这条路：Esc 会连菜单一起收掉，测不到编辑器的草稿还原
  await page.locator(".preset-editor-title").click();
  await page.waitForTimeout(400);
  const presetCancel = await page.evaluate(() => {
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return {
      panel: Boolean(document.querySelector(".kx-color-panel")),
      swatch: getComputedStyle(document.querySelector(".preset-color-line .ui-color-picker span")).getPropertyValue("--swatch").trim(),
      background: state.backgrounds?.["entry-a"]?.color ?? "(none)"
    };
  });
  check(
    "2.7 色盘取消（点浮层外）：编辑器草稿与背景都还原",
    !presetCancel.panel && presetCancel.swatch === "#123321" && presetCancel.background === "#123321",
    J(presetCancel)
  );

  await page.locator(".preset-color-line .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#445566");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const presetConfirm = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    const state = JSON.parse(localStorage.getItem("todo-note-state-v3") ?? "{}");
    return {
      preset: settings.appearance?.themePresets?.[0]?.color ?? "(none)",
      name: settings.appearance?.themePresets?.[0]?.name ?? "(none)",
      background: state.backgrounds?.["entry-a"]?.color ?? "(none)"
    };
  });
  check(
    "2.7 色盘确认：写进 themePresets（名称草稿一并带上）并应用为背景",
    presetConfirm.preset === "#445566" && presetConfirm.name === "我的底色" && presetConfirm.background === "#445566",
    J(presetConfirm)
  );
  const presetCount = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    const after = settings.appearance?.themePresets ?? [];
    return { sameLength: after.length === (window.__presetLen ?? after.length), before: null };
  });
  check("2.7 预设数量不变（只改当前一项）", Boolean(presetBefore) && presetCount.sameLength, J(presetCount));

  // ---- 2.8 预设色块 CSS：饱满胶囊（不是 18×18 方块）----
  const swatchCss = await page.evaluate(() => {
    const preset = document.querySelector(".color-grid button[title*='右键编辑']");
    const uiSwatch = document.querySelector(".ui-color-row .ui-color-picker");
    const uiSpan = uiSwatch?.querySelector("span");
    const editorSwatch = document.querySelector(".preset-color-line .ui-color-picker");
    const spanFills = (button, span) => {
      if (!button || !span) return false;
      const a = button.getBoundingClientRect();
      const b = span.getBoundingClientRect();
      // span 铺满的是按钮的**内容盒**（按钮还有 1px 边框），差 ≤4 逻辑像素即算铺满；
      // 出问题时的样子是 span 被 UA 内边距挤成 ~18px 的小方块，一眼能分辨
      return a.width - b.width <= 4 && a.height - b.height <= 4 && b.width >= a.width * 0.6;
    };
    return {
      presetHeight: preset ? getComputedStyle(preset).height : "",
      presetBg: preset ? getComputedStyle(preset).backgroundColor : "",
      uiPadding: uiSwatch ? getComputedStyle(uiSwatch).padding : "",
      uiRadius: uiSwatch ? getComputedStyle(uiSwatch).borderRadius : "",
      uiSpanFills: spanFills(uiSwatch, uiSpan),
      uiSpanRadius: uiSpan ? getComputedStyle(uiSpan).borderRadius : "",
      editorPadding: editorSwatch ? getComputedStyle(editorSwatch).padding : ""
    };
  });
  check(
    "2.8 色块是饱满胶囊（padding 0、span 铺满、圆角同步、无方块）",
    swatchCss.presetHeight === "30px" &&
      swatchCss.presetBg !== "rgba(0, 0, 0, 0)" &&
      swatchCss.uiPadding === "0px" &&
      swatchCss.uiSpanFills &&
      swatchCss.uiRadius === "999px" &&
      swatchCss.editorPadding === "0px",
    J(swatchCss)
  );

  check("2 取色体系无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 需求 2.6（续）：系统视图（我的一天）改临期配色 → 卡片实时变、保存后保持
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }],
    tasks: [makeTask({ id: "task-today", markdown: "今天到期", dueDate: localToday(), dueTime: "", myDay: true })],
    settings: { features: { dueHighlight: "solid" } }
  });
  await page.locator(".system-nav .nav-row", { hasText: "我的一天" }).click();
  await page.waitForTimeout(600);
  const beforeDue = await page.evaluate(() => document.querySelector(".task-card")?.getAttribute("style") ?? "");
  check("2.6 我的一天里卡片有临期底色（默认配色）", beforeDue.includes("--due-color"), J(beforeDue));

  await openListMenu(page);
  const dueRow = await page.evaluate(() => document.querySelectorAll(".due-color-row .ui-color-picker").length);
  check("2.6 视图菜单里有临期高亮色四档", dueRow === 4, String(dueRow));
  await page.locator(".due-color-row .ui-color-picker").nth(1).click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#00a000");
  await page.locator(".kx-color-hex").press("Enter");
  await page.waitForTimeout(300);
  const previewDue = await page.evaluate(() => document.querySelector(".task-card")?.getAttribute("style") ?? "");
  check("2.6 系统视图预览实时染卡片（修好「预览不染」）", previewDue.includes("#00a000"), J(previewDue));
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(700);
  const savedDue = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return {
      stored: settings.appearance?.dueColors?.["my-day"] ?? null,
      card: document.querySelector(".task-card")?.getAttribute("style") ?? ""
    };
  });
  check(
    "2.6 保存后卡片保持新色（视图键 my-day 落盘）",
    Array.isArray(savedDue.stored) && savedDue.stored[1] === "#00a000" && savedDue.card.includes("#00a000"),
    J(savedDue)
  );
  check("2.6 系统视图无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 追加需求 1：工具箱与工具子页的头部图标 = 页面标题口径（34）
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: new Date().toISOString() }]
  });
  // 先在条目页量一次「页面标题图标」的基准
  await page.locator('.tree-row[data-node-id="entry-a"]').click();
  await page.waitForTimeout(400);
  const reference = await page.evaluate(() => {
    const svg = document.querySelector(".workspace .header-icon svg");
    const box = svg?.getBoundingClientRect();
    return box ? Math.round(box.height) : -1;
  });
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(500);
  const toolboxHeader = await page.evaluate(() => {
    const svg = document.querySelector(".toolbox-header .toolbox-header-icon svg");
    const box = svg?.getBoundingClientRect();
    return box ? Math.round(box.height) : -1;
  });
  await page.locator(".toolbox-card").first().click();
  await page.waitForTimeout(500);
  const toolHeader = await page.evaluate(() => {
    const svg = document.querySelector(".toolbox-sub-bar .toolbox-header-icon svg");
    const box = svg?.getBoundingClientRect();
    return box ? Math.round(box.height) : -1;
  });
  check(
    "追加1 工具箱头部图标 = 页面标题口径（与我的一天一致）",
    reference > 0 && toolboxHeader === reference,
    J({ reference, toolboxHeader })
  );
  check("追加1 工具子页头部图标同口径", toolHeader === reference, J({ reference, toolHeader }));
  check("追加1 无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 追加需求 2：传输工具 ⋯ 菜单（配置在上 + 分割线 + UI颜色在下）+ relay 三选一
// ===========================================================================
{
  const { page, errors } = await freshPage(desktop);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: new Date().toISOString() }]
  });
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(500);
  await page.evaluate(() => {
    const card = [...document.querySelectorAll(".toolbox-card")].find((item) => (item.textContent ?? "").includes("传输"));
    card?.click();
  });
  await page.waitForTimeout(600);
  check("追加2 传输工具页打开", (await page.locator(".transfer-page").count()) === 1);
  await page.locator(".toolbox-sub-bar button[title='外观']").click({ force: true });
  await page.waitForTimeout(400);

  // 定位复用：贴在按钮正下方 + 右缘对齐按钮右缘（与「我的一天」那些页面的列表菜单同一套）
  const anchored = await page.evaluate(() => {
    const button = document.querySelector(".toolbox-sub-bar button[title='外观']");
    const menu = document.querySelector(".context-menu");
    if (!button || !menu) return null;
    const b = button.getBoundingClientRect();
    const m = menu.getBoundingClientRect();
    return {
      rightGap: Math.round(Math.abs(m.right - b.right)),
      topGap: Math.round(Math.abs(m.top - (b.bottom + 6))),
      menuTop: Math.round(m.top),
      buttonBottom: Math.round(b.bottom)
    };
  });
  check(
    "追加2 菜单贴在三点按钮下方、右缘对齐按钮右缘（复用现有锚定逻辑）",
    Boolean(anchored) && anchored.rightGap <= 2 && anchored.topGap <= 2,
    J(anchored)
  );

  const menuStructure = await page.evaluate(() => {
    const menu = document.querySelector(".context-menu");
    const kids = [...(menu?.children ?? [])];
    const relayIndex = kids.findIndex((el) => (el.textContent ?? "").includes("relay 服务"));
    const sepIndex = kids.findIndex((el) => el.classList.contains("menu-separator"));
    const uiIndex = kids.findIndex((el) => (el.textContent ?? "").includes("UI颜色"));
    const bgIndex = kids.findIndex((el) => (el.textContent ?? "").includes("背景颜色"));
    return { relayIndex, sepIndex, uiIndex, bgIndex, total: kids.length };
  });
  check(
    "追加2 结构与普通页面同一套：配置在上 → 分割线 → UI颜色/背景颜色在下",
    menuStructure.relayIndex === 0 &&
      menuStructure.sepIndex > menuStructure.relayIndex &&
      menuStructure.uiIndex > menuStructure.sepIndex &&
      menuStructure.bgIndex > menuStructure.uiIndex,
    J(menuStructure)
  );

  await page.locator(".context-menu .menu-item-button", { hasText: "relay 服务" }).click();
  await page.waitForTimeout(400);
  const relayItems = await page.evaluate(() => {
    const panel = document.querySelector(".submenu-panel");
    const items = [...panel.querySelectorAll(".menu-item-button")];
    return items.map((item) => {
      const check = item.querySelector(".menu-item-check");
      const label = item.querySelector(".menu-item-label");
      return {
        text: (item.textContent ?? "").replace("✓", "").trim(),
        disabled: item.hasAttribute("disabled"),
        checkVisible: check ? getComputedStyle(check).visibility : "none",
        checkLeftOfLabel: check && label ? check.getBoundingClientRect().left < label.getBoundingClientRect().left : false
      };
    });
  });
  check(
    "追加2 relay 三选一：复用同步配置 / 使用默认服务 / 自定义服务",
    relayItems.length === 3 &&
      relayItems[0].text.includes("复用同步配置") &&
      relayItems[1].text.includes("使用默认服务") &&
      relayItems[2].text.includes("自定义服务"),
    J(relayItems)
  );
  check("追加2 勾选图标在左侧显示", relayItems.every((item) => item.checkLeftOfLabel), J(relayItems));
  check("追加2 默认勾选在「使用默认服务」", relayItems[1].checkVisible === "visible" && relayItems[0].checkVisible === "hidden" && relayItems[2].checkVisible === "hidden", J(relayItems));
  check("追加2 同步未开启时「复用同步配置」灰着不可点", relayItems[0].disabled === true, J(relayItems));
  const relayHint = await page.evaluate(() => document.querySelector(".relay-hint")?.textContent?.trim() ?? "");
  check("追加2 提示只留「下次上线时生效」", relayHint === "下次上线时生效", J(relayHint));

  const widthBefore = await page.evaluate(() => document.querySelector(".submenu-panel")?.offsetWidth ?? -1);
  await page.locator(".submenu-panel .menu-item-button", { hasText: "自定义服务" }).click();
  await page.waitForTimeout(350);
  const widthCustom = await page.evaluate(() => document.querySelector(".submenu-panel")?.offsetWidth ?? -1);
  await page.locator(".submenu-panel .menu-item-button", { hasText: "使用默认服务" }).click();
  await page.waitForTimeout(350);
  const widthDefault = await page.evaluate(() => ({
    width: document.querySelector(".submenu-panel")?.offsetWidth ?? -1,
    stored: JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}").transfer?.relay ?? "(none)"
  }));
  check("追加2 点不同选项子菜单宽度不变", widthBefore === widthCustom && widthCustom === widthDefault.width, J({ widthBefore, widthCustom, widthDefault }));
  check("追加2「使用默认服务」写进 settings.transfer.relay = default", widthDefault.stored === "default", J(widthDefault));

  // ---- 每个工具单独调色：改传输工具的主题色，只写它自己那一键 ----
  await page.locator(".context-menu .ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#336699");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const toolColor = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    const view = document.querySelector(".toolbox-view");
    return {
      transfer: settings.toolbox?.toolAccents?.transfer ?? "(none)",
      base: settings.toolbox?.accent ?? "",
      accent: getComputedStyle(view).getPropertyValue("--accent").trim()
    };
  });
  check(
    "追加2 工具子页改主题色写进 toolAccents[工具id]，主界面那层不动",
    toolColor.transfer === "#336699" && toolColor.base === "" && toolColor.accent === "#336699",
    J(toolColor)
  );

  // 再点一次 ⋯ 收起（toggle，与记账/日记的齿轮同语义）
  await page.locator(".toolbox-sub-bar button[title='外观']").click({ force: true });
  await page.waitForTimeout(350);
  check("追加2 ⋯ 再点一次收起（toggle）", (await page.locator(".context-menu").count()) === 0);

  // 回主界面：主界面有自己的 ⋯ 菜单，且主题色**不受工具子页影响**
  await page.locator(".toolbox-sub-bar button[title='返回工具箱']").click({ force: true });
  await page.waitForTimeout(450);
  const mainHeader = await page.evaluate(() => {
    const button = document.querySelector(".toolbox-header button[title='外观']");
    const view = document.querySelector(".toolbox-view");
    return { hasButton: Boolean(button), accent: getComputedStyle(view).getPropertyValue("--accent").trim() };
  });
  check("追加2 工具箱主界面也有 ⋯ 菜单按钮", mainHeader.hasButton, J(mainHeader));
  check("追加2 主界面主题色不受工具子页改动影响（各调各的）", mainHeader.accent !== "#336699", J(mainHeader));
  await page.locator(".toolbox-header button[title='外观']").click({ force: true });
  await page.waitForTimeout(400);
  const mainMenu = await page.evaluate(() => {
    const menu = document.querySelector(".context-menu");
    return {
      ui: (menu?.textContent ?? "").includes("UI颜色"),
      background: (menu?.textContent ?? "").includes("背景颜色"),
      relay: (menu?.textContent ?? "").includes("relay 服务"),
      swatch: menu?.querySelector(".ui-color-row .ui-color-picker span")
        ? getComputedStyle(menu.querySelector(".ui-color-row .ui-color-picker span")).getPropertyValue("--swatch").trim()
        : ""
    };
  });
  check(
    "追加2 主界面 ⋯ 菜单有 UI颜色 + 背景颜色（没有 relay：那只在传输页）",
    mainMenu.ui && mainMenu.background && !mainMenu.relay,
    J(mainMenu)
  );
  // 主界面改主题色 → 只写 toolbox.accent（两层互不干扰）
  await page.locator(".context-menu .ui-color-row .ui-color-picker").click({ force: true });
  await page.waitForSelector(".kx-color-panel", { timeout: 8000 });
  await page.waitForTimeout(400);
  await page.locator(".kx-color-hex").fill("#884422");
  await page.locator("[data-color-confirm]").click();
  await page.waitForTimeout(600);
  const mainColor = await page.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem("todo-note-settings-v3") ?? "{}");
    return {
      base: settings.toolbox?.accent ?? "",
      transfer: settings.toolbox?.toolAccents?.transfer ?? "(none)"
    };
  });
  check(
    "追加2 主界面改主题色只写 toolbox.accent（工具那层原样保留）",
    mainColor.base === "#884422" && mainColor.transfer === "#336699",
    J(mainColor)
  );
  check("追加2 传输菜单无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 需求 4：时刻双轨（overscroll 不冒泡 + 定宽）
// ===========================================================================
for (const [name, context] of [["桌面", desktop], ["移动端", mobile]]) {
  const { page, errors } = await freshPage(context);
  const now = new Date().toISOString();
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: now }],
    tasks: [makeTask({ id: "task-time", markdown: "带时刻的卡", dueDate: localToday(2), dueTime: "09:00" })]
  });
  // 移动端先点进条目（列表页里卡片还没挂出来）
  await page.locator(".tree-row").first().click();
  await page.waitForTimeout(600);
  await page.locator(".task-card").first().locator(".task-due-date").click({ force: true });
  await page.waitForTimeout(500);
  const opened = await page.evaluate(() => Boolean(document.querySelector(".task-date-popover")));
  check(`4（${name}）日期浮层打开`, opened);
  await page.locator(".task-date-popover .dp-time-trigger").first().click({ force: true });
  await page.waitForSelector(".task-date-popover .time-picker", { timeout: 5000 });
  await page.waitForTimeout(300);
  const wheel = await page.evaluate(() => {
    const picker = document.querySelector(".task-date-popover .time-picker");
    const col = picker.querySelector(".time-col");
    const fontControl = Number.parseFloat(
      getComputedStyle(document.querySelector(".app-shell")).getPropertyValue("--font-control")
    );
    return {
      width: Number.parseFloat(getComputedStyle(picker).width),
      expected: fontControl * 6,
      // offsetWidth 是布局像素（rect 是缩放后的视觉像素，别混用两套口径）
      colWidth: col.offsetWidth,
      overscroll: getComputedStyle(col).overscrollBehaviorY,
      band: document.querySelector(".task-date-popover .time-band")?.offsetWidth ?? -1,
      pickerVisual: Math.round(picker.getBoundingClientRect().width)
    };
  });
  check(
    `4（${name}）双轨定宽（6 个字宽，两列各约 3 字）`,
    Math.abs(wheel.width - wheel.expected) <= 1 && wheel.colWidth >= wheel.expected / 2 - 8,
    J(wheel)
  );
  check(`4（${name}）时间列 overscroll-behavior: contain（滑动不冒泡到下拉同步）`, wheel.overscroll === "contain", J(wheel));
  check(`4（${name}）灰带与双轨同宽（不用单独改）`, Math.abs(wheel.band - wheel.width) <= 10, J(wheel));
  check(`4（${name}）无脚本报错`, errors.length === 0, errors[0] ?? "");
  await page.close();
}

// ===========================================================================
// 追加需求 2（移动端）：主界面与工具子页的 ⋯ 菜单同样锚定在按钮上
// ===========================================================================
{
  const { page, errors } = await freshPage(mobile);
  await seedState(page, {
    nodes: [{ id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: null, createdAt: new Date().toISOString() }]
  });
  await page.locator(".system-nav .nav-row", { hasText: "工具箱" }).click();
  await page.waitForTimeout(600);

  async function anchoredGap(selector) {
    return page.evaluate((sel) => {
      const button = document.querySelector(sel);
      const menu = document.querySelector(".context-menu");
      if (!button || !menu) return null;
      const b = button.getBoundingClientRect();
      const m = menu.getBoundingClientRect();
      return {
        rightGap: Math.round(Math.abs(m.right - b.right)),
        topGap: Math.round(Math.abs(m.top - (b.bottom + 6))),
        insideViewport: m.x >= 0 && m.y >= 0 && m.right <= window.innerWidth && m.bottom <= window.innerHeight
      };
    }, selector);
  }

  check("追加2（移动端）工具箱主界面有 ⋯ 按钮", (await page.locator(".toolbox-header button[title='外观']").count()) === 1);
  await page.locator(".toolbox-header button[title='外观']").click({ force: true });
  await page.waitForTimeout(400);
  const mainGap = await anchoredGap(".toolbox-header button[title='外观']");
  check(
    "追加2（移动端）主界面菜单锚在按钮下方、右缘对齐、不出屏",
    Boolean(mainGap) && mainGap.rightGap <= 2 && mainGap.topGap <= 2 && mainGap.insideViewport,
    J(mainGap)
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);

  await page.locator(".toolbox-card").first().click();
  await page.waitForTimeout(500);
  await page.locator(".toolbox-sub-bar button[title='外观']").click({ force: true });
  await page.waitForTimeout(400);
  const toolGap = await anchoredGap(".toolbox-sub-bar button[title='外观']");
  check(
    "追加2（移动端）工具子页菜单同样锚定",
    Boolean(toolGap) && toolGap.rightGap <= 2 && toolGap.topGap <= 2 && toolGap.insideViewport,
    J(toolGap)
  );
  check("追加2（移动端）无脚本报错", errors.length === 0, errors[0] ?? "");
  await page.close();
}

console.log(`\n合计：${passes} 通过 / ${failures} 失败`);
await browser.close();
process.exit(failures === 0 ? 0 : 1);

// v0.8.10 定向回归：剪切板界面重做（开关 / 说明浮层 / 双区设备勾选 / 窄屏不溢出）、
// 无内联说明、保存与校验。需要先 npm run dev（vite 1420）。
// 发现链路的真机回归在 scripts 之外：core 单测 + kklaptop 上的 clipboard_probe。
import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const url = process.env.KXTODO_TEST_URL || "http://127.0.0.1:1420/";
const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
let passes = 0;
let failures = 0;
function check(name, ok, detail = "") {
  if (ok) {
    passes += 1;
    console.log(`  PASS ${name}`);
  } else {
    failures += 1;
    console.log(`  FAIL ${name}${detail ? ` :: ${detail}` : ""}`);
  }
}
const J = (value) => JSON.stringify(value);

async function freshPage({ width = 1440, height = 1000, mobile = false } = {}) {
  const context = await browser.newContext(
    mobile
      ? {
          viewport: { width, height },
          isMobile: true,
          hasTouch: true,
          userAgent:
            "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
        }
      : { viewport: { width, height } }
  );
  const page = await context.newPage();
  page.setDefaultNavigationTimeout(120000);
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.addInitScript(() => {
    const now = new Date().toISOString();
    const nodes = [
      { id: "my-day", kind: "system", name: "我的一天", icon: "sun", parentId: null, createdAt: now },
      { id: "scheduled", kind: "system", name: "定时任务", icon: "clock", parentId: null, createdAt: now },
      { id: "cat-a", kind: "category", name: "分组甲", icon: "folder", parentId: null, createdAt: now, expanded: true },
      { id: "entry-a", kind: "entry", name: "条目甲", icon: "inbox", parentId: "cat-a", createdAt: now }
    ];
    localStorage.setItem("todo-note-state-v3", JSON.stringify({
      nodes,
      tasks: [],
      backgrounds: {},
      selectedNodeId: "entry-a"
    }));
    localStorage.setItem("todo-note-settings-v3", JSON.stringify({
      appearance: { uiScale: 1 },
      features: { sync: false },
      updates: { autoCheck: false }
    }));
  });
  await page.goto(url, { waitUntil: "load" });
  await page.waitForTimeout(600);
  return { context, page };
}

async function openClipboard(page, mobile) {
  if (mobile) await page.locator('.sidebar .nav-row[title="工具箱"]').click();
  else await page.locator('.nav-row[title="工具箱"]').click();
  await page.waitForTimeout(300);
  await page.locator(".toolbox-card", { hasText: "剪切板同步" }).click();
  await page.waitForTimeout(400);
}

function withinViewport(rect, viewport) {
  return rect.x >= -1 && rect.x + rect.width <= viewport.width + 1 &&
    rect.y >= -1 && rect.y + rect.height <= viewport.height + 1;
}

// ---------------------------------------------------------------------------
// 1. 桌面：标准开关、说明浮层、无内联注释、保存校验
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage();
  await openClipboard(page, false);
  check("1 工具页打开", (await page.locator(".clipboard-page").count()) === 1);
  check("1 无内联说明段落", (await page.locator(".clipboard-hint, .clipboard-note").count()) === 0);

  const switchSize = await page.locator(".clipboard-hero .ui-switch").evaluate((el) => ({
    width: el.offsetWidth,
    height: el.offsetHeight
  }));
  check("1 总开关是 42×24 标准开关", switchSize.width === 42 && switchSize.height === 24, J(switchSize));

  await page.locator(".clipboard-hero .clipboard-icon-button[title='说明']").click();
  const info = page.locator(".clipboard-info");
  check("1 ⓘ 打开统一说明", (await info.count()) === 1);
  const infoText = await info.innerText();
  check("1 说明覆盖配对/安全/安卓限制",
    infoText.includes("局域网") && infoText.includes("统一服务器") && infoText.includes("P2P") &&
    infoText.includes("Android") && infoText.includes("不保存"));
  const infoRect = await info.boundingBox();
  check("1 说明浮层在视口内", withinViewport(infoRect, { width: 1440, height: 1000 }), J(infoRect));
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  if ((await info.count()) > 0) await page.locator(".clipboard-info-head button").click();
  check("1 关闭说明", (await info.count()) === 0);

  // 开箱即用：默认只开局域网
  await page.locator(".clipboard-hero .ui-switch").click();
  await page.waitForTimeout(500);
  let stored = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).clipboard);
  check("1 总开关一键落盘、默认局域网", stored.enabled === true && stored.lanEnabled === true, J(stored));

  // 广域网缺账户密码时保存被拒
  await page.locator(".clipboard-inline-switch", { hasText: "P2P" }).locator(".ui-switch").click();
  await page.waitForTimeout(200);
  check("1 开启广域网后出现共用账户密码", (await page.locator(".clipboard-form-grid input").count()) === 2);
  await page.locator(".clipboard-actions button").click();
  await page.waitForTimeout(400);
  stored = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).clipboard);
  check("1 缺凭据不落盘", stored.wanP2pEnabled === false, J(stored));

  // 填齐服务器与账户
  await page.locator(".clipboard-inline-switch", { hasText: "统一服务器" }).locator(".ui-switch").click();
  await page.waitForTimeout(150);
  await page.locator(".clipboard-server-url").fill("192.168.31.102:52177");
  const accountInputs = page.locator(".clipboard-form-grid input");
  await accountInputs.nth(0).fill("demo-user");
  await accountInputs.nth(1).fill("demo-pass");
  await page.locator(".clipboard-actions button").click();
  await page.waitForTimeout(500);
  stored = await page.evaluate(() => JSON.parse(localStorage.getItem("todo-note-settings-v3")).clipboard);
  check("1 广域网账户/密码/服务器落盘",
    stored.serverEnabled === true && stored.account === "demo-user" && stored.password === "demo-pass" &&
    stored.serverUrl === "192.168.31.102:52177", J(stored));

  // 折叠同步方式
  await page.locator(".clipboard-section-head", { hasText: "同步方式" }).click();
  await page.waitForTimeout(150);
  check("1 同步方式可折叠", (await page.locator(".clipboard-method").count()) === 0);
  await page.locator(".clipboard-section-head", { hasText: "同步方式" }).click();
  await page.waitForTimeout(150);
  check("1 同步方式可展开", (await page.locator(".clipboard-method").count()) === 2);

  const noOverflow = await page.locator(".clipboard-page").evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
  check("1 桌面无横向溢出", noOverflow);
  await context.close();
}

// ---------------------------------------------------------------------------
// 2. 窄屏（360 / 392）：默认态、广域网展开、长设备名与三段标签都不越界
// ---------------------------------------------------------------------------
for (const width of [360, 392]) {
  const { context, page } = await freshPage({ width, height: 820, mobile: true });
  await openClipboard(page, true);
  await page.locator(".clipboard-hero .ui-switch").click();
  await page.waitForTimeout(300);
  // 展开广域网 + 接收内容，把最长的表单形态画出来
  await page.locator(".clipboard-inline-switch", { hasText: "统一服务器" }).locator(".ui-switch").click();
  await page.locator(".clipboard-inline-switch", { hasText: "P2P" }).locator(".ui-switch").click();
  await page.waitForTimeout(150);
  await page.locator(".clipboard-form-grid input").nth(0).fill("一个很长很长的账户名字");
  await page.locator(".clipboard-form-grid input").nth(1).fill("password");
  await page.locator(".clipboard-section-head", { hasText: "接收内容" }).click();
  await page.waitForTimeout(150);

  // 注入运行中的状态：长设备名 + 三段传输标签 + 未勾选态
  await page.evaluate(() => {
    window.__kxtodoClipboardStatus.set({
      running: true,
      enabled: true,
      deviceId: "abcdefghijklmnopqrstuvwxyz234567",
      deviceName: "kxtodo·Android(abcdefgh)",
      port: 52179,
      discoveryPort: 52178,
      localAddresses: ["192.168.31.102"],
      transports: { lan: true, server: true, p2p: true },
      serverRegistered: true,
      p2pActive: true,
      peers: [
        {
          id: "peer-long",
          name: "这是一台名字非常非常长的设备用来测试安卓窄屏下的换行与标签",
          transports: ["lan", "server", "p2p"],
          selected: true
        },
        { id: "peer-2", name: "书房电脑", transports: ["server"], selected: false }
      ],
      lastError: "",
      lastEvent: { at: new Date().toISOString(), kind: "sent", detail: "已发送到 2 台设备", peer: "" },
      sent: 2,
      received: 0,
      saveDir: "/tmp/clipboard"
    });
  });
  await page.waitForTimeout(300);

  check(`2（${width}）运行态与服务器/P2P 徽章`, (await page.locator(".clipboard-status-text").innerText()).trim() === "运行中" &&
    (await page.locator(".clipboard-badge.on").count()) === 2);
  check(`2（${width}）局域网设备列表含长名字`, (await page.locator(".clipboard-peer-row", { hasText: "这是一台名字" }).count()) >= 1);
  // 长名字在局域网与广域网各出现一次（同一设备按传输分组）
  check(`2（${width}）广域网设备列表存在`, (await page.locator(".clipboard-peer-row", { hasText: "书房电脑" }).count()) >= 1);
  const selectedOff = await page.locator(".clipboard-peer-row", { hasText: "书房电脑" }).first().evaluate((el) => el.classList.contains("off"));
  check(`2（${width}）未勾选设备有淡出态`, selectedOff);

  const overflow = await page.evaluate(() => {
    const viewport = window.innerWidth;
    const bad = [];
    for (const el of document.querySelectorAll(".clipboard-page, .clipboard-page *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && (rect.left < -1 || rect.right > viewport + 1)) {
        bad.push(`${el.className || el.tagName}:${Math.round(rect.left)}..${Math.round(rect.right)}/${viewport}`);
      }
    }
    return {
      pageOverflow: document.documentElement.scrollWidth > viewport + 1,
      innerOverflow: document.querySelector(".clipboard-page").scrollWidth > document.querySelector(".clipboard-page").clientWidth + 1,
      bad: bad.slice(0, 6)
    };
  });
  check(`2（${width}）所有元素都在视口内且无横向滚动`, !overflow.pageOverflow && !overflow.innerOverflow && overflow.bad.length === 0, J(overflow));

  // 勾选切换立即生效（浏览器档 setClipboardPeer 是本地 no-op，先乐观更新）
  const row = page.locator(".clipboard-peer-row", { hasText: "书房电脑" }).first();
  await row.locator('input[type="checkbox"]').check();
  await page.waitForTimeout(250);
  const nowOn = await row.evaluate((el) => !el.classList.contains("off"));
  check(`2（${width}）勾选设备即时生效`, nowOn);

  // 刷新按钮不应抛错/卡住
  await page.locator(".clipboard-refresh").click();
  await page.waitForTimeout(400);
  check(`2（${width}）刷新按钮安全返回`, (await page.locator(".clipboard-page").count()) === 1);
  await context.close();
}

// ---------------------------------------------------------------------------
// 3. 360 窄屏说明浮层也不越界（统一滚动容器）
// ---------------------------------------------------------------------------
{
  const { context, page } = await freshPage({ width: 360, height: 780, mobile: true });
  await openClipboard(page, true);
  await page.locator(".clipboard-hero .clipboard-icon-button[title='说明']").click();
  await page.waitForTimeout(200);
  const rect = await page.locator(".clipboard-info").boundingBox();
  check("3 窄屏说明浮层不越界", withinViewport(rect, { width: 360, height: 780 }), J(rect));
  const scrollable = await page.locator(".clipboard-info").evaluate((el) => el.scrollHeight <= el.clientHeight + 1 || getComputedStyle(el).overflowY === "auto");
  check("3 说明内容可滚动", scrollable);
  await context.close();
}

console.log(`\n合计：${passes} 通过 / ${failures} 失败`);
if (errors.length) {
  console.log("脚本错误：");
  for (const error of errors) console.log(" -", error);
}
await browser.close();
process.exit(failures === 0 && errors.length === 0 ? 0 : 1);

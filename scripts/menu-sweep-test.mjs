// 菜单/浮层巡检回归：桌面 + 窄屏移动模拟下逐个唤起右键菜单、三点菜单、齿轮面板、
// 编辑器元数据浮层与二级子菜单，断言全部落在视口内（越界=安卓天气框那类 bug 复发），
// 并确认全程无 pageerror。用法：node scripts/menu-sweep-test.mjs（需先 npm run dev）。
import { chromium } from "playwright-core";

const URL = "http://127.0.0.1:1420/";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

let failures = 0;
function check(name, ok, extra = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? " — " + extra : ""}`);
  if (!ok) failures++;
}

const FLOATERS = ".context-menu, .header-menu-panel, .diary-gear-panel, .editor-meta-pop, .submenu-panel";

async function assertFloatersInViewport(page, label) {
  const boxes = await page.evaluate((selector) => {
    return [...document.querySelectorAll(selector)]
      .filter((el) => el.offsetParent !== null || getComputedStyle(el).position === "fixed")
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { cls: el.className.toString().slice(0, 40), x: r.x, y: r.y, right: r.right, bottom: r.bottom, w: r.width, h: r.height };
      });
  }, FLOATERS);
  const vw = page.viewportSize().width;
  const vh = page.viewportSize().height;
  for (const box of boxes) {
    const inside = box.x >= -1 && box.y >= -1 && box.right <= vw + 1 && box.bottom <= vh + 1 && box.w > 0;
    check(`${label} 浮层在视口内 [${box.cls}]`, inside, JSON.stringify(box));
  }
  if (boxes.length === 0) check(`${label} 有浮层打开`, false, "没量到任何浮层");
  return boxes.length;
}

async function closeAll(page) {
  await page.keyboard.press("Escape").catch(() => {});
  await page.waitForTimeout(150);
  await page.mouse.click(5, 5).catch(() => {});
  await page.waitForTimeout(150);
}

async function openCardMenu(page, selector, mobile) {
  const target = page.locator(selector).first();
  if (!mobile) {
    await target.click({ button: "right" });
    await page.waitForTimeout(300);
    return;
  }
  // 移动端真实手势是长按：合成 touch pointerdown 按住 600ms 再抬手
  const box = await target.boundingBox();
  const x = Math.round(box.x + box.width / 2);
  const y = Math.round(box.y + Math.min(40, box.height / 2));
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
    await new Promise((r) => setTimeout(r, 620));
    el.dispatchEvent(new PointerEvent("pointerup", opts));
  }, { x, y });
  await page.waitForTimeout(300);
}

async function seed(page, mobile) {
  await page.goto(URL, { waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(700);
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "load", timeout: 90000 });
  await page.waitForTimeout(700);
  if (mobile) {
    await page.locator(".custom-nav .tree-row", { hasText: "收集箱" }).first().click();
    await page.waitForTimeout(500);
  }
  await page.locator(".add-task-bar textarea").fill("巡检任务一条");
  await page.locator(".add-task-bar textarea").press("Enter");
  await page.waitForTimeout(500);
}

async function sweep(page, mobile, tag) {
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));

  // 任务卡片菜单（桌面右键 / 移动长按）+ 标签子菜单
  await openCardMenu(page, ".task-card", mobile);
  await assertFloatersInViewport(page, `${tag} 任务右键菜单`);
  await page.locator(".context-menu .menu-item-button", { hasText: "标签" }).first().click();
  await page.waitForTimeout(300);
  await assertFloatersInViewport(page, `${tag} 标签子菜单`);
  await closeAll(page);

  // 列表三点菜单 + 移动子菜单
  if (mobile) {
    await page.locator(".workspace .header-actions > button").first().click();
  } else {
    await page.locator(".workspace .header-actions button[title='列表菜单'], .workspace .header-actions > button").last().click();
  }
  await page.waitForTimeout(300);
  const gearOrMenu = await page.locator(".header-menu-panel, .context-menu").count();
  check(`${tag} 三点/齿轮入口有响应`, gearOrMenu > 0);
  await assertFloatersInViewport(page, `${tag} 列表菜单`);
  const moveItem = page.locator(".context-menu .menu-item-button", { hasText: "移动到分组" });
  if (await moveItem.count()) {
    await moveItem.first().click();
    await page.waitForTimeout(300);
    await assertFloatersInViewport(page, `${tag} 移动子菜单`);
  }
  await closeAll(page);

  // 编辑器元数据浮层（日期/表情或心情/天气/标签）
  if (mobile) {
    await page.locator(".task-card").first().dblclick();
  } else {
    await page.locator(".task-card .edit-button").first().click();
  }
  await page.waitForTimeout(900);
  for (const title of ["添加日期", "标签"]) {
    const trigger = page.locator(".editor-meta-trigger", { hasText: title }).first();
    if (!(await trigger.count())) continue;
    await trigger.click();
    await page.waitForTimeout(300);
    await assertFloatersInViewport(page, `${tag} 编辑器浮层[${title}]`);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);
  }
  await page.locator(".editor-actions .editor-icon-button").last().click();
  await page.waitForTimeout(400);

  // 日记：齿轮面板 + 日记菜单 + 卡片菜单的天气子菜单
  if (mobile) {
    await page.goBack();
    await page.waitForTimeout(400);
  }
  await page.locator(".system-nav .nav-row", { hasText: "日记" }).click();
  await page.waitForTimeout(600);
  await page.locator(".diary-fab").click();
  await page.waitForTimeout(900);
  for (const title of ["归属日期", "天气", "标签"]) {
    const trigger = page.locator(".editor-meta-trigger", { hasText: title }).first();
    if (!(await trigger.count())) continue;
    await trigger.click();
    await page.waitForTimeout(300);
    await assertFloatersInViewport(page, `${tag} 日记编辑器浮层[${title}]`);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);
  }
  // 写进内容再保存：空草稿关掉即消失，后面的日记卡片菜单就没有目标了
  await page.locator(".editor-title-input").fill("巡检日记");
  await page.keyboard.type("巡检正文内容");
  await page.locator(".editor-actions .editor-icon-button.primary").click();
  await page.waitForTimeout(600);

  await page.locator(".diary-view .header-actions > button").first().click();
  await page.waitForTimeout(300);
  await assertFloatersInViewport(page, `${tag} 日记齿轮面板`);
  await page.locator(".diary-gear-panel .menu-item-button", { hasText: "日记菜单" }).click();
  await page.waitForTimeout(400);
  await assertFloatersInViewport(page, `${tag} 日记三点菜单`);
  await closeAll(page);

  await openCardMenu(page, ".diary-card", mobile);
  await assertFloatersInViewport(page, `${tag} 日记卡片菜单`);
  const weather = page.locator(".context-menu .menu-item-button", { hasText: "天气" });
  if (await weather.count()) {
    await weather.first().click();
    await page.waitForTimeout(300);
    await assertFloatersInViewport(page, `${tag} 天气子菜单`);
  }
  await closeAll(page);

  check(`${tag} 全程无 pageerror`, errors.length === 0, errors.slice(0, 3).join(" | "));
}

const browser = await chromium.launch({ channel: "msedge", headless: true });

const desktop = await browser.newContext({ viewport: { width: 1280, height: 860 } });
const dpage = await desktop.newPage();
await seed(dpage, false);
await sweep(dpage, false, "桌面");
await desktop.close();

// 窄屏移动模拟：越界问题（天气框伸出屏幕）最容易在这里复发
const mob = await browser.newContext({
  viewport: { width: 360, height: 740 },
  userAgent: ANDROID_UA,
  hasTouch: true,
  isMobile: true,
  deviceScaleFactor: 2
});
const mpage = await mob.newPage();
await seed(mpage, true);
await sweep(mpage, true, "移动");
await mob.close();

await browser.close();
console.log(failures === 0 ? "MENU SWEEP ALL PASS" : `MENU SWEEP FAILURES: ${failures}`);
process.exit(failures === 0 ? 0 : 1);

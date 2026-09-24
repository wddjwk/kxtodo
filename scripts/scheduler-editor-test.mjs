// Focused browser scheduler regression. Run centrally against an existing Vite dev server.
// node scripts/scheduler-editor-test.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const url = process.env.KXTODO_TEST_URL || "http://127.0.0.1:1420/";
const browser = await chromium.launch({ channel: "msedge", headless: true });
const key = "todo-note-scheduler-v8";
async function ready(page) {
  await page.goto(url, { waitUntil: "load" });
  await page.evaluate(async () => {
    const { isHydrated } = await import("/src/lib/stores.ts");
    await new Promise((resolve) => {
      let stop;
      stop = isHydrated.subscribe((value) => { if (value) { resolve(); queueMicrotask(() => stop?.()); } });
    });
  });
}
async function persisted(page, editing) {
  await page.waitForFunction(({ key, editing }) => JSON.parse(localStorage.getItem(key) || "{}").tasks?.[0]?.editing === editing, { key, editing });
}
try {
  for (const mobile of [false, true]) {
    const context = await browser.newContext(mobile ? {
      viewport: { width: 392, height: 850 }, isMobile: true, hasTouch: true,
      userAgent: "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
    } : { viewport: { width: 1440, height: 1000 } });
    const page = await context.newPage();
    page.setDefaultNavigationTimeout(120000);
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    await ready(page);
    await page.locator('.sidebar .nav-row[title="定时任务"]').click();
    await page.locator('.scheduler-floating-add').click();
    await page.locator('.scheduler-step-content').waitFor();
    assert.equal(await page.locator('.scheduler-step-content').count(), 1);
    assert.equal(await page.locator('.scheduler-trigger-tile').count(), mobile ? 3 : 4);
    const boxes = await page.locator('.scheduler-trigger-tile').evaluateAll((tiles) => tiles.map((tile) => {
      const rect = tile.getBoundingClientRect(); return { top: rect.top, right: rect.right };
    }));
    assert.ok(Math.max(...boxes.map((b) => b.top)) - Math.min(...boxes.map((b) => b.top)) < 2, "trigger tiles should share one row");
    await page.locator('.scheduler-sentence button').nth(1).click();
    assert.equal(await page.locator('.scheduler-step-content').count(), 1);
    await page.locator('.action-editor').waitFor();
    if (mobile) {
      assert.equal(await page.locator('.action-editor textarea.notification-message').count(), 1);
      assert.equal(await page.locator('.notification-followups').count(), 0);
    } else {
      await page.getByText("参数、工作目录、超时与通知", { exact: true }).click();
      await page.locator('.notification-followups > .checkbox-line').nth(1).locator('input').check();
      await page.locator('.notification-followups input[placeholder="例如 DONE 或 ^ok"]').fill("DONE");
    }
    await page.locator('.scheduler-footer-actions button[type="submit"]').click();
    await persisted(page, false);
    await page.locator('.scheduler-config-button').click();
    await persisted(page, true);
    await page.reload({ waitUntil: "load" });
    if (mobile) await page.locator('.sidebar .nav-row[title="定时任务"]').click();
    await page.locator('.scheduler-editor').waitFor();
    assert.equal(await page.locator('.scheduler-step-content').count(), 1, "editing survives reload without expanding every step");
    const stored = await page.evaluate((key) => JSON.parse(localStorage.getItem(key)).tasks[0], key);
    assert.equal(stored.editing, true);
    if (!mobile) assert.equal(stored.action.stdoutNotification.condition.pattern, "DONE");
    await page.locator('.scheduler-footer-name input').fill("Unsaved scheduler draft");
    assert.equal(await page.locator('.scheduler-footer-actions button').filter({ hasText: "试运行" }).isDisabled(), true, "trial must not silently save/reschedule a dirty draft");
    assert.equal(await page.locator('.scheduler-sentence button').count(), 3);
    if (mobile) {
      const overflow = await page.locator('.scheduler-panel').evaluate((panel) => panel.scrollWidth > panel.clientWidth + 1);
      assert.equal(overflow, false, "mobile scheduler must not overflow horizontally");
    }
    assert.deepEqual(errors, []);
    await context.close();
  }
  console.log("PASS scheduler guided editor, disclosures, mobile bounds and editing persistence");
} finally { await browser.close(); }

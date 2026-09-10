import assert from "node:assert/strict";
import test from "node:test";
import { chromium, firefox, webkit } from "@playwright/test";
import { focusTargetByKeyboard } from "../scripts/browser-verify.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";

for (const [name, engine] of Object.entries({ chromium, firefox, webkit })) {
  test(`${name}: bounded keyboard qualification uses the real skip link`, { timeout: 120_000 }, async (t) => {
    const browser = await engine.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      for (const reachable of [true, false]) {
        await t.test(reachable ? "bypasses more than 160 navigation links" : "does not qualify a non-tabbable target", async () => {
          const page = await browser.newPage();
          try {
            await page.setContent(`<a href="#content" ${reachable ? "" : 'onclick="event.preventDefault()"'}>Skip to content</a><nav>${Array.from({ length: 210 }, (_, index) => `<a href="#nav-${index}">Navigation ${index}</a>`).join("")}</nav><main><h1 id="content">Content</h1><a id="target" href="#destination" tabindex="${reachable ? 0 : -1}">Preview</a></main>`);
            const target = page.locator("#target");
            if (reachable) {
              await focusTargetByKeyboard(page, target, name);
              assert.equal(await target.evaluate((element) => document.activeElement === element), true);
            } else {
              await assert.rejects(focusTargetByKeyboard(page, target, name), /not reached within bounded keyboard traversal/);
            }
          } finally { await page.close(); }
        });
      }
    } finally { await browser.close(); }
  });
}

import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { chromium, firefox, webkit } from "@playwright/test";
import { focusTargetByKeyboard } from "../scripts/browser-verify.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { PORTAL_ACCENT_BACKGROUNDS, renderPrimitiveTokenCss, validatePrimitiveTokens } from "../scripts/lib.mjs";

test("custom accents reach rendered utility consumers across skins and appearances", async () => {
  const tokens = { schema_version: 1, light: { accent: "#005f56" }, dark: { accent: "#72e2cf" } };
  validatePrimitiveTokens(tokens, "signal");
  const css = await readFile(new URL("../src/styles/utility-tokens.css", import.meta.url), "utf8")
    + await readFile(new URL("../src/styles/portal.css", import.meta.url), "utf8")
    + renderPrimitiveTokenCss(tokens);
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    await page.setContent(`<style>${css}</style><a id="link" style="color:var(--sl-color-text-accent)">Source</a><span id="strong" style="color:var(--cf-accent-strong)">Emphasis</span>`);
    for (const [skin, modes] of Object.entries(PORTAL_ACCENT_BACKGROUNDS)) {
      for (const [mode, backgrounds] of Object.entries(modes)) {
        await page.evaluate(({ skin, mode }) => {
          document.documentElement.dataset.cfpSkin = skin;
          document.documentElement.dataset.theme = mode;
        }, { skin, mode });
        const actual = await page.evaluate(() => {
          const style = getComputedStyle(document.documentElement);
          return {
            backgrounds: ["--cf-surface", "--cf-surface-raised", "--cf-surface-subtle", "--cf-accent-soft"].map((key) => style.getPropertyValue(key).trim()),
            colors: ["link", "strong"].map((id) => getComputedStyle(document.getElementById(id)).color),
          };
        });
        assert.deepEqual(actual.backgrounds, backgrounds, `${skin}/${mode}: validator matches rendered backgrounds`);
        const expected = mode === "light" ? "rgb(0, 95, 86)" : "rgb(114, 226, 207)";
        assert.deepEqual(actual.colors, [expected, expected], `${skin}/${mode}: configured accent is visible`);
      }
    }
  } finally { await browser.close(); }
});

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

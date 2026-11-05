// The review rail renders the page's earlier feedback as text (TSK-347). A
// note kind holding markup must show literally and never become an element,
// in both shipped copies of the design system kit. Borrows the portal's
// pinned Playwright like design-kit-check.mjs: run
// `npm run deps:install --prefix docs-portal` first, then
// `node --test scripts/design-kit-escape.test.mjs` from the repository root.
import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { hardenedChildEnvironment } from "../docs-portal/scripts/process-environment.mjs";

const REPOSITORY = fileURLToPath(new URL("..", import.meta.url));
const { chromium } = createRequire(path.join(REPOSITORY, "docs-portal/package.json"))("@playwright/test");
const KITS = ["cf-present", "cf-docs-portal"].map((skill) => [skill, path.join(REPOSITORY, "assets/base/agents/skills", skill, "resources/design-system")]);
const MARKUP = '<img src="x" data-cf-injected onerror="window.__cfInjected = 1">';

for (const [skill, kit] of KITS) {
  test(`${skill}: an earlier note kind holding markup renders as text`, { skip: process.platform === "win32", timeout: 120_000 }, async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "cf-kit-escape-"));
    const browser = await chromium.launch({ env: hardenedChildEnvironment() });
    try {
      await cp(kit, dir, { recursive: true });
      const page = path.join(dir, "present.reference.html");
      const html = await readFile(page, "utf8");
      const seeded = html.replace('"kind": "element"', `"kind": ${JSON.stringify(MARKUP)}`);
      assert.notEqual(seeded, html, "the reference page no longer carries an element note to seed");
      await writeFile(page, seeded);

      const tab = await browser.newPage();
      await tab.goto(pathToFileURL(page).href);
      await tab.waitForFunction(() => window.CF && !document.documentElement.hasAttribute("data-cf-preload"));
      await tab.waitForTimeout(300);
      const seen = await tab.evaluate(() => {
        const body = document.querySelector("[data-cf-earlier-body]");
        return {
          injected: body.querySelectorAll("[data-cf-injected], img").length,
          ran: window.__cfInjected === 1,
          targets: [...body.querySelectorAll(".cf-note-target")].map((el) => el.textContent),
        };
      });
      assert.equal(seen.injected, 0, "the note kind became an element");
      assert.equal(seen.ran, false, "the note kind ran script");
      assert.ok(seen.targets.some((text) => text.startsWith(`${MARKUP} · `)), `the note kind did not show literally: ${JSON.stringify(seen.targets)}`);
    } finally {
      await browser.close();
      await rm(dir, { recursive: true, force: true });
    }
  });
}

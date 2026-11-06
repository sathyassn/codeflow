// The design system kit writes page data into HTML only after escaping it
// (TSK-347). An earlier note kind holding markup must show literally in the
// review rail, and a heading id holding a quote must stay inside the table of
// contents link's attributes, in both shipped copies of the kit. Borrows the
// portal's pinned Playwright like design-kit-check.mjs: run
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
// The id as the DOM holds it, and as the page's HTML attribute writes it.
const HOSTILE_ID = 'hostile" data-cf-injected="toc';
const HOSTILE_ID_ATTRIBUTE = HOSTILE_ID.replaceAll('"', "&quot;");

/** Copy the kit, seed one reference page, open it and return what `probe` reads. */
async function seeded(kit, page, from, to, probe) {
  const dir = await mkdtemp(path.join(os.tmpdir(), "cf-kit-escape-"));
  const browser = await chromium.launch({ env: hardenedChildEnvironment() });
  try {
    await cp(kit, dir, { recursive: true });
    const file = path.join(dir, page);
    const html = await readFile(file, "utf8");
    const changed = html.replace(from, to);
    assert.notEqual(changed, html, `${page} no longer carries ${from} to seed`);
    await writeFile(file, changed);
    const tab = await browser.newPage();
    await tab.goto(pathToFileURL(file).href);
    await tab.waitForFunction(() => window.CF && !document.documentElement.hasAttribute("data-cf-preload"));
    await tab.waitForTimeout(300);
    return await tab.evaluate(probe);
  } finally {
    await browser.close();
    await rm(dir, { recursive: true, force: true });
  }
}

for (const [skill, kit] of KITS) {
  test(`${skill}: an earlier note kind holding markup renders as text`, { skip: process.platform === "win32", timeout: 120_000 }, async () => {
    const seen = await seeded(kit, "present.reference.html", '"kind": "element"', `"kind": ${JSON.stringify(MARKUP)}`, () => {
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
  });

  test(`${skill}: a heading id holding a quote stays inside its contents link`, { skip: process.platform === "win32", timeout: 120_000 }, async () => {
    const seen = await seeded(kit, "portal.reference.html", 'id="two-landing-paths"', `id="${HOSTILE_ID_ATTRIBUTE}"`, () => {
      const links = [...document.querySelectorAll("[data-cf-toc] .cf-toc-link")];
      return {
        injected: document.querySelectorAll("[data-cf-toc] [data-cf-injected]").length,
        targets: links.map((a) => a.dataset.target),
        hrefs: links.map((a) => a.getAttribute("href")),
      };
    });
    assert.equal(seen.injected, 0, "the heading id added an attribute to the contents link");
    assert.ok(seen.targets.includes(HOSTILE_ID), `no contents link targets the heading: ${JSON.stringify(seen.targets)}`);
    assert.ok(seen.hrefs.includes(`#${HOSTILE_ID}`), `no contents link points at the heading: ${JSON.stringify(seen.hrefs)}`);
  });
}

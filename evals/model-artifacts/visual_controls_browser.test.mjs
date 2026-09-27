// TSK-062 controls on a real render: each faulty answer fails exactly the
// figure rules recorded for it and each passing answer holds every rule,
// read off Chromium at 1280 and 390 px in light and dark with facts
// re-derived from the case's shipped fixture, the way
// figure-rules-browser.test.mjs reads the specimens (graphite skin, 720 px
// content box). Every figure a committed method answer carries is rendered
// too and must hold every rule.
//
// Run: npm run deps:install --prefix docs-portal
//      npx --prefix docs-portal playwright install chromium   (once)
//      node --test evals/model-artifacts/visual_controls_browser.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { controls, declarationOf, fixtureOf, grammar, methodAnswer, methodControls, prepare } from "./visual_controls.mjs";

const { chromium } = await import(new URL("../../docs-portal/node_modules/@playwright/test/index.mjs", import.meta.url));
const { hardenedChildEnvironment } = await import(new URL("../../docs-portal/scripts/process-environment.mjs", import.meta.url));

const styles = new URL("../../docs-portal/src/styles/", import.meta.url);
const css = (await Promise.all(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"].map((name) => readFile(new URL(name, styles), "utf8")))).join("\n");

async function probe(page, html) {
  const document = (theme) => `<!doctype html><html data-theme="${theme}" data-cfp-skin="graphite"><head><style>${css} body{margin:0;background:var(--cf-canvas);font-family:var(--cf-font-sans)} main{box-sizing:content-box;max-width:720px;margin:0 auto;padding:0 16px}</style></head><body><main>${html}</main></body></html>`;
  const observed = {};
  for (const [label, width, theme] of [["wide", 1280, "light"], ["narrow", 390, "light"], ["wideDark", 1280, "dark"], ["narrowDark", 390, "dark"]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(document(theme));
    observed[label] = (await page.evaluate(grammar.probeFigures, { clearance: grammar.THRESHOLDS.labelClearancePx }))[0];
  }
  return observed;
}

test("every control fails exactly the figure rules recorded for it", { skip: process.platform === "win32", timeout: 600_000 }, async () => {
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  const outcome = {};
  const expected = {};
  try {
    const page = await browser.newPage();
    for (const [caseId, entries] of Object.entries(controls.cases)) {
      const { readSource } = fixtureOf(caseId);
      for (const entry of entries) {
        const { html, evidence, composed } = prepare(await declarationOf(caseId, entry.declaration), readSource);
        const failures = grammar.figureRuleFailures({ ...(await probe(page, html)), evidence, composed });
        const key = `${caseId} ${entry.role} ${entry.declaration}`;
        outcome[key] = [...new Set(failures.map((failure) => failure.rule))].sort((a, b) => a - b);
        expected[key] = entry.rules;
      }
    }
    // Every figure a committed method answer carries holds every rule.
    for (const [caseId, entries] of Object.entries(methodControls.cases)) {
      for (const entry of entries) {
        if (!/\.json$|\/(passing|faulty)$/.test(entry.answer)) continue;
        const { figures, readSource } = await methodAnswer(caseId, entry);
        for (const declaration of figures) {
          const { html, evidence, composed } = prepare(declaration, readSource);
          const failures = grammar.figureRuleFailures({ ...(await probe(page, html)), evidence, composed });
          const key = `${caseId} method ${entry.answer} ${declaration.figure.id}`;
          outcome[key] = [...new Set(failures.map((failure) => failure.rule))].sort((a, b) => a - b);
          expected[key] = [];
        }
      }
    }
  } finally { await browser.close(); }
  assert.equal(Object.keys(outcome).length, Object.values(controls.cases).flat().length + 5);
  assert.ok(Object.values(outcome).some((rules) => rules.length > 0), "no control failed any rule");
  assert.deepEqual(outcome, expected);
});

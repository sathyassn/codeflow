// Each of the twelve figure rules, read off a real render. The figures render
// with the portal's own sheets, so what the probe reads is what a reader sees.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { canonicalJson, FIGURE_RULES, figureRuleFailures, probeFigures, renderFigure, THRESHOLDS } from "../scripts/figure-grammar.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { specimens } from "./page-shapes.mjs";

const styles = path.join(path.dirname(fileURLToPath(import.meta.url)), "../src/styles");

async function sheet() {
  const parts = await Promise.all(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"].map((name) => readFile(path.join(styles, name), "utf8")));
  return parts.join("\n");
}

async function probe(page, css, html, breakage = null) {
  const document = (theme) => `<!doctype html><html data-theme="${theme}" data-cfp-skin="instrument"><head><style>${css} body{margin:0;background:var(--cf-canvas);font-family:var(--cf-font-sans)} main{max-width:720px;margin:0 auto;padding:0 16px}</style></head><body><main>${html}</main></body></html>`;
  const observed = {};
  for (const [label, width, theme] of [["wide", 1280, "light"], ["narrow", 390, "light"], ["wideDark", 1280, "dark"], ["narrowDark", 390, "dark"]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(document(theme));
    if (breakage) await page.evaluate(breakage);
    observed[label] = (await page.evaluate(probeFigures, { clearance: THRESHOLDS.labelClearancePx }))[0];
  }
  return observed;
}

test("each of the twelve rules fails a figure built to break it", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const all = await specimens();
  const layering = all.find(({ name }) => name === "03-layering.json").declaration;
  const flow = all.find(({ name }) => name === "01-flow.json").declaration;
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    const html = renderFigure(layering, { idPrefix: "g" });
    const clean = await probe(page, css, html);
    assert.deepEqual(figureRuleFailures(clean), [], "the layering specimen holds every rule");
    const rulesOf = (observed, evidence = null) => [...new Set(figureRuleFailures({ ...observed, evidence }).map((failure) => failure.rule))];
    const breakages = {
      1: () => document.querySelector("figure.cf-fig").setAttribute("data-cf-figure", ""),
      2: () => document.querySelector(".cf-legend li").remove(),
      4: () => { for (const text of document.querySelectorAll(".cf-fig text")) text.style.fontSize = "8px"; },
      5: () => { const [wide, narrow] = document.querySelectorAll(".cf-fig-svg"); narrow.innerHTML = wide.innerHTML; narrow.setAttribute("viewBox", wide.getAttribute("viewBox")); },
      7: () => { for (const svg of document.querySelectorAll(".cf-fig-svg")) for (const group of svg.querySelectorAll("[data-state]")) group.replaceChildren(Object.assign(document.createElementNS("http://www.w3.org/2000/svg", "rect"), {})); },
      8: () => { for (const svg of document.querySelectorAll(".cf-fig-svg")) { const [a, b] = svg.querySelectorAll("text"); b.setAttribute("x", a.getAttribute("x")); b.setAttribute("y", a.getAttribute("y")); } },
      9: () => { document.querySelector(".cf-fig-caption").textContent = "First sentence. Second sentence."; },
      10: () => document.querySelector(".cf-fig-svg [data-state] *").setAttribute("fill", "#ff0000"),
      11: () => { for (const desc of document.querySelectorAll(".cf-fig-svg desc")) desc.remove(); },
      12: () => document.querySelector("details.cf-twin").remove(),
    };
    for (const [rule, breakage] of Object.entries(breakages)) {
      const observed = await probe(page, css, html, breakage);
      assert.ok(rulesOf(observed).includes(Number(rule)), `rule ${rule} (${FIGURE_RULES[rule]}) did not fail: ${JSON.stringify(figureRuleFailures(observed))}`);
    }
    // Rule 3: the flow specimen tells done from stop by shape alone.
    const flowObserved = await probe(page, css, renderFigure(flow, { idPrefix: "f" }));
    assert.ok(figureRuleFailures(flowObserved).some((failure) => failure.rule === 3 && /states done and stop differ on shape, need 2/.test(failure.message)));
    // Rule 6: the evidence re-derives a value the figure does not draw.
    const fact = layering.figure.facts[0];
    assert.ok(rulesOf(clean, { facts: [{ claim: fact.claim, source: fact.source, drawn: fact.value, derived: false, matches: false }] }).includes(6));
    assert.ok(rulesOf(clean, { facts: [], data: null }).includes(6));
  } finally { await browser.close(); }
});

test("the specimens that hold every rule, and the doctrine conflicts the gate names in the rest", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  const outcome = {};
  try {
    const page = await browser.newPage();
    for (const { name, declaration } of await specimens()) {
      const bound = declaration.figure.binding === "derived" ? { source: declaration.figure.source, derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
      const failures = figureRuleFailures(await probe(page, css, renderFigure(declaration, { idPrefix: "s", bound })));
      outcome[name] = [...new Set(failures.map((failure) => failure.rule))].sort((a, b) => a - b);
    }
  } finally { await browser.close(); }
  assert.deepEqual(outcome, {
    "01-flow.json": [3],
    "02-structure.json": [],
    "03-layering.json": [],
    "04-sequence.json": [],
    "05-state.json": [],
    "06-coverage.json": [],
    "07-extent.json": [],
    "08-derivation.json": [8],
    "09-graph.json": [3, 8],
    "10-extent-derived.json": [],
  }, canonicalJson(outcome));
});

// Each of the twelve figure rules, read off a real render. The figures render
// with the portal's own sheets, so what the probe reads is what a reader sees.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";
import { canonicalJson, composeFigure, FIGURE_RULES, figureRuleFailures, probeFigures, renderFigure, THRESHOLDS } from "../scripts/figure-grammar.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { specimens } from "./page-shapes.mjs";

const styles = path.join(path.dirname(fileURLToPath(import.meta.url)), "../src/styles");

async function sheet() {
  const parts = await Promise.all(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"].map((name) => readFile(path.join(styles, name), "utf8")));
  return parts.join("\n");
}

// The committed values a specimen is checked against: its authored facts, and
// for the derived specimen the policy values its bars encode.
const POLICY = { commit_desc_max_len: 50, commit_subject_max_len: 72 };
function fidelity(declaration) {
  const bound = declaration.figure.binding === "derived" ? { source: declaration.figure.source, derived: POLICY } : null;
  const composed = composeFigure(declaration, bound);
  const facts = declaration.figure.facts.map((fact) => ({ claim: fact.claim, source: fact.source, drawn: fact.value, derived: fact.value, matches: true }));
  return { bound, composed, evidence: { facts, data: bound === null ? null : { drawn: composed.drawnValues, derived: POLICY } } };
}

async function probe(page, css, html, breakage = null, argument = undefined) {
  const document = (theme) => `<!doctype html><html data-theme="${theme}" data-cfp-skin="instrument"><head><style>${css} body{margin:0;background:var(--cf-canvas);font-family:var(--cf-font-sans)} main{max-width:720px;margin:0 auto;padding:0 16px}</style></head><body><main>${html}</main></body></html>`;
  const observed = {};
  for (const [label, width, theme] of [["wide", 1280, "light"], ["narrow", 390, "light"], ["wideDark", 1280, "dark"], ["narrowDark", 390, "dark"]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(document(theme));
    if (breakage) await page.evaluate(breakage, argument);
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

// Rule 6 reads the bars and limits themselves: a render whose marks or value
// text no longer encode the committed values fails, although every attribute
// the figure carries about its values is left as rendered.
test("rule 6 reads each drawn value back off the rendered marks", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const declaration = (await specimens()).find(({ name }) => name === "10-extent-derived.json").declaration;
  const { bound, composed, evidence } = fidelity(declaration);
  const html = renderFigure(declaration, { idPrefix: "d", bound });
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    const clean = await probe(page, css, html);
    assert.deepEqual(figureRuleFailures({ ...clean, evidence, composed }), []);
    const sixes = (observed) => figureRuleFailures({ ...observed, evidence, composed }).filter((failure) => failure.rule === 6).map((failure) => failure.message);
    const metadata = () => [...document.querySelectorAll("figure.cf-fig, .cf-fig [data-cf-value]")].map((node) => `${node.getAttribute("data-cf-values")}|${node.getAttribute("data-cf-value")}|${node.getAttribute("data-cf-facts")}`).join("\n");
    const halved = await probe(page, css, html, () => { for (const bar of document.querySelectorAll(".cf-fig-svg [data-state] > rect:first-child")) bar.setAttribute("width", String(Number(bar.getAttribute("width")) / 2)); });
    assert.equal(await page.evaluate(metadata), await (async () => { await page.setContent(html); return page.evaluate(metadata); })(), "the halved render keeps every value attribute");
    const halvedSixes = sixes(halved);
    for (const label of ["wide", "narrow", "wide dark", "narrow dark"]) {
      assert.ok(halvedSixes.some((message) => message.startsWith(`${label}: the row-0 mark reads 25 on its scale and the committed value is 50`)), halvedSixes.join("\n"));
    }
    const moved = await probe(page, css, html, () => { for (const limit of document.querySelectorAll(".cf-fig-svg [data-state] > line:first-child")) { limit.setAttribute("x1", "200"); limit.setAttribute("x2", "200"); } });
    assert.ok(sixes(moved).some((message) => /^wide: the limit-0 mark reads [\d.]+ on its scale and the committed value is 72$/.test(message)), sixes(moved).join("\n"));
    const relabelled = await probe(page, css, html, () => { for (const text of document.querySelectorAll(".cf-fig-svg text")) if (text.textContent === "50 chars") text.textContent = "90 chars"; });
    assert.ok(sixes(relabelled).some((message) => message === 'narrow: the row-0 mark is labelled ["90 chars","Description"], not ["50 chars","Description"]'), sixes(relabelled).join("\n"));

    // A transform halves what the reader sees while every attribute and the
    // local box stay as drawn: on the bar, on its group, and through CSS.
    const bars = ".cf-fig-svg [data-state] > rect:first-child";
    const transforms = {
      "the bar": (selector) => { for (const bar of document.querySelectorAll(selector)) bar.setAttribute("transform", `translate(${Number(bar.getAttribute("x")) / 2} 0) scale(0.5 1)`); },
      "its group": (selector) => { for (const bar of document.querySelectorAll(selector)) bar.parentElement.setAttribute("transform", `translate(${Number(bar.getAttribute("x")) / 2} 0) scale(0.5 1)`); },
      "a CSS transform": (selector) => { for (const bar of document.querySelectorAll(selector)) { bar.style.transformBox = "view-box"; bar.style.transformOrigin = `${bar.getAttribute("x")}px 0`; bar.style.transform = "scaleX(0.5)"; } },
    };
    for (const [where, transform] of Object.entries(transforms)) {
      const messages = sixes(await probe(page, css, html, transform, bars));
      for (const label of ["wide", "narrow", "wide dark", "narrow dark"]) {
        assert.ok(messages.includes(`${label}: the row-0 mark is transformed, and the grammar draws no transform`), `${where}: ${messages.join("\n")}`);
        assert.ok(messages.some((message) => message.startsWith(`${label}: the row-0 mark reads 25 on its scale and the committed value is 50`)), `${where}: ${messages.join("\n")}`);
      }
    }
  } finally { await browser.close(); }
});

// The value label is compared whole: a limit label at the 60-character
// ceiling still has its value read after it.
test("rule 6 reads a value label past its sixtieth character", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const declaration = structuredClone((await specimens()).find(({ name }) => name === "10-extent-derived.json").declaration);
  declaration.figure.layout.limits[0].label = "L".repeat(60);
  const { bound, composed, evidence } = fidelity(declaration);
  const html = renderFigure(declaration, { idPrefix: "d", bound });
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    const sixes = (observed) => figureRuleFailures({ ...observed, evidence, composed }).filter((failure) => failure.rule === 6).map((failure) => failure.message);
    assert.deepEqual(sixes(await probe(page, css, html)), []);
    const relabelled = await probe(page, css, html, () => { for (const text of document.querySelectorAll(".cf-fig-svg text")) if (text.textContent.endsWith(" 72")) text.textContent = text.textContent.replace(/ 72$/, " 99"); });
    assert.ok(sixes(relabelled).some((message) => message.startsWith("wide: the limit-0 mark is labelled")), sixes(relabelled).join("\n"));
  } finally { await browser.close(); }
});

test("the specimens that hold every rule, and the doctrine conflicts the gate names in the rest", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  const outcome = {};
  try {
    const page = await browser.newPage();
    for (const { name, declaration } of await specimens()) {
      const { bound, composed, evidence } = fidelity(declaration);
      const failures = figureRuleFailures({ ...await probe(page, css, renderFigure(declaration, { idPrefix: "s", bound })), evidence, composed });
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

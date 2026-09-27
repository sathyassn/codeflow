// Each of the twelve figure rules, read off a real render. The figures render
// with the portal's own sheets, so what the probe reads is what a reader sees.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "@playwright/test";
import { bindDerivedData, canonicalJson, composeFigure, FIGURE_RULES, figureDomFailures, figureRuleFailures, probeFigures, readFigureDom, renderFigure, THRESHOLDS } from "../scripts/figure-grammar.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { specimen, specimens } from "./page-shapes.mjs";

const styles = path.join(path.dirname(fileURLToPath(import.meta.url)), "../src/styles");

// The sheets carry the portal's own faces inline, so text boxes are measured
// in the fonts a reader gets, not an engine's fallback: Firefox reports a
// taller box for the portal's faces than for its fallback.
async function sheet() {
  const parts = await Promise.all(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"].map((name) => readFile(path.join(styles, name), "utf8")));
  let css = parts.join("\n");
  for (const [reference, file] of [...css.matchAll(/url\("\.\.\/fonts\/([^"]+\.woff2)"\)/g)]) {
    const font = await readFile(path.join(styles, "../fonts", file));
    css = css.replace(reference, `url("data:font/woff2;base64,${font.toString("base64")}")`);
  }
  return css;
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

// Keep the established 720 px content-width specimen harness independent of
// the shell reset. Padding belongs outside that width in this isolated probe.
async function probe(page, css, html, breakage = null, argument = undefined) {
  const document = (theme) => `<!doctype html><html data-theme="${theme}" data-cfp-skin="graphite"><head><style>${css} body{margin:0;background:var(--cf-canvas);font-family:var(--cf-font-sans)} main{box-sizing:content-box;max-width:720px;margin:0 auto;padding:0 16px}</style></head><body><main>${html}</main></body></html>`;
  const observed = {};
  for (const [label, width, theme] of [["wide", 1280, "light"], ["narrow", 390, "light"], ["wideDark", 1280, "dark"], ["narrowDark", 390, "dark"]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(document(theme));
    await page.evaluate(async () => { await Promise.all([...document.fonts].map((face) => face.load())); await document.fonts.ready; });
    if (breakage) await page.evaluate(breakage, argument);
    observed[label] = (await page.evaluate(probeFigures, { clearance: THRESHOLDS.labelClearancePx }))[0];
  }
  return observed;
}

test("each of the twelve rules fails a figure built to break it", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const all = await specimens();
  const layering = all.find(({ name }) => name === "03-layering.json").declaration;
  const oneChannel = await specimen("controls/one-channel.json");
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
      // Every mark becomes a box drawn around one of the figure's labels.
      7: () => {
        for (const svg of document.querySelectorAll(".cf-fig-svg")) {
          const texts = [...svg.querySelectorAll("text")];
          [...svg.querySelectorAll("[data-state]")].forEach((group, index) => {
            const box = texts[index % texts.length].getBBox();
            const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
            for (const [name, value] of Object.entries({ x: box.x - 6, y: box.y - 4, width: box.width + 12, height: box.height + 8 })) rect.setAttribute(name, String(value));
            group.replaceChildren(rect);
          });
        }
      },
      8: () => { for (const svg of document.querySelectorAll(".cf-fig-svg")) { const [a, b] = svg.querySelectorAll("text"); b.setAttribute("x", a.getAttribute("x")); b.setAttribute("y", a.getAttribute("y")); } },
      9: () => { document.querySelector(".cf-fig-caption").textContent = "First sentence. Second sentence."; },
      10: () => document.querySelector(".cf-fig-svg [data-state] *").setAttribute("fill", "#ff0000"),
      11: () => { for (const desc of document.querySelectorAll(".cf-fig-svg desc")) desc.remove(); },
      12: () => document.querySelector("details.cf-fig-details").remove(),
    };
    for (const [rule, breakage] of Object.entries(breakages)) {
      const observed = await probe(page, css, html, breakage);
      assert.ok(rulesOf(observed).includes(Number(rule)), `rule ${rule} (${FIGURE_RULES[rule]}) did not fail: ${JSON.stringify(figureRuleFailures(observed))}`);
    }
    // Rule 5 accepts a narrow that draws the wide mark set again only when
    // the declaration says marks "same" with a reason; the reflow still
    // fails without it.
    const permutation = (failures) => failures.some((failure) => failure.rule === 5 && failure.message.includes("permutation"));
    // Each breakage runs in the page, so it cannot share a helper.
    const reflow = () => { const [wide, narrow] = document.querySelectorAll(".cf-fig-svg"); narrow.innerHTML = wide.innerHTML; };
    assert.ok(permutation(figureRuleFailures(await probe(page, css, html, reflow))), "an undeclared reflow fails rule 5");
    const declared = await probe(page, css, html, () => { const [wide, narrow] = document.querySelectorAll(".cf-fig-svg"); narrow.innerHTML = wide.innerHTML; document.querySelector("figure.cf-fig").setAttribute("data-cf-same-marks", "declared"); });
    assert.ok(!permutation(figureRuleFailures(declared)), "a declared same mark set with a reason passes the permutation check");
    // Rule 11 also fails a figure whose title line is not shown (SPC-014 B5),
    // or shows a name other than the SVG title.
    for (const hide of [() => { document.querySelector(".cf-fig-title").style.display = "none"; }, () => { document.querySelector(".cf-fig-name").textContent = "Another title"; }]) {
      const observed = await probe(page, css, html, hide);
      assert.ok(rulesOf(observed).includes(11), `the title rule did not fail: ${JSON.stringify(figureRuleFailures(observed))}`);
    }
    // Rule 3: the layering specimen with its remote plane declared on the
    // local layer mark, so only the cap tells the two planes apart.
    const oneChannelFailures = figureRuleFailures(await probe(page, css, renderFigure(oneChannel, { idPrefix: "f" })));
    assert.deepEqual([...new Set(oneChannelFailures.map((failure) => failure.rule))], [3], canonicalJson(oneChannelFailures));
    assert.ok(oneChannelFailures.some((failure) => /^wide: states layer and layer-remote differ on overlay, need 2$/.test(failure.message)), canonicalJson(oneChannelFailures));
    // Rule 6: the evidence re-derives a value the figure does not draw.
    const fact = layering.figure.facts[0];
    assert.ok(rulesOf(clean, { facts: [{ claim: fact.claim, source: fact.source, drawn: fact.value, derived: false, matches: false }] }).includes(6));
    assert.ok(rulesOf(clean, { facts: [], data: null }).includes(6));
  } finally { await browser.close(); }
});

// Rule 7 is about text in boxes: a coverage cell is a state mark with no text
// in it, so a grid of cells with no crossed cell is not boxed text, and boxes
// that each hold a label still are. The narrow coverage composition binds
// each column label to its cell and budgets its width, and sets the cells a
// label drop below the row name, so every label clears each cell it does not
// label (rule 8) at 390 px, on one line or wrapped, in Chromium, WebKit and
// Firefox.
const SIGNING = JSON.stringify({ channels: { release: ["binaries", "archives", "images"], nightly: ["binaries", "archives"] } });
test("a coverage grid is not boxed text and its narrow labels clear the cells they do not label", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const css = await sheet();
  for (const [engineName, engine] of Object.entries({ chromium, webkit, firefox })) {
    const browser = await engine.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      for (const name of ["coverage-derived.json", "coverage-wrapped.json"]) {
        const declaration = await specimen(`controls/${name}`);
        const bound = declaration.figure.binding === "derived" ? bindDerivedData(declaration.figure, (relative) => (relative === "config/signing.json" ? SIGNING : null)) : null;
        const failures = figureRuleFailures(await probe(page, css, renderFigure(declaration, { idPrefix: "c", bound })));
        assert.deepEqual(failures, [], `${engineName}, ${name}: ${canonicalJson(failures)}`);
      }
      const boxed = figureRuleFailures(await probe(page, css, renderFigure(await specimen("controls/boxed-text.json"), { idPrefix: "b" })));
      assert.deepEqual(boxed.filter((failure) => failure.rule === 7).map((failure) => failure.message.split(":")[0]), ["wide", "narrow", "wide dark", "narrow dark"], `${engineName}: ${canonicalJson(boxed)}`);
    } finally { await browser.close(); }
  }
});

// A narrow extent row sets its label above the bar and its value past the
// bar's end. A short bar puts the value under the label, so the two are set
// a full text box apart and never overprint (rule 8), in Chromium, WebKit
// and Firefox, whose box for the portal's faces is the tallest.
test("a short narrow extent bar keeps its value clear of its row label", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const declaration = await specimen("controls/extent-short.json");
  const css = await sheet();
  for (const [engineName, engine] of Object.entries({ chromium, webkit, firefox })) {
    const browser = await engine.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      const failures = figureRuleFailures(await probe(page, css, renderFigure(declaration, { idPrefix: "e" })));
      assert.deepEqual(failures, [], `${engineName}: ${canonicalJson(failures)}`);
    } finally { await browser.close(); }
  }
});

// Rule 7 classifies each mark on its own and decides for the figure: labels
// in boxes with nothing drawn between them fail whatever else the figure
// draws. An empty box added beside them, or one box left without its label,
// does not clear the form, and a container box whose bound label sits beside
// it is still a text box. Bars and cells with their labels beside them, bound
// or not, are marks, so an extent of bars and a coverage grid pass.
test("rule 7 finds labels in boxes however the rest of the figure is drawn", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const boxed = await specimen("controls/boxed-text.json");
  const variant = (change) => { const copy = structuredClone(boxed); for (const name of ["wide", "narrow"]) change(copy.figure[name].draw, name); return copy; };
  const padded = variant((draw, name) => draw.push({ state: "part", shape: "rect", x: 20, y: name === "wide" ? 110 : 150, w: 16, h: 16, id: "empty" }));
  const unlabelled = variant((draw) => draw.splice(draw.findIndex((item) => item.text === "Dead letters"), 1));
  const beside = variant((draw) => {
    for (const item of draw.filter((entry) => entry.text !== undefined)) {
      const box = draw.find((entry) => entry.id === item.for[0]);
      item.y = box.y + box.h + 18;
    }
  });
  const bars = structuredClone(await specimen("10-extent-derived.json"));
  bars.figure.id = "bars-only";
  delete bars.figure.layout.limits;
  bars.figure.states = bars.figure.states.filter((state) => state.name !== "limit");
  const barsBound = { source: bars.figure.source, derived: POLICY };
  const coverage = await specimen("controls/coverage-derived.json");
  const coverageBound = bindDerivedData(coverage.figure, (relative) => (relative === "config/signing.json" ? SIGNING : null));
  const css = await sheet();
  for (const [engineName, engine] of Object.entries({ chromium, webkit, firefox })) {
    const browser = await engine.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      const sevens = async (declaration, bound = null) => figureRuleFailures(await probe(page, css, renderFigure(declaration, { idPrefix: "r", bound }))).filter((failure) => failure.rule === 7).map((failure) => failure.message);
      for (const [name, declaration] of Object.entries({ boxed, padded, unlabelled, beside })) {
        assert.deepEqual(await sevens(declaration), ["wide", "narrow", "wide dark", "narrow dark"].map((label) => `${label}: labels sit in boxes with nothing drawn between them`), `${engineName}, ${name}`);
      }
      assert.deepEqual(await sevens(bars, barsBound), [], `${engineName}, extent bars with bound value labels`);
      assert.deepEqual(await sevens(coverage, coverageBound), [], `${engineName}, coverage cells with bound column names`);
    } finally { await browser.close(); }
  }
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

// Rule 8 on the derived extent layout in every engine the portal gate runs.
// The layout budgets label clearance from worst-case text metrics, so a text
// box an engine reports wider than another's (Firefox's reaches about 0.23em
// past the advance) still clears every mark it does not label, at 1280 and
// 390 px in both modes.
test("the derived extent layout clears rule 8 in Chromium, WebKit and Firefox", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const specimen = (await specimens()).find(({ name }) => name === "10-extent-derived.json").declaration;
  // The specimen, and a long unit inside the label budget (R5-3), whose
  // value labels take most of the room the layout reserves.
  const longUnit = structuredClone(specimen);
  longUnit.figure.layout.unit = "chars per line";
  const css = await sheet();
  for (const [name, engine] of Object.entries({ chromium, webkit, firefox })) {
    const browser = await engine.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      for (const [variant, declaration] of [["specimen", specimen], ["long unit", longUnit]]) {
        const { bound, composed, evidence } = fidelity(declaration);
        const html = renderFigure(declaration, { idPrefix: "d", bound });
        const observed = await probe(await browser.newPage(), css, html);
        const failures = figureRuleFailures({ ...observed, evidence, composed });
        assert.deepEqual(failures.filter((failure) => failure.rule === 8), [], `${name}, ${variant}: ${JSON.stringify(failures)}`);
        assert.deepEqual(failures, [], `${name}, ${variant}: ${JSON.stringify(failures)}`);
      }
    } finally { await browser.close(); }
  }
});

// The structural check: a rendered figure must be its clean render. Its DOM
// must equal the pinned declaration's drawing, root viewBox included, and
// its drawings must compute the geometry and visibility styles the kit sheets
// alone give them, so a stylesheet the kit does not ship cannot move, hide or
// clip a mark (R3-2, R3-3).
test("a figure that is not its clean render fails, whatever changed it", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const declaration = (await specimens()).find(({ name }) => name === "10-extent-derived.json").declaration;
  const { bound, composed, evidence } = fidelity(declaration);
  const html = renderFigure(declaration, { idPrefix: "d", bound });
  const css = await sheet();
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    const read = async (width, change = null, argument = undefined) => {
      await page.setViewportSize({ width, height: 900 });
      await page.setContent(`<!doctype html><html data-theme="light" data-cfp-skin="graphite"><head><style>${css}</style></head><body><main>${html}</main></body></html>`);
      if (change) await page.evaluate(change, argument);
      return (await page.evaluate(readFigureDom))[0];
    };
    const addSheet = (rule) => document.head.append(Object.assign(document.createElement("style"), { textContent: rule }));
    for (const width of [1280, 390]) {
      const clean = await read(width);
      assert.deepEqual(figureDomFailures(await read(width), clean), [], "positive control: a clean render is its own clean render");
      const cases = {
        "a stylesheet translate": [addSheet, ".cf-m-used { translate: 0 1000px; }", /a drawn <rect> has translate 0px 1000px where the kit sheets alone give none/],
        "a stylesheet rotate": [addSheet, ".cf-m-used { rotate: 12deg; }", /a drawn <rect> has rotate 12deg where the kit sheets alone give none/],
        "a stylesheet scale": [addSheet, ".cf-m-used { scale: 0.5 1; }", /a drawn <rect> has scale 0.5 1 where the kit sheets alone give none/],
        "a clipped root": [addSheet, ".cf-fig-svg { overflow: hidden; }", /a drawn <svg> has overflow hidden where the kit sheets alone give visible/],
        "a hidden mark": [addSheet, ".cf-m-used { visibility: hidden; }", /a drawn <rect> has visibility hidden/],
        "a changed viewBox": [() => { for (const svg of document.querySelectorAll(".cf-fig-svg")) { const [x, y, w, h] = svg.getAttribute("viewBox").split(" "); svg.setAttribute("viewBox", `${x} ${y} ${w} ${Number(h) / 2}`); } }, /the rendered figure is not the drawing its pinned declaration produces/],
        "a moved viewBox origin": [() => { for (const svg of document.querySelectorAll(".cf-fig-svg")) svg.setAttribute("viewBox", svg.getAttribute("viewBox").replace(/^0 /, "40 ")); }, /the rendered figure is not the drawing its pinned declaration produces/],
        "an inline style": [() => document.querySelector(".cf-fig-svg rect").setAttribute("style", "translate: 0 1000px"), /the rendered figure is not the drawing its pinned declaration produces/],
      };
      for (const [name, entry] of Object.entries(cases)) {
        const [change, argument, expected] = entry.length === 3 ? entry : [entry[0], undefined, entry[1]];
        const failures = figureDomFailures(await read(width, change, argument), clean);
        assert.ok(failures.some((failure) => expected.test(failure)), `${name} at ${width}: ${failures.join("\n")}`);
      }
    }
    // The readback, the secondary check, reads both axes: a bar translated
    // down its own length and more no longer sits on its row.
    const translated = await probe(page, css, html, addSheet, ".cf-m-used { translate: 0 1000px; }");
    const sixes = figureRuleFailures({ ...translated, evidence, composed }).filter((failure) => failure.rule === 6).map((failure) => failure.message);
    for (const label of ["wide", "narrow", "wide dark", "narrow dark"]) {
      assert.ok(sixes.some((message) => message.startsWith(`${label}: the row-0 mark spans y`)), sixes.join("\n"));
      assert.ok(sixes.includes(`${label}: the row-0 mark is transformed, and the grammar draws no transform`), sixes.join("\n"));
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
    "01-flow.json": [],
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

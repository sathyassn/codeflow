// The figure grammar module on its own: declarations validate and render for
// every family and facts re-derive from their sources. The rules that need a
// render are in figure-rules-browser.test.mjs.
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import test from "node:test";
import { checkFacts, composeFigure, drawnValuesMatch, FAMILIES, MARKS, markdownSections, MIN_PLOT_WIDTH, renderFigure, SHAPE_CLASSES, slugHeading, TEXT_CLASSES, validateDeclaration } from "../scripts/figure-grammar.mjs";

import { specimen, specimens } from "./page-shapes.mjs";

// A committed source for each fact, written so the fact holds: the anchored
// section carries the text, the items or the JSON value the fact asserts.
function sourcesFor(declaration) {
  const files = new Map();
  for (const fact of declaration.figure.facts) {
    const [file, anchor] = fact.source.split("#");
    if (fact.check.kind === "json") {
      const root = JSON.parse(files.get(file) ?? "{}");
      let cursor = root;
      const keys = fact.check.select.split(".");
      for (const key of keys.slice(0, -1)) cursor = cursor[key] ??= {};
      cursor[keys.at(-1)] = fact.value;
      files.set(file, JSON.stringify(root));
      continue;
    }
    const body = fact.check.kind === "contains" ? fact.check.text : Array.from({ length: fact.value }, (_, index) => `${index + 1}. step`).join("\n");
    const heading = anchor === undefined ? "" : `## ${anchor.replaceAll("-", " ")}\n\n`;
    files.set(file, `${files.get(file) ?? "# Source\n\n"}${heading}${body}\n\n`);
  }
  return (sourcePath) => files.get(sourcePath) ?? null;
}

test("every family validates, renders, keys its drawn states and re-derives its facts", async () => {
  const all = await specimens();
  assert.deepEqual([...new Set(all.map(({ declaration }) => declaration.figure.family))].sort(), [...FAMILIES].sort());
  for (const { name, declaration } of all) {
    validateDeclaration(declaration, name);
    const readSource = sourcesFor(declaration);
    const facts = checkFacts(declaration.figure, readSource);
    assert.ok(facts.every((fact) => fact.matches), `${name}: ${JSON.stringify(facts)}`);
    const policy = JSON.stringify({ git: { commit_desc_max_len: 50, commit_subject_max_len: 72 } });
    const bound = declaration.figure.binding === "derived" ? { source: declaration.figure.source, derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
    const html = renderFigure(declaration, { idPrefix: "t", bound, facts });
    assert.match(html, new RegExp(`^<figure class="cf-fig" data-cf-figure="${declaration.figure.family}"`), name);
    for (const state of declaration.figure.states) assert.match(html, new RegExp(`<li data-state="${state.name}"`), `${name} keys ${state.name}`);
    assert.equal((html.match(/<svg class="cf-fig-svg cf-fig-svg--(?:wide|narrow)"/g) ?? []).length, 2, name);
    assert.equal((html.match(/<details class="cf-twin"/g) ?? []).length, 1, name);
    if (bound !== null) assert.ok(drawnValuesMatch(composeFigure(declaration, bound).drawnValues, bound.derived), policy);
  }
});

// A legend key is read as its primary shape and its parts, the same way the
// drawn state group is: a key must be the drawing the figure uses for that
// state, never the mark's canonical form, so a crossed key never labels an
// uncrossed region and a box never labels a line.
function drawnForm(markup) {
  const elements = [...markup.replace(/<defs>.*?<\/defs>/g, "").matchAll(/<(path|rect|line|polyline|circle)\b([^>]*)\/>/g)]
    .map(([, tag, attributes]) => ({ tag, cls: attributes.match(/class="([^"]+)"/)?.[1], d: attributes.match(/ d="([^"]+)"/)?.[1] ?? "" }));
  const [primary, ...parts] = elements;
  const shape = primary.cls === "cf-m-cross" || primary.cls.endsWith("-cross") ? "cross"
    : primary.tag === "rect" ? "box"
    : primary.tag === "circle" ? "round"
    : primary.tag === "path" && /Z$/i.test(primary.d) ? "closed"
    : "line";
  return { shape, cls: primary.cls, parts: [...new Set(parts.map((part) => `${part.tag}.${part.cls}`))].sort() };
}

test("every legend key draws its state the way the figure draws it", async () => {
  for (const { name, declaration } of await specimens()) {
    const bound = declaration.figure.binding === "derived" ? { source: declaration.figure.source, derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
    const html = renderFigure(declaration, { idPrefix: "k", bound });
    const drawings = html.slice(0, html.indexOf('<ul class="cf-legend"'));
    for (const state of declaration.figure.states) {
      const group = drawings.match(new RegExp(`<g data-state="${state.name}"[^>]*>(.*?)</g>`))?.[1];
      const key = html.match(new RegExp(`<li data-state="${state.name}"[^>]*><svg class="cf-key"[^>]*>(.*?)</svg>`))?.[1];
      assert.ok(group && key, `${name} draws and keys ${state.name}`);
      const drawn = drawnForm(group);
      const keyed = drawnForm(key);
      // A vertical limit bar is keyed as a short upright line.
      const shape = (form) => (form.shape === "line" && form.cls === MARKS[state.mark].className && MARKS[state.mark].key === "bar-v" ? "line" : form.shape);
      assert.deepEqual({ shape: shape(keyed), cls: keyed.cls, parts: keyed.parts }, { shape: shape(drawn), cls: drawn.cls, parts: drawn.parts }, `${name} ${state.name}`);
    }
  }
  // The TSK-058 forms: an uncrossed copy region, a dashed run with square
  // ends, and a dashed arrow with an open head beside a dashed box.
  const [structure] = (await specimens()).filter(({ name }) => name === "02-structure.json");
  const keys = renderFigure(structure.declaration, { idPrefix: "k" });
  assert.doesNotMatch(keys.match(/<li data-state="copy".*?<\/li>/)[0], /cf-m-cross/);
  assert.match(keys.match(/<li data-state="compares".*?<\/li>/)[0], /<path class="cf-m-optional" d="M3 8H20"\/><rect class="cf-m-state"/);
});

test("two states that draw the same legend key are refused", async () => {
  const [derivation] = (await specimens()).filter(({ name }) => name === "08-derivation.json");
  assert.doesNotThrow(() => renderFigure(derivation.declaration, { idPrefix: "k" }));
  // Drawn as a plain box, "declares" keys exactly as "declared" does.
  const same = structuredClone(derivation.declaration);
  for (const composition of [same.figure.wide, same.figure.narrow]) {
    composition.draw = composition.draw.map((item) => (item.state === "declares" ? { state: "declares", shape: "rect", x: 1, y: 1, w: 10, h: 10 } : item));
  }
  assert.throws(() => renderFigure(same, { idPrefix: "k" }), /states declared and declares draw the same legend key/);
});

// The HTML each specimen renders to, committed so the Rust validator's
// reconstruction of a companion is pinned to the same bytes (see
// figures/render.rs in codeflow-core). CODEFLOW_UPDATE_FIGURE_FIXTURES=1
// rewrites them after a deliberate change to the drawing.
// The two coverage controls pin the narrow coverage layout on one line and
// wrapped onto a second.
test("the committed rendered specimens are what the grammar draws", async () => {
  const controls = await Promise.all(["controls/coverage-derived.json", "controls/coverage-wrapped.json"].map(async (name) => ({ name, declaration: await specimen(name) })));
  for (const { name, declaration } of [...await specimens(), ...controls]) {
    const bound = declaration.figure.binding === "derived" ? { source: declaration.figure.source, derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
    const html = `${renderFigure(declaration, { idPrefix: "cf-fig-0", bound })}\n`;
    const file = new URL(`./fixtures/figures/rendered/${name.replace(/\.json$/, ".html")}`, import.meta.url);
    if (process.env.CODEFLOW_UPDATE_FIGURE_FIXTURES === "1") await writeFile(file, html);
    assert.equal(await readFile(file, "utf8"), html, name);
  }
});

// The class vocabulary present's DOM guard accepts is every class the
// drawing code can write, read off the module source itself.
test("the exported class vocabulary covers every class the drawing writes", async () => {
  const source = await readFile(new URL("../scripts/figure-grammar.mjs", import.meta.url), "utf8");
  const written = new Set([...source.matchAll(/"(cf-[mf]-[a-z-]+)"/g)].map((match) => match[1]));
  assert.deepEqual([...written].filter((name) => !SHAPE_CLASSES.includes(name)), []);
  const texts = new Set([...source.matchAll(/"(cf-t--[a-z]+)"/g)].map((match) => match[1]));
  assert.deepEqual([...texts].sort(), [...TEXT_CLASSES].sort());
  for (const { declaration } of await specimens()) {
    const bound = declaration.figure.binding === "derived" ? { derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
    for (const [, classes] of renderFigure(declaration, { bound }).matchAll(/<(?:path|line|polyline|rect|circle) class="([^"]+)"/g)) assert.ok(SHAPE_CLASSES.includes(classes), classes);
  }
});

test("declarations are refused where the grammar can tell without a browser", async () => {
  const base = (await specimens()).find(({ name }) => name === "03-layering.json").declaration;
  const variant = (change) => { const copy = structuredClone(base); change(copy.figure); return copy; };
  const refusals = [
    [(figure) => { figure.family = "chart"; }, /family must be one of/],
    [(figure) => { figure.caption = "One sentence. And another."; }, /rule 9: the caption must be exactly one sentence/],
    [(figure) => { figure.states[0].mark = "sparkle"; }, /draws unknown mark sparkle/],
    [(figure) => { figure.wide.draw.push({ state: "ghost", shape: "rect", x: 0, y: 0, w: 10, h: 10 }); }, /draws undeclared state ghost/],
    [(figure) => { figure.facts = []; }, /rule 6: facts must list 1 to/],
    [(figure) => { figure.wide.draw.find((item) => item.text !== undefined).style = ["mono", "mono"]; }, /style must be distinct entries/],
    [(figure) => { figure.twin = null; }, /rule 12: twin must be facts or/],
    [(figure) => { delete figure.narrow; }, /rule 5: narrow must declare the recomposition/],
    [(figure) => { figure.facts[0].source = "../outside.md"; }, /source must be a repository path/],
    [(figure) => { figure.title = "Two parts \u2014 one dash"; }, /dash/],
  ];
  for (const [change, expected] of refusals) assert.throws(() => validateDeclaration(variant(change)), expected);
  // A state declared and never drawn is an unkeyed promise (rule 2), and a
  // narrow composition that drops a state it does not declare dropped fails
  // rule 5; both refuse at render.
  const undrawn = variant((figure) => { figure.states.push({ name: "unused", mark: "optional", means: "Never drawn" }); });
  assert.throws(() => renderFigure(undrawn), /rule 2: declared states are never drawn: unused/);
  const dropping = variant((figure) => { figure.narrow.drops = [figure.states[0].name]; });
  assert.throws(() => renderFigure(dropping), /rule 5: the narrow composition drops nothing but declares drops/);
});

// An extent keeps a usable plot width (R5-3). A unit whose value labels would
// leave less than the minimum is refused before drawing, and a derived value
// too long for the budget is refused when the figure is composed; the
// narrow scale of anything drawn runs forwards, at least that wide.
test("an extent layout keeps a positive, usable plot width or is refused", async () => {
  const base = (await specimens()).find(({ name }) => name === "10-extent-derived.json").declaration;
  const bound = (derived) => ({ source: base.figure.source, derived });
  const withUnit = (unit) => { const copy = structuredClone(base); copy.figure.layout.unit = unit; return copy; };
  const accepted = validateDeclaration(withUnit("chars per line"));
  const { narrow, wide } = composeFigure(accepted, bound({ commit_desc_max_len: 50, commit_subject_max_len: 72 }));
  for (const composition of [narrow, wide]) {
    const [start, end] = composition.scale.range;
    assert.ok(end - start >= MIN_PLOT_WIDTH, JSON.stringify(composition.scale));
    for (const bar of composition.draw.filter((item) => item.shape === "rect")) assert.ok(bar.w > 0, JSON.stringify(bar));
  }
  assert.throws(() => validateDeclaration(withUnit("milliseconds per successful transaction")), /the extent value labels leave -?[\d.]+ units to plot at the narrow width, under the 160 the layout needs; shorten layout\.unit/);
  const edge = withUnit("chars per line");
  assert.throws(() => composeFigure(edge, bound({ commit_desc_max_len: 50.12, commit_subject_max_len: 72000.25 })), /under the 160 the layout needs/);
});

test("facts re-derive from the anchored section, so a wrong fact under a valid anchor fails", () => {
  const source = "# Guide\n\n## Install\n\n1. Run the installer.\n2. Check the result.\n\n```md\n## Not a heading\n```\n\n## Install\n\n- again\n";
  assert.deepEqual(markdownSections(source).map((section) => section.anchor), ["guide", "install", "install-1"]);
  assert.equal(slugHeading("Plan graph (Plan v4.3)"), "plan-graph-plan-v43");
  const figure = (value, anchor = "install") => ({ facts: [{ claim: "two steps", source: `guide.md#${anchor}`, derive: "items", check: { kind: "count-items" }, value }] });
  const read = (file) => (file === "guide.md" ? source : null);
  assert.equal(checkFacts(figure(2), read)[0].matches, true);
  const wrong = checkFacts(figure(3), read)[0];
  assert.deepEqual([wrong.matches, wrong.derived, wrong.error], [false, 2, null]);
  assert.match(checkFacts(figure(2, "uninstall"), read)[0].error, /fact anchor does not exist: guide\.md#uninstall/);
  assert.equal(checkFacts(figure(1, "install-1"), read)[0].matches, true);
});

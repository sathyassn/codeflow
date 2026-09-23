// The figure grammar module on its own: declarations validate and render for
// every family and facts re-derive from their sources. The rules that need a
// render are in figure-rules-browser.test.mjs.
import assert from "node:assert/strict";
import test from "node:test";
import { checkFacts, composeFigure, drawnValuesMatch, FAMILIES, markdownSections, renderFigure, slugHeading, validateDeclaration } from "../scripts/figure-grammar.mjs";

import { specimens } from "./page-shapes.mjs";

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

test("declarations are refused where the grammar can tell without a browser", async () => {
  const base = (await specimens()).find(({ name }) => name === "03-layering.json").declaration;
  const variant = (change) => { const copy = structuredClone(base); change(copy.figure); return copy; };
  const refusals = [
    [(figure) => { figure.family = "chart"; }, /family must be one of/],
    [(figure) => { figure.caption = "One sentence. And another."; }, /rule 9: the caption must be exactly one sentence/],
    [(figure) => { figure.states[0].mark = "sparkle"; }, /draws unknown mark sparkle/],
    [(figure) => { figure.wide.draw.push({ state: "ghost", shape: "rect", x: 0, y: 0, w: 10, h: 10 }); }, /draws undeclared state ghost/],
    [(figure) => { figure.facts = []; }, /rule 6: facts must list 1 to/],
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

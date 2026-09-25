// Shared loading for the visual-doctrine control tests (TSK-062). The cases
// ship figure declarations in cf-evaluate-model fixtures; the controls are the
// faulty and passing answers a subject could leave, graded by the portal's own
// grammar module and configuration validator, not by a second implementation.
import { readFile } from "node:fs/promises";

const here = (relative) => new URL(relative, import.meta.url);
export const grammar = await import(here("../../docs-portal/scripts/figure-grammar.mjs"));

const resources = here("../../assets/base/agents/skills/cf-evaluate-model/resources/");
const json = async (url) => JSON.parse(await readFile(url, "utf8"));

export const fixtures = new Map((await json(new URL("fixtures.json", resources))).fixtures.map((fixture) => [fixture.id, fixture]));
export const cases = new Map((await json(new URL("cases.json", resources))).cases.map((entry) => [entry.id, entry]));
export const packs = new Map((await json(new URL("packs.json", resources))).packs.map((pack) => [pack.id, pack]));
export const controls = await json(here("./visual-controls/controls.json"));

// The fixture a case ships and a reader over its files as the subject found
// them: facts always re-derive from the shipped bytes, never from a file a
// subject could have edited to match its drawing.
export function fixtureOf(caseId) {
  const fixture = fixtures.get(cases.get(caseId).fixture);
  return { fixture, readSource: (relative) => fixture.files[relative] ?? null };
}

export async function declarationOf(caseId, reference) {
  if (reference.startsWith("fixture:")) return JSON.parse(fixtureOf(caseId).fixture.files[reference.slice("fixture:".length)]);
  return json(here(`./visual-controls/${reference}`));
}

// Everything the gate reads except the render: validation, re-derived facts,
// the derived binding and both compositions.
export function prepare(declaration, readSource) {
  grammar.validateDeclaration(declaration);
  const facts = grammar.checkFacts(declaration.figure, readSource);
  const bound = grammar.bindDerivedData(declaration.figure, readSource);
  const composed = grammar.composeFigure(declaration, bound);
  const evidence = { facts, data: bound === null ? null : { drawn: composed.drawnValues, derived: bound.derived } };
  return { facts, bound, composed, evidence, html: grammar.renderFigure(declaration, { idPrefix: "c", bound }) };
}

// The adapter's layer choice (adapter.mjs chooseLayer), which the page route
// is built from: `${layer.id}/${localRouteFor(source)}`.
export function layerFor(sourcePath, layers) {
  return layers.find((layer) => (layer.paths ?? []).includes(sourcePath) || (layer.prefixes ?? []).some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`)))
    ?? layers.find((layer) => layer.fallback);
}

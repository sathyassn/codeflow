// TSK-062 controls without a browser: every portal fixture's configuration
// passes the portal's own validator and binds declarations it ships to routes
// it publishes, and every control declaration validates, re-derives its facts
// as recorded and declares the family recorded for it. The render half is
// visual_controls_browser.test.mjs.
//
// Run: npm run deps:install --prefix docs-portal
//      node --test evals/model-artifacts/visual_controls.test.mjs
import assert from "node:assert/strict";
import test from "node:test";
import { cases, controls, declarationOf, fixtureOf, fixtures, layerFor, packs, prepare } from "./visual_controls.mjs";

const lib = await import(new URL("../../docs-portal/scripts/lib.mjs", import.meta.url));

const portalFixtures = [...fixtures.values()].filter((fixture) => fixture.files["docs-portal/portal.config.json"] !== undefined);
const visual = packs.get("visual-doctrine").cases;

test("every visual-doctrine case has graded controls, and only those cases", () => {
  assert.deepEqual(Object.keys(controls.cases).sort(), [...visual].sort());
  for (const [caseId, entries] of Object.entries(controls.cases)) {
    assert.ok(entries.some((entry) => entry.role === "passing"), `${caseId}: no passing control`);
    if (caseId !== "key-rotation-section-is-drawn") assert.ok(entries.some((entry) => entry.role === "faulty"), `${caseId}: no faulty control`);
  }
});

test("every portal fixture passes the portal configuration validator and binds what it ships", () => {
  assert.ok(portalFixtures.length >= 14, `${portalFixtures.length} portal fixtures`);
  for (const fixture of portalFixtures) {
    const config = lib.validatePortalConfig(JSON.parse(fixture.files["docs-portal/portal.config.json"]));
    const routes = new Set(Object.keys(fixture.files)
      .filter((file) => file.startsWith("docs/") && file.endsWith(".md"))
      .map((file) => `${layerFor(file, config.layers).id}/${lib.localRouteFor(file, config.source_roots)}`));
    for (const binding of config.figures) {
      assert.ok(fixture.files[binding.declaration] !== undefined, `${fixture.id}: ${binding.declaration} is not shipped`);
      assert.ok(routes.has(binding.route), `${fixture.id}: ${binding.route} is not a published route`);
    }
    const adoption = JSON.parse(fixture.files[".codeflow/docs-portal.json"]);
    assert.equal(adoption.root, "docs-portal", fixture.id);
  }
});

test("every control declaration validates, re-derives its facts as recorded and declares its family", async () => {
  for (const [caseId, entries] of Object.entries(controls.cases)) {
    const { readSource } = fixtureOf(caseId);
    for (const entry of entries) {
      const declaration = await declarationOf(caseId, entry.declaration);
      const label = `${caseId} ${entry.role} ${entry.declaration}`;
      const { facts } = prepare(declaration, readSource);
      assert.equal(declaration.figure.family, entry.family, label);
      assert.equal(facts.every((fact) => fact.matches) ? "match" : "mismatch", entry.facts, `${label}: ${JSON.stringify(facts)}`);
    }
  }
});

test("each fixture's shipped declarations are all under control", () => {
  for (const caseId of visual) {
    const shipped = Object.keys(fixtureOf(caseId).fixture.files).filter((file) => file.startsWith("docs/figures/"));
    const controlled = controls.cases[caseId].map((entry) => entry.declaration).filter((reference) => reference.startsWith("fixture:"));
    assert.deepEqual(controlled.map((reference) => reference.slice("fixture:".length)).sort(), shipped.sort(), caseId);
  }
  assert.ok(cases.size > 0);
});

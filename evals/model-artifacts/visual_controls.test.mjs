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
import { adapt, cases, controls, declarationOf, fixtureOf, fixtures, layerFor, methodAnswer, methodControls, observeAdaptedPage, packs, place, prepare } from "./visual_controls.mjs";

const pageClasses = await import(new URL("../../docs-portal/scripts/page-classes.mjs", import.meta.url));

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

test("every portal case tells the subject the guide does not build in its checkout", () => {
  for (const fixture of portalFixtures) {
    const scope = fixture.files["docs-portal/README.md"];
    assert.ok(scope?.includes("not in this checkout") && scope.includes("cannot run"), `${fixture.id}: no subject-visible scope`);
  }
  const portalIds = new Set(portalFixtures.map((fixture) => fixture.id));
  const portalCases = [...cases.values()].filter((entry) => portalIds.has(entry.fixture));
  assert.ok(portalCases.length >= 14);
  for (const entry of portalCases) assert.match(entry.prompt, /docs-portal\/README\.md/, entry.id);
});

// Two drafts carry a fact that does not re-derive; the adapter refuses them
// on rule 6, which is the defect those cases grade. Every other fixture
// adapts as shipped, so the subject's graded work is the only thing wrong.
const REFUSED_AS_SHIPPED = { "figure-limits-stale-value": "rule 6", "figure-planes-remote-claim": "rule 6" };

test("every shipped portal fixture adapts, except the drafts whose defect is a fact", { timeout: 300_000 }, async () => {
  for (const fixture of portalFixtures) {
    const { status, error } = await adapt(fixture.files);
    if (REFUSED_AS_SHIPPED[fixture.id]) assert.match(error ?? "", new RegExp(REFUSED_AS_SHIPPED[fixture.id]), fixture.id);
    else assert.equal(status, 0, `${fixture.id}: ${error}`);
  }
});

test("every passing control adapts where a subject would leave it", { timeout: 300_000 }, async () => {
  for (const [caseId, entries] of Object.entries(controls.cases)) {
    for (const entry of entries.filter((candidate) => candidate.role === "passing")) {
      const declaration = await declarationOf(caseId, entry.declaration);
      const { files, target } = place(fixtureOf(caseId).fixture.files, declaration, entry.placement, entry.declaration.replace(/^fixture:docs\/figures\//, ""));
      const { status, error, evidence } = await adapt(files);
      assert.equal(status, 0, `${caseId}: ${error}`);
      const bound = evidence.pages.flatMap((page) => page.figures.map((figure) => figure.declaration_path));
      assert.ok(bound.includes(target), `${caseId}: ${target} is not bound on any page`);
    }
  }
});

test("every figure in a committed method answer validates and re-derives its facts", async () => {
  let count = 0;
  for (const [caseId, entries] of Object.entries(methodControls.cases)) {
    for (const entry of entries) {
      if (!/\.json$|\/(passing|faulty)$/.test(entry.answer)) continue;
      const { figures, readSource } = await methodAnswer(caseId, entry);
      for (const declaration of figures) {
        const { facts } = prepare(declaration, readSource);
        assert.ok(facts.every((fact) => fact.matches), `${entry.answer}: ${JSON.stringify(facts)}`);
        count += 1;
      }
    }
  }
  assert.equal(count, 5);
});

test("the guide page and display answers adapt, and the class rules grade the guide page", { timeout: 300_000 }, async () => {
  for (const caseId of ["guide-page-from-a-policy-source", "display-panel-and-first-paint-take-different-carriers"]) {
    for (const entry of methodControls.cases[caseId]) {
      const { files, config } = await methodAnswer(caseId, entry);
      const { status, error, evidence, pages } = await adapt(files);
      assert.equal(status, 0, `${entry.answer}: ${error}`);
      if (caseId !== "guide-page-from-a-policy-source") continue;
      const route = "system/merge-policy";
      const assignments = pageClasses.classifyPortalPages(config, evidence.pages).filter((assignment) => assignment.route === route);
      assert.equal(assignments[0].pageClass, "explanatory");
      const failures = pageClasses.pageClassFailures(assignments, [observeAdaptedPage(pages[route], route)]);
      if (entry.decision === "pass") assert.deepEqual(failures, [], entry.answer);
      else assert.ok(failures.some((failure) => /carries no figure/.test(failure)), `${entry.answer}: ${failures}`);
    }
  }
});

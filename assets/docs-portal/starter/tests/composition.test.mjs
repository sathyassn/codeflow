import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { paletteSwatchFailures, PALETTE_PILL_GROUPS } from "../scripts/browser-verify.mjs";
import {
  ALTITUDE_PANELS, CARRIER_ELEMENTS, DERIVED_LOOKUPS, PAGE_CLASS_REASONS, PAGE_CLASSES, PANEL_CARRIERS, PANEL_CARRIER_ALTERNATES, RECORD_POINTER_COLUMNS, RECORD_POINTER_SOURCE,
  assertDeclaredCarriers, assertNoRecordRoutes, assertNoStaleSources, assertPageClassCoverage, classifyPortalPages, declaredCarrierFailures,
  pageClassFailures, recordPointerRoute, recordRouteFailures, staleSourceFailures,
} from "../scripts/page-classes.mjs";
import { COMPOSED_PAGE, SHELL_PAGE, panelBindings } from "./page-shapes.mjs";
import { commitFixture, configureFixture, portalFixture, runAdapter } from "./portal-fixture.mjs";

// What a composed page carries, counted inside each panel that owns it.
const CARRIED = Object.freeze({
  concept: Object.freeze({ figure: 1, stage: 0, table: 0, list: 0, pre: 0 }),
  architecture: Object.freeze({ figure: 1, stage: 0, table: 0, list: 0, pre: 0 }),
  technical: Object.freeze({ figure: 1, stage: 0, table: 2, list: 1, pre: 1 }),
});
const COMPLIANT_OBSERVATION = Object.freeze({
  headings: 1, provenance: true, displayControls: 2, commentChrome: 0,
  altitudePanels: [...ALTITUDE_PANELS], panelCarriers: CARRIED, pointerColumns: [], pointerRows: 0,
  sourceRegions: 0, companions: 3, headFigures: 0, tables: 2,
});
const POINTER_OBSERVATION = Object.freeze({
  headings: 1, provenance: false, displayControls: 2, commentChrome: 0,
  altitudePanels: [], panelCarriers: Object.freeze({ concept: null, architecture: null, technical: null }),
  pointerColumns: [...RECORD_POINTER_COLUMNS], pointerRows: 4,
});
const PROSE_PANEL = Object.freeze({ figure: 0, stage: 0, table: 0, list: 0, pre: 0 });

test("the page-class rules are declared once and enumerate what each class must show", () => {
  assert.deepEqual(ALTITUDE_PANELS, ["concept", "architecture", "technical"]);
  assert.deepEqual(CARRIER_ELEMENTS, ["figure", "stage", "table", "list", "pre"]);
  assert.deepEqual(Object.keys(PANEL_CARRIERS), [...ALTITUDE_PANELS]);
  // Every altitude demands a grammar figure; Technical adds a table and never
  // takes one in place of the figure. A cf-stage answers for no altitude.
  assert.deepEqual(ALTITUDE_PANELS.map((panel) => PANEL_CARRIERS[panel].requires), [["figure"], ["figure"], ["figure", "table"]]);
  assert.deepEqual(PANEL_CARRIER_ALTERNATES.technical.list, ["figure", "list"]);
  assert.deepEqual(PAGE_CLASS_REASONS, ["accepted-record", "governance", "no-relationship"]);
  assert.deepEqual(DERIVED_LOOKUPS, ["capability-registry"]);
  assert.deepEqual(Object.keys(PANEL_CARRIER_ALTERNATES), ["technical"]);
  assert.deepEqual(Object.keys(PANEL_CARRIER_ALTERNATES.technical), ["list"]);
  assert.deepEqual(RECORD_POINTER_COLUMNS, ["Folder", "Purpose", "Count", "Repository"]);
  assert.deepEqual(Object.keys(PAGE_CLASSES), ["explanatory", "illustrated", "passThrough", "derivedLookup", "recordPointer"]);
  assert.deepEqual(Object.values(PAGE_CLASSES).map((pageClass) => [pageClass.id, pageClass.label]), [
    ["explanatory", "explanatory page"],
    ["illustrated", "illustrated source"],
    ["pass-through", "pass-through source"],
    ["derived-lookup", "derived lookup"],
    ["record-pointer", "record pointer page"],
  ]);
  assert.deepEqual(PAGE_CLASSES.explanatory.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "provenance-line", "altitude-trio",
    "concept-carrier", "architecture-carrier", "technical-carrier", "bound-figures",
  ]);
  // An illustrated source needs a figure at its head and no trio; a
  // pass-through source needs only its unchanged region.
  assert.deepEqual(PAGE_CLASSES.illustrated.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "provenance-line", "source-region", "page-head-figure", "bound-figures",
  ]);
  assert.deepEqual(PAGE_CLASSES.passThrough.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "provenance-line", "source-region", "bound-figures",
  ]);
  assert.deepEqual(PAGE_CLASSES.derivedLookup.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "provenance-line", "derived-table",
  ]);
  assert.deepEqual(PAGE_CLASSES.recordPointer.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "folder-table",
  ]);
  assert.equal(PAGE_CLASSES.explanatory.requirements.find((requirement) => requirement.id === "altitude-trio").demand,
    "the altitude trio concept, architecture, technical");
  assert.equal(PAGE_CLASSES.recordPointer.requirements.find((requirement) => requirement.id === "folder-table").demand,
    "one folder table with the columns Folder, Purpose, Count, Repository");
  assert.equal(PAGE_CLASSES.explanatory.requirements.find((requirement) => requirement.id === "technical-carrier").demand,
    "a figure and a table inside the technical panel");
  for (const frozen of [ALTITUDE_PANELS, CARRIER_ELEMENTS, PANEL_CARRIERS, PANEL_CARRIER_ALTERNATES, RECORD_POINTER_COLUMNS, PAGE_CLASSES, PAGE_CLASSES.explanatory, PAGE_CLASSES.explanatory.requirements]) {
    assert.equal(Object.isFrozen(frozen), true);
  }
});

test("eligibility follows the adapter's own classification, not a source path", async () => {
  const root = await portalFixture();
  try {
    await configureFixture(root, {
      layers: [
        { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
        { id: "system", label: "System", description: "System", prefixes: ["docs/architecture"] },
        { id: "reference", label: "Reference", description: "Reference", fallback: true },
      ],
      records: { enabled: false, layer: "system", pointers: [{ folder: "docs/decisions", id_prefix: "ADR", purpose: "Accepted decisions." }] },
    });
    await mkdir(path.join(root, "docs/architecture"));
    await mkdir(path.join(root, "docs/decisions"));
    await writeFile(path.join(root, "docs/product.md"), COMPOSED_PAGE);
    await writeFile(path.join(root, "docs/architecture/system.md"), COMPOSED_PAGE);
    await writeFile(path.join(root, "docs/guide.md"), SHELL_PAGE);
    await writeFile(path.join(root, "docs/broken.md"), "---\ntitle: [unterminated\n---\n\n# Broken\n");
    await writeFile(path.join(root, "docs/decisions/ADR-0001-first.md"), "# ADR-0001: first\n");
    commitFixture(root, "add fixture sources across every layer");
    runAdapter(root);
    const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
    const { pages } = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.equal(pages.some((page) => page.source_path === "docs/broken.md" && page.stale), true);
    assert.deepEqual(classifyPortalPages(config, pages), [
      { route: "orient/product", source: "docs/product.md", pageClass: "explanatory", reason: null, figures: [] },
      { route: "reference/guide", source: "docs/guide.md", pageClass: "explanatory", reason: null, figures: [] },
      { route: "system/architecture/system", source: "docs/architecture/system.md", pageClass: "explanatory", reason: null, figures: [] },
      { route: "system/records", source: RECORD_POINTER_SOURCE, pageClass: "record-pointer" },
    ]);
    // A page class comes from the configuration by exact source, else by the
    // longest prefix; a binding follows its route.
    const classed = classifyPortalPages({
      ...config,
      page_classes: [{ source: "docs/guide.md", class: "illustrated" }, { prefix: "docs/architecture", class: "pass-through", reason: "governance" }],
      figures: [{ declaration: "figures/head.json", route: "reference/guide" }],
    }, pages);
    assert.deepEqual(classed.map((assignment) => [assignment.route, assignment.pageClass, assignment.reason ?? null, (assignment.figures ?? []).length]), [
      ["orient/product", "explanatory", null, 0],
      ["reference/guide", "illustrated", null, 1],
      ["system/architecture/system", "pass-through", "governance", 0],
      ["system/records", "record-pointer", null, 0],
    ]);
    assert.equal(recordPointerRoute(config), "system/records");
    assert.equal(recordPointerRoute({ ...config, records: { ...config.records, enabled: true, pointers: [] } }), null);
    // A source that did not build drops out of the eligible set, which is the
    // silence this gate exists to break: the run refuses it by name.
    const stale = staleSourceFailures(pages);
    assert.equal(stale.length, 1, JSON.stringify(stale));
    assert.match(stale[0], /^docs\/broken\.md did not build and is a stale stub at /);
    assert.throws(() => assertNoStaleSources(pages), (error) => {
      assert.match(error.message, /1 source\(s\) did not build, so the eligible set is incomplete/);
      assert.match(error.message, /docs\/broken\.md/);
      return true;
    });
    const built = pages.filter((page) => !page.stale);
    assert.equal(assertNoStaleSources(built), `no-stale-sources:${built.length}`);
    // A carrier declared in the configuration reaches its own assignment, and
    // one that matches no published page fails rather than sitting unused.
    const assignments = classifyPortalPages({ ...config, page_carriers: [{ source: "docs/product.md", technical: "list" }] }, pages);
    assert.deepEqual(assignments.find((assignment) => assignment.source === "docs/product.md").carriers, { technical: "list" });
    assert.equal(assertDeclaredCarriers(config, assignments), "declared-carriers:0");
    const absent = { ...config, page_carriers: [{ source: "docs/gone.md", technical: "list" }] };
    assert.deepEqual(declaredCarrierFailures(absent, assignments), [
      "portal.config.json page_carriers declares a carrier for docs/gone.md, which this build did not publish",
    ]);
    assert.throws(() => assertDeclaredCarriers(absent, assignments), /1 declared carrier\(s\) match no published page/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a compliant page cannot mask a noncompliant page and an unvisited route cannot pass", () => {
  const assignments = [
    { route: "orient/product", source: "docs/product.md", pageClass: "explanatory", figures: panelBindings("orient/product") },
    { route: "reference/guide", source: "docs/guide.md", pageClass: "explanatory", figures: [] },
    { route: "reference/unvisited", source: "docs/unvisited.md", pageClass: "explanatory", figures: [] },
  ];
  const observations = [
    { route: "orient/product", ...COMPLIANT_OBSERVATION },
    { route: "reference/guide", ...COMPLIANT_OBSERVATION, companions: 0, altitudePanels: ["concept"], panelCarriers: { concept: { ...PROSE_PANEL, table: 1 }, architecture: null, technical: null } },
  ];
  const failures = pageClassFailures(assignments, observations);
  assert.equal(failures.filter((failure) => failure.includes("docs/product.md")).length, 0);
  assert.deepEqual(failures, [
    "docs/guide.md (explanatory page at reference/guide) lacks the altitude trio concept, architecture, technical: missing architecture, technical; present concept",
    "docs/guide.md (explanatory page at reference/guide) lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 0, table 1, list 0, pre 0)",
    "docs/unvisited.md (explanatory page at reference/unvisited) was not verified: the run collected no observation for this route",
  ]);
  assert.throws(() => assertPageClassCoverage(assignments, observations), (error) => {
    assert.match(error.message, /3 failure\(s\) across 3 eligible source\(s\)/);
    assert.match(error.message, /docs\/guide\.md/);
    assert.match(error.message, /docs\/unvisited\.md/);
    return true;
  });
  assert.equal(assertPageClassCoverage(assignments.slice(0, 1), observations), "page-class-coverage:1");
  // A failure the caller found while exercising a page joins the report.
  assert.throws(() => assertPageClassCoverage(assignments.slice(0, 1), observations, ["docs/product.md (at orient/product): tabs do not hide inactive layers"]), (error) => {
    assert.match(error.message, /1 failure\(s\) across 1 eligible source\(s\)/);
    assert.match(error.message, /tabs do not hide inactive layers/);
    return true;
  });
  assert.throws(() => pageClassFailures([{ route: "x", source: "docs/x.md", pageClass: "invented" }], []), /unknown page class invented for docs\/x\.md/);
});

test("the utility chrome rules refuse a bare shell, a second heading and present chrome", () => {
  const assignment = { route: "orient/product", source: "docs/product.md", pageClass: "explanatory", figures: panelBindings("orient/product") };
  const cases = [
    [{ headings: 0 }, "lacks exactly one visible primary heading: 0 visible h1 element(s)"],
    [{ headings: 2 }, "lacks exactly one visible primary heading: 2 visible h1 element(s)"],
    [{ displayControls: 0 }, "lacks a visible Display control: no visible Display control"],
    [{ commentChrome: 1 }, "lacks no present Comment chrome: 1 present Comment element(s)"],
    [{ provenance: false }, "lacks the visible source provenance line: no visible provenance line"],
    [{ altitudePanels: [] }, "missing concept, architecture, technical; present none"],
  ];
  for (const [override, expected] of cases) {
    const failures = pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION, ...override }]);
    assert.equal(failures.length, 1, JSON.stringify(failures));
    assert.equal(failures[0].startsWith("docs/product.md (explanatory page at orient/product) "), true, failures[0]);
    assert.equal(failures[0].includes(expected), true, failures[0]);
  }
  assert.deepEqual(pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION }]), []);
});

test("each panel is held to the carrier its altitude names, inside the panel that owns it", () => {
  const assignment = { route: "orient/product", source: "docs/product.md", pageClass: "explanatory", figures: panelBindings("orient/product") };
  const shaped = (overrides) => [{ route: assignment.route, ...COMPLIANT_OBSERVATION, panelCarriers: { ...CARRIED, ...overrides } }];
  const failures = (overrides) => pageClassFailures([assignment], shaped(overrides));
  const where = "docs/product.md (explanatory page at orient/product)";
  assert.deepEqual(failures({}), []);
  // A table or a stage answers for no altitude: Architecture with a table
  // and no figure fails, and so does Concept with only a stage.
  assert.deepEqual(failures({ architecture: { ...PROSE_PANEL, table: 1 } }),
    [`${where} lacks a figure inside the architecture panel: the architecture panel carries no figure (figure 0, stage 0, table 1, list 0, pre 0)`]);
  assert.deepEqual(failures({ concept: { ...PROSE_PANEL, stage: 1, pre: 1 } }),
    [`${where} lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 1, table 0, list 0, pre 1)`]);
  // Technical needs both: a figure without a table fails, a table without a
  // figure fails, and the two together pass.
  assert.deepEqual(failures({ technical: { ...PROSE_PANEL, figure: 1, list: 4, pre: 2 } }),
    [`${where} lacks a figure and a table inside the technical panel: the technical panel carries no table (figure 1, stage 0, table 0, list 4, pre 2)`]);
  assert.deepEqual(failures({ technical: { ...PROSE_PANEL, table: 1 } }),
    [`${where} lacks a figure and a table inside the technical panel: the technical panel carries no figure (figure 0, stage 0, table 1, list 0, pre 0)`]);
  assert.deepEqual(failures({ technical: { ...PROSE_PANEL, figure: 1, table: 1 } }), []);
  // A figure in the wrong panel answers for that panel only.
  assert.deepEqual(failures({ concept: PROSE_PANEL, architecture: { ...PROSE_PANEL, figure: 2 } }),
    [`${where} lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)`]);
  // A panel the page never rendered is named once, by the trio rule.
  assert.deepEqual(pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION, altitudePanels: ["concept", "architecture"], panelCarriers: { ...CARRIED, technical: null } }]),
    [`${where} lacks the altitude trio concept, architecture, technical: missing technical; present concept, architecture`]);
  // Every figure the configuration binds is drawn: one fewer on the page fails.
  assert.deepEqual(pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION, companions: 2 }]),
    [`${where} lacks every figure bound to the route in the configuration: the configuration binds 3 figure(s) and the page draws 2`]);
  // A page whose subject is its own carrier declares that in the portal
  // configuration: its Technical panel is a list beside its figure, and the
  // figure is never let off.
  const declared = { route: "reference/release-checklist", source: "docs/release-checklist.md", pageClass: "explanatory", figures: panelBindings("reference/release-checklist"), carriers: { technical: "list" } };
  const observed = (technical) => [{ route: declared.route, ...COMPLIANT_OBSERVATION, panelCarriers: { ...CARRIED, technical } }];
  assert.deepEqual(pageClassFailures([declared], observed({ ...PROSE_PANEL, figure: 1, list: 3 })), []);
  assert.deepEqual(pageClassFailures([declared], observed({ ...PROSE_PANEL, list: 3 })), [
    "docs/release-checklist.md (explanatory page at reference/release-checklist) lacks a figure and a table inside the technical panel: the technical panel carries no figure, which the configuration declares for it (figure 0, stage 0, table 0, list 3, pre 0)",
  ]);
  // The declaration is per source: the same shape on a page that declared
  // nothing still owes its altitude a table.
  assert.equal(failures({ technical: { ...PROSE_PANEL, figure: 1, list: 3 } }).length, 1);
});

test("an illustrated, pass-through or derived page is held to its own class", () => {
  const chrome = { headings: 1, provenance: true, displayControls: 1, commentChrome: 0, altitudePanels: [], panelCarriers: { concept: null, architecture: null, technical: null }, pointerColumns: [], pointerRows: 0 };
  const illustrated = { route: "reference/guide", source: "docs/guide.md", pageClass: "illustrated", figures: [{ declaration: "figures/head.json", route: "reference/guide" }] };
  const passThrough = { route: "system/adr", source: "docs/decisions/ADR-0001.md", pageClass: "pass-through", reason: "accepted-record", figures: [] };
  const lookup = { route: "system/capabilities", source: "docs/capabilities.md", pageClass: "derived-lookup", figures: [] };
  const observe = (route, overrides) => ({ route, ...chrome, sourceRegions: 1, companions: 0, headFigures: 0, tables: 0, ...overrides });
  assert.deepEqual(pageClassFailures([illustrated, passThrough, lookup], [
    observe("reference/guide", { companions: 1, headFigures: 1 }),
    observe("system/adr", {}),
    observe("system/capabilities", { sourceRegions: 0, tables: 1 }),
  ]), []);
  assert.deepEqual(pageClassFailures([illustrated, passThrough, lookup], [
    observe("reference/guide", { companions: 1, headFigures: 0, sourceRegions: 0 }),
    observe("system/adr", { companions: 1 }),
    observe("system/capabilities", { sourceRegions: 0, tables: 0 }),
  ]), [
    "docs/guide.md (illustrated source at reference/guide) lacks the unchanged source region: 0 source region(s)",
    "docs/guide.md (illustrated source at reference/guide) lacks a companion figure at the page head: no figure drawn above the source",
    "docs/decisions/ADR-0001.md (pass-through source at system/adr) lacks every figure bound to the route in the configuration: the configuration binds 0 figure(s) and the page draws 1",
    "docs/capabilities.md (derived lookup at system/capabilities) lacks the generated lookup table: no table outside a figure",
  ]);
});

test("the record pointer page is required to carry its folder table", () => {
  const assignment = { route: "system/records", source: RECORD_POINTER_SOURCE, pageClass: "record-pointer" };
  assert.deepEqual(pageClassFailures([assignment], [{ route: assignment.route, ...POINTER_OBSERVATION }]), []);
  for (const [override, expected] of [
    [{ pointerColumns: [] }, "table columns are absent"],
    [{ pointerColumns: ["Folder", "Purpose"] }, "table columns are Folder, Purpose"],
    [{ pointerRows: 0 }, "the folder table has no rows"],
  ]) {
    const failures = pageClassFailures([assignment], [{ route: assignment.route, ...POINTER_OBSERVATION, ...override }]);
    assert.equal(failures.length, 1, JSON.stringify(failures));
    assert.equal(failures[0], `${RECORD_POINTER_SOURCE} (record pointer page at system/records) lacks one folder table with the columns Folder, Purpose, Count, Repository: ${expected}`);
  }
});

test("a build with the records switch off carries no per-record route", () => {
  const config = { records: { enabled: false, layer: "system", pointers: [{ folder: "docs/decisions", id_prefix: "ADR", purpose: "Accepted decisions." }] } };
  const clean = ["dist/index.html", "dist/system/records/index.html", "dist/system/architecture/index.html", "dist/pagefind/pagefind.js"];
  assert.deepEqual(recordRouteFailures(config, [{ route: "system/architecture", source_path: "docs/architecture.md" }], clean), []);
  assert.deepEqual(recordRouteFailures(config, [], [...clean, "dist/system/records/ADR-0001-first/index.html"]), [
    "dist/system/records/ADR-0001-first/index.html is a per-record route below system/records although the records switch is off",
  ]);
  assert.deepEqual(recordRouteFailures(config, [{ route: "system/decisions/ADR-0001-first", source_path: "docs/decisions/ADR-0001-first.md" }], clean), [
    "docs/decisions/ADR-0001-first.md owns the route system/decisions/ADR-0001-first although docs/decisions is a pointer folder and the records switch is off",
  ]);
  assert.deepEqual(recordRouteFailures({ records: { enabled: true, layer: null, pointers: [] } }, [{ route: "system/decisions/ADR-0001-first", source_path: "docs/decisions/ADR-0001-first.md" }], clean), []);
  assert.throws(() => assertNoRecordRoutes(config, [], [...clean, "dist/system/records/ADR-0001-first/index.html"]), /the records switch is off but the build publishes record routes/);
});

test("palette pills must show the live tokens of the palette they select", () => {
  const tokens = {
    graphite: { canvas: "rgb(242, 243, 244)", accent: "rgb(0, 95, 86)" },
    slate: { canvas: "rgb(238, 242, 246)", accent: "rgb(23, 92, 168)" },
    sage: { canvas: "rgb(246, 243, 238)", accent: "rgb(140, 70, 20)" },
  };
  const group = Object.entries(tokens).map(([skin, token]) => ({ skin, ...token }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group] }, PALETTE_PILL_GROUPS), []);
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [] }, PALETTE_PILL_GROUPS), [
    "expected 1 palette pill group(s), the page renders 0",
  ]);
  const hardCoded = group.map((pill) => ({ ...pill, accent: "rgb(0, 95, 86)" }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [hardCoded] }, PALETTE_PILL_GROUPS), [
    "display panel 1: the slate accent swatch is rgb(0, 95, 86), the live token is rgb(23, 92, 168)",
    "display panel 1: the sage accent swatch is rgb(0, 95, 86), the live token is rgb(140, 70, 20)",
    "display panel 1: every accent swatch is rgb(0, 95, 86), so the pills do not preview the palette they select",
  ]);
  const missing = group.map((pill) => ({ ...pill, canvas: "" }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [missing] }, PALETTE_PILL_GROUPS),
    Object.keys(tokens).map((skin) => `display panel 1: the ${skin} pill carries no canvas swatch`)
      .concat(["display panel 1: every canvas swatch is , so the pills do not preview the palette they select"]));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group.slice(0, 2)] }, PALETTE_PILL_GROUPS), [
    "display panel 1 offers graphite, slate, the tokens define graphite, slate, sage",
  ]);
});

test("the record table's phone labels are the adapter's columns", async () => {
  const css = await readFile(new URL("../src/styles/portal.css", import.meta.url), "utf8");
  const labelled = [...css.matchAll(/\.portal-record-folders td:nth-child\((\d+)\)::before \{ content: "([^"]+)"; \}/g)].map((match) => [Number(match[1]), match[2]]);
  assert.deepEqual(labelled, [[3, RECORD_POINTER_COLUMNS[2]], [4, RECORD_POINTER_COLUMNS[3]]]);
});

import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { paletteSwatchFailures, PALETTE_PILL_GROUPS } from "../scripts/browser-verify.mjs";
import {
  ALTITUDE_PANELS, CONCEPT_CARRIERS, PAGE_CLASSES, RECORD_POINTER_COLUMNS, RECORD_POINTER_SOURCE,
  assertNoRecordRoutes, assertPageClassCoverage, classifyPortalPages, pageClassFailures, recordPointerRoute, recordRouteFailures,
} from "../scripts/page-classes.mjs";
import { COMPOSED_PAGE, SHELL_PAGE } from "./page-shapes.mjs";
import { commitFixture, configureFixture, portalFixture, runAdapter, starterRoot } from "./portal-fixture.mjs";

const COMPLIANT_OBSERVATION = Object.freeze({
  headings: 1, provenance: true, displayControls: 2, commentChrome: 0,
  altitudePanels: [...ALTITUDE_PANELS], conceptCarriers: 1, pointerColumns: [], pointerRows: 0,
});
const POINTER_OBSERVATION = Object.freeze({
  headings: 1, provenance: false, displayControls: 2, commentChrome: 0,
  altitudePanels: [], conceptCarriers: 0, pointerColumns: [...RECORD_POINTER_COLUMNS], pointerRows: 4,
});

test("the page-class rules are declared once and enumerate what each class must show", () => {
  assert.deepEqual(ALTITUDE_PANELS, ["concept", "architecture", "technical"]);
  assert.deepEqual(CONCEPT_CARRIERS, ["figure", "table", "pre"]);
  assert.deepEqual(RECORD_POINTER_COLUMNS, ["Folder", "Purpose", "Count", "Repository"]);
  assert.deepEqual(Object.keys(PAGE_CLASSES), ["explanatory", "recordPointer"]);
  assert.deepEqual(Object.values(PAGE_CLASSES).map((pageClass) => [pageClass.id, pageClass.label]), [
    ["explanatory", "explanatory page"],
    ["record-pointer", "record pointer page"],
  ]);
  assert.deepEqual(PAGE_CLASSES.explanatory.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "provenance-line", "altitude-trio", "concept-figure",
  ]);
  assert.deepEqual(PAGE_CLASSES.recordPointer.requirements.map((requirement) => requirement.id), [
    "single-heading", "display-control", "no-present-chrome", "folder-table",
  ]);
  assert.equal(PAGE_CLASSES.explanatory.requirements.find((requirement) => requirement.id === "altitude-trio").demand,
    "the altitude trio concept, architecture, technical");
  assert.equal(PAGE_CLASSES.recordPointer.requirements.find((requirement) => requirement.id === "folder-table").demand,
    "one folder table with the columns Folder, Purpose, Count, Repository");
  for (const frozen of [ALTITUDE_PANELS, CONCEPT_CARRIERS, RECORD_POINTER_COLUMNS, PAGE_CLASSES, PAGE_CLASSES.explanatory, PAGE_CLASSES.explanatory.requirements]) {
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
      { route: "orient/product", source: "docs/product.md", pageClass: "explanatory" },
      { route: "reference/guide", source: "docs/guide.md", pageClass: "explanatory" },
      { route: "system/architecture/system", source: "docs/architecture/system.md", pageClass: "explanatory" },
      { route: "system/records", source: RECORD_POINTER_SOURCE, pageClass: "record-pointer" },
    ]);
    assert.equal(recordPointerRoute(config), "system/records");
    assert.equal(recordPointerRoute({ ...config, records: { ...config.records, enabled: true, pointers: [] } }), null);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a compliant page cannot mask a noncompliant page and an unvisited route cannot pass", () => {
  const assignments = [
    { route: "orient/product", source: "docs/product.md", pageClass: "explanatory" },
    { route: "reference/guide", source: "docs/guide.md", pageClass: "explanatory" },
    { route: "reference/unvisited", source: "docs/unvisited.md", pageClass: "explanatory" },
  ];
  const observations = [
    { route: "orient/product", ...COMPLIANT_OBSERVATION },
    { route: "reference/guide", ...COMPLIANT_OBSERVATION, altitudePanels: ["concept"], conceptCarriers: 0 },
  ];
  const failures = pageClassFailures(assignments, observations);
  assert.equal(failures.filter((failure) => failure.includes("docs/product.md")).length, 0);
  assert.deepEqual(failures, [
    "docs/guide.md (explanatory page at reference/guide) lacks the altitude trio concept, architecture, technical: missing architecture, technical; present concept",
    "docs/guide.md (explanatory page at reference/guide) lacks a figure, table, pre carrier in the Concept panel: the Concept panel carries prose only",
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
  const assignment = { route: "orient/product", source: "docs/product.md", pageClass: "explanatory" };
  const cases = [
    [{ headings: 0 }, "lacks exactly one visible primary heading: 0 visible h1 element(s)"],
    [{ headings: 2 }, "lacks exactly one visible primary heading: 2 visible h1 element(s)"],
    [{ displayControls: 0 }, "lacks a visible Display control: no visible Display control"],
    [{ commentChrome: 1 }, "lacks no present Comment chrome: 1 present Comment element(s)"],
    [{ provenance: false }, "lacks the visible source provenance line: no visible provenance line"],
    [{ altitudePanels: [] }, "missing concept, architecture, technical; present none"],
    [{ conceptCarriers: 0 }, "the Concept panel carries prose only"],
  ];
  for (const [override, expected] of cases) {
    const failures = pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION, ...override }]);
    assert.equal(failures.length, 1, JSON.stringify(failures));
    assert.equal(failures[0].startsWith("docs/product.md (explanatory page at orient/product) "), true, failures[0]);
    assert.equal(failures[0].includes(expected), true, failures[0]);
  }
  assert.deepEqual(pageClassFailures([assignment], [{ route: assignment.route, ...COMPLIANT_OBSERVATION }]), []);
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
    instrument: { canvas: "rgb(242, 243, 244)", accent: "rgb(0, 95, 86)" },
    editorial: { canvas: "rgb(238, 242, 246)", accent: "rgb(23, 92, 168)" },
    ink: { canvas: "rgb(246, 243, 238)", accent: "rgb(140, 70, 20)" },
  };
  const group = Object.entries(tokens).map(([skin, token]) => ({ skin, ...token }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group, group] }, PALETTE_PILL_GROUPS), []);
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group] }, PALETTE_PILL_GROUPS), [
    "expected 2 palette pill group(s), the page renders 1",
  ]);
  const hardCoded = group.map((pill) => ({ ...pill, accent: "rgb(0, 95, 86)" }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group, hardCoded] }, PALETTE_PILL_GROUPS), [
    "display panel 2: the editorial accent swatch is rgb(0, 95, 86), the live token is rgb(23, 92, 168)",
    "display panel 2: the ink accent swatch is rgb(0, 95, 86), the live token is rgb(140, 70, 20)",
    "display panel 2: every accent swatch is rgb(0, 95, 86), so the pills do not preview the palette they select",
  ]);
  const missing = group.map((pill) => ({ ...pill, canvas: "" }));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [missing, group] }, PALETTE_PILL_GROUPS),
    Object.keys(tokens).map((skin) => `display panel 1: the ${skin} pill carries no canvas swatch`)
      .concat(["display panel 1: every canvas swatch is , so the pills do not preview the palette they select"]));
  assert.deepEqual(paletteSwatchFailures({ tokens, groups: [group.slice(0, 2), group] }, PALETTE_PILL_GROUPS), [
    "display panel 1 offers instrument, editorial, the tokens define instrument, editorial, ink",
  ]);
});

test("the starter mirror of the verifier and its tests is byte-identical to the live copies", async () => {
  const mirror = path.join(starterRoot, "..", "assets/docs-portal/starter");
  const present = await readdir(mirror).then(() => true, () => false);
  assert.equal(present, true, "the codeflow checkout carries the starter mirror this test compares against");
  assert.deepEqual(await mirrorDrift(starterRoot, mirror, ["scripts", "tests"]), []);
});

test("the mirror equality check names a perturbed copy", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-mirror-"));
  try {
    for (const side of ["live", "mirror"]) {
      await mkdir(path.join(root, side, "scripts"), { recursive: true });
      await writeFile(path.join(root, side, "scripts/browser-verify.mjs"), "export const gate = 1;\n");
      await writeFile(path.join(root, side, "scripts/page-classes.mjs"), "export const rules = 1;\n");
    }
    assert.deepEqual(await mirrorDrift(path.join(root, "live"), path.join(root, "mirror"), ["scripts"]), []);
    await writeFile(path.join(root, "mirror/scripts/page-classes.mjs"), "export const rules = 2;\n");
    assert.deepEqual(await mirrorDrift(path.join(root, "live"), path.join(root, "mirror"), ["scripts"]), ["byte drift at scripts/page-classes.mjs"]);
    await rm(path.join(root, "mirror/scripts/page-classes.mjs"));
    assert.deepEqual(await mirrorDrift(path.join(root, "live"), path.join(root, "mirror"), ["scripts"]), ["scripts/page-classes.mjs is missing from the mirror"]);
    await writeFile(path.join(root, "mirror/scripts/page-classes.mjs"), "export const rules = 1;\n");
    await writeFile(path.join(root, "mirror/scripts/extra.mjs"), "export const extra = 1;\n");
    assert.deepEqual(await mirrorDrift(path.join(root, "live"), path.join(root, "mirror"), ["scripts"]), ["scripts/extra.mjs is in the mirror but not in the live copy"]);
  } finally { await rm(root, { recursive: true, force: true }); }
});

// The starter ships the verifier and its tests to every consumer, so a fix
// applied to one copy and not the other would leave consumers on a gate the
// repository no longer runs. Both directions are reported: a file the mirror
// lacks and a file only the mirror carries.
async function mirrorDrift(liveRoot, mirrorRoot, directories) {
  const drift = [];
  for (const directory of directories) {
    const live = await relativeFiles(path.join(liveRoot, directory), directory);
    const mirrored = await relativeFiles(path.join(mirrorRoot, directory), directory);
    for (const relative of live) {
      if (!mirrored.includes(relative)) { drift.push(`${relative} is missing from the mirror`); continue; }
      const [a, b] = await Promise.all([readFile(path.join(liveRoot, relative)), readFile(path.join(mirrorRoot, relative))]);
      if (!a.equals(b)) drift.push(`byte drift at ${relative}`);
    }
    for (const relative of mirrored) if (!live.includes(relative)) drift.push(`${relative} is in the mirror but not in the live copy`);
  }
  return drift.sort();
}

async function relativeFiles(directory, prefix) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const relative = `${prefix}/${entry.name}`;
    if (entry.isDirectory()) files.push(...await relativeFiles(path.join(directory, entry.name), relative));
    else if (entry.isFile()) files.push(relative);
  }
  return files.sort();
}

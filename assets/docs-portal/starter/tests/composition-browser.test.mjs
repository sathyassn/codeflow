import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "@playwright/test";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { collectBuiltArtifacts } from "../scripts/publication.mjs";
import { observePortalPage, revealAltitudePanel } from "../scripts/browser-verify.mjs";
import { RECORD_POINTER_COLUMNS, assertNoRecordRoutes, assertPageClassCoverage, classifyPortalPages, pageClassFailures } from "../scripts/page-classes.mjs";
import { COMPOSED_PAGE, MISSING_CONCEPT_PAGE, MISSING_TECHNICAL_PAGE, PROSE_TRIO_PAGE, SHELL_PAGE } from "./page-shapes.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture } from "./portal-fixture.mjs";

test("the gate reads a real build and names every source that is not composed", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    await configureFixture(root, {
      layers: [
        { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md", "docs/adoption.md"] },
        { id: "system", label: "System", description: "System", prefixes: ["docs/architecture"] },
        { id: "reference", label: "Reference", description: "Reference", fallback: true },
      ],
      records: { enabled: false, layer: "system", pointers: [{ folder: "docs/decisions", id_prefix: "ADR", purpose: "Accepted decisions." }] },
    });
    await rm(path.join(root, "docs/seed.md"));
    for (const directory of ["docs/architecture", "docs/decisions"]) await mkdir(path.join(root, directory));
    const sources = {
      "docs/product.md": COMPOSED_PAGE,
      "docs/adoption.md": MISSING_TECHNICAL_PAGE,
      "docs/architecture/system.md": COMPOSED_PAGE,
      "docs/architecture/present.md": MISSING_CONCEPT_PAGE,
      "docs/releasing.md": SHELL_PAGE,
      "docs/checklist.md": PROSE_TRIO_PAGE,
    };
    for (const [relative, contents] of Object.entries(sources)) await writeFile(path.join(root, relative), contents);
    await writeFile(path.join(root, "docs/decisions/ADR-0001-first.md"), "# ADR-0001: first\n");
    commitFixture(root, "compose two pages and leave four short of their class");
    runLocalAdapter(root);
    buildFixture(root);

    const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
    const { pages } = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    const artifacts = await collectBuiltArtifacts(path.join(root, "dist"));
    assertNoRecordRoutes(config, pages, artifacts.map((artifact) => artifact.path));
    const assignments = classifyPortalPages(config, pages);
    assert.deepEqual(assignments.map((assignment) => assignment.route), [
      "orient/adoption", "orient/product", "reference/checklist", "reference/releasing",
      "system/architecture/present", "system/architecture/system", "system/records",
    ]);

    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    let observations;
    try {
      const page = await browser.newPage();
      observations = [];
      for (const assignment of assignments) {
        await page.setContent(await readFile(path.join(root, "dist", assignment.route, "index.html"), "utf8"));
        observations.push({ route: assignment.route, ...await observePortalPage(page) });
      }
    } finally { await browser.close(); }

    const failures = pageClassFailures(assignments, observations);
    // The two composed pages and the generated pointer page carry their class.
    for (const passing of ["orient/product", "system/architecture/system", "system/records"]) {
      assert.deepEqual(failures.filter((failure) => failure.includes(`at ${passing})`)), [], passing);
    }
    assert.deepEqual(failures, [
      "docs/adoption.md (explanatory page at orient/adoption) lacks the altitude trio concept, architecture, technical: missing technical; present concept, architecture",
      "docs/checklist.md (explanatory page at reference/checklist) lacks a figure, table, pre carrier in the Concept panel: the Concept panel carries prose only",
      "docs/releasing.md (explanatory page at reference/releasing) lacks the altitude trio concept, architecture, technical: missing concept, architecture, technical; present none",
      "docs/releasing.md (explanatory page at reference/releasing) lacks a figure, table, pre carrier in the Concept panel: the Concept panel carries prose only",
      "docs/architecture/present.md (explanatory page at system/architecture/present) lacks the altitude trio concept, architecture, technical: missing concept; present architecture, technical",
      "docs/architecture/present.md (explanatory page at system/architecture/present) lacks a figure, table, pre carrier in the Concept panel: the Concept panel carries prose only",
    ]);
    assert.throws(() => assertPageClassCoverage(assignments, observations), /6 failure\(s\) across 7 eligible source\(s\)/);

    const pointer = observations.find((observation) => observation.route === "system/records");
    assert.deepEqual(pointer.pointerColumns, [...RECORD_POINTER_COLUMNS]);
    assert.equal(pointer.pointerRows, 1);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a strict-ID preview inside a shut panel is reached by opening that panel", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    await rm(path.join(root, "docs/seed.md"));
    // The citation goes in the Technical panel, which the tablist shuts on
    // load, so the fixture reproduces what the composed guide pages do.
    await writeFile(path.join(root, "docs/product.md"), COMPOSED_PAGE.replace("## Technical\n", "## Technical\n\nThe registry entry is CAP-001.\n"));
    await writeFile(path.join(root, "docs/capability.md"), "---\nid: CAP-001\ntitle: Portal composition\nstatus: active\n---\n\n# Portal composition\n");
    commitFixture(root, "cite a record id inside the technical panel");
    runLocalAdapter(root);
    buildFixture(root);

    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      await page.goto(pathToFileURL(path.join(root, "dist/orient/product/index.html")).href);
      await page.addScriptTag({ content: await readFile(path.join(root, "public/portal-tabs.js"), "utf8") });
      const trigger = page.locator(".portal-id-preview > a").first();
      assert.equal(await trigger.getAttribute("href"), "/reference/capability/");
      assert.equal(await trigger.isVisible(), false, "the citation starts inside the shut panel");
      assert.equal(await revealAltitudePanel(page, trigger), "portal-panel-technical");
      assert.equal(await trigger.isVisible(), true);
      assert.equal(await page.locator('[role="tab"][aria-controls="portal-panel-technical"]').first().getAttribute("aria-selected"), "true");
      // Already open stays open, and an element outside every panel needs no
      // panel at all, which is the path a page without a trio keeps.
      assert.equal(await revealAltitudePanel(page, trigger), "portal-panel-technical");
      assert.equal(await revealAltitudePanel(page, page.locator("h1").first()), null);
    } finally { await browser.close(); }
  } finally { await rm(root, { recursive: true, force: true }); }
});

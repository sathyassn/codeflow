import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "@playwright/test";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { collectBuiltArtifacts } from "../scripts/publication.mjs";
import { applyDisplayState, assertAltitudeTablist, observePortalPage, revealAltitudePanel } from "../scripts/browser-verify.mjs";
import { RECORD_POINTER_COLUMNS, assertNoRecordRoutes, assertPageClassCoverage, classifyPortalPages, pageClassFailures } from "../scripts/page-classes.mjs";
import { COMPOSED_PAGE, MISSING_CONCEPT_PAGE, MISSING_TECHNICAL_PAGE, NO_TABLE_TECHNICAL_PAGE, PANEL_DECLARATIONS, SHELL_PAGE, panelBindings, writeFigureInputs } from "./page-shapes.mjs";
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
      // Each panel's figure is bound here. A panel left unbound is a panel of
      // prose, and a figure bound to the wrong panel answers for that panel.
      figures: [
        ...panelBindings("orient/product"),
        ...panelBindings("orient/adoption", ["concept", "architecture"]),
        ...panelBindings("system/architecture/system"),
        ...panelBindings("system/architecture/present", ["architecture", "technical"]),
        ...panelBindings("system/architecture/planes", ["concept", "technical"]),
        ...panelBindings("reference/checklist", ["architecture", "technical"]),
        ...panelBindings("reference/runbook"),
        { declaration: PANEL_DECLARATIONS.concept, route: "reference/handbook", panel: "architecture" },
        ...panelBindings("reference/handbook", ["technical"]),
      ],
    });
    await rm(path.join(root, "docs/seed.md"));
    for (const directory of ["docs/architecture", "docs/decisions"]) await mkdir(path.join(root, directory));
    await writeFigureInputs(root);
    const sources = {
      "docs/product.md": COMPOSED_PAGE,
      "docs/adoption.md": MISSING_TECHNICAL_PAGE,
      "docs/architecture/system.md": COMPOSED_PAGE,
      "docs/architecture/present.md": MISSING_CONCEPT_PAGE,
      "docs/architecture/planes.md": COMPOSED_PAGE,
      "docs/releasing.md": SHELL_PAGE,
      "docs/checklist.md": COMPOSED_PAGE,
      "docs/runbook.md": NO_TABLE_TECHNICAL_PAGE,
      "docs/handbook.md": COMPOSED_PAGE,
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
      "orient/adoption", "orient/product", "reference/checklist", "reference/handbook",
      "reference/releasing", "reference/runbook", "system/architecture/planes",
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
      "docs/checklist.md (explanatory page at reference/checklist) lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)",
      "docs/handbook.md (explanatory page at reference/handbook) lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)",
      "docs/releasing.md (explanatory page at reference/releasing) lacks the altitude trio concept, architecture, technical: missing concept, architecture, technical; present none",
      "docs/runbook.md (explanatory page at reference/runbook) lacks a figure and a table inside the technical panel: the technical panel carries no table (figure 1, stage 0, table 0, list 0, pre 0)",
      "docs/architecture/planes.md (explanatory page at system/architecture/planes) lacks a figure inside the architecture panel: the architecture panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)",
      "docs/architecture/present.md (explanatory page at system/architecture/present) lacks the altitude trio concept, architecture, technical: missing concept; present architecture, technical",
    ]);
    assert.throws(() => assertPageClassCoverage(assignments, observations), /7 failure\(s\) across 10 eligible source\(s\)/);

    const pointer = observations.find((observation) => observation.route === "system/records");
    assert.deepEqual(pointer.pointerColumns, [...RECORD_POINTER_COLUMNS]);
    assert.equal(pointer.pointerRows, 1);
  } finally { await rm(root, { recursive: true, force: true }); }
});

// A carrier is one thing the reader sees, with something in it. The rows a
// stage renders are its own internals, and an empty figure, table or list is a
// shape with nothing in it, so neither can answer for the carrier an altitude
// calls for. The rule reads the DOM, so the fixtures are authored as markup.
test("a hollow carrier, or one inside another carrier, does not count", { skip: process.platform === "win32", timeout: 120_000 }, async () => {
  const stage = '<figure class="portal-stage"><div class="portal-stage-flow"><ul class="portal-stage-group"><li class="portal-stage-node"><span class="k">first</span></li></ul><table><tbody><tr><td>inside</td></tr></tbody></table></div><figcaption>a stage</figcaption></figure>';
  const panel = (name, body) => `<section class="portal-altitude" data-altitude="${name}" id="portal-panel-${name}">${body}</section>`;
  const document = (concept, architecture, technical) =>
    `<h1>Composed page</h1><div class="portal-provenance">source</div><button data-testid="portal-display-btn">Display</button>` +
    `<div class="sl-markdown-content">${panel("concept", concept)}${panel("architecture", architecture)}${panel("technical", technical)}</div>`;
  const chrome = { headings: 1, provenance: true, displayControls: 1, commentChrome: 0 };
  const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
  try {
    const page = await browser.newPage();
    const observe = async (html) => {
      await page.setContent(html);
      return { route: "reference/checklist", ...await observePortalPage(page), ...chrome, pointerColumns: [], pointerRows: 0 };
    };
    const declared = { route: "reference/checklist", source: "docs/checklist.md", pageClass: "explanatory", carriers: { technical: "list" } };

    const hollow = await observe(document(
      '<figure class="cf-fig"><figcaption></figcaption></figure><p>prose</p>',
      "<table><thead><tr><th>Part</th></tr></thead><tbody></tbody></table>",
      `${stage}<ul><li></li></ul>`,
    ));
    assert.deepEqual(hollow.panelCarriers, {
      concept: { figure: 0, stage: 0, table: 0, list: 0, pre: 0 },
      architecture: { figure: 0, stage: 0, table: 0, list: 0, pre: 0 },
      technical: { figure: 0, stage: 1, table: 0, list: 0, pre: 0 },
    });
    assert.deepEqual(pageClassFailures([declared], [hollow]), [
      "docs/checklist.md (explanatory page at reference/checklist) lacks a figure inside the concept panel: the concept panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)",
      "docs/checklist.md (explanatory page at reference/checklist) lacks a figure inside the architecture panel: the architecture panel carries no figure (figure 0, stage 0, table 0, list 0, pre 0)",
      "docs/checklist.md (explanatory page at reference/checklist) lacks a figure and a table inside the technical panel: the technical panel carries no figure and no list, which the configuration declares for it (figure 0, stage 1, table 0, list 0, pre 0)",
    ]);

    // The same page, authored: a grammar figure in every panel, and the
    // checklist the declaration is for. The legend list and the twin table
    // inside a figure are its own parts, not further carriers.
    const figure = '<figure class="cf-fig"><svg class="cf-fig-svg"></svg><ul class="cf-legend"><li>key</li></ul><figcaption>a figure</figcaption><details class="cf-twin"><table><tbody><tr><td>row</td></tr></tbody></table></details></figure>';
    const authored = await observe(document(
      figure,
      `${figure}<table><thead><tr><th>Part</th></tr></thead><tbody><tr><td>First</td></tr></tbody></table>`,
      `${figure}<ul><li>Cut the release branch</li><li>Run the gate</li></ul>`,
    ));
    assert.deepEqual(authored.panelCarriers, {
      concept: { figure: 1, stage: 0, table: 0, list: 0, pre: 0 },
      architecture: { figure: 1, stage: 0, table: 1, list: 0, pre: 0 },
      technical: { figure: 1, stage: 0, table: 0, list: 1, pre: 0 },
    });
    assert.deepEqual(pageClassFailures([declared], [authored]), []);
  } finally { await browser.close(); }
});

// Presence in the document is not reach: the trio is a tablist a reader can
// operate, so each way it can come apart is broken on a real build and the
// check is required to name it.
test("the altitude tablist is held to three connected tabs that each open their panel", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    await rm(path.join(root, "docs/seed.md"));
    await writeFile(path.join(root, "docs/product.md"), COMPOSED_PAGE);
    commitFixture(root, "compose a page that carries the trio");
    runLocalAdapter(root);
    buildFixture(root);
    const built = pathToFileURL(path.join(root, "dist/orient/product/index.html")).href;
    const tabScript = await readFile(path.join(root, "public/portal-tabs.js"), "utf8");
    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      const load = async (breakage) => {
        await page.goto(built);
        await page.addScriptTag({ content: tabScript });
        if (breakage) await page.evaluate(breakage);
      };
      await load(null);
      assert.deepEqual(await assertAltitudeTablist(page), ["portal-panel-concept", "portal-panel-architecture", "portal-panel-technical"]);
      await load(() => document.querySelector(".portal-altitude-tabs").remove());
      await assert.rejects(assertAltitudeTablist(page), /offers 0 tab\(s\), the trio needs 3/);
      await load(() => document.querySelector('[role="tab"][aria-controls="portal-panel-technical"]').setAttribute("aria-controls", "portal-panel-elsewhere"));
      await assert.rejects(assertAltitudeTablist(page), /tabs control .*portal-panel-elsewhere/);
      await load(() => document.querySelector("#portal-panel-technical").style.setProperty("display", "none", "important"));
      await assert.rejects(assertAltitudeTablist(page), /selecting the technical tab did not open its panel/);
    } finally { await browser.close(); }
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

// A theme the page has not stored is a theme its own appearance owner can undo,
// and a scan that lands in between reads one mode's text on the other mode's
// canvas. Both paths through the helper are held to the same postcondition.
test("setting the display state leaves the preference and the attribute agreeing", { skip: process.platform === "win32", timeout: 120_000 }, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "portal-display-"));
  try {
    // A built page carries its component behaviour in an ES module bundle,
    // which a file URL will not load, so the panel here carries the two writes
    // the component makes on a pill click and nothing else.
    await writeFile(path.join(directory, "control.html"), `<!doctype html><html data-theme="light" data-cfp-skin="graphite"><body>
<button type="button" data-testid="portal-display-btn">Display</button>
<div data-testid="portal-display-panel" hidden>
<div class="pills" data-group="skin"><button type="button" data-value="sage" data-testid="skin-sage">Sage</button></div>
<div class="pills" data-group="appearance"><button type="button" data-value="dark" data-testid="appearance-dark">Dark</button></div>
</div>
<script>
const root = document.documentElement;
const panel = document.querySelector('[data-testid="portal-display-panel"]');
document.querySelector('[data-testid="portal-display-btn"]').addEventListener("click", () => { panel.hidden = !panel.hidden; });
panel.addEventListener("click", (event) => {
  const pill = event.target.closest(".pills button");
  if (!pill) return;
  if (pill.closest(".pills").dataset.group === "appearance") { localStorage.setItem("starlight-theme", pill.dataset.value); root.dataset.theme = pill.dataset.value; }
  else { localStorage.setItem("cf-portal-skin", pill.dataset.value); root.dataset.cfpSkin = pill.dataset.value; }
});
</script></body></html>`);
    await writeFile(path.join(directory, "bare.html"), `<!doctype html><html data-theme="light" data-cfp-skin="graphite"><body><h1>A surface with no Display control</h1></body></html>`);
    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      for (const [file, expected] of [["control.html", "control"], ["bare.html", "preference"]]) {
        await page.goto(pathToFileURL(path.join(directory, file)).href);
        await page.evaluate(() => localStorage.clear());
        assert.equal(await applyDisplayState(page, "sage", "dark"), expected, `${file} took the wrong path`);
        assert.deepEqual(await page.evaluate(() => ({
          theme: document.documentElement.dataset.theme,
          skin: document.documentElement.dataset.cfpSkin,
          appearance: localStorage.getItem("starlight-theme"),
          palette: localStorage.getItem("cf-portal-skin"),
        })), { theme: "dark", skin: "sage", appearance: "dark", palette: "sage" }, `${file} left the preference and the attribute disagreeing`);
      }
    } finally { await browser.close(); }
  } finally { await rm(directory, { recursive: true, force: true }); }
});

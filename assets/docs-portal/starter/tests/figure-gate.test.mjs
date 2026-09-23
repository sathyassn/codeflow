// The figure gate on a real build: one fixture repository whose pages cover
// every page class and every way a figure binding can go wrong. The adapter
// refuses what it can prove wrong from committed inputs; the built site is
// then read in a browser, where the class rules and the twelve figure rules
// are applied to what actually renders.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { chromium } from "@playwright/test";
import { sha256 } from "../scripts/lib.mjs";
import { figureGateFailures, observePortalPage } from "../scripts/browser-verify.mjs";
import { classifyPortalPages, declaredCarrierFailures, pageClassFailures } from "../scripts/page-classes.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { COMPOSED_PAGE, FIGURE_FACTS_PATH, SHELL_PAGE, panelBindings, specimen, writeFigureInputs } from "./page-shapes.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture } from "./portal-fixture.mjs";

const GUIDE = [
  "# Install guide", "",
  "Read [the product page](product.md) before installing.", "",
  "<div class=\"raw\">Raw markup stays text.</div>", "",
  "## Install", "",
  "1. Run the installer.",
  "2. Check the result.", "",
  "## Verify", "",
  "Run the check and read its report.", "",
].join("\n");
const BARE = "# Bare source\n\nAn illustrated source with nothing bound to it.\n";
const BROKEN = "# Broken figure\n\nA source whose bound figure breaks a rule.\n";
const ADR = "---\nid: ADR-0001\nstatus: accepted\n---\n\n# ADR-0001: first decision\n\nThe decision, recorded as it was accepted.\n";
const POLICY = `${JSON.stringify({ git: { commit_desc_max_len: 50, commit_subject_max_len: 72 } }, null, 2)}\n`;

const LAYERS = [
  { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
  { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
  { id: "reference", label: "Reference", description: "Reference", fallback: true },
];
const PAGE_CLASSES = [
  { source: "docs/guide.md", class: "illustrated" },
  { source: "docs/bare.md", class: "illustrated" },
  { source: "docs/broken.md", class: "illustrated" },
  { prefix: "docs/decisions", class: "pass-through", reason: "accepted-record" },
];
const FIGURES = [
  ...panelBindings("orient/product"),
  { declaration: "figures/commit-limits.json", route: "reference/guide" },
  { declaration: "figures/install-steps.json", route: "reference/guide", anchor: "install" },
  { declaration: "figures/broken.json", route: "reference/broken" },
];

async function mixedFixture() {
  const root = await selfContainedPortalFixture();
  await rm(path.join(root, "docs/seed.md"));
  await mkdir(path.join(root, "docs/decisions"));
  const sources = {
    "docs/product.md": COMPOSED_PAGE,
    "docs/plain.md": SHELL_PAGE,
    "docs/guide.md": GUIDE,
    "docs/bare.md": BARE,
    "docs/broken.md": BROKEN,
    "docs/decisions/ADR-0001-first.md": ADR,
    "policy.json": POLICY,
  };
  for (const [relative, contents] of Object.entries(sources)) await writeFile(path.join(root, relative), contents);
  await writeFigureInputs(root);
  const limits = await specimen("10-extent-derived.json");
  await writeFile(path.join(root, "figures/commit-limits.json"), `${JSON.stringify(limits, null, 2)}\n`);
  const steps = await specimen("04-sequence.json");
  steps.figure.id = "install-steps";
  steps.figure.facts = [{ claim: "The install section lists two steps", source: "docs/guide.md#install", derive: "numbered items in the Install section", check: { kind: "count-items" }, value: 2 }];
  await writeFile(path.join(root, "figures/install-steps.json"), `${JSON.stringify(steps, null, 2)}\n`);
  // The flow specimen draws done and stop apart by shape alone, so it breaks
  // rule 3 on any page it lands on.
  const broken = await specimen("01-flow.json");
  broken.figure.facts = [{ claim: "The source says a rule is broken", source: "docs/broken.md", derive: "the source text", check: { kind: "contains", text: "breaks a rule" }, value: true }];
  await writeFile(path.join(root, "figures/broken.json"), `${JSON.stringify(broken, null, 2)}\n`);
  await configureFixture(root, { layers: LAYERS, page_carriers: [], page_classes: PAGE_CLASSES, figures: FIGURES });
  commitFixture(root, "mixed figure fixture");
  return root;
}

async function reconfigure(root, overrides, message) {
  await configureFixture(root, overrides);
  commitFixture(root, message);
}

// Serves the built site the way the preview does, so the figure sheet, the
// container queries and the theme tokens all load.
async function serve(directory) {
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, "http://127.0.0.1");
    let file = path.join(directory, decodeURIComponent(url.pathname));
    try { if ((await stat(file)).isDirectory()) file = path.join(file, "index.html"); } catch { /* not found below */ }
    try {
      const body = await readFile(file);
      const type = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".svg": "image/svg+xml", ".json": "application/json" }[path.extname(file)] ?? "application/octet-stream";
      response.writeHead(200, { "content-type": type });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((resolve) => server.close(resolve)) };
}

test("the adapter refuses a figure binding it can prove wrong from committed inputs", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const root = await mixedFixture();
  try {
    const refusals = [
      [{ page_classes: [...PAGE_CLASSES.slice(0, 3), { prefix: "docs/decisions", class: "pass-through" }] }, /pass-through docs\/decisions needs a reason from accepted-record, governance, no-relationship/],
      [{ page_classes: [...PAGE_CLASSES.slice(0, 3), { prefix: "docs/decisions", class: "pass-through", reason: "tidy" }] }, /pass-through docs\/decisions needs a reason/],
      [{ page_classes: [...PAGE_CLASSES.slice(0, 3), { prefix: "docs/decisions", class: "pass-through", reason: "no-relationship" }] }, /page_classes docs\/decisions note/],
      [{ figures: [...FIGURES, { declaration: "figures/commit-limits.json", route: "reference/nowhere" }] }, /bound to reference\/nowhere, which is not a published route/],
      [{ figures: [...FIGURES.slice(0, -1), { declaration: "figures/broken.json", route: "reference/guide", anchor: "uninstall" }] }, /anchor #uninstall names no heading in docs\/guide\.md/],
      [{ figures: [...FIGURES, { declaration: "figures/broken.json", route: "system/decisions/ADR-0001-first" }] }, /a pass-through page carries no figure/],
      [{ figures: [...FIGURES, { declaration: "figures/broken.json", route: "reference/plain" }] }, /an explanatory page binds a figure to an altitude panel/],
      [{ figures: [...FIGURES, { declaration: "figures/concept.json", route: "reference/bare", panel: "concept" }] }, /an illustrated source binds a figure to its head or a section anchor, not a panel/],
    ];
    for (const [overrides, expected] of refusals) {
      await reconfigure(root, overrides, "a binding the adapter refuses");
      const result = runLocalAdapter(root, false);
      assert.match(result.stderr, expected);
      await reconfigure(root, { page_classes: PAGE_CLASSES, figures: FIGURES }, "restore the fixture");
    }

    // The anchor exists and the fact is wrong: the fidelity rule names it.
    const steps = JSON.parse(await readFile(path.join(root, "figures/install-steps.json"), "utf8"));
    steps.figure.facts[0].value = 3;
    await writeFile(path.join(root, "figures/install-steps.json"), `${JSON.stringify(steps, null, 2)}\n`);
    commitFixture(root, "a wrong fact under a valid anchor");
    assert.match(runLocalAdapter(root, false).stderr, /figures\/install-steps\.json: rule 6 \(fidelity\): fact "The install section lists two steps" draws 3 but docs\/guide\.md#install gives 2/);
    steps.figure.facts[0].value = 2;
    await writeFile(path.join(root, "figures/install-steps.json"), `${JSON.stringify(steps, null, 2)}\n`);
    commitFixture(root, "restore the fact");

    // Declarations and fact sources are committed inputs: an uncommitted edit
    // to either refuses the build.
    await writeFile(path.join(root, FIGURE_FACTS_PATH), "# Figure facts\n\nedited\n");
    assert.match(runLocalAdapter(root, false).stderr, /figures\/facts\.md/);
    commitFixture(root, "commit the edited facts");
    assert.match(runLocalAdapter(root, false).stderr, /rule 6 \(fidelity\)/);

    // A source that spoofs the markers the validator reads is refused.
    await writeFile(path.join(root, FIGURE_FACTS_PATH), (await import("./page-shapes.mjs")).FIGURE_FACTS);
    await writeFile(path.join(root, "docs/guide.md"), `${GUIDE}\n<!-- codeflow-companion-end -->\n`);
    commitFixture(root, "a source that spoofs a companion marker");
    assert.match(runLocalAdapter(root, false).stderr, /an as-is source may not carry a codeflow marker comment/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("the mixed fixture renders every class and the gates name only what falls short", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const root = await mixedFixture();
  try {
    const adapted = runLocalAdapter(root);
    assert.match(adapted.stdout, /portal: page classes explanatory 2, illustrated 3, pass-through 1, derived-lookup 0; 6 bound figures from 6 declarations/);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    const byRoute = new Map(evidence.pages.map((page) => [page.route, page]));

    // Every route carries its class, reason and bound figures.
    assert.deepEqual([...evidence.pages].sort((a, b) => (a.route < b.route ? -1 : 1)).map((page) => [page.route, page.page_class, page.class_reason, page.figures.map((figure) => figure.placement)]), [
      ["orient/product", "explanatory", null, ["panel", "panel", "panel"]],
      ["reference/bare", "illustrated", null, []],
      ["reference/broken", "illustrated", null, ["head"]],
      ["reference/guide", "illustrated", null, ["head", "anchor"]],
      ["reference/plain", "explanatory", null, []],
      ["system/decisions/ADR-0001-first", "pass-through", "accepted-record", []],
    ]);
    const limits = evidence.figures.find((figure) => figure.declaration_path === "figures/commit-limits.json");
    assert.deepEqual(limits.derived, {
      source_path: "policy.json", source_sha256: sha256(POLICY), select: "git",
      values: { commit_desc_max_len: 50, commit_subject_max_len: 72 },
      drawn: { commit_desc_max_len: 50, commit_subject_max_len: 72 },
    });

    // The unchanged source: strip the recorded insertions from the rendered
    // region and the committed body after its title comes back byte for byte.
    const guide = byRoute.get("reference/guide");
    const rendered = await readFile(path.join(root, guide.output_markdown));
    const region = guide.source_region;
    let bytes = rendered.subarray(region.output_offset_bytes, region.output_offset_bytes + region.region_bytes);
    assert.equal(sha256(bytes), region.region_sha256);
    for (const insert of [...region.inserts].reverse()) {
      const block = bytes.subarray(insert.offset_bytes, insert.offset_bytes + insert.block_bytes);
      assert.equal(sha256(block), insert.block_sha256);
      assert.match(block.toString("utf8"), new RegExp(`Figure declared in <code>${insert.declaration_path.replace(".", "\\.")}</code>, not part of the page source\\.`));
      bytes = Buffer.concat([bytes.subarray(0, insert.offset_bytes), bytes.subarray(insert.offset_bytes + insert.block_bytes)]);
    }
    assert.equal(bytes.toString("utf8"), Buffer.from(GUIDE).subarray(region.source_start_bytes).toString("utf8"));
    assert.equal(Buffer.from(GUIDE).subarray(0, region.source_start_bytes).toString("utf8"), "# Install guide\n\n");
    // The twin carries the same bytes, so it attributes the figure the same way.
    assert.deepEqual(await readFile(path.join(root, guide.markdown_twin)), rendered);

    buildFixture(root);
    const html = await readFile(path.join(root, "dist/reference/guide/index.html"), "utf8");
    assert.match(html, /&lt;div class=(?:"|&quot;)raw(?:"|&quot;)&gt;Raw markup stays text\.&lt;\/div&gt;/);
    assert.match(html, /<a href="\/orient\/product\/">the product page<\/a>/);

    const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
    const assignments = classifyPortalPages(config, evidence.pages);
    assert.deepEqual(declaredCarrierFailures(config, assignments), []);
    const site = await serve(path.join(root, "dist"));
    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage();
      const visitRoute = async (route) => { await page.goto(`${site.origin}/${route}/`, { waitUntil: "networkidle" }); };
      const observations = [];
      for (const assignment of assignments) {
        await visitRoute(assignment.route);
        observations.push({ route: assignment.route, ...await observePortalPage(page) });
      }
      // The composed page, the illustrated guide with its figures, the
      // pass-through record and the broken figure's page all carry their
      // class; the bare illustrated source and the plain page do not.
      assert.deepEqual(pageClassFailures(assignments, observations), [
        "docs/bare.md (illustrated source at reference/bare) lacks a companion figure at the page head: no figure drawn above the source",
        "docs/plain.md (explanatory page at reference/plain) lacks the altitude trio concept, architecture, technical: missing concept, architecture, technical; present none",
      ]);

      const { failures, drawn } = await figureGateFailures(page, visitRoute, assignments, evidence);
      assert.equal(drawn, 6);
      assert.ok(failures.length > 0);
      for (const failure of failures) assert.match(failure, /^docs\/broken\.md \(at reference\/broken, page head, figures\/broken\.json\): rule \d+ /);
      assert.ok(failures.some((failure) => /rule 3 \(two channels, never hue alone\): wide: states done and stop differ on shape, need 2/.test(failure)), failures.join("\n"));
    } finally {
      await browser.close();
      await site.close();
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

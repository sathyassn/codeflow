// The figure gate on a real build: one fixture repository whose pages cover
// every page class and every way a figure binding can go wrong. The adapter
// refuses what it can prove wrong from committed inputs; the built site is
// then read in a browser, where the class rules and the twelve figure rules
// are applied to what actually renders.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { chromium } from "@playwright/test";
import { sha256 } from "../scripts/lib.mjs";
import { figureGateFailures, observePortalPage, pageCssFailures, pinnedBuiltSheets, pinnedDeclarations, pinnedKitSheets, pinnedRuntimeScripts } from "../scripts/browser-verify.mjs";
import { REGENERATE, builtRuntimeScripts } from "../scripts/runtime-scripts.mjs";
import { drawnValuesMatch } from "../scripts/figure-grammar.mjs";
import { GitSnapshot } from "../scripts/git-snapshot.mjs";
import { classifyPortalPages, declaredCarrierFailures, pageClassFailures } from "../scripts/page-classes.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { COMPOSED_PAGE, FIGURE_FACTS_PATH, SHELL_PAGE, panelBindings, specimen, writeFigureInputs } from "./page-shapes.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture, starterRoot } from "./portal-fixture.mjs";

const GUIDE = [
  "# Install guide", "",
  "Read [the product page](product.md) before installing.", "",
  "<div class=\"raw\">Raw markup stays text.</div>", "",
  "## Install", "",
  "1. Run the installer.",
  "2. Check the result.", "",
  "## Verify", "",
  "Run the check and read its report.", "",
  "```sh",
  "codeflow validate --portal portal",
  "```", "",
].join("\n");
// A fenced code block renders through Expressive Code, which writes its own
// sheet link, module script and token custom properties into the content.
const CODE_FENCE = "```sh\ncodeflow validate --portal portal\n```\n\n";
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
  { declaration: "figures/commit-limits.json", route: "orient/product", panel: "technical", anchor: "limits" },
  { declaration: "figures/commit-limits.json", route: "reference/guide" },
  { declaration: "figures/install-steps.json", route: "reference/guide", anchor: "install" },
  { declaration: "figures/broken.json", route: "reference/broken" },
];

async function mixedFixture() {
  const root = await selfContainedPortalFixture();
  await rm(path.join(root, "docs/seed.md"));
  await mkdir(path.join(root, "docs/decisions"));
  const sources = {
    "docs/product.md": `${COMPOSED_PAGE.replace("The controls, then", `${CODE_FENCE}The controls, then`)}\n### Limits\n\nThe commit limits a message is held to.\n`,
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
  // The layering specimen with its remote plane declared on the local layer
  // mark: only the cap tells the two planes apart, so it breaks rule 3 on any
  // page it lands on.
  const broken = await specimen("controls/one-channel.json");
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

// `browser:verify` awaits its entry point at the top level, so a module-level
// binding declared below it is read before initialization. The first consumer
// build with bound figures failed that way; the entry point must come last.
test("the browser gate's entry point runs after every module-level binding", async () => {
  const source = await readFile(path.join(starterRoot, "scripts/browser-verify.mjs"), "utf8");
  const lines = source.split("\n");
  const entry = lines.findIndex((line) => line.startsWith("if (process.argv[1] && "));
  assert.notEqual(entry, -1, "browser-verify.mjs has an entry point");
  const later = lines.slice(entry).filter((line) => /^(export )?(const|let|var|class) /.test(line));
  assert.deepEqual(later, [], "module-level bindings declared after the entry point");
});

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
      [{ figures: [...FIGURES, { declaration: "figures/broken.json", route: "orient/product", anchor: "limits" }] }, /an explanatory page binds a figure to an altitude panel/],
      [{ figures: [...FIGURES, { declaration: "figures/broken.json", route: "orient/product", panel: "concept", anchor: "limits" }] }, /anchor #limits is not a section inside the concept panel/],
      [{ figures: [...FIGURES, { declaration: "figures/broken.json", route: "orient/product", panel: "technical", anchor: "technical" }] }, /anchor #technical is not a section inside the technical panel/],
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

test("the mixed fixture renders every class and the gates name only what falls short", { skip: process.platform === "win32", timeout: 900_000 }, async () => {
  const root = await mixedFixture();
  try {
    const adapted = runLocalAdapter(root);
    assert.match(adapted.stdout, /portal: page classes explanatory 2, illustrated 3, pass-through 1, derived-lookup 0; 7 bound figures from 6 declarations/);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    const byRoute = new Map(evidence.pages.map((page) => [page.route, page]));

    // Every route carries its class, reason and bound figures.
    assert.deepEqual([...evidence.pages].sort((a, b) => (a.route < b.route ? -1 : 1)).map((page) => [page.route, page.page_class, page.class_reason, page.figures.map((figure) => figure.placement)]), [
      ["orient/product", "explanatory", null, ["panel", "panel", "panel", "anchor"]],
      ["reference/bare", "illustrated", null, []],
      ["reference/broken", "illustrated", null, ["head"]],
      ["reference/guide", "illustrated", null, ["head", "anchor"]],
      ["reference/plain", "explanatory", null, []],
      ["system/decisions/ADR-0001-first", "pass-through", "accepted-record", []],
    ]);
    const limits = evidence.figures.find((figure) => figure.declaration_path === "figures/commit-limits.json");
    // The drawn values are read back off the drawing grid, so they match the
    // derived values within the grid's rounding, the bound rule 6 applies.
    const { drawn: drawnLimits, ...derivedLimits } = limits.derived;
    assert.deepEqual(derivedLimits, {
      source_path: "policy.json", source_sha256: sha256(POLICY), select: "git",
      values: { commit_desc_max_len: 50, commit_subject_max_len: 72 },
    });
    assert.ok(drawnValuesMatch(drawnLimits, derivedLimits.values), JSON.stringify(drawnLimits));

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
    // Record the built artifacts, as the workflow does: the stylesheets a
    // page may load are the CSS files among them.
    const recorded = spawnSync(process.execPath, ["scripts/evidence.mjs"], { cwd: root, encoding: "utf8" });
    assert.equal(recorded.status, 0, recorded.stderr);
    const built = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.ok(built.artifacts.some((artifact) => artifact.path.endsWith(".css")));
    const html = await readFile(path.join(root, "dist/reference/guide/index.html"), "utf8");
    assert.match(html, /&lt;div class=(?:"|&quot;)raw(?:"|&quot;)&gt;Raw markup stays text\.&lt;\/div&gt;/);
    assert.match(html, /<a href="\/orient\/product\/">the product page<\/a>/);
    // Both the illustrated guide and the explanatory product page carry a
    // code block as Expressive Code writes it.
    for (const route of ["reference/guide", "orient/product"]) {
      const builtHtml = await readFile(path.join(root, `dist/${route}/index.html`), "utf8");
      assert.match(builtHtml, /<div class="expressive-code"><link rel="stylesheet" href="\/_astro\/ec\.[\w-]+\.css"><script type="module" src="\/_astro\/ec\.[\w-]+\.js"><\/script>/, route);
      assert.match(builtHtml, /<span style="--0:#[0-9A-F]{6};--1:#[0-9A-F]{6}">/, route);
    }

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

      const snapshot = new GitSnapshot(path.resolve(root, config.repository_root));
      const declarations = pinnedDeclarations(snapshot, evidence.repository.commit, evidence);
      assert.equal(declarations.size, 6);
      const misrecorded = structuredClone(evidence);
      misrecorded.figures[0].declaration_sha256 = "0".repeat(64);
      assert.throws(() => pinnedDeclarations(snapshot, evidence.repository.commit, misrecorded), new RegExp(`figure declaration ${misrecorded.figures[0].declaration_path} does not match its recorded hash`));
      const kitSheets = pinnedKitSheets(snapshot, evidence.repository.commit, path.relative(path.resolve(root, config.repository_root), root));
      const inlineScripts = pinnedRuntimeScripts(snapshot, evidence.repository.commit, path.relative(path.resolve(root, config.repository_root), root), config.theme);
      // The committed list of the runtime's inline scripts is what a fresh
      // build of this lockfile emits; when either moves, regenerate it.
      const committedScripts = JSON.parse(await readFile(path.join(root, "scripts/runtime-scripts.json"), "utf8"));
      assert.deepEqual(await builtRuntimeScripts(path.join(root, "dist"), config.theme, committedScripts), committedScripts.scripts, `scripts/runtime-scripts.json is stale against a fresh build; run: ${REGENERATE}`);
      assert.equal(committedScripts.lock_sha256, sha256(await readFile(path.join(root, "package-lock.json"))), `scripts/runtime-scripts.json was built from another lockfile; run: ${REGENERATE}`);
      // The clean control: the real build, with the site's own sheets and
      // chrome styles, fails only the figure built to break a rule.
      const { failures, drawn } = await figureGateFailures(page, visitRoute, assignments, built, declarations, kitSheets, inlineScripts);
      assert.equal(drawn, 7);
      assert.ok(failures.length > 0);
      for (const failure of failures) assert.match(failure, /^docs\/broken\.md \(at reference\/broken, page head, figures\/broken\.json\): rule \d+ /);
      assert.deepEqual(failures.filter((failure) => /served page|page CSS|executable content|clean copy|not visible to a reader|effective opacity|legend key sits|twin marker/.test(failure)), []);
      assert.ok(failures.some((failure) => /rule 3 \(two channels, never hue alone\): wide: states layer and layer-remote differ on overlay, need 2/.test(failure)), failures.join("\n"));

      // Page CSS (R3-2, R4-1, R4-2). Each rule is refused at its source, as a
      // stylesheet that is not a built sheet, and is also seen by the checks
      // behind that: every computed property against a clean copy of the page,
      // the ancestors of each figure, and the figure's visibility.
      const guideOnly = assignments.filter((assignment) => assignment.route === "reference/guide");
      const hostile = async (change) => (await figureGateFailures(page, async (route) => { await visitRoute(route); await page.evaluate(change.inject, change.css); }, guideOnly, built, declarations, kitSheets, inlineScripts)).failures;
      const addSheet = (css) => document.head.append(Object.assign(document.createElement("style"), { textContent: css }));
      const onGuide = /^docs\/guide\.md \(at reference\/guide(?:, (?:page head|#[^,]+), figures\/(?:commit-limits|install-steps)\.json)?\): /;
      const cases = [
        [".cf-fig { opacity: 0; }", /<figure class="cf-fig[^"]*"> computes opacity 0 where the clean copy computes 1/, /the figure is not visible to a reader/],
        [".cf-companion { opacity: 0; }", /<div class="cf-companion[^"]*"> computes opacity 0 where the clean copy computes 1/, /an ancestor <div class="cf-companion[^"]*"> has opacity 0 where the clean copy has 1/],
        [".cf-fig { clip-path: inset(50%); }", /<figure class="cf-fig[^"]*"> computes clip-path inset\(50%\) where the clean copy computes none/],
        [".cf-fig { filter: opacity(0); }", /<figure class="cf-fig[^"]*"> computes filter opacity\(0\) where the clean copy computes none/],
        [".cf-fig-caption { visibility: hidden; }", /<figcaption class="cf-fig-caption"> computes visibility hidden where the clean copy computes visible/, /the caption is not visible to a reader/],
        [".cf-fig { translate: 0 1000px; }", /<figure class="cf-fig[^"]*"> computes translate 0px 1000px where the clean copy computes none/],
        [".cf-fig-svg .cf-m-trans { stroke-dasharray: 0 100000; }", /computes stroke-dasharray 0px, 100000px where the clean copy computes none/],
        [".cf-m-used { translate: 0 1000px; }", /a drawn <rect> has translate 0px 1000px where the kit sheets alone give none/, /the row-0 mark spans y/],
      ];
      for (const [css, ...expected] of cases) {
        const failures = await hostile({ inject: addSheet, css });
        assert.ok(failures.some((failure) => onGuide.test(failure) && /page CSS: a stylesheet from a <style> element in the head is not a built sheet/.test(failure)), `${css}: ${failures.join("\n")}`);
        for (const pattern of expected) assert.ok(failures.some((failure) => onGuide.test(failure) && / rule 6 \(/.test(failure) && pattern.test(failure)), `${css} ${pattern}: ${failures.join("\n")}`);
      }
      // Starlight's content rules reach into a companion that does not opt
      // out of them: the list rule pushes each legend key 10px above its
      // label and the details rule paints the twin marker as a dot. The clean
      // copy carries the same site styles, so only the absolute reading of
      // the legend and the marker sees it, at every width and mode.
      const opted = await hostile({ inject: () => { for (const companion of document.querySelectorAll(".cf-companion")) companion.classList.remove("not-content"); }, css: null });
      for (const [width, mode] of [[1440, "light"], [390, "light"], [1440, "dark"], [390, "dark"]]) {
        const where = `docs/guide.md (at reference/guide, ${width}px ${mode}): figure install-steps: `;
        assert.ok(opted.some((failure) => failure.startsWith(where) && /the [a-z-]+ legend key sits \d+(?:\.\d)?px above its label/.test(failure)), `${width} ${mode}: ${opted.join("\n")}`);
        assert.ok(opted.some((failure) => failure.startsWith(where) && /the twin marker is not the figure sheet's chevron/.test(failure)), `${width} ${mode}: ${opted.join("\n")}`);
      }

      // A style attribute on the content, the figure's ancestor, is refused
      // at its source and seen on the ancestor chain.
      const attributed = await hostile({ inject: () => document.querySelector(".sl-markdown-content").setAttribute("style", "opacity: 0.5"), css: null });
      assert.ok(attributed.some((failure) => /page CSS: a style attribute on <div class="sl-markdown-content[^"]*"> carries CSS the site's sheets do not/.test(failure)), attributed.join("\n"));
      assert.ok(attributed.some((failure) => /an ancestor <div class="sl-markdown-content[^"]*"> has opacity 0\.5 where the clean copy has 1/.test(failure)), attributed.join("\n"));

      // Executable content (R5-1). The review's CSSOM insertion leaves the
      // served sheet unchanged, so only its rule list and the figure's own
      // visibility can show it; handlers and scripts in the content are
      // refused where they stand.
      const insertRule = () => { const sheet = [...document.styleSheets].find((candidate) => candidate.media.mediaText !== "print"); sheet.insertRule(".cf-fig {opacity:0}", sheet.cssRules.length); };
      const inserted = await hostile({ inject: insertRule, css: null });
      assert.ok(inserted.some((failure) => /page CSS: the stylesheet \/_astro\/common\.[^ ]+\.css holds \d+ rules that are not the \d+ its served bytes parse to/.test(failure)), inserted.join("\n"));
      assert.ok(inserted.some((failure) => /rule 6 .*: wide: the figure is not visible to a reader/.test(failure)), inserted.join("\n"));
      const handler = await hostile({ inject: () => document.querySelector(".sl-markdown-content p").setAttribute("onclick", "void 0"), css: null });
      assert.ok(handler.some((failure) => /executable content: an event-handler attribute on <p>/.test(failure)), handler.join("\n"));
      const script = await hostile({ inject: () => document.querySelector(".sl-markdown-content").append(Object.assign(document.createElement("script"), { textContent: "void 0" })), css: null });
      assert.ok(script.some((failure) => /executable content: a <script> element in the page content/.test(failure)), script.join("\n"));

      // The same insertion written into the built page, where the clean copy
      // runs it too: the served page is not the recorded one, it carries a
      // script, and the figure must be visible whatever the copy shows. A
      // closed shadow root in the built page, beside the open-root control,
      // is read from the served bytes before the browser consumes it (R5-2).
      const builtGuide = path.join(root, "dist/reference/guide/index.html");
      const builtBytes = await readFile(builtGuide, "utf8");
      const intoContent = (markup) => builtBytes.replace(/(<div class="sl-markdown-content"[^>]*>)/, `$1${markup}`);
      const onDisk = async (markup) => {
        await writeFile(builtGuide, intoContent(markup));
        try { return (await figureGateFailures(page, visitRoute, guideOnly, built, declarations, kitSheets, inlineScripts)).failures; } finally { await writeFile(builtGuide, builtBytes); }
      };
      const rerun = await onDisk("<script>{ const sheet = [...document.styleSheets].find((candidate) => candidate.media.mediaText !== \"print\"); sheet.insertRule(\".cf-fig {opacity:0}\", sheet.cssRules.length); }</script>");
      assert.ok(rerun.some((failure) => /served page: dist\/reference\/guide\/index\.html is served with sha256 \w+, not the recorded \w+/.test(failure)), rerun.join("\n"));
      assert.ok(rerun.some((failure) => /executable content: a <script> element in the page content/.test(failure)), rerun.join("\n"));
      assert.ok(rerun.some((failure) => /holds \d+ rules that are not the \d+ its served bytes parse to/.test(failure)), rerun.join("\n"));
      assert.ok(rerun.some((failure) => /rule 6 .*: wide: the figure is not visible to a reader/.test(failure)), rerun.join("\n"));
      // The clean copy runs only the runtime's scripts, so it does not repeat
      // the page's insertion and the figure differs from it.
      assert.ok(rerun.some((failure) => /rule 6 .*: wide: <figure class="cf-fig[^"]*"> computes opacity 0 where the clean copy computes 1/.test(failure)), rerun.join("\n"));

      // Inline scripts outside the content are the runtime's own: an extra
      // one in the page chrome fails, and so does a changed Starlight script,
      // with the regeneration to run if the runtime really changed.
      const regenerate = `if the runtime changed, regenerate scripts/runtime-scripts.json: ${REGENERATE}`;
      const extra = await hostile({ inject: () => document.head.append(Object.assign(document.createElement("script"), { textContent: "void 0" })), css: null });
      assert.ok(extra.some((failure) => failure.includes("executable content: inline scripts outside the content are not ones the site's runtime emits (sha256 ") && failure.endsWith(regenerate)), extra.join("\n"));
      assert.ok(builtBytes.includes("window.StarlightThemeProvider = (() => {"));
      await writeFile(builtGuide, builtBytes.replace("window.StarlightThemeProvider = (() => {", "window.StarlightThemeProvider = (() => { "));
      let changed;
      try { changed = (await figureGateFailures(page, visitRoute, guideOnly, built, declarations, kitSheets, inlineScripts)).failures; } finally { await writeFile(builtGuide, builtBytes); }
      assert.ok(changed.some((failure) => failure.includes("executable content: inline scripts outside the content are not ones the site's runtime emits (sha256 ") && failure.endsWith(regenerate)), changed.join("\n"));
      for (const mode of ["closed", "open"]) {
        const shadowed = await onDisk(`<div><template shadowrootmode="${mode}"><style>:host{opacity:0}</style>Shadow</template></div>`);
        assert.ok(shadowed.some((failure) => /served page: the served page declares a shadow root/.test(failure)), `${mode}: ${shadowed.join("\n")}`);
        assert.ok(shadowed.some((failure) => /served page: dist\/reference\/guide\/index\.html is served with sha256/.test(failure)), `${mode}: ${shadowed.join("\n")}`);
      }

      // Code blocks. The clean control above holds the real build, whose
      // blocks link their recorded sheet and script and carry token custom
      // properties. A real property on a token, a custom property outside a
      // block's frame, a style element, and a figure inside a block, real or
      // imitated, are refused; an imitated block holding only the properties
      // Expressive Code writes is harmless and passes.
      const pageCss = (failures) => failures.filter((failure) => /page CSS|executable content/.test(failure));
      // The block's sheet reaches only blocks: each rule that styles an
      // element is scoped to .expressive-code, the rest declare only custom
      // properties, and no other built sheet reads those properties.
      const codeSheet = built.artifacts.find((artifact) => /^dist\/_astro\/ec\.[\w-]+\.css$/.test(artifact.path)).path;
      const codeScript = built.artifacts.find((artifact) => /^dist\/_astro\/ec\.[\w-]+\.js$/.test(artifact.path)).path;
      const reach = await page.evaluate((text) => {
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(text);
        const parts = (selector) => { const found = []; let depth = 0; let current = ""; for (const character of selector) { if (character === "(") depth += 1; if (character === ")") depth -= 1; if (character === "," && depth === 0) { found.push(current.trim()); current = ""; } else current += character; } return [...found, current.trim()]; };
        const outside = [];
        const walk = (rules) => { for (const rule of rules) {
          if (rule instanceof CSSStyleRule) {
            const scoped = parts(rule.selectorText).every((part) => /(?:^|[\s>+~])(?:[\w-]*|\*)\.expressive-code(?![\w-])/.test(part.replace(/:(?:not|is|where|has)\([^)]*\)/g, "")));
            if (!scoped && ![...rule.style].every((name) => name.startsWith("--"))) outside.push(rule.selectorText);
          } else if (rule.cssRules) walk(rule.cssRules);
        } };
        walk(sheet.cssRules);
        return outside;
      }, await readFile(path.join(root, codeSheet), "utf8"));
      assert.deepEqual(reach, []);
      for (const other of built.artifacts.filter((artifact) => artifact.path.endsWith(".css") && artifact.path !== codeSheet)) {
        assert.doesNotMatch(await readFile(path.join(root, other.path), "utf8"), /var\(\s*--ec-/, other.path);
      }
      assert.doesNotMatch(kitSheets, /--ec-/);
      for (const css of ["color: red", "opacity: 0", "transform: scale(2)", "display: none", "background: url(/x.png)", "--0:#82AAFF;color:red", "--0:url(/x.png)"]) {
        const failures = await hostile({ inject: (style) => document.querySelector(".expressive-code pre span[style]").setAttribute("style", style), css });
        assert.ok(failures.some((failure) => /page CSS: a style attribute on <span> carries CSS the site's sheets do not/.test(failure)), `${css}: ${failures.join("\n")}`);
      }
      const outside = await hostile({ inject: () => [...document.querySelectorAll(".sl-markdown-content p")].find((paragraph) => !paragraph.closest(".cf-companion")).setAttribute("style", "--0:#82AAFF"), css: null });
      assert.ok(outside.some((failure) => /page CSS: a style attribute on <p> carries CSS the site's sheets do not/.test(failure)), outside.join("\n"));
      const blockStyle = await hostile({ inject: () => document.querySelector(".expressive-code").append(Object.assign(document.createElement("style"), { textContent: ".cf-fig{opacity:0}" })), css: null });
      assert.ok(blockStyle.some((failure) => /page CSS: a <style> element in the page content/.test(failure)), blockStyle.join("\n"));
      const wrapped = await hostile({ inject: () => { const companion = document.querySelector(".cf-companion"); const block = Object.assign(document.createElement("div"), { className: "expressive-code" }); companion.replaceWith(block); block.append(companion); }, css: null });
      assert.ok(wrapped.some((failure) => /page CSS: a figure or companion inside a code block/.test(failure)), wrapped.join("\n"));
      // Each carrier on an element is judged on its own: an allowed token
      // attribute does not excuse a style element or a link beside it in the
      // frame (Codex CB-1, Grok F2). The sheet scopes on the expressive-code
      // class whatever the tag, so a figure or companion on or inside any
      // element with that class fails (Codex CB-2, Grok F1). The real page,
      // a code block beside its companions, is the positive control.
      const sheets = pinnedBuiltSheets(built.artifacts);
      const cssAfter = async (inject) => { await visitRoute("reference/guide"); await page.evaluate(inject); return pageCssFailures(page, sheets); };
      assert.deepEqual(await cssAfter("void 0"), []);
      for (const [markup, pattern] of [
        ["<style style=\"--0:#82AAFF\">.cf-fig{opacity:0}</style>", /^a <style> element in the page content carries CSS/],
        ["<link style=\"--0:#82AAFF\" rel=\"stylesheet\" href=\"/evil.css\">", /^a <link> element in the page content carries CSS/],
      ]) {
        const failures = await cssAfter(`document.querySelector(".expressive-code pre").insertAdjacentHTML("beforeend", ${JSON.stringify(markup)})`);
        assert.ok(failures.some((failure) => pattern.test(failure)), `${markup}: ${failures.join("\n")}`);
      }
      const wrap = (tag) => `{ const companion = document.querySelector(".cf-companion"); const wrapper = document.createElement("${tag}"); wrapper.className = "note expressive-code"; companion.replaceWith(wrapper); wrapper.append(companion); }`;
      for (const inject of [...["div", "section", "article", "span", "aside"].map(wrap), "document.querySelector('.cf-companion').classList.add('expressive-code')", "document.querySelector('figure.cf-fig').classList.add('expressive-code')", "document.querySelector('.cf-legend').classList.add('expressive-code')"]) {
        const failures = await cssAfter(inject);
        assert.ok(failures.includes("a figure or companion inside a code block carries CSS the site's sheets do not"), `${inject}: ${failures.join("\n")}`);
      }
      const imitated = await hostile({ inject: () => [...document.querySelectorAll(".sl-markdown-content p")].find((paragraph) => !paragraph.closest(".cf-companion")).insertAdjacentHTML("afterend", "<div class=\"expressive-code\"><figure><pre><span style=\"--0:#FFFFFF;--0fw:bold\">imitated</span></pre></figure></div>"), css: null });
      assert.deepEqual(pageCss(imitated), [], imitated.join("\n"));

      // An Expressive Code link or script to an asset the evidence does not
      // record, or a recorded asset served with other bytes, fails.
      const astro = path.join(root, "dist/_astro");
      await writeFile(path.join(astro, "ec.zzzzz.css"), ".expressive-code{}\n");
      await writeFile(path.join(astro, "ec.zzzzz.js"), "void 0;\n");
      try {
        const link = await hostile({ inject: () => document.querySelector(".expressive-code").prepend(Object.assign(document.createElement("link"), { rel: "stylesheet", href: "/_astro/ec.zzzzz.css" })), css: null });
        assert.ok(link.some((failure) => /page CSS: a <link> element in the page content carries CSS the site's sheets do not/.test(failure)), link.join("\n"));
        const script = await hostile({ inject: () => { const element = document.createElement("script"); element.type = "module"; element.src = "/_astro/ec.zzzzz.js"; document.querySelector(".expressive-code").prepend(element); }, css: null });
        assert.ok(script.some((failure) => /executable content: a <script> element in the page content/.test(failure)), script.join("\n"));
        assert.ok(script.some((failure) => /executable content: the script \/_astro\/ec\.zzzzz\.js is not a built script the evidence records/.test(failure)), script.join("\n"));
      } finally {
        await rm(path.join(astro, "ec.zzzzz.css"));
        await rm(path.join(astro, "ec.zzzzz.js"));
      }
      for (const [asset, pattern] of [[codeSheet, /page CSS: the stylesheet \/_astro\/ec\.[\w-]+\.css is served with sha256 \w+, not the recorded \w+/], [codeScript, /executable content: the script \/_astro\/ec\.[\w-]+\.js is served with sha256 \w+, not the recorded \w+/]]) {
        const original = await readFile(path.join(root, asset));
        await writeFile(path.join(root, asset), Buffer.concat([original, Buffer.from("\n/* edited */\n")]));
        let failures;
        try { failures = (await figureGateFailures(page, visitRoute, guideOnly, built, declarations, kitSheets, inlineScripts)).failures; } finally { await writeFile(path.join(root, asset), original); }
        assert.ok(failures.some((failure) => pattern.test(failure)), `${asset}: ${failures.join("\n")}`);
      }
    } finally {
      await browser.close();
      await site.close();
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

// The Rust validator reads what the adapter wrote. With the portal in a
// folder of its repository, as consumers adopt it, `validate --portal`
// accepts the illustrated page with its inserted figure and the explanatory
// page with its panel figure, and refuses a route once its body no longer
// equals the committed source or a companion no longer draws what its pinned
// declaration draws, even with every recorded hash rewritten to match. Runs
// where this checkout has built the codeflow binary.
const codeflowBinary = path.join(starterRoot, "..", "target", "debug", process.platform === "win32" ? "codeflow.exe" : "codeflow");
test("validate --portal accepts the inserted figures and refuses a tampered source or companion", { skip: process.platform === "win32", timeout: 300_000 }, async (context) => {
  if (!(await stat(codeflowBinary).then(() => true, () => false))) { context.skip("the codeflow binary is not built in this checkout"); return; }
  const root = await mkdtemp(path.join(starterRoot, ".portal-test-runtime-"));
  try {
    const portal = path.join(root, "portal");
    await mkdir(portal);
    for (const item of [".gitignore", ".node-version", "astro.config.mjs", "package.json", "package-lock.json", "portal.config.json", "scripts", "src", "public", "tsconfig.json"]) {
      await cp(path.join(starterRoot, item), path.join(portal, item), { recursive: true });
    }
    await mkdir(path.join(root, ".codeflow"));
    await mkdir(path.join(root, "docs"));
    await mkdir(path.join(root, "figures"));
    await writeFile(path.join(root, ".codeflow/project.toml"), "schema_version = 1\n");
    await writeFile(path.join(root, "docs/guide.md"), GUIDE);
    await writeFile(path.join(root, "docs/product.md"), "# Product\n\n## Concept\n\nAn explanatory page with one panel figure.\n");
    const steps = await specimen("04-sequence.json");
    steps.figure.id = "install-steps";
    steps.figure.facts = [{ claim: "The install section lists two steps", source: "docs/guide.md#install", derive: "numbered items in the Install section", check: { kind: "count-items" }, value: 2 }];
    await writeFile(path.join(root, "figures/install-steps.json"), `${JSON.stringify(steps, null, 2)}\n`);
    const concept = await specimen("03-layering.json");
    concept.figure.facts = [{ claim: "The product page has a concept panel", source: "docs/product.md#concept", derive: "the Concept section", check: { kind: "contains", text: "one panel figure" }, value: true }];
    await writeFile(path.join(root, "figures/concept.json"), `${JSON.stringify(concept, null, 2)}\n`);
    const config = JSON.parse(await readFile(path.join(portal, "portal.config.json"), "utf8"));
    Object.assign(config, {
      repository_root: "..", source_roots: ["docs"], exclude: [], primitive_tokens: null, repository_url: null, release_version: null,
      records: { enabled: false, layer: null, pointers: [] }, layers: LAYERS, base: "/", page_carriers: [],
      page_classes: [{ source: "docs/guide.md", class: "illustrated" }],
      figures: [{ declaration: "figures/install-steps.json", route: "reference/guide" }, { declaration: "figures/concept.json", route: "orient/product", panel: "concept" }],
    });
    const configText = `${JSON.stringify(config, null, 2)}\n`;
    await writeFile(path.join(portal, "portal.config.json"), configText);
    const { GENERATOR } = await import("../scripts/generator.mjs");
    await writeFile(path.join(root, ".codeflow/docs-portal.json"), `${JSON.stringify({
      schema_version: 2, root: "portal", starter_version: GENERATOR.version, runtime_ownership: "managed", generator: GENERATOR,
      files: { "portal.config.json": { ownership: "user-owned", pristine_sha256: sha256(configText) } },
    }, null, 2)}\n`);
    for (const args of [["init", "-q"], ["config", "user.email", "portal-tests@codeflow.invalid"], ["config", "user.name", "CodeFlow portal tests"], ["add", "-A"], ["commit", "-q", "-m", "nested portal fixture"]]) {
      const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
      assert.equal(result.status, 0, result.stderr);
    }
    // The build steps the workflow runs, with the dependencies of this
    // checkout: adapt, build the site, record the built artifacts.
    runLocalAdapter(portal);
    buildFixture(portal);
    const recorded = spawnSync(process.execPath, ["scripts/evidence.mjs"], { cwd: portal, encoding: "utf8" });
    assert.equal(recorded.status, 0, recorded.stderr);
    const validate = () => spawnSync(codeflowBinary, ["validate", "--portal", "portal"], { cwd: root, encoding: "utf8" });
    const accepted = validate();
    assert.match(accepted.stdout + accepted.stderr, /validate --portal: 2 page\(s\) clean/, accepted.stdout + accepted.stderr);

    // Rewrite one byte of the committed body inside the rendered region and
    // every hash that records it: only the source equality can still fail.
    const evidencePath = path.join(portal, ".portal/generated/evidence.json");
    const pristine = await readFile(evidencePath, "utf8");
    const evidence = JSON.parse(pristine);
    const guide = evidence.pages.find((page) => page.route === "reference/guide");
    const originals = new Map();
    for (const page of evidence.pages) originals.set(page.route, await readFile(path.join(portal, page.output_markdown)));
    const rendered = await readFile(path.join(portal, guide.output_markdown));
    const tampered = Buffer.from(rendered.toString("utf8").replace("2. Check the result.", "2. Chuck the result."));
    const region = guide.source_region;
    guide.source_region.region_sha256 = sha256(tampered.subarray(region.output_offset_bytes, region.output_offset_bytes + region.region_bytes));
    guide.output_markdown_sha256 = sha256(tampered);
    guide.markdown_twin_sha256 = sha256(tampered);
    await writeFile(path.join(portal, guide.output_markdown), tampered);
    await writeFile(path.join(portal, guide.markdown_twin), tampered);
    await writeFile(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
    const refused = validate();
    assert.notEqual(refused.status, 0);
    assert.match(refused.stdout + refused.stderr, /reference\/guide rendered source region does not equal the committed source: the page body no longer equals the committed source/);

    // Edit the visible caption inside each companion and rewrite every hash
    // the evidence records for it: only the reconstruction of the pinned
    // declaration can tell the companion no longer draws what it declares.
    // With `smuggle`, a comment carrying the pristine companion sits beside
    // the edited one: a comment renders nothing, so it proves nothing.
    const captionEdit = (text) => text.replace(/(<figcaption class="cf-fig-caption">)[^<]*/, "$1An edited caption.");
    const editCaption = async (route, rewrite = null, smuggle = false, edit = captionEdit) => {
      const record = JSON.parse(pristine);
      const page = record.pages.find((entry) => entry.route === route);
      const original = originals.get(route).toString("utf8");
      let edited = edit(original);
      if (smuggle) edited = edited.replace("<div class=\"cf-companion not-content\"", `<!-- ${original.match(/<div class="cf-companion not-content"[^\n]*/)[0]} -->\n\n<div class="cf-companion not-content"`);
      assert.notEqual(edited, original, route);
      rewrite?.(page, Buffer.byteLength(edited) - Buffer.byteLength(original), Buffer.from(edited));
      page.output_markdown_sha256 = sha256(edited);
      page.markdown_twin_sha256 = sha256(edited);
      for (const entry of record.pages) {
        const text = entry.route === route ? edited : originals.get(entry.route);
        await writeFile(path.join(portal, entry.output_markdown), text);
        await writeFile(path.join(portal, entry.markdown_twin), text);
      }
      await writeFile(evidencePath, `${JSON.stringify(record, null, 2)}\n`);
      const result = validate();
      assert.notEqual(result.status, 0, result.stdout);
      return result.stdout + result.stderr;
    };
    assert.match(await editCaption("reference/guide", (page, growth, bytes) => {
      const region = page.source_region;
      const insert = region.inserts[0];
      insert.block_bytes += growth;
      region.region_bytes += growth;
      const start = region.output_offset_bytes + insert.offset_bytes;
      insert.block_sha256 = sha256(bytes.subarray(start, start + insert.block_bytes));
      region.region_sha256 = sha256(bytes.subarray(region.output_offset_bytes, region.output_offset_bytes + region.region_bytes));
    }), /the companion figures\/install-steps\.json is not the recorded insertion at its bound place/);
    assert.match(await editCaption("orient/product"), /orient\/product does not render the figure its declaration figures\/concept\.json draws/);
    const smuggled = await editCaption("orient/product", null, true);
    assert.match(smuggled, /orient\/product renders a companion that no bound declaration draws in the concept panel/);
    assert.match(smuggled, /orient\/product does not render the figure its declaration figures\/concept\.json draws/);

    // Page content carries no CSS (R4-1, R4-2). A style block beside the
    // untouched companion, with every hash rewritten, fails on that alone;
    // so do a style attribute and a stylesheet link.
    const ownCss = /orient\/product renders page CSS, which only the site's own sheets may carry: /;
    for (const rule of [".cf-fig { opacity: 0; }", ".cf-companion { opacity: 0; }", ".cf-fig { clip-path: inset(50%); }", ".cf-fig { filter: opacity(0); }", ".cf-fig-caption { visibility: hidden; }", ".cf-fig { translate: 0 1000px; }", ".cf-fig-svg .cf-m-trans { stroke-dasharray: 0 100000; }"]) {
      const output = await editCaption("orient/product", null, false, (text) => text.replace("<div class=\"cf-companion not-content\"", `<style>${rule}</style>\n\n<div class="cf-companion not-content"`));
      assert.match(output, new RegExp(`${ownCss.source}a <style> element`), `${rule}: ${output}`);
      assert.doesNotMatch(output, /does not render the figure|renders a companion that no bound/, `${rule}: the companion itself is unchanged`);
    }
    assert.match(await editCaption("orient/product", null, false, (text) => text.replace("## Concept", "## Concept\n\n<div style=\"opacity: 0\">hidden</div>")), new RegExp(`${ownCss.source}a style attribute on <div>`));
    assert.match(await editCaption("orient/product", null, false, (text) => text.replace("## Concept", "## Concept\n\n<link rel=\"stylesheet\" href=\"/elsewhere.css\">")), new RegExp(`${ownCss.source}a <link> element`));

    // Nor executable content (R5-1): the review's CSSOM insertion from a
    // script, and an event handler, each fail beside the untouched companion.
    const ownScript = /orient\/product renders executable content, which only the site's runtime may carry: /;
    const insertion = "<script>\ndocument.styleSheets[0].insertRule(\n  \".cf-fig {opacity:0}\", document.styleSheets[0].cssRules.length\n);\n</script>";
    const scripted = await editCaption("orient/product", null, false, (text) => text.replace("<div class=\"cf-companion not-content\"", `${insertion}\n\n<div class="cf-companion not-content"`));
    assert.match(scripted, new RegExp(`${ownScript.source}a <script> element`), scripted);
    assert.doesNotMatch(scripted, /does not render the figure|renders a companion that no bound/, scripted);
    assert.match(await editCaption("orient/product", null, false, (text) => text.replace("## Concept", "## Concept\n\n<p onclick=\"void 0\">x</p>")), new RegExp(`${ownScript.source}an event-handler attribute on <p>`));

    // The built page is read too, before any template becomes a shadow root
    // (R5-2): a closed root, beside the open-root control, added only to the
    // built output with its recorded hash rewritten, fails.
    const builtGuide = path.join(portal, "dist/reference/guide/index.html");
    const builtBytes = await readFile(builtGuide, "utf8");
    for (const mode of ["closed", "open"]) {
      const record = JSON.parse(pristine);
      const edited = builtBytes.replace(/(<div class="sl-markdown-content"[^>]*>)/, `$1<div><template shadowrootmode="${mode}"><style>:host{opacity:0}</style>Shadow</template></div>`);
      assert.notEqual(edited, builtBytes);
      record.artifacts.find((artifact) => artifact.path === "dist/reference/guide/index.html").sha256 = sha256(edited);
      for (const entry of record.pages) {
        await writeFile(path.join(portal, entry.output_markdown), originals.get(entry.route));
        await writeFile(path.join(portal, entry.markdown_twin), originals.get(entry.route));
      }
      await writeFile(builtGuide, edited);
      await writeFile(evidencePath, `${JSON.stringify(record, null, 2)}\n`);
      const result = validate();
      assert.notEqual(result.status, 0, result.stdout);
      assert.match(result.stdout + result.stderr, /built page dist\/reference\/guide\/index\.html carries CSS or executable content in its content: (?:[^\n]*, )?a declarative shadow root/, `${mode}: ${result.stdout}${result.stderr}`);
      await writeFile(builtGuide, builtBytes);
    }
    // Inline scripts outside the content are the runtime's own (R5-1): an
    // extra one in the head fails, and so does a changed Starlight script,
    // each naming the regeneration to run if the runtime really changed.
    const builtOnly = async (edited) => {
      const record = JSON.parse(pristine);
      record.artifacts.find((artifact) => artifact.path === "dist/reference/guide/index.html").sha256 = sha256(edited);
      await writeFile(builtGuide, edited);
      await writeFile(evidencePath, `${JSON.stringify(record, null, 2)}\n`);
      try { const result = validate(); assert.notEqual(result.status, 0, result.stdout); return result.stdout + result.stderr; } finally { await writeFile(builtGuide, builtBytes); }
    };
    const regenerate = /built page dist\/reference\/guide\/index\.html carries inline scripts the site's runtime does not emit \(sha256 \w{12}\); if the runtime changed, regenerate scripts\/runtime-scripts\.json: npm run build && node scripts\/runtime-scripts\.mjs dist/;
    assert.match(await builtOnly(builtBytes.replace("</head>", "<script>void 0</script></head>")), regenerate);
    assert.ok(builtBytes.includes("window.StarlightThemeProvider = (() => {"));
    assert.match(await builtOnly(builtBytes.replace("window.StarlightThemeProvider = (() => {", "window.StarlightThemeProvider = (() => { ")), regenerate);

    // The guide's code block passed above as Expressive Code writes it. A
    // real property on a token, a custom property outside a block's frame, a
    // link or script to an asset the evidence does not record, a style
    // element in a block and a figure inside a block each fail; a recorded
    // asset whose bytes changed fails its artifact hash.
    const content = /built page dist\/reference\/guide\/index\.html carries CSS or executable content in its content: /;
    assert.match(builtBytes, /<span style="--0:#[0-9A-F]{6};--1:#[0-9A-F]{6}">/);
    const token = builtBytes.match(/<span style="(--0:#[0-9A-F]{6};--1:#[0-9A-F]{6})">/)[1];
    for (const css of ["color:red", "opacity:0", "transform:scale(2)", "display:none", "background:url(/x.png)", `${token};color:red`]) {
      assert.match(await builtOnly(builtBytes.replace(`<span style="${token}">`, `<span style="${css}">`)), new RegExp(`${content.source}a style attribute on <span>`), css);
    }
    assert.match(await builtOnly(builtBytes.replace(/(<div class="sl-markdown-content"[^>]*>)/, `$1<p style="${token}">x</p>`)), new RegExp(`${content.source}a style attribute on <p>`));
    const blockHead = builtBytes.match(/<div class="expressive-code"><link rel="stylesheet" href="\/_astro\/ec\.[\w-]+\.css"><script type="module" src="\/_astro\/ec\.[\w-]+\.js"><\/script>/)[0];
    for (const [head, kind] of [
      [blockHead.replace(/ec\.[\w-]+\.css/, "ec.zzzzz.css"), "a <link> element"],
      [blockHead.replace(/ec\.[\w-]+\.js/, "ec.zzzzz.js"), "a <script> element"],
      [`${blockHead}<style>.cf-fig{opacity:0}</style>`, "a <style> element"],
      [`${blockHead}<div class="cf-companion not-content"></div>`, "figure or companion markup inside a code block"],
    ]) {
      assert.match(await builtOnly(builtBytes.replace(blockHead, head)), new RegExp(`${content.source}(?:[^\n]*, )?${kind}`), head);
    }
    // An allowed token attribute excuses only itself (Codex CB-1, Grok F2),
    // and no figure may sit in the sheet's class scope whatever the tag
    // (Codex CB-2, Grok F1), each through the built page.
    const inFrame = (markup) => builtBytes.replace(`<span style="${token}">`, `${markup}<span style="${token}">`);
    assert.match(await builtOnly(inFrame(`<style style="${token}">.cf-fig{opacity:0}</style>`)), new RegExp(`${content.source}a <style> element`));
    assert.match(await builtOnly(inFrame(`<link style="${token}" rel="stylesheet" href="/evil.css">`)), new RegExp(`${content.source}a <link> element`));
    const companionOpen = builtBytes.match(/<div class="cf-companion not-content"/)[0];
    for (const tag of ["div", "section", "article", "span"]) {
      assert.match(await builtOnly(builtBytes.replace(companionOpen, `<${tag} class="expressive-code">${companionOpen}`)), new RegExp(`${content.source}(?:[^\\n]*, )?figure or companion markup inside a code block`), tag);
    }
    assert.match(await builtOnly(builtBytes.replace(companionOpen, '<div class="cf-companion not-content expressive-code"')), new RegExp(`${content.source}(?:[^\\n]*, )?figure or companion markup inside a code block`));
    const codeSheet = JSON.parse(pristine).artifacts.find((artifact) => /^dist\/_astro\/ec\.[\w-]+\.css$/.test(artifact.path)).path;
    const sheetBytes = await readFile(path.join(portal, codeSheet));
    await writeFile(evidencePath, pristine);
    await writeFile(path.join(portal, codeSheet), Buffer.concat([sheetBytes, Buffer.from("\n/* edited */\n")]));
    try {
      const edited = validate();
      assert.notEqual(edited.status, 0, edited.stdout);
      assert.match(edited.stdout + edited.stderr, new RegExp(`built artifact hash mismatch: ${codeSheet.replaceAll(".", "\\.")}`));
    } finally { await writeFile(path.join(portal, codeSheet), sheetBytes); }
    await writeFile(evidencePath, pristine);
    const restored = validate();
    assert.match(restored.stdout + restored.stderr, /validate --portal: 2 page\(s\) clean/, restored.stdout + restored.stderr);
  } finally { await rm(root, { recursive: true, force: true }); }
});

// A page-level motion override must reach the isolated clean comparison too.
// Otherwise the kit's motion variables create false rule-6 differences even
// though the page and clean copy use exactly the same committed sheets.
test("the figure gate matches page motion without accepting changed figure styles", { skip: process.platform === "win32", timeout: 180_000 }, async () => {
  const root = await selfContainedPortalFixture();
  let site;
  let browser;
  try {
    await writeFigureInputs(root);
    await configureFixture(root, {
      page_classes: [{ source: "docs/seed.md", class: "illustrated" }],
      figures: [{ declaration: "figures/architecture.json", route: "reference/seed" }],
    });
    commitFixture(root, "motion context fixture");
    runLocalAdapter(root);
    buildFixture(root);
    const recorded = spawnSync(process.execPath, ["scripts/evidence.mjs"], { cwd: root, encoding: "utf8" });
    assert.equal(recorded.status, 0, recorded.stderr);
    const built = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
    const assignments = classifyPortalPages(config, built.pages);
    const snapshot = new GitSnapshot(root);
    const declarations = pinnedDeclarations(snapshot, built.repository.commit, built);
    const kitSheets = pinnedKitSheets(snapshot, built.repository.commit, "");
    const inlineScripts = pinnedRuntimeScripts(snapshot, built.repository.commit, "", config.theme);
    site = await serve(path.join(root, "dist"));
    browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    for (const reducedMotion of ["reduce", "no-preference"]) {
      const context = await browser.newContext({ reducedMotion: reducedMotion === "reduce" ? "no-preference" : "reduce" });
      try {
        const page = await context.newPage();
        await page.emulateMedia({ reducedMotion });
        const visitRoute = async (route) => { await page.goto(`${site.origin}/${route}/`, { waitUntil: "networkidle" }); };
        const clean = await figureGateFailures(page, visitRoute, assignments, built, declarations, kitSheets, inlineScripts);
        assert.equal(clean.drawn, 1);
        assert.deepEqual(clean.failures, [], `${reducedMotion}: matching sheets must pass`);
        const altered = await figureGateFailures(page, async (route) => {
          await visitRoute(route);
          await page.evaluate(() => { document.querySelector("figure.cf-fig").style.opacity = "0"; });
        }, assignments, built, declarations, kitSheets, inlineScripts);
        assert.ok(altered.failures.some((failure) => /rule 6.*opacity/.test(failure)), `${reducedMotion}: changed figure opacity must still fail`);
      } finally { await context.close(); }
    }
  } finally {
    await browser?.close();
    await site?.close();
    await rm(root, { recursive: true, force: true });
  }
});

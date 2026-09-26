import { verifyChrome } from "./chrome-verify.mjs";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { lstat, mkdir, mkdtemp, readdir, rename, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import AxeBuilder from "@axe-core/playwright";
import { chromium, firefox, webkit } from "@playwright/test";
import { GitSnapshot } from "./git-snapshot.mjs";
import { assertGeneratorIdentity } from "./generator.mjs";
import { stopChild, withSignalAwareChildLifecycle } from "./child-lifecycle.mjs";
import { PORTAL_ACCENT_BACKGROUNDS, pinnedSourceUrl, safeRelative, validatePortalConfig, withBase } from "./lib.mjs";
import { REGENERATE, RUNTIME_SCRIPTS_FILE, allowedInlineScripts, scriptSha256 } from "./runtime-scripts.mjs";
import { canonicalJson, composeFigure, FIGURE_RULES, figureDomFailures, figureRuleFailures, probeFigures, readFigureDom, renderFigure, THRESHOLDS } from "./figure-grammar.mjs";
import { ALTITUDE_PANELS, CARRIER_ELEMENTS, PAGE_CLASSES, RECORD_POINTER_COLUMNS, assertDeclaredCarriers, assertNoRecordRoutes, assertNoStaleSources, assertPageClassCoverage, classifyPortalPages } from "./page-classes.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";
import { assertNoSymlink, assertToolOutputRoots, collectBuiltArtifacts, hashBoundedRegularFile, readBoundedRegularFile, withWorkflowLease } from "./publication.mjs";

const root = process.cwd();
const MAX_SERVER_OUTPUT_BYTES = 64 * 1024;
const MAX_RESULT_ERROR_BYTES = 16 * 1024;
const MAX_RESULTS_BYTES = 1024 * 1024;
const MAX_BROWSER_EVIDENCE_BYTES = 64 * 1024 * 1024;
const MAX_BROWSER_ARTIFACT_BYTES = 32 * 1024 * 1024;
const MAX_BROWSER_ARTIFACTS = 64;
const MAX_RUNTIME_DIAGNOSTICS = 128;
const MAX_RUNTIME_DIAGNOSTIC_BYTES = 4 * 1024;
const runId = process.env.PORTAL_BROWSER_RUN ?? "local";
if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(runId)) throw new Error("PORTAL_BROWSER_RUN is invalid");
// One shared Display panel serves the header at every viewport.
const PORTAL_SKINS = Object.freeze(Object.keys(PORTAL_ACCENT_BACKGROUNDS));
export const PALETTE_PILL_GROUPS = 1;

async function verifyPortal(lifecycle) {
  const configBytes = await readBoundedRegularFile(path.join(root, "portal.config.json"), 64 * 1024, "portal configuration");
  const config = validatePortalConfig(JSON.parse(configBytes));
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  const evidenceBytes = await readBoundedRegularFile(path.join(root, ".portal/generated/evidence.json"), 8 * 1024 * 1024, "portal evidence manifest");
  const generated = JSON.parse(evidenceBytes);
  assertGeneratorIdentity(generated?.generator);
  const repository = path.resolve(root, config.repository_root);
  const snapshot = new GitSnapshot(repository);
  const head = snapshot.resolveHead();
  const configSha256 = digest(configBytes);
  if (generated?.schema_version !== 1 || generated?.repository?.commit !== head || generated?.config_sha256 !== configSha256 || !Array.isArray(generated?.pages) || !Array.isArray(generated?.artifacts)) {
    throw new Error("browser verification requires current commit-bound portal evidence");
  }
  const beforeArtifacts = await collectBuiltArtifacts(path.join(root, "dist"));
  assertArtifactClaims(generated.artifacts, beforeArtifacts, "before browser verification");
  // The page classes are settled before a browser starts: every eligible
  // source is named here, so a page the run never reaches fails as a missing
  // observation instead of passing unseen.
  assertNoRecordRoutes(config, generated.pages, beforeArtifacts.map((artifact) => artifact.path));
  assertNoStaleSources(generated.pages);
  const assignments = classifyPortalPages(config, generated.pages);
  assertDeclaredCarriers(config, assignments);
  const surfaces = await discoverSurfaceRoutes(generated.pages, root);
  const declarations = pinnedDeclarations(snapshot, head, generated);
  const kitSheets = pinnedKitSheets(snapshot, head, path.relative(repository, root));
  const inlineScripts = pinnedRuntimeScripts(snapshot, head, path.relative(repository, root), config.theme);

  const outputRelative = safeRelative(`.portal/browser-evidence/${runId}`, "browser evidence path");
  const output = path.join(root, outputRelative);
  await assertNoSymlink(root, ".portal");
  await mkdir(path.join(root, ".portal/browser-evidence"), { recursive: true });
  await assertNoSymlink(root, ".portal/browser-evidence");
  await assertNoSymlink(root, outputRelative);
  await mkdir(output);
  await assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]);

  const port = await availablePort();
  const origin = `http://127.0.0.1:${port}`;
  const siteRoot = new URL(config.base, origin).toString();
  const server = spawn(process.execPath, [path.join("node_modules", "astro", "bin", "astro.mjs"), "preview", "--host", "127.0.0.1", "--port", String(port)], {
    cwd: root,
    env: hardenedChildEnvironment(process.env, { BROWSER: "none" }),
    stdio: ["ignore", "pipe", "pipe"],
  });
  const untrackServer = lifecycle.trackChild(server);
  let serverOutput = Buffer.alloc(0);
  server.stdout.on("data", (chunk) => { serverOutput = appendBounded(serverOutput, chunk, MAX_SERVER_OUTPUT_BYTES); });
  server.stderr.on("data", (chunk) => { serverOutput = appendBounded(serverOutput, chunk, MAX_SERVER_OUTPUT_BYTES); });

  const results = [];
  try {
    await waitForServer(siteRoot, server, () => serverOutput.toString("utf8"));
    for (const [name, engine] of Object.entries({ chromium, firefox, webkit })) {
      lifecycle.throwIfInterrupted();
      results.push(await verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, assignments, declarations, kitSheets, inlineScripts, lifecycle }));
    }
  } catch (error) {
    lifecycle.throwIfInterrupted();
    results.push({ engine: "preview", status: "failed", error: boundedError(error) });
  } finally {
    await stopChild(server, "SIGTERM");
    untrackServer();
  }

  lifecycle.throwIfInterrupted();
  const teardownVerified = await fetch(siteRoot).then(() => false, () => true);
  const afterArtifacts = await collectBuiltArtifacts(path.join(root, "dist"));
  assertArtifactClaims(generated.artifacts, afterArtifacts, "after browser verification");
  if (JSON.stringify(beforeArtifacts) !== JSON.stringify(afterArtifacts)) throw new Error("built artifacts changed during browser verification");
  const evidence = {
    schema_version: 1,
    run_id: runId,
    build: {
      repository_commit: head,
      config_sha256: configSha256,
      evidence_sha256: digest(evidenceBytes),
      generator: generated.generator,
      artifact_claims_sha256: digest(Buffer.from(JSON.stringify(afterArtifacts))),
      base: config.base,
    },
    origin: "task-owned-loopback",
    port,
    headless: true,
    results,
    artifacts: null,
    teardown_verified: teardownVerified,
  };
  const artifacts = await writeBrowserEvidence(output, evidence);
  if (!teardownVerified || results.some((result) => result.status !== "passed") || artifacts.some((artifact) => artifact.status === "failed")) {
    throw new Error(`portal browser verification failed; inspect ${outputRelative}/results.json`);
  }
  console.log(`portal browser verification passed: ${outputRelative}/results.json`);
}

async function verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, assignments, declarations, kitSheets, inlineScripts, lifecycle }) {
  const profile = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-${runId}-${name}-`));
  const trace = path.join(output, `${name}-trace.zip`);
  const runtime = { console: [], page: [], request: [], remote: [] };
  let context;
  let removeContextCleanup = () => {};
  let traceStarted = false;
  try {
    context = await lifecycle.acquire(engine.launchPersistentContext(profile, {
      headless: true,
      env: hardenedChildEnvironment(),
      colorScheme: "dark",
      reducedMotion: "reduce",
      hasTouch: true,
      viewport: { width: 1440, height: 900 },
    }), (lateContext) => lateContext.close());
    lifecycle.throwIfInterrupted();
    removeContextCleanup = lifecycle.addCleanup(() => context?.close());
    await context.addInitScript(() => {
      window.__codeflowThemeBeforePaint = null;
      requestAnimationFrame(() => { window.__codeflowThemeBeforePaint = document.documentElement.dataset.theme ?? null; });
    });
    // The named review screenshots carry visual evidence. Keep the trace to
    // action/network metadata, and keep it only for an engine that fails.
    await context.tracing.start({ screenshots: false, snapshots: false, sources: false });
    traceStarted = true;
    // A route that cannot be continued or aborted is a request failure,
    // recorded for the isolation check, never a rejection that ends the run.
    await context.route("**/*", async (route) => {
      try {
        const url = new URL(route.request().url());
        if (["http:", "https:"].includes(url.protocol) && url.origin === origin) return await route.continue();
        pushBoundedDiagnostic(runtime.remote, route.request().url());
        return await route.abort("blockedbyclient");
      } catch (error) {
        pushBoundedDiagnostic(runtime.request, `${route.request().url()}: ${boundedError(error)}`);
      }
    });
    const page = context.pages()[0] ?? await context.newPage();
    page.on("console", (message) => { if (message.type() === "error") pushBoundedDiagnostic(runtime.console, message.text()); });
    page.on("pageerror", (error) => pushBoundedDiagnostic(runtime.page, error.message));
    page.on("requestfailed", (request) => {
      if (!runtime.remote.includes(request.url())) pushBoundedDiagnostic(runtime.request, `${request.method()} ${request.url()}: ${request.failure()?.errorText}`);
    });

    await visit(page, siteRoot);
    if (await page.locator("html").getAttribute("data-portal-theme") !== config.theme) throw new Error(`${name}: configured utility theme was not applied`);
    await assertBeforePaintTheme(page, name, "dark");
    await assertSemantics(page, name);
    await assertLayout(page, name);
    await assertA11y(page, name, "/");
    await assertScreenReaderStructure(page, name);
    await assertKeyboardPath(page, name);
    await searchForResult(page, name, config, generated.pages);

    await page.locator('[data-testid="portal-display-btn"]').first().click();
    await page.locator('[data-testid="appearance-light"]').first().click();
    if (await page.locator("html").getAttribute("data-theme") !== "light") throw new Error(`${name}: Appearance Light did not apply`);
    await page.keyboard.press("Escape");
    await page.reload({ waitUntil: "networkidle" });
    if (await page.locator("html").getAttribute("data-theme") !== "light") throw new Error(`${name}: light preference did not persist`);
    await assertBeforePaintTheme(page, name, "light");
    await assertDisplaySettings(page, name);
    await assertPaletteSwatches(page, name, origin, config, assignments[0]);

    for (const layer of config.layers) {
      await visit(page, routeUrl(origin, config.base, layer.id));
      if ((await page.locator("h1").first().textContent())?.trim() !== layer.label) throw new Error(`${name}: ${layer.id} layer heading mismatch`);
      await assertA11y(page, name, `${layer.id} layer`);
    }

    await assertDeepLink(page, name, origin, config.base, surfaces.deepLink);
    const compositionResult = await assertPortalComposition(page, name, origin, config, assignments);
    const figureResult = await assertFigureGate(page, name, origin, config, assignments, generated, declarations, kitSheets, inlineScripts);
    const previewResult = await assertStrictIdPreview(page, name, origin, config, surfaces.strictPreview);
    await assertSourceLink(page, name, origin, config, generated);
    const chromeRoutes = generated.artifacts.filter((artifact) => artifact.path.endsWith(".html"))
      .map((artifact) => new URL(config.base + artifact.path.replace(/^dist\//, "").replace(/index\.html$/, ""), origin).href);
    const altitudeAssignment = assignments.find((assignment) => assignment.pageClass === PAGE_CLASSES.explanatory.id);
    const chromeResult = await verifyChrome(page, chromeRoutes, name,
      altitudeAssignment ? routeUrl(origin, config.base, altitudeAssignment.route) : null);
    if (!await page.evaluate(() => matchMedia("(prefers-reduced-motion: reduce)").matches)) throw new Error(`${name}: chrome verification changed the motion preference`);
    await visit(page, siteRoot);
    await assertThemeMatrix(page, name, output);
    await assertKeyboardPath(page, name);

    await page.setViewportSize({ width: 375, height: 812 });
    await visit(page, siteRoot);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (overflow > 1) throw new Error(`${name}: mobile page overflows by ${overflow}px`);
    const targets = await page.locator(".portal-journey a, button, summary").evaluateAll((items) => items.map((item) => ({ text: item.textContent?.trim().slice(0, 80), width: item.getBoundingClientRect().width, height: item.getBoundingClientRect().height })).filter((target) => target.width > 0 && target.height > 0));
    const undersized = targets.filter((target) => target.width < 24 || target.height < 24);
    if (undersized.length) throw new Error(`${name}: primary target smaller than 24 CSS pixels: ${JSON.stringify(undersized)}`);
    await page.screenshot({ path: path.join(output, `${name}-mobile.png`), fullPage: true });
    const runtimeFailures = meaningfulRuntimeDiagnostics(runtime);
    if (Object.values(runtimeFailures).some((items) => items.length)) throw new Error(`${name}: runtime/network isolation failure: ${JSON.stringify(runtimeFailures)}`);

    await finishTrace(context, trace, true);
    traceStarted = false;
    await context.close();
    removeContextCleanup();
    context = null;
    return {
      engine: name,
      status: "passed",
      checks: ["landmarks-and-names", "axe-wcag22-aa", "screen-reader-structure", "layout", "search-slash-hit-and-follow", "system-and-mode-persistence-before-paint", "display-settings-persist", "palette-pill-swatches-from-live-tokens", "layer-journey", "deep-link", compositionResult, figureResult, previewResult, chromeResult, "source-link", "keyboard-traversal-and-focus", "skins-light-dark", "target-size", "responsive", "console", "network-isolation"],
    };
  } catch (error) {
    lifecycle.throwIfInterrupted();
    if (context && traceStarted) await finishTrace(context, trace, false).catch(() => {});
    return { engine: name, status: "failed", error: boundedError(error) };
  } finally {
    if (context) await context.close().catch(() => {});
    removeContextCleanup();
    await rm(profile, { recursive: true, force: true });
  }
}

async function assertSemantics(page, engine) {
  if (await page.locator("main").count() !== 1) throw new Error(`${engine}: expected one main landmark`);
  if (await page.locator("h1").count() !== 1) throw new Error(`${engine}: expected one primary heading`);
  if (await page.getByRole("navigation").count() < 1) throw new Error(`${engine}: navigation landmark is absent`);
  const unnamed = await page.locator("button, input, select, textarea").evaluateAll((items) => items.filter((item) => !item.getAttribute("aria-label") && !item.getAttribute("aria-labelledby") && !item.textContent?.trim() && !item.getAttribute("title")).length);
  if (unnamed) throw new Error(`${engine}: ${unnamed} controls lack an accessible name`);
}

async function assertScreenReaderStructure(page, engine) {
  const snapshot = await page.locator("body").ariaSnapshot();
  if (!snapshot.includes("heading") || !snapshot.includes("navigation") || !snapshot.includes("main")) throw new Error(`${engine}: accessibility tree lacks the expected document structure`);
}

async function assertLayout(page, engine) {
  const geometry = await page.evaluate(() => {
    const main = document.querySelector("main")?.getBoundingClientRect();
    const header = document.querySelector("header")?.getBoundingClientRect();
    return { main: main && [main.left, main.top, main.right, main.bottom], header: header && [header.left, header.top, header.right, header.bottom], overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth };
  });
  if (!geometry.main || !geometry.header || geometry.overflow > 1 || geometry.main[2] <= geometry.main[0] || geometry.main[3] <= geometry.main[1]) throw new Error(`${engine}: invalid desktop layout ${JSON.stringify(geometry)}`);
}

async function assertA11y(page, engine, surface) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
  if (result.violations.length) throw new Error(`${engine}: ${surface} accessibility violations: ${JSON.stringify(result.violations.map((item) => ({ id: item.id, impact: item.impact, nodes: item.nodes.slice(0, 4).map((node) => ({ target: node.target, failure: node.failureSummary })) })))}`);
}

async function assertBeforePaintTheme(page, engine, expected) {
  await page.waitForFunction(() => window.__codeflowThemeBeforePaint !== null);
  const actual = await page.evaluate(() => window.__codeflowThemeBeforePaint);
  if (actual !== expected) throw new Error(`${engine}: expected ${expected} before first paint, received ${actual}`);
}

async function searchForResult(page, engine, config, pages) {
  const search = page.locator(".cf-search-trigger");
  await search.waitFor({ state: "visible" });
  await page.locator("[data-cf-mounted]").waitFor();
  // "/" is first-class portal chrome: it must open the dialog with the query
  // focused, and a portal-owned term must return a followable Pagefind hit.
  await page.keyboard.press("/");
  const input = page.getByRole("dialog", { name: "Search the guide", exact: true }).locator("input");
  await input.waitFor({ state: "visible" });
  if (!await input.evaluate((element) => element === document.activeElement)) throw new Error(`${engine}: "/" did not focus the search input`);
  const corpus = [config.title, config.description, ...pages.slice(0, 16).flatMap((item) => [item?.title, item?.source_path])].filter((value) => typeof value === "string").join(" ");
  const candidates = [...new Set(corpus.split(/\s+/).map((word) => word.replace(/[^\p{L}\p{N}]/gu, "")).filter((word) => word.length >= 2))].slice(0, 12);
  if (!candidates.length) candidates.push(config.title);
  for (const candidate of candidates) {
    await input.fill(candidate);
    const hit = page.locator(".cf-hit").first();
    if (await hit.waitFor({ state: "visible", timeout: 2_000 }).then(() => true, () => false)) {
      const href = await hit.getAttribute("data-href");
      const target = new URL(href, page.url());
      if (target.origin !== new URL(page.url()).origin) throw new Error(`${engine}: search hit left the portal origin: ${href}`);
      await hit.click();
      await page.waitForURL((url) => url.pathname === target.pathname, { timeout: 10_000 });
      return;
    }
  }
  throw new Error(`${engine}: search returned no result for portal-owned terms`);
}

async function assertDisplaySettings(page, engine) {
  const open = async () => {
    const button = page.locator('[data-testid="portal-display-btn"]').first();
    if (await page.locator('[data-testid="portal-display-panel"]').first().isHidden()) await button.click();
  };
  const accentOf = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--cf-accent").trim());
  const fontOf = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--cf-font-sans").trim());
  for (const family of ["Archivo", "Inter", "IBM Plex Sans"]) {
    const loaded = await page.evaluate(async (name) => {
      const faces = await document.fonts.load(`400 16px "${name}"`);
      return faces.length > 0 && faces.every((face) => face.status === "loaded");
    }, family);
    if (!loaded) throw new Error(`${engine}: bundled font ${family} was not available`);
  }
  const before = { accent: await accentOf(), font: await fontOf() };
  await open();
  await page.locator('[data-testid="skin-slate"]').first().click();
  const slateAccent = await accentOf();
  if (!slateAccent || slateAccent === before.accent) throw new Error(`${engine}: Palette Slate did not change the accent token`);
  await page.locator('[data-testid="typeface-plex"]').first().click();
  if (!(await fontOf()).includes("IBM Plex Sans")) throw new Error(`${engine}: Font Plex Sans did not change the sans stack`);
  await page.reload({ waitUntil: "networkidle" });
  const persisted = await page.evaluate(() => ({ skin: document.documentElement.dataset.cfpSkin, typeface: document.documentElement.dataset.cfpTypeface }));
  if (persisted.skin !== "slate" || persisted.typeface !== "plex") throw new Error(`${engine}: display settings did not persist across reload: ${JSON.stringify(persisted)}`);
  if (await accentOf() !== slateAccent) throw new Error(`${engine}: persisted skin did not reapply its accent token`);
  await page.evaluate(() => { for (const key of ["cf-portal-skin", "cf-portal-typeface", "cf-portal-scale"]) localStorage.removeItem(key); });
  await page.reload({ waitUntil: "networkidle" });
  if (await accentOf() !== before.accent) throw new Error(`${engine}: clearing display settings did not restore the configured theme`);
}

// Every eligible source is visited and measured against the rules its page
// class declares. Failures are collected rather than thrown one at a time, so
// the report names every offending source in one run and a compliant page
// never stands in for a noncompliant one.
async function assertPortalComposition(page, engine, origin, config, assignments) {
  const observations = [];
  const interactionFailures = [];
  for (const assignment of assignments) {
    await visit(page, routeUrl(origin, config.base, assignment.route));
    observations.push({ route: assignment.route, ...await observePortalPage(page) });
    // A page whose class declares the trio is held to its tablist. Skipping a
    // page that renders no tabs would let the chrome disappear silently, which
    // is the one failure the panels themselves cannot show.
    if (assignment.pageClass !== PAGE_CLASSES.explanatory.id) continue;
    try {
      await assertAltitudeInteraction(page, engine, origin, config, assignment.route);
    } catch (error) {
      interactionFailures.push(`${assignment.source} (at ${assignment.route}): ${error.message}`);
    }
  }
  try {
    return `${engine}:${assertPageClassCoverage(assignments, observations, interactionFailures)}`;
  } catch (error) {
    throw new Error(`${engine}: ${error.message}`);
  }
}

// The figure gate: every figure the adapter bound is drawn on its page and
// holds all twelve rules of the grammar, read off the render at a wide and a
// narrow width, in light and in dark. Each failure names the page, the place
// on the page and the rule.
async function assertFigureGate(page, engine, origin, config, assignments, generated, declarations, kitSheets, inlineScripts) {
  const { failures, drawn } = await figureGateFailures(page, (route) => visit(page, routeUrl(origin, config.base, route)), assignments, generated, declarations, kitSheets, inlineScripts);
  if (failures.length) throw new Error(`${engine}: figure gate: ${failures.length} failure(s)\n  ${failures.join("\n  ")}`);
  return `${engine}:figure-gate:${drawn}`;
}

// Each recorded figure declaration as committed at the evidenced commit, held
// to the hash the evidence records: the render is measured against it.
const MAX_DECLARATION_BYTES = 256 * 1024;
export function pinnedDeclarations(snapshot, commit, generated) {
  const declarations = new Map();
  for (const entry of generated.figures ?? []) {
    const bytes = snapshot.bytes(["cat-file", "blob", `${commit}:${safeRelative(entry.declaration_path, "figure declaration path")}`], MAX_DECLARATION_BYTES, "figure declaration");
    if (digest(bytes) !== entry.declaration_sha256) throw new Error(`figure declaration ${entry.declaration_path} does not match its recorded hash`);
    declarations.set(entry.declaration_path, JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)));
  }
  return declarations;
}

// The kit sheets as committed at the evidenced commit: the only stylesheets a
// clean render of a figure gets, so the gate can tell whether anything else
// on a page moves, hides or clips a drawing.
// The hashes an inline script outside the page content may have: the
// runtime's fixed scripts as committed at the evidenced commit, and the
// pre-paint script for the configured theme.
export function pinnedRuntimeScripts(snapshot, commit, portalRelative, theme) {
  const file = safeRelative(path.posix.join(portalRelative.split(path.sep).join("/") || ".", RUNTIME_SCRIPTS_FILE), "runtime script list path");
  const list = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(snapshot.bytes(["cat-file", "blob", `${commit}:${file}`], 64 * 1024, "runtime script list")));
  return allowedInlineScripts(theme, list);
}

export const KIT_SHEETS = Object.freeze(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"]);
export function pinnedKitSheets(snapshot, commit, portalRelative) {
  return KIT_SHEETS.map((name) => {
    const file = safeRelative(path.posix.join(portalRelative.split(path.sep).join("/") || ".", "src/styles", name), "kit sheet path");
    return new TextDecoder("utf-8", { fatal: true }).decode(snapshot.bytes(["cat-file", "blob", `${commit}:${file}`], 1024 * 1024, "kit sheet"));
  }).join("\n");
}

// `declarations` maps each declaration path to its pinned declaration; rule 6
// reads the drawn values back against it and the committed values.
// `kitSheets` is the committed kit CSS: each rendered figure's DOM must equal
// a clean render of its pinned declaration, and its drawings must compute the
// same chosen geometry and visibility styles as that render under the kit
// alone. That comparison is secondary to three checks on every page:
//   1. Page CSS and executable content are refused at their source. The
//      served page must be the built page the evidence records, with no
//      declarative shadow root before the browser consumes its templates.
//      Every stylesheet must be a link in the head to a built CSS file the
//      evidence records, served with its recorded hash, importing nothing,
//      and holding exactly the rules its served bytes parse to. Every external
//      script must be a built script the evidence records, served with its
//      hash. Content and a figure's ancestors carry no style element, link,
//      style attribute, script or shadow root, and no element carries an
//      event handler, a script URL, a frame or an embedded document. The one
//      exception is a code block exactly as Expressive Code writes it (see
//      CODE_BLOCK_ASSET), which never holds a figure.
//   2. Every computed property of each companion, figure and figure
//      descendant, pseudo-elements included, equals a clean copy of the same
//      page: loaded afresh, stripped of any CSS the first check refuses, with
//      the pinned rendering in place of each figure.
//   3. Each figure, its caption and its legend are visible at full opacity,
//      whatever the clean copy shows, and no ancestor moves, clips, filters
//      or hides it unless the clean copy's does too.
export async function figureGateFailures(page, visitRoute, assignments, generated, declarations, kitSheets, inlineScripts) {
  const failures = [];
  let drawn = 0;
  const pinnedSheets = pinnedBuiltSheets(generated.artifacts);
  const pinnedScripts = pinnedBuiltAssets(generated.artifacts, ".js");
  if (!(inlineScripts instanceof Set)) throw new Error("the figure gate needs the runtime's pinned inline scripts");
  // The clean render gets its own context where the browser allows one (a
  // persistent profile has only its own), so nothing from the page carries.
  const browser = page.context().browser();
  const cleanContext = browser === null ? null : await browser.newContext();
  const clean = await (cleanContext ?? page.context()).newPage();
  // A fresh context has its own media preferences, and site sheets read
  // them: the code block sheet sets its theme properties on :root by colour
  // scheme, and the portal sheet stops transitions for reduced motion. The
  // clean copy takes the page's own preferences.
  await clean.emulateMedia(await page.evaluate(() => ({
    colorScheme: ["dark", "light"].find((scheme) => matchMedia(`(prefers-color-scheme: ${scheme})`).matches) ?? "no-preference",
    reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches ? "reduce" : "no-preference",
  })));
  // The clean copy runs only the runtime's scripts: the recorded built
  // scripts by URL and the allowlisted inline scripts by hash.
  let cleanPolicy = null;
  const routeFailures = [];
  await clean.route("**/*", (route) => cleanDocumentRoute(route, cleanPolicy, routeFailures));
  try {
    const recorded = new Map((generated.figures ?? []).map((entry) => [entry.declaration_path, entry]));
    for (const assignment of assignments) {
      await page.setViewportSize({ width: 1440, height: 900 });
      await visitRoute(assignment.route);
      for (const failure of await servedPageFailures(page, assignment.route, generated.artifacts)) failures.push(`${assignment.source} (at ${assignment.route}): served page: ${failure}`);
      for (const failure of await pageCssFailures(page, pinnedSheets, clean)) failures.push(`${assignment.source} (at ${assignment.route}): page CSS: ${failure}`);
      for (const failure of await pageScriptFailures(page, pinnedScripts, inlineScripts)) failures.push(`${assignment.source} (at ${assignment.route}): executable content: ${failure}`);
      if (!assignment.figures?.length) continue;
      const observed = {};
      for (const [label, width, mode] of [["wide", 1440, "light"], ["narrow", 390, "light"], ["wideDark", 1440, "dark"], ["narrowDark", 390, "dark"]]) {
        await page.setViewportSize({ width, height: 900 });
        await visitRoute(assignment.route);
        await page.evaluate(showForReading, { theme: mode, root: null, strip: null, code: null, expected: null });
        await settle(page);
        observed[label] = await page.evaluate(probeFigures, { clearance: THRESHOLDS.labelClearancePx });
        observed[`${label}Dom`] = await page.evaluate(readFigureDom);
        observed[`${label}Context`] = await page.evaluate(readFigureContext);
        for (const failure of figureChromeFailures(await page.evaluate(readFigureChrome))) failures.push(`${assignment.source} (at ${assignment.route}, ${width}px ${mode}): ${failure}`);
        observed[`${label}Url`] = page.url();
        observed[`${label}Root`] = await page.evaluate(() => [...document.documentElement.attributes].filter((attribute) => attribute.name.startsWith("data-")).map((attribute) => [attribute.name, attribute.value]));
        observed[`${label}Places`] = await page.evaluate(() => [...document.querySelectorAll("figure.cf-fig")].map((figure) => {
          const companion = figure.closest(".cf-companion");
          return {
            declaration: companion?.dataset.cfCompanion ?? null,
            placement: companion?.dataset.cfPlacement ?? null,
            altitude: figure.closest(".portal-altitude")?.dataset.altitude ?? null,
          };
        }));
      }
      await page.setViewportSize({ width: 1440, height: 900 });
      const places = observed.widePlaces;
      const expected = assignment.figures.map((binding) => binding.declaration).sort();
      const found = places.map((place) => place.declaration).sort();
      if (canonicalJson(found) !== canonicalJson(expected)) {
        failures.push(`${assignment.source} (at ${assignment.route}): the configuration binds ${expected.join(", ") || "no figure"} and the page draws ${found.join(", ") || "none"}`);
      }
      const rendered = [];
      const wheres = [];
      for (const [index, place] of places.entries()) {
        drawn += 1;
        const binding = assignment.figures.find((candidate) => candidate.declaration === place.declaration);
        const where = `${assignment.source} (at ${assignment.route}, ${place.altitude ? `${place.altitude} panel` : place.placement === "anchor" ? `#${binding?.anchor}` : "page head"}, ${place.declaration ?? "an unbound figure"})`;
        wheres.push(where);
        const entry = recorded.get(place.declaration);
        if (entry === undefined) failures.push(`${where}: the evidence manifest records no such figure`);
        const evidence = entry === undefined ? null : {
          facts: entry.facts.map((fact) => ({ ...fact, matches: canonicalJson(fact.derived) === canonicalJson(fact.drawn) })),
          data: entry.derived === null ? null : { drawn: entry.derived.drawn, derived: entry.derived.values },
        };
        let composed = null;
        let expectedHtml = null;
        try {
          const declaration = declarations.get(place.declaration);
          if (declaration === undefined) throw new Error("its pinned declaration is not available");
          const bound = entry?.derived ? { source: declaration.figure.source, derived: entry.derived.values } : null;
          composed = composeFigure(declaration, bound);
          expectedHtml = renderFigure(declaration, { idPrefix: `cf-fig-${assignment.figures.indexOf(binding)}`, bound, facts: entry?.facts ?? null });
        } catch (error) { failures.push(`${where}: rule 6 (${FIGURE_RULES[6]}): the committed composition cannot be rebuilt: ${error.message}`); }
        rendered.push(expectedHtml);
        const ruleFailures = figureRuleFailures({
          wide: observed.wide[index], narrow: observed.narrow[index], wideDark: observed.wideDark[index], narrowDark: observed.narrowDark[index], evidence, composed,
        });
        for (const failure of ruleFailures) failures.push(`${where}: rule ${failure.rule} (${failure.name}): ${failure.message}`);
        if (expectedHtml === null) continue;
        for (const [label, width] of [["wide", 1440], ["narrow", 390], ["wideDark", 1440], ["narrowDark", 390]]) {
          await clean.setViewportSize({ width, height: 900 });
          const root = observed[`${label}Root`].map(([name, value]) => ` ${name}="${value.replaceAll("&", "&amp;").replaceAll("\"", "&quot;")}"`).join("");
          await clean.setContent(`<!doctype html><html${root}><head><style>${kitSheets}</style></head><body><main>${expectedHtml}</main></body></html>`);
          const [reading] = await clean.evaluate(readFigureDom);
          for (const message of figureDomFailures(observed[`${label}Dom`][index], reading)) failures.push(`${where}: rule 6 (${FIGURE_RULES[6]}): ${label}: ${message}`);
        }
      }
      // The clean copy of the whole page: the same document, the same site
      // sheets and the same display state, without page CSS or page scripts
      // and with each figure as its pinned declaration renders it.
      const site = new URL(observed.wideUrl);
      const suffix = assignment.route === "index" ? "" : `${assignment.route}/`;
      const base = site.pathname.endsWith(suffix) ? site.pathname.slice(0, site.pathname.length - suffix.length) : "/";
      cleanPolicy = `script-src ${[...pinnedScripts.map((script) => `${site.origin}${base}${script.path.slice("dist/".length)}`), ...[...inlineScripts].map((hex) => `'sha256-${Buffer.from(hex, "hex").toString("base64")}'`)].join(" ")}; object-src 'none'`;
      for (const [label, width, mode] of [["wide", 1440, "light"], ["narrow", 390, "light"], ["wideDark", 1440, "dark"], ["narrowDark", 390, "dark"]]) {
        await clean.setViewportSize({ width, height: 900 });
        await clean.goto(observed[`${label}Url`], { waitUntil: "networkidle" });
        await clean.evaluate(showForReading, { theme: mode, root: observed[`${label}Root`], strip: pinnedSheets.map((sheet) => sheet.path.slice("dist/".length)), code: codeBlockAssets(pinnedSheets), expected: rendered });
        await settle(clean);
        const baseline = await clean.evaluate(readFigureContext);
        observed[`${label}Context`].forEach((reading, index) => {
          for (const message of figureContextFailures(reading, baseline[index])) failures.push(`${wheres[index] ?? assignment.source}: rule 6 (${FIGURE_RULES[6]}): ${label}: ${message}`);
        });
      }
    }
  } catch (error) {
    // An aborted clean copy fails its navigation; name the fetch that failed.
    if (routeFailures.length) throw new Error(`${routeFailures.join("; ")}; then ${error.message}`, { cause: error });
    throw error;
  } finally {
    await clean.close();
    await cleanContext?.close();
  }
  failures.push(...routeFailures);
  return { failures, drawn };
}

// The clean copy's document route: the document as served, under the pinned
// script policy once one is set. A fetch that fails (a socket hang-up under
// load) aborts the route and is recorded as a failure of the check, never
// left as a rejection, which would end the verifier with its workflow lease
// and preview server still held.
export async function cleanDocumentRoute(route, policy, failures) {
  try {
    if (route.request().resourceType() !== "document" || policy === null) return await route.fallback();
    const response = await route.fetch();
    await route.fulfill({ response, headers: { ...response.headers(), "content-security-policy": policy } });
  } catch (error) {
    failures.push(`the clean copy could not load ${route.request().url()}: ${error?.message ?? error}`);
    await route.abort("failed").catch(() => {});
  }
}

// The stylesheets or scripts a build may serve: the files of that kind among
// its recorded artifacts, each with the hash the evidence records for it.
export function pinnedBuiltAssets(artifacts, extension) {
  return (Array.isArray(artifacts) ? artifacts : []).filter((artifact) => typeof artifact?.path === "string" && artifact.path.startsWith("dist/") && artifact.path.endsWith(extension)).map((artifact) => ({ path: artifact.path, sha256: artifact.sha256 }));
}
export function pinnedBuiltSheets(artifacts) {
  return pinnedBuiltAssets(artifacts, ".css");
}

// Starlight renders every fenced code block with Expressive Code, which
// writes into the page content, at the head of the first block, a link and a
// module script for its own built assets, and custom properties on the
// highlighted tokens. The content rule lets exactly that through, as
// `validate --portal` does, judging each carrier on an element on its own:
// the link and script are children of a div.expressive-code block with
// exactly the attributes Expressive Code writes, pointing at a recorded
// _astro/ec.<hash> asset that is then served with its hash; a style attribute
// sits on the pre of a block's frame (div.expressive-code > figure > pre) or
// inside it and declares only the custom properties Expressive Code writes,
// with a hex colour, a fixed keyword or a whole number of ch. That attribute
// never excuses a style element, another link or a script on the same element.
// The sheet scopes its rules on the expressive-code class whatever the tag,
// so figure markup on or inside any element with that class fails. With that
// exclusion neither the properties, which reach only their element's
// descendants, nor the sheet can touch a figure: every rule of the sheet that
// styles an element is scoped to .expressive-code, and its only other rules
// declare its --ec-* theme properties on :root, which nothing outside a block
// reads (the clean copy keeps the sheet, so they compare equal). A Markdown
// source cannot write an imitated block: the generated-page check refuses its
// style.
// This plane matches an asset by the recorded path suffix and the ec. name,
// then requires its served bytes to equal the recorded hash; validate --portal
// requires the exact URL under the configured base. A page must pass both.
export const CODE_BLOCK_ASSET = /^dist\/_astro\/ec\.[A-Za-z0-9_-]{1,32}\.(?:css|js)$/;
export function codeBlockAssets(pinned) {
  return pinned.filter((asset) => CODE_BLOCK_ASSET.test(asset.path)).map((asset) => asset.path.slice("dist/".length));
}

async function servedSha256(page, url) {
  const response = await page.request.get(url);
  return { ok: response.ok(), sha256: createHash("sha256").update(await response.body()).digest("hex"), text: await response.text() };
}

// Check 1, the page itself: it is served as the built page the evidence
// records, and its bytes, read before any template becomes a shadow root,
// declare none.
export async function servedPageFailures(page, route, artifacts) {
  const built = route === "index" ? "dist/index.html" : `dist/${route}/index.html`;
  const recorded = (Array.isArray(artifacts) ? artifacts : []).find((artifact) => artifact?.path === built);
  const served = await servedSha256(page, page.url());
  const failures = [];
  if (recorded === undefined) failures.push(`the evidence records no built page ${built}`);
  else if (!served.ok || served.sha256 !== recorded.sha256) failures.push(`${built} is served with sha256 ${served.sha256}, not the recorded ${recorded.sha256}`);
  if (/<template\b[^>]*\sshadowroot(?:mode)?\s*=/i.test(served.text)) failures.push("the served page declares a shadow root");
  return failures;
}

// Check 1, scripts: executable content reaches the page only from the site's
// runtime. External scripts are built scripts the evidence records, served
// with their hash; inline scripts outside the content are the runtime's own,
// by the hashes committed in scripts/runtime-scripts.json. The content and a figure's ancestors carry no script,
// event handler, script URL, frame or embedded document. What the runtime's
// own scripts create in the page chrome is theirs (the search dialog's form,
// for one, has a script URL as its action); the built page is read for these
// before any script runs (servedPageFailures and validate --portal).
export async function pageScriptFailures(page, pinnedScripts, inlineScripts) {
  const found = await page.evaluate((code) => {
    const content = document.querySelector(".sl-markdown-content") ?? document.querySelector("main");
    // A code block's module script, exactly as Expressive Code writes it.
    const codeBlockScript = (element) => element.parentElement?.localName === "div" && element.parentElement.classList.contains("expressive-code")
      && element.attributes.length === 2 && element.getAttribute("type") === "module" && element.childNodes.length === 0
      && /^\/(?!\/)/.test(element.getAttribute("src") ?? "") && code.some((file) => new URL(element.src).pathname.endsWith(`/${file}`));
    const runs = (value) => value.split(";").some((part) => part.replace(/^[\u0000-\u0020]+|[\u0000-\u0020]+$/g, "").replace(/[\t\n\r]/g, "").toLowerCase().startsWith("javascript:"));
    const urls = ["href", "src", "action", "formaction", "xlink:href", "data", "poster", "background"];
    const carriers = new Set();
    const scope = new Set(content ? [content, ...content.querySelectorAll("*")] : []);
    for (const figure of document.querySelectorAll("figure.cf-fig")) for (let node = figure; node; node = node.parentElement) scope.add(node);
    for (const element of scope) {
      const tag = element.localName;
      if (tag === "script" && !codeBlockScript(element)) carriers.add("a <script> element in the page content");
      if (["iframe", "frame", "frameset", "object", "embed"].includes(tag)) carriers.add(`an <${tag}> element`);
      if (tag === "template" && (element.hasAttribute("shadowrootmode") || element.hasAttribute("shadowroot"))) carriers.add("a declarative shadow root");
      for (const attribute of element.attributes) {
        if (/^on./i.test(attribute.name)) carriers.add(`an event-handler attribute on <${tag}>`);
        else if (attribute.name === "srcdoc") carriers.add(`an embedded document on <${tag}>`);
        else if ((urls.includes(attribute.name) || (["animate", "set"].includes(tag) && ["to", "from", "by", "values"].includes(attribute.name))) && runs(attribute.value)) carriers.add(`a script URL on <${tag}>`);
      }
    }
    return {
      carriers: [...carriers],
      scripts: [...document.scripts].filter((script) => script.src).map((script) => script.src),
      inline: [...document.scripts].filter((script) => !script.src && !content?.contains(script)).map((script) => script.text),
    };
  }, codeBlockAssets(pinnedScripts));
  const failures = [...found.carriers];
  const unknown = [...new Set(found.inline.map(scriptSha256).filter((sha) => !inlineScripts.has(sha)))];
  if (unknown.length) failures.push(`inline scripts outside the content are not ones the site's runtime emits (sha256 ${unknown.map((sha) => sha.slice(0, 12)).join(", ")}); if the runtime changed, regenerate ${RUNTIME_SCRIPTS_FILE}: ${REGENERATE}`);
  const origin = new URL(page.url()).origin;
  for (const source of found.scripts) {
    const url = new URL(source);
    const pin = url.origin === origin ? pinnedScripts.find((candidate) => url.pathname.endsWith(`/${candidate.path.slice("dist/".length)}`)) : undefined;
    if (pin === undefined) { failures.push(`the script ${url.origin === origin ? url.pathname : url.href} is not a built script the evidence records`); continue; }
    const served = await servedSha256(page, source);
    if (!served.ok || served.sha256 !== pin.sha256) failures.push(`the script ${url.pathname} is served with sha256 ${served.sha256}, not the recorded ${pin.sha256}`);
  }
  return failures;
}

// Check 1: CSS may reach the page only from the site's own built sheets, and
// each sheet holds exactly the rules its served bytes parse to, read in
// `parser`, a page no page script reaches.
export async function pageCssFailures(page, pinnedSheets, parser = null) {
  const found = await page.evaluate((code) => {
    const content = document.querySelector(".sl-markdown-content") ?? document.querySelector("main");
    const scope = new Set(content ? content.querySelectorAll("*") : []);
    for (const figure of document.querySelectorAll("figure.cf-fig")) for (let node = figure; node; node = node.parentElement) scope.add(node);
    // A code block exactly as Expressive Code writes it: its stylesheet link
    // at the head of a block, and custom properties in a block's frame.
    const isBlock = (node) => node?.localName === "div" && node.classList.contains("expressive-code");
    const codeBlockLink = (element) => element.localName === "link" && isBlock(element.parentElement) && content?.contains(element)
      && element.attributes.length === 2 && element.getAttribute("rel") === "stylesheet"
      && /^\/(?!\/)/.test(element.getAttribute("href") ?? "") && code.some((file) => new URL(element.href).pathname.endsWith(`/${file}`));
    const trim = (text) => text.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "");
    const declared = (name, value) => {
      if (name === "--ecIndent" || name === "--ecMaxLine") return /^\d{1,4}ch$/.test(value);
      const token = /^--\d{1,2}(bg|fs|fw|td)?$/.exec(name);
      if (token === null) return false;
      if (token[1] === "fs") return value === "italic";
      if (token[1] === "fw") return value === "bold";
      if (token[1] === "td") return ["underline", "line-through", "underline line-through"].includes(value);
      return /^#(?:[0-9A-Fa-f]{3,4}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$/.test(value);
    };
    const codeBlockStyle = (element) => {
      const pre = element.closest("pre");
      if (pre === null || pre.parentElement?.localName !== "figure" || !isBlock(pre.parentElement.parentElement)) return false;
      const declarations = element.getAttribute("style").split(";").map(trim).filter(Boolean);
      return declarations.length > 0 && declarations.every((declaration) => {
        const colon = declaration.indexOf(":");
        return colon > 0 && declared(trim(declaration.slice(0, colon)), trim(declaration.slice(colon + 1)));
      });
    };
    const carriers = new Set();
    // The code block sheet scopes its rules on the expressive-code class
    // whatever the tag, so no figure markup (a kit class or a figure data
    // attribute, as validate --portal reads it) may sit on or inside any
    // element with that class.
    const figureMarkup = (element) => [...element.classList].some((name) => /^(?:cf-companion|cf-fig|cf-m-|cf-f-|cf-t--)/.test(name) || ["cf-t", "cf-legend", "cf-key", "cf-twin", "cf-twin-scroll"].includes(name))
      || [...element.attributes].some((attribute) => /^data-cf-(?:companion|figure)/.test(attribute.name));
    for (const element of document.querySelectorAll(".expressive-code, .expressive-code *")) if (figureMarkup(element)) { carriers.add("a figure or companion inside a code block"); break; }
    for (const element of scope) {
      if (element.localName === "style") carriers.add(`a <style> element in ${content?.contains(element) ? "the page content" : "the page"}`);
      if (element.localName === "link" && content?.contains(element) && !codeBlockLink(element)) carriers.add("a <link> element in the page content");
      if (element.hasAttribute("style") && !(content?.contains(element) && codeBlockStyle(element))) carriers.add(`a style attribute on <${element.localName}${element.classList.length ? ` class="${element.getAttribute("class")}"` : ""}>`);
      if (element.shadowRoot) carriers.add(`a shadow root on <${element.localName}>`);
      if (element.localName === "template" && (element.hasAttribute("shadowrootmode") || element.hasAttribute("shadowroot"))) carriers.add("a declarative shadow root");
    }
    const sheets = [...document.styleSheets].map((sheet) => {
      let imports = [];
      let rules = null;
      try {
        imports = [...sheet.cssRules].filter((rule) => rule instanceof CSSImportRule).map((rule) => rule.href);
        rules = [...sheet.cssRules].map((rule) => rule.cssText);
      } catch { imports = ["an unreadable rule list"]; }
      const owner = sheet.ownerNode;
      return { href: sheet.href, owner: owner?.localName ?? null, inHead: owner?.parentElement === document.head, codeBlock: owner ? codeBlockLink(owner) : false, imports, rules };
    });
    return { carriers: [...carriers], sheets, adopted: document.adoptedStyleSheets.length };
  }, codeBlockAssets(pinnedSheets));
  const failures = found.carriers.map((carrier) => `${carrier} carries CSS the site's sheets do not`);
  if (found.adopted) failures.push(`the document adopts ${found.adopted} constructed stylesheet(s)`);
  const origin = new URL(page.url()).origin;
  for (const sheet of found.sheets) {
    if (sheet.owner !== "link" || !(sheet.inHead || sheet.codeBlock) || sheet.href === null) { failures.push(`a stylesheet from ${sheet.owner ? `a <${sheet.owner}> element` : "no element"}${sheet.inHead ? " in the head" : ""} is not a built sheet`); continue; }
    const url = new URL(sheet.href);
    const pin = url.origin === origin ? pinnedSheets.find((candidate) => url.pathname.endsWith(`/${candidate.path.slice("dist/".length)}`)) : undefined;
    if (pin === undefined) { failures.push(`the stylesheet ${url.origin === origin ? url.pathname : url.href} is not a built sheet the evidence records`); continue; }
    const served = await servedSha256(page, sheet.href);
    if (!served.ok || served.sha256 !== pin.sha256) failures.push(`the stylesheet ${url.pathname} is served with sha256 ${served.sha256}, not the recorded ${pin.sha256}`);
    for (const imported of sheet.imports) failures.push(`the stylesheet ${url.pathname} imports ${imported}`);
    if (parser !== null) {
      await parser.goto("about:blank");
      const parsed = await parser.evaluate(({ text, baseURL }) => {
        const sheet = new CSSStyleSheet({ baseURL });
        sheet.replaceSync(text);
        return [...sheet.cssRules].map((rule) => rule.cssText);
      }, { text: served.text, baseURL: sheet.href });
      if (canonicalJson(parsed) !== canonicalJson(sheet.rules)) failures.push(`the stylesheet ${url.pathname} holds ${sheet.rules?.length ?? "unreadable"} rules that are not the ${parsed.length} its served bytes parse to`);
    }
  }
  return failures;
}

// Runs in the page. Sets the display state to read, and on the clean copy
// removes every stylesheet that is not a built sheet in the head or a code
// block's recorded sheet at the head of its block, every style element, and
// style attributes in the content and on a figure's ancestors, then puts each
// figure's pinned rendering in its place.
function showForReading({ theme, root, strip, code, expected }) {
  const html = document.documentElement;
  if (root !== null) {
    for (const attribute of [...html.attributes]) if (attribute.name.startsWith("data-")) html.removeAttribute(attribute.name);
    for (const [name, value] of root) html.setAttribute(name, value);
  }
  html.dataset.theme = theme;
  for (const panel of document.querySelectorAll(".portal-altitude")) panel.hidden = false;
  if (strip === null) return;
  const served = (node, files) => files.some((file) => new URL(node.href, location.href).pathname.endsWith(`/${file}`));
  const built = (node) => node.localName === "link" && ((node.parentElement === document.head && served(node, strip))
    || (node.parentElement?.localName === "div" && node.parentElement.classList.contains("expressive-code") && served(node, code)));
  for (const node of [...document.querySelectorAll("style, link[rel~='stylesheet']")]) if (!built(node)) node.remove();
  document.adoptedStyleSheets = [];
  const content = document.querySelector(".sl-markdown-content") ?? document.querySelector("main");
  const scope = new Set(content ? content.querySelectorAll("[style]") : []);
  for (const figure of document.querySelectorAll("figure.cf-fig")) for (let node = figure.parentElement; node; node = node.parentElement) scope.add(node);
  for (const node of scope) node.removeAttribute("style");
  const figures = [...document.querySelectorAll("figure.cf-fig")];
  expected.forEach((markup, index) => {
    if (markup === null || !figures[index]) return;
    const template = document.createElement("template");
    template.innerHTML = markup;
    const pristine = template.content.querySelector("figure.cf-fig");
    if (pristine) figures[index].replaceWith(pristine);
  });
}

// Fonts loaded and every finite transition or animation at its end, so two
// readings of the same page compare settled values.
async function settle(page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    for (const animation of document.getAnimations()) {
      const timing = animation.effect?.getComputedTiming?.();
      if (timing && Number.isFinite(timing.endTime)) animation.finish();
    }
  });
}

// Runs in the page, self-contained. For each figure in document order: every
// computed property of its companion, the figure and each descendant, with
// the ::before and ::after boxes that draw; the geometry and visibility
// effects of each ancestor to the root; and whether the figure, its caption
// and its legend are visible.
// The legend and the twin as the page lays them out. The clean copy carries
// the same site styles, so a site rule that reaches into the figure shows in
// both and only an absolute reading can see it: each legend key centred on
// its label, and the twin marker the figure sheet's chevron with no fill or
// mask from the site.
export function readFigureChrome() {
  return [...document.querySelectorAll("figure.cf-fig")].map((figure) => {
    const keys = [...figure.querySelectorAll(".cf-legend li")].filter((item) => item.getClientRects().length > 0).map((item) => {
      const key = item.querySelector("svg.cf-key").getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(item);
      range.setStartAfter(item.querySelector("svg.cf-key"));
      const label = range.getBoundingClientRect();
      return { state: item.dataset.state, offset: (key.top + key.height / 2) - (label.top + label.height / 2) };
    });
    const summary = figure.querySelector(".cf-twin > summary");
    const marker = summary === null ? null : getComputedStyle(summary, "::before");
    return {
      id: figure.dataset.cfFigureId,
      keys,
      marker: marker === null ? null : { background: marker.backgroundColor, image: marker.backgroundImage, mask: marker.maskImage || marker.webkitMaskImage || "none", width: marker.width, border: marker.borderRightStyle },
    };
  });
}

export function figureChromeFailures(figures) {
  const failures = [];
  for (const figure of figures) {
    for (const key of figure.keys) {
      if (Math.abs(key.offset) > 1) failures.push(`figure ${figure.id}: the ${key.state} legend key sits ${Math.abs(key.offset).toFixed(1)}px ${key.offset < 0 ? "above" : "below"} its label`);
    }
    const marker = figure.marker;
    if (marker !== null && (!/^(transparent|rgba\(0, 0, 0, 0\))$/.test(marker.background) || marker.image !== "none" || marker.mask !== "none" || marker.width !== "6px" || marker.border !== "solid")) {
      failures.push(`figure ${figure.id}: the twin marker is not the figure sheet's chevron (${JSON.stringify(marker)})`);
    }
  }
  return failures;
}

export function readFigureContext() {
  const EFFECTS = ["opacity", "visibility", "display", "content-visibility", "clip-path", "mask-image", "filter", "backdrop-filter", "transform", "translate", "rotate", "scale", "perspective", "offset-path", "zoom"];
  const name = (element) => `<${element.localName}${element.getAttribute("class") ? ` class="${element.getAttribute("class")}"` : ""}>`;
  const computed = (element, pseudo = null) => {
    const style = getComputedStyle(element, pseudo);
    const values = {};
    for (let index = 0; index < style.length; index += 1) values[style[index]] = style.getPropertyValue(style[index]);
    return values;
  };
  const visible = (element) => !!element && (typeof element.checkVisibility === "function" ? element.checkVisibility({ opacityProperty: true, visibilityProperty: true, contentVisibilityAuto: true }) : element.getClientRects().length > 0);
  return [...document.querySelectorAll("figure.cf-fig")].map((figure) => {
    const companion = figure.closest(".cf-companion");
    const elements = [...(companion ? [companion] : []), figure, ...figure.querySelectorAll("*")];
    const styles = elements.flatMap((element) => {
      const entries = [{ element: name(element), values: computed(element) }];
      for (const pseudo of ["::before", "::after"]) {
        const values = computed(element, pseudo);
        if (values.content && values.content !== "none" && values.content !== "normal") entries.push({ element: `${name(element)}${pseudo}`, values });
      }
      return entries;
    });
    const chain = [];
    let opacity = 1;
    for (let node = figure; node; node = node.parentElement) {
      const style = getComputedStyle(node);
      opacity *= Number(style.opacity);
      if (node !== figure) chain.push({ element: name(node), values: Object.fromEntries(EFFECTS.map((property) => [property, style.getPropertyValue(property)])) });
    }
    const box = figure.getBoundingClientRect();
    return {
      styles,
      chain,
      opacity,
      visible: { figure: visible(figure) && box.width > 0 && box.height > 0, caption: visible(figure.querySelector(".cf-fig-caption")), legend: visible(figure.querySelector(".cf-legend")) },
    };
  });
}

// Checks 2 and 3 for one figure, against the clean copy of its page.
export function figureContextFailures(observed, baseline) {
  if (!observed || !baseline) return ["the figure or its clean copy could not be read"];
  const failures = [];
  for (const [part, shown] of Object.entries(observed.visible)) if (!shown) failures.push(`the ${part} is not visible to a reader`);
  if (!(observed.opacity >= 1)) failures.push(`the figure draws at an effective opacity of ${observed.opacity}`);
  if (observed.chain.length !== baseline.chain.length) failures.push("the figure sits in a different place in the page from its clean copy");
  else observed.chain.forEach((ancestor, position) => {
    const clean = baseline.chain[position];
    for (const [property, value] of Object.entries(ancestor.values)) {
      if (value !== clean.values[property]) failures.push(`an ancestor ${ancestor.element} has ${property} ${value} where the clean copy has ${clean.values[property]}`);
    }
  });
  if (observed.styles.length !== baseline.styles.length) {
    failures.push(`the companion draws ${observed.styles.length} styled boxes where its clean copy draws ${baseline.styles.length}`);
    return failures;
  }
  observed.styles.forEach((entry, position) => {
    const clean = baseline.styles[position];
    if (entry.element !== clean.element) { failures.push(`${entry.element} stands where the clean copy has ${clean.element}`); return; }
    const properties = new Set([...Object.keys(entry.values), ...Object.keys(clean.values)]);
    const differing = [...properties].filter((property) => entry.values[property] !== clean.values[property]).sort();
    if (!differing.length) return;
    const shown = differing.slice(0, 4).map((property) => `${property} ${entry.values[property] ?? "unset"} where the clean copy computes ${clean.values[property] ?? "unset"}`);
    failures.push(`${entry.element} computes ${shown.join(", ")}${differing.length > 4 ? `, and ${differing.length - 4} more` : ""}`);
  });
  return failures;
}

// One structural read per page: the same observation the declared class rules
// are written against, taken from the rendered document rather than from the
// Markdown the adapter wrote.
export function observePortalPage(page) {
  return page.evaluate(({ panels, carriers, columns }) => {
    const shown = (element) => element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
    const panel = (name) => document.querySelector(`.portal-altitude[data-altitude="${name}"]`);
    const selector = { figure: "figure.cf-fig", stage: ".portal-stage", table: "table", list: "ul, ol", pre: "pre" };
    // A carrier is what the reader sees as one thing. The rows and lists a
    // stage renders inside itself are its own internals, not further carriers,
    // so only a carrier with no carrier above it counts, and the stage counts
    // once. A carrier with nothing in it is not a carrier either: an empty
    // figure, a table with no body row, a list with no item carry nothing.
    const filled = (element, carrier) => {
      const text = (node) => node.textContent.trim().length > 0;
      if (carrier === "table") return [...element.querySelectorAll("tbody tr")].some(text);
      if (carrier === "list") return [...element.querySelectorAll("li")].some(text);
      if (carrier === "figure") return element.querySelector("svg.cf-fig-svg") !== null;
      if (carrier === "stage") return element.querySelector("img, svg, pre, table, .portal-stage") !== null || text(element);
      return text(element);
    };
    const own = (element) => (element.parentElement?.closest("figure, .portal-stage") ?? null) === null;
    const carried = (element) => Object.fromEntries(carriers.map((carrier) => [
      carrier,
      [...element.querySelectorAll(selector[carrier])].filter((node) => own(node) && filled(node, carrier)).length,
    ]));
    // The folder table is the table whose headers are the folder columns, not
    // whichever table the page happens to render first, so a pointer page that
    // also carries an unrelated table still reads correctly.
    const tables = [...document.querySelectorAll(".sl-markdown-content table")];
    const headersOf = (table) => [...table.querySelectorAll("thead th")].map((cell) => cell.textContent.trim());
    const folderTable = tables.find((table) => headersOf(table).join("\u0000") === columns.join("\u0000")) ?? null;
    return {
      headings: [...document.querySelectorAll("h1")].filter(shown).length,
      provenance: [...document.querySelectorAll(".portal-provenance")].some(shown),
      displayControls: [...document.querySelectorAll('[data-testid="portal-display-btn"]')].filter(shown).length,
      commentChrome: document.querySelectorAll("[data-testid=comment-btn], .cf-float, .cf-dock, .cf-hint, .cf-composer").length,
      altitudePanels: panels.filter((name) => panel(name) !== null),
      panelCarriers: Object.fromEntries(panels.map((name) => [name, panel(name) === null ? null : carried(panel(name))])),
      pointerColumns: folderTable === null ? [] : headersOf(folderTable),
      pointerRows: folderTable?.querySelectorAll("tbody tr").length ?? 0,
      sourceRegions: document.querySelectorAll("[data-cf-source-region]").length,
      companions: [...document.querySelectorAll(".cf-companion")].filter((companion) => companion.querySelector("figure.cf-fig svg.cf-fig-svg") !== null).length,
      headFigures: document.querySelectorAll('.cf-companion[data-cf-placement="head"] figure.cf-fig').length,
      tables: tables.filter((table) => table.closest("figure") === null && table.querySelector("tbody tr") !== null).length,
    };
  }, { panels: ALTITUDE_PANELS, carriers: CARRIER_ELEMENTS, columns: RECORD_POINTER_COLUMNS });
}

// The trio is a real tablist or it is not the trio: three tabs, each naming the
// panel it controls, and each opening that panel alone. Exercising every tab is
// what proves a panel can be reached, which presence in the document does not.
export async function assertAltitudeTablist(page) {
  const tabs = page.locator('.portal-altitude-tabs [role="tab"]');
  const count = await tabs.count();
  if (count !== ALTITUDE_PANELS.length) throw new Error(`altitude tablist offers ${count} tab(s), the trio needs ${ALTITUDE_PANELS.length}`);
  const controlled = [];
  for (let index = 0; index < count; index += 1) controlled.push(await tabs.nth(index).getAttribute("aria-controls"));
  const trio = await page.evaluate((panels) => panels.map((name) => document.querySelector(`.portal-altitude[data-altitude="${name}"]`)?.id ?? null), ALTITUDE_PANELS);
  const missing = ALTITUDE_PANELS.filter((name, index) => trio[index] === null);
  if (missing.length) throw new Error(`the tablist has no panel to control for ${missing.join(", ")}`);
  if (controlled.join(",") !== trio.join(",")) throw new Error(`tabs control ${controlled.map((id) => id ?? "nothing").join(", ")}, the trio panels are ${trio.join(", ")}`);
  const visiblePanels = async () => {
    let visible = 0;
    for (const id of trio) if (await page.locator(`#${id}`).isVisible()) visible += 1;
    return visible;
  };
  for (let index = 0; index < count; index += 1) {
    await tabs.nth(index).click();
    if (!await page.locator(`#${trio[index]}`).isVisible()) throw new Error(`selecting the ${ALTITUDE_PANELS[index]} tab did not open its panel`);
    if (await visiblePanels() !== 1) throw new Error(`selecting the ${ALTITUDE_PANELS[index]} tab left ${await visiblePanels()} panels open`);
  }
  return trio;
}

async function assertAltitudeInteraction(page, engine, origin, config, route) {
  const tabs = page.locator('.portal-altitude-tabs [role="tab"]');
  const panelIds = [];
  for (const id of await page.evaluate((panels) => panels.map((name) => document.querySelector(`.portal-altitude[data-altitude="${name}"]`)?.id ?? null), ALTITUDE_PANELS)) panelIds.push(id);
  const visiblePanels = async () => {
    let visible = 0;
    for (const id of panelIds) if (id !== null && await page.locator(`#${id}`).isVisible()) visible += 1;
    return visible;
  };
  if (await visiblePanels() !== 1) throw new Error(`shows ${await visiblePanels()} panels at once, tabs must hide inactive layers`);
  await assertA11y(page, engine, `initial altitude panel on ${route}`);
  await assertAltitudeTablist(page);
  const count = await tabs.count();
  await tabs.nth(1).click();
  if (await page.locator(`#${panelIds[0]}`).isVisible()) throw new Error("first altitude panel is still visible while the second tab is selected");
  if (!await page.locator(`#${panelIds[1]}`).isVisible() || await visiblePanels() !== 1) throw new Error("selected altitude tab did not reveal exactly its own panel");
  const anchor = await tabs.nth(1).getAttribute("data-anchor");
  if (await page.evaluate(() => decodeURIComponent(location.hash.slice(1))) !== anchor) throw new Error("selected altitude tab is not recorded in the URL");
  await tabs.nth(1).focus();
  await page.keyboard.press("ArrowLeft");
  if (await tabs.nth(0).getAttribute("aria-selected") !== "true" || !await page.locator(`#${panelIds[0]}`).isVisible()) throw new Error("arrow keys do not move the altitude selection");
  await page.keyboard.press("End");
  if (await tabs.nth(count - 1).getAttribute("aria-selected") !== "true") throw new Error("End does not select the last altitude tab");
  await visit(page, `${routeUrl(origin, config.base, route)}#${encodeURIComponent(anchor)}`);
  if (!await page.locator(`#${panelIds[1]}`).isVisible() || await page.locator(`#${panelIds[0]}`).isVisible()) throw new Error(`loading #${anchor} did not show that panel only`);
  await assertA11y(page, engine, `altitude tabs on ${route}`);
}

// Each palette pill previews the skin it selects. The check opens each Display
// panel and reads the colour the swatch elements actually paint, compared with
// the colour the live stylesheet computes for that skin, so a missing swatch, a
// hard coded colour, or one copy of the current skin painted onto every pill
// fails instead of looking plausible.
async function assertPaletteSwatches(page, engine, origin, config, assignment) {
  if (assignment === undefined) throw new Error(`${engine}: no generated page can prove the Display panels`);
  await visit(page, routeUrl(origin, config.base, assignment.route));
  // The panel paints its swatches in the same call that marks the selected
  // pill, so waiting for that mark proves the component ran without waiting on
  // the swatches this check is about.
  await page.locator('[data-testid="portal-display-btn"]').click();
  await page.locator('.cf-pills[data-pref="skin"] button[aria-checked]').first().waitFor({ state: "attached" });
  const tokens = await page.evaluate((skins) => {
    const root = document.documentElement;
    const computed = getComputedStyle(root);
    // The tokens are hex and the swatches paint in the browser's own colour
    // space, so each token is resolved through the page before comparison.
    const probe = document.createElement("span");
    probe.setAttribute("aria-hidden", "true");
    probe.style.display = "none";
    document.body.append(probe);
    const painted = (value) => {
      probe.style.backgroundColor = "";
      probe.style.backgroundColor = value;
      return getComputedStyle(probe).backgroundColor;
    };
    const selected = root.dataset.cfpSkin;
    const table = {};
    for (const skin of skins) {
      root.dataset.cfpSkin = skin;
      table[skin] = { canvas: painted(computed.getPropertyValue("--cf-canvas").trim()), accent: painted(computed.getPropertyValue("--cf-accent").trim()) };
    }
    if (selected === undefined) delete root.dataset.cfpSkin; else root.dataset.cfpSkin = selected;
    probe.remove();
    return table;
  }, PORTAL_SKINS);
  const buttons = page.locator('[data-testid="portal-display-btn"]');
  const panels = page.locator('[data-testid="portal-display-panel"]');
  const groups = [];
  for (let index = 0; index < await panels.count(); index += 1) {
    // A reader opens one panel at a time, and the component closes the others,
    // so each panel is opened, read and closed in turn.
    if (await buttons.nth(index).isVisible() && await panels.nth(index).isHidden()) await buttons.nth(index).click();
    groups.push(await panels.nth(index).evaluate((panel) => [...panel.querySelectorAll('.cf-pills[data-pref="skin"] button')].map((pill) => {
      const swatch = (name) => {
        const element = pill.querySelector(`.${name}`);
        return element === null ? "" : getComputedStyle(element).backgroundColor;
      };
      return { skin: pill.dataset.value, canvas: swatch("cf-dot-canvas"), accent: swatch("cf-dot-accent") };
    })));
    await page.keyboard.press("Escape");
  }
  const failures = paletteSwatchFailures({ tokens, groups }, PALETTE_PILL_GROUPS);
  if (failures.length) throw new Error(`${engine}: ${assignment.route}: palette pills do not show the live palette they select\n  ${failures.join("\n  ")}`);
}

export function paletteSwatchFailures(observation, expectedGroups) {
  const skins = Object.keys(observation.tokens);
  const failures = [];
  if (observation.groups.length !== expectedGroups) {
    failures.push(`expected ${expectedGroups} palette pill group(s), the page renders ${observation.groups.length}`);
  }
  observation.groups.forEach((pills, index) => {
    const label = `display panel ${index + 1}`;
    const offered = pills.map((pill) => pill.skin);
    if (offered.join(",") !== skins.join(",")) failures.push(`${label} offers ${offered.join(", ") || "no palette"}, the tokens define ${skins.join(", ")}`);
    for (const pill of pills) {
      const token = observation.tokens[pill.skin];
      if (token === undefined) continue;
      for (const role of ["canvas", "accent"]) {
        if (!pill[role]) failures.push(`${label}: the ${pill.skin} pill carries no ${role} swatch`);
        else if (pill[role] !== token[role]) failures.push(`${label}: the ${pill.skin} ${role} swatch is ${pill[role]}, the live token is ${token[role]}`);
      }
    }
    for (const role of ["canvas", "accent"]) {
      const distinct = new Set(pills.map((pill) => pill[role]));
      if (pills.length > 1 && distinct.size === 1) failures.push(`${label}: every ${role} swatch is ${[...distinct][0]}, so the pills do not preview the palette they select`);
    }
  });
  return failures;
}

async function assertDeepLink(page, engine, origin, base, route) {
  if (route === null) throw new Error(`${engine}: no generated page exposes a deep-link heading`);
  await visit(page, routeUrl(origin, base, route));
  const target = page.locator(".sl-markdown-content h2[id], .sl-markdown-content h3[id]").first();
  const targetId = await target.getAttribute("id");
  await visit(page, `${routeUrl(origin, base, route)}#${encodeURIComponent(targetId)}`);
  if (!await target.isVisible()) throw new Error(`${engine}: deep-link target is not visible`);
  if (!await page.evaluate((id) => decodeURIComponent(location.hash.slice(1)) === id, targetId)) throw new Error(`${engine}: deep-link fragment did not persist`);
}

// A cited id usually sits in an altitude panel that is shut when the page
// loads, so reach it the way a reader does: select that panel's tab and wait
// for the tablist to report the change. A trigger outside any panel is already
// where the reader can see it and needs nothing.
export async function revealAltitudePanel(page, target) {
  const panelId = await target.evaluate((element) => element.closest(".portal-altitude")?.id ?? null);
  if (panelId === null) return null;
  const tab = page.locator(`.portal-altitude-tabs [role="tab"][aria-controls="${panelId}"]`).first();
  if (await tab.count() === 0) return null;
  if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
  await page.locator(`.portal-altitude-tabs [role="tab"][aria-controls="${panelId}"][aria-selected="true"]`).first().waitFor({ state: "attached" });
  await page.locator(`#${panelId}`).waitFor({ state: "visible" });
  return panelId;
}

async function assertStrictIdPreview(page, engine, origin, config, route) {
  if (route === null) return "strict-id-preview-not-applicable";
  const sourceUrl = routeUrl(origin, config.base, route);
  await visit(page, sourceUrl);
  const trigger = page.locator(".portal-id-preview > a").first();
  const tooltip = trigger.locator("xpath=following-sibling::*[@role='tooltip']");
  const href = await trigger.getAttribute("href");
  if (!href || new URL(href, page.url()).origin !== origin) throw new Error(`${engine}: strict-ID trigger is not an ordinary local link`);
  const targetUrl = new URL(href, page.url()).toString();
  await revealAltitudePanel(page, trigger);
  await trigger.hover();
  await tooltip.waitFor({ state: "visible" });
  await focusTargetByKeyboard(page, trigger, engine);
  await tooltip.waitFor({ state: "visible" });
  await page.keyboard.press("Enter");
  await page.waitForURL(targetUrl);
  await visit(page, sourceUrl);
  const touchTrigger = page.locator(".portal-id-preview > a").first();
  const touchTooltip = touchTrigger.locator("xpath=following-sibling::*[@role='tooltip']");
  await revealAltitudePanel(page, touchTrigger);
  await focusTargetByKeyboard(page, touchTrigger, engine);
  await page.keyboard.press("Escape");
  await touchTooltip.waitFor({ state: "hidden" });
  await touchTrigger.tap();
  await touchTooltip.waitFor({ state: "visible" });
  await touchTrigger.tap();
  await page.waitForURL(targetUrl);
  return "strict-id-hover-keyboard-touch-escape-navigation";
}

async function assertSourceLink(page, engine, origin, config, generated) {
  if (config.repository_url === null) return;
  const candidate = generated.pages.find((item) => item && item.stale === false && typeof item.route === "string" && typeof item.source_path === "string");
  if (!candidate) throw new Error(`${engine}: no active page can prove source provenance`);
  await visit(page, routeUrl(origin, config.base, candidate.route));
  const expected = pinnedSourceUrl(config.repository_url, generated.repository.commit, candidate.source_path);
  if (expected === null) {
    const provenance = page.locator(".portal-provenance").first();
    const text = await provenance.textContent();
    if (!text?.includes(candidate.source_path) || !text.includes(generated.repository.commit)) throw new Error(`${engine}: visible source provenance fallback is absent`);
    if (await provenance.locator("a").count()) throw new Error(`${engine}: unknown repository provider manufactured a source link`);
    return;
  }
  const links = await page.locator(".portal-provenance a").evaluateAll((items) => items.map((item) => item.href));
  if (!links.includes(expected)) throw new Error(`${engine}: committed source link is absent or provider-incompatible`);
}

// The page owns its appearance and its skin. The Display panel stores
// `starlight-theme` and `cf-portal-skin` and reapplies every attribute from
// that stored state, and Starlight's own pre-paint script reads the same
// preference back, so writing `data-theme` on its own leaves the attribute and
// the preference disagreeing and lets the next apply undo it. Set the state the
// way a reader sets it, and where a page does not expose the control write the
// same preferences the control writes.
export async function applyDisplayState(page, skin, mode) {
  const display = page.locator('[data-testid="portal-display-btn"]').first();
  if (await display.isVisible() && await page.locator('[data-testid="portal-display-panel"]').first().isHidden()) await display.click();
  const skinPill = page.locator(`[data-testid="skin-${skin}"]`).first();
  const appearancePill = page.locator(`[data-testid="appearance-${mode}"]`).first();
  if (await skinPill.count() > 0 && await appearancePill.count() > 0) {
    if (await page.locator('[data-testid="portal-display-panel"]').first().isHidden()) {
      await page.locator('[data-testid="portal-display-btn"]').first().click();
    }
    await skinPill.click();
    await appearancePill.click();
    await page.keyboard.press("Escape");
    return "control";
  }
  await page.evaluate(({ skin, mode }) => {
    try {
      localStorage.setItem("starlight-theme", mode);
      localStorage.setItem("cf-portal-skin", skin);
    } catch {
      /* storage unavailable: the attributes still apply for this page view */
    }
    document.documentElement.dataset.cfpSkin = skin;
    document.documentElement.dataset.theme = mode;
  }, { skin, mode });
  return "preference";
}

// The muted text token is read once per skin and mode inside a single task,
// with nothing painted in between, so the wait below has a value to wait for
// that comes from the page rather than from a second copy of the token table.
async function appearanceTokenReference(page, engine) {
  const reference = await page.evaluate((skins) => {
    const root = document.documentElement;
    const before = { skin: root.dataset.cfpSkin, theme: root.dataset.theme };
    const style = getComputedStyle(root);
    const table = {};
    for (const skin of skins) {
      table[skin] = {};
      for (const mode of ["light", "dark"]) {
        root.dataset.cfpSkin = skin;
        root.dataset.theme = mode;
        table[skin][mode] = style.getPropertyValue("--cf-text-muted").trim();
      }
    }
    if (before.skin === undefined) delete root.dataset.cfpSkin;
    else root.dataset.cfpSkin = before.skin;
    if (before.theme === undefined) delete root.dataset.theme;
    else root.dataset.theme = before.theme;
    return table;
  }, PORTAL_SKINS);
  for (const skin of PORTAL_SKINS) {
    for (const mode of ["light", "dark"]) {
      if (!reference[skin][mode]) throw new Error(`${engine}: ${skin}/${mode} declares no muted text token to settle on`);
    }
  }
  return reference;
}

// No sleep: a scan starts only once the root has resolved the mode it was
// asked for, measured on a token that mode owns.
async function settleDisplayState(page, engine, skin, mode, expectedMuted) {
  try {
    await page.waitForFunction(({ skin, mode, expectedMuted }) => {
      const root = document.documentElement;
      if (root.dataset.cfpSkin !== skin || root.dataset.theme !== mode) return false;
      let stored = mode;
      try {
        stored = localStorage.getItem("starlight-theme");
      } catch {
        /* storage unavailable: the attributes above carry the state instead */
      }
      if (stored !== mode) return false;
      return getComputedStyle(root).getPropertyValue("--cf-text-muted").trim() === expectedMuted;
    }, { skin, mode, expectedMuted });
  } catch {
    const observed = await page.evaluate(() => {
      const root = document.documentElement;
      let stored = "unavailable";
      try {
        stored = localStorage.getItem("starlight-theme");
      } catch {
        /* storage unavailable: reported as such */
      }
      return { skin: root.dataset.cfpSkin, theme: root.dataset.theme, stored, muted: getComputedStyle(root).getPropertyValue("--cf-text-muted").trim() };
    });
    throw new Error(`${engine}: ${skin}/${mode} did not settle before the scan: expected muted ${expectedMuted}, observed ${JSON.stringify(observed)}`);
  }
}

async function assertThemeMatrix(page, engine, output) {
  await page.setViewportSize({ width: 1440, height: 900 });
  const reference = await appearanceTokenReference(page, engine);
  const accents = { light: new Set(), dark: new Set() };
  for (const skin of PORTAL_SKINS) {
    for (const mode of ["light", "dark"]) {
      await applyDisplayState(page, skin, mode);
      await settleDisplayState(page, engine, skin, mode, reference[skin][mode]);
      const tokens = await page.evaluate(() => {
        const style = getComputedStyle(document.documentElement);
        return { font: style.getPropertyValue("--cf-font-sans").trim(), accent: style.getPropertyValue("--cf-accent").trim() };
      });
      if (!tokens.font || !tokens.accent) throw new Error(`${engine}: ${skin}/${mode} utility tokens are absent`);
      accents[mode].add(tokens.accent);
      await assertA11y(page, engine, `${skin}/${mode}`);
      await page.screenshot({ path: path.join(output, `${engine}-${skin}-${mode}.png`), fullPage: true });
    }
  }
  if (accents.light.size !== PORTAL_SKINS.length || accents.dark.size !== PORTAL_SKINS.length) throw new Error(`${engine}: the ${PORTAL_SKINS.length} utility skins do not produce a distinct palette each per mode`);
  // The matrix hands the page back the way a reader left it: the display keys
  // return to the configured default and the light appearance the checks before
  // this one chose is restored, both in storage and on the root.
  await page.evaluate(() => {
    try {
      for (const key of ["cf-portal-skin", "cf-portal-typeface", "cf-portal-scale"]) localStorage.removeItem(key);
      localStorage.setItem("starlight-theme", "light");
    } catch {
      /* storage unavailable: the reload below restores the configured defaults */
    }
  });
  await page.reload({ waitUntil: "networkidle" });
  if (await page.locator("html").getAttribute("data-theme") !== "light") throw new Error(`${engine}: the theme matrix did not restore the light appearance a reader had chosen`);
}

async function assertKeyboardPath(page, engine) {
  const sequence = await keyboardSequence(page, "Tab");
  const effective = sequence.length >= 3 ? sequence : await keyboardSequence(page, "Alt+Tab");
  if (effective.length < 3) throw new Error(`${engine}: keyboard traversal did not reach three distinct controls`);
  for (const item of effective.slice(0, 16)) {
    if (!item.visible) throw new Error(`${engine}: keyboard-focused control is hidden or obscured: ${JSON.stringify(item)}`);
  }
}

async function keyboardSequence(page, key) {
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  const sequence = [];
  for (let index = 0; index < 32; index += 1) {
    await page.keyboard.press(key);
    const item = await activeGeometry(page);
    if (item.signature && !sequence.some((prior) => prior.signature === item.signature)) sequence.push(item);
    if (sequence.length >= 8) break;
  }
  return sequence;
}

export async function focusTargetByKeyboard(page, target, engine) {
  const skipLink = page.getByRole("link", { name: "Skip to content", exact: true });
  for (const key of ["Tab", "Alt+Tab"]) {
    await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
    for (let index = 0; index < 160; index += 1) {
      await page.keyboard.press(key);
      if (await target.evaluate((element) => document.activeElement === element)) return;
      // Use the real keyboard bypass for long repository navigation. Do not
      // focus the target directly or infer reachability from its tabindex.
      if (await skipLink.count() === 1
        && await skipLink.evaluate((element) => document.activeElement === element)) {
        await page.keyboard.press("Enter");
      }
    }
  }
  throw new Error(`${engine}: strict-ID trigger was not reached within bounded keyboard traversal`);
}

async function activeGeometry(page) {
  return page.evaluate(() => {
    const active = document.activeElement;
    if (!(active instanceof HTMLElement) || active === document.body) return { signature: null, visible: false };
    const rect = active.getBoundingClientRect();
    const x = Math.max(0, Math.min(innerWidth - 1, rect.left + rect.width / 2));
    const y = Math.max(0, Math.min(innerHeight - 1, rect.top + rect.height / 2));
    const top = document.elementFromPoint(x, y);
    return {
      signature: `${active.tagName}:${active.getAttribute("href") ?? active.getAttribute("aria-label") ?? active.textContent?.trim().slice(0, 80)}`,
      visible: rect.width > 0 && rect.height > 0 && rect.top >= 0 && rect.bottom <= innerHeight && !!top && (top === active || active.contains(top) || top.contains(active)),
    };
  });
}

export async function discoverSurfaceRoutes(pages, portalRoot = root) {
  const active = pages.filter((item) => item && item.stale === false && typeof item.route === "string" && typeof item.output_markdown === "string" && typeof item.output_markdown_sha256 === "string");
  let remaining = 256 * 1024 * 1024;
  let deepLink = null;
  let strictPreview = null;
  // The composition gate visits every eligible source in its own pass, so this
  // scan only selects the one page each single-surface check needs. Every page
  // is still read here, because its committed output hash is verified.
  for (const page of active) {
    const relative = safeRelative(page.output_markdown, "generated page output");
    const maximum = Math.min(8 * 1024 * 1024, remaining);
    const bytes = await readBoundedRegularFile(path.join(portalRoot, relative), maximum, "generated page output");
    remaining -= bytes.length;
    if (digest(bytes) !== page.output_markdown_sha256) throw new Error(`generated page output hash mismatch while selecting browser surfaces: ${relative}`);
    const text = bytes.toString("utf8");
    if (deepLink === null && /^#{2,3}\s+\S/m.test(text)) deepLink = page.route;
    if (strictPreview === null && text.includes("portal-id-preview")) strictPreview = page.route;
  }
  return { deepLink, strictPreview };
}

export function assertArtifactClaims(claimed, actual, phase) {
  const normalize = (items) => items.map((item) => ({ path: safeRelative(item?.path, "built artifact"), sha256: item?.sha256 }));
  const expected = normalize(claimed);
  const observed = normalize(actual);
  if (JSON.stringify(expected) !== JSON.stringify(observed)) throw new Error(`current dist bytes do not match commit-bound artifact claims ${phase}`);
}

export function appendBounded(current, chunk, maximum) {
  const next = Buffer.concat([current, Buffer.from(chunk)]);
  if (next.length <= maximum) return next;
  return next.subarray(next.length - maximum);
}

export function boundedError(error) {
  const bytes = Buffer.from(String(error?.stack ?? error));
  if (bytes.length <= MAX_RESULT_ERROR_BYTES) return bytes.toString("utf8");
  return `${bytes.subarray(0, MAX_RESULT_ERROR_BYTES).toString("utf8")}\n[diagnostic truncated]`;
}

export function meaningfulRuntimeDiagnostics(runtime) {
  const cancelled = (value) => /\b(?:cancelled|NS_BINDING_ABORTED|net::ERR_ABORTED)\b/i.test(value);
  const cancelledModule = runtime.request.some((value) => cancelled(value) && /\/_astro\/[^ ]+\.js\b/.test(value));
  return {
    console: [...runtime.console],
    page: runtime.page.filter((value) => !(cancelledModule && value === "TypeError: Importing a module script failed.")),
    request: runtime.request.filter((value) => !cancelled(value)),
    remote: [...runtime.remote],
  };
}

function pushBoundedDiagnostic(target, value) {
  if (target.length >= MAX_RUNTIME_DIAGNOSTICS) return;
  const bytes = Buffer.from(String(value));
  target.push(bytes.subarray(0, MAX_RUNTIME_DIAGNOSTIC_BYTES).toString("utf8"));
}

function routeUrl(origin, base, route) {
  return new URL(withBase(base, route), origin).toString();
}

async function visit(page, url) {
  const response = await page.goto(url, { waitUntil: "networkidle" });
  if (response?.status() >= 400) throw new Error(`${url} returned ${response.status()}`);
  // Playwright returns null for same-document navigations such as adding a
  // fragment to the page already under test. Treat that as success only when
  // the browser reached the requested URL and retained a live document.
  if (!response) {
    const reached = await page.evaluate((target) => location.href === target && document.readyState !== "loading", url);
    if (!reached) throw new Error(`${url} did not produce a document response or same-document navigation`);
  }
}

// An engine's trace is kept only when the engine fails: a passing engine is
// proven by its results, and a full run's traces outgrow the evidence caps.
export async function finishTrace(context, trace, passed) {
  if (passed) await context.tracing.stop();
  else await context.tracing.stop({ path: trace });
}

// The results are written before the evidence files are inventoried, so a
// long run never loses them. The inventory then records each file, or a
// failed artifact where a file is over its cap, would take the evidence past
// its aggregate cap or cannot be read, and the results are rewritten with it.
// The caller fails the run on a failed artifact.
export async function writeBrowserEvidence(output, evidence, limits = {}) {
  const { artifactBytes = MAX_BROWSER_ARTIFACT_BYTES, totalBytes = MAX_BROWSER_EVIDENCE_BYTES, resultsBytes = MAX_RESULTS_BYTES, count = MAX_BROWSER_ARTIFACTS } = limits;
  const file = path.join(output, "results.json");
  const encode = (value) => {
    const encoded = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
    if (encoded.length > resultsBytes) throw new Error(`browser evidence results exceed ${resultsBytes} bytes`);
    return encoded;
  };
  await writeFile(file, encode(evidence), { flag: "wx" });
  // The results file is budgeted at its cap, so it and the artifacts always
  // fit the aggregate.
  const artifacts = await evidenceInventory(output, { exclude: "results.json", artifactBytes, totalBytes: totalBytes - resultsBytes, count: count - 1 });
  const encoded = encode({ ...evidence, artifacts });
  await writeFile(`${file}.partial`, encoded, { flag: "wx" });
  await rename(`${file}.partial`, file);
  return artifacts;
}

async function evidenceInventory(directory, { exclude, artifactBytes, totalBytes, count }) {
  const entries = (await readdir(directory)).filter((entry) => entry !== exclude).sort();
  const artifacts = [];
  let total = 0;
  for (const entry of entries.slice(0, count)) {
    const file = path.join(directory, entry);
    const remaining = totalBytes - total;
    try {
      const size = (await lstat(file)).size;
      if (size > artifactBytes) throw new Error(`${size} bytes is over the ${artifactBytes} byte artifact cap`);
      if (size > remaining) throw new Error(`${size} bytes would take the evidence past its ${totalBytes} byte cap`);
      const result = await hashBoundedRegularFile(file, Math.min(artifactBytes, remaining), "browser evidence artifact");
      total += result.bytes;
      artifacts.push({ file: entry, ...result });
    } catch (error) {
      artifacts.push({ file: entry, status: "failed", error: boundedError(error?.message ?? error) });
    }
  }
  if (entries.length > count) artifacts.push({ file: null, status: "failed", error: `${entries.length - count} more files past the ${count} artifact limit` });
  return artifacts;
}

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function availablePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolve(address.port));
    });
  });
}

async function waitForServer(base, processHandle, output) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (processHandle.exitCode !== null) throw new Error(`portal preview exited before readiness: ${output()}`);
    if (await fetch(base).then((response) => response.ok, () => false)) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`portal preview did not become ready: ${output()}`);
}

// The entry point runs last, once every module-level binding is initialized:
// a top-level await above a later `const` reads it before initialization.
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await withSignalAwareChildLifecycle((lifecycle) => withWorkflowLease(root, () => verifyPortal(lifecycle)));
}

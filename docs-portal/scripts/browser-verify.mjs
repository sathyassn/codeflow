import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
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
import { ALTITUDE_PANELS, CARRIER_ELEMENTS, PAGE_CLASSES, RECORD_POINTER_COLUMNS, assertDeclaredCarriers, assertNoRecordRoutes, assertNoStaleSources, assertPageClassCoverage, classifyPortalPages } from "./page-classes.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";
import { assertNoSymlink, assertToolOutputRoots, collectBuiltArtifacts, hashBoundedRegularFile, readBoundedRegularFile, withWorkflowLease } from "./publication.mjs";

const root = process.cwd();
const MAX_SERVER_OUTPUT_BYTES = 64 * 1024;
const MAX_RESULT_ERROR_BYTES = 16 * 1024;
const MAX_RESULTS_BYTES = 1024 * 1024;
const MAX_BROWSER_EVIDENCE_BYTES = 64 * 1024 * 1024;
const MAX_RUNTIME_DIAGNOSTICS = 128;
const MAX_RUNTIME_DIAGNOSTIC_BYTES = 4 * 1024;
const runId = process.env.PORTAL_BROWSER_RUN ?? "local";
if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(runId)) throw new Error("PORTAL_BROWSER_RUN is invalid");
// The skins the utility tokens actually define, read from the token contract
// rather than repeated here, and the Display panels Starlight renders on a
// guide page: the header panel and the mobile menu panel. Both palette pill
// groups are verified, so a swatch that is correct in one panel cannot cover
// the other. The splash landing has no mobile menu and so no second panel,
// which is why this check runs on a generated page and not on the landing.
const PORTAL_SKINS = Object.freeze(Object.keys(PORTAL_ACCENT_BACKGROUNDS));
export const PALETTE_PILL_GROUPS = 2;

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await withSignalAwareChildLifecycle((lifecycle) => withWorkflowLease(root, () => verifyPortal(lifecycle)));
}

async function verifyPortal(lifecycle) {
  const configBytes = await readBoundedRegularFile(path.join(root, "portal.config.json"), 64 * 1024, "portal configuration");
  const config = validatePortalConfig(JSON.parse(configBytes));
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  const evidenceBytes = await readBoundedRegularFile(path.join(root, ".portal/generated/evidence.json"), 8 * 1024 * 1024, "portal evidence manifest");
  const generated = JSON.parse(evidenceBytes);
  assertGeneratorIdentity(generated?.generator);
  const repository = path.resolve(root, config.repository_root);
  const head = new GitSnapshot(repository).resolveHead();
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
      results.push(await verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, assignments, lifecycle }));
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
  const artifacts = await evidenceInventory(output);
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
    artifacts,
    teardown_verified: teardownVerified,
  };
  const encoded = Buffer.from(`${JSON.stringify(evidence, null, 2)}\n`);
  const artifactBytes = artifacts.reduce((total, artifact) => total + artifact.bytes, 0);
  if (encoded.length > MAX_RESULTS_BYTES || artifacts.length + 1 > 64 || artifactBytes + encoded.length > MAX_BROWSER_EVIDENCE_BYTES) {
    throw new Error("browser evidence result envelope exceeds its file, count, or aggregate byte limit");
  }
  await writeFile(path.join(output, "results.json"), encoded, { flag: "wx" });
  if (!teardownVerified || results.some((result) => result.status !== "passed")) {
    throw new Error(`portal browser verification failed; inspect ${outputRelative}/results.json`);
  }
  console.log(`portal browser verification passed: ${outputRelative}/results.json`);
}

async function verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, assignments, lifecycle }) {
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
    // action/network metadata so all engines fit the deterministic evidence
    // envelope instead of duplicating an unbounded screenshot timeline.
    await context.tracing.start({ screenshots: false, snapshots: false, sources: false });
    traceStarted = true;
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (["http:", "https:"].includes(url.protocol) && url.origin === origin) return route.continue();
      pushBoundedDiagnostic(runtime.remote, route.request().url());
      return route.abort("blockedbyclient");
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
    const previewResult = await assertStrictIdPreview(page, name, origin, config, surfaces.strictPreview);
    await assertSourceLink(page, name, origin, config, generated);
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

    await context.tracing.stop({ path: trace });
    traceStarted = false;
    await context.close();
    removeContextCleanup();
    context = null;
    return {
      engine: name,
      status: "passed",
      checks: ["landmarks-and-names", "axe-wcag22-aa", "screen-reader-structure", "layout", "search-slash-hit-and-follow", "system-and-mode-persistence-before-paint", "display-settings-persist", "palette-pill-swatches-from-live-tokens", "layer-journey", "deep-link", compositionResult, previewResult, "source-link", "keyboard-traversal-and-focus", "skins-light-dark", "target-size", "responsive", "console", "network-isolation"],
    };
  } catch (error) {
    lifecycle.throwIfInterrupted();
    if (context && traceStarted) await context.tracing.stop({ path: trace }).catch(() => {});
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
  const search = page.getByRole("button", { name: "Search" }).first();
  await search.waitFor({ state: "visible" });
  await page.waitForFunction(() => !document.querySelector("button[data-open-modal]")?.disabled);
  // "/" is first-class portal chrome: it must open the dialog with the query
  // focused, and a portal-owned term must return a followable Pagefind hit.
  await page.keyboard.press("/");
  const input = page.getByRole("dialog", { name: "Search" }).locator("input");
  await input.waitFor({ state: "visible" });
  if (!await input.evaluate((element) => element === document.activeElement)) throw new Error(`${engine}: "/" did not focus the search input`);
  const corpus = [config.title, config.description, ...pages.slice(0, 16).flatMap((item) => [item?.title, item?.source_path])].filter((value) => typeof value === "string").join(" ");
  const candidates = [...new Set(corpus.split(/\s+/).map((word) => word.replace(/[^\p{L}\p{N}]/gu, "")).filter((word) => word.length >= 2))].slice(0, 12);
  if (!candidates.length) candidates.push(config.title);
  for (const candidate of candidates) {
    await input.fill(candidate);
    const hit = page.locator("dialog[open] a").first();
    if (await hit.waitFor({ state: "visible", timeout: 2_000 }).then(() => true, () => false)) {
      const href = await hit.getAttribute("href");
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
  await page.locator('[data-testid="skin-editorial"]').first().click();
  const editorialAccent = await accentOf();
  if (!editorialAccent || editorialAccent === before.accent) throw new Error(`${engine}: Palette Cool did not change the accent token`);
  await page.locator('[data-testid="typeface-plex"]').first().click();
  if (!(await fontOf()).includes("IBM Plex Sans")) throw new Error(`${engine}: Font Plex Sans did not change the sans stack`);
  await page.reload({ waitUntil: "networkidle" });
  const persisted = await page.evaluate(() => ({ skin: document.documentElement.dataset.cfpSkin, typeface: document.documentElement.dataset.cfpTypeface }));
  if (persisted.skin !== "editorial" || persisted.typeface !== "plex") throw new Error(`${engine}: display settings did not persist across reload: ${JSON.stringify(persisted)}`);
  if (await accentOf() !== editorialAccent) throw new Error(`${engine}: persisted skin did not reapply its accent token`);
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

// One structural read per page: the same observation the declared class rules
// are written against, taken from the rendered document rather than from the
// Markdown the adapter wrote.
export function observePortalPage(page) {
  return page.evaluate(({ panels, carriers, columns }) => {
    const shown = (element) => element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
    const panel = (name) => document.querySelector(`.portal-altitude[data-altitude="${name}"]`);
    const selector = { figure: "figure", stage: ".portal-stage", table: "table", list: "ul, ol", pre: "pre" };
    const carried = (element) => Object.fromEntries(carriers.map((carrier) => [carrier, element.querySelectorAll(selector[carrier]).length]));
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
  await page.locator('.pills[data-group="skin"] button[aria-pressed]').first().waitFor({ state: "attached" });
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
    groups.push(await panels.nth(index).evaluate((panel) => [...panel.querySelectorAll('.pills[data-group="skin"] button')].map((pill) => {
      const swatch = (name) => {
        const element = pill.querySelector(`.${name}`);
        return element === null ? "" : getComputedStyle(element).backgroundColor;
      };
      return { skin: pill.dataset.value, canvas: swatch("sw-canvas"), accent: swatch("sw-accent") };
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

async function evidenceInventory(directory) {
  const entries = (await readdir(directory)).sort();
  if (entries.length > 64) throw new Error("browser evidence contains more than 64 artifacts");
  const artifacts = [];
  let total = 0;
  for (const entry of entries) {
    const file = path.join(directory, entry);
    const remaining = MAX_BROWSER_EVIDENCE_BYTES - total;
    if (remaining <= 0) throw new Error(`browser evidence exceeds ${MAX_BROWSER_EVIDENCE_BYTES} bytes`);
    const result = await hashBoundedRegularFile(file, Math.min(32 * 1024 * 1024, remaining), "browser evidence artifact");
    const artifact = { file: path.basename(file), ...result };
    total += artifact.bytes;
    if (total > MAX_BROWSER_EVIDENCE_BYTES) throw new Error(`browser evidence exceeds ${MAX_BROWSER_EVIDENCE_BYTES} bytes: ${total}`);
    artifacts.push(artifact);
  }
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

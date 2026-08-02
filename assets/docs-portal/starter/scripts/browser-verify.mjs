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
import { withSignalAwareChildLifecycle } from "./child-lifecycle.mjs";
import { pinnedSourceUrl, safeRelative, validatePortalConfig, withBase } from "./lib.mjs";
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

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await withSignalAwareChildLifecycle((lifecycle) => withWorkflowLease(root, () => verifyPortal(lifecycle)));
}

async function verifyPortal(lifecycle) {
  const configBytes = await readBoundedRegularFile(path.join(root, "portal.config.json"), 64 * 1024, "portal configuration");
  const config = validatePortalConfig(JSON.parse(configBytes));
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  const evidenceBytes = await readBoundedRegularFile(path.join(root, ".portal/generated/evidence.json"), 8 * 1024 * 1024, "portal evidence manifest");
  const generated = JSON.parse(evidenceBytes);
  const repository = path.resolve(root, config.repository_root);
  const head = new GitSnapshot(repository).resolveHead();
  const configSha256 = digest(configBytes);
  if (generated?.schema_version !== 1 || generated?.repository?.commit !== head || generated?.config_sha256 !== configSha256 || !Array.isArray(generated?.pages) || !Array.isArray(generated?.artifacts)) {
    throw new Error("browser verification requires current commit-bound portal evidence");
  }
  const beforeArtifacts = await collectBuiltArtifacts(path.join(root, "dist"));
  assertArtifactClaims(generated.artifacts, beforeArtifacts, "before browser verification");
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
    env: { ...process.env, BROWSER: "none" },
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
      results.push(await verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, lifecycle }));
    }
  } catch (error) {
    lifecycle.throwIfInterrupted();
    results.push({ engine: "preview", status: "failed", error: boundedError(error) });
  } finally {
    server.kill("SIGTERM");
    await Promise.race([new Promise((resolve) => server.once("exit", resolve)), new Promise((resolve) => setTimeout(resolve, 5_000))]);
    if (server.exitCode === null) {
      const exited = new Promise((resolve) => server.once("exit", resolve));
      server.kill("SIGKILL");
      await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 5_000))]);
    }
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

async function verifyEngine(name, engine, { origin, siteRoot, output, config, generated, surfaces, lifecycle }) {
  const profile = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-${runId}-${name}-`));
  const trace = path.join(output, `${name}-trace.zip`);
  const runtime = { console: [], page: [], request: [], remote: [] };
  let context;
  let removeContextCleanup = () => {};
  let traceStarted = false;
  try {
    context = await lifecycle.acquire(engine.launchPersistentContext(profile, {
      headless: true,
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
    // Screenshot-backed traces retain the interaction timeline without copying
    // every traversed document snapshot into the bounded evidence envelope.
    await context.tracing.start({ screenshots: true, snapshots: false, sources: false });
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

    await page.getByLabel("Select theme").first().selectOption("light");
    await page.reload({ waitUntil: "networkidle" });
    if (await page.locator("html").getAttribute("data-theme") !== "light") throw new Error(`${name}: light preference did not persist`);
    await assertBeforePaintTheme(page, name, "light");

    for (const layer of config.layers) {
      await visit(page, routeUrl(origin, config.base, layer.id));
      if ((await page.locator("h1").first().textContent())?.trim() !== layer.label) throw new Error(`${name}: ${layer.id} layer heading mismatch`);
      await assertA11y(page, name, `${layer.id} layer`);
    }

    await assertDeepLink(page, name, origin, config.base, surfaces.deepLink);
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
      checks: ["landmarks-and-names", "axe-wcag22-aa", "screen-reader-structure", "layout", "search", "system-and-mode-persistence-before-paint", "layer-journey", "deep-link", previewResult, "source-link", "keyboard-traversal-and-focus", "signal-and-folio-light-dark", "target-size", "responsive", "console", "network-isolation"],
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
  await search.click();
  const input = page.getByRole("dialog", { name: "Search" }).locator("input");
  const corpus = [config.title, config.description, ...pages.slice(0, 16).flatMap((item) => [item?.title, item?.source_path])].filter((value) => typeof value === "string").join(" ");
  const candidates = [...new Set(corpus.split(/\s+/).map((word) => word.replace(/[^\p{L}\p{N}]/gu, "")).filter((word) => word.length >= 2))].slice(0, 12);
  if (!candidates.length) candidates.push(config.title);
  for (const candidate of candidates) {
    await input.fill(candidate);
    if (await page.locator("dialog[open] a").first().waitFor({ state: "visible", timeout: 2_000 }).then(() => true, () => false)) {
      await page.keyboard.press("Escape");
      return;
    }
  }
  throw new Error(`${engine}: search returned no result for portal-owned terms`);
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

async function assertStrictIdPreview(page, engine, origin, config, route) {
  if (route === null) return "strict-id-preview-not-applicable";
  const sourceUrl = routeUrl(origin, config.base, route);
  await visit(page, sourceUrl);
  const trigger = page.locator(".portal-id-preview > a").first();
  const tooltip = trigger.locator("xpath=following-sibling::*[@role='tooltip']");
  const href = await trigger.getAttribute("href");
  if (!href || new URL(href, page.url()).origin !== origin) throw new Error(`${engine}: strict-ID trigger is not an ordinary local link`);
  const targetUrl = new URL(href, page.url()).toString();
  await trigger.hover();
  await tooltip.waitFor({ state: "visible" });
  await focusTargetByKeyboard(page, trigger, engine);
  await tooltip.waitFor({ state: "visible" });
  await page.keyboard.press("Enter");
  await page.waitForURL(targetUrl);
  await visit(page, sourceUrl);
  const touchTrigger = page.locator(".portal-id-preview > a").first();
  const touchTooltip = touchTrigger.locator("xpath=following-sibling::*[@role='tooltip']");
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

async function assertThemeMatrix(page, engine, output) {
  await page.setViewportSize({ width: 1440, height: 900 });
  for (const utilityTheme of ["signal", "folio"]) {
    for (const mode of ["light", "dark"]) {
      const tokens = await page.evaluate(async ({ utilityTheme, mode }) => {
        document.documentElement.dataset.portalTheme = utilityTheme;
        document.documentElement.dataset.theme = mode;
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const style = getComputedStyle(document.documentElement);
        return { font: style.getPropertyValue("--sl-font").trim(), accent: style.getPropertyValue("--sl-color-accent").trim() };
      }, { utilityTheme, mode });
      if (!tokens.font || !tokens.accent) throw new Error(`${engine}: ${utilityTheme}/${mode} utility tokens are absent`);
      if (utilityTheme === "folio" && !tokens.font.includes("Georgia")) throw new Error(`${engine}: folio typography was not applied`);
      if (utilityTheme === "signal" && !tokens.font.includes("Inter")) throw new Error(`${engine}: signal typography was not applied`);
      await assertA11y(page, engine, `${utilityTheme}/${mode}`);
      await page.screenshot({ path: path.join(output, `${engine}-${utilityTheme}-${mode}.png`), fullPage: true });
    }
  }
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

async function focusTargetByKeyboard(page, target, engine) {
  for (const key of ["Tab", "Alt+Tab"]) {
    await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
    for (let index = 0; index < 160; index += 1) {
      await page.keyboard.press(key);
      if (await target.evaluate((element) => document.activeElement === element)) return;
    }
  }
  throw new Error(`${engine}: strict-ID trigger is not keyboard reachable`);
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
  for (const page of active) {
    const relative = safeRelative(page.output_markdown, "generated page output");
    const maximum = Math.min(8 * 1024 * 1024, remaining);
    const bytes = await readBoundedRegularFile(path.join(portalRoot, relative), maximum, "generated page output");
    remaining -= bytes.length;
    if (digest(bytes) !== page.output_markdown_sha256) throw new Error(`generated page output hash mismatch while selecting browser surfaces: ${relative}`);
    const text = bytes.toString("utf8");
    if (deepLink === null && /^#{2,3}\s+\S/m.test(text)) deepLink = page.route;
    if (strictPreview === null && text.includes("portal-id-preview")) strictPreview = page.route;
    if (deepLink !== null && strictPreview !== null) break;
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

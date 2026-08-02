import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { chromium, firefox, webkit } from "@playwright/test";
import { assertNoSymlink, assertToolOutputRoots } from "./publication.mjs";
import { safeRelative } from "./lib.mjs";

const root = process.cwd();
const runId = process.env.PORTAL_BROWSER_RUN ?? "local";
if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(runId)) throw new Error("PORTAL_BROWSER_RUN is invalid");
const outputRelative = safeRelative(`.portal/browser-evidence/${runId}`, "browser evidence path");
const output = path.join(root, outputRelative);
await assertNoSymlink(root, ".portal/browser-evidence");
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]);

const port = await availablePort();
const origin = `http://127.0.0.1:${port}`;
const server = spawn(process.execPath, [path.join("node_modules", "astro", "bin", "astro.mjs"), "preview", "--host", "127.0.0.1", "--port", String(port)], {
  cwd: root,
  env: { ...process.env, BROWSER: "none" },
  stdio: ["ignore", "pipe", "pipe"],
});
let serverOutput = "";
server.stdout.on("data", (chunk) => { serverOutput += chunk; });
server.stderr.on("data", (chunk) => { serverOutput += chunk; });

const results = [];
try {
  await waitForServer(origin, server);
  for (const [name, engine] of Object.entries({ chromium, firefox, webkit })) {
    results.push(await verifyEngine(name, engine, origin, output));
  }
} finally {
  server.kill("SIGTERM");
  await Promise.race([new Promise((resolve) => server.once("exit", resolve)), new Promise((resolve) => setTimeout(resolve, 5_000))]);
  if (server.exitCode === null) server.kill("SIGKILL");
}

const teardownVerified = await fetch(origin).then(() => false, () => true);
const evidence = { schema_version: 1, run_id: runId, origin: "task-owned-loopback", port, headless: true, results, teardown_verified: teardownVerified };
await writeFile(path.join(output, "results.json"), `${JSON.stringify(evidence, null, 2)}\n`, { flag: "wx" });
if (!teardownVerified || results.some((result) => result.status !== "passed")) {
  throw new Error(`portal browser verification failed: ${JSON.stringify(evidence)}`);
}
console.log(`portal browser verification passed: ${outputRelative}/results.json`);

async function verifyEngine(name, engine, base, outputRoot) {
  const profile = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-${runId}-${name}-`));
  const trace = path.join(outputRoot, `${name}-trace.zip`);
  const screenshots = [];
  const runtime = { console: [], page: [], request: [], remote: [] };
  let context;
  try {
    context = await engine.launchPersistentContext(profile, { headless: true, colorScheme: "dark", reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
    await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (!["http:", "https:"].includes(url.protocol) || url.origin === base) return route.continue();
      runtime.remote.push(route.request().url());
      return route.abort("blockedbyclient");
    });
    const page = context.pages()[0] ?? await context.newPage();
    page.on("console", (message) => { if (message.type() === "error") runtime.console.push(message.text()); });
    page.on("pageerror", (error) => runtime.page.push(error.message));
    page.on("requestfailed", (request) => {
      if (!runtime.remote.includes(request.url())) runtime.request.push(`${request.method()} ${request.url()}: ${request.failure()?.errorText}`);
    });
    await visit(page, `${base}/`);
    await assertSemantics(page, name);
    await assertLayout(page, name);
    await assertA11y(page, name, "/");
    const desktop = path.join(outputRoot, `${name}-desktop.png`);
    await page.screenshot({ path: desktop, fullPage: true });
    screenshots.push(await artifact(desktop));

    const search = page.getByRole("button", { name: "Search" }).first();
    await search.waitFor({ state: "visible" });
    await page.waitForFunction(() => !document.querySelector("button[data-open-modal]")?.disabled);
    await search.click();
    const dialog = page.getByRole("dialog", { name: "Search" });
    await dialog.locator("input").fill("documentation portal");
    await page.waitForFunction(() => document.querySelectorAll("dialog[open] a").length > 0);
    await page.keyboard.press("Escape");

    await page.getByLabel("Select theme").first().selectOption("light");
    await page.reload({ waitUntil: "networkidle" });
    if (await page.locator("html").getAttribute("data-theme") !== "light") throw new Error(`${name}: light preference did not persist`);
    await assertA11y(page, name, "light mode");

    for (const [route, heading] of [["/orient/", "Orient"], ["/system/", "System"], ["/records/", "Records"]]) {
      await visit(page, `${base}${route}`);
      if ((await page.locator("h1").first().textContent())?.trim() !== heading) throw new Error(`${name}: ${route} heading mismatch`);
      await assertA11y(page, name, route);
    }

    await visit(page, `${base}/system/architecture/`);
    const target = page.locator(".sl-markdown-content h2[id], .sl-markdown-content h3[id]").first();
    const targetId = await target.getAttribute("id");
    if (!targetId) throw new Error(`${name}: deep-link target fixture is absent`);
    await visit(page, `${base}/system/architecture/#${encodeURIComponent(targetId)}`);
    if (await page.evaluate((id) => location.hash.slice(1) === encodeURIComponent(id) || decodeURIComponent(location.hash.slice(1)) === id, targetId) !== true) throw new Error(`${name}: deep-link fragment did not persist`);
    if (!await target.isVisible()) throw new Error(`${name}: deep-link target is not visible`);

    await assertFocusIsVisible(page, name);
    await page.setViewportSize({ width: 375, height: 812 });
    await visit(page, `${base}/`);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (overflow > 1) throw new Error(`${name}: mobile page overflows by ${overflow}px`);
    const targets = await page.locator(".portal-journey a, button, summary").evaluateAll((items) => items.filter((item) => getComputedStyle(item).display !== "none").map((item) => ({ text: item.textContent?.trim(), width: item.getBoundingClientRect().width, height: item.getBoundingClientRect().height })));
    if (targets.some((target) => target.width < 24 || target.height < 24)) throw new Error(`${name}: primary target smaller than 24 CSS pixels`);
    const mobile = path.join(outputRoot, `${name}-mobile.png`);
    await page.screenshot({ path: mobile, fullPage: true });
    screenshots.push(await artifact(mobile));
    if (Object.values(runtime).some((items) => items.length)) throw new Error(`${name}: runtime/network isolation failure: ${JSON.stringify(runtime)}`);
    await context.tracing.stop({ path: trace });
    await context.close();
    context = null;
    return { engine: name, status: "passed", checks: ["landmarks-and-names", "axe-wcag22-aa", "layout", "search", "mode-persistence", "deep-link", "focus-not-obscured", "target-size", "responsive", "console", "network-isolation"], screenshots, trace: await artifact(trace) };
  } catch (error) {
    if (context) await context.tracing.stop({ path: trace }).catch(() => {});
    return { engine: name, status: "failed", error: String(error?.stack ?? error), screenshots };
  } finally {
    if (context) await context.close().catch(() => {});
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

async function assertLayout(page, engine) {
  const geometry = await page.evaluate(() => {
    const main = document.querySelector("main")?.getBoundingClientRect();
    const header = document.querySelector("header")?.getBoundingClientRect();
    return { viewport: [innerWidth, innerHeight], main: main && [main.left, main.top, main.right, main.bottom], header: header && [header.left, header.top, header.right, header.bottom], overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth };
  });
  if (!geometry.main || !geometry.header || geometry.overflow > 1 || geometry.main[2] <= geometry.main[0] || geometry.main[3] <= geometry.main[1]) throw new Error(`${engine}: invalid desktop layout ${JSON.stringify(geometry)}`);
}

async function assertA11y(page, engine, surface) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
  if (result.violations.length) throw new Error(`${engine}: ${surface} accessibility violations: ${JSON.stringify(result.violations.map((item) => ({ id: item.id, impact: item.impact, nodes: item.nodes.length })))}`);
}

async function assertFocusIsVisible(page, engine) {
  for (let index = 0; index < 16; index += 1) {
    await page.keyboard.press("Tab");
    const result = await page.evaluate(() => {
      const active = document.activeElement;
      if (!(active instanceof HTMLElement)) return { ok: false, reason: "no active element" };
      const rect = active.getBoundingClientRect();
      const x = Math.max(0, Math.min(innerWidth - 1, rect.left + rect.width / 2));
      const y = Math.max(0, Math.min(innerHeight - 1, rect.top + rect.height / 2));
      const top = document.elementFromPoint(x, y);
      return { ok: rect.width > 0 && rect.height > 0 && rect.top >= 0 && rect.bottom <= innerHeight && !!top && (top === active || active.contains(top) || top.contains(active)), tag: active.tagName, text: active.textContent?.trim().slice(0, 80) };
    });
    if (!result.ok) throw new Error(`${engine}: focused control is hidden or obscured: ${JSON.stringify(result)}`);
  }
}

async function visit(page, url) {
  const response = await page.goto(url, { waitUntil: "networkidle" });
  if (response?.status() >= 400 || !response && page.url() !== url) throw new Error(`${url} returned ${response?.status()}`);
}

async function artifact(file) {
  const bytes = await readFile(file);
  return { file: path.basename(file), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
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

async function waitForServer(base, processHandle) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (processHandle.exitCode !== null) throw new Error(`portal preview exited before readiness: ${serverOutput}`);
    if (await fetch(base).then((response) => response.ok, () => false)) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`portal preview did not become ready: ${serverOutput}`);
}

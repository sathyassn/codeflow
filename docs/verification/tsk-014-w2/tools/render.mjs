// Sequential, exact-owned, headless render + accessibility + network evidence
// for the TSK-014 W2 study.
//
// Lifecycle contract:
//   - the browser and locked modules resolve BEFORE any owned output is touched;
//   - an alternate --output-root is accepted only under a positive ownership
//     contract (owner-only, under the host temp dir after canonical resolution,
//     carrying a marker whose token matches the invocation). Nothing is ever
//     deleted outside a closed known name set;
//   - one marked task root holds the profile and every redirected environment
//     root, including an owner-only XDG_RUNTIME_DIR;
//   - owned identities are captured after launch and refreshed during cleanup;
//   - after the bounded close, any still-live exact identity triggers the
//     fingerprint-rechecked TERM/KILL fallback;
//   - the marked root is removed ONLY on positive proof: an inventory that
//     completed successfully and showed zero live exact identities. Any
//     uncertainty retains the root and fails;
//   - only ENOENT proves removal; other stat errors are uncertainty;
//   - file: is admitted only for canonical descendants of the canonical study
//     root, so a symlink escape is rejected too.
//
// Self-test flags: --output-root <dir> --output-token <token>, --page <rel>,
// --pages <n>, --inject-post-close-survivor, --inject-inventory-failure.

import { access, chmod, lstat, mkdir, mkdtemp, readdir, readFile, realpath, rm, stat, writeFile } from "node:fs/promises";
import { execFileSync, spawn } from "node:child_process";
import { tmpdir } from "node:os";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { readFileSync } from "node:fs";
import { isContained } from "./path-containment.mjs";

const studyRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(studyRoot, "../../..");
const webModules = join(repoRoot, "crates/codeflow-present/web/node_modules");
const SELFTEST_MARKER = ".cf-w2-selftest-root";

const argOf = (name) => {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
};
const requestedOutputRoot = argOf("--output-root");
const outputToken = argOf("--output-token");
const pageLimit = argOf("--pages") ? Number(argOf("--pages")) : Infinity;
const onlyPage = argOf("--page");
const injectSurvivor = process.argv.includes("--inject-post-close-survivor");
const injectInventoryFailure = process.argv.includes("--inject-inventory-failure");

const CLOSE_TIMEOUT_MS = 15_000;
const NAV_TIMEOUT_MS = 20_000;
const TERMINATION_GRACE_MS = 2_500;
const INVENTORY_TIMEOUT_MS = 10_000;
const IDENTITY_DEADLINE_MS = 10_000;
const VIEWPORTS = { desktop: { width: 1440, height: 900 }, mobile: { width: 390, height: 844 } };

const CANDIDATES = [
  "cases/p1/a-wave-lanes/index.html",
  "cases/p1/b-blocking-matrix/index.html",
  "cases/p1/c-critical-ribbon/index.html",
  "cases/p2/a-evidence-grid/index.html",
  "cases/p2/b-provenance-rail/index.html",
  "cases/p3/a-finding-anchored-delta/index.html",
  "cases/p3/b-convergence-ledger/index.html",
  "cases/d1/a-concept-dependency-path/index.html",
  "cases/d1/b-task-first-entry/index.html",
  "cases/d1/c-contract-map/index.html",
  "cases/d2/a-chain-in-place/index.html",
  "cases/d2/b-evidence-adjacent-margin/index.html",
];
const BASELINES = [
  "baselines/p1/baseline.html",
  "baselines/p2/baseline.html",
  "baselines/p3/baseline.html",
  "baselines/d1/baseline.html",
  "baselines/d2/baseline.html",
];
const slugOf = (page) => page
  .replace(/^cases\//, "").replace(/\/index\.html$/, "")
  .replace(/^baselines\//, "").replace(/\/baseline\.html$/, "-baseline")
  .replace(/^tools\/fixtures\//, "fixture-").replace(/\.html$/, "")
  .replace(/\//g, "-");
const fullPlan = [
  ...CANDIDATES.flatMap((page) => [
    ...["light", "dark"].flatMap((mode) =>
      Object.keys(VIEWPORTS).map((viewport) => ({ page, mode, viewport, motion: "no-preference" }))),
    { page, mode: "light", viewport: "desktop", motion: "reduce" },
  ]),
  ...BASELINES.flatMap((page) =>
    Object.keys(VIEWPORTS).map((viewport) => ({ page, mode: "light", viewport, motion: "no-preference" }))),
];
const plan = (onlyPage
  ? [{ page: onlyPage, mode: "light", viewport: "desktop", motion: "no-preference" }]
  : fullPlan).slice(0, pageLimit);
const nameOf = (job) => `${slugOf(job.page)}-${job.mode}-${job.viewport}${job.motion === "reduce" ? "-reduced" : ""}`;

const refuse = (message) => { process.stderr.write(`REFUSED ${message}\n`); process.exit(2); };

// ---------------------------------- positive ownership contract for an output root
async function resolveOutputRoot() {
  if (!requestedOutputRoot) return studyRoot;
  if (!outputToken || !/^[0-9a-f-]{16,}$/u.test(outputToken)) {
    refuse("--output-root requires a --output-token issued by tools/selftest.mjs");
  }
  let canonical;
  try { canonical = await realpath(requestedOutputRoot); }
  catch { refuse(`--output-root does not exist: ${requestedOutputRoot}`); }
  const canonicalTemp = await realpath(tmpdir());
  if (!isContained(canonicalTemp, canonical, path)) {
    refuse(`--output-root is not inside the host temporary directory: ${canonical}`);
  }
  const info = await stat(canonical);
  if (!info.isDirectory()) refuse(`--output-root is not a directory: ${canonical}`);
  if (process.platform !== "win32") {
    if ((info.mode & 0o077) !== 0) refuse(`--output-root is not owner-only: ${canonical}`);
    if (info.uid !== process.getuid()) refuse(`--output-root is not owned by this user: ${canonical}`);
  }
  const markerPath = join(canonical, SELFTEST_MARKER);
  const markerInfo = await lstat(markerPath).catch(() => null);
  if (!markerInfo || !markerInfo.isFile()) refuse(`--output-root carries no ${SELFTEST_MARKER} marker: ${canonical}`);
  const recorded = (await readFile(markerPath, "utf8")).trim();
  if (recorded !== outputToken) refuse(`--output-root marker token does not match this invocation: ${canonical}`);
  return canonical;
}

// ------------------------------------------------------- process identities
let inventoryFailureArmed = false;
function processInventory() {
  if (inventoryFailureArmed) throw new Error("injected process-inventory failure (self-test)");
  try {
    if (process.platform === "win32") {
      const systemRoot = process.env.SystemRoot ?? process.env.SYSTEMROOT ?? "C:\\Windows";
      const powershell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
      const raw = execFileSync(powershell, ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
        "Get-CimInstance Win32_Process | Select-Object ProcessId,CreationDate,CommandLine | ConvertTo-Json -Compress"],
        { encoding: "utf8", timeout: INVENTORY_TIMEOUT_MS });
      const parsed = JSON.parse(raw || "[]");
      return (Array.isArray(parsed) ? parsed : [parsed]).map((e) => ({
        pid: Number(e.ProcessId), created: String(e.CreationDate ?? ""), command: String(e.CommandLine ?? ""),
      }));
    }
    const raw = execFileSync("/bin/ps", ["-axo", "pid=,lstart=,command="],
      { encoding: "utf8", timeout: INVENTORY_TIMEOUT_MS, maxBuffer: 8 * 1024 * 1024 });
    return raw.split("\n").flatMap((line) => {
      const match = line.match(/^\s*(\d+)\s+((?:\S+\s+){4}\d{4})\s+(.+)$/u);
      return match ? [{ pid: Number(match[1]), created: match[2], command: match[3] }] : [];
    });
  } catch (error) {
    throw new Error(`Could not inspect owned process identities: ${error.message}`);
  }
}
const fingerprint = (pid, inventory = processInventory()) => {
  const found = inventory.find((p) => p.pid === pid);
  return found ? `${found.created}|${found.command}` : null;
};
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

const ownedById = new Map();
function refreshOwned(marker) {
  for (const p of processInventory()) {
    if (!p.command.includes(marker)) continue;
    ownedById.set(`${p.pid}|${p.created}|${p.command}`, { pid: p.pid, print: `${p.created}|${p.command}` });
  }
  return [...ownedById.values()];
}
function liveOwned() {
  const inventory = processInventory();
  return [...ownedById.values()].filter((o) => fingerprint(o.pid, inventory) === o.print);
}
async function terminateOwned(targets) {
  const signal = (list, sig) => {
    for (const o of list) {
      // Exact identity re-verified immediately before signalling; only the
      // precise pid is signalled, never a process group.
      if (fingerprint(o.pid) !== o.print) continue;
      try { process.kill(o.pid, sig); }
      catch (error) { if (error?.code !== "ESRCH") throw new Error(`could not ${sig} ${o.pid}: ${error.message}`); }
    }
  };
  signal(targets, "SIGTERM");
  const grace = Date.now() + TERMINATION_GRACE_MS;
  let remaining = liveOwned();
  while (remaining.length && Date.now() < grace) { await delay(100); remaining = liveOwned(); }
  if (remaining.length) signal(remaining, "SIGKILL");
  const deadline = Date.now() + IDENTITY_DEADLINE_MS;
  while (liveOwned().length) {
    if (Date.now() >= deadline) throw new Error("exact-owned browser identities survived TERM and KILL");
    await delay(100);
  }
}

async function findBrowser() {
  for (const candidate of [
    process.env.CF_W2_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    process.env.ProgramFiles && join(process.env.ProgramFiles, "Google", "Chrome", "Application", "chrome.exe"),
    process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, "Google", "Chrome", "Application", "chrome.exe"),
    "/usr/bin/google-chrome", "/usr/bin/chromium",
  ].filter(Boolean)) {
    try { await access(candidate); return candidate; } catch { /* next */ }
  }
  throw new Error("No local Chrome or Chromium is available for the W2 render");
}

// ------------------------------------------- 1. discovery, before any mutation
const outputRoot = await resolveOutputRoot();
const executablePath = await findBrowser();
const playwright = await import(pathToFileURL(join(webModules, "playwright-core/index.js")).href);
const { chromium } = playwright.chromium ? playwright : playwright.default;
const axeSource = readFileSync(join(webModules, "axe-core/axe.min.js"), "utf8");
const playwrightVersion = JSON.parse(readFileSync(join(webModules, "playwright-core/package.json"), "utf8")).version;
const axeVersion = JSON.parse(readFileSync(join(webModules, "axe-core/package.json"), "utf8")).version;
const canonicalStudyRoot = await realpath(studyRoot);

// --------------------------------- 2. owned-output reset over a closed name set
const rendersDir = join(outputRoot, "renders");
const checksDir = join(outputRoot, "checks");
const knownChecks = new Set(["axe.json", "network.json", "answers.json", "lifecycle.json"]);
// Closed set: everything the full plan can emit plus whatever this invocation
// will emit. Nothing else is ever deleted, in the study or in a self-test root.
const clearableRenders = new Set([...fullPlan, ...plan].map(nameOf).map((n) => `${n}.png`));
// Two phases, deliberately. Both directories are inventoried in full before
// anything is removed, so an unexpected file in one cannot be discovered only
// after known outputs in the other have already been deleted.
const inventory = [];
for (const [label, dir, allowed] of [["renders", rendersDir, clearableRenders], ["checks", checksDir, knownChecks]]) {
  await mkdir(dir, { recursive: true });
  for (const name of await readdir(dir)) inventory.push({ label, dir, name, known: allowed.has(name) });
}
const unexpected = inventory.filter((entry) => !entry.known);
if (unexpected.length) {
  process.stderr.write(`refusing to touch files this run does not own (nothing was deleted):\n  ` +
    `${unexpected.map((e) => `${e.label}/${e.name}`).join("\n  ")}\n`);
  process.exit(1);
}
for (const entry of inventory) await rm(join(entry.dir, entry.name), { force: true });

// ----------------------------------------------------------------- 3. render
const taskRoot = await mkdtemp(join(tmpdir(), "cf-w2-render-"));
const marker = join(taskRoot, ".cf-w2-render-root");
const profile = join(taskRoot, "profile");
const results = [];
const remoteRequests = [];
const outsideStudyRequests = [];
const pageErrors = [];
const cleanupErrors = [];
let closeFallbackUsed = false;
let survivorInjected = false;
let cleanupProven = false;
let rootRemoved = false;
let failure;
let context;

try {
  await writeFile(marker, `${taskRoot}\n`, { mode: 0o600 });
  for (const sub of ["profile", "home", "appdata", "localappdata", "temp", "xdg-runtime"]) {
    await mkdir(join(taskRoot, sub), { recursive: true });
  }
  await chmod(join(taskRoot, "xdg-runtime"), 0o700);

  const allowed = ["PATH", "LANG", "LC_ALL", "SystemRoot", "WINDIR"];
  const env = Object.fromEntries(allowed.flatMap((k) => (process.env[k] === undefined ? [] : [[k, process.env[k]]])));
  Object.assign(env, {
    HOME: join(taskRoot, "home"), USERPROFILE: join(taskRoot, "home"),
    APPDATA: join(taskRoot, "appdata"), LOCALAPPDATA: join(taskRoot, "localappdata"),
    TMPDIR: join(taskRoot, "temp"), TMP: join(taskRoot, "temp"), TEMP: join(taskRoot, "temp"),
    XDG_RUNTIME_DIR: join(taskRoot, "xdg-runtime"),
    XDG_CONFIG_HOME: join(taskRoot, "home"), XDG_CACHE_HOME: join(taskRoot, "temp"),
    XDG_DATA_HOME: join(taskRoot, "home"), XDG_STATE_HOME: join(taskRoot, "home"),
  });

  context = await chromium.launchPersistentContext(profile, {
    executablePath, headless: true, env, viewport: VIEWPORTS.desktop,
    // The determinism flags exist so two consecutive normal runs can be compared
    // by digest. Without them Chromium's tiled raster path re-rasterises a
    // handful of anti-aliased edge and glyph pixels differently between runs —
    // no layout or content change, but enough to move every digest.
    args: ["--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-sync",
      "--disable-lcd-text", "--disable-partial-raster", "--disable-checker-imaging",
      "--disable-threaded-animation", "--disable-threaded-scrolling",
      "--disable-new-content-rendering-timeout", "--run-all-compositor-stages-before-draw",
      "--disable-image-animation-resync", "--force-device-scale-factor=1"],
  });
  refreshOwned(taskRoot);
  if (!ownedById.size) throw new Error("no task-owned browser process could be identified");

  if (injectSurvivor) {
    // Self-test only. The marker stays in the survivor's own argv: a shell
    // wrapper would exec away and lose it, which is how a real late child can
    // escape an identity set captured only once.
    const child = spawn(process.execPath, ["-e", `setTimeout(() => {}, 120000); // ${taskRoot}`],
      { detached: true, stdio: "ignore" });
    child.unref();
    await delay(400);
    refreshOwned(taskRoot);
    survivorInjected = true;
  }

  const browserVersion = context.browser()?.version() ?? "unknown";
  const page = context.pages()[0] ?? await context.newPage();
  page.setDefaultTimeout(NAV_TIMEOUT_MS);
  page.on("pageerror", (error) => pageErrors.push({ kind: "pageerror", text: error.message, at: page.url() }));
  page.on("console", (message) => {
    if (message.type() === "error") pageErrors.push({ kind: "console", text: message.text(), at: page.url() });
  });

  // file: is admitted only for canonical descendants of the canonical study
  // root. fileURLToPath gives platform-correct decoding; realpath collapses
  // symlinks, so a link inside the study that points outside it is rejected.
  await page.route("**/*", async (route) => {
    const url = route.request().url();
    if (url === "about:blank") return route.continue();
    if (!url.startsWith("file:")) {
      remoteRequests.push({ url, page: page.url() });
      return route.abort();
    }
    let requested;
    try { requested = fileURLToPath(url); }
    catch (error) {
      outsideStudyRequests.push({ url, page: page.url(), reason: `undecodable file URL: ${error.message}` });
      return route.abort();
    }
    let canonical;
    try { canonical = await realpath(requested); }
    catch {
      outsideStudyRequests.push({ url, page: page.url(), reason: "path does not resolve to an existing file" });
      return route.abort();
    }
    if (!isContained(canonicalStudyRoot, canonical, path)) {
      outsideStudyRequests.push({ url, page: page.url(), reason: "outside the canonical study root", canonical });
      return route.abort();
    }
    return route.continue();
  });

  for (const job of plan) {
    const name = nameOf(job);
    await page.setViewportSize(VIEWPORTS[job.viewport]);
    await page.emulateMedia({ colorScheme: job.mode, reducedMotion: job.motion });
    await page.goto(pathToFileURL(join(studyRoot, job.page)).href, { waitUntil: "load" });
    await page.waitForFunction(
      () => document.documentElement.dataset.ready === "true" || !document.querySelector("script"),
      null, { timeout: NAV_TIMEOUT_MS });
    // A page that says it is ready has still only queued its layout. Without a
    // settle, a small number of anti-aliased edge pixels rasterise differently
    // between otherwise identical runs, so two consecutive normal runs cannot be
    // compared by digest. Wait for fonts, then for two committed frames.
    await page.evaluate(async () => {
      await (document.fonts?.ready ?? Promise.resolve());
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
    await page.screenshot({ path: join(rendersDir, `${name}.png`), fullPage: true });

    const answer = await page.evaluate(() => document.documentElement.dataset.answer ?? null);
    await page.addScriptTag({ content: axeSource });
    const axeResult = await page.evaluate(async () => globalThis.axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"] },
    }));
    results.push({
      render: name, page: job.page, mode: job.mode, viewport: job.viewport, motion: job.motion,
      answer: answer ? JSON.parse(answer) : null,
      violations: axeResult.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.length })),
    });
    process.stdout.write(`${name}: ${results.at(-1).violations.length ? "AXE VIOLATION" : "axe clean"}\n`);
    await page.goto("about:blank");
  }

  await writeFile(join(checksDir, "axe.json"), `${JSON.stringify({
    browser_version: browserVersion, playwright_core_version: playwrightVersion, axe_core_version: axeVersion,
    results: results.map(({ answer, ...rest }) => rest),
  }, null, 2)}\n`);
  await writeFile(join(checksDir, "network.json"), `${JSON.stringify({
    policy: "file: admitted only for canonical descendants of the canonical study root; everything else aborted",
    study_root: studyRoot, canonical_study_root: canonicalStudyRoot,
    remote_requests: remoteRequests, outside_study_file_requests: outsideStudyRequests, page_errors: pageErrors,
  }, null, 2)}\n`);
  await writeFile(join(checksDir, "answers.json"), `${JSON.stringify({
    note: "the answer each candidate published at runtime; verify.mjs recomputes the expected answer from frontmatter",
    answers: results.filter((r) => r.answer).map((r) => ({ render: r.render, page: r.page, answer: r.answer })),
  }, null, 2)}\n`);
} catch (error) {
  failure = error;
} finally {
  if (injectInventoryFailure) inventoryFailureArmed = true;   // proof boundary only

  if (context) {
    try {
      await Promise.race([
        context.close(),
        new Promise((_, reject) => setTimeout(() => reject(new Error("bounded close expired")), CLOSE_TIMEOUT_MS)),
      ]);
    } catch (closeError) { cleanupErrors.push(closeError); }
  }

  // Positive proof only: the root is removed when an inventory completed and
  // showed zero live exact identities. Any inventory failure is uncertainty.
  let survivors = [];
  try {
    refreshOwned(taskRoot);
    survivors = liveOwned();
    if (survivors.length) {
      closeFallbackUsed = true;
      process.stderr.write(`${survivors.length} exact-owned identities outlived the close; terminating\n`);
      await terminateOwned(survivors);
      survivors = liveOwned();
    }
    cleanupProven = survivors.length === 0;
  } catch (cleanupError) {
    cleanupErrors.push(cleanupError);
    cleanupProven = false;
  }

  if (cleanupProven) {
    try { await rm(taskRoot, { recursive: true, force: true }); rootRemoved = true; }
    catch (removeError) { cleanupErrors.push(removeError); }
  } else {
    process.stderr.write(`retaining ${taskRoot}: task-owned process exit could not be proven\n`);
  }

  // Only ENOENT proves removal; anything else is uncertainty, never success.
  let rootState;
  try { await stat(taskRoot); rootState = "present"; }
  catch (error) {
    if (error.code === "ENOENT") rootState = "absent";
    else { rootState = "unknown"; cleanupErrors.push(new Error(`could not stat the task root: ${error.message}`)); }
  }

  let lifecycleWritten = true;
  try {
    await writeFile(join(checksDir, "lifecycle.json"), `${JSON.stringify({
      task_root: taskRoot,
      owned_process_identities: ownedById.size,
      survivor_injected: survivorInjected,
      inventory_failure_injected: injectInventoryFailure,
      close_fallback_used: closeFallbackUsed,
      cleanup_proven: cleanupProven,
      surviving_process_identities: survivors.length,
      task_root_removed: rootRemoved,
      task_root_state: rootState,
      task_root_remains: rootState !== "absent",
      xdg_runtime_dir_redirected: true,
      page_errors: pageErrors.length,
      remote_requests: remoteRequests.length,
      outside_study_file_requests: outsideStudyRequests.length,
      cleanup_errors: cleanupErrors.map((e) => e.message),
    }, null, 2)}\n`);
  } catch (writeError) {
    lifecycleWritten = false;
    process.stderr.write(`FAIL could not write lifecycle evidence: ${writeError.message}\n`);
  }

  const problems = [];
  if (failure) problems.push(`run failed: ${failure.message}`);
  cleanupErrors.forEach((e) => problems.push(`cleanup failed: ${e.message}`));
  if (!lifecycleWritten) problems.push("lifecycle evidence could not be written");
  if (!cleanupProven) problems.push("task-owned process exit could not be proven; the marked root was retained");
  if (closeFallbackUsed) problems.push("a normal render needed the exact-owned termination fallback");
  if (survivors.length) problems.push(`${survivors.length} task-owned process identities survived`);
  if (rootState !== "absent") problems.push(`the marked task root is ${rootState}`);
  if (pageErrors.length) problems.push(`${pageErrors.length} console or page error(s)`);
  if (remoteRequests.length) problems.push(`${remoteRequests.length} non-file request(s)`);
  if (outsideStudyRequests.length) problems.push(`${outsideStudyRequests.length} file request(s) outside the study root`);
  const violating = results.filter((r) => r.violations.length);
  if (violating.length) problems.push(`axe violations in ${violating.map((r) => r.render).join(", ")}`);

  process.stdout.write(`\n${results.length} renders · ${ownedById.size} owned identities, ${survivors.length} survivors · ` +
    `cleanup ${cleanupProven ? "proven" : "UNPROVEN"} · root ${rootState} · ` +
    `${pageErrors.length} page errors · ${remoteRequests.length} remote · ${outsideStudyRequests.length} outside-study\n`);
  if (problems.length) {
    problems.forEach((p) => process.stderr.write(`FAIL ${p}\n`));
    process.exitCode = 1;
  }
}

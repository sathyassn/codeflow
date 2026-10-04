import { randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { access, cp, mkdir, mkdtemp, readdir, readFile, rm, rmdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import axe from "axe-core";
import { validateWindowsQualificationConfinement } from "./windows-qualification-scope.mjs";
import { assertNoPolicyViolations, recordPolicyViolations } from "./csp-violations.mjs";
import { codeflowBinary } from "./codeflow-binary.mjs";
import { closeWaitMs } from "./browser-close-bound.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = codeflowBinary(repoRoot);
await access(codeflow);
// A figure block the grammar draws with no rule failure (the portal's state specimen).
const figureDeclaration = JSON.parse(await readFile(join(repoRoot, "docs-portal/tests/fixtures/figures/05-state.json"), "utf8"));
// A revision a pre-removal build stored with diagram blocks (TSK-087).
const retiredRevision = JSON.parse(await readFile(join(repoRoot, "crates/codeflow-present/tests/fixtures/retired-diagram/revision.json"), "utf8"));

const functionalEnvironmentNames = [
  "APPDATA", "CARGO_HOME", "COMSPEC", "HOME", "LANG", "LC_ALL", "LC_CTYPE",
  "LOCALAPPDATA", "PATHEXT", "PATH", "RUSTUP_HOME", "SystemRoot", "SYSTEMROOT", "TEMP", "TERM",
  "TMP", "TMPDIR", "USERPROFILE", "WINDIR", "XDG_RUNTIME_DIR",
];
const runPrefix = process.env.CF_PRESENT_RUN_PREFIX ?? "tsk007";
if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(runPrefix)) {
  throw new Error(`Invalid qualification run prefix: ${runPrefix}`);
}
const injectCleanupFailure = process.argv.includes("--inject-cleanup-failure");
const injectCloseTimeout = process.argv.includes("--inject-close-timeout");
const injectSlowClose = process.argv.includes("--inject-slow-close");
const NAVIGATION_TIMEOUT_MS = 45_000;
const BOOTSTRAP_COMMIT_TIMEOUT_MS = 120_000;
// A healthy close returns in well under a second. A busy macOS host (load
// average 50 to 100) was measured at 10 to 20 seconds for a close that then
// succeeded, and one at load 200 took over 25, so the bound has to separate a
// slow close from a hung one, not a quiet machine from a busy one. Only a
// hung close earns the exact-owned fallback. closeWaitMs trims the wait when
// the run is late, so teardown still ends before the parent check's kill.
const BROWSER_CLOSE_TIMEOUT_MS = 60_000;
// The injected hang never settles; this bound only ends the wait for it.
const INJECTED_CLOSE_HANG_BOUND_MS = 1_000;
// A close that takes at least this long, longer than the former 15 s bound and
// well inside the current one, which must not use the fallback. It overlaps the
// real close, so a slow host adds nothing to it.
const INJECTED_SLOW_CLOSE_MS = 20_000;
const BROWSER_TERMINATION_GRACE_MS = 2_500;
const PROCESS_INVENTORY_TIMEOUT_MS = 10_000;
// A busy machine lists more than execFileSync's 1 MiB default; the inventory
// must not fail on the length of other processes' command lines.
const PROCESS_INVENTORY_MAX_BUFFER = 64 * 1024 * 1024;

class BoundedTimeoutError extends Error {
  constructor(timeout, phase) {
    super(`Timed out after ${timeout}ms: ${phase}`);
    this.name = "BoundedTimeoutError";
  }
}
const runId = `${runPrefix}-${process.pid}-${randomUUID()}`;
const windowsProfileConfinement = qualifyWindowsEnvironment();
// Keep the root name short: Chromium puts its singleton socket under TMPDIR
// (this root's tmp/) and aborts at launch when that Unix socket path exceeds
// 107 bytes on Linux. mkdtemp's suffix keeps the root unique; the marker keeps
// the full runId.
const runRoot = await mkdtemp(join(tmpdir(), `${runPrefix}-${process.pid}-`));
const marker = join(runRoot, ".cf-present-qualification-root");
const project = join(runRoot, "project");
const home = join(runRoot, "home");
const userProfile = join(runRoot, "user-profile");
const localAppData = join(runRoot, "local-app-data");
const appData = join(runRoot, "app-data");
const xdgState = join(runRoot, "xdg-state");
const taskTemp = join(runRoot, "tmp");
const browserProfileV1 = join(runRoot, "browser-profile-v1");
const browserProfileV2 = join(runRoot, "browser-profile-v2");
const output = join(runRoot, "output");
const trace = join(runRoot, "trace");
const qualificationDeadline = Date.now() + 180_000;
const evidenceDestination = process.env.CF_PRESENT_EVIDENCE_DIR
  ? resolve(process.env.CF_PRESENT_EVIDENCE_DIR)
  : null;
const childEnvironment = {
  ...allowedEnvironment(functionalEnvironmentNames),
  HOME: home,
  USERPROFILE: userProfile,
  LOCALAPPDATA: localAppData,
  APPDATA: appData,
  XDG_STATE_HOME: xdgState,
  TMPDIR: taskTemp,
  TMP: taskTemp,
  TEMP: taskTemp,
  NODE_DISABLE_COMPILE_CACHE: "1",
  // A service this check starts exits once this process is gone.
  CF_PRESENT_OWNER_PID: String(process.pid),
  AWS_SECRET_ACCESS_KEY: "aws-real-browser-canary",
  AWS_SESSION_TOKEN: "aws-session-real-browser-canary",
  OPENAI_API_KEY: "openai-real-browser-canary",
  ANTHROPIC_API_KEY: "anthropic-real-browser-canary",
  ANTHROPIC_AUTH_TOKEN: "anthropic-auth-real-browser-canary",
};
const browserEnvironment = allowedEnvironment([
  "APPDATA", "HOME", "LANG", "LC_ALL", "LOCALAPPDATA", "PATH", "SYSTEMROOT",
  "TEMP", "TMP", "TMPDIR", "USERPROFILE", "WINDIR", "XDG_RUNTIME_DIR",
]);
browserEnvironment.HOME = home;
browserEnvironment.USERPROFILE = userProfile;
browserEnvironment.LOCALAPPDATA = localAppData;
browserEnvironment.APPDATA = appData;
browserEnvironment.TMPDIR = taskTemp;
browserEnvironment.TMP = taskTemp;
browserEnvironment.TEMP = taskTemp;

assertChildEnvironmentDoesNotInheritUnrelatedHostValues();

let context;
let sessionId;
let port;
let servicePid;
let serviceInstance;
let serviceFingerprint;
let result;
let primaryError;
let tracingStarted = false;
let browserExecutable;
let browserVersion;
let playwrightCoreVersion;
let browserCloseFallbacks = 0;
let closeTimeoutInjectionPending = injectCloseTimeout;
let slowCloseInjectionPending = injectSlowClose;
const requests = [];
const responses = [];
const consoleErrors = [];
const observerTasks = [];
try {
  await writeFile(marker, `${runId}\n`, { mode: 0o600 });
  await Promise.all([
    mkdir(project),
    mkdir(home),
    mkdir(userProfile),
    mkdir(localAppData),
    mkdir(appData),
    mkdir(xdgState),
    mkdir(browserProfileV1),
    mkdir(browserProfileV2),
    mkdir(output),
    mkdir(trace),
    mkdir(taskTemp),
  ]);
  browserExecutable = await findBrowser();
  playwrightCoreVersion = JSON.parse(
    await readFile(join(webRoot, "node_modules", "playwright-core", "package.json"), "utf8"),
  ).version;
  if (!playwrightCoreVersion) throw new Error("Could not record the Playwright qualification version");
  run("git", ["init", "--quiet"], project);

  const firstDocument = join(project, "review-v1.json");
  const secondDocument = join(project, "review-v2.json");
  await writeFile(firstDocument, `${JSON.stringify(documentFixture("First revision"), null, 2)}\n`);
  await writeFile(secondDocument, `${JSON.stringify(documentFixture("Second revision"), null, 2)}\n`);

  const opened = run(codeflow, ["present", "open", firstDocument, "--no-launch"], project);
  sessionId = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const bootstrapPath = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  if (!sessionId || !bootstrapPath) throw new Error(`Could not parse presentation open output: ${opened}`);
  const listed = JSON.parse(run(codeflow, ["present", "list"], project));
  servicePid = listed[0]?.service_pid;
  port = listed[0]?.service_port;
  serviceInstance = listed[0]?.service_instance;
  if (!Number.isInteger(servicePid) || !Number.isInteger(port) || !serviceInstance) {
    throw new Error(`Presentation list omitted its service identity: ${JSON.stringify(listed)}`);
  }
  serviceFingerprint = processFingerprint(servicePid);
  if (!serviceFingerprint) throw new Error(`Presentation service ${servicePid} had no observable process identity`);

  context = await launchBrowser(browserProfileV1);
  recordBrowserVersion(context);
  observeContext(context, requests, responses, consoleErrors, observerTasks);
  const browserProcessesV1 = await waitForProfileProcess(browserProfileV1);
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  tracingStarted = true;
  const page = context.pages()[0] ?? await context.newPage();
  await openAuthenticatedPresentation(page, bootstrapPath, port);
  const initialText = await page.locator("body").innerText();
  if (!initialText.includes("First revision")) {
    throw new Error(`Authenticated application omitted its document text at ${page.url()}: ${initialText.slice(0, 2_000)}`);
  }
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  const isolated = await page.evaluate(() => {
    const chrome = document.querySelector("#cf-present-chrome");
    const control = document.querySelector("#cf-comment-toggle");
    const box = control.getBoundingClientRect();
    const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    const authored = document.querySelector(".isolation-label");
    return getComputedStyle(chrome).display !== "none"
      && control.contains(hit)
      && getComputedStyle(authored).fontWeight === "700";
  });
  if (!isolated) throw new Error("Authored CSS hid or overlaid review chrome, or valid scoped styling failed");
  await page.locator("[data-cf-figure-block]").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-figure-block]")?.getAttribute("data-cf-figure-block") === "ready");
  await page.locator("code[data-cf-language='rust']").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready");
  if (injectCleanupFailure) {
    throw new Error("injected real-browser cleanup failure");
  }

  for (const mode of ["light", "dark"]) {
    // Pass10 chrome: appearance lives behind the Settings panel as pills.
    await page.getByTestId("settings-btn").click();
    await page.getByTestId("settings-panel")
      .getByRole("button", { name: mode === "light" ? "Light" : "Dark", exact: true })
      .click();
    await page.keyboard.press("Escape");
    await page.getByTestId("settings-panel").waitFor({ state: "detached" });
    // browser-check owns the shared chrome's WCAG checks across skins and
    // appearances. This real pass records launch, persisted feedback and cleanup.
    await page.screenshot({ path: join(output, `review-${mode}.png`), fullPage: true });
  }

  // Single Comment mode: arm, then use the rail's secondary tools; each
  // capture opens the composer, and the note body is saved per capture.
  await page.locator("#cf-comment-toggle").click();
  await page.locator(".cf-hint.on").waitFor();
  const dock = page.getByTestId("notes-dock");
  if (!await dock.isVisible()) {
    throw new Error(
      `Authenticated review controls did not mount; console=${JSON.stringify(consoleErrors)}, responses=${JSON.stringify(responses)}`,
    );
  }
  await dock.locator(".cf-tools summary").click();
  const saveComposerNote = async (body) => {
    await page.getByTestId("composer").waitFor();
    await page.getByTestId("composer-text").fill(body);
    await page.getByTestId("composer-save").click();
    await page.getByTestId("composer").waitFor({ state: "detached" });
  };

  await page.getByRole("button", { name: "Pick element" }).click();
  await page.locator("code[data-cf-language='rust']").click();
  await saveComposerNote("Keep the implementation example aligned with the verified contract.");

  await page.getByRole("button", { name: "Select area" }).click();
  const regionTarget = page.locator("[data-cf-block-id='flow'] [data-cf-figure-block]");
  await regionTarget.scrollIntoViewIfNeeded();
  const regionBox = await regionTarget.boundingBox();
  if (!regionBox) throw new Error("Real-browser region target is not visible");
  await page.mouse.move(regionBox.x + 12, regionBox.y + 12);
  await page.mouse.down();
  await page.mouse.move(
    regionBox.x + Math.min(120, regionBox.width - 12),
    regionBox.y + Math.min(80, regionBox.height - 12),
  );
  await page.mouse.up();
  await saveComposerNote("Preserve this visual relationship in the next revision.");

  await page.getByRole("button", { name: "Whole document" }).click();
  await saveComposerNote("Review the complete document hierarchy before approval.");

  await dock.locator(".cf-note-row .a").getByText("Whole document", { exact: true }).waitFor();
  const pendingMarkers = await page.locator(".cf-marker").count();
  if (pendingMarkers !== 3) {
    throw new Error(`Real review retained ${pendingMarkers} target markers, expected 3`);
  }
  await page.getByLabel("Verdict").selectOption("approve_with_notes");
  await page.getByRole("button", { name: "Submit review" }).click();
  await page.getByRole("status").getByText(/Review received/u).waitFor();
  await assertNoPolicyViolations(page, "first revision");
  const delivered = JSON.parse(run(codeflow, ["present", "feedback", sessionId], project).trim());
  if (delivered.verdict !== "approve_with_notes" || delivered.notes?.length !== 3) {
    throw new Error(`The real browser review was incomplete: ${JSON.stringify(delivered)}`);
  }
  const targets = delivered.notes.map((note) => (
    note.element_selector ? "element" : note.region_selector ? "region" : note.selector ? "text" : "block"
  ));
  if (targets.join(",") !== "element,region,region") {
    throw new Error(`The real browser review delivered unexpected target types: ${targets.join(",")}`);
  }
  if (delivered.notes[2].region_selector.scope !== "document") {
    throw new Error("Whole-document feedback did not retain document scope");
  }
  for (const context of ["First revision", "Real service Authenticated loopback journey", "qualified", "Approve or request"]) {
    if (!delivered.notes[2].excerpt?.text?.includes(context)) {
      throw new Error(`Whole-document feedback lost ${context} context`);
    }
  }
  if (!delivered.notes[0].excerpt?.text?.includes("qualified")) {
    throw new Error("Element feedback lost visible code context needed by the consuming harness");
  }
  // Retain this synthetic fixture's actual CLI envelope so a reviewing native
  // harness can inspect the same feedback, not merely a test's pass summary.
  await writeFile(join(output, "feedback-envelope.json"), `${JSON.stringify(delivered, null, 2)}\n`, { mode: 0o600 });
  run(codeflow, [
    "present", "resolve", sessionId, delivered.event_id,
    "--event-version", "2", "--status", "addressed",
  ], project);

  run(codeflow, ["present", "update", sessionId, secondDocument], project);
  await page.getByTestId("toast").getByText(/A newer document revision is available/u).waitFor();
  await page.screenshot({ path: join(output, "revision-notice.png"), fullPage: true });
  await bounded(
    context.tracing.stop({ path: join(trace, "revision-v1.zip") }),
    15_000,
    "stop revision-one trace",
  );
  tracingStarted = false;
  browserCloseFallbacks += await closeBrowser(context, browserProfileV1, browserProcessesV1);
  context = undefined;
  const shown = run(codeflow, ["present", "show", sessionId, "--no-launch"], project);
  const secondBootstrapPath = shown.match(/owner-private bootstrap file (.+?) in the isolated profile/u)?.[1];
  if (!secondBootstrapPath) throw new Error(`Could not parse presentation show output: ${shown}`);

  context = await launchBrowser(browserProfileV2);
  recordBrowserVersion(context);
  observeContext(context, requests, responses, consoleErrors, observerTasks);
  const browserProcessesV2 = await waitForProfileProcess(browserProfileV2);
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  tracingStarted = true;
  const revisedPage = context.pages()[0] ?? await context.newPage();
  await openAuthenticatedPresentation(revisedPage, secondBootstrapPath, port);
  await revisedPage.locator("#cf-present-document").getByText("Second revision").waitFor();
  const exportPath = join(output, "review.html");
  run(codeflow, ["present", "export", sessionId, "--out", exportPath, "--theme", "graphite", "--mode", "dark"], project);
  const exportPage = await context.newPage();
  await exportPage.goto(pathToFileURL(exportPath).href);
  await exportPage.locator("#cf-present-document").getByText("Second revision").waitFor();
  await exportPage.waitForFunction(() => (
    document.querySelector("[data-cf-figure-block]")?.getAttribute("data-cf-figure-block") === "ready"
  ));
  await exportPage.waitForFunction(() => (
    document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready"
  ));
  const exportAccessibility = await exportPage.evaluate(async () => globalThis.axe.run(document, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] },
  }));
  if (exportAccessibility.violations.length) {
    throw new Error(`export accessibility violations: ${exportAccessibility.violations.map((item) => item.id).join(", ")}`);
  }
  await exportPage.screenshot({ path: join(output, "export-dark.png"), fullPage: true });
  await assertNoPolicyViolations(exportPage, "offline export");
  await assertNoPolicyViolations(revisedPage, "second revision");
  await delay(100);
  await Promise.all(observerTasks);
  await exportPage.close();

  // A stored diagram revision (TSK-087): the served page and a new export show
  // the conversion notice and each escaped source, and draw nothing.
  const revisionPath = await findRevisionFile(runRoot, sessionId, 2);
  await writeFile(revisionPath, `${JSON.stringify({ ...retiredRevision, revision: 2 }, null, 2)}\n`, { mode: 0o600 });
  const retiredRequestStart = requests.length;
  await revisedPage.reload({ waitUntil: "load" });
  await assertRetiredPage(revisedPage, "served retired revision");
  const retiredScripts = requests.slice(retiredRequestStart).filter((request) => /\/app\/assets\/.+\.js$/u.test(request));
  if (retiredScripts.length) throw new Error(`The retired revision page requested scripts: ${retiredScripts.join(", ")}`);
  await revisedPage.screenshot({ path: join(output, "retired-revision.png"), fullPage: true });
  const retiredExportPath = join(output, "retired.html");
  run(codeflow, ["present", "export", sessionId, "--out", retiredExportPath], project);
  const retiredExportPage = await context.newPage();
  await retiredExportPage.goto(pathToFileURL(retiredExportPath).href);
  await assertRetiredPage(retiredExportPage, "exported retired revision");
  await retiredExportPage.close();

  if (requests.some(isRemoteRequest)) {
    throw new Error(`Real presentation attempted remote network: ${requests.join(", ")}`);
  }
  if (consoleErrors.length) throw new Error(`Real presentation console errors: ${consoleErrors.join(" | ")}`);

  await bounded(
    context.tracing.stop({ path: join(trace, "revision-v2-and-export.zip") }),
    15_000,
    "stop revision-two trace",
  );
  tracingStarted = false;
  browserCloseFallbacks += await closeBrowser(context, browserProfileV2, browserProcessesV2);
  context = undefined;
  if (!injectCloseTimeout && browserCloseFallbacks > 0) {
    throw new Error(
      `Browser close required ${browserCloseFallbacks} exact-owned termination fallback(s)`,
    );
  }
  run(codeflow, ["present", "close", sessionId], project);
  await assertPortClosed(port);
  await waitForProcessIdentityToDisappear(servicePid, serviceFingerprint);
  run(codeflow, ["present", "clear", sessionId, "--older-than", "0h"], project);
  const after = JSON.parse(run(codeflow, ["present", "list"], project));
  if (after.length !== 0) throw new Error(`Presentation state remained after clear: ${JSON.stringify(after)}`);

  result = {
    schema_version: 1,
    run_id: runId,
    platform: `${process.platform}-${process.arch}`,
    headless: true,
    toolchain: {
      browser_version: browserVersion,
      playwright_core_version: playwrightCoreVersion,
    },
    task_owned_resources: {
      project,
      home,
      browser_profiles: [browserProfileV1, browserProfileV2],
      service_port: port,
      service_pid: servicePid,
      service_instance: serviceInstance,
      data_namespace: runId,
      environment_roots: { home, userProfile, localAppData, appData, xdgState, taskTemp },
      windows_profile_confinement: windowsProfileConfinement,
      output,
      trace,
    },
    checks: {
      real_service_bootstrap: "pass",
      declarative_rendering: "pass",
      accessibility_light_dark: "pass",
      syntax_and_figure: "pass",
      retired_diagram_revision_page_and_export: "pass",
      csp_violations: "none observed",
      element_region_and_document_feedback_delivery: "pass",
      feedback_delivery_and_resolution: "pass",
      immutable_revision_update: "pass",
      offline_export: "pass",
      remote_network: "none observed",
      console_errors: "none observed",
      service_port_teardown: "pass",
      service_process_identity_teardown: "pass",
      browser_process_and_profile_teardown: "pass",
      browser_close_fallbacks: browserCloseFallbacks,
      session_clear: "pass",
      windows_profile_confinement: windowsProfileConfinement ? "pass" : "not applicable",
      windows_profile_or_vm_teardown: windowsProfileConfinement
        ? "not evaluated; outer native-Windows lane required"
        : "not applicable",
    },
  };
  await writeFile(join(output, "results.json"), `${JSON.stringify(result, null, 2)}\n`);
  if (evidenceDestination) {
    await mkdir(evidenceDestination, { recursive: true });
    await cp(output, join(evidenceDestination, "output"), { recursive: true, force: true });
    await cp(trace, join(evidenceDestination, "trace"), { recursive: true, force: true });
  }
} catch (error) {
  primaryError = error;
} finally {
  const cleanupErrors = [];
  const attempt = async (phase, operation) => {
    try {
      await operation();
    } catch (error) {
      cleanupErrors.push(new Error(`Cleanup failed during ${phase}: ${error.message}`, { cause: error }));
    }
  };
  let ownedBrowserProcesses = [];
  await attempt("capture owned browser identities", async () => {
    ownedBrowserProcesses = mergeProcesses(
      browserProcessTree(browserProfileV1),
      browserProcessTree(browserProfileV2),
    );
  });
  if (context && tracingStarted) {
    await attempt("stop failed browser trace", async () => {
      await bounded(
        context.tracing.stop({ path: join(trace, "failure.zip") }),
        15_000,
        "stop failed browser trace",
      );
      tracingStarted = false;
    });
  }
  if (context) {
    await attempt("close failed browser context", async () => {
      const closed = await closeBrowserContext(
        context,
        [browserProfileV1, browserProfileV2],
        ownedBrowserProcesses,
        "close failed browser context",
      );
      ownedBrowserProcesses = mergeProcesses(ownedBrowserProcesses, closed.processes);
      context = undefined;
    });
  }
  await attempt("capture late owned browser identities", async () => {
    ownedBrowserProcesses = mergeProcesses(
      ownedBrowserProcesses,
      browserProcessTree(browserProfileV1),
      browserProcessTree(browserProfileV2),
    );
  });
  await attempt("wait for owned browser identities", async () => {
    await waitForProcessIdentitiesToDisappear(ownedBrowserProcesses, "failed browser run");
  });
  for (const profile of [browserProfileV1, browserProfileV2]) {
    await attempt(`wait for profile processes using ${profile}`, async () => {
      await waitForNoProfileProcesses(profile);
    });
  }
  if (!result && evidenceDestination) {
    await attempt("copy failure evidence", async () => {
      if (!await pathExists(output)) return;
      await mkdir(evidenceDestination, { recursive: true });
      await cp(output, join(evidenceDestination, "failure-output"), { recursive: true, force: true });
      if (await pathExists(trace)) {
        await cp(trace, join(evidenceDestination, "failure-trace"), { recursive: true, force: true });
      }
    });
  }
  if (sessionId && !result) {
    await attempt("close presentation session", async () => {
      run(codeflow, ["present", "close", sessionId], project, false);
    });
    if (port) {
      await attempt("wait for presentation port", async () => assertPortClosed(port));
    }
    if (servicePid && serviceFingerprint) {
      await attempt("wait for presentation service identity", async () => {
        await waitForProcessIdentityToDisappear(servicePid, serviceFingerprint);
      });
    }
    await attempt("clear presentation session", async () => {
      run(codeflow, ["present", "clear", sessionId, "--older-than", "0h"], project, false);
    });
  }
  await attempt("remove marked qualification root", async () => {
    const markerValue = await readFile(marker, "utf8").catch(() => "");
    if (markerValue === `${runId}\n`) {
      await rm(runRoot, { recursive: true, force: false });
    } else if (markerValue === "" && await pathExists(runRoot) && (await readdir(runRoot)).length === 0) {
      // mkdtemp can succeed immediately before marker creation fails. Only an
      // empty, task-namespaced root is safe to remove without the marker.
      await rmdir(runRoot);
    } else if (await pathExists(runRoot)) {
      throw new Error(`Refusing to clean unmarked or foreign qualification root ${runRoot}`);
    }
    if (await pathExists(runRoot)) throw new Error(`Qualification root remained after cleanup: ${runRoot}`);
  });
  await attempt("post-close qualification quiescence", async () => {
    await delay(2_000);
    if (await pathExists(runRoot)) {
      throw new Error(`Qualification root was recreated during post-close quiescence: ${runRoot}`);
    }
  });
  if (cleanupErrors.length > 0) {
    const failures = primaryError ? [primaryError, ...cleanupErrors] : cleanupErrors;
    throw new AggregateError(
      failures,
      "cf-present qualification failed; every exact-owned cleanup phase was attempted",
      primaryError ? { cause: primaryError } : undefined,
    );
  }
}
if (primaryError) throw primaryError;
if (result) {
  result.checks.task_owned_temp_and_artifact_cleanup = "pass";
  if (evidenceDestination) {
    await writeFile(
      join(evidenceDestination, "output", "results.json"),
      `${JSON.stringify(result, null, 2)}\n`,
    );
  }
  process.stdout.write(`cf-present real browser passed: ${sessionId}, port ${port}, isolated teardown verified\n`);
}

function documentFixture(revisionText) {
  return {
    schema_version: 1,
    title: "Presentation qualification",
    language: "en",
    provenance: { task_id: "TSK-007", spec_id: "SPC-004", adr_id: "ADR-0052" },
    blocks: [
      { type: "narrative", id: "outcome", markdown: revisionText },
      { type: "status", id: "evidence", items: [
        { label: "Real service", state: "pass", detail: "Authenticated loopback journey" },
        { label: "Native Windows", state: "not_run", detail: "Never inferred from this macOS run" },
      ] },
      { type: "code", id: "code", language: "rust", code: "fn qualified() -> bool { true }", caption: "Qualification example" },
      { type: "figure", id: "flow", declaration: figureDeclaration },
      { type: "feedback_prompt", id: "decision", prompt: "Approve or request a concrete change." },
      { type: "html", id: "css-isolation", title: "CSS isolation", html: "<style>body, #cf-present-chrome { display:none } .isolation-label { font-weight:700 } .isolation-overlay { position:fixed; inset:0; z-index:2147483647 }</style><p class='isolation-label'>Valid authored styling stays local.</p><div class='isolation-overlay' aria-hidden='true'></div>" },
    ],
  };
}

function run(executable, args, cwd, enforceDeadline = true) {
  if (enforceDeadline) assertWithinDeadline(`run ${executable}`);
  return execFileSync(executable, args, {
    cwd,
    env: childEnvironment,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 30_000,
  });
}

async function assertPortClosed(candidatePort) {
  const deadline = Date.now() + 10_000;
  while (await portAcceptsConnections(candidatePort)) {
    if (Date.now() >= deadline) {
      throw new Error(`Presentation listener ${candidatePort} remained after close`);
    }
    await delay(100);
  }
}

async function portAcceptsConnections(candidatePort) {
  const { connect } = await import("node:net");
  return await new Promise((resolvePromise) => {
    const socket = connect({ host: "127.0.0.1", port: candidatePort });
    const timer = setTimeout(() => {
      socket.destroy();
      resolvePromise(false);
    }, 1_000);
    socket.once("connect", () => {
      clearTimeout(timer);
      socket.destroy();
      resolvePromise(true);
    });
    socket.once("error", () => {
      clearTimeout(timer);
      resolvePromise(false);
    });
  });
}

async function launchBrowser(profile) {
  assertWithinDeadline("launch browser");
  const launched = await chromium.launchPersistentContext(profile, {
    executablePath: browserExecutable,
    headless: true,
    viewport: { width: 1280, height: 900 },
    env: browserEnvironment,
    // Never pass --disable-crashpad-for-testing: on Linux Chromium it makes
    // the network service abort with an FD ownership violation and restart in
    // a tight loop, so no navigation commits and the runner is starved.
    args: [
      "--disable-background-networking",
      "--disable-component-update",
      "--disable-breakpad",
      "--disable-crash-reporter",
      "--disable-default-apps",
      "--disable-sync",
      "--no-default-browser-check",
      "--noerrdialogs",
      "--no-first-run",
    ],
  });
  await launched.addInitScript({ content: axe.source });
  await recordPolicyViolations(launched);
  return launched;
}

async function findRevisionFile(root, id, revision) {
  const name = `${String(revision).padStart(20, "0")}.json`;
  const walk = async (directory) => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const path = join(directory, entry.name);
      if (entry.name === id && directory.endsWith("sessions")) return join(path, "revisions", name);
      const found = await walk(path);
      if (found) return found;
    }
    return null;
  };
  const found = await walk(root);
  if (!found) throw new Error(`No stored revision ${revision} for session ${id}`);
  return found;
}

async function assertRetiredPage(page, label) {
  const evidence = await page.evaluate(() => ({
    notice: document.querySelector("aside.version-warning")?.textContent ?? "",
    kept: document.querySelector("main")?.textContent ?? "",
    sources: [...document.querySelectorAll("main pre code")].map((code) => code.textContent),
    hooks: document.querySelectorAll("[data-cf-diagram], [data-cf-figure-block], template").length,
    scripts: document.querySelectorAll("script").length,
  }));
  if (!evidence.notice.includes("This revision holds a diagram block, which was removed with Mermaid")) {
    throw new Error(`${label} omitted the conversion notice: ${JSON.stringify(evidence)}`);
  }
  // T114-1: every other block renders as it always did.
  for (const kept of [retiredRevision.content.document.blocks[0].markdown, retiredRevision.content.document.blocks[2].summary]) {
    if (!evidence.kept.includes(kept)) throw new Error(`${label} omitted ${JSON.stringify(kept)}: ${JSON.stringify(evidence)}`);
  }
  const expected = [retiredRevision.content.document.blocks[1].source, retiredRevision.content.document.blocks[2].blocks[0].source];
  if (JSON.stringify(evidence.sources) !== JSON.stringify(expected)) {
    throw new Error(`${label} did not show each diagram source as text: ${JSON.stringify(evidence)}`);
  }
  if (evidence.hooks || evidence.scripts) throw new Error(`${label} carries a drawing hook or script: ${JSON.stringify(evidence)}`);
  await assertNoPolicyViolations(page, label);
}

function recordBrowserVersion(openContext) {
  const observedBrowserVersion = openContext.browser()?.version();
  if (!observedBrowserVersion) {
    throw new Error("Could not record the driven browser version");
  }
  if (browserVersion && browserVersion !== observedBrowserVersion) {
    throw new Error(
      `Driven browser version changed during qualification: ${browserVersion} to ${observedBrowserVersion}`,
    );
  }
  browserVersion = observedBrowserVersion;
}

async function openAuthenticatedPresentation(page, bootstrapPath, servicePort) {
  // The private file immediately submits into the loopback service. Waiting
  // for its full load event conflates that handoff with the destination page
  // and can hang behind long-lived application requests. A cold renderer on a
  // loaded shared runner gets more startup room, still capped by the journey's
  // whole 180-second deadline.
  await page.goto(pathToFileURL(bootstrapPath).href, {
    waitUntil: "commit",
    timeout: timeoutWithinQualification(BOOTSTRAP_COMMIT_TIMEOUT_MS, "bootstrap document commit"),
  });
  await page.waitForURL(
    new RegExp(`^http://127\\.0\\.0\\.1:${servicePort}/app/`, "u"),
    {
      waitUntil: "domcontentloaded",
      timeout: timeoutWithinQualification(NAVIGATION_TIMEOUT_MS, "authenticated application readiness"),
    },
  );
}

function observeContext(
  observedContext,
  observedRequests,
  observedResponses,
  observedConsoleErrors,
  observedTasks,
) {
  const observedPages = new WeakSet();
  const observePage = (observedPage) => {
    if (observedPages.has(observedPage)) return;
    observedPages.add(observedPage);
    observedPage.setDefaultTimeout(15_000);
    // A cold shared runner may still be CPU-bound after the qualification's
    // release-build rehearsal. Keep navigation bounded, but leave enough room
    // for the local bootstrap and application assets to complete under load.
    observedPage.setDefaultNavigationTimeout(NAVIGATION_TIMEOUT_MS);
    observedPage.on("console", (message) => {
      if (message.type() === "error") {
        const location = message.location();
        const task = Promise.all(message.args().map(async (argument) => {
          try {
            return await argument.jsonValue();
          } catch {
            return argument.toString();
          }
        })).then((args) => {
          observedConsoleErrors.push(
            `${observedPage.url()}:${location.lineNumber ?? 0}:${location.columnNumber ?? 0}: `
              + `${message.type()} ${message.text()} args=${JSON.stringify(args)}`,
          );
        });
        observedTasks.push(task);
      }
    });
    observedPage.on("pageerror", (error) => {
      const details = Object.fromEntries(Object.getOwnPropertyNames(error).map((name) => [name, error[name]]));
      observedConsoleErrors.push(
        `${observedPage.url()}: pageerror ${error.stack || error.message || String(error)} details=${JSON.stringify(details)}`,
      );
    });
  };
  observedContext.on("request", (request) => observedRequests.push(request.url()));
  observedContext.on("response", (response) => observedResponses.push(`${response.status()} ${response.url()}`));
  observedContext.on("requestfailed", (request) => {
    observedConsoleErrors.push(
      `requestfailed ${request.url()} ${request.failure()?.errorText ?? "unknown failure"}`,
    );
  });
  observedContext.on("page", observePage);
  for (const observedPage of observedContext.pages()) observePage(observedPage);
}

async function closeBrowser(openContext, profile, knownProcesses) {
  const closed = await closeBrowserContext(
    openContext,
    [profile],
    knownProcesses,
    `close browser using ${profile}`,
  );
  await rm(profile, { recursive: true, force: false });
  if (await pathExists(profile)) throw new Error(`Owned browser profile remained after cleanup: ${profile}`);
  return closed.usedFallback ? 1 : 0;
}

async function closeBrowserContext(openContext, profiles, knownProcesses, phase) {
  let ownedProcesses = mergeProcesses(
    knownProcesses,
    ...profiles.map((profile) => browserProcessTree(profile)),
  );
  const injectTimeout = closeTimeoutInjectionPending && phase.startsWith("close browser using ");
  if (injectTimeout) closeTimeoutInjectionPending = false;
  const injectSlow = !injectTimeout
    && slowCloseInjectionPending
    && phase.startsWith("close browser using ");
  if (injectSlow) slowCloseInjectionPending = false;
  const closePromise = injectTimeout
    ? new Promise(() => {})
    : injectSlow
      ? Promise.all([delay(INJECTED_SLOW_CLOSE_MS), openContext.close()]).then(() => undefined)
      : openContext.close();
  let usedFallback = false;
  try {
    await bounded(
      closePromise,
      injectTimeout
        ? INJECTED_CLOSE_HANG_BOUND_MS
        : closeWaitMs(BROWSER_CLOSE_TIMEOUT_MS, qualificationDeadline - Date.now()),
      phase,
    );
  } catch (error) {
    ownedProcesses = mergeProcesses(
      ownedProcesses,
      ...profiles.map((profile) => browserProcessTree(profile)),
    );
    let terminationError;
    try {
      await terminateOwnedProcessIdentities(ownedProcesses, phase);
    } catch (candidate) {
      terminationError = candidate;
    }
    usedFallback = true;
    if (terminationError) {
      throw new AggregateError(
        [error, terminationError],
        `${phase} failed and exact-owned browser termination also failed`,
        { cause: error },
      );
    }
    if (!(error instanceof BoundedTimeoutError)) throw error;
    await Promise.race([closePromise.catch(() => undefined), delay(1_000)]);
  }
  ownedProcesses = mergeProcesses(
    ownedProcesses,
    ...profiles.map((profile) => browserProcessTree(profile)),
  );
  await waitForProcessIdentitiesToDisappear(ownedProcesses, phase);
  for (const profile of profiles) await waitForNoProfileProcesses(profile);
  return { processes: ownedProcesses, usedFallback };
}

async function waitForProfileProcess(profile) {
  const deadline = Date.now() + 10_000;
  let processes = browserProcessTree(profile);
  while (processes.length === 0) {
    if (Date.now() >= deadline) throw new Error(`No owned browser process used profile ${profile}`);
    await delay(100);
    processes = browserProcessTree(profile);
  }
  return processes;
}

async function waitForNoProfileProcesses(profile) {
  const deadline = Date.now() + 10_000;
  let processes = processesUsingProfile(profile);
  while (processes.length > 0) {
    if (Date.now() >= deadline) {
      throw new Error(`Owned browser processes remained for ${profile}: ${JSON.stringify(processes)}`);
    }
    await delay(100);
    processes = processesUsingProfile(profile);
  }
}

function processesUsingProfile(profile) {
  return processInventory().filter((process) => process.command.includes(profile));
}

function browserProcessTree(profile) {
  const inventory = processInventory();
  const selected = new Map(
    inventory
      .filter((process) => process.command.includes(profile) || process.command.includes(runRoot))
      .map((process) => [process.pid, process]),
  );
  let changed = true;
  while (changed) {
    changed = false;
    for (const process of inventory) {
      if (!selected.has(process.pid) && selected.has(process.ppid)) {
        selected.set(process.pid, process);
        changed = true;
      }
    }
  }
  return [...selected.values()];
}

function mergeProcesses(...groups) {
  return [...new Map(groups.flat().map((process) => [
    `${process.pid}|${process.created}|${process.command}`,
    process,
  ])).values()];
}

function processFingerprint(pid, inventory = processInventory()) {
  const process = inventory.find((candidate) => candidate.pid === pid);
  return process ? `${process.created}|${process.command}` : null;
}

async function waitForProcessIdentityToDisappear(pid, fingerprint) {
  const deadline = Date.now() + 10_000;
  while (processFingerprint(pid) === fingerprint) {
    if (Date.now() >= deadline) throw new Error(`Owned service process ${pid} remained after close`);
    await delay(100);
  }
}

async function waitForProcessIdentitiesToDisappear(processes, label) {
  const identities = new Map(processes.map((process) => [
    process.pid,
    `${process.created}|${process.command}`,
  ]));
  const deadline = Date.now() + 10_000;
  let remaining = matchingProcessIdentities(identities);
  while (remaining.length > 0) {
    if (Date.now() >= deadline) {
      throw new Error(`Owned ${label} process identities remained: ${JSON.stringify(remaining)}`);
    }
    await delay(100);
    remaining = matchingProcessIdentities(identities);
  }
}

async function terminateOwnedProcessIdentities(processes, label) {
  const identities = new Map(processes.map((process) => [
    process.pid,
    `${process.created}|${process.command}`,
  ]));
  signalMatchingProcessIdentities(identities, "SIGTERM", label);
  // Node maps SIGTERM to TerminateProcess on Windows; this bounded grace period
  // is meaningful only on platforms where the browser can handle SIGTERM.
  const gracefulDeadline = Date.now() + BROWSER_TERMINATION_GRACE_MS;
  let remaining = matchingProcessIdentities(identities);
  while (remaining.length > 0 && Date.now() < gracefulDeadline) {
    await delay(100);
    remaining = matchingProcessIdentities(identities);
  }
  if (remaining.length > 0) {
    signalMatchingProcessIdentities(new Map(remaining), "SIGKILL", label);
  }
  await waitForProcessIdentitiesToDisappear(processes, label);
}

function signalMatchingProcessIdentities(identities, signal, label) {
  for (const [pid, fingerprint] of identities) {
    if (processFingerprint(pid) !== fingerprint) continue;
    try {
      process.kill(pid, signal);
    } catch (error) {
      if (error?.code !== "ESRCH") {
        throw new Error(`Could not ${signal} exact-owned ${label} process ${pid}: ${error.message}`);
      }
    }
  }
}

function matchingProcessIdentities(identities) {
  const inventory = processInventory();
  return [...identities].filter(
    ([pid, fingerprint]) => processFingerprint(pid, inventory) === fingerprint,
  );
}

function processInventory() {
  try {
    if (process.platform === "win32") {
      const systemRoot = process.env.SystemRoot ?? process.env.SYSTEMROOT ?? "C:\\Windows";
      const powershell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
      const raw = execFileSync(powershell, [
        "-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
        "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate,CommandLine | ConvertTo-Json -Compress",
      ], { env: browserEnvironment, encoding: "utf8", timeout: PROCESS_INVENTORY_TIMEOUT_MS, maxBuffer: PROCESS_INVENTORY_MAX_BUFFER });
      const parsed = JSON.parse(raw || "[]");
      return (Array.isArray(parsed) ? parsed : [parsed]).map((entry) => ({
        pid: Number(entry.ProcessId),
        ppid: Number(entry.ParentProcessId),
        created: String(entry.CreationDate ?? ""),
        command: String(entry.CommandLine ?? ""),
      }));
    }
    const raw = execFileSync("/bin/ps", ["-axo", "pid=,ppid=,lstart=,command="], {
      env: browserEnvironment,
      encoding: "utf8",
      timeout: PROCESS_INVENTORY_TIMEOUT_MS,
      maxBuffer: PROCESS_INVENTORY_MAX_BUFFER,
    });
    return raw.split("\n").flatMap((line) => {
      const match = line.match(/^\s*(\d+)\s+(\d+)\s+((?:\S+\s+){4}\d{4})\s+(.+)$/u);
      return match ? [{
        pid: Number(match[1]),
        ppid: Number(match[2]),
        created: match[3],
        command: match[4],
      }] : [];
    });
  } catch (error) {
    throw new Error(`Could not inspect owned process identities: ${error.message}`);
  }
}

function isRemoteRequest(request) {
  const url = new URL(request);
  return ["http:", "https:", "ws:", "wss:"].includes(url.protocol)
    && !["127.0.0.1", "localhost", "::1"].includes(url.hostname);
}

function allowedEnvironment(names, source = process.env) {
  return Object.fromEntries(names.flatMap((name) => (
    source[name] === undefined ? [] : [[name, source[name]]]
  )));
}

function qualifyWindowsEnvironment() {
  if (process.platform !== "win32") return null;
  const disposableProfileRoot = process.env.CF_PRESENT_WINDOWS_DISPOSABLE_PROFILE_ROOT;
  const systemRoot = process.env.SystemRoot ?? process.env.SYSTEMROOT ?? "C:\\Windows";
  const powershell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
  const knownLocalAppData = execFileSync(powershell, [
    "-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
    "[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)",
  ], {
    env: allowedEnvironment(functionalEnvironmentNames),
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 10_000,
  }).trim();
  return validateWindowsQualificationConfinement({
    disposableProfileRoot,
    userProfile: process.env.USERPROFILE,
    localAppData: process.env.LOCALAPPDATA,
    appData: process.env.APPDATA,
    temp: process.env.TEMP,
    tmp: process.env.TMP,
    knownLocalAppData,
  });
}

function assertChildEnvironmentDoesNotInheritUnrelatedHostValues() {
  const simulatedHost = {
    ...process.env,
    NPM_TOKEN: "npm-token-must-not-cross",
    NODE_OPTIONS: "--throw-deprecation",
  };
  const environment = allowedEnvironment(functionalEnvironmentNames, simulatedHost);
  Object.assign(environment, {
    HOME: home,
    USERPROFILE: userProfile,
    LOCALAPPDATA: localAppData,
    APPDATA: appData,
    XDG_STATE_HOME: xdgState,
    TMPDIR: taskTemp,
    TMP: taskTemp,
    TEMP: taskTemp,
  });
  const expected = { home, userProfile, localAppData, appData, xdgState, taskTemp };
  execFileSync(process.execPath, ["-e", [
    "for (const name of ['NPM_TOKEN', 'NODE_OPTIONS']) {",
    "  if (process.env[name] !== undefined) throw new Error(`${name} crossed the runner boundary`);",
    "}",
    "const expected = JSON.parse(process.argv[1]);",
    "for (const [name, value] of Object.entries({ HOME: expected.home, USERPROFILE: expected.userProfile, LOCALAPPDATA: expected.localAppData, APPDATA: expected.appData, XDG_STATE_HOME: expected.xdgState, TMPDIR: expected.taskTemp, TMP: expected.taskTemp, TEMP: expected.taskTemp })) {",
    "  if (process.env[name] !== value) throw new Error(`${name} did not use its task-owned root`);",
    "}",
  ].join("\n"), JSON.stringify(expected)], {
    env: environment,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 5_000,
  });
}

function assertWithinDeadline(phase) {
  if (Date.now() >= qualificationDeadline) {
    throw new Error(`Qualification exceeded its 180-second deadline before ${phase}`);
  }
}

function timeoutWithinQualification(ceiling, phase) {
  const remaining = qualificationDeadline - Date.now();
  if (remaining <= 0) {
    throw new Error(`Qualification exceeded its 180-second deadline before ${phase}`);
  }
  return Math.min(ceiling, remaining);
}

async function bounded(promise, timeout, phase) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new BoundedTimeoutError(timeout, phase)), timeout);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function delay(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function findBrowser() {
  for (const candidate of [
    process.env.CF_PRESENT_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    process.env.ProgramFiles && join(process.env.ProgramFiles, "Google", "Chrome", "Application", "chrome.exe"),
    process.env["ProgramFiles(x86)"]
      && join(process.env["ProgramFiles(x86)"], "Google", "Chrome", "Application", "chrome.exe"),
    process.env.LOCALAPPDATA
      && join(process.env.LOCALAPPDATA, "Google", "Chrome", "Application", "chrome.exe"),
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean)) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Continue to the next qualified local browser.
    }
  }
  throw new Error("No qualified local Chrome or Chromium route is available");
}

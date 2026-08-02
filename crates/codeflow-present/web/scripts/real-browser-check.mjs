import { randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { access, cp, mkdir, mkdtemp, readdir, readFile, rm, rmdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import axe from "axe-core";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
await access(codeflow);

const runId = `tsk007-${process.pid}-${randomUUID()}`;
const runRoot = await mkdtemp(join(tmpdir(), `${runId}-`));
const marker = join(runRoot, ".cf-present-qualification-root");
const project = join(runRoot, "project");
const home = join(runRoot, "home");
const browserProfileV1 = join(runRoot, "browser-profile-v1");
const browserProfileV2 = join(runRoot, "browser-profile-v2");
const output = join(runRoot, "output");
const trace = join(runRoot, "trace");
const qualificationDeadline = Date.now() + 180_000;
const evidenceDestination = process.env.CF_PRESENT_EVIDENCE_DIR
  ? resolve(process.env.CF_PRESENT_EVIDENCE_DIR)
  : null;
const childEnvironment = {
  ...process.env,
  HOME: home,
  XDG_STATE_HOME: join(home, "state"),
  TMPDIR: join(runRoot, "tmp"),
  NODE_DISABLE_COMPILE_CACHE: "1",
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
browserEnvironment.TMPDIR = childEnvironment.TMPDIR;
browserEnvironment.TMP = childEnvironment.TMPDIR;
browserEnvironment.TEMP = childEnvironment.TMPDIR;

let context;
let sessionId;
let port;
let servicePid;
let serviceInstance;
let serviceFingerprint;
let result;
const requests = [];
const responses = [];
const consoleErrors = [];
const observerTasks = [];
try {
  await writeFile(marker, `${runId}\n`, { mode: 0o600 });
  await Promise.all([
    mkdir(project),
    mkdir(home),
    mkdir(browserProfileV1),
    mkdir(browserProfileV2),
    mkdir(output),
    mkdir(trace),
    mkdir(childEnvironment.TMPDIR),
  ]);
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
  observeContext(context, requests, responses, consoleErrors, observerTasks);
  const browserProcessesV1 = await waitForProfileProcess(browserProfileV1);
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  const page = context.pages()[0] ?? await context.newPage();
  await page.goto(pathToFileURL(bootstrapPath).href);
  await page.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"));
  // The application keeps an authenticated long-poll open, so network-idle is
  // not a valid readiness signal. DOM readiness plus owned content is.
  await page.waitForLoadState("domcontentloaded");
  const initialText = await page.locator("body").innerText();
  if (!initialText.includes("First revision")) {
    throw new Error(`Authenticated application omitted its document text at ${page.url()}: ${initialText.slice(0, 2_000)}`);
  }
  await page.locator("[data-cf-diagram]").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-diagram]")?.getAttribute("data-cf-diagram") === "ready");
  await page.locator("code[data-cf-language='rust']").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready");

  for (const mode of ["light", "dark"]) {
    await page.getByLabel("Mode").selectOption(mode);
    const accessibility = await page.evaluate(async () => globalThis.axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] },
    }));
    if (accessibility.violations.length) {
      throw new Error(`${mode} accessibility violations: ${accessibility.violations.map((item) => item.id).join(", ")}`);
    }
    await page.screenshot({ path: join(output, `review-${mode}.png`), fullPage: true });
  }

  const reviewButton = page.locator("#cf-review-toggle");
  if (await reviewButton.isVisible()) {
    await reviewButton.click();
  } else if (!await page.locator("#cf-feedback-panel").isVisible()) {
    throw new Error(
      `Authenticated review controls did not mount; console=${JSON.stringify(consoleErrors)}, responses=${JSON.stringify(responses)}`,
    );
  }
  await page.getByLabel("Verdict").selectOption("approve");
  await page.getByRole("button", { name: "Submit review" }).click();
  await page.getByRole("status").getByText(/Review received/u).waitFor();
  const delivered = JSON.parse(run(codeflow, ["present", "feedback", sessionId], project).trim());
  if (delivered.verdict !== "approve") throw new Error("The real browser review was not delivered");
  run(codeflow, [
    "present", "resolve", sessionId, delivered.event_id,
    "--event-version", "2", "--status", "addressed",
  ], project);

  run(codeflow, ["present", "update", sessionId, secondDocument], project);
  await page.getByText(/A newer document revision is available/u).waitFor();
  await page.screenshot({ path: join(output, "revision-notice.png"), fullPage: true });
  await bounded(
    context.tracing.stop({ path: join(trace, "revision-v1.zip") }),
    15_000,
    "stop revision-one trace",
  );
  await closeBrowser(context, browserProfileV1, browserProcessesV1);
  context = undefined;
  const shown = run(codeflow, ["present", "show", sessionId, "--no-launch"], project);
  const secondBootstrapPath = shown.match(/owner-private bootstrap file (.+?) in the isolated profile/u)?.[1];
  if (!secondBootstrapPath) throw new Error(`Could not parse presentation show output: ${shown}`);

  context = await launchBrowser(browserProfileV2);
  observeContext(context, requests, responses, consoleErrors, observerTasks);
  const browserProcessesV2 = await waitForProfileProcess(browserProfileV2);
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  const revisedPage = context.pages()[0] ?? await context.newPage();
  await revisedPage.goto(pathToFileURL(secondBootstrapPath).href);
  await revisedPage.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"));
  await revisedPage.getByText("Second revision").waitFor();
  const exportPath = join(output, "review.html");
  run(codeflow, ["present", "export", sessionId, "--out", exportPath, "--theme", "technical", "--mode", "dark"], project);
  const exportPage = await context.newPage();
  await exportPage.goto(pathToFileURL(exportPath).href);
  await exportPage.getByText("Second revision").waitFor();
  await exportPage.waitForFunction(() => (
    document.querySelector("[data-cf-diagram]")?.getAttribute("data-cf-diagram") === "ready"
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
  await delay(100);
  await Promise.all(observerTasks);
  await exportPage.close();

  if (requests.some(isRemoteRequest)) {
    throw new Error(`Real presentation attempted remote network: ${requests.join(", ")}`);
  }
  if (consoleErrors.length) throw new Error(`Real presentation console errors: ${consoleErrors.join(" | ")}`);

  await bounded(
    context.tracing.stop({ path: join(trace, "revision-v2-and-export.zip") }),
    15_000,
    "stop revision-two trace",
  );
  await closeBrowser(context, browserProfileV2, browserProcessesV2);
  context = undefined;
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
    task_owned_resources: {
      project,
      home,
      browser_profiles: [browserProfileV1, browserProfileV2],
      service_port: port,
      service_pid: servicePid,
      service_instance: serviceInstance,
      data_namespace: runId,
      output,
      trace,
    },
    checks: {
      real_service_bootstrap: "pass",
      declarative_rendering: "pass",
      accessibility_light_dark: "pass",
      syntax_and_diagram: "pass",
      feedback_delivery_and_resolution: "pass",
      immutable_revision_update: "pass",
      offline_export: "pass",
      remote_network: "none observed",
      console_errors: "none observed",
      service_port_teardown: "pass",
      service_process_identity_teardown: "pass",
      browser_process_and_profile_teardown: "pass",
      session_clear: "pass",
    },
  };
  await writeFile(join(output, "results.json"), `${JSON.stringify(result, null, 2)}\n`);
  if (evidenceDestination) {
    await mkdir(evidenceDestination, { recursive: true });
    await cp(output, join(evidenceDestination, "output"), { recursive: true, force: true });
    await cp(trace, join(evidenceDestination, "trace"), { recursive: true, force: true });
  }
} finally {
  if (context) {
    let ownedBrowserProcesses = mergeProcesses(
      browserProcessTree(browserProfileV1),
      browserProcessTree(browserProfileV2),
    );
    try {
      await bounded(
        context.tracing.stop({ path: join(trace, "failure.zip") }),
        15_000,
        "stop failed browser trace",
      );
    } catch {
      // The context may have failed before tracing started.
    }
    await bounded(context.close(), 15_000, "close failed browser context").catch(() => {});
    ownedBrowserProcesses = mergeProcesses(
      ownedBrowserProcesses,
      browserProcessTree(browserProfileV1),
      browserProcessTree(browserProfileV2),
    );
    await waitForProcessIdentitiesToDisappear(ownedBrowserProcesses, "failed browser run");
  }
  if (!result && evidenceDestination && await pathExists(output)) {
    await mkdir(evidenceDestination, { recursive: true });
    await cp(output, join(evidenceDestination, "failure-output"), { recursive: true, force: true });
    if (await pathExists(trace)) {
      await cp(trace, join(evidenceDestination, "failure-trace"), { recursive: true, force: true });
    }
  }
  if (sessionId) {
    tryRun(codeflow, ["present", "close", sessionId], project);
    if (port) await assertPortClosed(port);
    if (servicePid && serviceFingerprint) {
      await waitForProcessIdentityToDisappear(servicePid, serviceFingerprint);
    }
    tryRun(codeflow, ["present", "clear", sessionId, "--older-than", "0h"], project);
  }
  const markerValue = await readFile(marker, "utf8").catch(() => "");
  if (markerValue === `${runId}\n`) {
    await rm(runRoot, { recursive: true, force: false });
  } else if (markerValue === "" && (await readdir(runRoot)).length === 0) {
    // mkdtemp can succeed immediately before marker creation fails. Only an
    // empty, task-namespaced root is safe to remove without the marker.
    await rmdir(runRoot);
  } else {
    throw new Error(`Refusing to clean unmarked or foreign qualification root ${runRoot}`);
  }
  if (await pathExists(runRoot)) throw new Error(`Qualification root remained after cleanup: ${runRoot}`);
  await delay(2_000);
  if (await pathExists(runRoot)) {
    throw new Error(`Qualification root was recreated during post-close quiescence: ${runRoot}`);
  }
}
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
      { type: "diagram", id: "flow", kind: "flowchart", source: "flowchart LR\nInput-->Review-->Evidence", acc_title: "Qualification flow", acc_description: "Input moves through review to evidence." },
      { type: "feedback_prompt", id: "decision", prompt: "Approve or request a concrete change." },
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

function tryRun(executable, args, cwd) {
  try {
    run(executable, args, cwd, false);
  } catch {
    // The primary failure remains authoritative; cleanup stays bounded.
  }
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
    executablePath: await findBrowser(),
    headless: true,
    viewport: { width: 1280, height: 900 },
    env: browserEnvironment,
    args: [
      "--disable-background-networking",
      "--disable-component-update",
      "--disable-breakpad",
      "--disable-crash-reporter",
      "--disable-crashpad-for-testing",
      "--disable-default-apps",
      "--disable-sync",
      "--no-default-browser-check",
      "--noerrdialogs",
      "--no-first-run",
    ],
  });
  await launched.addInitScript({ content: axe.source });
  return launched;
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
    observedPage.setDefaultNavigationTimeout(20_000);
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
  let ownedProcesses = mergeProcesses(knownProcesses, browserProcessTree(profile));
  await bounded(openContext.close(), 15_000, `close browser using ${profile}`);
  ownedProcesses = mergeProcesses(ownedProcesses, browserProcessTree(profile));
  await waitForProcessIdentitiesToDisappear(ownedProcesses, `browser using ${profile}`);
  await waitForNoProfileProcesses(profile);
  await rm(profile, { recursive: true, force: false });
  if (await pathExists(profile)) throw new Error(`Owned browser profile remained after cleanup: ${profile}`);
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

function processFingerprint(pid) {
  const process = processInventory().find((candidate) => candidate.pid === pid);
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
  let remaining = [...identities].filter(([pid, fingerprint]) => processFingerprint(pid) === fingerprint);
  while (remaining.length > 0) {
    if (Date.now() >= deadline) {
      throw new Error(`Owned ${label} process identities remained: ${JSON.stringify(remaining)}`);
    }
    await delay(100);
    remaining = [...identities].filter(([pid, fingerprint]) => processFingerprint(pid) === fingerprint);
  }
}

function processInventory() {
  try {
    if (process.platform === "win32") {
      const systemRoot = process.env.SystemRoot ?? process.env.SYSTEMROOT ?? "C:\\Windows";
      const powershell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
      const raw = execFileSync(powershell, [
        "-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
        "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate,CommandLine | ConvertTo-Json -Compress",
      ], { env: browserEnvironment, encoding: "utf8", timeout: 10_000 });
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
      timeout: 5_000,
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

function allowedEnvironment(names) {
  return Object.fromEntries(names.flatMap((name) => (
    process.env[name] === undefined ? [] : [[name, process.env[name]]]
  )));
}

function assertWithinDeadline(phase) {
  if (Date.now() >= qualificationDeadline) {
    throw new Error(`Qualification exceeded its 180-second deadline before ${phase}`);
  }
}

async function bounded(promise, timeout, phase) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`Timed out after ${timeout}ms: ${phase}`)), timeout);
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

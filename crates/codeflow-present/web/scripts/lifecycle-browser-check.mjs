// The review window lifecycle in the browser `codeflow` launches itself
// (TSK-441 AC-6 to AC-8). A debug build adds a loopback debugging port to that
// launch when CF_PRESENT_TEST_DEBUGGING_PORT is set, and runs it headless, so
// this check drives the real isolated browser, not a browser of its own, and
// never opens a window on the host:
//
//   open        the session launches its browser; a window shows the review
//   AC-8        the window closes, the browser keeps running, and
//               `present show` opens a new window in that same browser
//   AC-7        a window reopened within the grace keeps the browser; with no
//               window past the grace the browser quits, the session stays open
//   AC-6        a queued note, then End review: the note reaches
//               `present feedback`, the browser exits and the session is closed
//               and cleaned up as `present close` leaves it
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { codeflowBinary } from "./codeflow-binary.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = codeflowBinary(repoRoot);
// limits::WINDOW_CLOSE_GRACE_SECONDS, plus one event poll (EVENT_POLL_SECONDS)
// for a page whose poll the service sees end only when it answers.
const GRACE_MS = 15_000;
const QUIT_BOUND_MS = GRACE_MS + 25_000 + 15_000;

const runRoot = await mkdtemp(join(tmpdir(), "cflife-"));
const project = join(runRoot, "project");
const home = join(runRoot, "home");
const taskTemp = join(runRoot, "tmp");
const output = process.env.CF_PRESENT_EVIDENCE_DIR ? resolve(process.env.CF_PRESENT_EVIDENCE_DIR) : join(runRoot, "output");
await Promise.all([mkdir(project), mkdir(home), mkdir(taskTemp), mkdir(output, { recursive: true })]);
const debuggingPort = await freePort();
const env = {
  PATH: process.env.PATH ?? "",
  LANG: process.env.LANG ?? "en_US.UTF-8",
  HOME: home,
  TMPDIR: taskTemp,
  CF_PRESENT_TEST_DEBUGGING_PORT: String(debuggingPort),
  // A service this check starts exits once this process is gone.
  CF_PRESENT_OWNER_PID: String(process.pid),
};
const log = [];
const step = (message) => {
  const line = `${new Date().toISOString()} ${message}`;
  log.push(line);
  console.log(line);
};

let sessionId;
let browser;
let failure;
try {
  run("git", ["init", "--quiet"]);
  const document = join(project, "review.json");
  await writeFile(document, `${JSON.stringify({
    schema_version: 1,
    title: "Window lifecycle",
    language: "en",
    blocks: [{ type: "narrative", id: "outcome", markdown: "The review window closes, reopens and ends." }],
  }, null, 2)}\n`);

  const opened = run(codeflow, ["present", "open", document]);
  sessionId = opened.match(/opened ([0-9a-f-]{36})/u)?.[1];
  if (!sessionId) throw new Error(`present open did not launch: ${opened}`);
  const firstPid = session().browser_pid;
  if (!Number.isInteger(firstPid)) throw new Error(`no browser recorded: ${JSON.stringify(session())}`);
  step(`open launched browser ${firstPid}`);
  browser = await connect();
  let page = await reviewPage(browser);

  // AC-8: close the only window; the browser runs on with none.
  await page.close();
  await until(async () => (await appTargets()).length === 0, 10_000, "the window to close");
  if (!alive(firstPid)) throw new Error("the browser quit as its window closed");
  if (session().status !== "active") throw new Error("closing the window closed the session");
  step("window closed; browser and session remain");

  const shown = run(codeflow, ["present", "show", sessionId]);
  if (!/opened a window for/u.test(shown)) throw new Error(`present show did not open a window: ${shown}`);
  page = await reviewPage(browser);
  if (session().browser_pid !== firstPid || !alive(firstPid)) throw new Error("present show replaced the browser");
  step(`present show opened a window in browser ${firstPid}`);
  await page.screenshot({ path: join(output, "lifecycle-reopened.png") });

  // AC-7: a window open past the grace keeps the browser.
  await delay(GRACE_MS + 5_000);
  if (!alive(firstPid)) throw new Error("the browser quit while a window was open");
  step("browser kept with a window open past the grace");

  // AC-7: a window reopened within the grace keeps the browser.
  await page.close();
  await until(async () => (await appTargets()).length === 0, 10_000, "the window to close");
  await delay(GRACE_MS / 3);
  run(codeflow, ["present", "show", sessionId]);
  page = await reviewPage(browser);
  await delay(GRACE_MS);
  if (!alive(firstPid) || session().browser_pid !== firstPid) throw new Error("a window reopened within the grace lost the browser");
  step("window reopened within the grace kept the browser");

  // AC-7: no window past the grace: the browser quits, the session stays.
  await page.close();
  const closedAt = Date.now();
  await until(async () => !alive(firstPid), QUIT_BOUND_MS, "the browser to quit after the grace");
  const quitAfter = Date.now() - closedAt;
  if (quitAfter < GRACE_MS - 1_000) throw new Error(`the browser quit after ${quitAfter} ms, before the grace`);
  // The service clears the record once the process group is gone and the
  // profile is removed, a moment after the main process exits.
  await until(async () => session().browser_pid === undefined, 15_000, "the quit browser's record to clear");
  const kept = session();
  if (kept.status !== "active" || kept.browser_pid !== undefined) throw new Error(`session after the quit: ${JSON.stringify(kept)}`);
  step(`browser quit ${quitAfter} ms after its last window closed; session open`);
  await browser.close().catch(() => {});

  // The session still serves: show launches a new browser.
  const relaunched = run(codeflow, ["present", "show", sessionId]);
  if (!/opened /u.test(relaunched)) throw new Error(`present show did not relaunch: ${relaunched}`);
  const secondPid = session().browser_pid;
  if (!Number.isInteger(secondPid) || secondPid === firstPid) throw new Error("present show did not launch a new browser");
  browser = await connect();
  page = await reviewPage(browser);
  step(`present show relaunched browser ${secondPid}`);

  // AC-6: queue a note, then End review.
  await page.locator("#cf-comment-toggle").click();
  await page.getByTestId("notes-dock").locator(".cf-tools summary").click();
  await page.getByRole("button", { name: "Whole document" }).click();
  await page.getByTestId("composer-text").fill("Queued before End review.");
  await page.getByTestId("composer-save").click();
  await page.getByTestId("composer").waitFor({ state: "detached" });
  await page.screenshot({ path: join(output, "lifecycle-before-end.png") });
  page.once("dialog", (dialog) => { void dialog.accept(); });
  const servicePort = session().service_port;
  await page.getByTestId("end-review").click();
  await until(async () => !alive(secondPid), 30_000, "End review to quit the browser");
  await until(async () => session().status === "closed" && session().service_pid === undefined, 30_000, "the session to close and its service to exit");
  const ended = session();
  if (ended.browser_pid !== undefined) throw new Error(`End review left a browser recorded: ${JSON.stringify(ended)}`);
  if (await portOpen(servicePort)) throw new Error("End review left the service listening");
  const feedback = run(codeflow, ["present", "feedback", sessionId]).trim();
  const delivered = JSON.parse(feedback);
  if (!delivered.notes?.some((note) => note.body === "Queued before End review.")) {
    throw new Error(`the queued note was not delivered: ${feedback}`);
  }
  step("End review delivered the queued note, quit the browser and closed the session");
  console.log(JSON.stringify({ result: "pass", session: sessionId, log }, null, 2));
} catch (error) {
  failure = error;
} finally {
  await browser?.close().catch(() => {});
  if (sessionId) {
    try { run(codeflow, ["present", "close", sessionId]); } catch { /* already closed */ }
  }
  await writeFile(join(output, "lifecycle-log.txt"), `${log.join("\n")}\n`).catch(() => {});
  if (!process.env.CF_PRESENT_EVIDENCE_DIR) await rm(runRoot, { recursive: true, force: true }).catch(() => {});
}
if (failure) {
  console.error(failure);
  process.exit(1);
}

function run(executable, args) {
  return execFileSync(executable, args, { cwd: project, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 60_000 });
}

function session() {
  const listed = JSON.parse(run(codeflow, ["present", "list"]));
  const found = listed.find((entry) => entry.id === sessionId);
  if (!found) throw new Error(`session ${sessionId} is not listed`);
  return found;
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function connect() {
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${debuggingPort}`);
    } catch (error) {
      if (Date.now() >= deadline) throw error;
      await delay(250);
    }
  }
}

async function appTargets() {
  const response = await fetch(`http://127.0.0.1:${debuggingPort}/json/list`);
  const targets = await response.json();
  return targets.filter((target) => target.type === "page" && /\/app\/$/u.test(target.url));
}

// A loaded host (load average in the hundreds) was seen to take over a minute
// to commit the bootstrap, as real-browser-check allows for.
async function reviewPage(connected) {
  const deadline = Date.now() + 120_000;
  for (;;) {
    for (const context of connected.contexts()) {
      for (const candidate of context.pages()) {
        if (/^http:\/\/127\.0\.0\.1:\d+\/app\/$/u.test(candidate.url())) {
          await candidate.locator("#cf-comment-toggle").waitFor({ state: "visible", timeout: 60_000 });
          return candidate;
        }
      }
    }
    if (Date.now() >= deadline) throw new Error("no review window appeared");
    await delay(250);
  }
}

async function until(check, timeout, what) {
  const deadline = Date.now() + timeout;
  while (!await check()) {
    if (Date.now() >= deadline) throw new Error(`timed out after ${timeout} ms waiting for ${what}`);
    await delay(250);
  }
}

async function portOpen(candidate) {
  const { connect: dial } = await import("node:net");
  return await new Promise((resolvePromise) => {
    const socket = dial({ host: "127.0.0.1", port: candidate });
    socket.once("connect", () => { socket.destroy(); resolvePromise(true); });
    socket.once("error", () => resolvePromise(false));
  });
}

async function freePort() {
  return await new Promise((resolvePromise, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolvePromise(port));
    });
  });
}

function delay(ms) {
  return new Promise((resolvePromise) => { setTimeout(resolvePromise, ms); });
}

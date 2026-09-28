// Delivery end to end (TSK-120, SPC-014 B6 page states and B8): the real
// `codeflow` binary serves documents/v2-forms.json and a task-owned headless
// Chrome profile answers through the page. No model or listener runs in the
// background: each agent step is one CLI command this script runs.
// - no listener: an answer shows "Stored, waiting for agent" and stays
//   pending in the store;
// - the next wait delivers it and the form shows "Delivered to agent";
//   `present ack` makes it "Acknowledged by agent", a separate state;
// - a wait already running is woken by an answer the page sends;
// - the rail shows a review's delivery and its acknowledgment apart;
// - after the service is killed and restarted, a pending answer is
//   delivered once and a later wait finds nothing.
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
const fixture = join(webRoot, "../tests/fixtures/contract-v2/documents/v2-forms.json");

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-delivery-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
};
const options = { cwd: project, env: environment, encoding: "utf8", timeout: 60_000 };
// One CLI command; its exit status, standard output and standard error.
const cli = (args) => {
  const result = spawnSync(codeflow, args, options);
  if (result.error) throw result.error;
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
};
const run = (args) => {
  const result = cli(args);
  if (result.status !== 0) throw new Error(`codeflow ${args.join(" ")}: exit ${result.status}: ${result.stderr.trim()}`);
  return result.stdout;
};
// A command started now and finished later, as a waiting agent's shell loop runs it.
const start = (args) => {
  const child = spawn(codeflow, args, { cwd: project, env: environment, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  let exited = false;
  const done = new Promise((resolveDone, reject) => {
    child.on("error", reject);
    child.on("close", (status) => { exited = true; resolveDone({ status, stdout, stderr }); });
  });
  return { done, running: () => !exited };
};
const lines = (stdout) => stdout.split("\n").filter(Boolean).map((line) => JSON.parse(line));
const delay = (ms) => new Promise((done) => setTimeout(done, ms));

let sessionId = null;
let context = null;
const passed = [];
try {
  for (const directory of [project, environment.HOME, environment.TMPDIR, environment.XDG_STATE_HOME, join(root, "profile")]) await mkdir(directory, { recursive: true });
  execFileSync("git", ["init", "--quiet"], { cwd: project, env: environment });
  const document = join(project, "forms.json");
  await writeFile(document, await readFile(fixture, "utf8"));
  const opened = run(["present", "open", document, "--no-launch"]);
  sessionId = opened.match(/session ([0-9a-f-]+) ready/u)?.[1] ?? null;
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  if (!sessionId || !bootstrap) throw new Error(`could not parse present open output: ${opened}`);

  context = await chromium.launchPersistentContext(join(root, "profile"), {
    executablePath: await findBrowser(),
    headless: true,
    viewport: { width: 1280, height: 900 },
    args: ["--disable-background-networking", "--disable-component-update", "--disable-default-apps", "--disable-sync", "--no-default-browser-check", "--no-first-run"],
  });
  const page = context.pages()[0] ?? await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const openApp = async (path) => {
    const port = JSON.parse(run(["present", "list"]))[0]?.service_port;
    await page.goto(pathToFileURL(path).href, { waitUntil: "commit", timeout: 120_000 });
    await page.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  };
  await openApp(bootstrap);

  const form = page.locator("article[data-cf-form='store-choice']");
  const decision = page.locator("article[data-cf-form='d-scope']");
  const field = (id) => form.locator(`[data-cf-field='${id}']`);
  const stateOf = async (article) => ({
    state: await article.getAttribute("data-cf-form-state"),
    says: (await article.locator("[data-cf-form-state]").innerText()).trim(),
  });
  const waitState = async (article, state) => {
    await page.waitForFunction(({ id, state }) => document.querySelector(`article[data-cf-form='${id}']`)?.getAttribute("data-cf-form-state") === state,
      { id: await article.getAttribute("data-cf-form"), state }, { timeout: 20_000 }).catch(async (error) => {
      throw new Error(`the form never reached ${state}; it is ${JSON.stringify(await stateOf(article))}`, { cause: error });
    });
    return stateOf(article);
  };
  const answerStore = async () => {
    await field("home").locator("input[value='local']").check();
    await field("keep-days").locator("input").fill("30");
    await form.locator("[data-cf-form-action='submit']").click();
    return waitState(form, "stored");
  };
  const pending = () => lines(run(["present", "responses", "list", sessionId, "--status", "pending"]));
  const waitV2 = (seconds) => cli(["present", "feedback", sessionId, "--wait", "--timeout", String(seconds), "--format", "v2"]);

  // No listener: the answer is stored and waits in the store.
  {
    const stored = await answerStore();
    assert.equal(stored.says, "Stored, waiting for agent");
    await delay(1_500);
    assert.deepEqual(await stateOf(form), stored, "no listener: the form moved on");
    const waiting = pending();
    assert.equal(waiting.length, 1, "no listener: pending events");
    assert.equal(waiting[0].kind, "answer");
    assert.equal(waiting[0].form_id, "store-choice");
    passed.push(`no listener: the form says "${stored.says}" and the answer stays pending in the store`);
  }

  // The next wait delivers it; the form shows the delivery, then the ack.
  {
    const waited = waitV2(10);
    assert.equal(waited.status, 0, waited.stderr);
    const [line] = lines(waited.stdout);
    assert.equal(line.kind, "answer");
    assert.equal(line.untrusted, true);
    const delivered = await waitState(form, "delivered");
    assert.equal(delivered.says, "Delivered to agent");
    assert.deepEqual(pending(), [], "delivered: still pending");
    assert.equal(run(["present", "ack", sessionId, line.event_id]).trim(), `acknowledged ${line.event_id}`);
    const acknowledged = await waitState(form, "acknowledged");
    assert.equal(acknowledged.says, "Acknowledged by agent");
    assert.ok(await form.locator("[data-cf-form-action='amend']").isVisible(), "acknowledged: no correction offered");
    passed.push(`delivery: the next wait prints the answer and the form says "${delivered.says}"; present ack makes it "${acknowledged.says}"`);
  }

  // A wait already running is woken by an answer sent from the page.
  {
    const waiting = start(["present", "feedback", sessionId, "--wait", "--timeout", "30", "--format", "v2"]);
    await delay(1_000);
    assert.ok(waiting.running(), "running wait: it ended with nothing pending");
    await decision.locator("input[value='a']").check();
    await decision.locator("[data-cf-form-action='submit']").click();
    const woken = await waiting.done;
    assert.equal(woken.status, 0, woken.stderr);
    const [line] = lines(woken.stdout);
    assert.equal(line.form_id, "d-scope");
    assert.deepEqual(line.values, { choice: "a" });
    const delivered = await waitState(decision, "delivered");
    assert.equal(delivered.says, "Delivered to agent");
    assert.equal((await stateOf(form)).state, "acknowledged", "running wait: the other form changed");
    passed.push("running wait: an answer sent from the page wakes the wait, which prints it, and the form shows the delivery");
  }

  // Reviews: delivery and acknowledgment are shown apart in the rail.
  {
    const post = (eventId) => page.evaluate(async ({ eventId, sessionId }) => {
      const response = await fetch("/app/api/reviews", {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json", "X-CF-Present": "1" },
        body: JSON.stringify({ event_id: eventId, session_id: sessionId, revision: 1, verdict: "approve", notes: [] }),
      });
      return response.status;
    }, { eventId, sessionId });
    const [acked, unacked] = [crypto.randomUUID(), crypto.randomUUID()];
    assert.equal(await post(acked), 201);
    assert.equal(await post(unacked), 201);
    const delivered = lines(run(["present", "feedback", sessionId, "--format", "v2"]));
    assert.deepEqual(delivered.map((line) => [line.kind, line.event_id]), [["review", acked], ["review", unacked]]);
    run(["present", "ack", sessionId, acked]);
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    const rail = await page.locator("[data-testid='feedback-history'] li").evaluateAll((items) => items
      .filter((item) => item.querySelector(".cf-history-meta"))
      .map((item) => [...item.querySelector(".cf-history-meta").children].map((part) => part.textContent.trim())));
    assert.equal(rail.length, 2, `rail: ${JSON.stringify(rail)}`);
    assert.ok(rail[0].includes("delivered") && rail[0].includes("acknowledged"), `rail: the acknowledged review shows ${JSON.stringify(rail[0])}`);
    assert.ok(rail[1].includes("delivered") && !rail[1].includes("acknowledged"), `rail: the delivered review shows ${JSON.stringify(rail[1])}`);
    passed.push(`rail: a delivered review reads ${JSON.stringify(rail[1].slice(0, 2))} and an acknowledged one ${JSON.stringify(rail[0].slice(0, 3))}`);
  }

  // A service restart: the pending answer is delivered once, none is lost.
  {
    const stored = await answerStore();
    assert.equal(stored.says, "Stored, waiting for agent");
    const [answer] = pending();
    const pid = JSON.parse(run(["present", "list"]))[0]?.service_pid;
    process.kill(pid, "SIGKILL");
    for (let attempt = 0; ; attempt += 1) {
      try {
        process.kill(pid, 0);
      } catch {
        break;
      }
      assert.ok(attempt < 100, "restart: the service did not exit");
      await delay(50);
    }
    const recovered = run(["present", "show", sessionId, "--no-launch"]);
    const again = recovered.match(/owner-private bootstrap file (.+?) in the isolated profile/u)?.[1];
    if (!again) throw new Error(`could not parse present show output: ${recovered}`);
    await openApp(again);
    assert.deepEqual(pending().map((line) => line.event_id), [answer.event_id], "restart: the pending answer");
    const first = waitV2(10);
    assert.equal(first.status, 0, first.stderr);
    assert.deepEqual(lines(first.stdout).map((line) => line.event_id), [answer.event_id], "restart: delivered once");
    const second = waitV2(1);
    assert.equal(second.status, 6, "restart: a later wait found an event");
    assert.equal(second.stdout, "");
    passed.push("restart: after the service is killed and show restarts it, the pending answer is delivered once and the next wait times out (exit 6)");
  }

  assert.deepEqual(errors, [], "page errors");
  await context.close();
  context = null;
  run(["present", "close", sessionId]);
  run(["present", "clear", sessionId, "--older-than", "0d"]);
  sessionId = null;
  process.stdout.write(`cf-present delivery checks passed:\n${passed.map((line) => `  ${line}`).join("\n")}\n`);
} finally {
  if (context) await context.close().catch(() => undefined);
  if (sessionId) {
    try { run(["present", "close", sessionId]); } catch { /* the service may already be gone */ }
  }
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

async function findBrowser() {
  for (const candidate of [
    process.env.CF_PRESENT_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean)) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Try the next local browser.
    }
  }
  throw new Error("No local Chrome or Chromium is available");
}

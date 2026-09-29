// Delivery end to end (TSK-120, SPC-014 B6 page states and B8): the real
// `codeflow` binary serves documents/v2-forms.json and a task-owned headless
// Chrome profile answers through the page. No model or listener runs in the
// background: each agent step is one CLI command this script runs.
// - no listener: an answer shows "Stored, waiting for agent" and stays
//   pending in the store;
// - the next wait delivers it and the form shows "Delivered to agent";
//   `present ack` makes it "Acknowledged by agent", a separate state;
// - a wait already running is woken by an answer the page sends;
// - a reload after each of those states shows the same state and words;
// - a second tab loaded before the first answer cannot store a second
//   original answer: it is refused (answer_exists) and shows the stored one,
//   following the latest correction's states, never the original's;
// - the rail shows a review's delivery and its acknowledgment apart;
// - after the service is killed and restarted, a pending answer is
//   delivered once and a later wait finds nothing;
// - after a reload, Amend starts from the last sent values: a correction
//   that changes one field sends the others unchanged, and a reason left
//   empty stays absent, even for a field named like an object member
//   (constructor) (TSK-176);
// - a closure binds every form to its answer as a reload renders it, before
//   the forms latch closed: a tab following an original the other tab
//   corrected, a page that never saw an answer, and a state not yet seen;
// - a tab whose answer is refused as closed (410) before its poll hears the
//   closure marks its words as last known, then takes the closure's answer
//   when the poll brings it, and stays closed.
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { codeflowBinary } from "./codeflow-binary.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = codeflowBinary(repoRoot);
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
  // The state the service renders with the page: a reload loses nothing.
  const afterReload = async (article, state) => {
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    return waitState(article, state);
  };
  const waitV2 = (seconds) => cli(["present", "feedback", sessionId, "--wait", "--timeout", String(seconds), "--format", "v2"]);
  // A second tab, loaded before any answer: its forms are open.
  const other = await context.newPage();
  other.on("pageerror", (error) => errors.push(error.message));
  await other.goto(page.url(), { waitUntil: "domcontentloaded" });
  await other.locator("#cf-comment-toggle").waitFor({ state: "visible" });

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
    assert.deepEqual(await afterReload(form, "stored"), stored, "reload: stored");
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
    assert.deepEqual(await afterReload(form, "delivered"), delivered, "reload: delivered");
    assert.equal(run(["present", "ack", sessionId, line.event_id]).trim(), `acknowledged ${line.event_id}`);
    const acknowledged = await waitState(form, "acknowledged");
    assert.equal(acknowledged.says, "Acknowledged by agent");
    assert.ok(await form.locator("[data-cf-form-action='amend']").isVisible(), "acknowledged: no correction offered");
    assert.deepEqual(await afterReload(form, "acknowledged"), acknowledged, "reload: acknowledged");
    assert.ok(await form.locator("[data-cf-form-action='amend']").isVisible(), "reload: no correction offered");
    passed.push(`delivery: the next wait prints the answer and the form says "${delivered.says}"; present ack makes it "${acknowledged.says}"`);
    passed.push("reload: after each of stored, delivered and acknowledged, a reload shows the same state and words");
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

  // The second tab still shows both questions unanswered. Its answer is a
  // second original answer: refused, nothing stored, and the tab shows what
  // a reload would, and offers a correction (SPC-014 B6, C120-1).
  const EXISTS = "This question was already answered from another copy of this page; your draft is kept here, unsent. Use Correct this answer to send it as a correction.";
  const refusedInOther = async (id, fill) => {
    const late = other.locator(`article[data-cf-form='${id}']`);
    assert.equal(await late.getAttribute("data-cf-form-state"), "editing", `second tab, ${id}: the form was not open`);
    const before = lines(run(["present", "responses", "list", sessionId, "--form", id]));
    await fill(late);
    const refusal = other.waitForResponse((response) => new URL(response.url()).pathname === "/app/api/answers");
    await late.locator("[data-cf-form-action='submit']").click();
    const response = await refusal;
    assert.equal(response.status(), 409);
    const body = await response.json();
    assert.equal(body.error, "answer_exists");
    assert.deepEqual(lines(run(["present", "responses", "list", sessionId, "--form", id])), before, `second tab, ${id}: something was stored`);
    assert.ok(await late.locator("[data-cf-form-action='amend']").isVisible(), `second tab, ${id}: no correction offered`);
    return { late, details: body.details, before };
  };
  const shown = async (late) => ({
    state: await late.getAttribute("data-cf-form-state"),
    says: (await late.locator("[data-cf-form-state]").innerText()).trim(),
  });
  {
    const { late, details, before } = await refusedInOther("d-scope", (late) => late.locator("input[value='b']").check());
    assert.deepEqual(details, { answer_id: before[0].event_id, latest_answer_id: before[0].event_id, state: "delivered" });
    await other.waitForFunction(() => document.querySelector("article[data-cf-form='d-scope']")?.getAttribute("data-cf-form-state") === "delivered", null, { timeout: 20_000 });
    const says = (await shown(late)).says;
    assert.equal(says, `Delivered to agent. ${EXISTS}`);
    assert.ok(await late.locator("input[value='b']").isChecked(), "second tab: the draft was lost");
    passed.push(`second tab: a tab loaded before the first answer gets answer_exists (409) for its own, stores nothing, and says "${says}" with a correction offered`);
  }

  // The original O is acknowledged and its correction M is pending when the
  // second tab's answer is refused. The tab follows M, stored, delivered,
  // then acknowledged, and never shows O's acknowledgment (C120-1).
  {
    await form.locator("[data-cf-form-action='amend']").click();
    await field("home").locator("input[value='local']").check();
    await field("keep-days").locator("input").fill("45");
    await form.locator("[data-cf-form-action='submit']").click();
    await waitState(form, "stored");
    const [original, correction] = lines(run(["present", "responses", "list", sessionId, "--form", "store-choice"]));
    assert.deepEqual([original.status, correction.status, correction.kind], ["acknowledged", "pending", "amendment"]);
    await other.evaluate(() => {
      const article = document.querySelector("article[data-cf-form='store-choice']");
      window.cfShownStates = [];
      new MutationObserver(() => window.cfShownStates.push(article.getAttribute("data-cf-form-state")))
        .observe(article, { attributes: true, attributeFilter: ["data-cf-form-state"] });
    });
    const { late, details } = await refusedInOther("store-choice", async (late) => {
      await late.locator("[data-cf-field='home'] input[value='repo']").check();
      await late.locator("[data-cf-field='keep-days'] input").fill("9");
    });
    assert.deepEqual(details, { answer_id: original.event_id, latest_answer_id: correction.event_id, state: "stored" });
    await other.waitForFunction(() => document.querySelector("article[data-cf-form='store-choice']")?.getAttribute("data-cf-form-state") === "stored", null, { timeout: 20_000 });
    assert.equal((await shown(late)).says, `Stored, waiting for agent. ${EXISTS}`);
    await delay(1_500);
    assert.equal((await shown(late)).state, "stored", "second tab: the pending correction moved on");
    const waited = waitV2(10);
    assert.equal(waited.status, 0, waited.stderr);
    assert.deepEqual(lines(waited.stdout).map((line) => line.event_id), [correction.event_id]);
    await other.waitForFunction(() => document.querySelector("article[data-cf-form='store-choice']")?.getAttribute("data-cf-form-state") === "delivered", null, { timeout: 20_000 });
    run(["present", "ack", sessionId, correction.event_id]);
    await other.waitForFunction(() => document.querySelector("article[data-cf-form='store-choice']")?.getAttribute("data-cf-form-state") === "acknowledged", null, { timeout: 20_000 });
    assert.equal((await shown(late)).says, "Acknowledged by agent");
    const states = (await other.evaluate(() => window.cfShownStates)).filter((state, index, all) => state !== all[index - 1]);
    assert.deepEqual(states.slice(-3), ["stored", "delivered", "acknowledged"], `second tab: ${JSON.stringify(states)}`);
    assert.ok(!states.slice(0, -1).includes("acknowledged"), `second tab showed an acknowledgment early: ${JSON.stringify(states)}`);
    await other.close();
    passed.push(`second tab, correction pending: the refusal names the original and the pending correction, and the tab shows the correction ${states.slice(-3).join(", then ")}, never the original's acknowledgment`);
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
    assert.ok(rail[0].includes("delivered") && rail[0].includes("acknowledged by agent"), `rail: the acknowledged review shows ${JSON.stringify(rail[0])}`);
    assert.ok(rail[1].includes("delivered") && !rail[1].includes("acknowledged by agent"), `rail: the delivered review shows ${JSON.stringify(rail[1])}`);
    passed.push(`rail: a delivered review reads ${JSON.stringify(rail[1].slice(0, 2))} and an acknowledged one ${JSON.stringify(rail[0].slice(0, 3))}`);
  }

  // A service restart: the pending answer is delivered once, none is lost.
  // The form already holds an acknowledged answer, so this is a correction.
  {
    await form.locator("[data-cf-form-action='amend']").click();
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
    assert.deepEqual(lines(first.stdout).map((line) => [line.kind, line.event_id]), [["amendment", answer.event_id]], "restart: delivered once");
    const second = waitV2(1);
    assert.equal(second.status, 6, "restart: a later wait found an event");
    assert.equal(second.stdout, "");
    passed.push("restart: after the service is killed and show restarts it, the pending answer is delivered once and the next wait times out (exit 6)");
  }

  // Reload between send and Amend (TSK-176): the page loaded after the last
  // correction was sent, and Amend starts from its values, so changing one
  // field sends the others unchanged. Before the fix the controls opened
  // empty and the page refused the correction ("Answer this question.").
  {
    const [original] = lines(run(["present", "responses", "list", sessionId, "--form", "store-choice"]));
    await afterReload(form, "delivered");
    await form.locator("[data-cf-form-action='amend']").click();
    assert.ok(await field("home").locator("input[value='local']").isChecked(), "reload, Amend: the sent choice is not selected");
    assert.equal(await field("keep-days").locator("input").inputValue(), "30", "reload, Amend: the sent number is not in its field");
    await field("keep-days").locator("input").fill("31");
    await form.locator("[data-cf-form-action='submit']").click();
    const stored = await waitState(form, "stored");
    assert.equal(stored.says, "Stored, waiting for agent");
    const amended = lines(run(["present", "responses", "list", sessionId, "--form", "store-choice"])).at(-1);
    assert.equal(amended.kind, "amendment");
    assert.equal(amended.amends, original.event_id, "reload, Amend: the correction names another answer");
    assert.deepEqual(amended.values, { home: "local", "keep-days": 31 }, "reload, Amend: the other fields did not go unchanged");
    // Deliver it, so the form stands where the closure case below expects.
    const waited = waitV2(10);
    assert.equal(waited.status, 0, waited.stderr);
    assert.deepEqual(lines(waited.stdout).map((line) => line.event_id), [amended.event_id]);
    await waitState(form, "delivered");
    passed.push("reload, then Amend: the controls start from the last sent values; changing one field stores an amendment with the others unchanged");
  }

  // A field named constructor whose optional reason was left empty: after a
  // reload its reason stays empty, and a correction to another field sends
  // no reason for it (TSK-176 review). Before the fix the page read the
  // object's inherited constructor and sent it as the reviewer's reason.
  {
    const names = join(project, "names.json");
    await writeFile(names, `${JSON.stringify({
      schema_version: 2,
      title: "Inherited names",
      blocks: [{
        type: "form",
        id: "names",
        title: "Field names",
        fields: [
          { id: "constructor", label: "Constructor", kind: "text", rationale: "optional" },
          { id: "other", label: "Other", kind: "text" },
        ],
        required: ["constructor"],
      }],
    }, null, 2)}\n`);
    const opened = run(["present", "open", names, "--no-launch"]);
    const id = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    if (!id || !bootstrap) throw new Error(`could not parse present open output: ${opened}`);
    const port = JSON.parse(run(["present", "list"])).find((entry) => entry.id === id)?.service_port;
    const tab = await context.newPage();
    tab.on("pageerror", (error) => errors.push(error.message));
    try {
      await tab.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit", timeout: 120_000 });
      await tab.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
      await tab.locator("#cf-comment-toggle").waitFor({ state: "visible" });
      const named = tab.locator("article[data-cf-form='names']");
      const at = (field) => named.locator(`[data-cf-field='${field}']`);
      const settled = (state) => tab.waitForFunction((state) => document.querySelector("article[data-cf-form='names']")?.getAttribute("data-cf-form-state") === state, state, { timeout: 20_000 });
      await at("constructor").locator("[data-cf-value]").fill("first value");
      await at("other").locator("[data-cf-value]").fill("one");
      await named.locator("[data-cf-form-action='submit']").click();
      await settled("stored");
      await tab.reload({ waitUntil: "domcontentloaded" });
      await tab.locator("#cf-comment-toggle").waitFor({ state: "visible" });
      await settled("stored");
      assert.equal(await at("constructor").locator("[data-cf-value]").inputValue(), "first value");
      assert.equal(await at("constructor").locator("[data-cf-rationale-input]").inputValue(), "", "reload: an empty reason shows inherited text");
      await named.locator("[data-cf-form-action='amend']").click();
      await at("other").locator("[data-cf-value]").fill("two");
      await named.locator("[data-cf-form-action='submit']").click();
      await settled("stored");
      const [first, amended] = lines(run(["present", "responses", "list", id, "--form", "names"]));
      assert.deepEqual([first.rationales, amended.kind, amended.amends], [{}, "amendment", first.event_id]);
      assert.deepEqual(amended.values, { constructor: "first value", other: "two" });
      assert.deepEqual(amended.rationales, {}, "reload, Amend: the correction sent a reason the reviewer never gave");
    } finally {
      await tab.close();
      run(["present", "close", id]);
      run(["present", "clear", id, "--older-than", "0d"]);
    }
    passed.push("inherited name: a field named constructor with its optional reason left empty keeps an empty reason after a reload, and a correction to another field sends no reason for it");
  }

  // Closure cases run on their own sessions. Tab A stores the original O
  // with its polls held from the start; tab B, loaded after, stores the
  // correction M; the agent takes both and acknowledges them; then the
  // session closes. The closure body a held poll gets is the one the
  // service sent B. Page C, when asked for, loads before any answer.
  const inTab = (tab, id) => tab.locator(`article[data-cf-form='${id}']`);
  const shownIn = async (tab, id) => ({
    state: await inTab(tab, id).getAttribute("data-cf-form-state"),
    says: (await inTab(tab, id).locator("[data-cf-form-state]").innerText()).trim(),
    original: await inTab(tab, id).getAttribute("data-cf-answer-id"),
  });
  const stateIn = (tab, id, state) => tab.waitForFunction(({ id, state }) => document.querySelector(`article[data-cf-form='${id}']`)?.getAttribute("data-cf-form-state") === state, { id, state }, { timeout: 20_000 });
  const closeAfterCorrection = async (name, withC) => {
    const copy = join(project, `${name}.json`);
    await writeFile(copy, await readFile(fixture, "utf8"));
    const opened = run(["present", "open", copy, "--no-launch"]);
    const id = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    if (!id || !bootstrap) throw new Error(`could not parse present open output: ${opened}`);
    const port = JSON.parse(run(["present", "list"])).find((entry) => entry.id === id)?.service_port;
    let release;
    const gate = new Promise((done) => { release = done; });
    const session = { id, release, closure: null, tabs: [] };
    const held = async () => {
      const tab = await context.newPage();
      tab.on("pageerror", (error) => errors.push(error.message));
      await tab.route("**/app/api/events/poll", async (route) => {
        await gate;
        await route.fulfill({ status: 200, contentType: "application/json", body: session.closure });
      });
      session.tabs.push(tab);
      return tab;
    };
    const tabA = await held();
    await tabA.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit", timeout: 120_000 });
    await tabA.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
    await tabA.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    if (withC) {
      session.tabC = await held();
      await session.tabC.goto(tabA.url(), { waitUntil: "domcontentloaded" });
      await session.tabC.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    }

    const a = inTab(tabA, "store-choice");
    await a.locator("[data-cf-field='home'] input[value='local']").check();
    await a.locator("[data-cf-field='keep-days'] input").fill("30");
    await a.locator("[data-cf-form-action='submit']").click();
    await stateIn(tabA, "store-choice", "stored");

    const tabB = await context.newPage();
    tabB.on("pageerror", (error) => errors.push(error.message));
    session.tabs.push(tabB);
    await tabB.goto(tabA.url(), { waitUntil: "domcontentloaded" });
    await tabB.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    await stateIn(tabB, "store-choice", "stored");
    const b = inTab(tabB, "store-choice");
    await b.locator("[data-cf-form-action='amend']").click();
    await b.locator("[data-cf-field='home'] input[value='local']").check();
    await b.locator("[data-cf-field='keep-days'] input").fill("60");
    await b.locator("[data-cf-form-action='submit']").click();
    await stateIn(tabB, "store-choice", "stored");

    const [original, correction] = lines(run(["present", "responses", "list", id, "--form", "store-choice"]));
    assert.equal(correction.kind, "amendment");
    const delivered = cli(["present", "feedback", id, "--wait", "--timeout", "10", "--format", "v2"]);
    assert.equal(delivered.status, 0, delivered.stderr);
    assert.deepEqual(lines(delivered.stdout).map((line) => line.event_id), [original.event_id, correction.event_id]);
    run(["present", "ack", id, original.event_id]);
    run(["present", "ack", id, correction.event_id]);
    await stateIn(tabB, "store-choice", "acknowledged");
    assert.equal((await shownIn(tabA, "store-choice")).state, "stored", `${name}: tab A heard a state before the closure`);

    const closed = tabB.waitForResponse(async (response) => new URL(response.url()).pathname === "/app/api/events/poll"
      && (await response.json().catch(() => ({}))).kind === "session_closed", { timeout: 30_000 });
    run(["present", "close", id]);
    session.closure = await (await closed).text();
    return Object.assign(session, { tabA, original, correction });
  };
  const finish = async (session) => {
    for (const tab of session.tabs) await tab.close();
    run(["present", "clear", session.id, "--older-than", "0d"]);
  };

  // C120-R2-1: the closure is the first state A and C hear. A reload shows M
  // acknowledged, so A and C must close on it, with the original kept for a
  // correction.
  {
    const session = await closeAfterCorrection("forms-closure", true);
    const { tabA, tabC, original } = session;
    session.release();
    for (const tab of [tabA, tabC]) await stateIn(tab, "store-choice", "closed").catch(() => undefined);
    const results = { A: await shownIn(tabA, "store-choice"), C: await shownIn(tabC, "store-choice"), C_decision: await shownIn(tabC, "d-scope") };
    const expected = "Acknowledged by agent. This session is now closed; nothing more can be sent.";
    assert.deepEqual(
      { A: [results.A.state, results.A.says, results.A.original], C: [results.C.state, results.C.says, results.C.original] },
      { A: ["closed", expected, original.event_id], C: ["closed", expected, original.event_id] },
      `closure: tabs close where a reload would; the closure was ${session.closure}`,
    );
    assert.equal(results.C_decision.says, "This session is closed. Your draft is kept here; nothing can be sent.");
    await finish(session);
    passed.push(`closure rebinding: a tab following the original and a page that never saw an answer both close on the correction the other tab stored, "${expected}", with the original kept for a correction`);
  }

  // C120-R3-2: tab A's correction is refused as closed (410) before its
  // poll hears the closure. The refusal carries no answer, so A's words
  // are marked as last known; the held poll then brings the closure, and A
  // takes M acknowledged with the original kept. Nothing reopens, the
  // draft stays, and nothing is stored. The 410 is fulfilled by the test
  // with the service's refusal: a closed session's service stops.
  {
    const session = await closeAfterCorrection("forms-refused", false);
    const { tabA, original, correction } = session;
    const a = inTab(tabA, "store-choice");
    await a.locator("[data-cf-form-action='amend']").click();
    await a.locator("[data-cf-field='keep-days'] input").fill("90");
    await tabA.route("**/app/api/answers", (route) => route.fulfill({
      status: 410,
      contentType: "application/json",
      body: JSON.stringify({ error: "session_closed", message: "session is closed", details: {} }),
    }), { times: 1 });
    await a.locator("[data-cf-form-action='submit']").click();
    await stateIn(tabA, "store-choice", "closed");
    const refused = await shownIn(tabA, "store-choice");
    session.release();
    const expected = "Acknowledged by agent. This session is now closed; nothing more can be sent.";
    await tabA.waitForFunction((words) => document.querySelector("article[data-cf-form='store-choice'] [data-cf-form-state]")?.textContent === words, expected, { timeout: 20_000 }).catch(() => undefined);
    const after = await shownIn(tabA, "store-choice");
    assert.deepEqual([after.state, after.says, after.original], ["closed", expected, original.event_id],
      `refused first: the closure after a 410 was not taken; the tab says ${JSON.stringify(after)} after ${JSON.stringify(refused)}; the closure was ${session.closure}`);
    assert.equal(refused.says, "Last known: Stored, waiting for agent. This session is now closed; nothing more can be sent.");
    assert.equal(await a.locator("[data-cf-field='keep-days'] input").inputValue(), "90", "refused first: the draft changed");
    assert.ok(await a.locator("[data-cf-field='keep-days'] input").isDisabled(), "refused first: the draft is editable");
    assert.equal(await tabA.locator("[data-cf-form] [data-cf-form-action]:visible").count(), 0, "refused first: an action is offered");
    assert.equal((await shownIn(tabA, "d-scope")).says, "This session is closed. Your draft is kept here; nothing can be sent.");
    const stored = lines(run(["present", "responses", "list", session.id, "--form", "store-choice"]));
    assert.deepEqual(stored.map((line) => line.event_id), [original.event_id, correction.event_id], "refused first: something was stored");
    await finish(session);
    passed.push(`refused first: a correction refused as closed before the poll says "${refused.says}", then the closure makes it "${expected}" with the original kept, the draft read only and nothing stored`);
  }

  // The closure carries the states the page has not seen: the agent
  // acknowledged the correction just before the session closed. The page's
  // next poll gets that closure (fulfilled by the test: a closed session's
  // service stops), and the form shows the state before it latches.
  {
    const answered = lines(run(["present", "responses", "list", sessionId, "--form", "store-choice"]));
    const [original, correction] = [answered[0], answered.at(-1)];
    const digest = await form.getAttribute("data-cf-form-digest");
    let release;
    const gate = new Promise((done) => { release = done; });
    await page.route("**/app/api/events/poll", async (route) => {
      await gate;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          cursor: "9:9:9",
          kind: "session_closed",
          forms: [{ form_id: "store-choice", form_digest: digest, answer_id: original.event_id, latest_answer_id: correction.event_id, state: "acknowledged" }],
        }),
      });
    }, { times: 1 });
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    assert.equal((await stateOf(form)).state, "delivered", "closure: the correction was not shown as delivered");
    run(["present", "ack", sessionId, correction.event_id]);
    release();
    await waitState(form, "closed");
    const closed = await stateOf(form);
    assert.equal(closed.says, "Acknowledged by agent. This session is now closed; nothing more can be sent.");
    assert.equal(await form.locator(".cf-form__state-stored").innerText(), "Acknowledged by agent");
    // The decision is not in this closure, as a form past its bound would
    // not be: its words are the last known ones, and say so.
    const other = await stateOf(decision);
    assert.equal(other.says, "Last known: Delivered to agent. This session is now closed; nothing more can be sent.");
    passed.push(`closure: a closure carrying an acknowledgment the page had not seen shows "${closed.says}"`);
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

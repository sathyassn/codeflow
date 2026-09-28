// Forms end to end (TSK-119, SPC-014 B6): the real `codeflow` binary serves
// documents/v2-forms.json and a task-owned headless Chrome profile answers
// its form and its v2 decision through the page. Each case checks what the
// page shows and what the session's responses.jsonl holds afterwards:
// - nothing is preselected and the recommended option is only labelled;
// - a draft lives in page memory only: localStorage, sessionStorage,
//   IndexedDB and cookies hold none of its text, before and after a
//   revision notice, which the draft survives without a reload;
// - a request for the old revision is refused as stale, the draft is kept
//   and the reviewer confirms it against the current revision when the
//   question there is unchanged; a confirmation or correction the service
//   refuses leaves an operable send against the current revision;
// - a question changed at the current revision is never confirmed: the
//   draft is shown read only, as text, and the page asks for a reload;
// - the page refuses what the server would, before sending; a body the
//   server refuses as malformed or over 64 KiB is refused with its typed
//   error and the store is unchanged;
// - a failed request, one whose response was lost after the store took
//   it, and one the store could not take (a typed 503) are resent with the
//   same request id and stored once;
// - a correction is an amendment naming the original answer;
// - a decline carries its reason;
// - a session_closed refusal of an answer closes every form and the chrome,
//   which drops the review draft; closure while answers are in flight stays
//   closed when their replies arrive, stored or failed.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { access, chmod, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
const fixture = join(webRoot, "../tests/fixtures/contract-v2/documents/v2-forms.json");
const ANSWERS = "/app/api/answers";

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-forms-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
};
const run = (args) => {
  try {
    return execFileSync(codeflow, args, { cwd: project, env: environment, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 60_000 });
  } catch (error) {
    throw new Error(`codeflow ${args.join(" ")}: exit ${error.status}: ${String(error.stderr).trim()}`, { cause: error });
  }
};
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
  const port = JSON.parse(run(["present", "list"]))[0]?.service_port;

  context = await chromium.launchPersistentContext(join(root, "profile"), {
    executablePath: await findBrowser(),
    headless: true,
    viewport: { width: 1280, height: 900 },
    args: ["--disable-background-networking", "--disable-component-update", "--disable-default-apps", "--disable-sync", "--no-default-browser-check", "--no-first-run"],
  });
  const page = context.pages()[0] ?? await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const sent = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === ANSWERS) sent.push(request.postData() ?? "");
  });
  await page.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit", timeout: 120_000 });
  await page.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });

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
  const ledger = async () => {
    const path = await find(root, "responses.jsonl");
    if (!path) return [];
    return (await readFile(path, "utf8")).split("\n").filter(Boolean).map((line) => JSON.parse(line));
  };
  // A form's [hidden] parts must stay out of sight: a display rule in the
  // stylesheet beats the user agent's [hidden] rule unless the form's own wins.
  const hiddenShown = () => page.locator("[data-cf-form] [hidden]").evaluateAll((parts) => parts
    .filter((part) => getComputedStyle(part).display !== "none")
    .map((part) => part.className || part.getAttribute("data-cf-form-action") || part.tagName));
  const draftOf = () => form.evaluate((article) => [...article.querySelectorAll("[data-cf-value], [data-cf-rationale-input]")]
    .map((control) => (control.type === "radio" || control.type === "checkbox" ? `${control.id}:${control.checked}` : `${control.id}=${control.value}`)));

  // Nothing preselected; the recommended option is labelled only.
  {
    const checked = await page.locator("#cf-present-document input:checked").count();
    assert.equal(checked, 0, "render: an option is preselected");
    const filled = await page.locator("#cf-present-document .cf-field .cf-input").evaluateAll((inputs) => inputs.filter((input) => input.value !== "").length);
    assert.equal(filled, 0, "render: a field has a value");
    const recommended = await form.locator(".cf-recommended").evaluateAll((labels) => labels.map((label) => label.closest(".cf-option").querySelector("input").value));
    assert.deepEqual(recommended, ["local"], "render: the recommended label");
    assert.equal((await stateOf(form)).state, "editing");
    assert.deepEqual(await hiddenShown(), [], "render: a hidden part of a form shows");
    assert.equal(await form.locator("[data-cf-decline-reason]").isVisible(), false, "render: the decline reason shows before Decline");
    passed.push("render: nothing preselected, 'Recommended' labels local only, the form starts in editing with its hidden parts out of sight");
  }

  // A draft in page memory only, across a revision notice without a reload.
  const DRAFT_TEXT = "Kept only in this page";
  await field("home").locator("input[value='local']").check();
  await field("home").locator("[data-cf-rationale-input]").fill(DRAFT_TEXT);
  await field("keep-days").locator("input").fill("30");
  await field("channels").locator("input[value='rail']").check();
  await field("contact").locator("input").fill("reviewer@example.org");
  const storage = async (where) => {
    const kept = await page.evaluate(async () => ({
      local: Object.entries(localStorage),
      session: Object.entries(sessionStorage),
      databases: typeof indexedDB.databases === "function" ? (await indexedDB.databases()).map((database) => database.name) : [],
      cookie: document.cookie,
    }));
    const all = JSON.stringify(kept);
    for (const text of [DRAFT_TEXT, "reviewer@example.org", "\"30\""]) assert.ok(!all.includes(text), `${where}: browser storage holds ${text}: ${all}`);
    assert.deepEqual(kept.session, [], `${where}: sessionStorage is not empty`);
    assert.deepEqual(kept.databases, [], `${where}: an IndexedDB database exists`);
    assert.ok(kept.local.every(([key]) => /^cf-present-(theme|mode|typeface|scale|appearance)/u.test(key) || !key.includes("draft")), `${where}: localStorage ${JSON.stringify(kept.local)}`);
    return kept;
  };
  await storage("drafting");
  const draft = await draftOf();
  await page.evaluate(() => { window.cfFormsCheckMarker = "same page"; });
  const revised = JSON.parse(await readFile(fixture, "utf8"));
  revised.title = "Forms fixture, revision 2";
  await writeFile(join(project, "forms-2.json"), `${JSON.stringify(revised, null, 2)}\n`);
  run(["present", "update", sessionId, join(project, "forms-2.json")]);
  const staleNotice = await waitState(form, "stale");
  assert.equal(await page.evaluate(() => window.cfFormsCheckMarker), "same page", "revision notice: the page reloaded");
  assert.deepEqual(await draftOf(), draft, "revision notice: the draft changed");
  await storage("after the revision notice");
  passed.push(`draft: in page memory only (no text in localStorage, sessionStorage, IndexedDB or cookies) and kept across the revision notice without a reload; the form says ${JSON.stringify(staleNotice.says)}`);

  // Stale: the page's revision is 1, revision 2 is current.
  await form.locator("[data-cf-form-action='submit']").click();
  const stale = await waitState(form, "stale");
  assert.match(stale.says, /Revision 2 is current\. This question is unchanged there\. Confirm/u, `stale: ${stale.says}`);
  assert.ok(await form.locator("[data-cf-form-action='confirm']").isVisible(), "stale: no confirm");
  assert.deepEqual(await draftOf(), draft, "stale: the draft changed");
  assert.deepEqual(await ledger(), [], "stale: something was stored");
  // The service refuses the confirmation (its body is changed on the way
  // out): the confirmation is settled, and the form edits against revision 2
  // with an operable Submit that makes a new request (R119-4).
  const refuseOnce = async (change) => page.route(`**${ANSWERS}`, async (route) => {
    const body = JSON.parse(route.request().postData());
    change(body);
    await route.continue({ postData: JSON.stringify(body) });
  }, { times: 1 });
  await refuseOnce((body) => { body.values.home = "cloud"; });
  await form.locator("[data-cf-form-action='confirm']").click();
  const refusedConfirm = await waitState(form, "editing");
  assert.match(refusedConfirm.says, /service refused the answer/u, `refused confirm: ${refusedConfirm.says}`);
  assert.ok(await form.locator("[data-cf-form-action='submit']").isVisible(), "refused confirm: no Submit");
  assert.equal(await form.locator("[data-cf-form-action='confirm']").isVisible(), false, "refused confirm: Confirm is still offered");
  assert.deepEqual(await ledger(), [], "refused confirm: something was stored");
  await form.locator("[data-cf-form-action='submit']").click();
  const stored = await waitState(form, "stored");
  assert.equal(stored.says, "Stored, waiting for agent");
  let lines = await ledger();
  assert.equal(lines.length, 1, "confirm: one line");
  assert.equal(lines[0].revision, 2, "confirm: stored against revision 2");
  assert.deepEqual(lines[0].values, { home: "local", "keep-days": 30, channels: ["rail"], contact: "reviewer@example.org" });
  assert.deepEqual(lines[0].rationales, { home: DRAFT_TEXT });
  const [staleBody, confirmBody, sendBody] = sent.slice(-3).map((body) => JSON.parse(body));
  assert.equal(staleBody.revision, 1);
  assert.equal(confirmBody.revision, 2);
  assert.equal(sendBody.revision, 2, "after a refused confirm: the send names revision 2");
  assert.equal(new Set([staleBody.request_id, confirmBody.request_id, sendBody.request_id]).size, 3, "each send is a new request");
  passed.push(`stale: refused with the draft kept, then "${stale.says}"; a refused confirmation leaves Submit, which stores it against revision 2 as a new request`);

  // A correction is an amendment naming the original answer. Revision 3
  // leaves the question as it was: the correction is refused as stale and
  // confirmed; the service refuses that confirmation, and "Send correction"
  // still sends it, against revision 3 (R119-4).
  const third = JSON.parse(await readFile(fixture, "utf8"));
  third.title = "Forms fixture, revision 3";
  await writeFile(join(project, "forms-3.json"), `${JSON.stringify(third, null, 2)}\n`);
  run(["present", "update", sessionId, join(project, "forms-3.json")]);
  await form.locator("[data-cf-form-action='amend']").click();
  await field("keep-days").locator("input").fill("7");
  await form.locator("[data-cf-form-action='submit']").click();
  const staleCorrection = await waitState(form, "stale");
  assert.match(staleCorrection.says, /Revision 3 is current\. This question is unchanged there\./u, staleCorrection.says);
  await refuseOnce((body) => { body.values.home = "cloud"; });
  await form.locator("[data-cf-form-action='confirm']").click();
  await waitState(form, "editing");
  const correct = form.locator("[data-cf-form-action='submit']");
  assert.ok(await correct.isVisible(), "refused corrected confirm: no send action");
  assert.equal(await correct.innerText(), "Send correction");
  await correct.click();
  await waitState(form, "stored");
  lines = await ledger();
  assert.equal(lines.length, 2);
  assert.equal(lines[1].event, "amendment");
  assert.equal(lines[1].amends, lines[0].answer_id);
  assert.equal(lines[1].revision, 3);
  assert.equal(lines[1].values["keep-days"], 7);
  passed.push("amendment: 'Correct this answer' stores an amendment naming the original answer, both lines stay; a refused confirmation of a stale correction leaves 'Send correction', which stores it against revision 3");

  // The page refuses before sending what the server would refuse.
  {
    const before = sent.length;
    await decision.locator("[data-cf-form-action='submit']").click();
    const said = await stateOf(decision);
    assert.equal(said.state, "stale", "page check: state");
    assert.match(await decision.locator(".cf-field__error").innerText(), /Answer this question\./u);
    // The error marks the unanswered choice, not the optional rationale.
    const marked = await decision.locator("[aria-invalid='true']").evaluateAll((controls) => controls.map((control) => control.type));
    assert.ok(marked.length > 0 && marked.every((type) => type === "radio"), `page check: the error marks ${JSON.stringify(marked)}`);
    assert.equal(await decision.locator("[data-cf-rationale-input]").getAttribute("aria-invalid"), null, "page check: the rationale is marked");
    await decision.locator("input[value='b']").check();
    await form.locator("[data-cf-form-action='amend']").click();
    await field("contact").locator("input").fill("not-an-address");
    await form.locator("[data-cf-form-action='submit']").click();
    assert.match(await field("contact").locator(".cf-field__error").innerText(), /email address/u);
    await field("keep-days").locator("input").fill("1.5");
    await form.locator("[data-cf-form-action='submit']").click();
    assert.match(await field("keep-days").locator(".cf-field__error").innerText(), /whole number/u);
    assert.equal(sent.length, before, "page check: a request was sent");
    assert.equal((await ledger()).length, 2);
    passed.push("page checks: a missing required choice, a bad email and a fraction are refused on the page with the field's message marking only the answer, and nothing is sent");
  }

  // Server refusals reach the page as typed errors; the draft is kept and
  // the store is unchanged. The route changes the body on its way out.
  {
    const before = await ledger();
    await field("contact").locator("input").fill("reviewer@example.org");
    await field("keep-days").locator("input").fill("7");
    const kept = await draftOf();
    await page.route(`**${ANSWERS}`, async (route) => {
      const body = JSON.parse(route.request().postData());
      body.values.home = "cloud";
      await route.continue({ postData: JSON.stringify(body) });
    }, { times: 1 });
    await form.locator("[data-cf-form-action='submit']").click();
    await page.waitForFunction(() => document.querySelector("[data-cf-field='home'] .cf-field__error")?.hidden === false);
    assert.match(await field("home").locator(".cf-field__error").innerText(), /Choose one of the options/u);
    assert.match((await stateOf(form)).says, /service refused the answer/u);
    assert.deepEqual(await draftOf(), kept, "invalid_answer: the draft changed");
    assert.deepEqual(await ledger(), before, "invalid_answer: the store changed");

    await page.route(`**${ANSWERS}`, async (route) => {
      const body = JSON.parse(route.request().postData());
      body.values.notes = "x".repeat(70_000);
      await route.continue({ postData: JSON.stringify(body) });
    }, { times: 1 });
    await form.locator("[data-cf-form-action='submit']").click();
    await page.waitForFunction(() => /64 KiB limit/u.test(document.querySelector("article[data-cf-form='store-choice'] [data-cf-form-state]")?.textContent ?? ""));
    assert.deepEqual(await draftOf(), kept, "answer_too_large: the draft changed");
    assert.deepEqual(await ledger(), before, "answer_too_large: the store changed");

    // Malformed JSON from the page's origin with the page's headers.
    const malformed = await page.evaluate(async (path) => {
      const response = await fetch(path, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json", "X-CF-Present": "1" }, body: "{\"request_id\":" });
      return { status: response.status, body: await response.json() };
    }, ANSWERS);
    assert.equal(malformed.status, 422);
    assert.equal(malformed.body.error, "invalid_answer");
    assert.deepEqual(await ledger(), before, "malformed: the store changed");

    // Over 64 KiB typed on the page: refused before sending.
    const count = sent.length;
    await field("home").locator("[data-cf-rationale-input]").fill("y".repeat(70_000));
    await form.locator("[data-cf-form-action='submit']").click();
    assert.match((await stateOf(form)).says, /over the 64 KiB limit/u);
    assert.equal(sent.length, count, "oversized draft: a request was sent");
    await field("home").locator("[data-cf-rationale-input]").fill(DRAFT_TEXT);
    passed.push("typed refusals: invalid_answer shows the field error, answer_too_large the limit, malformed JSON is a 422; each keeps the draft and leaves the store unchanged; a draft over 64 KiB is not sent");
  }

  // A failed request is resent with the same bytes, and a response lost
  // after the store took the answer is resent and stored once.
  {
    const before = (await ledger()).length;
    await page.route(`**${ANSWERS}`, (route) => route.abort("connectionreset"), { times: 1 });
    await form.locator("[data-cf-form-action='submit']").click();
    const failed = await waitState(form, "failed");
    assert.match(failed.says, /Not confirmed as stored/u);
    assert.equal((await ledger()).length, before, "failed: something was stored");
    const first = sent.at(-1);
    await page.route(`**${ANSWERS}`, async (route) => {
      await route.fetch();
      await route.abort("connectionreset");
    }, { times: 1 });
    await form.locator("[data-cf-form-action='resend']").click();
    await waitState(form, "failed");
    const second = sent.at(-1);
    assert.equal(second, first, "resend: the bytes differ");
    assert.equal((await ledger()).length, before + 1, "lost response: the store did not take it");
    await form.locator("[data-cf-form-action='resend']").click();
    await waitState(form, "stored");
    assert.equal(sent.at(-1), first, "second resend: the bytes differ");
    const after = await ledger();
    assert.equal(after.length, before + 1, "lost response: stored twice");
    assert.equal(after.at(-1).request_id, JSON.parse(first).request_id);
    passed.push("failed and lost responses: a reset request shows Resend; the same bytes are resent, and when the store took them before the response was lost, the resend returns the original receipt and the answer is stored once");
  }

  // The store is unavailable (the ledger cannot be opened): a typed 503.
  // The page keeps the draft and offers the same request again; once the
  // store is back, the resend stores the answer once.
  {
    const path = await find(root, "responses.jsonl");
    const before = await ledger();
    await form.locator("[data-cf-form-action='amend']").click();
    await field("keep-days").locator("input").fill("21");
    const kept = await draftOf();
    await chmod(path, 0o400);
    let failed;
    try {
      await form.locator("[data-cf-form-action='submit']").click();
      failed = await waitState(form, "failed");
    } finally {
      await chmod(path, 0o600);
    }
    assert.match(failed.says, /Not confirmed as stored/u, `store unavailable: ${failed.says}`);
    assert.doesNotMatch(failed.says, /Not stored|was not stored/u);
    assert.ok(await form.locator("[data-cf-form-action='resend']").isVisible(), "store unavailable: no resend offer");
    assert.deepEqual(await draftOf(), kept, "store unavailable: the draft changed");
    assert.deepEqual(await ledger(), before, "store unavailable: the store changed");
    const refused = sent.at(-1);
    await form.locator("[data-cf-form-action='resend']").click();
    await waitState(form, "stored");
    assert.equal(sent.at(-1), refused, "store unavailable: the resend differs");
    const after = await ledger();
    assert.equal(after.length, before.length + 1);
    assert.equal(after.at(-1).request_id, JSON.parse(refused).request_id);
    assert.equal(after.at(-1).values["keep-days"], 21);
    passed.push(`store unavailable: a 503 keeps the draft and offers the same request again ("${failed.says}"); the resend stores it once`);
  }

  // A decline carries its reason.
  {
    const reason = decision.locator("[data-cf-decline-reason]");
    assert.equal(await reason.isVisible(), false, "decline: the reason shows before Decline");
    await decision.locator("[data-cf-form-action='decline']").click();
    assert.equal(await reason.isVisible(), true, "decline: Decline does not show the reason");
    await reason.fill("Not my call.");
    await decision.locator("[data-cf-form-action='decline']").click();
    // The decision still names revision 1: confirm it against revision 3.
    const stale = await waitState(decision, "stale");
    assert.match(stale.says, /Revision 3 is current/u);
    await decision.locator("[data-cf-form-action='confirm']").click();
    await waitState(decision, "stored");
    const line = (await ledger()).at(-1);
    assert.equal(line.form_id, "d-scope");
    assert.equal(line.outcome, "decline");
    assert.equal(line.reason, "Not my call.");
    assert.deepEqual(line.values, {});
    assert.equal(await reason.isVisible(), false, "decline: the reason shows after the decline is stored");
    assert.deepEqual(await hiddenShown(), [], "decline: a hidden part of a form shows");
    passed.push("decline: the reason box shows only after Decline, is stored with no values, and hides once stored");
  }

  // Revision 4 relabels an option under the same value: the question the
  // reviewer saw is not the current one, so a correction is never
  // confirmed. The draft stays read only and is listed as text (R119-3).
  {
    const before = await ledger();
    const fourth = JSON.parse(await readFile(fixture, "utf8"));
    fourth.title = "Forms fixture, revision 4";
    const home = fourth.blocks.find((block) => block.id === "store-choice").fields.find((item) => item.id === "home");
    home.options.find((option) => option.value === "local").label = "Store on this machine only";
    await writeFile(join(project, "forms-4.json"), `${JSON.stringify(fourth, null, 2)}\n`);
    run(["present", "update", sessionId, join(project, "forms-4.json")]);
    await form.locator("[data-cf-form-action='amend']").click();
    await field("keep-days").locator("input").fill("12");
    const kept = await draftOf();
    await form.locator("[data-cf-form-action='submit']").click();
    const changed = await waitState(form, "changed");
    assert.equal(changed.says, "This question changed in revision 4. Copy your draft from the list below, then reload the page to answer the current question.");
    assert.equal(await form.locator("[data-cf-form-action]:visible").count(), 0, "changed: an action is offered");
    assert.deepEqual(await draftOf(), kept, "changed: the draft changed");
    assert.ok(await field("keep-days").locator("input").isDisabled(), "changed: the draft is editable");
    const listed = await form.locator(".cf-form__kept li").allInnerTexts();
    for (const line of ["Store: Private local store", `Store, why: ${DRAFT_TEXT}`, "Days to keep: 12", "Where to show state: Rail", "Follow-up address: reviewer@example.org"]) {
      assert.ok(listed.includes(line), `changed: the draft list lacks ${JSON.stringify(line)}: ${JSON.stringify(listed)}`);
    }
    assert.deepEqual(await ledger(), before, "changed: something was stored");
    const staleBody = JSON.parse(sent.at(-1));
    assert.equal(staleBody.revision, 3, "changed: the correction named another revision");
    passed.push(`changed question: a relabelled option at revision 4 gives "${changed.says}", no action, the draft read only and listed as text (${listed.length} lines), and nothing stored`);
  }

  // A session_closed refusal of an answer (fulfilled here by the test, as
  // the service stops once a session closes) closes every form and the
  // chrome, which drops the restored review draft.
  {
    const draftKey = `cf-present-draft:${sessionId}`;
    await page.evaluate((key) => sessionStorage.setItem(key, JSON.stringify({
      revision: 4,
      notes: [{ client_id: "restored-note", block_id: "store-choice", block_label: "Where should answers live?", kind: "comment", body: "A review note kept across the reload.", target_summary: "Where should answers live?" }],
      verdict: "approve_with_notes",
      instruction: "",
    })), draftKey);
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    await page.getByTestId("toast").getByText(/Restored 1 unsent note/u).waitFor({ timeout: 20_000 });
    // The reload shows the stored decline; a new choice starts from Amend.
    assert.equal((await stateOf(decision)).state, "stored", "reload: the stored decline is not shown");
    await decision.locator("[data-cf-form-action='amend']").click();
    await decision.locator("input[value='a']").check();
    await page.route(`**${ANSWERS}`, (route) => route.fulfill({
      status: 410,
      contentType: "application/json",
      body: JSON.stringify({ error: "session_closed", message: "session is closed", details: {} }),
    }), { times: 1 });
    await decision.locator("[data-cf-form-action='submit']").click();
    await waitState(decision, "closed");
    await waitState(form, "closed");
    await page.getByTestId("toast").getByText(/This review session is closed\./u).waitFor({ timeout: 20_000 });
    await page.waitForFunction((key) => sessionStorage.getItem(key) === null, draftKey, { timeout: 10_000 });
    assert.ok(await field("keep-days").locator("input").isDisabled(), "answer 410: a sibling form is editable");
    passed.push("answer 410: a session_closed refusal closes the form, its sibling and the chrome, and drops the restored review draft");
  }

  // Closure while answers are in flight: the replies arrive after it, one
  // stored and one failed, and neither reopens its form (R119-7).
  {
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    await field("home").locator("input[value='local']").check();
    await field("keep-days").locator("input").fill("5");
    await field("channels").locator("input[value='rail']").check();
    await field("contact").locator("input").fill("reviewer@example.org");
    await decision.locator("[data-cf-form-action='amend']").click();
    await decision.locator("input[value='a']").check();
    const before = (await ledger()).length;
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    const failedRequest = page.waitForEvent("requestfailed", (request) => new URL(request.url()).pathname === ANSWERS);
    await page.route(`**${ANSWERS}`, async (route) => {
      const body = JSON.parse(route.request().postData());
      if (body.form_id === "store-choice") {
        const response = await route.fetch();
        await gate;
        await route.fulfill({ response });
      } else {
        await gate;
        await route.abort("connectionreset");
      }
    }, { times: 2 });
    await form.locator("[data-cf-form-action='submit']").click();
    await decision.locator("[data-cf-form-action='submit']").click();
    await waitState(form, "submitting");
    await waitState(decision, "submitting");
    for (let attempt = 0; (await ledger()).length === before; attempt += 1) {
      assert.ok(attempt < 100, "in flight: the store never took the answer");
      await new Promise((done) => setTimeout(done, 100));
    }
    const kept = await draftOf();
    run(["present", "close", sessionId]);
    await waitState(form, "closed");
    await waitState(decision, "closed");
    release();
    await failedRequest;
    await page.waitForFunction(() => /Stored, waiting for agent\. This session is now closed/u.test(document.querySelector("article[data-cf-form='store-choice'] [data-cf-form-state]")?.textContent ?? ""), null, { timeout: 20_000 });
    const storedLate = await stateOf(form);
    const failedLate = await stateOf(decision);
    assert.equal(storedLate.state, "closed", "late receipt: the form reopened");
    assert.equal(failedLate.state, "closed", "late failure: the form reopened");
    // The decision's correction failed, but its stored decline stands: it
    // closes on that answer's state, as a reload shows it.
    assert.equal(failedLate.says, "Stored, waiting for agent. This session is now closed; nothing more can be sent.");
    // The late receipt reads as stored first, in the stored colour, inside
    // the one status region; the closed sentence follows it.
    const colours = await form.locator("[data-cf-form-state]").evaluate((line) => {
      const stored = line.querySelector(".cf-form__state-stored");
      return {
        role: line.getAttribute("role"),
        first: stored === line.firstChild,
        stored: stored ? getComputedStyle(stored).color : null,
        closed: getComputedStyle(line).color,
      };
    });
    assert.equal(colours.role, "status");
    assert.ok(colours.first, "late receipt: the stored part is not first");
    assert.ok(colours.stored && colours.stored !== colours.closed, `late receipt: the stored part is in the closed colour ${JSON.stringify(colours)}`);
    assert.equal(await page.locator("[data-cf-form] [data-cf-form-action]:visible").count(), 0, "after closure: an action is offered");
    assert.ok(await field("keep-days").locator("input").isDisabled(), "late receipt: the draft is editable");
    assert.ok(await decision.locator("input[value='a']").isDisabled(), "late failure: the draft is editable");
    assert.deepEqual(await draftOf(), kept, "closed: the draft changed");
    assert.equal((await ledger()).length, before + 1, "in flight: the store holds the answer once");
    passed.push(`closed in flight: a receipt that lands after closure says "${storedLate.says}", its stored part first in the stored colour, and a failed correction keeps its stored answer: "${failedLate.says}"; neither form reopens, and the draft stays read only`);
  }
  // The page never sends null: an unanswered field is absent. (The one
  // malformed body above was sent by the test, not the page.)
  for (const body of sent.filter((text) => text !== "{\"request_id\":")) {
    const values = JSON.parse(body).values ?? {};
    assert.ok(Object.values(values).every((value) => value !== null), `a request sent null: ${body}`);
  }
  passed.push(`no null: none of the ${sent.length - 1} answer requests the page sent holds a null value`);
  assert.deepEqual(errors, [], "page errors");
  await storage("at the end");
  await context.close();
  context = null;
  run(["present", "clear", sessionId, "--older-than", "0d"]);
  sessionId = null;
  process.stdout.write(`cf-present form checks passed:\n${passed.map((line) => `  ${line}`).join("\n")}\n`);
} finally {
  if (context) await context.close().catch(() => undefined);
  if (sessionId) {
    try { run(["present", "close", sessionId]); } catch { /* the service may already be gone */ }
  }
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

async function find(directory, name) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isFile() && entry.name === name) return path;
    if (entry.isDirectory() && !entry.isSymbolicLink()) {
      const found = await find(path, name).catch(() => null);
      if (found) return found;
    }
  }
  return null;
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

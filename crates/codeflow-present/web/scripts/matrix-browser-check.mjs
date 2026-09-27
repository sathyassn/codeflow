// The annotation matrix end to end (TSK-071): the real `codeflow` binary
// serves a document with one block of every type, and a task-owned headless
// Chrome profile takes a note on every applicable cell of the block by gesture
// matrix in `cf-present/references/document-authoring.md` by a real gesture:
// a mouse selection for text, a click for an element, a drag for an area.
// `codeflow present feedback` must return each note with its kind and
// selector, a text note its quote with prefix and suffix, and every element
// and area note a JPEG crop the size of what it anchors (within 2 px at the
// capture scale) that is not one colour. A cell that fails names its block,
// its gesture and the step.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
const fixture = join(webRoot, "../tests/fixtures/annotation/every-block.json");
const reference = join(repoRoot, "assets/base/agents/skills/cf-present/references/document-authoring.md");
const MAX_CROP = { width: 480, height: 360 };

// How each applicable cell is exercised. `text` selects `words` inside the
// first `in` element that holds them; `element` clicks the centre of `click`
// and expects `label` in the chip; `area` Shift-drags a box inside `box`, or
// the whole block. `entities` marks the cells TSK-118 proves in
// check:entities (figure parts and stage entities, with PNG crops).
const RECIPES = {
  narrative: { block: "prose", text: { in: "p", words: "runtime draws" }, element: { click: "h2", label: "Why this page exists" } },
  bullets: { block: "points", text: { in: "li", words: "quote them" }, element: { click: "li:nth-of-type(2)", label: "Click a part to name it." } },
  callout: { block: "caution", text: { in: "p", words: "stay on this machine" }, element: { click: "h2", label: "Held work" } },
  comparison: { block: "options", text: { in: "li", words: "smaller reviews" }, element: { click: "article:nth-of-type(2) h2", label: "Switch to PNG" } },
  decision: { block: "choice", text: { in: "p", words: "element and area crops" }, element: { click: "h2", label: "Which crop format ships?" } },
  table: { block: "limits", text: { in: "td", words: "256 KiB" }, element: { click: "tbody tr:nth-of-type(3) td:nth-of-type(1)", label: "Quote length" } },
  status: { block: "checks", text: { in: "li span", words: "one crop was a sliver" }, element: { click: "li:nth-of-type(2)", label: "Linux Chrome run" } },
  code: { block: "snippet", text: { in: "code", words: "256 * 1024" }, element: { click: ".cf-line:nth-of-type(2)", label: "let limit = 256 * 1024;" } },
  diff: { block: "change", text: { in: "del", words: "128 * 1024" }, element: { click: "ins", label: "let limit = 256 * 1024;" } },
  tree: { block: "files", text: { in: "li", words: "state.rs" }, element: { click: "li li li:nth-of-type(2) > span", label: "limits.rs" } },
  figure: { block: "flow", element: { entities: true } },
  media: { block: "screenshot", text: { in: "figcaption", words: "stands in" }, element: { click: "img", label: "Four coloured quadrants" }, area: { box: "img", plain: true } },
  disclosure: { block: "more", text: { in: "summary", words: "More detail", context: false }, element: { click: "summary", label: "More detail" }, area: { closed: "details" } },
  tabs: { block: "views", text: { in: ".tabs__labels", words: "Risks" }, element: { click: "details:nth-of-type(2) > summary", label: "Risks" }, area: { closed: "details" } },
  feedback_prompt: { block: "ask", text: { in: "p", words: "Mark anything" }, element: { click: "p", label: "Mark anything that is wrong, then submit the review." } },
  html: { block: "stage", text: { in: "svg text", words: "Service" }, element: { entities: true } },
};

const matrix = readMatrix(await readFile(reference, "utf8"));
assert.deepEqual(Object.keys(RECIPES).sort(), [...matrix.keys()].sort(), "the suite and the matrix name different block types");
const cells = [];
for (const [type, row] of matrix) {
  for (const gesture of ["text", "element", "area"]) {
    const recipe = RECIPES[type];
    const has = gesture === "area" || Boolean(recipe[gesture]);
    assert.equal(has, row[gesture], `${type} ${gesture}: the matrix says ${row[gesture] ? "yes" : "no"}, the suite ${has ? "has" : "lacks"} a recipe`);
    if (!row[gesture]) continue;
    if (recipe[gesture]?.entities) continue;
    cells.push({ type, gesture, block: recipe.block, recipe: recipe[gesture] ?? {} });
  }
}

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-matrix-"));
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
try {
  for (const directory of [project, environment.HOME, environment.TMPDIR, environment.XDG_STATE_HOME, join(root, "profile")]) await mkdir(directory, { recursive: true });
  execFileSync("git", ["init", "--quiet"], { cwd: project, env: environment });
  const document = join(project, "every-block.json");
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
  await page.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit", timeout: 120_000 });
  await page.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  // Figures draw and code highlights when they scroll into view.
  await page.locator("[data-cf-block-id='flow']").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-block-id='flow'] [data-cf-figure-block]")?.getAttribute("data-cf-figure-block") === "ready");
  await page.locator("[data-cf-block-id='snippet'] code").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-block-id='snippet'] code")?.getAttribute("data-cf-highlight") === "ready");

  // The sections list names no block hidden in a disclosure or tab (QA defect 8).
  const listed = await page.locator(".cf-section-route a").evaluateAll((links) => links.map((link) => link.getAttribute("href")));
  for (const hidden of ["more-text", "plan-text", "risk-text"]) assert.ok(!listed.includes(`#${hidden}`), `the sections list names the hidden block ${hidden}`);
  for (const shown of ["prose", "more", "views"]) assert.ok(listed.includes(`#${shown}`), `the sections list lacks ${shown}: ${listed.join(" ")}`);
  await phoneWidth(page);
  process.stdout.write("sections and phone width passed: no hidden block in the sections list; at 375 px the sheet stays closed on arming, a gesture, a save and a pin, the float keeps ESC on screen, Comment opens the sheet and Done leaves\n");

  const expected = [];
  for (const cell of cells) {
    const where = `${cell.type} ${cell.gesture}`;
    const step = async (name, action) => {
      try {
        return await action();
      } catch (error) {
        throw new Error(`${where}, ${name}: ${error.message.split("\n")[0]}`, { cause: error });
      }
    };
    await step("arm Comment", () => armComment(page));
    await step("reveal the block", () => reveal(page, cell.block));
    const pinned = await step("gesture", () => gesture(page, cell));
    await step("save the note", () => saveNote(page, `matrix: ${where}`));
    expected.push({ ...cell, where, ...pinned });
  }
  // A whole-document note crops what is on screen, not the whole page
  // squeezed into a sliver (QA defect 8).
  await armComment(page);
  await reveal(page, "options");
  const onScreen = await page.evaluate(() => {
    const rect = document.getElementById("cf-present-document").getBoundingClientRect();
    return { width: Math.min(rect.right, innerWidth) - Math.max(rect.left, 0), height: Math.min(rect.bottom, innerHeight) - Math.max(rect.top, 0) };
  });
  await page.evaluate(() => { document.querySelector("details.cf-tools").open = true; });
  await page.getByTestId("tool-whole-doc").click();
  await saveNote(page, "matrix: whole document");
  expected.push({ where: "whole document", gesture: "document", box: onScreen, summary: `Region: the ${Math.round(onScreen.width)}×${Math.round(onScreen.height)} px on screen` });

  await page.getByLabel("Verdict").selectOption("approve_with_notes");
  await page.getByRole("button", { name: "Submit review" }).click();
  await page.getByRole("status").getByText(/Review received/u).waitFor({ timeout: 60_000 }).catch(async (error) => {
    const said = await page.locator("[role=status], [role=alert]").allInnerTexts();
    throw new Error(`submit: no "Review received"; the page says ${JSON.stringify(said)}`, { cause: error });
  });
  assert.deepEqual(errors, []);

  // The v1 stream carries every note, its kind, its selector and its crop.
  const lines = run(["present", "feedback", sessionId]).trim().split("\n").filter(Boolean);
  assert.equal(lines.length, 1, "one review envelope");
  const envelope = JSON.parse(lines[0]);
  assert.equal(envelope.notes.length, expected.length);
  const images = [];
  for (const cell of expected) {
    const note = envelope.notes.find((candidate) => candidate.body === `matrix: ${cell.where}`);
    assert.ok(note, `${cell.where}, delivery: no note in the envelope`);
    if (cell.gesture !== "document") assert.equal(note.block_id, cell.expectBlock ?? cell.block, `${cell.where}, delivery: block`);
    if (cell.gesture === "text") {
      assert.equal(note.selector?.exact, cell.recipe.words, `${cell.where}, delivery: quote`);
      assert.equal(typeof note.selector.prefix, "string", `${cell.where}, delivery: prefix`);
      assert.equal(typeof note.selector.suffix, "string", `${cell.where}, delivery: suffix`);
      // Context is the text around the quote in its root; a quote that is
      // the whole root (a disclosure summary) has none.
      if (cell.recipe.context !== false) assert.ok(note.selector.prefix.length + note.selector.suffix.length > 0, `${cell.where}, delivery: no context around the quote`);
      continue;
    }
    if (cell.gesture === "element") {
      assert.ok(note.element_selector?.element_path, `${cell.where}, delivery: no element selector`);
      assert.ok(note.element_selector.label.includes(cell.recipe.label), `${cell.where}, delivery: label ${JSON.stringify(note.element_selector.label)}`);
    } else {
      assert.ok(note.region_selector, `${cell.where}, delivery: no region selector`);
      assert.equal(note.region_selector.scope, cell.gesture === "document" ? "document" : "block", `${cell.where}, delivery: region scope`);
    }
    assert.equal(note.excerpt?.image?.media_type, "image/jpeg", `${cell.where}, delivery: crop format`);
    images.push({ where: cell.where, data: note.excerpt.image.data_base64, box: cell.box });
  }
  // Decode each crop in the browser: its size against the anchored bounds at
  // the capture scale, and more than one colour.
  const decoded = await page.evaluate(async (items) => Promise.all(items.map(async ({ where, data }) => {
    const image = new Image();
    image.src = `data:image/jpeg;base64,${data}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d");
    context.drawImage(image, 0, 0);
    const pixels = context.getImageData(0, 0, image.width, image.height).data;
    const colours = new Set();
    for (let index = 0; index < pixels.length && colours.size < 3; index += 16) {
      colours.add(`${pixels[index] >> 4},${pixels[index + 1] >> 4},${pixels[index + 2] >> 4}`);
    }
    return { where, width: image.width, height: image.height, colours: colours.size };
  })), images.map(({ where, data }) => ({ where, data })));
  // One saved crop per cell for the verification record, on request.
  if (process.env.CF_PRESENT_EVIDENCE_DIR) {
    await mkdir(process.env.CF_PRESENT_EVIDENCE_DIR, { recursive: true });
    for (const { where, data } of images) {
      await writeFile(join(process.env.CF_PRESENT_EVIDENCE_DIR, `${where.replace(/\s+/gu, "-")}.jpg`), Buffer.from(data, "base64"));
    }
  }
  for (const [index, crop] of decoded.entries()) {
    const { box } = images[index];
    const scale = Math.min(MAX_CROP.width / box.width, MAX_CROP.height / box.height, 1);
    assert.ok(Math.abs(crop.width - box.width * scale) <= 2 && Math.abs(crop.height - box.height * scale) <= 2,
      `${crop.where}, crop: ${crop.width} x ${crop.height} px for bounds ${box.width.toFixed(1)} x ${box.height.toFixed(1)} at scale ${scale.toFixed(3)}`);
    assert.ok(crop.colours > 1, `${crop.where}, crop: one colour`);
  }
  // Lifecycle on the same session (TSK-071 criterion 4).
  // An approval with no notes can be sent (QA defect 9). Submitting turned
  // Comment off, which closes the rail.
  await armComment(page);
  await page.getByLabel("Verdict").selectOption("approve");
  const submit = page.getByTestId("submit-all");
  assert.equal(await submit.isEnabled(), true, "lifecycle: Submit is disabled for an approval with no notes");
  await submit.click();
  const approval = await nextEnvelope();
  assert.equal(approval.verdict, "approve");
  assert.equal(approval.notes.length, 0);
  // An unsent note survives the revision and a reload (QA defect 4).
  const draftCell = { type: "bullets", gesture: "text", block: "points", recipe: RECIPES.bullets.text };
  await armComment(page);
  await reveal(page, "points");
  await gesture(page, draftCell);
  await saveNote(page, "draft: kept across the reload");
  // update: a revision that keeps the prose and drops the decision re-anchors
  // the prose note and orphans the decision notes with their reason.
  const revised = JSON.parse(await readFile(fixture, "utf8"));
  revised.blocks = revised.blocks.filter((block) => block.id !== "choice");
  revised.blocks[0].markdown = `An added opening line.\n\n${revised.blocks[0].markdown}`;
  await writeFile(join(project, "every-block-2.json"), `${JSON.stringify(revised, null, 2)}\n`);
  run(["present", "update", sessionId, join(project, "every-block-2.json")]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  await armComment(page);
  const restored = await page.getByTestId("toast").innerText();
  assert.match(restored, /Restored 1 unsent note from revision 1\./u, "lifecycle, draft: no restore notice");
  await armComment(page);
  assert.equal(await page.getByTestId("note-row").count(), 1, "lifecycle, draft: the unsent note is gone");
  await page.getByLabel("Verdict").selectOption("approve_with_notes");
  await page.getByTestId("submit-all").click();
  const kept = await nextEnvelope();
  assert.equal(kept.revision, 2);
  assert.deepEqual(kept.notes.map((note) => note.body), ["draft: kept across the reload"], "lifecycle, draft: delivered notes");
  assert.equal(await page.evaluate((id) => sessionStorage.getItem(`cf-present-draft:${id}`), sessionId), null, "lifecycle, draft: kept after submit");
  await armComment(page);
  const earlier = page.getByTestId("feedback-history");
  await earlier.waitFor({ timeout: 20_000 });
  await earlier.locator("summary").click();
  const notices = await earlier.innerText();
  assert.match(notices, /Unpositioned: /u, `lifecycle, update: no orphan notice: ${notices.slice(0, 800)}`);
  assert.match(notices, /Matched uniquely in this revision\.|Moved: /u, `lifecycle, update: no re-anchor notice`);
  // resolve closes the first review; history shows both revisions, both reviews and the resolution.
  const before = JSON.parse(run(["present", "history", sessionId]));
  const delivered = before.feedback_events.filter((event) => event.event === "delivered" && event.event_id === envelope.event_id).at(-1);
  run(["present", "resolve", sessionId, envelope.event_id, "--event-version", String(delivered.sequence), "--status", "addressed"]);
  const history = JSON.parse(run(["present", "history", sessionId]));
  assert.equal(history.revisions.length, 2, "lifecycle, history: revisions");
  assert.equal(history.feedback_events.filter((event) => event.event === "received").length, 3, "lifecycle, history: reviews");
  assert.ok(history.feedback_events.some((event) => event.event === "addressed" && event.event_id === envelope.event_id), "lifecycle, history: resolution");
  // export: the document without chrome or markers.
  const exported = join(root, "every-block-export.html");
  run(["present", "export", sessionId, "--out", exported, "--mode", "light"]);
  const html = await readFile(exported, "utf8");
  // The inlined stylesheet names the chrome classes; no element carries them.
  for (const chrome of [/id="cf-present-chrome"/u, /class="[^"]*\bcf-marker/u, /data-testid="/u, /id="cf-feedback-panel"/u, /data-anchor-block=/u]) {
    assert.doesNotMatch(html, chrome, `lifecycle, export: carries ${chrome}`);
  }
  assert.ok(html.includes("data-cf-block-id=\"prose\""), "lifecycle, export: lost the document");
  // close then clear leave no session state behind.
  await context.close();
  context = null;
  run(["present", "close", sessionId]);
  // close returns only once the service has exited, so clear right after it
  // finds nothing running.
  run(["present", "clear", sessionId, "--older-than", "0d"]);
  assert.deepEqual(JSON.parse(run(["present", "list"])), [], "lifecycle, clear: a session is still listed");
  assert.throws(() => run(["present", "history", sessionId]), "lifecycle, clear: history still readable");
  const cleared = sessionId;
  sessionId = null;
  process.stdout.write(`lifecycle passed: an approval with no notes, an unsent note kept across update and reload, update re-anchors and orphans with reasons, resolve, history of 2 revisions and 3 reviews, export without chrome, close and clear of ${cleared}\n`);
  process.stdout.write(`cf-present annotation matrix passed: ${cells.length} cells and a whole-document note over ${matrix.size} block types (${cells.filter((cell) => cell.gesture === "text").length} text, ${cells.filter((cell) => cell.gesture === "element").length} element, ${cells.filter((cell) => cell.gesture === "area").length} area), each delivered with its selector; ${decoded.length} JPEG crops sized to their anchors and not one colour\n`);
  for (const cell of expected) process.stdout.write(`  ${cell.where}: ${cell.summary}\n`);
} finally {
  if (context) await context.close().catch(() => undefined);
  if (sessionId) {
    try { run(["present", "close", sessionId]); } catch { /* the service may already be gone */ }
  }
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

/** The next review envelope `present feedback` delivers, waiting up to 30 s. */
async function nextEnvelope() {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const line = run(["present", "feedback", sessionId]).trim();
    if (line) return JSON.parse(line);
    await new Promise((done) => setTimeout(done, 250));
  }
  throw new Error("no review envelope arrived");
}

/** The block by gesture table: type to { text, element, area } applicability. */
function readMatrix(markdown) {
  const section = markdown.split("\n### What a reviewer can mark on each block\n")[1]?.split("\n#")[0];
  assert.ok(section, "the authoring reference has no annotation matrix");
  const rows = new Map();
  for (const line of section.split("\n")) {
    const match = line.match(/^\| `([a-z_]+)` \| (.+) \| (.+) \| (.+) \|$/u);
    if (!match) continue;
    const [, type, ...rest] = match;
    const [text, element, area] = rest.map((cell) => {
      assert.match(cell, /^(yes\b|no: \S)/u, `${type}: a cell must start "yes" or "no: <reason>": ${cell}`);
      return cell.startsWith("yes");
    });
    rows.set(type, { text, element, area });
  }
  return rows;
}

async function armComment(page) {
  if (await page.locator(".cf-hint.on").count() === 0) {
    await page.locator("#cf-comment-toggle").click();
    await page.locator(".cf-hint.on").waitFor();
  }
  // At desktop width the rail sits beside the document; where it covers it, close it.
  if ((await page.locator("#cf-feedback-panel").getAttribute("data-open")) === "true" && await page.locator(".cf-feedback-close").isVisible()) {
    await page.locator(".cf-feedback-close").click({ force: true });
    await page.waitForFunction(() => document.getElementById("cf-feedback-panel")?.getAttribute("data-open") === "false");
  }
}

async function reveal(page, blockId) {
  await page.locator(`[data-cf-block-id='${blockId}']`).first().evaluate((element) => element.scrollIntoView({ block: "center", behavior: "instant" }));
}

// One gesture; returns what the pinned target should crop and how the chip named it.
async function gesture(page, cell) {
  const scope = `[data-cf-block-id='${cell.block}']`;
  if (cell.gesture === "text") {
    const span = await page.evaluate(({ scope, within, words }) => {
      // The words may cross text nodes, as highlighted code does.
      for (const element of document.querySelectorAll(`${scope} ${within}`)) {
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        const nodes = [];
        let text = "";
        while (walker.nextNode()) {
          nodes.push({ node: walker.currentNode, start: text.length });
          text += walker.currentNode.data;
        }
        const at = text.indexOf(words);
        if (at < 0) continue;
        const locate = (offset) => {
          const hit = nodes.findLast((entry) => entry.start <= offset);
          return [hit.node, offset - hit.start];
        };
        const glyph = (offset) => {
          const range = document.createRange();
          range.setStart(...locate(offset));
          const [node, inside] = locate(offset);
          range.setEnd(node, inside + 1);
          return range.getBoundingClientRect();
        };
        const first = glyph(at);
        const last = glyph(at + words.length - 1);
        return { x0: first.left + 1, y0: first.top + first.height / 2, x1: last.right - 1, y1: last.top + last.height / 2 };
      }
      return null;
    }, { scope, within: cell.recipe.in, words: cell.recipe.words });
    if (!span) throw new Error(`no "${cell.recipe.words}" in ${cell.recipe.in}`);
    await page.mouse.move(span.x0, span.y0);
    await page.mouse.down();
    await page.mouse.move(span.x1, span.y1, { steps: 8 });
    await page.mouse.up();
    const selected = await page.evaluate(() => String(getSelection()));
    if (selected !== cell.recipe.words) throw new Error(`the drag selected ${JSON.stringify(selected)}`);
    const summary = await chip(page, "Text");
    return { summary };
  }
  if (cell.gesture === "element") {
    const target = page.locator(`${scope} ${cell.recipe.click}`).first();
    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    if (!box) throw new Error(`${cell.recipe.click} has no box`);
    await page.mouse.click(box.x + Math.min(box.width / 2, 40), box.y + box.height / 2);
    const summary = await chip(page, "Element");
    if (!summary.includes(cell.recipe.label.slice(0, 40))) throw new Error(`the chip names ${JSON.stringify(summary)}`);
    const pinned = await page.evaluate(() => {
      const rect = document.querySelector(".cf-hot-sel")?.getBoundingClientRect();
      return rect ? { width: rect.width, height: rect.height } : null;
    });
    if (!pinned) throw new Error("no pinned element is marked");
    return { summary, box: pinned };
  }
  // Area: a drag inside the block, or inside a named part of it. A block
  // that hides others is dragged over while they are hidden (QA defect 2).
  if (cell.recipe.closed) {
    await page.locator(scope).first().evaluate((block, selector) => {
      for (const details of block.querySelectorAll(selector)) details.open = false;
    }, cell.recipe.closed);
  }
  const area = page.locator(`${scope} ${cell.recipe.box ?? ""}`.trim()).first();
  const box = await area.boundingBox();
  if (!box) throw new Error("the area target has no box");
  const inset = { x: Math.min(24, box.width / 6), y: Math.min(12, box.height / 4) };
  const from = { x: box.x + inset.x, y: box.y + inset.y };
  const to = { x: box.x + box.width - inset.x, y: box.y + box.height - inset.y };
  if (to.x - from.x < 24 || to.y - from.y < 20) throw new Error(`the block is too small to drag in: ${JSON.stringify(box)}`);
  const plain = Boolean(cell.recipe.plain);
  if (!plain) await page.keyboard.down("Shift");
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 12 });
  const draft = await page.evaluate(() => {
    const rect = document.querySelector(".cf-region-draft")?.getBoundingClientRect();
    return rect ? { width: rect.width, height: rect.height } : null;
  });
  await page.mouse.up();
  if (!plain) await page.keyboard.up("Shift");
  if (!draft) throw new Error(`the ${plain ? "plain" : "Shift"} drag drew no box`);
  const summary = await chip(page, "Region");
  return { summary, box: { width: to.x - from.x, height: to.y - from.y } };
}

async function chip(page, kind) {
  const float = page.getByTestId("float-chip");
  await float.waitFor({ timeout: 10_000 }).catch(async () => {
    const status = await page.locator(".cf-status").first().innerText().catch(() => "");
    throw new Error(`no chip opened; status ${JSON.stringify(status)}`);
  });
  const shown = (await float.locator(".lab").innerText()).trim();
  if (shown !== kind) throw new Error(`the chip says ${shown}, expected ${kind}`);
  const composerQuote = await float.locator(".q").innerText();
  await page.getByTestId("float-comment").click();
  const composer = page.getByTestId("composer");
  await composer.waitFor();
  return `${kind}: ${composerQuote}`;
}

// At phone width the rail is a bottom sheet (QA defect 7): taking, saving and
// reopening a note leave it closed so the target stays in view; the float keeps
// ESC on screen; the Comment button opens the sheet and Done leaves Comment.
async function phoneWidth(page) {
  const step = async (name, action) => {
    try {
      return await action();
    } catch (error) {
      throw new Error(`phone width, ${name}: ${error.message.split("\n")[0]}`, { cause: error });
    }
  };
  const sheetOpen = async () => (await page.locator("#cf-feedback-panel").getAttribute("data-open")) === "true";
  await page.setViewportSize({ width: 375, height: 812 });
  await page.emulateMedia({ colorScheme: "dark" });
  try {
    await step("arm Comment", async () => {
      await page.locator("#cf-comment-toggle").click();
      await page.locator(".cf-hint.on").waitFor();
      assert.equal(await sheetOpen(), false, "the sheet opened on arming");
    });
    await reveal(page, "prose");
    await step("float", async () => {
      const heading = await page.locator("[data-cf-block-id='prose'] h2").boundingBox();
      await page.mouse.click(heading.x + heading.width - 4, heading.y + heading.height / 2);
      const escape = await page.getByTestId("float-esc").boundingBox();
      assert.ok(escape.x >= 0 && escape.x + escape.width <= 375, `ESC at ${escape.x}..${escape.x + escape.width} of 375`);
      assert.equal(await sheetOpen(), false, "the sheet opened on a gesture");
    });
    await step("save", async () => {
      await page.getByTestId("float-comment").click();
      await saveNote(page, "phone: kept in view");
      assert.equal(await sheetOpen(), false, "the sheet opened on save");
      await page.getByTestId("toast").getByText(/The Comment button opens your notes and Submit\./u).waitFor();
    });
    await step("reopen from the pin", async () => {
      await page.getByTestId("note-marker").first().click();
      await page.getByTestId("composer").waitFor();
      assert.equal(await sheetOpen(), false, "the sheet opened from a pin");
      await page.getByTestId("composer-delete").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
      assert.equal(await page.getByTestId("note-marker").count(), 0, "the phone note was not removed");
    });
    await step("Comment opens the sheet", async () => {
      await page.locator("#cf-comment-toggle").click();
      await page.waitForFunction(() => document.getElementById("cf-feedback-panel")?.getAttribute("data-open") === "true");
      await page.locator(".cf-feedback-close").click();
    });
    await step("Done leaves", async () => {
      await page.getByTestId("comment-leave").click();
      await page.locator(".cf-hint.on").waitFor({ state: "detached" });
    });
  } finally {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: "light" });
  }
}

async function saveNote(page, body) {
  const composer = page.getByTestId("composer");
  await page.getByTestId("composer-text").fill(body);
  await page.getByTestId("composer-save").click();
  await composer.waitFor({ state: "detached" });
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

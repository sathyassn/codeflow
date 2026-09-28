// The annotation matrix end to end (TSK-071): the real `codeflow` binary
// serves a document with one block of every type, and a task-owned headless
// Chrome profile takes a note on every applicable cell of the block by gesture
// matrix in `cf-present/references/document-authoring.md` by a real gesture:
// a mouse selection for text, a click for an element, a drag for an area.
// `codeflow present feedback` must return each note with its kind and
// selector, a text note its quote with prefix and suffix, and every element
// and area note a JPEG crop of what it anchors: the size of the rectangle the
// note reloads (within 2 px at the capture scale), not one colour, at least
// LIKENESS_FLOOR like the page inside that rectangle, more like it than the
// same-size rectangle just outside it, and in register: no less like it than
// the same rectangle moved 4 or 12 px any way (likeness.mjs). A cell that fails
// names its block, its gesture and the step.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { LIKENESS_FLOOR, NEARBY_OFFSETS, REGISTRATION_SLACK, gridShape, inkGrid, likeness, placed } from "./likeness.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
const fixture = join(webRoot, "../tests/fixtures/annotation/every-block.json");
const legacyFixture = join(webRoot, "../tests/fixtures/annotation/legacy-decision.json");
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
  status: { block: "checks", text: { in: "li span", words: "one crop was a sliver" }, element: { click: "li:nth-of-type(2)", label: "Linux Chrome run · no Linux host" } },
  code: { block: "snippet", text: { in: "code", words: "256 * 1024" }, element: { click: ".cf-line:nth-of-type(2)", label: "let limit = 256 * 1024;" } },
  diff: { block: "change", text: { in: "del", words: "128 * 1024" }, element: { click: "ins", label: "let limit = 256 * 1024;" } },
  tree: { block: "files", text: { in: "li", words: "state.rs" }, element: { click: "li li li:nth-of-type(2) > span", label: "limits.rs" } },
  figure: { block: "flow", element: { entities: true } },
  media: { block: "screenshot", text: { in: "figcaption", words: "stands in" }, element: { click: "img", label: "Four coloured quadrants" }, area: { box: "img", plain: true } },
  disclosure: { block: "more", text: { in: "summary", words: "More detail", context: false }, element: { click: "summary", label: "More detail" }, area: { closed: "details" } },
  tabs: { block: "views", text: { in: ".tabs__labels", words: "Risks" }, element: { click: "details:nth-of-type(2) > summary", label: "Risks" }, area: { closed: "details" } },
  feedback_prompt: { block: "ask", text: { in: "p", words: "Mark anything" }, element: { click: "p", label: "Mark anything that is wrong, then submit the review." } },
  html: { block: "stage", text: { in: "svg text", words: "Service" }, element: { entities: true } },
  // TSK-119: a form's option label is its words; a click on a field's input
  // pins the field. The v1 decision keeps its row, on its own v1 document.
  form: { block: "survey", text: { in: ".cf-option__label", words: "Private local store" }, element: { click: "[data-cf-field='keep-days'] input", label: "Days to keep" }, area: { box: "[data-cf-field='store']" } },
  decision_v1: { block: "legacy", document: "legacy", text: { in: "p", words: "element and area notes" }, element: { click: "h2", label: "Which crop format shipped?" } },
};

const matrix = readMatrix(await readFile(reference, "utf8"));
assert.deepEqual(Object.keys(RECIPES).sort(), [...matrix.keys()].sort(), "the suite and the matrix name different block types");
const cells = [];
const legacyCells = [];
for (const [type, row] of matrix) {
  for (const gesture of ["text", "element", "area"]) {
    const recipe = RECIPES[type];
    const has = gesture === "area" || Boolean(recipe[gesture]);
    assert.equal(has, row[gesture], `${type} ${gesture}: the matrix says ${row[gesture] ? "yes" : "no"}, the suite ${has ? "has" : "lacks"} a recipe`);
    if (!row[gesture]) continue;
    if (recipe[gesture]?.entities) continue;
    (recipe.document === "legacy" ? legacyCells : cells).push({ type, gesture, block: recipe.block, recipe: recipe[gesture] ?? {} });
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
  const answerRequests = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === "/app/api/answers") answerRequests.push(request.url());
  });
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
  for (const width of [1280, 375]) await closeMarkers(page, width);
  process.stdout.write("close markers passed: at 1280 and 375 px, thirteen notes (two adjacent diff lines, two adjacent code lines, nine on one prose line, three of them on the same words) have markers that do not overlap and sit off their anchors, each in the gutter level with its line on a desktop, 2 px clear of the sections list, or just above it on a phone, leaving that row only when no free stretch of it holds the marker, also after widening from 375 px with the rail closed, and a click at each marker's centre opens its own note\n");
  process.stdout.write("sections and phone width passed: no hidden block in the sections list; at 375 px the sheet stays closed on arming, a gesture, a save and a pin, the float keeps ESC on screen, markers stay off the heading and the selected line, Comment opens the sheet and dismisses the save hint, and Done leaves\n");

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
    await step("reveal the block", () => reveal(page, cell.block, cell.recipe.box ?? cell.recipe.in));
    const pinned = await step("gesture", () => gesture(page, cell));
    await step("save the note", () => saveNote(page, `matrix: ${where}`));
    const reloaded = cell.gesture === "text" ? {} : await step("read the anchored page", () => anchoredPage(page, sessionId));
    expected.push({ ...cell, where, ...pinned, ...reloaded });
  }
  // Marking a form never answers it (SPC-014 B6): after the form cells
  // above, a click on an option, on a text field and on Submit answer, a
  // drag across option labels and a box over the form each pin a note and
  // leave every control, the draft and the store as they were; nothing is
  // sent.
  {
    const controls = () => page.evaluate(() => [...document.querySelectorAll("[data-cf-form] [data-cf-value], [data-cf-form] [data-cf-rationale-input]")]
      .map((control) => (control.type === "radio" || control.type === "checkbox" ? `${control.id}:${control.checked}` : `${control.id}=${control.value}`)));
    const pristine = await controls();
    assert.ok(pristine.length > 10 && pristine.every((state) => state.endsWith(":false") || state.endsWith("=")), `form control: the form cells changed a control: ${pristine.join(" ")}`);
    const discard = async (what) => {
      await page.getByTestId("float-chip").waitFor({ timeout: 10_000 }).catch((error) => {
        throw new Error(`form control, ${what}: no note was pinned`, { cause: error });
      });
      await page.keyboard.press("Escape");
      await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 10_000 });
      assert.deepEqual(await controls(), pristine, `form control, ${what}: a control changed`);
      assert.deepEqual(answerRequests, [], `form control, ${what}: an answer was sent`);
    };
    for (const [what, target] of [
      ["a click on an option", "[data-cf-block-id='survey'] input[value='repo']"],
      ["a click on an option's label", "[data-cf-block-id='survey'] [data-cf-field='notify'] .cf-option__label"],
      ["a click on a text field", "[data-cf-block-id='survey'] [data-cf-field='contact'] input"],
      ["a click on a decision's option", "[data-cf-block-id='choice'] input[value='png']"],
      ["a click on Submit answer", "[data-cf-block-id='survey'] [data-cf-form-action='submit']"],
    ]) {
      await armComment(page);
      await page.locator(target).first().evaluate((element) => element.scrollIntoView({ block: "center", behavior: "instant" }));
      const box = await page.locator(target).first().boundingBox();
      assert.ok(box, `form control, ${what}: no box`);
      await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
      await discard(what);
    }
    await armComment(page);
    await reveal(page, "survey", "[data-cf-field='store']");
    await gesture(page, { type: "form", gesture: "text", block: "survey", recipe: { in: ".cf-option__label", words: "Committed JSON Lines" } });
    await page.keyboard.press("Escape");
    await page.getByTestId("composer").waitFor({ state: "detached", timeout: 10_000 }).catch(() => undefined);
    assert.deepEqual(await controls(), pristine, "form control, a text selection: a control changed");
    await armComment(page);
    await reveal(page, "survey", "[data-cf-field='channels']");
    await gesture(page, { type: "form", gesture: "area", block: "survey", recipe: { box: "[data-cf-field='channels']" } });
    await page.keyboard.press("Escape");
    await page.getByTestId("composer").waitFor({ state: "detached", timeout: 10_000 }).catch(() => undefined);
    assert.deepEqual(await controls(), pristine, "form control, an area: a control changed");
    assert.deepEqual(answerRequests, [], "form control: an answer was sent");
    assert.equal(await findFile(root, "responses.jsonl"), null, "form control: the answer store was written");
    process.stdout.write("form control passed: clicks on options, a label, a text field, a decision option and Submit answer, a text selection across option labels and an area over the form pin notes and leave every control, the draft and the store unchanged; no answer is sent\n");
  }
  // A plain drag (no Shift) over the blank ends of diff lines selects those
  // lines as text; it never pins the whole diff as one element (QA defect 6).
  // Its quote is what the reader sees: no screen-reader label, no marker.
  {
    await armComment(page);
    await reveal(page, "change");
    const drag = await page.evaluate(() => {
      const lines = [...document.querySelectorAll("[data-cf-block-id='change'] pre .cf-line")];
      const text = (line) => {
        const range = document.createRange();
        range.selectNodeContents(line);
        return range.getBoundingClientRect();
      };
      const [first, last] = [lines[4], lines[6]].map((line) => ({ line: line.getBoundingClientRect(), text: text(line) }));
      return {
        from: { x: first.text.right + 24, y: first.line.top + first.line.height / 2 },
        to: { x: Math.min(last.line.right - 8, last.text.right + 160), y: last.line.top + last.line.height / 2 },
        blank: first.line.right - first.text.right,
      };
    });
    assert.ok(drag.blank > 60, `diff whitespace: the line has no blank end to drag from: ${JSON.stringify(drag)}`);
    await page.mouse.move(drag.from.x, drag.from.y);
    await page.mouse.down();
    await page.mouse.move(drag.to.x, drag.to.y, { steps: 12 });
    await page.mouse.up();
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText({ timeout: 10_000 })).trim();
    const quote = await page.evaluate(() => String(getSelection()));
    assert.equal(kind, "Text", `diff whitespace: a plain drag pinned ${kind}`);
    assert.ok(quote.includes("256 * 1024") && quote.includes("bytes <= limit"), `diff whitespace: the drag selected ${JSON.stringify(quote)}`);
    const summary = await chip(page, "Text");
    assert.doesNotMatch(summary, /Added|Removed|(^|\n|: )[+-] /u, `diff whitespace: the chip quotes ${JSON.stringify(summary)}`);
    await saveNote(page, "matrix: diff whitespace");
    expected.push({ type: "diff", gesture: "text", block: "change", where: "diff whitespace", recipe: { seen: ["256 * 1024", "bytes <= limit"] }, summary });
    process.stdout.write(`diff whitespace passed: a plain drag over the blank ends of three diff lines selects them as text, not the whole diff, quoted as ${JSON.stringify(summary)}\n`);
  }
  // Markers park beside what they mark, and a saved area keeps a faint
  // outline while Comment is on (P2-2): no area marker sits on its area.
  {
    const layout = await page.evaluate(() => {
      const box = (element) => {
        const rect = element.getBoundingClientRect();
        return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
      };
      return {
        areas: [...document.querySelectorAll("[data-testid=note-marker]")].filter((marker) => /, Area: /u.test(marker.getAttribute("aria-label") ?? "")).map(box),
        outlines: [...document.querySelectorAll("[data-testid=saved-area]")].map(box),
      };
    });
    const areaCells = expected.filter((cell) => cell.gesture === "area").length;
    assert.equal(layout.outlines.length, areaCells, `saved areas: ${layout.outlines.length} outlines for ${areaCells} area notes`);
    assert.equal(layout.areas.length, areaCells, "saved areas: a marker is missing");
    for (const [index, marker] of layout.areas.entries()) {
      const area = layout.outlines[index];
      const overlaps = marker.left < area.right && area.left < marker.right && marker.top < area.bottom && area.top < marker.bottom;
      assert.ok(!overlaps, `saved areas: marker ${index + 1} ${JSON.stringify(marker)} sits on its area ${JSON.stringify(area)}`);
    }
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
  expected.push({ where: "whole document", gesture: "document", box: onScreen, summary: `Area: the whole document, ${Math.round(onScreen.width)}×${Math.round(onScreen.height)} px on screen` });

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
    assert.equal(note.kind, "comment", `${cell.where}, delivery: kind`);
    // One selector, of the gesture's own kind.
    const selectors = ["selector", "element_selector", "region_selector"].filter((key) => note[key]);
    const own = { text: "selector", element: "element_selector", area: "region_selector", document: "region_selector" }[cell.gesture];
    assert.deepEqual(selectors, [own], `${cell.where}, delivery: selectors`);
    if (cell.gesture !== "document") assert.equal(note.block_id, cell.expectBlock ?? cell.block, `${cell.where}, delivery: block`);
    if (cell.gesture === "text" && cell.recipe.seen) {
      // A diff quote holds the lines' words, never a label or a marker.
      const exact = note.selector?.exact ?? "";
      for (const words of cell.recipe.seen) assert.ok(exact.includes(words), `${cell.where}, delivery: quote ${JSON.stringify(exact)}`);
      assert.doesNotMatch(exact, /Added|Removed|(^|\n)[+-](?![+-])/u, `${cell.where}, delivery: quote ${JSON.stringify(exact)}`);
      continue;
    }
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
      // A diff line is named by its words, without its label or marker.
      assert.doesNotMatch(note.element_selector.label, /Added|Removed|(^|: )[+-] /u, `${cell.where}, delivery: label ${JSON.stringify(note.element_selector.label)}`);
    } else {
      assert.ok(note.region_selector, `${cell.where}, delivery: no region selector`);
      assert.equal(note.region_selector.scope, cell.gesture === "document" ? "document" : "block", `${cell.where}, delivery: region scope`);
    }
    assert.equal(note.excerpt?.image?.media_type, "image/jpeg", `${cell.where}, delivery: crop format`);
    images.push({ where: cell.where, data: note.excerpt.image.data_base64, box: cell.box, inside: cell.inside, outside: cell.outside, nearby: cell.nearby });
  }
  // Decode each crop in the browser: its size against the rectangle the note
  // reloads at the capture scale, more than one colour, and its picture
  // against the page inside that rectangle and just outside it.
  // The ink grid and likeness come from likeness.mjs, the same functions
  // likeness.test.mjs holds to a stripe field and shifted ink.
  const decodeCrops = async (items, inkGrid, likeness, gridShape) => {
    const load = async (type, data) => {
      const image = new Image();
      image.src = `data:${type};base64,${data}`;
      await image.decode();
      return image;
    };
    const pixelsOf = (image) => {
      const canvas = document.createElement("canvas");
      canvas.width = image.width;
      canvas.height = image.height;
      const context = canvas.getContext("2d");
      context.drawImage(image, 0, 0);
      return context.getImageData(0, 0, image.width, image.height).data;
    };
    const grid = (image, columns, rows) => inkGrid(pixelsOf(image), image.width, image.height, columns, rows);
    return Promise.all(items.map(async ({ where, data, inside, outside, nearby }) => {
      const image = await load("image/jpeg", data);
      const pixels = pixelsOf(image);
      const colours = new Set();
      for (let index = 0; index < pixels.length && colours.size < 3; index += 16) {
        colours.add(`${pixels[index] >> 4},${pixels[index + 1] >> 4},${pixels[index + 2] >> 4}`);
      }
      const result = { where, width: image.width, height: image.height, colours: colours.size };
      if (!inside) return result;
      const { columns, rows } = gridShape(image.width, image.height);
      const crop = inkGrid(pixels, image.width, image.height, columns, rows);
      result.inside = likeness(crop, grid(await load("image/png", inside), columns, rows));
      result.outside = likeness(crop, grid(await load("image/png", outside), columns, rows));
      result.nearby = [];
      for (const shot of nearby) result.nearby.push(likeness(crop, grid(await load("image/png", shot), columns, rows)));
      return result;
    }));
  };
  const payload = images.map(({ where, data, inside, outside, nearby }) => ({ where, data, inside, outside, nearby: nearby ?? [] }));
  const decoded = await page.evaluate(`(${decodeCrops})(${JSON.stringify(payload)}, ${inkGrid}, ${likeness}, ${gridShape})`);
  // One saved crop per cell for the verification record, on request.
  if (process.env.CF_PRESENT_EVIDENCE_DIR) {
    await mkdir(process.env.CF_PRESENT_EVIDENCE_DIR, { recursive: true });
    for (const { where, data, inside, outside, nearby } of images) {
      const name = where.replace(/\s+/gu, "-");
      await writeFile(join(process.env.CF_PRESENT_EVIDENCE_DIR, `${name}.jpg`), Buffer.from(data, "base64"));
      // The page inside the rectangle the note reloads, and just outside it.
      if (inside) await writeFile(join(process.env.CF_PRESENT_EVIDENCE_DIR, `${name}.page.png`), Buffer.from(inside, "base64"));
      if (outside) await writeFile(join(process.env.CF_PRESENT_EVIDENCE_DIR, `${name}.outside.png`), Buffer.from(outside, "base64"));
      for (const [index, shot] of (nearby ?? []).entries()) await writeFile(join(process.env.CF_PRESENT_EVIDENCE_DIR, `${name}.nearby-${index}.png`), Buffer.from(shot, "base64"));
    }
  }
  for (const [index, crop] of decoded.entries()) {
    const { box } = images[index];
    const scale = Math.min(MAX_CROP.width / box.width, MAX_CROP.height / box.height, 1);
    assert.ok(Math.abs(crop.width - box.width * scale) <= 2 && Math.abs(crop.height - box.height * scale) <= 2,
      `${crop.where}, crop: ${crop.width} x ${crop.height} px for bounds ${box.width.toFixed(1)} x ${box.height.toFixed(1)} at scale ${scale.toFixed(3)}`);
    assert.ok(crop.colours > 1, `${crop.where}, crop: one colour`);
  }
  // Every crop's picture against the page, all listed before any failure: it
  // must clear the likeness floor, beat the page just outside, and be at
  // least as like its own rectangle as the same rectangle moved 4 or 12 px.
  const fitOf = (crop) => `${crop.inside.toFixed(2)} inside, ${crop.outside.toFixed(2)} outside, ${crop.nearby.length ? Math.max(...crop.nearby).toFixed(2) : "none"} best nearby`;
  const misplaced = decoded.filter((crop) => crop.inside !== undefined && !placed(crop.inside, crop.outside, crop.nearby));
  if (misplaced.length) {
    for (const crop of decoded.filter((item) => item.inside !== undefined)) process.stderr.write(`  ${crop.where}: likeness ${fitOf(crop)}\n`);
    assert.fail(misplaced.map((crop) => `${crop.where}, crop: likeness ${fitOf(crop)}; it needs at least ${LIKENESS_FLOOR} inside, more than outside, and no less than ${REGISTRATION_SLACK} below the best nearby`).join("; "));
  }
  // The v1 decision row, on a schema_version 1 document of its own: each
  // cell pins a note of its gesture's kind on the decision, delivered with
  // its selector, and each element and area note with a JPEG crop.
  {
    const document = join(project, "legacy-decision.json");
    await writeFile(document, await readFile(legacyFixture, "utf8"));
    const legacyOpened = run(["present", "open", document, "--no-launch"]);
    const legacyId = legacyOpened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const legacyBootstrap = legacyOpened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    assert.ok(legacyId && legacyBootstrap, `legacy decision: could not parse ${legacyOpened}`);
    const legacyPage = await context.newPage();
    await legacyPage.goto(pathToFileURL(legacyBootstrap).href, { waitUntil: "commit", timeout: 120_000 });
    await legacyPage.waitForURL(/^http:\/\/127\.0\.0\.1:\d+\/app\//u, { waitUntil: "domcontentloaded", timeout: 60_000 });
    await legacyPage.locator("#cf-comment-toggle").waitFor({ state: "visible" });
    assert.equal(await legacyPage.locator("[data-cf-block-id='legacy'] .decision__status").textContent(), "Accepted", "legacy decision: its status");
    for (const cell of legacyCells) {
      await armComment(legacyPage);
      await reveal(legacyPage, cell.block);
      await gesture(legacyPage, cell);
      await saveNote(legacyPage, `legacy: ${cell.gesture}`);
    }
    await legacyPage.getByLabel("Verdict").selectOption("approve_with_notes");
    await legacyPage.getByRole("button", { name: "Submit review" }).click();
    await legacyPage.getByRole("status").getByText(/Review received/u).waitFor({ timeout: 60_000 });
    let line = "";
    for (let attempt = 0; attempt < 120 && !line; attempt += 1) {
      line = run(["present", "feedback", legacyId]).trim();
      if (!line) await new Promise((done) => setTimeout(done, 250));
    }
    const legacyEnvelope = JSON.parse(line);
    assert.equal(legacyEnvelope.notes.length, legacyCells.length, "legacy decision: notes");
    for (const cell of legacyCells) {
      const note = legacyEnvelope.notes.find((candidate) => candidate.body === `legacy: ${cell.gesture}`);
      assert.ok(note, `legacy decision ${cell.gesture}: no note`);
      assert.equal(note.block_id, "legacy", `legacy decision ${cell.gesture}: block`);
      const own = { text: "selector", element: "element_selector", area: "region_selector" }[cell.gesture];
      assert.deepEqual(["selector", "element_selector", "region_selector"].filter((key) => note[key]), [own], `legacy decision ${cell.gesture}: selectors`);
      if (cell.gesture === "text") assert.equal(note.selector.exact, cell.recipe.words);
      else assert.equal(note.excerpt?.image?.media_type, "image/jpeg", `legacy decision ${cell.gesture}: crop`);
      if (cell.gesture === "element") assert.ok(note.element_selector.label.includes(cell.recipe.label), `legacy decision: label ${note.element_selector.label}`);
    }
    await legacyPage.close();
    run(["present", "close", legacyId]);
    run(["present", "clear", legacyId, "--older-than", "0d"]);
    process.stdout.write(`legacy decision passed: on a schema_version 1 document, the v1 decision renders its status and takes ${legacyCells.map((cell) => cell.gesture).join(", ")} notes, each delivered with its own selector (element and area with a JPEG crop)\n`);
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
  process.stdout.write(`cf-present annotation matrix passed: ${cells.length} cells and a whole-document note over ${matrix.size} block types (${cells.filter((cell) => cell.gesture === "text").length} text, ${cells.filter((cell) => cell.gesture === "element").length} element, ${cells.filter((cell) => cell.gesture === "area").length} area), each delivered with its kind and selector; ${decoded.length} JPEG crops sized to the rectangle each note reloads, not one colour, and ${decoded.filter((crop) => crop.inside !== undefined).length} of them at least ${LIKENESS_FLOOR} like the page inside that rectangle, more like it than just outside, and in register with the same rectangle moved 4 or 12 px\n`);
  for (const cell of expected) {
    const crop = decoded.find((item) => item.where === cell.where);
    const fit = crop?.inside !== undefined ? ` (likeness ${fitOf(crop)})` : "";
    process.stdout.write(`  ${cell.where}: ${cell.summary}${fit}\n`);
  }
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
    const match = line.match(/^\| `([a-z_]+)`( \(v1\))? \| (.+) \| (.+) \| (.+) \|$/u);
    if (!match) continue;
    // The schema_version 1 decision keeps its own row, `decision_v1` here.
    const [, name, legacy, ...rest] = match;
    const type = legacy ? `${name}_v1` : name;
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

// A block taller than the window (the form) is revealed at the part the
// gesture uses.
async function reveal(page, blockId, inner) {
  await page.locator(`[data-cf-block-id='${blockId}']`).first().evaluate((element, inner) => {
    const part = inner && element.getBoundingClientRect().height > innerHeight * 0.8 ? element.querySelector(inner) : null;
    (part ?? element).scrollIntoView({ block: "center", behavior: "instant" });
  }, inner ?? null);
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
    if (!summary.includes(cell.recipe.label.slice(0, 40)) || /Added|Removed|: [+-] /u.test(summary)) throw new Error(`the chip names ${JSON.stringify(summary)}`);
    return { summary };
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
  const summary = await chip(page, "Area");
  return { summary };
}

/**
 * The rectangle the note just saved reloads, resolved from its saved selector
 * as the runtime resolves it (resolveElement, resolveRegion in selection.ts),
 * and screenshots of the page inside it and of the same-size rectangle just
 * outside it, with the markers and outlines hidden.
 */
async function anchoredPage(page, session) {
  await page.mouse.move(2, 2);
  const target = await page.evaluate(({ session, NEARBY_OFFSETS }) => {
    const notes = JSON.parse(sessionStorage.getItem(`cf-present-draft:${session}`) ?? "null")?.notes ?? [];
    const note = notes.at(-1);
    const root = document.getElementById("cf-present-document");
    const block = root.querySelector(`[data-cf-block-id="${CSS.escape(note.block_id)}"]`);
    let element = null;
    if (note.region_selector) {
      element = note.region_selector.scope === "document" ? root : root.querySelector(`[data-cf-block-id="${CSS.escape(note.region_selector.anchor_id)}"]`);
    } else if (block?.dataset.cfBlockDigest === note.element_selector.block_digest) {
      const found = note.element_selector.element_path === ":scope" ? block : block.querySelector(`:scope > ${note.element_selector.element_path}`);
      element = found?.localName === note.element_selector.tag_name ? found : null;
    }
    if (!element) return null;
    element.scrollIntoView({ block: "center", behavior: "instant" });
    const selector = note.region_selector;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      return selector ? {
        x: rect.left + rect.width * selector.x_ppm / 1e6,
        y: rect.top + rect.height * selector.y_ppm / 1e6,
        width: rect.width * selector.width_ppm / 1e6,
        height: rect.height * selector.height_ppm / 1e6,
      } : { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    };
    let box = measure();
    // Room for the rectangle just below it, under the fixed chrome.
    const chrome = Math.max(...[...document.querySelectorAll(".cf-topbar, .cf-hint.on")].map((bar) => bar.getBoundingClientRect().bottom), 0);
    if (box.y + 2 * box.height + 2 > innerHeight) {
      scrollBy({ top: Math.floor(box.y - chrome - 4), behavior: "instant" });
      box = measure();
    }
    // The same-size rectangle just below (or above) it; a box too tall for
    // that moves as far as the screen allows, at least a quarter of its height.
    const room = Math.max(innerHeight - (box.y + box.height), box.y - chrome);
    const shift = Math.min(box.height + 1, room) * (innerHeight - (box.y + box.height) >= box.y - chrome ? 1 : -1);
    const outside = { ...box, y: box.y + shift };
    const fits = Math.abs(shift) >= box.height / 4 && Math.min(box.y, outside.y) >= chrome - 0.5 && Math.max(box.y, outside.y) + box.height <= innerHeight + 0.5;
    // The same rectangle moved 4 or 12 px each way, where it stays on screen
    // below the chrome: a crop in place is at least as like its own rectangle.
    const nearby = NEARBY_OFFSETS
      .map(([dx, dy]) => ({ ...box, x: box.x + dx, y: box.y + dy }))
      .filter((rect) => rect.x >= 0 && rect.x + rect.width <= innerWidth && rect.y >= chrome - 0.5 && rect.y + rect.height <= innerHeight + 0.5);
    return { box, outside, nearby, fits };
  }, { session, NEARBY_OFFSETS });
  if (!target) throw new Error("the saved selector resolves to nothing");
  const style = await page.addStyleTag({ content: ".cf-marker-layer, .cf-toast { visibility: hidden !important; } .cf-hot, .cf-hot-sel { outline: none !important; background: none !important; }" });
  try {
    if (!target.fits) throw new Error(`the rectangle and its neighbour do not fit on screen: ${JSON.stringify(target)}`);
    const viewport = page.viewportSize();
    const shot = async ({ x, y, width, height }) => {
      const clip = { x: Math.max(0, x), y: Math.max(0, y) };
      Object.assign(clip, { width: Math.min(width, viewport.width - clip.x), height: Math.min(height, viewport.height - clip.y) });
      return (await page.screenshot({ clip, animations: "disabled", caret: "hide" })).toString("base64");
    };
    const nearby = [];
    for (const rect of target.nearby) nearby.push(await shot(rect));
    return { box: { width: target.box.width, height: target.box.height }, inside: await shot(target.box), outside: await shot(target.outside), nearby };
  } finally {
    await style.evaluate((element) => element.remove());
  }
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
// ESC on screen; the Comment button opens the sheet, and the save hint goes
// with it; Done leaves Comment. With no gutter beside a full-width block, a
// marker sits above the line it marks, never on its words (P2-3).
async function phoneWidth(page) {
  const step = async (name, action) => {
    try {
      return await action();
    } catch (error) {
      throw new Error(`phone width, ${name}: ${error.message.split("\n")[0]}`, { cause: error });
    }
  };
  const sheetOpen = async () => (await page.locator("#cf-feedback-panel").getAttribute("data-open")) === "true";
  const markerClear = async (rect, what) => {
    const marker = await page.getByTestId("note-marker").first().boundingBox();
    const overlaps = marker.x < rect.x + rect.width && rect.x < marker.x + marker.width && marker.y < rect.y + rect.height && rect.y < marker.y + marker.height;
    assert.ok(!overlaps, `the marker ${JSON.stringify(marker)} sits on ${what} ${JSON.stringify(rect)}`);
    assert.ok(marker.x >= 0 && marker.x + marker.width <= 375, `the marker is off screen: ${JSON.stringify(marker)}`);
  };
  await page.setViewportSize({ width: 375, height: 812 });
  await page.emulateMedia({ colorScheme: "dark" });
  try {
    await step("arm Comment", async () => {
      await page.locator("#cf-comment-toggle").click();
      await page.locator(".cf-hint.on").waitFor();
      assert.equal(await sheetOpen(), false, "the sheet opened on arming");
    });
    await reveal(page, "prose");
    const heading = await page.locator("[data-cf-block-id='prose'] h2").boundingBox();
    await step("float", async () => {
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
      await markerClear(await page.locator("[data-cf-block-id='prose'] h2").boundingBox(), "the heading");
    });
    await step("Comment opens the sheet", async () => {
      await page.locator("#cf-comment-toggle").click();
      await page.waitForFunction(() => document.getElementById("cf-feedback-panel")?.getAttribute("data-open") === "true");
      // Well inside the hint's own 3.4 s: opening the sheet dismissed it.
      await page.getByTestId("toast").waitFor({ state: "detached", timeout: 1000 });
      await page.locator(".cf-feedback-close").click();
    });
    await step("reopen from the pin", async () => {
      await page.getByTestId("note-marker").first().click();
      await page.getByTestId("composer").waitFor();
      assert.equal(await sheetOpen(), false, "the sheet opened from a pin");
      await page.getByTestId("composer-delete").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
      assert.equal(await page.getByTestId("note-marker").count(), 0, "the phone note was not removed");
    });
    await step("text marker", async () => {
      await reveal(page, "prose");
      await gesture(page, { type: "narrative", gesture: "text", block: "prose", recipe: RECIPES.narrative.text });
      await saveNote(page, "phone: a marker beside the words");
      const line = await page.evaluate((words) => {
        const paragraph = document.querySelector("[data-cf-block-id='prose'] p");
        const node = [...paragraph.childNodes].find((child) => child.nodeType === Node.TEXT_NODE && child.data.includes(words));
        const range = document.createRange();
        const at = node.data.indexOf(words);
        range.setStart(node, at);
        range.setEnd(node, at + words.length);
        const rect = range.getClientRects()[0];
        return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
      }, RECIPES.narrative.text.words);
      await markerClear(line, "the selected line");
      await page.getByTestId("note-marker").first().click();
      await page.getByTestId("composer-delete").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
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

// Notes whose anchors lie closer than a marker's height keep their markers
// apart (TSK-158, AC-1 as amended): no two marker boxes meet, none sits on
// its anchor or within 2 px of the sections list, and each is where the
// placement rule for the width puts it, level with its line in the gutter on
// a desktop or just above it on a phone, leaving that row only when no free
// stretch of the row holds it; a click at a marker's centre opens that
// marker's own note. The phone run then widens the window with the rail
// still closed and checks the desktop rule again.
async function closeMarkers(page, width) {
  const notes = [];
  const save = async (body) => {
    await page.getByTestId("composer-text").fill(body);
    await page.getByTestId("composer-save").click();
    await page.getByTestId("composer").waitFor({ state: "detached" });
  };
  // Each anchor is measured when the layout is checked, so a check after a
  // resize compares the markers with the lines where they are now.
  const measure = (note) => page.evaluate(({ block, click, words }) => {
    let rect;
    if (words) {
      const node = [...document.querySelector(`[data-cf-block-id='${block}'] p`).childNodes].find((child) => child.nodeType === Node.TEXT_NODE && child.data.includes(words));
      const range = document.createRange();
      const at = node.data.indexOf(words);
      range.setStart(node, at);
      range.setEnd(node, at + words.length);
      rect = range.getClientRects()[0];
    } else {
      rect = document.querySelector(`[data-cf-block-id='${block}'] ${click}`).getBoundingClientRect();
    }
    return { left: rect.left + scrollX, top: rect.top + scrollY, right: rect.right + scrollX, bottom: rect.bottom + scrollY };
  }, note);
  const checkLayout = async (layoutWidth, label) => {
    const where = (what) => `close markers ${label}, ${what}`;
    const anchors = [];
    for (const note of notes) anchors.push(await measure(note));
    for (const [i, anchor] of anchors.entries()) if (i > 4) assert.equal(anchor.top, anchors[4].top, where(`${notes[i].what} is not on the first prose line`));
    const markers = await page.getByTestId("note-marker").evaluateAll((all) => all.map((marker) => {
      const rect = marker.getBoundingClientRect();
      return { left: rect.left + scrollX, top: rect.top + scrollY, right: rect.right + scrollX, bottom: rect.bottom + scrollY };
    }));
    assert.equal(markers.length, notes.length, where("a marker is missing"));
    assert.ok(markers[9].right - markers[9].left > markers[0].right - markers[0].left, where("the tenth marker is not the wider two-digit one"));
    const meets = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
    const gutter = layoutWidth >= 800;
    // The sections list is fixed at the left on a desktop, so no marker may
    // sit in its column: an entry under a marker could not be uncovered.
    const route = await page.locator(".cf-section-route").evaluate((el) => {
      const rect = el.getBoundingClientRect();
      return rect.width > 0 ? { right: rect.right + scrollX } : null;
    });
    assert.equal(route !== null, gutter, where(`the sections list is ${route ? "shown" : "hidden"}`));
    const overflowed = [];
    for (const [i, marker] of markers.entries()) {
      const anchor = anchors[i];
      const { what } = notes[i];
      const name = `marker ${i + 1} (${what}) ${JSON.stringify(marker)}`;
      for (const [j, other] of markers.entries()) {
        if (j > i) assert.ok(!meets(marker, other), where(`markers ${i + 1} (${what}) and ${j + 1} (${notes[j].what}) overlap: ${JSON.stringify([marker, other])}`));
      }
      assert.ok(!meets(marker, anchor), where(`${name} sits on its anchor`));
      if (gutter) {
        assert.ok(marker.left >= route.right + 2, where(`${name} is not 2 px clear of the sections list, which ends at ${route.right}`));
        assert.ok(marker.right <= anchor.left, where(`${name} is not in the gutter left of its line ${JSON.stringify(anchor)}`));
        const level = marker.top < anchor.bottom && anchor.top < marker.bottom;
        if (!level) {
          assert.ok(marker.top >= anchor.bottom, where(`${name} is neither level with nor below its line ${JSON.stringify(anchor)}`));
          overflowed.push(i);
        }
      } else if (marker.top >= anchor.bottom) {
        // A full row above the line overflows below it.
        overflowed.push(i);
      } else {
        assert.ok(marker.bottom <= anchor.top && anchor.top - marker.bottom <= 8, where(`${name} is neither just above nor below its line ${JSON.stringify(anchor)}`));
      }
    }
    // Only the nine-note prose row may overflow, and only once it is full:
    // no free stretch of the row, from its left bound (past the sections list
    // on a desktop, else the page edge) to the overflowed marker's own place,
    // holds that marker.
    const row = markers.filter((marker, i) => i >= 4 && !overflowed.includes(i));
    const bound = route ? route.right + 2 : (await page.evaluate(() => scrollX)) + 2;
    // A text note's marker starts from its block, so each prose marker's own
    // place follows from the page, not from where the markers ended up:
    // derived from the prose block, the document and the sections list.
    const geometry = await page.evaluate(() => {
      const block = document.querySelector("[data-cf-block-id='prose']").getBoundingClientRect();
      const root = document.getElementById("cf-present-document").getBoundingClientRect();
      return { blockLeft: block.left + scrollX, blockRight: block.right + scrollX, rootRight: root.right + scrollX, viewRight: innerWidth + scrollX };
    });
    for (const i of overflowed) {
      const { what } = notes[i];
      assert.ok(i >= 4, where(`marker ${i + 1} (${what}) left its line's row`));
      const size = markers[i].right - markers[i].left;
      const own = baseRight(geometry, size, route && route.right);
      assert.ok(Math.abs(markers[i].right - own) <= 1, where(`marker ${i + 1} (${what}) ${JSON.stringify(markers[i])} overflowed away from its own place, which ends at ${own}`));
      const free = freeStretch(row, bound, own, size);
      assert.ok(free === null, where(`marker ${i + 1} (${what}) left its row while ${JSON.stringify(free)} was free`));
      // An overflow chain starts at its line: each overflowed marker is just
      // below the line or directly under another marker.
      const under = markers.some((other, j) => j !== i && other.left < markers[i].right && markers[i].left < other.right && markers[i].top - other.bottom >= 0 && markers[i].top - other.bottom <= 3);
      assert.ok(under || markers[i].top - anchors[i].bottom <= 12, where(`marker ${i + 1} (${what}) ${JSON.stringify(markers[i])} is far below its line with a free place above it`));
    }
    if (!gutter) assert.ok(overflowed.length > 0, where("nine markers on one line fit one phone row, so no overflow was exercised"));
    const side = (i) => (markers[i].top >= anchors[i].bottom ? "below" : "above");
    console.log(`close markers ${label}: ${markers.length} disjoint, ${row.length} in the prose row, overflow ${overflowed.map((i) => `${i + 1} ${side(i)}`).join(", ") || "none"}`);
  };
  await page.setViewportSize({ width, height: width < 800 ? 812 : 900 });
  try {
    await armComment(page);
    for (const [block, click] of [["change", "del"], ["change", "ins"], ["snippet", ".cf-line:nth-of-type(2)"], ["snippet", ".cf-line:nth-of-type(3)"]]) {
      await reveal(page, block);
      const cell = { gesture: "element", block, recipe: { click, label: "" } };
      await gesture(page, cell).catch((error) => { throw new Error(`close markers at ${width} px, ${block} ${click}: ${error.message}`); });
      const body = `close ${notes.length + 1}`;
      await save(body);
      notes.push({ body, block, click, what: `${block} ${click}` });
    }
    // Nine on one prose line, three of them on the same words: the line's
    // markers take a row, and the notes from the tenth on have two-digit,
    // wider markers.
    for (const words of ["Every block", "runtime draws", "Every block", "block type", "the runtime", "Every block", "type the", "runtime", "draws"]) {
      await reveal(page, "prose");
      await gesture(page, { gesture: "text", block: "prose", recipe: { in: "p", words } });
      const body = `close ${notes.length + 1}`;
      await save(body);
      notes.push({ body, block: "prose", words, what: `prose "${words}"` });
    }
    // Every check runs on the final layout, after all the notes are saved.
    await checkLayout(width, `at ${width} px`);
    // A click at each marker's centre must reach that marker (Playwright
    // clicks the centre and waits until the marker is the hit target there).
    for (const [i, { body, what }] of notes.entries()) {
      // Centred first, so the sticky comment hint never covers the marker.
      const marker = page.getByTestId("note-marker").nth(i);
      await marker.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));
      await marker.click({ timeout: 5000 }).catch((error) => { throw new Error(`close markers at ${width} px, marker ${i + 1} (${what}) takes no click at its centre: ${error.message.split("\n")[0]}`); });
      await page.getByTestId("composer").waitFor();
      assert.equal(await page.getByTestId("composer-text").inputValue(), body, `close markers at ${width} px, marker ${i + 1} (${what}) opened another note`);
      await page.getByTestId("composer-cancel").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
    }
    if (width < 800) {
      // Armed at phone width, the rail stays closed; widened, the desktop
      // rule applies with the sections list shown and the rail still closed.
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
      assert.equal(await page.locator("#cf-feedback-panel").getAttribute("data-open"), "false", "close markers after widening: the rail opened");
      await checkLayout(1280, `widened from ${width} to 1280 px with the rail closed`);
    }
  } finally {
    // The last marker drawn is on top, so it takes the click even where
    // markers overlap, and the first failure stays the one reported.
    while (await page.getByTestId("note-marker").count()) {
      const marker = page.getByTestId("note-marker").last();
      await marker.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));
      await marker.click();
      await page.getByTestId("composer-delete").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
    }
    await page.setViewportSize({ width: 1280, height: 900 });
  }
}

// The first stretch of a marker row, between `start` and `end`, wide enough
// for a marker of `size` with a 2 px gap on each side; null when the row is
// full. Row markers outside the stretch do not matter.
function freeStretch(row, start, end, size) {
  const inside = row.filter((marker) => marker.right + 2 > start && marker.left - 2 < end).sort((a, b) => a.left - b.left);
  let from = start;
  for (const marker of [...inside, { left: end + 2, right: end }]) {
    if (marker.left - 2 - from >= size) return { from, to: marker.left - 2 };
    from = Math.max(from, marker.right + 2);
  }
  return null;
}

// Where a prose marker of `size` px ends in its own place, as the placement
// rule puts it: in the gutter left of the block, past the sections list
// (`routeRight`) when that narrows it; with no list, above the line at the
// block's right end, kept on the page.
function baseRight(geometry, size, routeRight) {
  if (routeRight !== null) return Math.max(geometry.blockLeft - 8, routeRight + 2 + size);
  return Math.min(geometry.blockRight, geometry.rootRight - 2, geometry.viewRight - 0.03 * size - 8);
}

// Negative controls for the row oracle (Codex C158-R2-2): a row with one
// marker at its left bound and room after it is not full, nor is one with a
// gap in the middle; a packed row is.
assert.deepEqual(freeStretch([{ left: 2, right: 40 }], 2, 360, 38), { from: 42, to: 360 });
assert.deepEqual(freeStretch([{ left: 2, right: 40 }, { left: 122, right: 160 }], 2, 160, 38), { from: 42, to: 120 });
assert.equal(freeStretch([{ left: 2, right: 40 }, { left: 42, right: 80 }, { left: 82, right: 120 }], 2, 120, 38), null);
// Codex's shifted sparse row: an overflow chain at the page's left edge is
// not in its own place, whatever the row holds.
assert.ok(Math.abs(40 - baseRight({ blockLeft: 328, blockRight: 1000, rootRight: 1100, viewRight: 1280 }, 38, 264)) > 1);
assert.ok(Math.abs(40 - baseRight({ blockLeft: 20, blockRight: 355, rootRight: 375, viewRight: 375 }, 38, null)) > 1);

async function saveNote(page, body) {
  const composer = page.getByTestId("composer");
  await page.getByTestId("composer-text").fill(body);
  await page.getByTestId("composer-save").click();
  await composer.waitFor({ state: "detached" });
}

async function findFile(directory, name) {
  const { readdir } = await import("node:fs/promises");
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isFile() && entry.name === name) return path;
    if (entry.isDirectory() && !entry.isSymbolicLink()) {
      const found = await findFile(path, name).catch(() => null);
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

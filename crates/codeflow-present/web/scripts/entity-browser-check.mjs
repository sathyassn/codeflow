// Review entities end to end (SPC-014 B2 to B4, TSK-118): the real `codeflow`
// binary serves the operator's delivery document at schema_version 2, and a
// task-owned headless Chrome profile targets a rectangle, a thin arrow, a
// label inside a group, a node's sibling label and a drawn figure mark by
// mouse, touch and keyboard, in light and dark. Each capture must name the
// intended entity in the float chip, the composer and the rail; "select
// enclosing" climbs from an entity to its block; the service must accept
// every entity note, store its own label, a PNG crop and crop_check where
// it cannot measure, and keep the v1 feedback stream free of entity data.
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
const fixture = join(webRoot, "../tests/fixtures/contract-v2/documents/delivery-v2.json");

// Each target: how to find it, the entity it must resolve to, the label the
// service gives it, and where on it a pointer lands.
const TARGETS = [
  { name: "rectangle", block: "stage-registry", locate: "[data-cf-target='file-a']", entity: "file-a", label: "writes TSK-100.md" },
  // 4 px above a 1.5 px line: only the thin-stroke padding makes it a hit.
  { name: "thin arrow", block: "stage-registry", locate: "[data-cf-target='reject-b']", entity: "reject-b", label: "rejected: someone pushed since you looked", offsetY: -4 },
  { name: "label inside a group", block: "stage-registry", locate: "[data-cf-group='agent-a'] text", entity: "agent-a", label: "Agent A one laptop", keyboard: "[data-cf-group='agent-a']" },
  { name: "sibling label", block: "stage-registry", locate: "text[data-cf-for='push-a']", entity: "push-a", label: "add ids/TSK/100, push", keyboard: "[data-cf-target='push-a']" },
  // The figure draws each mark in both compositions; the shown one is the target.
  { name: "figure mark", block: "stage-flow", locate: "[data-cf-entity='s3']", entity: "s3", label: "Spec", drawn: true },
];
const GESTURES = ["mouse", "touch", "keyboard"];
const THEMES = ["light", "dark"];

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-entities-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
};
const run = (args) => execFileSync(codeflow, args, { cwd: project, env: environment, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 60_000 });
let sessionId = null;
let context = null;
try {
  for (const directory of [project, environment.HOME, environment.TMPDIR, environment.XDG_STATE_HOME, join(root, "profile")]) await mkdir(directory, { recursive: true });
  execFileSync("git", ["init", "--quiet"], { cwd: project, env: environment });
  const document = join(project, "delivery.json");
  await writeFile(document, await readFile(fixture, "utf8"));
  const opened = run(["present", "open", document, "--no-launch"]);
  sessionId = opened.match(/session ([0-9a-f-]+) ready/u)?.[1] ?? null;
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  if (!sessionId || !bootstrap) throw new Error(`could not parse present open output: ${opened}`);
  const port = JSON.parse(run(["present", "list"]))[0]?.service_port;

  context = await chromium.launchPersistentContext(join(root, "profile"), {
    executablePath: await findBrowser(),
    headless: true,
    hasTouch: true,
    viewport: { width: 1280, height: 900 },
    args: ["--disable-background-networking", "--disable-component-update", "--disable-default-apps", "--disable-sync", "--no-default-browser-check", "--no-first-run"],
  });
  const page = context.pages()[0] ?? await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit", timeout: 120_000 });
  await page.waitForURL(new RegExp(`^http://127\\.0\\.0\\.1:${port}/app/`, "u"), { waitUntil: "domcontentloaded", timeout: 60_000 });
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  await page.locator("[data-cf-block-id='stage-flow'] [data-cf-figure-block]").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-block-id='stage-flow'] [data-cf-figure-block]")?.getAttribute("data-cf-figure-block") === "ready");

  // Framing is visible in the live page: the stage and the migrated figure.
  const titles = await page.evaluate(() => ({
    stage: document.querySelector("[data-cf-block-id='stage-registry'] .cf-frame-title")?.innerText ?? null,
    figure: document.querySelector("[data-cf-block-id='stage-flow'] .cf-fig-title")?.innerText ?? null,
  }));
  assert.match(titles.stage ?? "", /^Figure 2 · How a task number is issued$/u, JSON.stringify(titles));
  assert.match(titles.figure ?? "", /^Figure 5 · From a question to a release$/u, JSON.stringify(titles));

  const expected = [];
  for (const theme of THEMES) {
    await page.getByTestId("settings-btn").click();
    await page.getByTestId("settings-panel").getByRole("button", { name: theme === "light" ? "Light" : "Dark", exact: true }).click();
    await page.keyboard.press("Escape");
    await page.getByTestId("settings-panel").waitFor({ state: "detached" });
    for (const target of TARGETS) {
      for (const gesture of GESTURES) {
        const where = `${theme} ${gesture} ${target.name}`;
        const chip = await capture(page, target, gesture).catch((error) => { throw new Error(`${where}: ${error.message.split("\n")[0]}`); });
        if (chip !== null) assert.ok(chip.includes(target.label), `${where}: the chip shows ${JSON.stringify(chip)}`);
        await saveNote(page, `${where}: keep this part as drawn.`, target.label, where);
        expected.push({ where, target, variant: target.shown });
      }
    }
  }
  // A drawing shown at a different scale on each axis (T118-3): stretched to
  // about 0.3 px per unit vertically, the stroke padding must hold inside the
  // service's 8 unit tolerance on that axis, or the submit below is refused.
  const stretch = (on) => page.evaluate((stretched) => {
    const svg = document.querySelector("[data-cf-block-id='stage-registry'] svg");
    if (stretched) {
      svg.setAttribute("preserveAspectRatio", "none");
      svg.style.height = "130px";
    } else {
      svg.removeAttribute("preserveAspectRatio");
      svg.style.height = "auto";
    }
    const matrix = svg.getScreenCTM();
    return { x: Math.hypot(matrix.a, matrix.b), y: Math.hypot(matrix.c, matrix.d) };
  }, on);
  const scale = await stretch(true);
  assert.ok(scale.y < 0.75 && scale.x > scale.y * 1.5, `the stage did not stretch: ${JSON.stringify(scale)}`);
  const stretched = { ...TARGETS[0], name: "rectangle on a stretched drawing" };
  await capture(page, stretched, "mouse");
  await saveNote(page, "Stretched: keep this part as drawn.", stretched.label, stretched.name);
  expected.push({ where: stretched.name, target: stretched });
  await stretch(false);
  // QA defect 5 in its figure form: a drag across a figure's label pins, or
  // says why it cannot; it is never a dead gesture.
  await armComment(page);
  const word = page.locator("[data-cf-block-id='stage-flow'] [data-cf-for]").filter({ visible: true }).first();
  await word.scrollIntoViewIfNeeded();
  const span = await word.boundingBox();
  await page.mouse.move(span.x + 1, span.y + span.height / 2);
  await page.mouse.down();
  await page.mouse.move(span.x + span.width - 1, span.y + span.height / 2, { steps: 6 });
  await page.mouse.up();
  const dragged = await page.getByTestId("float-chip").waitFor({ timeout: 5_000 }).then(() => page.getByTestId("float-chip").innerText(), () => null);
  const told = await page.getByRole("status").innerText().catch(() => "");
  assert.ok(dragged !== null || told.trim().length > 0, "a drag across a figure label was a dead gesture");
  if (dragged !== null) {
    await page.getByTestId("float-comment").click();
    const quote = (await word.textContent()).trim();
    await saveNote(page, "The label wording is right.", quote, "defect 5 drag");
    expected.push({ where: "defect 5 drag", target: { block: "stage-flow", drag: true } });
  }
  // Select enclosing: from the rectangle, Shift+Enter climbs to its block.
  await armPickElement(page);
  await focusByKeyboard(page, "[data-cf-block-id='stage-registry'] [data-cf-target='file-a']");
  await page.keyboard.press("Shift+Enter");
  const enclosing = await page.evaluate(() => document.activeElement?.getAttribute("data-cf-block-id") ?? null);
  assert.equal(enclosing, "stage-registry", "select enclosing did not climb to the block");
  await page.keyboard.press("Enter");
  await saveNote(page, "The whole stage: keep the order of the messages.", "How a task number is issued", "select enclosing");

  await page.getByLabel("Verdict").selectOption("approve_with_notes");
  await page.getByRole("button", { name: "Submit review" }).click();
  await page.getByRole("status").getByText(/Review received/u).waitFor({ timeout: 60_000 });
  assert.deepEqual(errors, []);

  // The v1 stream carries no entity data; the history keeps it.
  const delivered = JSON.parse(run(["present", "feedback", sessionId]).trim());
  assert.equal(delivered.notes.length, expected.length + 1);
  assert.ok(delivered.notes.every((note) => note.entity_selector === undefined), "the v1 stream carried an entity selector");
  const history = JSON.parse(run(["present", "history", sessionId]));
  assert.equal(history.schema_version, 2);
  const stored = history.feedback_events.filter((event) => event.envelope).flatMap((event) => event.envelope.notes);
  assert.equal(stored.length, expected.length + 1);
  expected.forEach(({ where, target, variant }, index) => {
    const note = stored[index];
    assert.equal(note.block_id, target.block, where);
    if (target.drag) return;
    assert.equal(note.entity_selector?.entity_id, target.entity, `${where}: ${JSON.stringify(note.entity_selector)}`);
    assert.equal(note.entity_selector.label, target.label, where);
    if (target.drawn) assert.equal(note.entity_selector.variant, variant, where);
    assert.equal(note.excerpt?.image?.media_type, "image/png", `${where}: ${note.excerpt?.image?.media_type}`);
    assert.ok(note.entity_selector.crop_box, `${where}: no crop box`);
    // A group holding text has no server bounds, so its crop is unverified.
    assert.equal(note.entity_selector.crop_check, target.name === "label inside a group" ? "unverified" : undefined, where);
  });
  assert.equal(stored.at(-1).entity_selector, undefined, "the block note carried an entity");
  // A new revision re-anchors every note and says so when a note moved or
  // lost its part (B1): file-a is relabelled, reject-b is removed.
  const revised = JSON.parse(await readFile(fixture, "utf8"));
  const registry = revised.blocks.find((block) => block.id === "stage-registry");
  registry.html = registry.html
    .replace(">writes TSK-100.md<", ">writes the TSK-100 file<")
    .replace(/<line data-cf-target='reject-b'[^>]*><\/line><text data-cf-for='reject-b'[^>]*>[^<]*<\/text>/u, "");
  assert.ok(!registry.html.includes("reject-b") && registry.html.includes("TSK-100 file"), "the revision did not change the stage");
  await writeFile(join(project, "delivery-2.json"), `${JSON.stringify(revised, null, 2)}\n`);
  run(["present", "update", sessionId, join(project, "delivery-2.json")]);
  await page.reload({ waitUntil: "domcontentloaded" });
  // Earlier feedback lives in the review rail.
  await page.locator("#cf-comment-toggle").waitFor({ state: "visible" });
  await armComment(page);
  const earlier = page.getByTestId("feedback-history");
  await earlier.waitFor({ timeout: 20_000 }).catch(async (error) => {
    throw new Error(`no earlier feedback after the revision at ${page.url()}: ${(await page.locator("body").innerText()).slice(0, 600)}; ${error.message.split("\n")[0]}`);
  });
  await earlier.locator("summary").click();
  const notices = await earlier.innerText();
  assert.match(notices, /The part it names was relabelled in this revision\./u, notices.slice(0, 2_000));
  // A part whose label held while its block changed is named as the same part (Grok F1).
  assert.match(notices, /Still names the same part; the block around it changed in this revision\./u, notices.slice(0, 2_000));
  assert.doesNotMatch(notices, /The part it names changed/u, notices.slice(0, 2_000));
  assert.match(notices, /Shown on the block: /u, notices.slice(0, 2_000));
  // The offline export keeps the framing outside the stage frame.
  const exported = join(root, "delivery-export.html");
  run(["present", "export", sessionId, "--out", exported, "--mode", "light"]);
  await page.goto(pathToFileURL(exported).href);
  await page.waitForFunction(() => document.querySelector("[data-cf-block-id='stage-flow'] [data-cf-figure-block]")?.getAttribute("data-cf-figure-block") === "ready", null, { timeout: 30_000 });
  const exportedTitles = await page.evaluate(() => [...document.querySelectorAll(".cf-frame-title, .cf-fig-title")]
    .filter((line) => line.checkVisibility())
    .map((line) => line.innerText));
  assert.ok(exportedTitles.includes("Figure 2 · How a task number is issued"), JSON.stringify(exportedTitles));
  assert.ok(exportedTitles.includes("Figure 5 · From a question to a release"), JSON.stringify(exportedTitles));
  process.stdout.write(`cf-present entity checks passed: ${TARGETS.length} targets by ${GESTURES.join(", ")} in ${THEMES.join(" and ")}, one on a drawing stretched unevenly, a figure label drag that pins (QA defect 5), select enclosing, framing titles, ${stored.length} notes stored with server labels and PNG crops, re-anchored with relabel and block fallback notices, and framing titles visible in the export\n`);
} finally {
  if (context) await context.close().catch(() => undefined);
  if (sessionId) {
    try { run(["present", "close", sessionId]); } catch { /* the service may already be gone */ }
  }
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

async function variantOf(page) {
  return page.evaluate(() => document.activeElement?.closest("svg[data-cf-variant]")?.getAttribute("data-cf-variant") ?? null);
}

async function armComment(page) {
  if (await page.locator(".cf-hint.on").count() === 0) {
    await page.locator("#cf-comment-toggle").click();
    await page.locator(".cf-hint.on").waitFor();
  }
}

async function armPickElement(page) {
  await armComment(page);
  const tools = page.getByTestId("notes-dock").locator(".cf-tools");
  if (!await tools.evaluate((element) => element.open)) await tools.locator("summary").click();
  await page.getByTestId("tool-pick-element").click();
}

// Arrow through the keyboard stops until the one wanted has focus.
async function focusByKeyboard(page, selector) {
  for (let step = 0; step < 1500; step += 1) {
    if (await page.evaluate((wanted) => document.activeElement?.matches(wanted) ?? false, selector)) return;
    await page.keyboard.press("ArrowRight");
  }
  throw new Error(`the keyboard never reached ${selector}`);
}

// One capture; returns the chip text for a pointer gesture, or null for the
// keyboard, which opens the composer directly.
async function capture(page, target, gesture) {
  // A horizontal line has no height, which Playwright counts as hidden, so
  // only a drawn figure (two compositions, one shown) filters by visibility.
  const all = page.locator(`[data-cf-block-id='${target.block}'] ${target.locate}`);
  const locator = (target.drawn ? all.filter({ visible: true }) : all).first();
  await locator.scrollIntoViewIfNeeded();
  if (gesture === "keyboard") {
    await armPickElement(page);
    await focusByKeyboard(page, `[data-cf-block-id='${target.block}'] ${target.keyboard ?? target.locate}`);
    target.shown = await variantOf(page);
    await page.keyboard.press("Enter");
    return null;
  }
  await armComment(page);
  const box = await locator.boundingBox();
  if (!box) throw new Error(`${target.name} is not visible`);
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2 + (target.offsetY ?? 0);
  target.shown = await locator.evaluate((element) => element.closest("svg[data-cf-variant]")?.getAttribute("data-cf-variant") ?? null);
  if (gesture === "mouse") await page.mouse.click(x, y);
  else await page.touchscreen.tap(x, y);
  const chip = page.getByTestId("float-chip");
  await chip.waitFor({ timeout: 10_000 });
  const text = await chip.innerText();
  await page.getByTestId("float-comment").click();
  return text;
}

async function saveNote(page, body, label, where) {
  const composer = page.getByTestId("composer");
  await composer.waitFor();
  const shown = await composer.innerText();
  assert.ok(shown.includes(label), `${where}: the composer shows ${JSON.stringify(shown.slice(0, 200))}`);
  await page.getByTestId("composer-text").fill(body);
  await page.getByTestId("composer-save").click();
  await composer.waitFor({ state: "detached" });
  const row = page.getByTestId("note-row").last();
  const rail = await row.innerText();
  assert.ok(rail.includes(label), `${where}: the rail shows ${JSON.stringify(rail)}`);
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

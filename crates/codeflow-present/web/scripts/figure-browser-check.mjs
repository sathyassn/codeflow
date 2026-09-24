// The figure block end to end: the real `codeflow` binary validates a present
// document carrying every authored specimen, the service renders each block's
// placeholder, the export draws it with the grammar module, and the portal's
// own probe reads the result in light and dark at 1280 and 390 px. The rule
// outcomes must equal the portal's specimen table, so a specimen renders the
// same in present as in the portal. A declaration the grammar refuses shows
// its reason in place of the figure.
//
// Usage: node scripts/figure-browser-check.mjs [--screenshots <dir>]
import { execFileSync } from "node:child_process";
import { access, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { canonicalJson, figureRuleFailures, probeFigures, THRESHOLDS } from "../src/figure-grammar.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = resolve(process.env.CF_PRESENT_CODEFLOW ?? join(repoRoot, "target/debug/codeflow"));
const specimenRoot = join(repoRoot, "docs-portal/tests/fixtures/figures");
const screenshotIndex = process.argv.indexOf("--screenshots");
const screenshots = screenshotIndex > 0 ? resolve(process.argv[screenshotIndex + 1]) : null;

// The portal's specimen outcome table (docs-portal/tests/figure-rules-browser.test.mjs),
// for the authored specimens present can draw.
const EXPECTED = {
  "01-flow.json": [3],
  "02-structure.json": [],
  "03-layering.json": [],
  "04-sequence.json": [],
  "05-state.json": [],
  "06-coverage.json": [],
  "07-extent.json": [],
  "08-derivation.json": [8],
  "09-graph.json": [3, 8],
};

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-figures-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
};
const run = (args) => execFileSync(codeflow, args, { cwd: project, env: environment, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 30_000 });
let sessionId = null;
let browser = null;
try {
  for (const directory of [project, environment.HOME, environment.TMPDIR, environment.XDG_STATE_HOME]) await mkdir(directory, { recursive: true });
  execFileSync("git", ["init", "--quiet"], { cwd: project, env: environment });

  const names = (await readdir(specimenRoot)).filter((name) => Object.hasOwn(EXPECTED, name)).sort();
  if (canonicalJson(names) !== canonicalJson(Object.keys(EXPECTED).sort())) throw new Error(`specimens changed: ${names.join(", ")}`);
  const blocks = [];
  for (const name of names) {
    blocks.push({ type: "figure", id: name.replace(/\.json$/u, ""), declaration: JSON.parse(await readFile(join(specimenRoot, name), "utf8")) });
  }
  const refused = structuredClone(blocks.find((block) => block.id === "03-layering"));
  refused.id = "refused";
  refused.declaration.figure.caption = "One sentence. And another.";
  blocks.push(refused);
  const document = join(project, "figures.json");
  await writeFile(document, `${JSON.stringify({ schema_version: 1, title: "Figure block specimens", blocks }, null, 2)}\n`);

  const opened = run(["present", "open", document, "--no-launch"]);
  sessionId = opened.match(/session ([0-9a-f-]+) ready/u)?.[1] ?? null;
  if (sessionId === null) throw new Error(`could not parse present open output: ${opened}`);

  browser = await chromium.launch({ executablePath: await findBrowser(), headless: true });
  const page = await browser.newPage();
  const consoleErrors = [];
  page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  const observed = new Map();
  for (const mode of ["light", "dark"]) {
    const exported = join(root, `figures-${mode}.html`);
    run(["present", "export", sessionId, "--out", exported, "--mode", mode]);
    for (const [label, width] of [["wide", 1280], ["narrow", 390]]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(pathToFileURL(exported).href);
      await page.waitForFunction(() => [...document.querySelectorAll("[data-cf-figure-block]")].every((block) => block.getAttribute("data-cf-figure-block") !== "pending"), null, { timeout: 30_000 });
      const states = await page.evaluate(() => Object.fromEntries([...document.querySelectorAll("[data-cf-figure-block]")].map((block) => [block.closest("[data-cf-block-id]").dataset.cfBlockId, { state: block.dataset.cfFigureBlock, status: block.querySelector("[data-cf-figure-status]")?.textContent ?? "" }])));
      for (const name of names) {
        const id = name.replace(/\.json$/u, "");
        if (states[id]?.state !== "ready") throw new Error(`${id} did not draw in ${mode} at ${width}: ${JSON.stringify(states[id])}`);
      }
      if (states.refused?.state !== "failed" || !/rule 9: the caption must be exactly one sentence/u.test(states.refused.status)) {
        throw new Error(`the refused declaration did not show its reason: ${JSON.stringify(states.refused)}`);
      }
      const probes = await page.evaluate(probeFigures, { clearance: THRESHOLDS.labelClearancePx });
      if (probes.length !== names.length) throw new Error(`${mode} at ${width}: probed ${probes.length} figures, expected ${names.length}`);
      probes.forEach((probe, index) => {
        const key = `${names[index]}`;
        const entry = observed.get(key) ?? {};
        entry[mode === "light" ? label : `${label}Dark`] = probe;
        observed.set(key, entry);
      });
      if (screenshots !== null) {
        await mkdir(screenshots, { recursive: true });
        await page.screenshot({ path: join(screenshots, `present-figures-${mode}-${width}.png`), fullPage: true });
      }
    }
  }
  if (consoleErrors.length) throw new Error(`console errors: ${consoleErrors.join(" | ")}`);

  const outcome = {};
  const detail = {};
  for (const name of names) {
    const failures = figureRuleFailures(observed.get(name));
    outcome[name] = [...new Set(failures.map((failure) => failure.rule))].sort((a, b) => a - b);
    detail[name] = { rules: outcome[name], first: [...new Set(failures.map((failure) => `rule ${failure.rule}: ${failure.message}`))].slice(0, 4) };
  }
  if (canonicalJson(outcome) !== canonicalJson(EXPECTED)) {
    throw new Error(`present figure outcomes differ from the portal specimen table:\n${JSON.stringify(detail, null, 2)}`);
  }
  process.stdout.write(`cf-present figure checks passed: ${names.length} specimens drawn in light and dark at 1280 and 390 px with the portal's rule outcomes, one refused declaration shown\n`);
} finally {
  await browser?.close();
  if (sessionId !== null) await closeSession(sessionId);
  await rm(root, { recursive: true, force: true });
}

// Close stops the service asynchronously; clear succeeds once it has exited.
async function closeSession(id) {
  try { run(["present", "close", id]); } catch { /* the clear below proves the outcome */ }
  const deadline = Date.now() + 15_000;
  for (;;) {
    try {
      run(["present", "clear", id, "--older-than", "0h"]);
      return;
    } catch (error) {
      if (Date.now() > deadline) throw error;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
}

async function findBrowser() {
  const candidates = [
    process.env.CF_PRESENT_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Continue to the next explicit executable candidate.
    }
  }
  throw new Error("No qualified browser executable found; set CF_PRESENT_BROWSER");
}

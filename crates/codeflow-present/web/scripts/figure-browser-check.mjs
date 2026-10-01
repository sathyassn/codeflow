// The figure block end to end: the real `codeflow` binary validates a present
// document carrying every authored specimen, the service renders each block's
// placeholder, the export draws it with the grammar module, and the portal's
// own probe reads the result in light and dark at 1280 and 390 px. The rule
// outcomes must equal the portal's specimen table, so a specimen renders the
// same in present as in the portal. A declaration the grammar refuses shows
// its reason in place of the figure. The grammar's field reference example
// must validate and derive its fact. The DOM guard passes every specimen the
// grammar draws and refuses each attribute, value and reference it does not.
//
// Usage: node scripts/figure-browser-check.mjs [--screenshots <dir>]
import { execFileSync } from "node:child_process";
import { access, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { readFileSync, realpathSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { assertNoPolicyViolations, recordPolicyViolations } from "./csp-violations.mjs";
import { canonicalJson, checkFacts, composeFigure, figureRuleFailures, probeFigures, renderFigure, THRESHOLDS, validateDeclaration } from "../src/figure-grammar.mjs";
import { codeflowBinary } from "./codeflow-binary.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = codeflowBinary(repoRoot);
const specimenRoot = join(repoRoot, "docs-portal/tests/fixtures/figures");
const screenshotIndex = process.argv.indexOf("--screenshots");
const screenshots = screenshotIndex > 0 ? resolve(process.argv[screenshotIndex + 1]) : null;

// The portal's specimen outcome table (docs-portal/tests/figure-rules-browser.test.mjs),
// for the authored specimens present can draw.
const EXPECTED = {
  "01-flow.json": [],
  "02-structure.json": [],
  "03-layering.json": [],
  "04-sequence.json": [],
  "05-state.json": [],
  "06-coverage.json": [],
  "07-extent.json": [],
  "08-derivation.json": [],
  "09-graph.json": [],
};

await access(codeflow);
await checkReferenceExample();
const root = await mkdtemp(join(tmpdir(), "cf-present-figures-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
  // A service this check starts exits once this process is gone.
  CF_PRESENT_OWNER_PID: String(process.pid),
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
  await checkGuard(browser);
  const page = await browser.newPage();
  await recordPolicyViolations(page);
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
      await assertNoPolicyViolations(page, `${mode} export at ${width}`);
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
    const { declaration } = blocks.find((block) => block.id === name.replace(/\.json$/u, ""));
    const failures = figureRuleFailures({ ...observed.get(name), composed: composeFigure(declaration) });
    outcome[name] = [...new Set(failures.map((failure) => failure.rule))].sort((a, b) => a - b);
    detail[name] = { rules: outcome[name], first: [...new Set(failures.map((failure) => `rule ${failure.rule}: ${failure.message}`))].slice(0, 4) };
  }
  if (canonicalJson(outcome) !== canonicalJson(EXPECTED)) {
    throw new Error(`present figure outcomes differ from the portal specimen table:\n${JSON.stringify(detail, null, 2)}`);
  }
  process.stdout.write(`cf-present figure checks passed: ${names.length} specimens drawn in light and dark at 1280 and 390 px with the portal's rule outcomes, one refused declaration shown, the DOM guard accepting the grammar boundary, a 1001-point polyline included, and refusing 18 departures from it\n`);
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

// The field reference in figure-grammar.md section 6 shows one declaration.
// It must validate, draw and derive its fact, so the documentation cannot
// drift from the module it documents.
// The guard runs in a page against the grammar's own output for every
// specimen, then against that output with one thing the grammar never writes.
async function checkGuard(browser) {
  const bundled = await build({ absWorkingDir: webRoot, entryPoints: ["src/figure-guard.ts"], bundle: true, format: "iife", globalName: "FigureGuard", write: false, logLevel: "silent" });
  const drawn = {};
  for (const name of (await readdir(specimenRoot)).filter((file) => file.endsWith(".json")).sort()) {
    const declaration = JSON.parse(await readFile(join(specimenRoot, name), "utf8"));
    const bound = declaration.figure.binding === "derived" ? { derived: { commit_desc_max_len: 50, commit_subject_max_len: 72 } } : null;
    drawn[name] = renderFigure(declaration, { idPrefix: "cf-present-figure-1", bound });
  }
  // The grammar's own boundary: a 211-character mark id with the text that
  // labels it, a text carrying all four styles, and a 1001-point polyline,
  // since the grammar sets no point count. No specimen draws a polyline, so
  // one straight critical arc of the graph becomes one.
  const boundary = JSON.parse(await readFile(join(specimenRoot, "09-graph.json"), "utf8"));
  const mark = boundary.figure.wide.draw.find((item) => item.state !== undefined && item.id !== undefined);
  mark.id = `m${"-a".repeat(105)}`;
  const label = boundary.figure.wide.draw.find((item) => item.text !== undefined);
  label.for = [mark.id];
  label.style = ["strong", "mute", "head", "mono"];
  const line = boundary.figure.wide.draw.find((item) => item.state === "done" && /^M[\d.]+ [\d.]+L[\d.]+ [\d.]+$/u.test(item.d ?? ""));
  const [x0, y0, x1, y1] = line.d.match(/[\d.]+/gu).map(Number);
  delete line.d;
  line.shape = "polyline";
  line.points = Array.from({ length: 1001 }, (_, step) => [Math.round((x0 + ((x1 - x0) * step) / 1000) * 100) / 100, Math.round((y0 + ((y1 - y0) * step) / 1000) * 100) / 100]);
  validateDeclaration(boundary);
  drawn["boundary"] = renderFigure(boundary, { idPrefix: "cf-present-figure-1" });
  if ((drawn["boundary"].match(/<polyline [^>]*points="([^"]*)"/u)?.[1].split(" ").length ?? 0) !== 1001) throw new Error("the boundary figure does not draw a 1001-point polyline");
  // A narrow that keeps the wide marks with a reason carries the grammar's
  // declared-same marker, which the guard must accept.
  if (!Object.values(drawn).some((figure) => figure.includes('data-cf-same-marks="declared"'))) throw new Error("no specimen draws a declared same mark set");
  const page = await browser.newPage();
  try {
    await page.setContent("<!doctype html><html><body></body></html>");
    await page.addScriptTag({ content: bundled.outputFiles[0].text });
    const result = await page.evaluate(({ drawn, clean }) => {
      const guard = (html, change = null) => {
        const template = document.createElement("template");
        template.innerHTML = html;
        change?.(template.content);
        return globalThis.FigureGuard.unsafeFigureNode(template.content);
      };
      const svgElement = (name) => document.createElementNS("http://www.w3.org/2000/svg", name);
      const set = (selector, name, value) => (root) => root.querySelector(selector).setAttribute(name, value);
      const html = drawn[clean];
      return {
        specimens: Object.fromEntries(Object.entries(drawn).map(([name, figure]) => [name, guard(figure)])),
        refused: {
          contenteditable: guard(html, set("figure", "contenteditable", "true")),
          "data-unknown": guard(html, set("figure", "data-unknown", "yes")),
          "escaped css url": guard(html, set("svg [data-state] rect", "filter", "u\\72l(https://example.invalid/x#y)")),
          "remote fill": guard(html, set("svg [data-state] rect", "fill", "url(https://example.invalid/x#y)")),
          "fill outside the figure": guard(html, set("svg [data-state] rect", "fill", "url(#elsewhere)")),
          "labelled outside the figure": guard(html, set("svg.cf-fig-svg", "aria-labelledby", "elsewhere-t elsewhere-d")),
          "event handler": guard(html, set("svg.cf-fig-svg", "onload", "void 0")),
          "inline style": guard(html, set("figure", "style", "color: red")),
          "extra class": guard(html, set("figure", "class", "cf-fig extra")),
          "namespaced href": guard(html, (root) => root.querySelector("svg [data-state] rect").setAttributeNS("http://www.w3.org/1999/xlink", "xlink:href", "#x")),
          foreignObject: guard(html, (root) => root.querySelector("svg.cf-fig-svg").append(svgElement("foreignObject"))),
          "html title in svg": guard(html, (root) => root.querySelector("svg.cf-fig-svg").append(document.createElement("title"))),
          "unknown shape class": guard(html, set("svg [data-state] rect", "class", "cf-m-totally-unknown")),
          "unknown text style": guard(html, set("svg text", "class", "cf-t cf-t--unlisted")),
          "repeated text style": guard(html, set("svg text", "class", "cf-t cf-t--mono cf-t--mono")),
          "unknown family": guard(html, set("figure", "data-cf-figure", "unlisted")),
          "undeclared state": guard(html, set("svg [data-state]", "data-state", "unlisted")),
          "same marks not declared": guard(html, set("figure", "data-cf-same-marks", "yes")),
        },
      };
    }, { drawn, clean: "03-layering.json" });
    const passed = Object.entries(result.specimens).filter(([, reason]) => reason !== null);
    if (passed.length) throw new Error(`the guard refused grammar output: ${JSON.stringify(Object.fromEntries(passed))}`);
    const expected = {
      contenteditable: "a contenteditable attribute on <figure>",
      "data-unknown": "a data-unknown attribute on <figure>",
      "escaped css url": "a filter attribute on <rect>",
      "remote fill": "an unexpected fill value on <rect>",
      "fill outside the figure": "a fill reference outside the figure on <rect>",
      "labelled outside the figure": "an aria-labelledby reference outside the figure on <svg>",
      "event handler": "a onload attribute on <svg>",
      "inline style": "a style attribute on <figure>",
      "extra class": "an unexpected class value on <figure>",
      "namespaced href": "a xlink:href attribute on <rect>",
      foreignObject: "a <foreignObject> element",
      "html title in svg": "a <title> element",
      "unknown shape class": "an unexpected class value on <rect>",
      "unknown text style": "an unexpected class value on <text>",
      "repeated text style": "an unexpected class value on <text>",
      "unknown family": "an unexpected data-cf-figure value on <figure>",
      "undeclared state": "a data-state the figure does not declare on <g>",
      "same marks not declared": "an unexpected data-cf-same-marks value on <figure>",
    };
    if (canonicalJson(result.refused) !== canonicalJson(expected)) throw new Error(`the guard's refusals differ:\n${JSON.stringify(result.refused, null, 2)}`);
  } finally { await page.close(); }
}

// Each fact re-derives from a file inside the repository it cites. The path
// is resolved through every symlink, final component and parents alike, and
// refused unless the real path stays under the repository's real root, so a
// fact can never derive from text that exists only outside the checkout.
function repositorySourceReader(root) {
  const realRoot = realpathSync(root);
  return (path) => {
    let real;
    try { real = realpathSync(join(realRoot, path)); } catch { return null; }
    if (real !== realRoot && !real.startsWith(realRoot + sep)) throw new Error(`fact source resolves outside the repository: ${path}`);
    return readFileSync(real, "utf8");
  };
}

// The reader keeps in-repository citations, an in-repository symlink
// included, and refuses a missing file, a missing anchor, and a symlinked
// file or parent directory that leads outside the repository.
async function checkSourceContainment() {
  const base = await mkdtemp(join(tmpdir(), "cf-fact-sources-"));
  try {
    const repo = join(base, "repo");
    const proof = "# Proof\n\n## Proof\n\nThe cited sentence.\n";
    await mkdir(join(repo, "docs"), { recursive: true });
    await mkdir(join(base, "outside"));
    await writeFile(join(repo, "docs/proof.md"), proof);
    await writeFile(join(base, "outside.md"), proof);
    await writeFile(join(base, "outside/proof.md"), proof);
    symlinkSync("docs/proof.md", join(repo, "alias.md"));
    symlinkSync("../outside.md", join(repo, "linked.md"));
    symlinkSync("../outside", join(repo, "linkdir"));
    const fact = (source) => ({ claim: source, source, derive: "the cited sentence", check: { kind: "contains", text: "The cited sentence." }, value: true });
    const sources = ["docs/proof.md#proof", "alias.md#proof", "absent.md#proof", "docs/proof.md#elsewhere", "linked.md#proof", "linkdir/proof.md#proof"];
    const results = checkFacts({ facts: sources.map(fact) }, repositorySourceReader(repo)).map((result) => [result.source, result.matches, result.error]);
    const expected = [
      ["docs/proof.md#proof", true, null],
      ["alias.md#proof", true, null],
      ["absent.md#proof", false, "fact source does not exist: absent.md"],
      ["docs/proof.md#elsewhere", false, "fact anchor does not exist: docs/proof.md#elsewhere"],
      ["linked.md#proof", false, "fact source resolves outside the repository: linked.md"],
      ["linkdir/proof.md#proof", false, "fact source resolves outside the repository: linkdir/proof.md"],
    ];
    if (canonicalJson(results) !== canonicalJson(expected)) throw new Error(`fact source containment differs:\n${JSON.stringify(results, null, 2)}`);
  } finally { await rm(base, { recursive: true, force: true }); }
}

async function checkReferenceExample() {
  const grammar = await readFile(join(repoRoot, "assets/base/agents/skills/cf-present/resources/figure-grammar.md"), "utf8");
  const section = grammar.slice(grammar.indexOf("## 6. Figure declaration"));
  const example = section.match(/```json\n([\s\S]*?)\n```/u)?.[1];
  if (example === undefined) throw new Error("figure-grammar.md section 6 has no JSON example");
  const declaration = JSON.parse(example);
  validateDeclaration(declaration, "figure-grammar.md section 6 example");
  renderFigure(declaration);
  await checkSourceContainment();
  const readSource = repositorySourceReader(repoRoot);
  const facts = checkFacts(declaration.figure, readSource);
  if (!facts.every((fact) => fact.matches)) throw new Error(`figure-grammar.md section 6 example facts do not derive: ${JSON.stringify(facts)}`);
  // The figure block the conversion section points authors at to copy.
  const review = JSON.parse(await readFile(join(repoRoot, "assets/base/agents/skills/cf-present/assets/review-document.example.json"), "utf8"));
  const converted = review.blocks.find((block) => block.type === "figure")?.declaration;
  validateDeclaration(converted, "review-document.example.json figure");
  renderFigure(converted);
  const convertedFacts = checkFacts(converted.figure, readSource);
  if (!convertedFacts.every((fact) => fact.matches)) throw new Error(`review-document.example.json figure facts do not derive: ${JSON.stringify(convertedFacts)}`);
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

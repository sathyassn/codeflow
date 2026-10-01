// Shared loading for the visual-doctrine control tests (TSK-062). The cases
// ship figure declarations in cf-evaluate-model fixtures; the controls are the
// faulty and passing answers a subject could leave, graded by the portal's own
// grammar module and configuration validator, not by a second implementation.
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = (relative) => new URL(relative, import.meta.url);
export const grammar = await import(here("../../docs-portal/scripts/figure-grammar.mjs"));

const resources = here("../../assets/base/agents/skills/cf-evaluate-model/resources/");
const json = async (url) => JSON.parse(await readFile(url, "utf8"));

export const fixtures = new Map((await json(new URL("fixtures.json", resources))).fixtures.map((fixture) => [fixture.id, fixture]));
export const cases = new Map((await json(new URL("cases.json", resources))).cases.map((entry) => [entry.id, entry]));
export const packs = new Map((await json(new URL("packs.json", resources))).packs.map((pack) => [pack.id, pack]));
export const controls = await json(here("./visual-controls/controls.json"));
export const methodControls = await json(here("./method-controls/controls.json"));

// The fixture a case ships and a reader over its files as the subject found
// them: facts always re-derive from the shipped bytes, never from a file a
// subject could have edited to match its drawing.
export function fixtureOf(caseId) {
  const fixture = fixtures.get(cases.get(caseId).fixture);
  return { fixture, readSource: (relative) => fixture.files[relative] ?? null };
}

export async function declarationOf(caseId, reference) {
  if (reference.startsWith("fixture:")) return JSON.parse(fixtureOf(caseId).fixture.files[reference.slice("fixture:".length)]);
  return json(here(`./visual-controls/${reference}`));
}

// Everything the gate reads except the render: validation, re-derived facts,
// the derived binding and both compositions.
export function prepare(declaration, readSource) {
  grammar.validateDeclaration(declaration);
  const facts = grammar.checkFacts(declaration.figure, readSource);
  const bound = grammar.bindDerivedData(declaration.figure, readSource);
  const composed = grammar.composeFigure(declaration, bound);
  const evidence = { facts, data: bound === null ? null : { drawn: composed.drawnValues, derived: bound.derived } };
  return { facts, bound, composed, evidence, html: grammar.renderFigure(declaration, { idPrefix: "c", bound }) };
}

// The adapter's layer choice (adapter.mjs chooseLayer), which the page route
// is built from: `${layer.id}/${localRouteFor(source)}`.
export function layerFor(sourcePath, layers) {
  return layers.find((layer) => (layer.paths ?? []).includes(sourcePath) || (layer.prefixes ?? []).some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`)))
    ?? layers.find((layer) => layer.fallback);
}

// A fixture as a subject would leave it: its files plus an answer's files and
// configuration edit, committed in a throwaway repository, then run through
// the portal's own adapter from the adopted root. The adapter refuses what it
// can prove wrong from committed inputs (a missing panel heading, an anchor
// that names no heading, a fact that does not re-derive); the browser build
// and its page class gate are not run here.
const ADAPTER = fileURLToPath(here("../../docs-portal/scripts/adapter.mjs"));
export async function adapt(files) {
  const root = await mkdtemp(path.join(os.tmpdir(), "cf-visual-control-"));
  try {
    for (const [relative, content] of Object.entries({ ".codeflow/project.toml": "schema_version = 1\n", ...files })) {
      await mkdir(path.dirname(path.join(root, relative)), { recursive: true });
      await writeFile(path.join(root, relative), content);
    }
    const git = (args) => {
      const result = spawnSync("git", ["-C", root, "-c", "user.email=controls@codeflow.invalid", "-c", "user.name=controls", "-c", "commit.gpgsign=false", ...args], { encoding: "utf8" });
      if (result.status !== 0) throw new Error(`git ${args[0]}: ${result.stderr}`);
    };
    git(["init", "-q"]);
    git(["add", "-A"]);
    git(["commit", "-q", "--no-verify", "-m", "fixture"]);
    const run = spawnSync(process.execPath, [ADAPTER], { cwd: path.join(root, "docs-portal"), encoding: "utf8" });
    const error = run.status === 0 ? null : (run.stderr.split("\n").find((line) => /^\w*Error: /.test(line)) ?? run.stderr.trim());
    const evidence = run.status === 0 ? JSON.parse(await readFile(path.join(root, "docs-portal/.portal/generated/evidence.json"), "utf8")) : null;
    const pages = {};
    if (evidence !== null) {
      for (const page of evidence.pages) pages[page.route] = await readFile(path.join(root, "docs-portal/src/content/docs", `${page.route}.md`), "utf8");
    }
    return { status: run.status, error, evidence, pages };
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

// The fixture files with an answer put where a subject would leave it: a
// replacement for a shipped draft, or a new declaration bound to a page.
export function place(fixtureFiles, declaration, placement, name) {
  const files = { ...fixtureFiles };
  const config = JSON.parse(files["docs-portal/portal.config.json"]);
  const target = placement.replaces ?? `docs/figures/${name}`;
  files[target] = `${JSON.stringify(declaration, null, 2)}\n`;
  if (placement.bind) config.figures = [...config.figures, { declaration: target, ...placement.bind }];
  if (placement.page_class) {
    const source = config.layers.flatMap((layer) => layer.paths ?? []).find((page) => placement.bind.route.endsWith(page.replace(/^docs\//, "").replace(/\.md$/, "")));
    config.page_classes = [...config.page_classes, { source, class: placement.page_class }];
  }
  files["docs-portal/portal.config.json"] = `${JSON.stringify(config, null, 2)}\n`;
  return { files, target };
}

// A method answer laid over its case's fixture: a directory answer's docs/
// files replace or add files, and its config.json adds figure bindings and
// page classes. A present document answer carries its figures in blocks.
async function tree(directory, prefix = "") {
  const out = {};
  for (const name of await readdir(directory)) {
    const full = path.join(directory, name);
    if ((await stat(full)).isDirectory()) Object.assign(out, await tree(full, `${prefix}${name}/`));
    else out[`${prefix}${name}`] = await readFile(full);
  }
  return out;
}

export async function methodAnswer(caseId, entry) {
  const location = here(`./method-controls/${entry.answer}`);
  const { fixture } = fixtureOf(caseId);
  if (entry.answer.endsWith(".json")) {
    const document = await json(location);
    const figures = document.blocks.filter((block) => block.type === "figure").map((block) => block.declaration);
    return { files: fixture.files, figures, readSource: (relative) => fixture.files[relative] ?? null };
  }
  const answer = await tree(fileURLToPath(location));
  const edit = JSON.parse(answer["config.json"]);
  const files = { ...fixture.files };
  for (const [relative, bytes] of Object.entries(answer)) if (relative.startsWith("docs/")) files[relative] = bytes;
  const config = JSON.parse(files["docs-portal/portal.config.json"]);
  config.figures = [...config.figures, ...edit.figures];
  config.page_classes = [...config.page_classes, ...edit.page_classes];
  files["docs-portal/portal.config.json"] = `${JSON.stringify(config, null, 2)}\n`;
  const figures = edit.figures.map((binding) => JSON.parse(String(files[binding.declaration])));
  const readSource = (relative) => (files[relative] === undefined ? null : String(files[relative]));
  return { files, config, figures, readSource };
}

// What the class gate observes in a panel, read from the adapted page rather
// than a built one: each altitude section's grammar figures, cf-stage
// figures, tables, list items and code blocks, and the page-class marker the
// adapter writes. The utility chrome (heading, Display control, no Comment)
// is the runtime's and is taken as present; the browser build that observes
// it is not run here.
export function observeAdaptedPage(text, route) {
  const panelCarriers = { concept: null, architecture: null, technical: null };
  for (const match of text.matchAll(/<section class="portal-altitude"[^>]*data-altitude="(\w+)">([\s\S]*?)<\/section>/g)) {
    const body = match[2];
    const count = (pattern) => (body.match(pattern) ?? []).length;
    panelCarriers[match[1]] = {
      figure: count(/<figure class="cf-fig"/g), stage: count(/<figure class="portal-stage"/g),
      table: count(/^\|[\s:|-]+\|$/gm), list: count(/^(?:[-*+]|\d{1,9}[.)])\s/gm), pre: count(/^```/gm) / 2,
    };
  }
  return {
    route, pageClassMarker: text.match(/data-cf-page-class="([^"]*)"/)?.[1] ?? null, headings: 1, displayControls: 1, commentChrome: 0, provenance: text.includes('class="portal-provenance"'),
    altitudePanels: Object.keys(panelCarriers).filter((panel) => panelCarriers[panel] !== null), panelCarriers,
    pointerColumns: [], pointerRows: 0, companions: (text.match(/data-cf-companion=/g) ?? []).length,
  };
}

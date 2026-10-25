// The present type scale and page head on the real render (TSK-444): the
// `codeflow` binary validates and exports a document with a head, sections,
// subsections and every titled block, and headless Chrome reads the computed
// sizes the cascade gives each element, in light and dark at the compact,
// default and large size steps. The document title must be larger than every
// section, a section than every subsection, and a subsection than every block
// title (comparison column, decision, form, callout, tree, figure and frame
// titles); the summary and every section lead take the lede size. A later
// rule that lifts a block title to display size is injected as a negative
// control, and the check must catch it.
//
// Usage: node scripts/type-scale-browser-check.mjs [--styles-from <git revision>]
// With --styles-from, the export's stylesheet is replaced by that revision's
// styles.css and figure.css, so the same check shows a failure before the
// change.
import { execFileSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { installedChrome } from "./browser-executables.mjs";
import { codeflowBinary } from "./codeflow-binary.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "../../..");
const codeflow = codeflowBinary(repoRoot);
const stylesIndex = process.argv.indexOf("--styles-from");
const stylesFrom = stylesIndex > 0 ? process.argv[stylesIndex + 1] : null;
const SCALES = ["compact", "default", "large"];
const S2_DEFAULT = { title: 34, lede: 19, h2: 24, h3: 18, block: 15 };
// Figure line, two comparison columns, callout, tree, table frame and decision.
const BLOCK_TITLES = 7;
const MUTATION = "#cf-present-document .comparison h2 { font-size: var(--cf-fs-display); }";

const toned = JSON.parse(await readFile(join(repoRoot, "docs-portal/tests/fixtures/figures/controls/layering-toned.json"), "utf8"));
const DOCUMENT = {
  schema_version: 2,
  title: "Type scale reference",
  summary: "A head, sections with leads, subsections and every titled block, to measure the scale a reader sees.",
  language: "en",
  provenance: { task_id: "TSK-444", adr_id: "ADR-0053" },
  blocks: [
    { type: "narrative", id: "open", markdown: "## A section\n\nThe section lead.\n\nBody text after the lead.\n\n### A subsection\n\nBody text in the subsection." },
    { type: "figure", id: "planes", declaration: toned },
    { type: "comparison", id: "options", columns: [{ title: "First option", markdown: "One way." }, { title: "Second option", markdown: "Another way." }] },
    { type: "callout", id: "note", tone: "note", title: "A callout title", markdown: "A note." },
    { type: "tree", id: "files", label: "A tree title", nodes: [{ label: "src", children: [{ label: "main.rs" }] }] },
    { type: "table", id: "sizes", title: "A table title", columns: ["Role", "Size"], rows: [["Section", "24 px"]] },
    { type: "decision", id: "choice", title: "A decision title", markdown: "Pick one.", options: [{ value: "a", label: "A" }, { value: "b", label: "B" }] },
    { type: "narrative", id: "close", markdown: "## Another section\n\nAnother lead.\n\n### Another subsection\n\nMore body text." },
  ],
};

const MEASURE = () => {
  const root = document.getElementById("cf-present-document");
  const sizes = (selector) => [...root.querySelectorAll(selector)].filter((node) => node.getClientRects().length).map((node) => ({ text: node.textContent.trim().slice(0, 40), size: parseFloat(getComputedStyle(node).fontSize) }));
  return {
    title: sizes(".cf-doc-head .cf-doc-title"),
    summary: sizes(".cf-doc-head .cf-doc-summary"),
    h2: sizes(".block--narrative h2"),
    lead: sizes(".block--narrative h2 + p"),
    h3: sizes(".block--narrative h3"),
    blockTitles: sizes(".comparison h2, .decision h2, .cf-form__title, .callout > h2, .block--tree h2, .cf-fig-title, .cf-frame-title"),
  };
};

function violations(m) {
  const found = [];
  const one = (list, name) => { if (list.length === 0) found.push(`no ${name} measured`); return list.map((entry) => entry.size); };
  const [title] = one(m.title, "title");
  const h2 = one(m.h2, "section");
  const h3 = one(m.h3, "subsection");
  const blocks = one(m.blockTitles, "block title");
  if (m.blockTitles.length !== BLOCK_TITLES) found.push(`measured ${m.blockTitles.length} block titles, the document has ${BLOCK_TITLES}`);
  if (found.length) return found;
  if (!(title > Math.max(...h2))) found.push(`title ${title} is not above section ${Math.max(...h2)}`);
  if (!(Math.min(...h2) > Math.max(...h3))) found.push(`section ${Math.min(...h2)} is not above subsection ${Math.max(...h3)}`);
  const top = m.blockTitles.reduce((a, b) => (b.size > a.size ? b : a));
  if (!(Math.min(...h3) > top.size)) found.push(`subsection ${Math.min(...h3)} is not above block title "${top.text}" ${top.size}`);
  const lede = m.summary[0]?.size;
  if (lede === undefined) found.push("no summary measured");
  else if (m.lead.length < 2 || m.lead.some((entry) => entry.size !== lede)) found.push(`section leads ${JSON.stringify(m.lead.map((entry) => entry.size))} do not take the lede size ${lede}`);
  return found;
}

await access(codeflow);
const root = await mkdtemp(join(tmpdir(), "cf-present-type-scale-"));
const project = join(root, "project");
const environment = {
  PATH: process.env.PATH ?? "",
  HOME: join(root, "home"),
  TMPDIR: join(root, "tmp"),
  XDG_STATE_HOME: join(root, "state"),
  LANG: "C.UTF-8",
  CF_PRESENT_OWNER_PID: String(process.pid),
};
const run = (args) => execFileSync(codeflow, args, { cwd: project, env: environment, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 30_000 });
let sessionId = null;
let browser = null;
const failures = [];
try {
  for (const directory of [project, environment.HOME, environment.TMPDIR, environment.XDG_STATE_HOME]) await mkdir(directory, { recursive: true });
  execFileSync("git", ["init", "--quiet"], { cwd: project, env: environment });
  const documentPath = join(project, "type-scale.json");
  await writeFile(documentPath, `${JSON.stringify(DOCUMENT, null, 2)}\n`);
  const opened = run(["present", "open", documentPath, "--no-launch"]);
  sessionId = opened.match(/session ([0-9a-f-]+) ready/u)?.[1] ?? null;
  if (sessionId === null) throw new Error(`could not parse present open output: ${opened}`);

  const replacement = stylesFrom === null ? null : ["styles.css", "figure.css"].map((name) => execFileSync("git", ["show", `${stylesFrom}:crates/codeflow-present/web/src/${name}`], { cwd: repoRoot, encoding: "utf8" })).join("\n");
  browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  for (const mode of ["light", "dark"]) {
    const exported = join(root, `type-scale-${mode}.html`);
    run(["present", "export", sessionId, "--out", exported, "--mode", mode]);
    await page.goto(pathToFileURL(exported).href);
    await page.waitForFunction(() => [...document.querySelectorAll("[data-cf-figure-block]")].every((block) => block.getAttribute("data-cf-figure-block") === "ready"), null, { timeout: 30_000 });
    if (replacement !== null) await page.evaluate((css) => { document.querySelector("style[data-cf-present-export-style]").textContent = css; }, replacement);
    for (const scale of SCALES) {
      await page.evaluate((step) => { document.documentElement.dataset.cfScale = step; }, scale);
      const measured = await page.evaluate(MEASURE);
      const found = violations(measured);
      if (scale === "default" && found.length === 0) {
        const got = { title: measured.title[0].size, lede: measured.summary[0].size, h2: measured.h2[0].size, h3: measured.h3[0].size, block: Math.max(...measured.blockTitles.map((entry) => entry.size)) };
        if (JSON.stringify(got) !== JSON.stringify(S2_DEFAULT)) found.push(`default sizes ${JSON.stringify(got)} are not S2 ${JSON.stringify(S2_DEFAULT)}`);
      }
      process.stdout.write(`${mode} ${scale}: title ${measured.title.map((e) => e.size)} summary ${measured.summary.map((e) => e.size)} h2 ${[...new Set(measured.h2.map((e) => e.size))]} lead ${[...new Set(measured.lead.map((e) => e.size))]} h3 ${[...new Set(measured.h3.map((e) => e.size))]} block titles ${[...new Set(measured.blockTitles.map((e) => e.size))]} (${measured.blockTitles.map((e) => e.text.replace(/\s+/g, " ").slice(0, 18)).join(" / ")})${found.length ? ` FAIL: ${found.join("; ")}` : ""}\n`);
      failures.push(...found.map((message) => `${mode} ${scale}: ${message}`));
    }
    // Negative control: the cascade, not the stylesheet text, decides.
    await page.evaluate(() => { document.documentElement.dataset.cfScale = "default"; });
    await page.addStyleTag({ content: MUTATION });
    const mutated = violations(await page.evaluate(MEASURE));
    if (!mutated.some((message) => message.startsWith("subsection"))) failures.push(`${mode}: the cascade control (${MUTATION}) was not caught`);
    else process.stdout.write(`${mode} control caught: ${mutated.join("; ")}\n`);
  }
} finally {
  await browser?.close();
  if (sessionId !== null) {
    try { run(["present", "close", sessionId]); } catch { /* the service exits with this process */ }
  }
  await rm(root, { recursive: true, force: true });
}
if (failures.length) {
  process.stderr.write(`cf-present type scale check failed:\n  ${failures.join("\n  ")}\n`);
  process.exit(1);
}
process.stdout.write("cf-present type scale checks passed: title > section > subsection > every block title, summary and leads at the lede size, in light and dark at compact, default and large, and the cascade control caught\n");

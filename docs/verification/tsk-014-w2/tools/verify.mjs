// Integrity checks for the TSK-014 W2 study. Deterministic only: a clean run
// says the study is internally consistent, never that a composition reads.
// G1-G8 remain not run — see observer-state.md.
//
//   node tools/verify.mjs            compare against SHA256SUMS, fail on drift
//   node tools/verify.mjs --update   regenerate SHA256SUMS (explicit only)

import { createHash } from "node:crypto";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const studyRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// CF_W2_REPO_ROOT exists so tools/selftest.mjs can verify a disposable copy of
// the study against the real repository. Ordinary runs never set it.
const repoRoot = process.env.CF_W2_REPO_ROOT
  ? resolve(process.env.CF_W2_REPO_ROOT)
  : resolve(studyRoot, "../../..");
const updating = process.argv.includes("--update");
const failures = [];
const notes = [];
const check = (ok, message) => { if (!ok) failures.push(message); };
const read = (rel) => readFile(join(studyRoot, rel), "utf8");
const normalise = (text) => text.replace(/\s+/g, " ").trim();

const P1 = ["cases/p1/a-wave-lanes/index.html", "cases/p1/b-blocking-matrix/index.html", "cases/p1/c-critical-ribbon/index.html"];
const P2 = ["cases/p2/a-evidence-grid/index.html", "cases/p2/b-provenance-rail/index.html"];
const CANDIDATES = [...P1, ...P2];
const BASELINES = ["baselines/p1/baseline.html", "baselines/p2/baseline.html"];
const DOCS = ["README.md", "rubric.md", "rubric.lock.json", "content-inventory.md", "answer-key.md",
  "observer-state.md", "NEXT.md", "board.html"];

// ------------------------------------------------- 1. rubric pre-registration
const lock = JSON.parse(await read("rubric.lock.json"));
const rubricDigest = createHash("sha256").update(await readFile(join(studyRoot, "rubric.md"))).digest("hex");
check(rubricDigest === lock.sha256,
  `rubric.md no longer matches its lock (${rubricDigest} vs ${lock.sha256}); a changed gate needs a new dated rubric, not an edit`);
notes.push(`rubric lock: bytes match ${lock.sha256.slice(0, 12)}…, ordering evidence recorded as ${lock.ordering_evidence.method}`);

// ------------------------------------------- 2. P1 dataset + derived answers
const stripComment = (v) => v.split(" #")[0].trim().replace(/^"|"$/g, "");
const parseList = (v) => {
  const inner = stripComment(v).replace(/^\[|\]$/g, "").trim();
  return inner ? inner.split(",").map((x) => x.trim()).filter(Boolean) : [];
};
const repoTasks = new Map();
for (const id of ["005", "006", "007", "008", "009", "010", "011", "012", "013", "014"]) {
  const fm = (await readFile(join(repoRoot, `project-management/tasks/TSK-${id}.md`), "utf8")).split("---")[1];
  const field = (key) => (fm.match(new RegExp(`^${key}:\\s*(.*)$`, "m")) ?? ["", ""])[1];
  repoTasks.set(`TSK-${id}`, {
    status: stripComment(field("status")), deps: parseList(field("depends_on")),
    specs: parseList(field("specs")), target: stripComment(field("integration_target")),
  });
}

const datasets = [];
for (const rel of P1) {
  const raw = (await read(rel)).match(/<script type="application\/json" id="plan-data">([\s\S]*?)<\/script>/);
  if (!raw) { failures.push(`${rel}: no embedded plan dataset`); continue; }
  datasets.push({ rel, raw: raw[1], data: JSON.parse(raw[1]) });
}
if (datasets.length === P1.length) {
  datasets.slice(1).forEach((d) =>
    check(d.raw === datasets[0].raw, `${d.rel}: dataset is not byte-identical to ${datasets[0].rel}`));
  for (const task of datasets[0].data.tasks) {
    const repo = repoTasks.get(task.id);
    if (!repo) { failures.push(`${task.id} is not a real EPC-005 task`); continue; }
    check(task.status === repo.status, `${task.id}: status drifted from frontmatter`);
    check(task.deps.join(",") === repo.deps.join(","), `${task.id}: depends_on drifted from frontmatter`);
    check(task.specs.join(",") === repo.specs.join(","), `${task.id}: specs drifted from frontmatter`);
    check(task.target === repo.target, `${task.id}: integration_target drifted from frontmatter`);
  }
  check(datasets[0].data.tasks.length === repoTasks.size, "the dataset does not cover every EPC-005 task");
  notes.push(`P1 dataset: ${datasets[0].data.tasks.length} tasks verified against frontmatter, byte-identical across ${datasets.length} candidates`);
}

// The expected answer is recomputed here from the repository, independently of
// the shared model the candidates use.
const doneT = (id) => ["complete", "cancelled"].includes(repoTasks.get(id).status);
const openIds = [...repoTasks.keys()].filter((id) => !doneT(id));
const waveMemo = new Map();
const waveOf = (id) => {
  if (waveMemo.has(id)) return waveMemo.get(id);
  const blockers = repoTasks.get(id).deps.filter((d) => !doneT(d));
  const value = blockers.length ? 1 + Math.max(...blockers.map(waveOf)) : 1;
  waveMemo.set(id, value); return value;
};
openIds.forEach(waveOf);
const maxWave = Math.max(...openIds.map(waveOf));
const succ = (id) => openIds.filter((o) => repoTasks.get(o).deps.includes(id));
const longMemo = new Map();
const longestToSink = (id) => {
  if (longMemo.has(id)) return longMemo.get(id);
  const next = succ(id);
  const value = next.length ? 1 + Math.max(...next.map(longestToSink)) : 0;
  longMemo.set(id, value); return value;
};
openIds.forEach(longestToSink);
const expectedStartable = openIds.filter((id) => repoTasks.get(id).deps.every(doneT));
const expectedRoom = Object.fromEntries(openIds
  .map((id) => [id, (maxWave - longestToSink(id)) - waveOf(id)])
  .filter(([, slack]) => slack > 0));
let head = openIds.reduce((best, id) => (longestToSink(id) > longestToSink(best) ? id : best), openIds[0]);
const expectedChain = [head];
while (succ(head).length) {
  head = succ(head).reduce((best, s) => (longestToSink(s) > longestToSink(best) ? s : best));
  expectedChain.push(head);
}
const expected = { startable: expectedStartable, chain: expectedChain, room: expectedRoom };

const answersFile = JSON.parse(await read("checks/answers.json"));
const answered = new Set();
for (const entry of answersFile.answers) {
  answered.add(entry.page);
  check(JSON.stringify(entry.answer) === JSON.stringify(expected),
    `${entry.render}: published answer disagrees with the answer recomputed from frontmatter`);
}
P1.forEach((rel) => check(answered.has(rel), `${rel}: published no derived answer at runtime`));
notes.push(`derived answer agreed by all P1 candidates: startable ${expected.startable.join(", ")}; ` +
  `longest chain ${expected.chain.join(" → ")}; room ${Object.entries(expected.room).map(([k, v]) => `${k}:${v}`).join(", ")}`);

// -------------------------------------------------- 3. P2 fact registry bind
const facts = await read("shared/tsk007-facts.js");
for (const rel of P2) {
  check((await read(rel)).includes("shared/tsk007-facts.js"), `${rel}: does not render from the shared fact registry`);
}
const sourceMatch = facts.match(/source:\s*"([^"]+)"/);
check(Boolean(sourceMatch), "the fact registry declares no source path");
const sourceText = normalise(await readFile(join(repoRoot, sourceMatch[1]), "utf8"));
const quotes = [...facts.matchAll(/quotes?:\s*(?:\[([\s\S]*?)\]|"((?:[^"\\]|\\.)*)")/g)].flatMap((m) =>
  m[1] ? [...m[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((q) => q[1]) : [m[2]]);
check(quotes.length >= 15, `the fact registry carries only ${quotes.length} verbatim quotations`);
let missing = 0;
for (const quote of quotes) {
  if (!sourceText.includes(normalise(quote.replace(/\\"/g, '"')))) {
    failures.push(`quotation absent from ${sourceMatch[1]}: “${quote.slice(0, 60)}…”`);
    missing += 1;
  }
}
check(facts.includes("826715510440df53fc999e68d77429d7b49a6d55"), "the registry does not pin the exact candidate");
check(sourceText.includes("826715510440df53fc999e68d77429d7b49a6d55"), "the exact candidate is not in the source record");
notes.push(`P2 registry: ${quotes.length} quotations checked verbatim against ${sourceMatch[1]}, ${missing} missing`);

// ------------------------------------------------------- 4. source contracts
const motionless = [];
for (const rel of [...CANDIDATES, ...BASELINES, "board.html"]) {
  const html = await read(rel);
  check(/<html lang="/.test(html), `${rel}: missing lang`);
  check(/<title>/.test(html), `${rel}: missing title`);
  check(/<h1[ >]/.test(html), `${rel}: missing h1`);
  check(/name="viewport"/.test(html), `${rel}: missing viewport meta`);
  check(!/(https?:)?\/\/(?!www\.w3\.org\/2000\/svg)[a-z0-9]/i.test(html), `${rel}: contains a remote URL`);
  check(!/<script[^>]+src=["'](?!\.)/.test(html), `${rel}: loads a non-relative script`);
  for (const src of [...html.matchAll(/<script[^>]+src=["']([^"']+)["']/g)].map((m) => m[1])) {
    await stat(resolve(studyRoot, dirname(rel), src))
      .catch(() => failures.push(`${rel}: script ${src} does not resolve inside the study`));
  }
  check(!/<link[^>]+href=/.test(html), `${rel}: loads an external stylesheet`);
  check(!/\b(fetch|XMLHttpRequest|WebSocket)\s*\(/.test(html), `${rel}: contains a network call`);
  if (CANDIDATES.includes(rel)) {
    check(/prefers-color-scheme: dark/.test(html), `${rel}: no dark-mode handling`);
    const hasMotion = /(transition|animation|scroll-behavior):/.test(html);
    check(!hasMotion || /prefers-reduced-motion/.test(html), `${rel}: motion is not gated by prefers-reduced-motion`);
    if (!hasMotion) motionless.push(rel);
    check(/@media \(max-width/.test(html), `${rel}: no narrow-viewport adaptation`);
  }
}
notes.push(`motion: ${motionless.length} candidate page(s) declare none; the rest gate it behind prefers-reduced-motion`);

// ------------------------------------------------- 5. render + check coverage
const slugOf = (page) => page.replace(/^cases\//, "").replace(/\/index\.html$/, "")
  .replace(/^baselines\//, "").replace(/\/baseline\.html$/, "-baseline").replace(/\//g, "-");
const expectedRenders = new Set([
  ...CANDIDATES.flatMap((p) => [
    ...["light", "dark"].flatMap((m) => ["desktop", "mobile"].map((v) => `${slugOf(p)}-${m}-${v}.png`)),
    `${slugOf(p)}-light-desktop-reduced.png`]),
  ...BASELINES.flatMap((p) => ["desktop", "mobile"].map((v) => `${slugOf(p)}-light-${v}.png`)),
]);
const actualRenders = new Set((await readdir(join(studyRoot, "renders"))).filter((n) => n !== ".DS_Store"));
for (const name of expectedRenders) check(actualRenders.has(name), `missing render ${name}`);
for (const name of actualRenders) check(expectedRenders.has(name), `unexpected render present: ${name}`);
const expectedChecks = new Set(["axe.json", "network.json", "answers.json", "lifecycle.json"]);
const actualChecks = new Set(await readdir(join(studyRoot, "checks")));
for (const name of expectedChecks) check(actualChecks.has(name), `missing check record ${name}`);
for (const name of actualChecks) check(expectedChecks.has(name), `unexpected check record present: ${name}`);
notes.push(`renders: ${actualRenders.size} present, ${expectedRenders.size} expected, none unexpected`);

// -------------------------------------------------------- 6. machine evidence
const axeReport = JSON.parse(await read("checks/axe.json"));
const violating = axeReport.results.filter((r) => r.violations.length);
check(violating.length === 0, `axe violations in ${violating.map((r) => r.render).join(", ")}`);
const network = JSON.parse(await read("checks/network.json"));
check(network.remote_requests.length === 0, `${network.remote_requests.length} non-file requests were attempted`);
check(network.outside_study_file_requests.length === 0,
  `${network.outside_study_file_requests?.length} file request(s) outside the study root were attempted`);
check(network.page_errors.length === 0, `${network.page_errors.length} console or page error(s) were observed`);
check(network.study_root === studyRoot, "the recorded file: containment root is not this study");
const lifecycle = JSON.parse(await read("checks/lifecycle.json"));
check(lifecycle.close_fallback_used === false, "the render needed the exact-owned termination fallback");
check(lifecycle.surviving_process_identities === 0, "task-owned browser processes survived the render");
check(lifecycle.cleanup_proven === true, "task-owned process exit was not positively proven");
check(lifecycle.task_root_removed === true, "the marked task root was not removed");
check(lifecycle.task_root_state === "absent", `the marked task root is ${lifecycle.task_root_state}`);
check(lifecycle.owned_process_identities > 0, "no task-owned browser process was ever identified");
check(lifecycle.xdg_runtime_dir_redirected === true, "XDG_RUNTIME_DIR was not redirected into the task root");
check((lifecycle.cleanup_errors ?? []).length === 0, `cleanup errors were recorded: ${(lifecycle.cleanup_errors ?? []).join("; ")}`);
check(lifecycle.survivor_injected === false && lifecycle.inventory_failure_injected === false,
  "the recorded run was a self-test injection, not a normal render");
notes.push(`lifecycle: ${lifecycle.owned_process_identities} owned identities, 0 survivors, cleanup proven, root absent, XDG redirected`);
notes.push(`axe: ${axeReport.results.length} runs clean on Chrome ${axeReport.browser_version}, playwright-core ${axeReport.playwright_core_version}, axe-core ${axeReport.axe_core_version}`);

// ------------------------------------------------------------- 7. checksums
const inventory = [];
const walk = async (dir) => {
  for (const entry of (await readdir(join(studyRoot, dir), { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const rel = `${dir}/${entry.name}`;
    if (entry.isDirectory()) await walk(rel);
    else inventory.push(rel);
  }
};
for (const dir of ["baselines", "cases", "checks", "renders", "shared", "tools"]) await walk(dir);
inventory.push(...DOCS);
inventory.sort();
const lines = [];
for (const rel of inventory) {
  lines.push(`${createHash("sha256").update(await readFile(join(studyRoot, rel))).digest("hex")}  ${rel}`);
}
if (updating) {
  // The trusted inventory is never refreshed for an invalid state: every
  // non-checksum check above must have passed first.
  if (failures.length) {
    notes.forEach((n) => process.stdout.write(`ok   ${n}\n`));
    failures.forEach((f) => process.stdout.write(`FAIL ${f}\n`));
    process.stdout.write(`\nrefusing to refresh SHA256SUMS: ${failures.length} check(s) failed and the recorded ` +
      `inventory is left byte-identical\n`);
    process.exit(1);
  }
  await writeFile(join(studyRoot, "SHA256SUMS"), `${lines.join("\n")}\n`);
  notes.push(`SHA256SUMS rewritten for ${lines.length} files (--update, after all other checks passed)`);
} else {
  const recorded = new Map((await read("SHA256SUMS").catch(() => "")).split("\n").filter(Boolean)
    .map((line) => [line.slice(66), line.slice(0, 64)]));
  const current = new Map(lines.map((line) => [line.slice(66), line.slice(0, 64)]));
  check(recorded.size > 0, "SHA256SUMS is missing — run tools/verify.mjs --update to create it");
  for (const [rel, digest] of current) {
    if (!recorded.has(rel)) failures.push(`file not in SHA256SUMS: ${rel}`);
    else if (recorded.get(rel) !== digest) failures.push(`digest drift: ${rel}`);
  }
  for (const rel of recorded.keys()) if (!current.has(rel)) failures.push(`file recorded in SHA256SUMS is missing: ${rel}`);
  if (!failures.length) notes.push(`SHA256SUMS: ${current.size} files match`);
}

// ------------------------------------------------------------------ report
notes.forEach((n) => process.stdout.write(`ok   ${n}\n`));
failures.forEach((f) => process.stdout.write(`FAIL ${f}\n`));
process.stdout.write(failures.length
  ? `\n${failures.length} check(s) failed\n`
  : `\nall W2 deterministic checks passed — G1-G8 remain not run (observer-state.md)\n`);
process.exitCode = failures.length ? 1 : 0;

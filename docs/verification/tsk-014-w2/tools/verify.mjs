// Integrity checks for the TSK-014 W2 study. Deterministic only: a clean run
// says the study is internally consistent, never that a composition reads.
// G1-G8 remain not run — see observer-state.md.
//
//   node tools/verify.mjs            compare against SHA256SUMS, fail on drift
//   node tools/verify.mjs --update   regenerate SHA256SUMS (explicit only)

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readdir, readFile, realpath, stat, writeFile } from "node:fs/promises";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isContained } from "./path-containment.mjs";
import {
  evaluateSharedSource, isQualifiedRevision, literalPathspec, readInsideRoot,
} from "./source-authority.mjs";

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
const P3 = ["cases/p3/a-finding-anchored-delta/index.html", "cases/p3/b-convergence-ledger/index.html"];
const D1 = ["cases/d1/a-concept-dependency-path/index.html", "cases/d1/b-task-first-entry/index.html",
  "cases/d1/c-contract-map/index.html"];
const D2 = ["cases/d2/a-chain-in-place/index.html", "cases/d2/b-evidence-adjacent-margin/index.html"];
const CANDIDATES = [...P1, ...P2, ...P3, ...D1, ...D2];
const BASELINES = ["baselines/p1/baseline.html", "baselines/p2/baseline.html", "baselines/p3/baseline.html",
  "baselines/d1/baseline.html", "baselines/d2/baseline.html"];
const DOCS = ["README.md", "rubric.md", "rubric.lock.json", "content-inventory.md", "answer-key.md",
  "observer-state.md", "NEXT.md", "board.html"];

// Cases whose siblings render from one shared subject source rather than from a
// dataset embedded in each page. Drift is then impossible by construction; the
// check is that every sibling really loads it.
const SHARED_SOURCE = {
  p2: { file: "shared/tsk007-facts.js", pages: P2 },
  p3: { file: "shared/p3-convergence.js", pages: P3 },
  d1: { file: "shared/repo-entry.js", pages: D1 },
  d2: { file: "shared/record-authority.js", pages: D2 },
};

// A shared subject source is repository-controlled data, not trusted code. It
// is evaluated in a bounded node:vm context with no Node globals and code
// generation disabled (tools/source-authority.mjs), and only a JSON snapshot
// taken inside that same context crosses back. So nothing it defines ever runs
// here, no host property access can re-enter it, and every check below
// recomputes from its raw fact arrays rather than calling anything it exports.
const loadShared = async (rel, global) => {
  const loaded = evaluateSharedSource(await read(rel), global, { filename: rel });
  if (!loaded.ok) { failures.push(`${rel}: ${loaded.reason}`); return null; }
  return loaded.value;
};
const normQuote = (q) => normalise(q.replace(/\\"/g, '"'));

// Which root a declared source belongs to is explicit. Everything a shared
// source names is a repository path unless it is one of the study's own records
// listed here. There is no fallback: a repository claim can never be satisfied
// by a file this study wrote, a study record can never be satisfied by a
// repository file, and no read error degrades to empty content that a quotation
// check would then simply not find.
const STUDY_OWNED_SOURCES = new Set(["observer-state.md"]);
const sourceCache = new Map();
const sourceContent = async (rel) => {
  const key = typeof rel === "string" ? rel : JSON.stringify(rel);
  if (!sourceCache.has(key)) {
    const owner = STUDY_OWNED_SOURCES.has(key) ? "study" : "repository";
    const result = await readInsideRoot(owner === "study" ? studyRoot : repoRoot, rel);
    sourceCache.set(key, result.ok
      ? { ok: true, raw: result.raw, text: normalise(result.raw) }
      : { ok: false, reason: `declared ${owner} source ${key} ${result.reason}` });
  }
  return sourceCache.get(key);
};
const checkQuote = async (label, quote) => {
  if (!quote) return;
  if (typeof quote.text !== "string" || quote.text.trim() === "") {
    failures.push(`${label}: declares a quotation carrying no text`);
    return;
  }
  const source = await sourceContent(quote.source);
  if (!source.ok) { failures.push(`${label}: ${source.reason}`); return; }
  if (!source.text.includes(normQuote(quote.text))) {
    failures.push(`${label}: quotation absent from ${quote.source}: “${quote.text.slice(0, 60)}…”`);
  }
};

// Only these literal flags may reach git. Every other argument is a value a
// shared source controls, and has already been qualified — an object id, or a
// pathspec pinned to `:(literal,top)` so it names one path and cannot glob,
// exclude or re-root. An option-like value is refused here rather than becoming
// an option: `--output=<file>` alone would make `git log` write a file.
const GIT_LITERALS = new Set(["log", "show", "-1", "--format=%h%n%s%n%ad", "--date=short", "--no-ext-diff", "--"]);
const git = (args) => {
  for (const arg of args) {
    if (typeof arg !== "string") throw new Error(`git argument is not a string: ${String(arg)}`);
    if (arg.startsWith("-") && !GIT_LITERALS.has(arg)) {
      throw new Error(`refusing to pass an option-like git argument: ${arg}`);
    }
  }
  return execFileSync("git", ["-C", repoRoot, ...args], { encoding: "utf8", maxBuffer: 48 * 1024 * 1024 });
};

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
// Populated per case; every listed page must publish exactly this at runtime.
const expectedByCase = { p1: { pages: P1, answer: expected } };
notes.push(`derived answer expected of every P1 candidate: startable ${expected.startable.join(", ")}; ` +
  `longest chain ${expected.chain.join(" → ")}; room ${Object.entries(expected.room).map(([k, v]) => `${k}:${v}`).join(", ")}`);

// -------------------------------------------------- 3. P2 fact registry bind
const facts = await read("shared/tsk007-facts.js");
for (const [caseId, { file, pages }] of Object.entries(SHARED_SOURCE)) {
  for (const rel of pages) {
    check((await read(rel)).includes(file), `${rel}: does not render from the shared ${caseId} subject source ${file}`);
  }
}
const sourceMatch = facts.match(/source:\s*"([^"]+)"/);
check(Boolean(sourceMatch), "the fact registry declares no source path");
// The registry's declared source goes through the same authority contract as
// every other declared path: one named root, canonical, a regular file.
const p2Source = sourceMatch ? await sourceContent(sourceMatch[1]) : { ok: false, reason: "is absent" };
if (!p2Source.ok) failures.push(`the P2 fact registry's ${p2Source.reason}`);
const sourceText = p2Source.ok ? p2Source.text : "";
const quotes = [...facts.matchAll(/quotes?:\s*(?:\[([\s\S]*?)\]|"((?:[^"\\]|\\.)*)")/g)].flatMap((m) =>
  m[1] ? [...m[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((q) => q[1]) : [m[2]]);
check(quotes.length >= 15, `the fact registry carries only ${quotes.length} verbatim quotations`);
let missing = 0;
if (p2Source.ok) {
  for (const quote of quotes) {
    if (!sourceText.includes(normalise(quote.replace(/\\"/g, '"')))) {
      failures.push(`quotation absent from ${sourceMatch[1]}: “${quote.slice(0, 60)}…”`);
      missing += 1;
    }
  }
  check(sourceText.includes("826715510440df53fc999e68d77429d7b49a6d55"), "the exact candidate is not in the source record");
}
check(facts.includes("826715510440df53fc999e68d77429d7b49a6d55"), "the registry does not pin the exact candidate");
notes.push(`P2 registry: ${quotes.length} quotations checked verbatim against ${sourceMatch?.[1]}, ${missing} missing`);

// ------------------------------- 3b. P3 convergence bound to real git history
const p3 = await loadShared("shared/p3-convergence.js", "p3Convergence");
if (p3) {
  const STATES = new Set(["opened", "reopened", "closed", "extended", "residual"]);
  let hunkLines = 0;
  let p3Quotes = 0;
  for (const round of p3.rounds) {
    // A revision reaches a git argument only as an object id. A ref name, a
    // range or anything option-shaped is refused before git sees it.
    if (!isQualifiedRevision(round.sha)) {
      failures.push(`${round.id}: “${round.sha}” is not an abbreviated or full object id, so it is not read as a revision`);
      continue;
    }
    let log;
    try { log = git(["log", "-1", "--format=%h%n%s%n%ad", "--date=short", round.sha, "--"]).trim().split("\n"); }
    catch (error) { failures.push(`${round.id}: commit ${round.sha} is not in this repository (${error.message.split("\n")[0]})`); continue; }
    check(log[0] === round.sha, `${round.id}: abbreviated sha is ${log[0]}, recorded as ${round.sha}`);
    check(log[1] === round.subject, `${round.id}: subject is “${log[1]}”, recorded as “${round.subject}”`);
    check(log[2] === round.date, `${round.id}: date is ${log[2]}, recorded as ${round.date}`);
  }
  for (const finding of p3.findings) {
    check(finding.cells.length > 0, `${finding.id}: carries no round`);
    for (const cell of finding.cells) {
      check(p3.rounds.some((r) => r.id === cell.round), `${finding.id}/${cell.round}: not a recorded round`);
      cell.states.forEach((s) => check(STATES.has(s), `${finding.id}/${cell.round}: unknown state '${s}'`));
      if (cell.states.includes("reopened")) {
        check(["previous-fix", "outside-harness"].includes(cell.cause),
          `${finding.id}/${cell.round}: a re-opening must declare its cause`);
        check(finding.cells.some((c) => c.round === cell.answers),
          `${finding.id}/${cell.round}: answers ${cell.answers}, which has no cell in this finding`);
        // A re-opening that did not come from a round must name where it did
        // come from, so a candidate can draw the arrival from a real origin
        // instead of from the edge of the page.
        if (cell.cause === "outside-harness") {
          const origin = cell.origin;
          check(Boolean(origin?.label && origin?.detail && origin?.quote),
            `${finding.id}/${cell.round}: an outside-harness re-opening must carry an origin label, detail and quotation`);
          if (origin) {
            await checkQuote(`${finding.id}/${cell.round} origin`, origin.quote);
            p3Quotes += 1;
            for (const token of origin.tokens ?? []) {
              check(origin.detail.includes(token) && normQuote(origin.quote.text).includes(token),
                `${finding.id}/${cell.round}: origin token “${token}” is not in both the detail and its quotation`);
            }
            check((origin.tokens ?? []).length > 0,
              `${finding.id}/${cell.round}: an origin must pin at least one identifying token`);
          }
        } else {
          check(!cell.origin, `${finding.id}/${cell.round}: only an outside-harness re-opening may carry an origin`);
        }
      }
      check(!cell.states.includes("extended") || !cell.answers,
        `${finding.id}/${cell.round}: an extension must not claim to answer a round`);
      if (cell.states.includes("residual")) {
        check(Boolean(finding.residual), `${finding.id}: records a residual state but no residual text`);
      }
      if (!cell.hunk) continue;
      const sha = p3.rounds.find((r) => r.id === cell.round)?.sha;
      if (!isQualifiedRevision(sha)) {
        failures.push(`${finding.id}/${cell.round}: names no round carrying an object id, so no diff can be read`);
        continue;
      }
      // The declared path must be a real regular file inside this repository,
      // and it reaches git as a literal pathspec. Without that pinning, magic
      // such as `:(glob)**` would widen the diff and the lines below would
      // match against files the page never names.
      const declared = await readInsideRoot(repoRoot, cell.hunk.path);
      if (!declared.ok) {
        failures.push(`${finding.id}/${cell.round}: declared diff path “${cell.hunk.path}” ${declared.reason}`);
        continue;
      }
      if (typeof cell.hunk.header !== "string" || !Array.isArray(cell.hunk.lines)) {
        failures.push(`${finding.id}/${cell.round}: the declared hunk carries no header string and line list`);
        continue;
      }
      let diff = "";
      try { diff = git(["show", "--no-ext-diff", sha, "--", literalPathspec(cell.hunk.path)]); }
      catch { failures.push(`${finding.id}/${cell.round}: could not read the diff of ${sha}`); continue; }
      check(diff.includes(cell.hunk.header),
        `${finding.id}/${cell.round}: hunk header ${cell.hunk.header} is not in ${sha}'s diff of ${cell.hunk.path}`);
      const real = new Set(diff.split("\n"));
      for (const line of cell.hunk.lines) {
        if (real.has(line)) { hunkLines += 1; continue; }
        failures.push(`${finding.id}/${cell.round}: this line is not in ${sha}'s diff of ${cell.hunk.path}: ${line}`);
      }
    }
    for (const quote of finding.quotes) { await checkQuote(finding.id, quote); p3Quotes += 1; }
  }
  for (const [key, quote] of Object.entries(p3.gate)) { await checkQuote(`gate.${key}`, quote); p3Quotes += 1; }

  // Recomputed here from `findings`, not taken from the source's own answer.
  const reopened = p3.findings.filter((f) => f.cells.some((c) => c.states.includes("reopened")));
  const p3Expected = {
    multiRound: reopened.map((f) => f.id),
    external: reopened.filter((f) => f.cells.some((c) => c.states.includes("reopened") && c.cause === "outside-harness"))
      .map((f) => f.id),
    selfCaused: reopened.filter((f) => !f.cells.some((c) => c.states.includes("reopened") && c.cause === "outside-harness"))
      .map((f) => f.id),
  };
  expectedByCase.p3 = { pages: P3, answer: p3Expected };
  notes.push(`P3 convergence: ${p3.rounds.length} rounds matched against git log, ${hunkLines} diff lines matched ` +
    `character for character, ${p3Quotes} quotations verbatim; multi-round ${p3Expected.multiRound.join(", ")}, ` +
    `external cause ${p3Expected.external.join(", ") || "none"}`);
}

// ----------------------- 3c. D1 entry contract bound to AGENTS.md and the code
const entry = await loadShared("shared/repo-entry.js", "repoEntry");
if (entry) {
  let d1Quotes = 0;
  const ruleSources = ["crates/codeflow-cli/src/cmd/ci.rs", "crates/codeflow-core/src/hooks/git_hook.rs",
    "crates/codeflow-core/src/hooks/git_guard.rs"];
  const ruleTexts = [];
  for (const rel of ruleSources) {
    const source = await sourceContent(rel);
    if (source.ok) ruleTexts.push(source.raw);
    else failures.push(`D1 enforcement source: ${source.reason}`);
  }
  const emitsRule = (rule) => ruleTexts.some((text) => text.includes(`"${rule}"`));
  const planeIds = new Set(entry.planes.map((p) => p.id));

  for (const plane of entry.planes) {
    await checkQuote(`plane ${plane.id}`, plane.quote);
    d1Quotes += 1;
    // A narrow-viewport label may shorten a plane's name; it may not rename it.
    check(Boolean(plane.short), `plane ${plane.id}: carries no narrow-viewport label`);
    for (const word of (plane.short ?? "").split(/\s+/).filter(Boolean)) {
      check(plane.name.includes(word),
        `plane ${plane.id}: short label word “${word}” is not in the plane's name “${plane.name}”`);
    }
  }
  for (const concept of entry.concepts) {
    await checkQuote(`concept ${concept.id}`, concept.quote);
    d1Quotes += 1;
    concept.needs.forEach((need) =>
      check(entry.concepts.some((c) => c.id === need), `${concept.id}: needs unknown concept ${need}`));
  }
  for (const pre of entry.preconditions) {
    await checkQuote(`precondition ${pre.id}`, pre.error);
    d1Quotes += 1;
    for (const also of pre.also ?? []) { await checkQuote(`precondition ${pre.id} (also)`, also); d1Quotes += 1; }
    check(emitsRule(pre.rule), `${pre.id}: no enforcement source emits the rule id '${pre.rule}'`);
    check(entry.concepts.some((c) => c.id === pre.concept), `${pre.id}: attached to unknown concept ${pre.concept}`);
    pre.planes.forEach((id) => check(planeIds.has(id), `${pre.id}: names unknown plane '${id}'`));
  }
  for (const gate of entry.mutationGates) {
    check(emitsRule(gate.rule), `${gate.rule}: no enforcement source emits this rule id`);
    const gateSource = await sourceContent(gate.source);
    if (!gateSource.ok) failures.push(`${gate.rule}: ${gateSource.reason}`);
    else {
      check(gateSource.raw.includes(`"${gate.rule}"`),
        `${gate.rule}: its declared source ${gate.source} does not emit it`);
    }
    gate.planes.forEach((id) => check(planeIds.has(id), `${gate.rule}: names unknown plane '${id}'`));
    if (gate.quote) { await checkQuote(`gate ${gate.rule}`, gate.quote); d1Quotes += 1; }
  }
  for (const step of entry.steps) {
    step.refuses.forEach((id) => check(entry.preconditions.some((p) => p.id === id),
      `${step.id}: refuses unknown precondition ${id}`));
    (step.gates ?? []).forEach((rule) => check(entry.mutationGates.some((g) => g.rule === rule),
      `${step.id}: names unknown gate ${rule}`));
  }

  const capsSource = await sourceContent("docs/capabilities.md");
  if (!capsSource.ok) failures.push(`D1 scale: ${capsSource.reason}`);
  const caps = capsSource.raw ?? "";
  const shipped = (caps.match(/^status: shipped$/gm) ?? []).length;
  const building = (caps.match(/^status: building$/gm) ?? []).length;
  check(shipped === entry.scale.capabilitiesShipped,
    `capabilities.md records ${shipped} shipped, the D1 source says ${entry.scale.capabilitiesShipped}`);
  check(building === entry.scale.capabilitiesBuilding,
    `capabilities.md records ${building} building, the D1 source says ${entry.scale.capabilitiesBuilding}`);
  for (const id of entry.scale.buildingIds) {
    const block = caps.split(/^## /m).find((b) => b.startsWith(`${id} `));
    check(Boolean(block) && /^status: building$/m.test(block), `${id} is not a building capability in capabilities.md`);
  }

  // Recomputed here from `preconditions`: the gate of record is the rule that
  // refuses the most of them.
  const tally = new Map();
  entry.preconditions.forEach((e) => tally.set(e.rule, [...(tally.get(e.rule) ?? []), e.id]));
  const ranked = [...tally.entries()].sort((a, b) => b[1].length - a[1].length);
  check(ranked.length > 1 && ranked[0][1].length > ranked[1][1].length,
    "the gate of record is not uniquely the rule refusing the most preconditions");
  const anchorRule = ranked[0][0];
  const d1Expected = {
    mustBeTrue: entry.preconditions.map((e) => e.id),
    gate: anchorRule,
    refusedByGate: tally.get(anchorRule),
    planes: entry.preconditions.find((e) => e.rule === anchorRule).planes,
    siblingRules: [...tally.keys()].filter((r) => r !== anchorRule && r.startsWith("work.")),
    realBoundary: entry.planes.filter((p) => p.kind === "authoritative" && p.armedHere).map((p) => p.id),
  };
  expectedByCase.d1 = { pages: D1, answer: d1Expected };
  notes.push(`D1 entry: ${entry.preconditions.length} preconditions and ${entry.mutationGates.length} mutation gates ` +
    `bound to their emitting sources, ${d1Quotes} quotations verbatim; gate of record ${d1Expected.gate} ` +
    `(${d1Expected.refusedByGate.length} of ${entry.preconditions.length}), armed perimeter ${d1Expected.realBoundary.join(", ")}`);
}

// -------------------- 3d. D2 record authority bound to real record frontmatter
const authority = await loadShared("shared/record-authority.js", "recordAuthority");
if (authority) {
  let d2Quotes = 0;
  const RELATIONS = new Set(["governs", "supersedes-mechanism", "process"]);
  const EVIDENCE = new Set(["executed", "bounded", "absent"]);
  // Recomputed here from the raw record arrays rather than taken from the
  // source's own index, so a wrong index cannot make a link resolve.
  const recordIndex = new Map([authority.subject, ...authority.records].map((r) => [r.id, r]));
  for (const record of [authority.subject, ...authority.records]) {
    const source = await sourceContent(record.path);
    if (!source.ok) { failures.push(`${record.id}: ${source.reason}`); continue; }
    const fm = source.raw.split("---")[1] ?? "";
    const field = (key) => stripComment((fm.match(new RegExp(`^${key}:\\s*(.*)$`, "m")) ?? ["", ""])[1]);
    check(field("title") === record.title, `${record.id}: title drifted from ${record.path}`);
    for (const [key, want] of Object.entries(record.frontmatter)) {
      check(field(key) === want, `${record.id}: ${key} is “${field(key)}” in ${record.path}, recorded as “${want}”`);
    }
  }
  await checkQuote("partial scope", authority.partialScope);
  await checkQuote("no outcome change", authority.noOutcomeChange);
  d2Quotes += 2;
  for (const claim of authority.claims) {
    await checkQuote(`${claim.id} claim`, claim.quote);
    d2Quotes += 1;
    check(claim.authority.length > 0, `${claim.id}: names no governing decision`);
    for (const link of claim.authority) {
      check(RELATIONS.has(link.relation), `${claim.id}: unknown relation '${link.relation}'`);
      check(recordIndex.has(link.record), `${claim.id}: names unknown record ${link.record}`);
      await checkQuote(`${claim.id} → ${link.record}`, link.quote);
      d2Quotes += 1;
      if (link.relation === "supersedes-mechanism") {
        check(Boolean(link.scope), `${claim.id} → ${link.record}: a partial supersession must quote its scope`);
        await checkQuote(`${claim.id} → ${link.record} scope`, link.scope);
        d2Quotes += 1;
      }
    }
    check(EVIDENCE.has(claim.evidence.state), `${claim.id}: unknown evidence state '${claim.evidence.state}'`);
    await checkQuote(`${claim.id} evidence`, claim.evidence.quote);
    d2Quotes += 1;
    if (claim.evidence.alsoQuote) { await checkQuote(`${claim.id} evidence (also)`, claim.evidence.alsoQuote); d2Quotes += 1; }
    check(claim.evidence.state !== "absent" || claim.evidence.what === null,
      `${claim.id}: an absent evidence state must claim nothing`);
  }
  for (const verdict of authority.verdicts) { await checkQuote(`verdict ${verdict.id}`, verdict.quote); d2Quotes += 1; }

  // Recomputed here from `claims`. A record whose only link to a claim is a
  // named-mechanism supersession governs partially, and `superseded_by` staying
  // null on the records it reaches is exactly why this cannot be read off
  // frontmatter.
  const partialRecords = [...new Set(authority.claims.flatMap((c) =>
    c.authority.filter((a) => a.relation === "supersedes-mechanism").map((a) => a.record)))];
  check(partialRecords.length === 1, `expected exactly one partially superseding decision, found ${partialRecords.length}`);
  // Which decisions it leaves in force is read out of its own verbatim scope
  // sentence, not asserted: the sentence names them.
  const stillInForce = authority.records
    .filter((r) => r.kind === "decision" && r.id !== partialRecords[0] && authority.partialScope.text.includes(r.id))
    .map((r) => r.id);
  const central = authority.claims.find((c) => c.evidence.state === "absent");
  check(Boolean(central), "no claim is recorded as unbacked, so the case has no central finding");
  const d2Expected = {
    governing: partialRecords[0],
    reach: "partial",
    stillInForce,
    centralClaim: central?.id ?? null,
    centralEvidence: central?.evidence.state ?? null,
    unbacked: authority.claims.filter((c) => c.evidence.state === "absent").map((c) => c.id),
    bounded: authority.claims.filter((c) => c.evidence.state === "bounded").map((c) => c.id),
    backed: authority.claims.filter((c) => c.evidence.state === "executed").map((c) => c.id),
  };
  // The records the governing decision explicitly leaves in force must really
  // still be accepted and unsuperseded.
  check(stillInForce.length > 0, "the partial-supersession sentence names no decision it leaves in force");
  for (const id of stillInForce) {
    const record = recordIndex.get(id);
    check(record?.frontmatter.status === "accepted" && record?.frontmatter.superseded_by === "null",
      `${id} is left in force by the scope sentence but its frontmatter is not accepted with superseded_by null`);
  }
  expectedByCase.d2 = { pages: D2, answer: d2Expected };
  notes.push(`D2 authority: ${[authority.subject, ...authority.records].length} records checked against real ` +
    `frontmatter, ${d2Quotes} quotations verbatim; ${d2Expected.governing} governs ${d2Expected.reach}ly, ` +
    `${d2Expected.stillInForce.join(" and ")} still in force, unbacked ${d2Expected.unbacked.join(", ")}`);
}

// ------------------------------- 3e. every case's published answer, per case
const answersFile = JSON.parse(await read("checks/answers.json"));
// Every recorded render is compared, not one per page: a page that published
// the expected answer in its last render and something else in an earlier one
// must not pass.
const expectedFor = new Map(Object.entries(expectedByCase)
  .flatMap(([caseId, { pages, answer }]) => pages.map((page) => [page, { caseId, answer }])));
const answeredPages = new Set();
for (const record of answersFile.answers) {
  answeredPages.add(record.page);
  const want = expectedFor.get(record.page);
  if (!want) { failures.push(`${record.page}: published a derived answer no case expects`); continue; }
  check(JSON.stringify(record.answer) === JSON.stringify(want.answer),
    `${record.render}: published ${want.caseId} answer disagrees with the answer recomputed here`);
}
for (const page of expectedFor.keys()) {
  check(answeredPages.has(page), `${page}: published no derived answer at runtime`);
}
notes.push(`derived answers: ${Object.keys(expectedByCase).length} cases, ${expectedFor.size} candidates, ` +
  `${answersFile.answers.length} recorded renders, each agreeing with an answer recomputed here rather than ` +
  `taken from the page`);

// ------------------------------------------------------- 4. source contracts
const canonicalStudyRoot = await realpath(studyRoot);
const motionless = [];
for (const rel of [...CANDIDATES, ...BASELINES, "board.html"]) {
  const html = await read(rel);
  check(/<html lang="/.test(html), `${rel}: missing lang`);
  check(/<title>/.test(html), `${rel}: missing title`);
  check(/<h1[ >]/.test(html), `${rel}: missing h1`);
  check(/name="viewport"/.test(html), `${rel}: missing viewport meta`);
  check(!/(https?:)?\/\/(?!www\.w3\.org\/2000\/svg)[a-z0-9]/i.test(html), `${rel}: contains a remote URL`);
  check(!/<script[^>]+src=["'](?!\.)/.test(html), `${rel}: loads a non-relative script`);
  // "Relative" is not the same as "inside": `./../../AGENTS.md` is relative.
  // Each script is resolved canonically and must land on a regular file inside
  // the canonical study root.
  for (const src of [...html.matchAll(/<script[^>]+src=["']([^"']+)["']/g)].map((m) => m[1])) {
    const target = await realpath(resolve(studyRoot, dirname(rel), src)).catch(() => null);
    const stats = target === null ? null : await stat(target).catch(() => null);
    if (!stats?.isFile() || !isContained(canonicalStudyRoot, target, path)) {
      failures.push(`${rel}: script ${src} does not resolve to a file inside the study`);
    }
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

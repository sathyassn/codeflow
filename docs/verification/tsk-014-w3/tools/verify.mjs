// Integrity and admissibility checks for the TSK-014 W3 study.
//
// Two different jobs live here and they are kept apart on purpose:
//
//   INTEGRITY (carried over from W2) — the study is internally consistent, its
//   facts are the repository's, its renders are complete and reproducible.
//
//   ADMISSIBILITY (new in W3, rubric.v2.md section A) — the observation gates
//   are capable of meaning anything at all. A1 answer-channel separation, A2
//   declared distinctness, A3 a real intermediate context, A4 carrier honesty,
//   A5 motion, A6 source binding. A clean run here still says nothing about
//   whether a composition communicates.
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
// CF_W3_REPO_ROOT exists so tools/selftest.mjs can verify a disposable copy of
// the study against the real repository. Ordinary runs never set it.
const repoRoot = process.env.CF_W3_REPO_ROOT
  ? resolve(process.env.CF_W3_REPO_ROOT)
  : resolve(studyRoot, "../../..");
const updating = process.argv.includes("--update");
const failures = [];
const notes = [];
const check = (ok, message) => { if (!ok) failures.push(message); };
const read = (rel) => readFile(join(studyRoot, rel), "utf8");
const normalise = (text) => text.replace(/\s+/g, " ").trim();

const registry = JSON.parse(await read("registry.json"));
const caseIds = registry.cases.map((c) => c.id);
const pageOf = (candidate) => `cases/${candidate.case}/${candidate.id}/index.html`;
const CANDIDATES = registry.candidates.map(pageOf);
const BASELINES = caseIds.map((id) => `baselines/${id}/baseline.html`);
const PAGES_OF_CASE = Object.fromEntries(caseIds.map((id) => [
  id, registry.candidates.filter((c) => c.case === id).map(pageOf),
]));
const DOCS = ["README.md", "registry.json", "rubric.v2.md", "rubric.v2.lock.json",
  "rubric.v3.md", "rubric.v3.lock.json", "amendments.lock.json",
  "content-inventory.md", "answer-key.md", "observer-state.md", "NEXT.md", "hypotheses.md",
  "board.html"];

const SHARED_SOURCE = Object.fromEntries(registry.cases.map((c) => [c.id, c.shared_source]));

// A shared subject source is repository-controlled data, not trusted code. It is
// evaluated in a bounded node:vm context with no Node globals and code
// generation disabled (tools/source-authority.mjs), and only a JSON snapshot
// taken inside that same context crosses back, so nothing it defines ever runs
// here and every check below recomputes from its raw fact arrays.
const loadShared = async (rel, global) => {
  const loaded = evaluateSharedSource(await read(rel), global, { filename: rel });
  if (!loaded.ok) { failures.push(`${rel}: ${loaded.reason}`); return null; }
  return loaded.value;
};
const normQuote = (q) => normalise(q.replace(/\\"/g, '"'));

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
// shared source controls and has already been qualified — an object id, or a
// pathspec pinned to `:(literal,top)`.
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

// ------------------------------------------- 1. rubric v2 pre-registration
const lock = JSON.parse(await read("rubric.v2.lock.json"));
const rubricDigest = createHash("sha256").update(await readFile(join(studyRoot, "rubric.v2.md"))).digest("hex");
check(rubricDigest === lock.sha256,
  `rubric.v2.md no longer matches its lock (${rubricDigest} vs ${lock.sha256}); a changed gate needs a new versioned rubric, not an edit`);
check(lock.supersedes === "../tsk-014-w2/rubric.md",
  "the W3 lock does not record which rubric it versions");

// rubric.v2.md line 10 states that this tool asserts the rubric's modification
// time precedes every render. It did not, until now — which is recorded in
// rubric.v3.md rather than fixed by editing the pinned v2 bytes. The assertion
// is real from here on, and it is deliberately loud about what it cannot do:
// mtime does not survive a fresh checkout, so an unreadable or inverted time is
// reported as unproven ordering rather than quietly treated as a pass.
const rubricMtime = (await stat(join(studyRoot, "rubric.v2.md"))).mtimeMs;
const renderNames = await readdir(join(studyRoot, "renders"));
let earliestRender = Infinity;
for (const name of renderNames.filter((n) => n.endsWith(".png"))) {
  earliestRender = Math.min(earliestRender, (await stat(join(studyRoot, "renders", name))).mtimeMs);
}
check(Number.isFinite(earliestRender) && rubricMtime < earliestRender,
  "the rubric's modification time does not precede every render, so this checkout cannot witness the "
  + "ordering that makes the rubric a pre-registration; see rubric.v3.md 'What the ordering evidence is worth'");

// v3 governs this board. v2 stays byte-identical, exactly as v2 kept v1.
const lockV3 = JSON.parse(await read("rubric.v3.lock.json"));
const rubricV3Digest = createHash("sha256").update(await readFile(join(studyRoot, "rubric.v3.md"))).digest("hex");
check(rubricV3Digest === lockV3.sha256,
  `rubric.v3.md no longer matches its lock (${rubricV3Digest} vs ${lockV3.sha256})`);
check(lockV3.supersedes === "rubric.v2.md",
  "the v3 lock does not record which rubric it versions");
// W2's pinned rubric is not this study's to touch. It is located through the
// repository root rather than through this directory's parent, so a disposable
// copy of the study still checks the real W2 board instead of crashing on a
// sibling that only exists in the working tree.
const w2Dir = join(repoRoot, "docs/verification/tsk-014-w2");
const w2Lock = JSON.parse(await readFile(join(w2Dir, "rubric.lock.json"), "utf8"));
const w2Digest = createHash("sha256").update(await readFile(join(w2Dir, "rubric.md"))).digest("hex");
check(w2Digest === w2Lock.sha256,
  "the W2 rubric no longer matches its own lock; W3 must version it, never edit it");
notes.push(`rubric v2 lock: bytes match ${lock.sha256.slice(0, 12)}…; the W2 rubric is byte-identical to its own lock`);

// --------------------------------------------- 1b. A8 registration honesty (v3)
// The registry is only a pre-registration if its priority is real, and its
// priority is only knowable from what the files can witness. So the file has to
// carry the account rather than the assertion, and every post-registration
// change to a registered field has to appear as an amendment. Rewording a claim
// does not satisfy this; withdrawing it and recording what happened does.
const evidence = registry.pre_registration_evidence;
check(evidence && typeof evidence === "object",
  "A8: registry.json carries no pre_registration_evidence block");
for (const field of ["claim", "what_the_files_can_show", "what_the_files_cannot_show", "standing"]) {
  check(typeof evidence?.[field] === "string" && evidence[field].length > 40,
    `A8: pre_registration_evidence.${field} is missing or too thin to be an account`);
}
check(Array.isArray(registry.amendments),
  "A8: registry.json carries no amendments array; 'no amendments' is recorded as an empty array, never by omission");
for (const amendment of registry.amendments ?? []) {
  for (const field of ["id", "candidate", "field", "when", "what_happened", "why_it_matters", "recorded_by"]) {
    check(typeof amendment[field] === "string" && amendment[field].length > 0,
      `A8: amendment ${amendment.id ?? "(unnamed)"} is missing ${field}`);
  }
}

// An amendments array can be emptied as easily as a false claim can be written,
// and this study has already produced both. The amendments an outside seat
// established from evidence are therefore locked by digest over their verbatim
// before/after text: dropping one, renaming its field, or quietly rewording
// either side fails here, in the same way an edited rubric fails its own lock.
const amendmentLock = JSON.parse(await read("amendments.lock.json"));
for (const required of amendmentLock.required) {
  const found = (registry.amendments ?? []).find((a) => a.id === required.id);
  check(Boolean(found), `A8: locked amendment ${required.id} is not in registry.json; an established amendment `
    + "cannot be dropped, only added to");
  if (!found) continue;
  check(found.candidate === required.candidate && found.field === required.field,
    `A8: locked amendment ${required.id} should record ${required.candidate}.${required.field}, `
    + `not ${found.candidate}.${found.field}`);
  for (const side of ["before", "after"]) {
    const digest = createHash("sha256").update(found[side] ?? "", "utf8").digest("hex");
    check(digest === required[`${side}_sha256`],
      `A8: locked amendment ${required.id} has a ${side} wording that does not match its lock `
      + `(${digest.slice(0, 12)}… vs ${required[`${side}_sha256`].slice(0, 12)}…)`);
  }
  // The recorded `after` has to still be the field's live value, or the record
  // is describing a registry that no longer exists.
  const candidate = registry.candidates.find((c) => c.id === required.candidate);
  check(candidate?.[required.field] === found.after,
    `A8: amendment ${required.id} records an 'after' wording that is not ${required.candidate}'s live `
    + `${required.field}`);
}
const amendedFields = new Set((registry.amendments ?? []).map((a) => `${a.candidate}:${a.field}`));
notes.push(`A8 registration honesty: ${(registry.amendments ?? []).length} amendment(s) over `
  + `${amendedFields.size} candidate/field pair(s), ${amendmentLock.required.length} of them digest-locked `
  + "against their verbatim prior wording; pre-registration standing stated, not asserted");

// ------------------------------- 2. registry contract (rubric.v2 A2, A3, A4, A5)
const CARRIER_VERDICTS = new Set(["native", "escape-with-loss", "needs-new-block",
  "needs-generator-work", "needs-source-model"]);
const schema = JSON.parse(await readFile(
  join(repoRoot, "assets/base/present/schemas/document-v1.schema.json"), "utf8"));
const blockNames = new Set(Object.keys(schema.$defs));
const tabletWidth = registry.contexts.viewports.tablet.width;

for (const kase of registry.cases) {
  for (const field of ["surface_job", "question", "question_fitness", "compound", "shared_source"]) {
    check(kase[field] !== undefined, `${kase.id}: registers no ${field}`);
  }
  check(kase.question.trim().endsWith("?"), `${kase.id}: the registered question is not a question`);
  check(kase.question_fitness.answerable_by_each_encoding === true,
    `${kase.id}: does not assert that every candidate's encoding can answer the question`);
  check(typeof kase.question_fitness.discriminates === "string" && kase.question_fitness.discriminates.length > 20,
    `${kase.id}: does not say how the question discriminates between forms`);
  if (kase.compound.is_compound) {
    check(["decomposed", "explicit-optimisation"].includes(kase.compound.handling),
      `${kase.id}: a compound question must be decomposed or carry an explicit optimisation`);
    check((kase.compound.parts ?? []).length >= 2, `${kase.id}: a compound question must name its parts`);
  }
  const siblings = registry.candidates.filter((c) => c.case === kase.id);
  check(siblings.length === kase.candidates.length,
    `${kase.id}: registers ${kase.candidates.length} candidates but ${siblings.length} are defined`);
  // A2 — declared distinctness, before authoring rather than after.
  for (let i = 0; i < siblings.length; i += 1) {
    for (let j = i + 1; j < siblings.length; j += 1) {
      const a = siblings[i], b = siblings[j];
      check(a.primary_unit !== b.primary_unit || a.primary_axis !== b.primary_axis,
        `${kase.id}: ${a.id} and ${b.id} share both primary unit and primary axis`);
      check(!(a.primary_unit === b.primary_unit && a.primary_axis === b.primary_axis
        && a.encoded_relationship === b.encoded_relationship),
        `${kase.id}: ${a.id} and ${b.id} declare an identical encoding`);
    }
    if (kase.compound.is_compound && kase.compound.handling === "explicit-optimisation") {
      check(siblings[i].optimises && siblings[i].optimises !== "n/a",
        `${siblings[i].id}: a candidate for a compound question must name the part it optimises`);
      check(siblings[i].carries_other_by && siblings[i].carries_other_by !== "n/a",
        `${siblings[i].id}: must say what carries the part it does not optimise`);
    }
  }
}

for (const candidate of registry.candidates) {
  const page = pageOf(candidate);
  const html = await read(page).catch(() => null);
  if (html === null) { failures.push(`${candidate.id}: ${page} does not exist`); continue; }
  for (const field of ["primary_unit", "primary_axis", "encoded_relationship",
    "intermediate_composition", "narrow_composition", "states", "carrier", "motion"]) {
    check(candidate[field] !== undefined, `${candidate.id}: registers no ${field}`);
  }
  // A3 — the intermediate render must show a composition of its own.
  const { wide, narrow } = candidate.breakpoints ?? {};
  check(Number.isInteger(wide) && Number.isInteger(narrow) && narrow < tabletWidth && tabletWidth < wide,
    `${candidate.id}: the tablet width ${tabletWidth} does not fall strictly between its declared breakpoints`);
  check(html.includes(`max-width: ${wide}px`), `${candidate.id}: declares no media query at ${wide}px`);
  check(html.includes(`max-width: ${narrow}px`), `${candidate.id}: declares no media query at ${narrow}px`);
  // Every declared non-colour channel must actually be in the page.
  for (const state of candidate.states) {
    check(html.includes(state.non_colour_marker),
      `${candidate.id}: state “${state.name}” declares the non-colour marker “${state.non_colour_marker}”, which is not in the page`);
  }
  // A4 — carrier honesty, bound to the real contract.
  const carrier = candidate.carrier;
  check(CARRIER_VERDICTS.has(carrier.verdict), `${candidate.id}: unknown carrier verdict '${carrier.verdict}'`);
  check(Object.keys(registry.carrier_contracts).includes(carrier.surface),
    `${candidate.id}: names an unregistered carrier surface '${carrier.surface}'`);
  for (const block of carrier.native_blocks_considered ?? []) {
    check(blockNames.has(block),
      `${candidate.id}: names block type '${block}', which is not in the live document schema`);
  }
  if (carrier.verdict !== "native") {
    check(typeof carrier.loss === "string" && carrier.loss.length > 40,
      `${candidate.id}: a non-native carrier verdict must state its material loss`);
  }
  // A5 — motion is declared none, and checks/motion.json has to prove it.
  check(candidate.motion === "none",
    `${candidate.id}: declares motion '${candidate.motion}'; this study registers no motion evidence path`);
}
notes.push(`registry: ${registry.candidates.length} candidates across ${registry.cases.length} cases; `
  + `distinctness, breakpoints (tablet ${tabletWidth}), non-colour channels and carrier verdicts all bound`);

// Carrier claims are quoted from the source that imposes them.
let carrierQuotes = 0;
for (const [surface, contract] of Object.entries(registry.carrier_contracts)) {
  for (const fact of contract.facts) {
    if (!fact.quote) continue;
    await checkQuote(`carrier ${surface}/${fact.id}`, { text: fact.quote, source: fact.source });
    carrierQuotes += 1;
  }
}
const portalPkg = JSON.parse(await readFile(join(repoRoot, "docs-portal/package.json"), "utf8"));
check(!("@astrojs/mdx" in { ...portalPkg.dependencies, ...portalPkg.devDependencies }),
  "the portal now installs @astrojs/mdx, so the registry's no-component-runtime carrier fact is stale");
check(blockNames.has(registry.carrier_contracts["cf-present"].escape_block),
  "the registered escape block is not in the live document schema");
notes.push(`carrier: ${carrierQuotes} claims quoted verbatim from the sources that impose them; `
  + `${blockNames.size} block types in the live schema; portal MDX absent as recorded`);

// board.html reads the registry through a generated wrapper rather than over the
// network. There is one authority: the wrapper's payload must be registry.json
// byte for byte, so the board can never drift from what was registered.
const wrapper = await read("shared/registry.js");
const payload = wrapper.slice(wrapper.indexOf("window.__w3registry =\n") + "window.__w3registry =\n".length, -2);
check(payload === await read("registry.json"),
  "shared/registry.js is not registry.json byte for byte; regenerate the wrapper rather than editing it");

// The convergence hypothesis stays a hypothesis.
for (const hypothesis of registry.convergence_hypotheses ?? []) {
  check(hypothesis.status === "hypothesis",
    `${hypothesis.id}: a cross-candidate convergence may not be recorded as anything but a hypothesis here`);
  check((hypothesis.not_promoted_because ?? []).length >= 3,
    `${hypothesis.id}: must record why it is not recurrence evidence`);
}

// ------------------------------------------- 3. P1 dataset + derived answers
const stripComment = (v) => v.split(" #")[0].trim().replace(/^"|"$/g, "");
const parseList = (v) => {
  const inner = stripComment(v).replace(/^\[|\]$/g, "").trim();
  return inner ? inner.split(",").map((x) => x.trim()).filter(Boolean) : [];
};
const repoTasks = new Map();
for (const id of ["005", "006", "007", "008", "009", "010", "011", "012", "013", "014"]) {
  const raw = await readFile(join(repoRoot, `project-management/tasks/TSK-${id}.md`), "utf8");
  const fm = raw.split("---")[1];
  const field = (key) => (fm.match(new RegExp(`^${key}:\\s*(.*)$`, "m")) ?? ["", ""])[1];
  repoTasks.set(`TSK-${id}`, {
    status: stripComment(field("status")), deps: parseList(field("depends_on")),
    specs: parseList(field("specs")), target: stripComment(field("integration_target")),
    title: stripComment(field("title")),
  });
}

const P1_PAGES = [...PAGES_OF_CASE.p1, "baselines/p1/baseline.html"];
const datasets = [];
for (const rel of P1_PAGES) {
  const raw = (await read(rel)).match(/<script type="application\/json" id="plan-data">([\s\S]*?)<\/script>/);
  if (!raw) { failures.push(`${rel}: no embedded plan dataset`); continue; }
  datasets.push({ rel, raw: raw[1], data: JSON.parse(raw[1]) });
}
if (datasets.length === P1_PAGES.length) {
  datasets.slice(1).forEach((d) =>
    check(d.raw === datasets[0].raw, `${d.rel}: dataset is not byte-identical to ${datasets[0].rel}`));
  for (const task of datasets[0].data.tasks) {
    const repo = repoTasks.get(task.id);
    if (!repo) { failures.push(`${task.id} is not a real EPC-005 task`); continue; }
    check(task.status === repo.status, `${task.id}: status drifted from frontmatter`);
    check(task.deps.join(",") === repo.deps.join(","), `${task.id}: depends_on drifted from frontmatter`);
    check(task.specs.join(",") === repo.specs.join(","), `${task.id}: specs drifted from frontmatter`);
    check(task.target === repo.target, `${task.id}: integration_target drifted from frontmatter`);
    check(task.title === repo.title, `${task.id}: title is “${task.title}” here, “${repo.title}” in frontmatter`);
  }
  check(datasets[0].data.tasks.length === repoTasks.size, "the dataset does not cover every EPC-005 task");
}
// A short label may shorten a task's real title; it may never rename it.
let shortLabels = 0;
for (const rel of PAGES_OF_CASE.p1) {
  const html = await read(rel);
  const block = html.match(/const short = \{([\s\S]*?)\};/);
  if (!block) continue;
  for (const [, id, label] of block[1].matchAll(/"(TSK-\d{3})":\s*"([^"]+)"/g)) {
    const title = (repoTasks.get(id)?.title ?? "").toLowerCase();
    for (const word of label.toLowerCase().split(/\s+/).filter(Boolean)) {
      check(title.includes(word), `${rel}: short label word “${word}” for ${id} is not in its title “${title}”`);
    }
    shortLabels += 1;
  }
}
notes.push(`P1 dataset: ${datasets[0]?.data.tasks.length ?? 0} tasks verified against frontmatter including titles, `
  + `byte-identical across ${datasets.length} pages; ${shortLabels} short labels are word-subsets of real titles`);

// Recomputed here from the repository, independently of the shared model.
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
const slackOf = (id) => (maxWave - longestToSink(id)) - waveOf(id);
const expectedRoom = Object.fromEntries(openIds.map((id) => [id, slackOf(id)]).filter(([, s]) => s > 0));
const expectedMoves = openIds.filter((id) => slackOf(id) === 0);
let head = openIds.reduce((best, id) => (longestToSink(id) > longestToSink(best) ? id : best), openIds[0]);
const expectedChain = [head];
while (succ(head).length) {
  head = succ(head).reduce((best, s) => (longestToSink(s) > longestToSink(best) ? s : best));
  expectedChain.push(head);
}
const expectedByCase = {
  p1: { pages: PAGES_OF_CASE.p1, answer: {
    movesFinish: expectedMoves, room: expectedRoom, chain: expectedChain,
    startable: openIds.filter((id) => repoTasks.get(id).deps.every(doneT)),
  } },
};

// ---------------------------------------- 4. P2 registry, causes and stops
const p2 = await loadShared(SHARED_SOURCE.p2, "tsk007Facts");
if (p2) {
  const source = await sourceContent(p2.source);
  if (!source.ok) failures.push(`the P2 fact registry's ${source.reason}`);
  const text = source.ok ? source.text : "";
  const facts = await read(SHARED_SOURCE.p2);
  const quotes = [...facts.matchAll(/quotes?:\s*(?:\[([\s\S]*?)\]|"((?:[^"\\]|\\.)*)")/g)].flatMap((m) =>
    m[1] ? [...m[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((q) => q[1]) : [m[2]]);
  check(quotes.length >= 15, `the fact registry carries only ${quotes.length} verbatim quotations`);
  let missing = 0;
  for (const quote of quotes) {
    if (source.ok && !text.includes(normQuote(quote))) {
      failures.push(`quotation absent from ${p2.source}: “${quote.slice(0, 60)}…”`);
      missing += 1;
    }
  }
  check(facts.includes(p2.candidate), "the registry does not pin the exact candidate");

  // Every cause carries the record's own sentence, and every absence is
  // classified exactly once. A gap that is not classified would simply vanish
  // from the encoding, so it fails the run instead.
  const causeIds = new Set(p2.causes.map((c) => c.id));
  for (const cause of p2.causes) {
    await checkQuote(`p2 cause ${cause.id}`, { text: cause.quote, source: p2.source });
    if (cause.alsoQuote) await checkQuote(`p2 cause ${cause.id} (also)`, { text: cause.alsoQuote, source: p2.source });
  }
  let absences = 0;
  for (const claim of p2.claims) {
    claim.cells.forEach((cell, lane) => {
      const key = `${claim.id}:${lane}`;
      const classified = p2.absenceCauses[key];
      const isAbsence = cell.state !== "executed" && cell.state !== "na";
      if (isAbsence) {
        absences += 1;
        check(Boolean(classified), `${key}: an absence with no recorded cause`);
        check(!classified || causeIds.has(classified), `${key}: unknown cause '${classified}'`);
      } else {
        check(!classified, `${key}: is ${cell.state} but carries an absence cause`);
      }
    });
    // Only a claim short of acceptance may record a stop, and it must record one.
    const reach = claim.provenance ? claim.provenance.reaches
      : (claim.cells.some((c) => c.state === "executed") ? 4 : 0);
    if (reach < 4) check(causeIds.has(p2.stops[claim.id]), `${claim.id}: stops short but records no known stop cause`);
    else check(!p2.stops[claim.id], `${claim.id}: reaches acceptance but records a stop cause`);
  }
  check(Object.keys(p2.absenceCauses).length === absences,
    `absenceCauses has ${Object.keys(p2.absenceCauses).length} entries for ${absences} absences`);

  // Recomputed here from the raw arrays, not read off the source's own answer.
  const executedLanes = p2.lanes.filter((_, lane) => p2.claims.some((c) => c.cells[lane]?.state === "executed"));
  const causesByLane = Object.fromEntries(p2.lanes.map((name, lane) => [name,
    [...new Set(p2.claims.map((c) => p2.absenceCauses[`${c.id}:${lane}`]).filter(Boolean))].sort()]));
  const share = Object.fromEntries(p2.causes.map((c) => [c.id,
    Object.values(p2.absenceCauses).filter((id) => id === c.id).length]));
  expectedByCase.p2 = { pages: PAGES_OF_CASE.p2,
    answer: { executedLanes, causesByLane, absences, share } };
  notes.push(`P2 registry: ${quotes.length} quotations verbatim (${missing} missing), `
    + `${absences} absences each classified once against ${p2.causes.length} causes`);
}

// ------------------------------- 5. P3 convergence bound to real git history
const p3 = await loadShared(SHARED_SOURCE.p3, "p3Convergence");
if (p3) {
  const STATES = new Set(["opened", "reopened", "closed", "extended", "residual"]);
  let hunkLines = 0;
  let p3Quotes = 0;
  for (const round of p3.rounds) {
    if (!isQualifiedRevision(round.sha)) {
      failures.push(`${round.id}: “${round.sha}” is not an object id, so it is not read as a revision`);
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
        failures.push(`${finding.id}/${cell.round}: names no round carrying an object id`);
        continue;
      }
      const declared = await readInsideRoot(repoRoot, cell.hunk.path);
      if (!declared.ok) {
        failures.push(`${finding.id}/${cell.round}: declared diff path “${cell.hunk.path}” ${declared.reason}`);
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

  const reopened = p3.findings.filter((f) => f.cells.some((c) => c.states.includes("reopened")));
  expectedByCase.p3 = { pages: PAGES_OF_CASE.p3, answer: {
    multiRound: reopened.map((f) => f.id),
    external: reopened.filter((f) => f.cells.some((c) => c.states.includes("reopened") && c.cause === "outside-harness")).map((f) => f.id),
    selfCaused: reopened.filter((f) => !f.cells.some((c) => c.states.includes("reopened") && c.cause === "outside-harness")).map((f) => f.id),
  } };
  notes.push(`P3 convergence: ${p3.rounds.length} rounds matched against git log, ${hunkLines} diff lines matched `
    + `character for character, ${p3Quotes} quotations verbatim`);
}

// ------------------------- 6. D1 layers re-parsed out of the contract's table
const layers = await loadShared(SHARED_SOURCE.d1, "repoLayers");
if (layers) {
  const AUTHORITIES = new Set(layers.authorities.map((a) => a.id));
  const BANDS = new Set(layers.bands.map((b) => b.id));
  const KINDS = new Set(["file", "dir", "command", "local-state"]);
  const contract = await sourceContent(layers.contract);
  if (!contract.ok) failures.push(`D1: ${contract.reason}`);
  let d1Quotes = 0;

  // The organisation table is re-parsed here and compared cell by cell, so the
  // volatility axis is the contract's own ordering and not this study's opinion.
  const rows = [...(contract.raw ?? "").matchAll(/^\| ([A-Z]+) — ([^|]+?) \| ([^|]+?) \| ([^|]+?) \|$/gm)]
    .map((m) => ({ id: m[1], asks: m[2].trim(), livesIn: m[3].trim(), changes: m[4].trim() }));
  check(rows.length === layers.layers.length,
    `the contract table has ${rows.length} layer rows, the D1 source declares ${layers.layers.length}`);
  rows.forEach((row, index) => {
    const declared = layers.layers.find((l) => l.id === row.id);
    if (!declared) { failures.push(`${row.id} is in the contract table but not in the D1 source`); return; }
    check(declared.asks === row.asks, `${row.id}: asks drifted from the contract table`);
    check(declared.livesIn === row.livesIn, `${row.id}: 'lives in' drifted from the contract table`);
    check(declared.changes === row.changes, `${row.id}: 'changes' drifted from the contract table`);
    check(declared.cadence === index + 1,
      `${row.id}: cadence ${declared.cadence} is not its row index ${index + 1} in the contract table`);
  });

  for (const authority of layers.authorities) {
    check(BANDS.has(authority.band), `${authority.id}: unknown band '${authority.band}'`);
    await checkQuote(`authority ${authority.id}`, authority.quote);
    d1Quotes += 1;
  }
  for (const layer of layers.layers) {
    await checkQuote(`layer ${layer.id}`, layer.row);
    d1Quotes += 1;
    for (const home of layer.homes) {
      check(KINDS.has(home.kind), `${layer.id}/${home.path}: unknown home kind '${home.kind}'`);
      check(AUTHORITIES.has(home.authority), `${layer.id}/${home.path}: unknown authority '${home.authority}'`);
      if (home.split) check(AUTHORITIES.has(home.split), `${layer.id}/${home.path}: unknown split authority`);
      if (home.kind === "file" || home.kind === "dir") {
        // A declared path must be a real path in this checkout.
        const info = await stat(join(repoRoot, home.path)).catch(() => null);
        check(Boolean(info), `${layer.id}: declared home ${home.path} is not in this checkout`);
        if (info) {
          check(home.kind === "file" ? info.isFile() : info.isDirectory(),
            `${layer.id}: ${home.path} is declared as a ${home.kind} and is not one`);
        }
      } else {
        // A home that is not a path must say so and quote why.
        check(!(await stat(join(repoRoot, home.path)).catch(() => null)),
          `${layer.id}: ${home.path} is declared as ${home.kind} but exists as a path`);
        await checkQuote(`${layer.id} absent home`, home.absentQuote);
        d1Quotes += 1;
      }
    }
  }
  await checkQuote("spine", layers.spine.quote);
  await checkQuote("spine tail", layers.spine.tail);
  d1Quotes += 2;
  // The declaring sentence wraps across two lines in AGENTS.md, so both halves
  // are quoted and a node may be named in either. Spacing around the slash is
  // this study's typography, not the contract's, so it is normalised away.
  const spineSentence = `${normQuote(layers.spine.quote.text)} ${normQuote(layers.spine.tail.text)}`;
  for (const node of layers.spine.nodes) {
    check(layers.layers.some((l) => l.id === node.layer), `spine node ${node.id}: unknown layer ${node.layer}`);
    check(spineSentence.includes(node.label.replace(/ \/ /g, "/")),
      `spine node ${node.id}: “${node.label}” is not named in the quoted spine sentence`);
  }

  const bandOf = (id) => layers.authorities.find((a) => a.id === id).band;
  const paths = layers.layers.flatMap((l) => l.homes.map((h) => ({ ...h, layer: l.id })));
  expectedByCase.d1 = { pages: PAGES_OF_CASE.d1, answer: {
    order: [...layers.layers].sort((a, b) => a.cadence - b.cadence).map((l) => l.id),
    yours: paths.filter((p) => bandOf(p.authority) === "yours").map((p) => p.path),
    conditional: paths.filter((p) => bandOf(p.authority) === "conditional").map((p) => p.path),
    refuses: paths.filter((p) => bandOf(p.authority) === "refuses").map((p) => p.path),
    split: paths.filter((p) => p.split).map((p) => p.path),
    notInTree: paths.filter((p) => !["file", "dir"].includes(p.kind)).map((p) => p.path),
    rulesLayer: layers.layers.find((l) => l.asks === "how we work").id,
  } };
  notes.push(`D1 layers: ${rows.length} contract rows re-parsed and compared cell by cell, `
    + `${paths.length} declared homes checked against this checkout, ${d1Quotes} quotations verbatim`);
}

// -------------------- 7. D2 record authority bound to real record frontmatter
const authority = await loadShared(SHARED_SOURCE.d2, "recordAuthority");
if (authority) {
  let d2Quotes = 0;
  const RELATIONS = new Set(["governs", "supersedes-mechanism", "process"]);
  const EVIDENCE = new Set(["executed", "bounded", "absent"]);
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

  // The record's own shape is re-parsed out of SPC-004, so reach is drawn over
  // something real rather than over a list this study invented.
  const spc = await sourceContent(authority.subject.path);
  if (spc.ok) {
    const headings = [...spc.raw.matchAll(/^## (.+)$/gm)].map((m) => m[1].trim());
    const behaviourBlock = spc.raw.split(/^## Behavior$/m)[1]?.split(/^## /m)[0] ?? "";
    const items = (behaviourBlock.match(/^\s*\d+\.\s/gm) ?? []).length;
    check(items === authority.behaviourCount,
      `SPC-004 has ${items} numbered behaviours, the D2 source declares ${authority.behaviourCount}`);
    for (const section of authority.sections) {
      if (section.kind !== "heading") continue;
      check(headings.includes(section.label),
        `the D2 source declares section “${section.label}”, which is not a heading in SPC-004`);
    }
    // Every heading of the record must be on the spine, either as itself or —
    // for the one heading whose numbered items are the real units — as those
    // items. A heading the spine silently omits would make the drawn reach look
    // larger than it is.
    const declaredHeadings = authority.sections.filter((s) => s.kind === "heading").map((s) => s.label);
    const declaredItems = authority.sections.filter((s) => s.kind === "item").map((s) => s.label);
    for (const heading of headings) {
      check(declaredHeadings.includes(heading)
        || declaredItems.filter((label) => label.startsWith(`${heading} `)).length === authority.behaviourCount,
        `SPC-004 has heading “${heading}”, which the D2 source neither places on its record spine nor expands into its numbered items`);
    }
  }
  for (const [claimId, sectionId] of Object.entries(authority.sectionOfClaim)) {
    check(authority.sections.some((s) => s.id === sectionId), `${claimId}: unknown section '${sectionId}'`);
    check(authority.claims.some((c) => c.id === claimId), `unknown claim '${claimId}' placed on a section`);
  }

  for (const claim of authority.claims) {
    await checkQuote(`${claim.id} claim`, claim.quote);
    d2Quotes += 1;
    check(claim.authority.length > 0, `${claim.id}: names no governing decision`);
    check(Boolean(authority.sectionOfClaim[claim.id]), `${claim.id}: is not placed on a section of the record`);
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

  const partialRecords = [...new Set(authority.claims.flatMap((c) =>
    c.authority.filter((a) => a.relation === "supersedes-mechanism").map((a) => a.record)))];
  check(partialRecords.length === 1, `expected exactly one partially superseding decision, found ${partialRecords.length}`);
  const stillInForce = authority.records
    .filter((r) => r.kind === "decision" && r.id !== partialRecords[0] && authority.partialScope.text.includes(r.id))
    .map((r) => r.id);
  check(stillInForce.length > 0, "the partial-supersession sentence names no decision it leaves in force");
  for (const id of stillInForce) {
    const record = recordIndex.get(id);
    check(record?.frontmatter.status === "accepted" && record?.frontmatter.superseded_by === "null",
      `${id} is left in force by the scope sentence but its frontmatter is not accepted with superseded_by null`);
  }
  const central = authority.claims.find((c) => c.evidence.state === "absent");
  check(Boolean(central), "no claim is recorded as unbacked, so the case has no central finding");
  expectedByCase.d2 = { pages: PAGES_OF_CASE.d2, answer: {
    governing: partialRecords[0],
    reach: "partial",
    stillInForce,
    centralClaim: central?.id ?? null,
    centralEvidence: central?.evidence.state ?? null,
    unbacked: authority.claims.filter((c) => c.evidence.state === "absent").map((c) => c.id),
    bounded: authority.claims.filter((c) => c.evidence.state === "bounded").map((c) => c.id),
    backed: authority.claims.filter((c) => c.evidence.state === "executed").map((c) => c.id),
  } };
  notes.push(`D2 authority: ${[authority.subject, ...authority.records].length} records checked against real `
    + `frontmatter, ${authority.sections.length} record sections re-parsed out of SPC-004, ${d2Quotes} quotations verbatim`);
}

// ------------------------ 8. every case's published answer, per recorded render
const answersFile = JSON.parse(await read("checks/answers.json"));
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
check(expectedFor.size === CANDIDATES.length,
  `${expectedFor.size} candidates publish an answer, ${CANDIDATES.length} exist`);
notes.push(`derived answers: ${caseIds.length} cases, ${expectedFor.size} candidates, `
  + `${answersFile.answers.length} recorded renders, each agreeing with an answer recomputed here`);

// ---- 9. A1: the reader's channel must not state what the encoding is tested on
const textFile = JSON.parse(await read("checks/text.json"));
const caseOfPage = new Map(registry.candidates.map((c) => [pageOf(c), c.case]));
let leaks = 0;
let scanned = 0;
for (const record of textFile.renders) {
  const caseId = caseOfPage.get(record.page);
  if (!caseId) continue;              // baselines may say anything; that is the point
  const kase = registry.cases.find((c) => c.id === caseId);
  const haystack = record.text.toLowerCase();
  scanned += 1;
  for (const phrase of kase.forbidden_phrases) {
    if (haystack.includes(phrase.toLowerCase())) {
      failures.push(`A1 VOID ${record.render}: visible text contains the registered forbidden phrase “${phrase}”`);
      leaks += 1;
    }
  }
  for (const { pattern, why } of kase.forbidden_patterns) {
    const match = record.text.match(new RegExp(pattern, "u"));
    if (match) {
      failures.push(`A1 VOID ${record.render}: visible text matches /${pattern}/ (${why}) at “${match[0].slice(0, 60)}”`);
      leaks += 1;
    }
  }
  // The machine channel must not be in the reader's channel either.
  const answer = expectedFor.get(record.page);
  if (answer && record.text.includes(JSON.stringify(answer.answer))) {
    failures.push(`A1 VOID ${record.render}: the derived answer payload is visible on the page`);
    leaks += 1;
  }
}
check(scanned === CANDIDATES.length * 6,
  `A1 scanned ${scanned} candidate renders, expected ${CANDIDATES.length * 6}`);
notes.push(`A1 answer channel: ${scanned} candidate renders scanned against `
  + `${registry.cases.reduce((n, c) => n + c.forbidden_phrases.length + c.forbidden_patterns.length, 0)} registered leak tests, `
  + `${leaks} leak(s)`);

// -------------------------------------------------------------- 10. A5 motion
const motionFile = JSON.parse(await read("checks/motion.json"));
const moving = motionFile.probes.filter((p) => p.running || p.declaredAnimation || p.declaredTransition);
check(moving.length === 0,
  `A5: ${moving.length} probe(s) found movement on pages registered as motionless: `
  + `${moving.slice(0, 3).map((m) => m.render).join(", ")}`);
const probedRenders = new Set(motionFile.probes.map((p) => p.render));
check(motionFile.probes.length === probedRenders.size * 2,
  "A5: every render must be probed under both prefers-reduced-motion settings");
for (const setting of ["no-preference", "reduce"]) {
  check(motionFile.probes.some((p) => p.reduced_motion === setting),
    `A5: no probe was taken with prefers-reduced-motion: ${setting}`);
}
notes.push(`A5 motion: ${motionFile.probes.length} probes over ${probedRenders.size} renders under both `
  + `prefers-reduced-motion settings, 0 running animations, 0 declared transitions; no reduced-motion screenshot taken`);

// --------------------------------------------- 10b. A7 frame containment (v3)
// A still render is this study's whole evidence channel. Content a reader would
// have to scroll or resize to reach is not on the page, so a clipped scroll
// container and a mark drawn past its own viewBox both mean the render does not
// carry what the page contains — and every other gate here would still pass.
// A band that cannot satisfy this may be registered as invalidated, with a
// reason; it is then reported as invalidated rather than passed, and no
// observation gate may be recorded for it.
const frameFile = JSON.parse(await read("checks/frame.json"));
const invalidated = new Map((registry.invalidated_bands ?? []).map((b) => [b.render, b]));
const FAILURE_KINDS = new Set(["containment", "legibility", "composition"]);
const SEVERITIES = new Set(["minor", "material", "disqualifying"]);
for (const band of invalidated.values()) {
  // Two separate statements are required, not one. A band is inadmissible as
  // comparison evidence AND something about it demonstrably failed; a record
  // that carries only the first turns a real failure into a procedural shrug.
  check(typeof band.render === "string" && typeof band.evidence_status === "string"
    && typeof band.disposition === "string" && typeof band.fix_is === "string",
    `A7: an invalidated band must name its render, its evidence status, what fixing it would take, and its `
    + `disposition (${band.render ?? "(unnamed)"})`);
  const failure = band.failure;
  check(failure?.demonstrated === true,
    `A7: ${band.render} is registered invalidated without a demonstrated failure; a band is invalidated `
    + "because something failed, never merely to excuse it from observation");
  check(FAILURE_KINDS.has(failure?.kind),
    `A7: ${band.render} declares failure kind '${failure?.kind}', not one of ${[...FAILURE_KINDS].join(", ")}`);
  check(SEVERITIES.has(failure?.severity),
    `A7: ${band.render} declares severity '${failure?.severity}', not one of ${[...SEVERITIES].join(", ")}`);
  check(Array.isArray(failure?.failing_axes) && failure.failing_axes.length > 0,
    `A7: ${band.render} names no failing axis`);
  check(typeof failure?.what_fails === "string" && failure.what_fails.length > 40,
    `A7: ${band.render} does not say what fails`);
}
const frameProblems = frameFile.renders.filter((r) =>
  r.document_overflow || r.clipped.length || r.out_of_frame.length);
const unregistered = frameProblems.filter((r) => !invalidated.has(r.render));
check(unregistered.length === 0,
  `A7: ${unregistered.length} render(s) clip content or draw outside their own frame and are not registered `
  + `as invalidated: ${unregistered.map((r) => r.render).join(", ")}`);
// An invalidation the probe does not corroborate is either a stale exemption or
// a judgement the machine cannot make. The second is legitimate and has to say
// so; the first is how an exemption quietly starts excusing a clean render.
const staleInvalidations = [...invalidated.values()]
  .filter((band) => band.machine_visible !== false && !frameProblems.some((r) => r.render === band.render))
  .map((band) => band.render);
check(staleInvalidations.length === 0,
  `A7: ${staleInvalidations.join(", ")} claims to be machine-visible but the frame probe is clean for it; `
  + "a band the probe cannot see must record machine_visible: false");
for (const band of invalidated.values()) {
  if (band.machine_visible !== false) continue;
  check(frameProblems.every((r) => r.render !== band.render),
    `A7: ${band.render} records machine_visible: false but the frame probe does flag it`);
  check(band.failure?.kind === "composition",
    `A7: ${band.render} is invisible to the probe, so its failure must be a composition judgement, `
    + `not '${band.failure?.kind}'`);
}
const framedRenders = new Set(frameFile.renders.map((r) => r.render));
check(textFile.renders.every((t) => framedRenders.has(t.render)) && framedRenders.size === textFile.renders.length,
  "A7: the frame probe did not cover exactly the renders the visible-text capture covered");
const bySeverity = (level) => [...invalidated.values()].filter((b) => b.failure?.severity === level).length;
notes.push(`A7 frame containment: ${frameFile.renders.length} renders probed, `
  + `${frameProblems.length} with clipped or out-of-frame content, all ${frameProblems.length ? "registered" : "clean"}; `
  + `${invalidated.size} band(s) inadmissible, every one with a demonstrated failure `
  + `(${bySeverity("disqualifying")} disqualifying, ${bySeverity("material")} material, ${bySeverity("minor")} minor)`);

// ------------------------------------------------------- 11. source contracts
const canonicalStudyRoot = await realpath(studyRoot);
const resolvesInside = async (rel, href) => {
  const target = await realpath(resolve(studyRoot, dirname(rel), href)).catch(() => null);
  const stats = target === null ? null : await stat(target).catch(() => null);
  return Boolean(stats?.isFile()) && isContained(canonicalStudyRoot, target, path);
};
for (const rel of [...CANDIDATES, ...BASELINES, "board.html"]) {
  const html = await read(rel);
  check(/<html lang="/.test(html), `${rel}: missing lang`);
  check(/<title>/.test(html), `${rel}: missing title`);
  check(/<h1[ >]/.test(html), `${rel}: missing h1`);
  check(/name="viewport"/.test(html), `${rel}: missing viewport meta`);
  check(!/(https?:)?\/\/(?!www\.w3\.org\/2000\/svg)[a-z0-9]/i.test(html), `${rel}: contains a remote URL`);
  check(!/\b(fetch|XMLHttpRequest|WebSocket)\s*\(/.test(html), `${rel}: contains a network call`);
  // "Relative" is not the same as "inside": `./../../AGENTS.md` is relative. Each
  // script and stylesheet is resolved canonically and must land on a regular
  // file inside the canonical study root.
  for (const src of [...html.matchAll(/<script[^>]+src=["']([^"']+)["']/g)].map((m) => m[1])) {
    check(await resolvesInside(rel, src), `${rel}: script ${src} does not resolve to a file inside the study`);
  }
  for (const href of [...html.matchAll(/<link[^>]+href=["']([^"']+)["']/g)].map((m) => m[1])) {
    check(await resolvesInside(rel, href), `${rel}: stylesheet ${href} does not resolve to a file inside the study`);
  }
  if (CANDIDATES.includes(rel)) {
    check(/prefers-color-scheme: dark/.test(html), `${rel}: no dark-mode handling`);
    check(/@media \(max-width/.test(html), `${rel}: no narrow-viewport adaptation`);
    check(/dataset\.answer/.test(html), `${rel}: publishes no derived answer to the machine channel`);
  }
}
// A6 — every sibling really renders from its case's one shared source.
for (const [caseId, file] of Object.entries(SHARED_SOURCE)) {
  for (const rel of [...PAGES_OF_CASE[caseId], `baselines/${caseId}/baseline.html`]) {
    check((await read(rel)).includes(file.replace("shared/", "")),
      `${rel}: does not render from the shared ${caseId} subject source ${file}`);
  }
}
notes.push(`source contracts: ${CANDIDATES.length} candidates and ${BASELINES.length} baselines carry lang, title, `
  + `h1 and viewport, load nothing outside the study, and render from their case's one shared source`);

// -------------------------------------------------- 12. render + check coverage
const slugOf = (page) => page.replace(/^cases\//, "").replace(/\/index\.html$/, "")
  .replace(/^baselines\//, "").replace(/\/baseline\.html$/, "-baseline").replace(/\//g, "-");
const viewports = Object.keys(registry.contexts.viewports);
const expectedRenders = new Set([
  ...CANDIDATES.flatMap((p) => registry.contexts.modes.flatMap((m) => viewports.map((v) => `${slugOf(p)}-${m}-${v}.png`))),
  ...BASELINES.flatMap((p) => viewports.map((v) => `${slugOf(p)}-light-${v}.png`)),
]);
const actualRenders = new Set((await readdir(join(studyRoot, "renders"))).filter((n) => n !== ".DS_Store"));
for (const name of expectedRenders) check(actualRenders.has(name), `missing render ${name}`);
for (const name of actualRenders) check(expectedRenders.has(name), `unexpected render present: ${name}`);
const expectedChecks = new Set(["axe.json", "network.json", "answers.json", "lifecycle.json", "text.json",
  "motion.json", "frame.json"]);
const actualChecks = new Set(await readdir(join(studyRoot, "checks")));
for (const name of expectedChecks) check(actualChecks.has(name), `missing check record ${name}`);
for (const name of actualChecks) check(expectedChecks.has(name), `unexpected check record present: ${name}`);
notes.push(`renders: ${actualRenders.size} present across ${viewports.join("/")} in `
  + `${registry.contexts.modes.join("/")}, ${expectedRenders.size} expected, none unexpected`);

// ---------------------------------------------------------- 13. machine evidence
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
notes.push(`lifecycle: ${lifecycle.owned_process_identities} owned identities, 0 survivors, cleanup proven, root absent`);
notes.push(`axe: ${axeReport.results.length} runs clean on Chrome ${axeReport.browser_version}, `
  + `playwright-core ${axeReport.playwright_core_version}, axe-core ${axeReport.axe_core_version}`);

// -------------------------------------------------------------- 14. checksums
let inventory = [];
const walk = async (dir) => {
  for (const entry of (await readdir(join(studyRoot, dir), { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const rel = `${dir}/${entry.name}`;
    if (entry.isDirectory()) await walk(rel);
    else inventory.push(rel);
  }
};
for (const dir of ["baselines", "cases", "checks", "renders", "shared", "tools"]) await walk(dir);
inventory.push(...DOCS);
// The lifecycle record names the actual temporary root the run owned, which is
// a fresh path every time. Digesting it would make every second run look like
// drift and would quietly weaken the claim that matters: that the renders and
// every authored source are byte-identical between consecutive runs. Its
// content is checked in full in section 13 instead, so nothing goes unverified
// — only the verification method changes for this one file.
const VOLATILE = new Set(["checks/lifecycle.json"]);
const volatile = inventory.filter((rel) => VOLATILE.has(rel));
inventory = inventory.filter((rel) => !VOLATILE.has(rel));
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
    process.stdout.write(`\nrefusing to refresh SHA256SUMS: ${failures.length} check(s) failed and the recorded `
      + `inventory is left byte-identical\n`);
    process.exit(1);
  }
  await writeFile(join(studyRoot, "SHA256SUMS"), `${lines.join("\n")}\n`);
  notes.push(`SHA256SUMS rewritten for ${lines.length} files (--update, after all other checks passed); `
    + `${volatile.length} run-scoped record(s) verified by content instead: ${volatile.join(", ")}`);
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
  if (!failures.length) {
    notes.push(`SHA256SUMS: ${current.size} files match byte for byte across runs; `
      + `${volatile.length} run-scoped record(s) verified by content instead: ${volatile.join(", ")}`);
  }
}

// ------------------------------------------------------------------- report
notes.forEach((n) => process.stdout.write(`ok   ${n}\n`));
failures.forEach((f) => process.stdout.write(`FAIL ${f}\n`));
process.stdout.write(failures.length
  ? `\n${failures.length} check(s) failed\n`
  : "\nall W3 integrity and admissibility checks passed — a clean run means the observation gates are "
    + "ADMISSIBLE, not that any composition communicates (observer-state.md)\n");
process.exitCode = failures.length ? 1 : 0;

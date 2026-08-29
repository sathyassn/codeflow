// Deterministic self-tests for the W3 harness contracts that a healthy run
// never reaches.
//
//   A  post-close survivor      — a task-owned child outliving a successful
//      close triggers exact-owned TERM/KILL, fails the run, leaves no process,
//      and the root is not deleted while it is live.
//   B  file: containment        — traversal, percent-encoded traversal, an
//      absolute system path and a sibling-prefix path are all rejected.
//   B2 symlink escape           — a link inside the study pointing outside it is
//      rejected by canonical real-path containment (POSIX hosts).
//   C  update guard             — verify.mjs --update never refreshes the
//      inventory while an earlier check has failed.
//   D  unprovable cleanup       — when process inventory fails at the proof
//      boundary, the marked root is retained and the run fails.
//   E  output-root ownership    — an unmarked root, a wrong-token root and a
//      marked non-temp root are all refused, with canaries left byte-identical.
//   F  containment unit table   — pure path semantics, including the Windows
//      drive and UNC cases this host cannot execute.
//   G  shared-source sandbox    — a subject source cannot reach Node globals,
//      generate code, import a module, hang the verifier, leak into the host
//      realm, or lie about its own snapshot.
//   H  source authority (I/O)   — only a canonical regular file inside one
//      named root is readable; traversal, absolute paths, symlink escape,
//      directories, missing files and empty files are each refused by reason.
//   I  declared-value tables    — the pure lexical path and revision contracts,
//      including the Windows spellings this host cannot execute.
//   J  verifier wiring          — verify.mjs really applies G/H/I.
//   K  admissibility wiring     — NEW IN W3, and the reason this section exists:
//      every gate added because W2's board was inadmissible is itself attacked.
//      A candidate that states its answer where a reader can see it, a sibling
//      pair that shares unit and axis, a breakpoint pair the tablet frame does
//      not fall between, a carrier claim naming a block type the live schema
//      does not have, a declared motion, a state whose non-colour channel is not
//      in the page, and a board wrapper that has drifted from the registry must
//      each fail the real verifier with their own reason. A gate nobody has
//      tried to break is a gate nobody has tested.
//
// Every test writes only inside its own owner-only temporary root (or, for E3,
// a self-created directory inside the study that it removes again). The only
// process ever signalled is the exact pid render.mjs fingerprinted; no negative
// pid and no process-group signal is used anywhere, so nothing can reach this
// runner or the shell that launched it.

import { createHash, randomUUID } from "node:crypto";
import { chmod, cp, mkdtemp, mkdir, readFile, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isContained, CONTAINMENT_CASES } from "./path-containment.mjs";
import {
  DECLARED_PATH_CASES, REVISION_CASES, classifyDeclaredPath, evaluateSharedSource,
  isQualifiedRevision, literalPathspec, readInsideRoot,
} from "./source-authority.mjs";

const studyRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// Sections are selectable — `--only=G,H,I,J,K` — because A, B, B2, D, E and E4
// drive render.mjs, which needs process inventory, signal delivery and a profile
// socket directory that a restrictive host may deny. A selected run says so in
// its own summary; it never implies the unselected sections passed.
const only = (process.argv.find((a) => a.startsWith("--only=")) ?? "").slice("--only=".length)
  .split(",").map((s) => s.trim().toUpperCase()).filter(Boolean);
const runs = (section) => only.length === 0 || only.includes(section);
const repoRoot = resolve(studyRoot, "../../..");
const render = join(studyRoot, "tools/render.mjs");
const PROBE_PAGE = "cases/p1/a-critical-path-and-room/index.html";
const failures = [];
const notes = [];
const check = (ok, message) => { (ok ? notes : failures).push(message); return ok; };
const digest = async (p) => createHash("sha256").update(await readFile(p)).digest("hex");
const tail = (text, n = 400) => (text ?? "").slice(-n).trim();

// Every subprocess result is inspected: a spawn error or a null status (killed
// by signal, or timed out) is a test failure with its diagnostics attached.
function runNode(label, args, env = {}, expect = {}) {
  const result = spawnSync(process.execPath, args, {
    encoding: "utf8", env: { ...process.env, ...env }, timeout: 300_000,
    // detached: false means the child DOES share this process group on POSIX.
    // That is fine: safety here comes from render.mjs only ever signalling an
    // exact, freshly fingerprinted pid — never a negative pid, never a process
    // group — so no signal can reach this runner or the parent shell.
    detached: false, stdio: ["ignore", "pipe", "pipe"],
  });
  const diagnostics = `status=${result.status} signal=${result.signal ?? "none"} `
    + `error=${result.error ? result.error.message : "none"}\n  stdout…${tail(result.stdout)}\n  stderr…${tail(result.stderr)}`;
  if (result.error) failures.push(`${label}: subprocess error — ${diagnostics}`);
  else if (result.status === null) failures.push(`${label}: subprocess did not exit normally — ${diagnostics}`);
  else if (expect.status !== undefined && result.status !== expect.status) {
    failures.push(`${label}: expected exit ${expect.status}, got — ${diagnostics}`);
  }
  return { ...result, diagnostics };
}

async function markedRoot(prefix) {
  const dir = await mkdtemp(join(await realpath(tmpdir()), prefix));
  await chmod(dir, 0o700);
  const token = randomUUID();
  await writeFile(join(dir, ".cf-w3-selftest-root"), `${token}\n`, { mode: 0o600 });
  return { dir, token };
}
const liveCommandsContaining = (needle) => {
  if (process.platform === "win32") return [];
  const raw = execFileSync("/bin/ps", ["-axo", "pid=,command="], { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  return raw.split("\n").filter((line) => line.includes(needle));
};

// ------------------------------------------------- A. post-close survivor path
if (runs("A")) {
  const { dir, token } = await markedRoot("cf-w3-selftest-a-");
  const result = runNode("A", [render, "--inject-post-close-survivor",
    "--page", PROBE_PAGE, "--output-root", dir, "--output-token", token], {}, { status: 1 });
  const lifecycle = JSON.parse(await readFile(join(dir, "checks/lifecycle.json"), "utf8"));
  check(lifecycle.survivor_injected === true, "A: the injected task-owned survivor was captured as owned");
  check(lifecycle.close_fallback_used === true, "A: a survivor outliving a successful close triggers the fallback");
  check(lifecycle.surviving_process_identities === 0, "A: no task-owned identity remains after the fallback");
  check(lifecycle.cleanup_proven === true, "A: cleanup is positively proven before the root is removed");
  check(lifecycle.task_root_removed === true && lifecycle.task_root_state === "absent",
    "A: the root is removed once nothing task-owned is left");
  check(!(lifecycle.surviving_process_identities > 0 && lifecycle.task_root_removed === true),
    "A: the root is never removed while a task-owned identity is still live");
  check(liveCommandsContaining(lifecycle.task_root).length === 0, "A: the injected survivor is gone");
  check(/outlived the close/.test(result.stderr), "A: the survivor path is reported, not absorbed");
  await rm(dir, { recursive: true, force: true });
}

// ---------------------------------------------------- B. file: containment
if (runs("B")) {
  const { dir, token } = await markedRoot("cf-w3-selftest-b-");
  const result = runNode("B", [render, "--page", "tools/fixtures/outside-file-probe.html",
    "--output-root", dir, "--output-token", token], {}, { status: 1 });
  const network = JSON.parse(await readFile(join(dir, "checks/network.json"), "utf8"));
  const outside = network.outside_study_file_requests;
  const canonicalStudy = await realpath(studyRoot);
  check(outside.length >= 4, `B: every outside-study probe was aborted and recorded (${outside.length})`);
  check(network.remote_requests.length === 0, "B: outside-study reads are recorded distinctly from remote requests");
  check(outside.every((r) => !r.canonical || !isContained(canonicalStudy, r.canonical, path)),
    "B: no recorded request actually resolves inside the canonical study root");
  check(outside.some((r) => r.url.includes("tsk-014-w3-other")), "B: a sibling-prefix path is rejected");
  check(outside.some((r) => /CLAUDE\.md$/.test(decodeURIComponent(r.url))), "B: percent-encoded traversal is rejected");
  check(outside.some((r) => /\/etc\/hosts$/.test(decodeURIComponent(r.url))), "B: an absolute system path is rejected");
  if (!result.status) failures.push(`B: unexpected success — ${result.diagnostics}`);
  await rm(dir, { recursive: true, force: true });
}

// -------------------------------------------------------- B2. symlink escape
if (runs("B2") && process.platform !== "win32") {
  const { dir, token } = await markedRoot("cf-w3-selftest-b2-");
  const link = join(studyRoot, "tools/fixtures/escape-link.md");
  try {
    await rm(link, { force: true });
    await symlink(join(repoRoot, "AGENTS.md"), link);
    runNode("B2", [render, "--page", "tools/fixtures/symlink-escape-probe.html",
      "--output-root", dir, "--output-token", token], {}, { status: 1 });
    const network = JSON.parse(await readFile(join(dir, "checks/network.json"), "utf8"));
    const record = network.outside_study_file_requests.find((r) => r.url.endsWith("escape-link.md"));
    check(Boolean(record), "B2: a symlink inside the study pointing outside it is rejected");
    check(record?.reason === "outside the canonical study root",
      "B2: the symlink is rejected by canonical real-path containment, not by lexical position");
    check(record?.canonical?.endsWith("AGENTS.md") === true, "B2: the recorded canonical target is the escaped file");
  } finally {
    await rm(link, { force: true });
    await rm(dir, { recursive: true, force: true });
  }
} else if (runs("B2")) {
  notes.push("B2: symlink escape not executed on this host (win32); no claim made");
}

// -------------------------------------------------------- C. update guard
if (runs("C")) {
  const out = await mkdtemp(join(await realpath(tmpdir()), "cf-w3-selftest-c-"));
  const copy = join(out, "study");
  await cp(studyRoot, copy, { recursive: true });
  await writeFile(join(copy, "rubric.v2.md"), `${await readFile(join(copy, "rubric.v2.md"), "utf8")}\ntampered\n`);
  const before = await digest(join(copy, "SHA256SUMS"));
  const result = runNode("C", [join(copy, "tools/verify.mjs"), "--update"], { CF_W3_REPO_ROOT: repoRoot }, { status: 1 });
  check(before === await digest(join(copy, "SHA256SUMS")),
    "C: --update leaves SHA256SUMS byte-identical when the state is invalid");
  check(/refusing to refresh/.test(result.stdout + result.stderr), "C: the refusal to refresh is reported");
  await rm(out, { recursive: true, force: true });
}

// ------------------------------------------------- D. unprovable cleanup path
if (runs("D")) {
  const { dir, token } = await markedRoot("cf-w3-selftest-d-");
  const result = runNode("D", [render, "--inject-inventory-failure",
    "--page", PROBE_PAGE, "--output-root", dir, "--output-token", token], {}, { status: 1 });
  const lifecycle = JSON.parse(await readFile(join(dir, "checks/lifecycle.json"), "utf8"));
  check(lifecycle.inventory_failure_injected === true, "D: the inventory failure was injected at the proof boundary");
  check(lifecycle.cleanup_proven === false, "D: cleanup is not claimed when the inventory cannot be read");
  check(lifecycle.task_root_removed === false, "D: no deletion is attempted when process exit cannot be established");
  check(lifecycle.task_root_state === "present", "D: the marked root is retained as evidence");
  check(existsSync(lifecycle.task_root), "D: the retained root is really still on disk");
  check(lifecycle.cleanup_errors.some((m) => /injected process-inventory failure/.test(m)),
    "D: the inventory failure is recorded as cleanup uncertainty");
  check(/could not be proven/.test(result.stderr), "D: the unproven cleanup is reported");
  // The browser itself closed normally; only the proof failed. Clean the
  // retained root here, and confirm nothing task-owned is left behind.
  check(liveCommandsContaining(lifecycle.task_root).length === 0, "D: no task-owned process was left running");
  await rm(lifecycle.task_root, { recursive: true, force: true });
  await rm(dir, { recursive: true, force: true });
}

// ------------------------------------------ E. output-root ownership contract
if (runs("E")) {
  const canary = "do-not-touch\n";
  const cases = [];

  const unmarked = await mkdtemp(join(await realpath(tmpdir()), "cf-w3-selftest-e1-"));
  await chmod(unmarked, 0o700);
  cases.push({ label: "E1 unmarked root", dir: unmarked, token: randomUUID(), cleanup: unmarked });

  const wrong = await markedRoot("cf-w3-selftest-e2-");
  cases.push({ label: "E2 wrong token", dir: wrong.dir, token: randomUUID(), cleanup: wrong.dir });

  const nonTemp = join(studyRoot, `.selftest-nontemp-${randomUUID()}`);
  await mkdir(nonTemp, { recursive: true, mode: 0o700 });
  const nonTempToken = randomUUID();
  await writeFile(join(nonTemp, ".cf-w3-selftest-root"), `${nonTempToken}\n`, { mode: 0o600 });
  cases.push({ label: "E3 marked but not under temp", dir: nonTemp, token: nonTempToken, cleanup: nonTemp });

  try {
    for (const testCase of cases) {
      const canaryPath = join(testCase.dir, "canary.txt");
      await writeFile(canaryPath, canary);
      const before = await digest(canaryPath);
      const result = runNode(testCase.label,
        [render, "--page", PROBE_PAGE,
          "--output-root", testCase.dir, "--output-token", testCase.token], {}, { status: 2 });
      check(/^REFUSED/mu.test(result.stderr), `${testCase.label}: refused before any mutation`);
      check(before === await digest(canaryPath), `${testCase.label}: the foreign canary is byte-identical`);
      check(!existsSync(join(testCase.dir, "renders")) && !existsSync(join(testCase.dir, "checks")),
        `${testCase.label}: no output directory was created`);
    }
  } finally {
    for (const testCase of cases) await rm(testCase.cleanup, { recursive: true, force: true });
  }
}

// ------------------------------- E4. unexpected file blocks any known deletion
if (runs("E4")) {
  const { dir, token } = await markedRoot("cf-w3-selftest-e4-");
  // A known owned output in one directory, an unexpected sentinel in the other:
  // a single-pass reset would delete the known render before it ever reached
  // the sentinel in checks/.
  await mkdir(join(dir, "renders"), { recursive: true });
  await mkdir(join(dir, "checks"), { recursive: true });
  const known = join(dir, "renders/p1-a-critical-path-and-room-light-desktop.png");
  const sentinel = join(dir, "checks/sentinel.json");
  await writeFile(known, "known-owned-output\n");
  await writeFile(sentinel, "{\"do\":\"not touch\"}\n");
  const knownBefore = await digest(known);
  const sentinelBefore = await digest(sentinel);

  const result = runNode("E4 unexpected file present",
    [render, "--page", PROBE_PAGE, "--output-root", dir, "--output-token", token], {}, { status: 1 });
  check(/nothing was deleted/.test(result.stderr), "E4: the refusal states that nothing was deleted");
  check(/checks\/sentinel\.json/.test(result.stderr), "E4: the unexpected file is named in the refusal");
  check(knownBefore === await digest(known),
    "E4: a known owned output in the other directory is byte-identical after the refusal");
  check(sentinelBefore === await digest(sentinel), "E4: the unexpected sentinel is byte-identical");
  await rm(dir, { recursive: true, force: true });
}

// ------------------------------------------------ F. containment unit table
if (runs("F")) {
  let bad = 0;
  for (const testCase of CONTAINMENT_CASES) {
    const flavor = testCase.flavor === "win32" ? path.win32 : path.posix;
    if (isContained(testCase.root, testCase.target, flavor) !== testCase.contained) {
      failures.push(`F: ${testCase.flavor} containment wrong for ${testCase.why} (${testCase.root} vs ${testCase.target})`);
      bad += 1;
    }
  }
  check(bad === 0, `F: ${CONTAINMENT_CASES.length} containment cases hold, including win32 drive and UNC semantics `
    + "(evaluated as pure path logic; native Windows runtime was not exercised)");
}

// ------------------------------------------- G. shared-source sandbox
if (runs("G")) {
  const refused = (label, code, expect, globalName = "probe") => {
    const result = evaluateSharedSource(code, globalName, { timeoutMs: 2_000, filename: "probe.js" });
    if (result.ok) { failures.push(`G: ${label} was NOT refused`); return; }
    check(expect.test(result.reason), `G: ${label} is refused — ${result.reason}`);
  };

  refused("reading a Node global", "window.probe = { pid: process.pid };", /ReferenceError.*process/u);
  refused("calling require", 'window.probe = require("node:fs").readFileSync("/etc/hosts");', /ReferenceError.*require/u);
  refused("eval", 'window.probe = { v: eval("1 + 1") };', /EvalError|not allowed/u);
  refused("the Function constructor", 'window.probe = { v: new Function("return 1")() };', /EvalError|not allowed/u);
  refused("defining nothing", "const x = 1;", /did not define window\.probe/u);
  refused("a non-object global", 'window.probe = "text";', /did not define window\.probe/u);
  refused("a circular structure", "const a = {}; a.self = a; window.probe = a;", /could not be snapshotted/u);

  const imported = evaluateSharedSource(
    "window.probe = { started: true };\n"
    + 'import("node:fs").then((m) => { window.probe.loaded = typeof m.readFileSync; }, () => {});',
    "probe", { timeoutMs: 2_000 });
  check(imported.ok && imported.value.started === true && imported.value.loaded === undefined,
    "G: a dynamic import delivers no module into the snapshot");

  const started = process.hrtime.bigint();
  const looping = evaluateSharedSource("while (true) {} window.probe = {};", "probe", { timeoutMs: 500 });
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
  check(!looping.ok && /timed out|Script execution/iu.test(looping.reason),
    `G: an infinite loop is refused by the wall clock — ${looping.reason}`);
  check(elapsedMs < 10_000, `G: the infinite loop returned in ${Math.round(elapsedMs)}ms, inside its bound`);

  const leak = evaluateSharedSource(
    'globalThis.__cfLeaked = "escaped"; window.probe = { ok: true };', "probe", { timeoutMs: 2_000 });
  check(leak.ok, "G: a source that writes its own context global still loads");
  check(globalThis.__cfLeaked === undefined, "G: a context global does not appear on the host globalThis");

  const shaped = evaluateSharedSource(
    'window.probe = { a: [1, 2], f: () => 1, m: new Map([["k", "v"]]) };', "probe", { timeoutMs: 2_000 });
  check(shaped.ok && shaped.value.f === undefined && JSON.stringify(shaped.value.m) === "{}"
    && JSON.stringify(shaped.value.a) === "[1,2]",
    "G: the snapshot carries plain data only — functions and Maps do not cross back");
  const lying = evaluateSharedSource(
    "JSON.stringify = () => JSON.stringify; window.probe = { real: true };", "probe", { timeoutMs: 2_000 });
  check(lying.ok && lying.value.real === true,
    "G: replacing JSON.stringify inside the context does not change the snapshot");
  const trapped = evaluateSharedSource(
    "window.probe = { get boom() { while (true) {} } };", "probe", { timeoutMs: 500 });
  check(!trapped.ok, `G: a looping getter is bounded at snapshot time, not on host property access — ${trapped.reason}`);
}

// ------------------------------------------------- H. source authority (I/O)
if (runs("H")) {
  const base = await mkdtemp(join(await realpath(tmpdir()), "cf-w3-selftest-h-"));
  const root = join(base, "root");
  const outside = join(base, "outside");
  try {
    await mkdir(join(root, "sub"), { recursive: true });
    await mkdir(outside, { recursive: true });
    await writeFile(join(root, "ok.md"), "a real sentence inside the root\n");
    await writeFile(join(root, "empty.md"), "   \n");
    await writeFile(join(outside, "secret.md"), "a sentence outside the root\n");
    let symlinksExercised = false;
    if (process.platform !== "win32") {
      await symlink(join(outside, "secret.md"), join(root, "escape.md"));
      await symlink(join(root, "ok.md"), join(root, "inside-link.md"));
      symlinksExercised = true;
    }

    const attempt = async (rel) => readInsideRoot(root, rel);
    const good = await attempt("ok.md");
    check(good.ok && good.raw.includes("inside the root"), "H: a canonical regular file inside the root is read");

    const cases = [
      ["../outside/secret.md", /walks upward/u, "traversal out of the root"],
      [join(outside, "secret.md"), /absolute path/u, "an absolute path"],
      ["sub", /not a regular file/u, "a directory"],
      ["missing.md", /could not be resolved on disk/u, "a missing file"],
      ["empty.md", /is empty/u, "an empty file"],
      [":(glob)**", /pathspec magic/u, "pathspec magic"],
      ["--output=/tmp/x", /git would read as an option/u, "an option-shaped path"],
    ];
    for (const [rel, pattern, why] of cases) {
      const result = await attempt(rel);
      if (result.ok) { failures.push(`H: ${why} was NOT refused (${rel})`); continue; }
      check(pattern.test(result.reason), `H: ${why} is refused — ${result.reason}`);
    }

    if (symlinksExercised) {
      const escaped = await attempt("escape.md");
      check(!escaped.ok && /through a link/u.test(escaped.reason),
        `H: a symlink inside the root pointing outside it is refused — ${escaped.reason}`);
      const linked = await attempt("inside-link.md");
      check(linked.ok, "H: a symlink that stays inside the root is still readable, so the rule is containment, not links");
    } else {
      notes.push("H: symlink escape not executed on this host (win32); no claim made");
    }

    const crossRoot = await readInsideRoot(outside, "ok.md");
    check(!crossRoot.ok, `H: a file of the other root is not reachable from this one — ${crossRoot.reason}`);
  } finally {
    await rm(base, { recursive: true, force: true });
  }
}

// --------------------------------------------- I. declared-value unit tables
if (runs("I")) {
  let bad = 0;
  for (const testCase of DECLARED_PATH_CASES) {
    if (classifyDeclaredPath(testCase.value).ok !== testCase.ok) {
      failures.push(`I: declared-path qualification wrong for ${testCase.why} (${String(testCase.value)})`);
      bad += 1;
    }
  }
  for (const testCase of REVISION_CASES) {
    if (isQualifiedRevision(testCase.value) !== testCase.ok) {
      failures.push(`I: revision qualification wrong for ${testCase.why} (${String(testCase.value)})`);
      bad += 1;
    }
  }
  if (bad === 0) {
    notes.push(`I: ${DECLARED_PATH_CASES.length} declared-path and ${REVISION_CASES.length} revision cases hold, `
      + "including the Windows spellings this host cannot execute");
  }
  check(literalPathspec("a/b.mjs") === ":(literal,top)a/b.mjs",
    "I: a qualified path reaches git pinned to a literal, root-relative pathspec");
}

// -------------------------------------- shared tamper harness for J and K
// One disposable copy of the study, tampered one way at a time, run through the
// real verify.mjs. A scenario whose anchor has moved is a failure, not a skip:
// a test that silently stops testing is worse than no test.
async function tamperScenarios(label, scenarios, controlExpect) {
  const out = await mkdtemp(join(await realpath(tmpdir()), `cf-w3-selftest-${label.toLowerCase()}-`));
  const copy = join(out, "study");
  try {
    await cp(studyRoot, copy, { recursive: true });
    const pristine = new Map();
    for (const scenario of scenarios) {
      const target = join(copy, scenario.file);
      if (!pristine.has(scenario.file)) pristine.set(scenario.file, await readFile(target, "utf8"));
      const original = pristine.get(scenario.file);
      if (!original.includes(scenario.from)) {
        failures.push(`${label}: the fixture anchor for “${scenario.label}” is gone; the scenario proves nothing`);
        continue;
      }
      await writeFile(target, original.replace(scenario.from, scenario.to));
      const result = runNode(`${label} ${scenario.label}`, [join(copy, "tools/verify.mjs")],
        { CF_W3_REPO_ROOT: repoRoot }, { status: 1 });
      check(scenario.expect.test(result.stdout + result.stderr),
        `${label}: ${scenario.label} fails the verifier with its own reason`);
      await writeFile(target, original);
    }

    // Control. A copy can never pass outright — the recorded file: containment
    // root belongs to the real study, so that one check always fails here — but
    // it must report none of the reasons above, which is what shows each
    // refusal came from the tampering and not from the harness.
    const control = runNode(`${label} restored copy`, [join(copy, "tools/verify.mjs")], { CF_W3_REPO_ROOT: repoRoot });
    const controlOutput = control.stdout + control.stderr;
    check(!scenarios.some((scenario) => scenario.expect.test(controlOutput)),
      `${label}: the restored copy reports none of those reasons, so each refusal was the tampering`);
    if (controlExpect) {
      check(controlExpect.test(controlOutput),
        `${label}: the restored copy still binds its sources, so the fixtures were left exactly as found`);
    }
    return { copy, out };
  } finally {
    await rm(out, { recursive: true, force: true });
  }
}

// ------------------------------------------------------- J. verifier wiring
if (runs("J")) {
  const writeTarget = join(await realpath(tmpdir()), `cf-w3-git-should-never-write-${randomUUID()}`);
  const file = "shared/p3-convergence.js";
  const recordLine = 'const record = "project-management/tasks/TSK-014.md";';
  const harnessLine = 'const harness = "crates/codeflow-present/web/scripts/real-browser-check.mjs";';
  await tamperScenarios("J", [
    { file, label: "a source escaping the repository root",
      from: recordLine, to: 'const record = "../../../../../../etc/hosts";',
      expect: /declared repository source \.\.\/.*walks upward/u },
    { file, label: "a repository claim pointed at a study file",
      from: recordLine, to: 'const record = "answer-key.md";',
      expect: /declared repository source answer-key\.md could not be resolved on disk/u },
    { file, label: "an option as a revision",
      from: '{ id: "R1", sha: "86582dc0",', to: `{ id: "R1", sha: "--output=${writeTarget}",`,
      expect: /is not an object id/u },
    { file, label: "pathspec magic as a diff path",
      from: harnessLine, to: 'const harness = ":(glob)**";',
      expect: /declared diff path .* starts with ':'/u },
    { file, label: "a source reaching for a Node global",
      from: "window.p3Convergence = (() => {",
      to: "window.p3Convergence = (() => { globalThis.stolen = process.env;",
      expect: /did not evaluate inside the bounded context/u },
    { file, label: "a source importing a module",
      from: "window.p3Convergence = (() => {",
      to: 'import("node:fs");\nwindow.p3Convergence = (() => {',
      expect: /dynamic import|DYNAMIC_IMPORT/u },
  ], /P3 convergence: 8 rounds matched against git log/);
  check(!existsSync(writeTarget), "J: no revision reached git, so nothing was written by an injected git option");
}

// -------------------------------------------------- K. admissibility wiring
// Every gate that exists because W2's board was inadmissible, attacked once.
if (runs("K")) {
  // A1 reads the text captured off a render, not the page source, because the
  // question is what a reader actually saw. So the tampering goes there. A page
  // edited without re-rendering is caught by the checksum instead, and the
  // render/verify cycle re-captures the text every time.
  const captured = "checks/text.json";
  await tamperScenarios("K", [
    // A1 — the failure that voided W2's comprehension result, reproduced.
    { file: captured, label: "a candidate stating its answer in visible prose",
      from: "EPC-005 · PRESENTATION SYSTEM\\n\\nThe work that is not finished yet",
      to: "EPC-005 · PRESENTATION SYSTEM\\n\\nTSK-014 → TSK-011 → TSK-007 → TSK-010\\n\\nThe work that is not finished yet",
      expect: /A1 VOID .*visible text matches/u },
    { file: captured, label: "a candidate using a registered forbidden phrase",
      from: "The work that is not finished yet",
      to: "The work that is not finished yet, and two tasks can wait",
      expect: /A1 VOID .*forbidden phrase “can wait”/u },
    // A2 — sibling distinctness, which W2 discovered only after both were built.
    { file: "registry.json", label: "two siblings sharing unit and axis",
      from: '"primary_unit": "a whole path through the unfinished graph to its sink",\n      "primary_axis": "path length in tasks",',
      to: '"primary_unit": "an unfinished task",\n      "primary_axis": "wave position — the earliest wave the task can occupy, left to right",',
      expect: /share both primary unit and primary axis/u },
    // A3 — the intermediate context W2 never rendered.
    { file: "registry.json", label: "a tablet width outside a candidate's own breakpoints",
      from: '"breakpoints": { "wide": 1080, "narrow": 640 },\n      "intermediate_composition": "the single field splits',
      to: '"breakpoints": { "wide": 880, "narrow": 640 },\n      "intermediate_composition": "the single field splits',
      expect: /does not fall strictly between its declared breakpoints/u },
    // A4 — carrier feasibility, bound to the live contract rather than asserted.
    { file: "registry.json", label: "a carrier naming a block type the schema does not have",
      from: '"native_blocks_considered": ["table", "status", "diagram"],',
      to: '"native_blocks_considered": ["table", "status", "sparkline"],',
      expect: /names block type 'sparkline', which is not in the live document schema/u },
    // A5 — motion, which W2 "evidenced" with a duplicate screenshot.
    { file: "registry.json", label: "a candidate declaring motion with no evidence path",
      from: '"loss_is_material": true\n      },\n      "motion": "none"\n    },\n    {\n      "id": "b-paths-to-the-sink"',
      to: '"loss_is_material": true\n      },\n      "motion": "fade"\n    },\n    {\n      "id": "b-paths-to-the-sink"',
      expect: /declares motion 'fade'/u },
    // The non-colour channel behind every state has to be real.
    { file: "registry.json", label: "a state whose non-colour channel is not in the page",
      from: '{ "name": "on the unbroken run", "non_colour_marker": "stroke-width" }',
      to: '{ "name": "on the unbroken run", "non_colour_marker": "font-variant-emoji" }',
      expect: /declares the non-colour marker .*which is not in the page/u },
    // One authority for the registry: the board cannot drift from it.
    { file: "shared/registry.js", label: "a board wrapper drifting from the registry",
      from: '"study": "tsk-014-w3"', to: '"study": "tsk-014-w3-DRIFTED"',
      expect: /shared\/registry\.js is not registry\.json byte for byte/u },
    // The W2 rubric is not this study's to touch.
    { file: "rubric.v2.md", label: "an edited v2 rubric",
      from: "# Pre-registered comparison rubric — version 2",
      to: "# Pre-registered comparison rubric — version 2 (edited)",
      expect: /rubric\.v2\.md no longer matches its lock/u },
    // A7 — a render that clips its own content and is not registered as
    // invalidated. This is the gate every other check passed straight through.
    { file: "registry.json", label: "a clipping render that is not registered as invalidated",
      from: '"render": "p3-b-source-region-history-light-mobile",',
      to: '"render": "p3-b-source-region-history-light-mobile-NOT-THIS-ONE",',
      expect: /A7: \d+ render\(s\) clip content or draw outside their own frame and are not registered/u },
    // A7 — an invalidation that no longer matches anything, which is how a
    // stale exemption quietly starts excusing a render that is now clean.
    { file: "registry.json", label: "an invalidation the frame probe does not corroborate",
      from: '"render": "p2-baseline-light-mobile",',
      to: '"render": "p1-b-paths-to-the-sink-light-desktop",',
      expect: /claims to be machine-visible but the frame probe is clean for it/u },
    // A8 — the registration reasserting itself instead of accounting for itself.
    { file: "registry.json", label: "a registry that drops its amendments record",
      from: '"amendments": [', to: '"amendments_removed": [',
      expect: /A8: registry\.json carries no amendments array/u },
    { file: "registry.json", label: "an amendment with no reason recorded",
      from: '"why_it_matters": "A3 is only a constraint on a composition if the declaration precedes it. Rewritten to match the drawing, this field describes rather than constrains — and the drawing',
      to: '"why_it_matters_removed": "A3 is only a constraint on a composition if the declaration precedes it. Rewritten to match the drawing, this field describes rather than constrains — and the drawing',
      expect: /A8: amendment p1a-narrow-composition is missing why_it_matters/u },
    // A8 — an established amendment quietly dropped. This is the one the audit's
    // own first correction would have failed: it recorded one field change where
    // two had happened, and called the prior wording unrecoverable.
    { file: "registry.json", label: "a locked amendment dropped from the record",
      from: '"id": "p1a-intermediate-composition",', to: '"id": "p1a-intermediate-composition-renamed",',
      expect: /A8: locked amendment p1a-intermediate-composition is not in registry\.json/u },
    { file: "registry.json", label: "a locked amendment's prior wording quietly reworded",
      from: '"before": "the wave axis rotates to vertical: waves become rows down the page',
      to: '"before": "the wave axis rotates to vertical: waves become ROWS down the page',
      expect: /A8: locked amendment p1a-narrow-composition has a before wording that does not match its lock/u },
    { file: "registry.json", label: "a locked amendment whose 'after' is no longer the live field",
      from: '"narrow_composition": "the axis rotates. Waves become rows down the page and the run descends',
      to: '"narrow_composition": "the axis rotates. Waves become rows down the PAGE and the run descends',
      expect: /A8: amendment p1a-narrow-composition records an 'after' wording that is not/u },
    // A7 — an invalidated band that hides its failure behind the word.
    { file: "registry.json", label: "an invalidated band with no demonstrated failure",
      from: '"demonstrated": true,', to: '"demonstrated": false,',
      expect: /A7: .* is registered invalidated without a demonstrated failure/u },
    // A8 — the standing of the registration cannot be quietly dropped either.
    { file: "registry.json", label: "a registry that drops what its priority is worth",
      from: '"what_the_files_cannot_show": "Nothing about this file.',
      to: '"what_the_files_cannot_show": "n/a", "unused": "Nothing about this file.',
      expect: /A8: pre_registration_evidence\.what_the_files_cannot_show is missing or too thin/u },
  ], /A1 answer channel: \d+ candidate renders scanned/);
}

if (only.length) {
  const skipped = ["A", "B", "B2", "C", "D", "E", "E4", "F", "G", "H", "I", "J", "K"]
    .filter((section) => !runs(section));
  notes.push(`sections ${only.join(", ")} selected; ${skipped.join(", ")} NOT run and therefore not claimed`);
}
notes.forEach((n) => process.stdout.write(`ok   ${n}\n`));
failures.forEach((f) => process.stdout.write(`FAIL ${f}\n`));
process.stdout.write(failures.length
  ? `\n${failures.length} self-test check(s) failed\n`
  : "\nall W3 harness self-tests passed\n");
process.exitCode = failures.length ? 1 : 0;

// Deterministic self-tests for the W2 harness contracts that a healthy run
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

const studyRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(studyRoot, "../../..");
const render = join(studyRoot, "tools/render.mjs");
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
  const diagnostics = `status=${result.status} signal=${result.signal ?? "none"} ` +
    `error=${result.error ? result.error.message : "none"}\n  stdout…${tail(result.stdout)}\n  stderr…${tail(result.stderr)}`;
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
  await writeFile(join(dir, ".cf-w2-selftest-root"), `${token}\n`, { mode: 0o600 });
  return { dir, token };
}
const liveCommandsContaining = (needle) => {
  if (process.platform === "win32") return [];
  const raw = execFileSync("/bin/ps", ["-axo", "pid=,command="], { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  return raw.split("\n").filter((line) => line.includes(needle));
};

// ------------------------------------------------- A. post-close survivor path
{
  const { dir, token } = await markedRoot("cf-w2-selftest-a-");
  const result = runNode("A", [render, "--inject-post-close-survivor",
    "--page", "cases/p1/c-critical-ribbon/index.html", "--output-root", dir, "--output-token", token], {}, { status: 1 });
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
{
  const { dir, token } = await markedRoot("cf-w2-selftest-b-");
  const result = runNode("B", [render, "--page", "tools/fixtures/outside-file-probe.html",
    "--output-root", dir, "--output-token", token], {}, { status: 1 });
  const network = JSON.parse(await readFile(join(dir, "checks/network.json"), "utf8"));
  const outside = network.outside_study_file_requests;
  const canonicalStudy = await realpath(studyRoot);
  check(outside.length >= 4, `B: every outside-study probe was aborted and recorded (${outside.length})`);
  check(network.remote_requests.length === 0, "B: outside-study reads are recorded distinctly from remote requests");
  check(outside.every((r) => !r.canonical || !isContained(canonicalStudy, r.canonical, path)),
    "B: no recorded request actually resolves inside the canonical study root");
  check(outside.some((r) => r.url.includes("tsk-014-w2-other")), "B: a sibling-prefix path is rejected");
  check(outside.some((r) => /CLAUDE\.md$/.test(decodeURIComponent(r.url))), "B: percent-encoded traversal is rejected");
  check(outside.some((r) => /\/etc\/hosts$/.test(decodeURIComponent(r.url))), "B: an absolute system path is rejected");
  if (!result.status) failures.push(`B: unexpected success — ${result.diagnostics}`);
  await rm(dir, { recursive: true, force: true });
}

// -------------------------------------------------------- B2. symlink escape
if (process.platform !== "win32") {
  const { dir, token } = await markedRoot("cf-w2-selftest-b2-");
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
} else {
  notes.push("B2: symlink escape not executed on this host (win32); no claim made");
}

// -------------------------------------------------------- C. update guard
{
  const out = await mkdtemp(join(await realpath(tmpdir()), "cf-w2-selftest-c-"));
  const copy = join(out, "study");
  await cp(studyRoot, copy, { recursive: true });
  await writeFile(join(copy, "rubric.md"), `${await readFile(join(copy, "rubric.md"), "utf8")}\ntampered\n`);
  const before = await digest(join(copy, "SHA256SUMS"));
  const result = runNode("C", [join(copy, "tools/verify.mjs"), "--update"], { CF_W2_REPO_ROOT: repoRoot }, { status: 1 });
  check(before === await digest(join(copy, "SHA256SUMS")),
    "C: --update leaves SHA256SUMS byte-identical when the state is invalid");
  check(/refusing to refresh/.test(result.stdout + result.stderr), "C: the refusal to refresh is reported");
  await rm(out, { recursive: true, force: true });
}

// ------------------------------------------------- D. unprovable cleanup path
{
  const { dir, token } = await markedRoot("cf-w2-selftest-d-");
  const result = runNode("D", [render, "--inject-inventory-failure",
    "--page", "cases/p1/c-critical-ribbon/index.html", "--output-root", dir, "--output-token", token], {}, { status: 1 });
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
{
  const canary = "do-not-touch\n";
  const cases = [];

  const unmarked = await mkdtemp(join(await realpath(tmpdir()), "cf-w2-selftest-e1-"));
  await chmod(unmarked, 0o700);
  cases.push({ label: "E1 unmarked root", dir: unmarked, token: randomUUID(), cleanup: unmarked });

  const wrong = await markedRoot("cf-w2-selftest-e2-");
  cases.push({ label: "E2 wrong token", dir: wrong.dir, token: randomUUID(), cleanup: wrong.dir });

  const nonTemp = join(studyRoot, `.selftest-nontemp-${randomUUID()}`);
  await mkdir(nonTemp, { recursive: true, mode: 0o700 });
  const nonTempToken = randomUUID();
  await writeFile(join(nonTemp, ".cf-w2-selftest-root"), `${nonTempToken}\n`, { mode: 0o600 });
  cases.push({ label: "E3 marked but not under temp", dir: nonTemp, token: nonTempToken, cleanup: nonTemp });

  try {
    for (const testCase of cases) {
      const canaryPath = join(testCase.dir, "canary.txt");
      await writeFile(canaryPath, canary);
      const before = await digest(canaryPath);
      const result = runNode(testCase.label,
        [render, "--page", "cases/p1/c-critical-ribbon/index.html",
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
{
  const { dir, token } = await markedRoot("cf-w2-selftest-e4-");
  // A known owned output in one directory, an unexpected sentinel in the other:
  // a single-pass reset would delete the known render before it ever reached
  // the sentinel in checks/.
  await mkdir(join(dir, "renders"), { recursive: true });
  await mkdir(join(dir, "checks"), { recursive: true });
  const known = join(dir, "renders/p1-c-critical-ribbon-light-desktop.png");
  const sentinel = join(dir, "checks/sentinel.json");
  await writeFile(known, "known-owned-output\n");
  await writeFile(sentinel, "{\"do\":\"not touch\"}\n");
  const knownBefore = await digest(known);
  const sentinelBefore = await digest(sentinel);

  const result = runNode("E4 unexpected file present",
    [render, "--page", "cases/p1/c-critical-ribbon/index.html",
      "--output-root", dir, "--output-token", token], {}, { status: 1 });
  check(/nothing was deleted/.test(result.stderr), "E4: the refusal states that nothing was deleted");
  check(/checks\/sentinel\.json/.test(result.stderr), "E4: the unexpected file is named in the refusal");
  check(knownBefore === await digest(known),
    "E4: a known owned output in the other directory is byte-identical after the refusal");
  check(sentinelBefore === await digest(sentinel), "E4: the unexpected sentinel is byte-identical");
  await rm(dir, { recursive: true, force: true });
}

// ------------------------------------------------ F. containment unit table
{
  let bad = 0;
  for (const testCase of CONTAINMENT_CASES) {
    const flavor = testCase.flavor === "win32" ? path.win32 : path.posix;
    if (isContained(testCase.root, testCase.target, flavor) !== testCase.contained) {
      failures.push(`F: ${testCase.flavor} containment wrong for ${testCase.why} (${testCase.root} vs ${testCase.target})`);
      bad += 1;
    }
  }
  check(bad === 0, `F: ${CONTAINMENT_CASES.length} containment cases hold, including win32 drive and UNC semantics ` +
    `(evaluated as pure path logic; native Windows runtime was not exercised)`);
}

notes.forEach((n) => process.stdout.write(`ok   ${n}\n`));
failures.forEach((f) => process.stdout.write(`FAIL ${f}\n`));
process.stdout.write(failures.length ? `\n${failures.length} self-test check(s) failed\n` : `\nall W2 harness self-tests passed\n`);
process.exitCode = failures.length ? 1 : 0;

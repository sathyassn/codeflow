import { spawn } from "node:child_process";
import path from "node:path";
import { stopChild, withSignalAwareChildLifecycle } from "./child-lifecycle.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";
import { assertToolOutputRoots, withWorkflowLease } from "./publication.mjs";
import { lockDigestFailure } from "./runtime-scripts.mjs";

// A writer leads its own process group where the platform has one, so the
// workers it leaves behind stay visible to a contender and a stop reaches them.
const GROUPS = process.platform !== "win32";

const workflow = process.argv[2];
if (!["build", "check", "dev", "preview", "verify"].includes(workflow)) throw new Error("workflow must be build, check, dev, preview, or verify");
const root = process.cwd();
await withSignalAwareChildLifecycle(async (lifecycle) => {
  await withWorkflowLease(root, async (writers) => {
    await assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]);
      // The suites that need a browser engine run under `browser:verify`.
    if (workflow === "check") {
      const stale = await lockDigestFailure(root);
      if (stale !== null) throw new Error(stale);
      await run(lifecycle, writers, process.execPath, ["--test", "tests/adapter.test.mjs", "tests/composition.test.mjs", "tests/chrome.test.mjs", "tests/site-server.test.mjs"]);
    }
    if (workflow !== "preview") await run(lifecycle, writers, process.execPath, ["scripts/adapter.mjs"]);
    const astro = path.join("node_modules", "astro", "bin", "astro.mjs");
    for (const stage of workflow === "verify" ? ["check", "build"] : [workflow]) {
      await run(lifecycle, writers, process.execPath, [astro, stage]);
    }
    if (workflow === "build" || workflow === "verify") await run(lifecycle, writers, process.execPath, ["scripts/evidence.mjs"]);
  });
});

function run(lifecycle, writers, command, args) {
  return new Promise((resolve, reject) => {
    // The lease marks a writer as starting before it exists, so a coordinator
    // killed anywhere between the spawn and the record is never read as idle.
    writers.beginWriter().then(() => start(lifecycle, writers, command, args, resolve, reject), reject);
  });
}

function start(lifecycle, writers, command, args, resolve, reject) {
  let child;
  try {
    child = spawn(command, args, {
      cwd: root,
      env: hardenedChildEnvironment(),
      stdio: "inherit",
      windowsHide: true,
      detached: GROUPS,
    });
  } catch (error) {
    writers.abandonWriter().then(() => reject(error), () => reject(error));
    return;
  }
  const signalGroup = (signal) => {
    if (!GROUPS || child.pid === undefined) return false;
    try { process.kill(-child.pid, signal); return true; } catch { return false; }
  };
  if (GROUPS) {
    const kill = child.kill.bind(child);
    child.kill = (signal) => signalGroup(signal ?? "SIGTERM") || kill(signal);
  }
  const untrack = lifecycle.trackChild(child);
  const recorded = (child.pid === undefined ? writers.abandonWriter() : writers.recordWriter(child.pid)).then(() => null, (failure) => failure);
  const unproven = (what) => {
    writers.keepLease();
    return new Error(`${what}; the workflow lease is kept so no other run starts over it. Stop process ${child.pid} and anything it started, then run again (or remove .portal/workflow.lock once no portal process runs)`);
  };
  let settled = false;
  const finish = (error) => {
    if (settled) return;
    settled = true;
    untrack();
    recorded.then(async (recordError) => {
      let failure = recordError ?? error;
      if (child.pid !== undefined && !(await retireWriter(writers, child.pid, signalGroup))) {
        failure = unproven("a process the portal workflow started could not be proven stopped");
      }
      if (failure) reject(failure); else resolve();
    });
  };
  // A writer the lease cannot name must not keep running: it is stopped, and
  // the stop confirmed, before anything else proceeds.
  recorded.then(async (recordError) => {
    if (recordError === null) return;
    try { await stopChild(child, "SIGKILL"); }
    catch { finish(unproven("a process the portal workflow started could not be named in the lease and could not be proven stopped")); }
  });
  child.once("error", finish);
  child.once("exit", (code, signal) => finish(lifecycle.interruptedBy !== null || code === 0 ? null : new Error(`${args.join(" ")} failed with ${signal ?? code}`)));
}

// A writer leaves the lease only when its whole group is gone: workers it left
// behind are stopped, and while any remains the writer stays recorded and the
// caller keeps the lease. Without groups (Windows) the workers cannot be seen,
// so the writer stays recorded and a contender treats the lease as undecidable.
async function retireWriter(writers, pid, signalGroup) {
  if (!GROUPS) return true;
  const groupAlive = () => { try { process.kill(-pid, 0); return true; } catch (error) { return error?.code !== "ESRCH"; } };
  if (groupAlive()) {
    signalGroup("SIGKILL");
    for (let waited = 0; waited < 40 && groupAlive(); waited += 1) await new Promise((done) => setTimeout(done, 50));
    if (groupAlive()) return false;
  }
  await writers.forgetWriter(pid);
  return true;
}

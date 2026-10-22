import { spawn } from "node:child_process";
import path from "node:path";
import { withSignalAwareChildLifecycle } from "./child-lifecycle.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";
import { assertToolOutputRoots, withWorkflowLease } from "./publication.mjs";
import { lockDigestFailure } from "./runtime-scripts.mjs";

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
    const child = spawn(command, args, {
      cwd: root,
      env: hardenedChildEnvironment(),
      stdio: "inherit",
      windowsHide: true,
    });
    const untrack = lifecycle.trackChild(child);
    // The lease lists every writer, so a contender can see one outlive a
    // coordinator that was killed without cleanup.
    const recorded = (child.pid === undefined ? Promise.resolve() : writers.recordWriter(child.pid)).then(() => null, (failure) => failure);
    let settled = false;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      untrack();
      recorded.then(async (recordError) => {
        if (child.pid !== undefined) await writers.forgetWriter(child.pid);
        const failure = error ?? recordError;
        if (failure) reject(failure); else resolve();
      });
    };
    child.once("error", finish);
    child.once("exit", (code, signal) => finish(lifecycle.interruptedBy !== null || code === 0 ? null : new Error(`${args.join(" ")} failed with ${signal ?? code}`)));
  });
}

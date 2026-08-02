import { spawn } from "node:child_process";
import path from "node:path";
import { assertToolOutputRoots, withWorkflowLease } from "./publication.mjs";

const workflow = process.argv[2];
if (!["build", "check", "dev", "preview"].includes(workflow)) throw new Error("workflow must be build, check, dev, or preview");
const root = process.cwd();
let activeChild = null;
let activeKillTimer = null;
let interruptedBy = null;
const signals = ["SIGINT", "SIGTERM"];
const signalHandlers = new Map(signals.map((signal) => [signal, () => {
  interruptedBy ??= signal;
  if (activeChild !== null) {
    const child = activeChild;
    child.kill(signal);
    activeKillTimer ??= setTimeout(() => { if (activeChild === child) child.kill("SIGKILL"); }, 5_000);
    activeKillTimer.unref();
  }
}]));
for (const [signal, handler] of signalHandlers) process.once(signal, handler);

try {
  await withWorkflowLease(root, async () => {
    await assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]);
    if (workflow === "check") await run(process.execPath, ["--test", "tests/adapter.test.mjs"]);
    if (workflow !== "preview") await run(process.execPath, ["scripts/adapter.mjs"]);
    await run(process.execPath, [path.join("node_modules", "astro", "bin", "astro.mjs"), workflow]);
    if (workflow === "build") await run(process.execPath, ["scripts/evidence.mjs"]);
  });
} finally {
  for (const [signal, handler] of signalHandlers) process.off(signal, handler);
}
if (interruptedBy !== null) process.kill(process.pid, interruptedBy);

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, stdio: "inherit" });
    activeChild = child;
    let settled = false;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      if (activeKillTimer !== null) clearTimeout(activeKillTimer);
      activeKillTimer = null;
      activeChild = null;
      if (error) reject(error); else resolve();
    };
    child.once("error", finish);
    child.once("exit", (code, signal) => finish(interruptedBy !== null || code === 0 ? null : new Error(`${args.join(" ")} failed with ${signal ?? code}`)));
    if (interruptedBy !== null) child.kill(interruptedBy);
  });
}

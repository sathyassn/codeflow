import { spawn } from "node:child_process";
import path from "node:path";
import { withWorkflowLease } from "./publication.mjs";

const workflow = process.argv[2];
if (!["build", "check"].includes(workflow)) throw new Error("workflow must be build or check");
const root = process.cwd();

await withWorkflowLease(root, async () => {
  if (workflow === "check") await run(process.execPath, ["--test", "tests/adapter.test.mjs"]);
  await run(process.execPath, ["scripts/adapter.mjs"]);
  await run(process.execPath, [path.join("node_modules", "astro", "astro.js"), workflow]);
  if (workflow === "build") await run(process.execPath, ["scripts/evidence.mjs"]);
});

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => code === 0 ? resolve() : reject(new Error(`${args.join(" ")} failed with ${signal ?? code}`)));
  });
}

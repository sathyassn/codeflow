import { spawnSync } from "node:child_process";
import { readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const qualification = resolve(scriptDirectory, "real-browser-check.mjs");
const runPrefix = `tsk007-cleanup-${process.pid}`;
const completed = spawnSync(process.execPath, [qualification, "--inject-cleanup-failure"], {
  env: { ...process.env, CF_PRESENT_RUN_PREFIX: runPrefix },
  encoding: "utf8",
  stdio: ["ignore", "pipe", "pipe"],
  timeout: 210_000,
});

if (completed.error) throw completed.error;
if (completed.status === 0) {
  throw new Error("Injected real-browser failure unexpectedly succeeded");
}
if (!completed.stderr.includes("injected real-browser cleanup failure")) {
  throw new Error(
    `Injected failure lost its primary error\nstdout: ${completed.stdout}\nstderr: ${completed.stderr}`,
  );
}
if (completed.stderr.includes("Cleanup failed during")) {
  throw new Error(`Injected failure exposed a cleanup defect\nstderr: ${completed.stderr}`);
}

await delay(2_500);
const remnants = (await readdir(tmpdir())).filter((entry) => entry.startsWith(`${runPrefix}-`));
if (remnants.length > 0) {
  throw new Error(`Injected failure left task-owned temporary roots: ${remnants.join(", ")}`);
}

process.stdout.write("cf-present injected failure cleanup passed: primary error preserved, no owned root remained\n");

function delay(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

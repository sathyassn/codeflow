import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const qualification = resolve(scriptDirectory, "real-browser-check.mjs");
const packageDefinition = JSON.parse(
  await readFile(resolve(scriptDirectory, "..", "package.json"), "utf8"),
);
const expectedPlaywrightCoreVersion = packageDefinition.devDependencies?.["playwright-core"];
if (!expectedPlaywrightCoreVersion) {
  throw new Error("The web package does not declare playwright-core");
}
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

const fallbackEvidence = await mkdtemp(join(tmpdir(), `cf-present-close-fallback-${process.pid}-`));
try {
  const fallback = spawnSync(process.execPath, [qualification, "--inject-close-timeout"], {
    env: {
      ...process.env,
      CF_PRESENT_RUN_PREFIX: `${runPrefix}-close-timeout`,
      CF_PRESENT_EVIDENCE_DIR: fallbackEvidence,
    },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: 210_000,
  });
  if (fallback.error) throw fallback.error;
  if (fallback.status !== 0) {
    throw new Error(
      `Injected close timeout did not recover\nstdout: ${fallback.stdout}\nstderr: ${fallback.stderr}`,
    );
  }
  const results = JSON.parse(await readFile(join(fallbackEvidence, "output", "results.json"), "utf8"));
  if (results.checks?.browser_close_fallbacks !== 1) {
    throw new Error(`Injected close timeout did not use one exact-owned fallback: ${JSON.stringify(results)}`);
  }
  if (
    !results.toolchain?.browser_version
    || results.toolchain?.playwright_core_version !== expectedPlaywrightCoreVersion
  ) {
    throw new Error(`Injected close timeout omitted its qualified browser toolchain: ${JSON.stringify(results)}`);
  }
} finally {
  await rm(fallbackEvidence, { recursive: true, force: false });
}

process.stdout.write(
  "cf-present cleanup passed: primary failure preserved and exact-owned close timeout recovered\n",
);

function delay(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

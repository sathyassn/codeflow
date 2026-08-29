import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { withSignalAwareChildLifecycle } from "./child-lifecycle.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";

// These pinned packages declare lifecycle scripts, but the current portal does
// not need them. Keeping the exact path and version here makes a lockfile change
// fail until its lifecycle surface is reviewed; the scripts remain disabled.
export const REVIEWED_IGNORED_LIFECYCLE_SCRIPTS = new Map([
  ["node_modules/esbuild", "0.28.1"],
  ["node_modules/fsevents", "2.3.2"],
  ["node_modules/vite/node_modules/fsevents", "2.3.3"],
]);

const root = fileURLToPath(new URL("..", import.meta.url));

export function assertReviewedInstallScripts(lockfile) {
  if (!lockfile || typeof lockfile !== "object" || !lockfile.packages || typeof lockfile.packages !== "object") {
    throw new Error("package-lock.json has no packages inventory");
  }
  const declared = new Map(Object.entries(lockfile.packages)
    .filter(([, value]) => value?.hasInstallScript === true)
    .map(([packagePath, value]) => [packagePath, value.version]));
  for (const [packagePath, version] of REVIEWED_IGNORED_LIFECYCLE_SCRIPTS) {
    if (declared.get(packagePath) !== version) {
      throw new Error(`reviewed ignored lifecycle script changed or disappeared: ${packagePath}@${version}`);
    }
    declared.delete(packagePath);
  }
  if (declared.size > 0) {
    const additions = [...declared].map(([packagePath, version]) => `${packagePath}@${version}`).join(", ");
    throw new Error(`unreviewed dependency lifecycle scripts in package-lock.json: ${additions}`);
  }
}

async function install(lifecycle) {
  const lockfile = JSON.parse(await readFile(path.join(root, "package-lock.json"), "utf8"));
  assertReviewedInstallScripts(lockfile);
  const npmCli = process.env.npm_execpath;
  if (typeof npmCli !== "string" || !path.isAbsolute(npmCli)) {
    throw new Error("run the hardened install through `npm run deps:install`");
  }
  await lifecycle.wait(new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [npmCli, "ci", "--ignore-scripts", "--no-audit", "--no-fund"], {
      cwd: root,
      env: hardenedChildEnvironment(),
      shell: false,
      stdio: "inherit",
      windowsHide: true,
    });
    const untrack = lifecycle.trackChild(child);
    child.once("error", (error) => {
      untrack();
      reject(error);
    });
    child.once("exit", (code, signal) => {
      untrack();
      if (lifecycle.interruptedBy !== null) return;
      if (code === 0) resolve();
      else reject(new Error(`locked dependency install failed with ${signal ?? code}`));
    });
  }));
  console.log("portal dependencies installed with dependency lifecycle scripts disabled");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await withSignalAwareChildLifecycle(install);
}

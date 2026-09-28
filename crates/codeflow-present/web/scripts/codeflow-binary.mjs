import { join, resolve } from "node:path";

/**
 * The codeflow binary a browser check drives: `CF_PRESENT_CODEFLOW` when it
 * is set, else the debug build in `CARGO_TARGET_DIR` (resolved from the
 * working directory, as cargo resolves it), else the debug build in the
 * repository's own `target` directory.
 */
export function codeflowBinary(repoRoot, env = process.env) {
  if (env.CF_PRESENT_CODEFLOW) return resolve(env.CF_PRESENT_CODEFLOW);
  const targetDir = env.CARGO_TARGET_DIR ? resolve(env.CARGO_TARGET_DIR) : join(repoRoot, "target");
  return join(targetDir, "debug", "codeflow");
}

import { randomUUID } from "node:crypto";
import { chmod, open, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, join } from "node:path";

/**
 * Windows refuses a rename over a file that another process holds open
 * (EPERM, EACCES or EBUSY), for example while the service reads it. The
 * refusal clears when that read ends, so the rename is retried this many
 * times, RENAME_RETRY_DELAY_MS apart, before the write fails.
 */
export const RENAME_ATTEMPTS = 20;
export const RENAME_RETRY_DELAY_MS = 50;
const RETRYABLE_RENAME_CODES = new Set(["EPERM", "EACCES", "EBUSY"]);

/**
 * Replace `path` with `data` so that a concurrent reader sees either the old
 * file or the new one, never a partial write. The bytes go to a new
 * temporary file in the same directory, with the target's mode (or `mode`
 * when the target does not exist yet), and the temporary file is then
 * renamed over the target. The temporary file is removed when any step
 * fails.
 */
export async function writeFileAtomic(path, data, { mode = 0o600, renameFile = rename } = {}) {
  const targetMode = await stat(path).then((stats) => stats.mode & 0o777, (error) => {
    if (error.code === "ENOENT") return mode;
    throw error;
  });
  const temporary = join(dirname(path), `.${basename(path)}.${process.pid}.${randomUUID()}.tmp`);
  let renamed = false;
  try {
    const handle = await open(temporary, "wx", targetMode);
    try {
      await handle.writeFile(data);
      await handle.sync();
    } finally {
      await handle.close();
    }
    // open() applies the umask; set the mode itself so it matches exactly.
    await chmod(temporary, targetMode);
    for (let attempt = 1; ; attempt += 1) {
      try {
        await renameFile(temporary, path);
        renamed = true;
        return;
      } catch (error) {
        if (!RETRYABLE_RENAME_CODES.has(error.code) || attempt >= RENAME_ATTEMPTS) throw error;
        await new Promise((resolve) => { setTimeout(resolve, RENAME_RETRY_DELAY_MS); });
      }
    }
  } finally {
    if (!renamed) await rm(temporary, { force: true });
  }
}

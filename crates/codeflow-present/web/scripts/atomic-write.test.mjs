import assert from "node:assert/strict";
import { mkdtemp, readdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { RENAME_ATTEMPTS, writeFileAtomic } from "./atomic-write.mjs";

const posix = process.platform !== "win32";

async function scratch(t) {
  const root = await mkdtemp(join(tmpdir(), "cf-atomic-write-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

function refusal(code) {
  return Object.assign(new Error(`${code}: rename refused`), { code });
}

test("the write replaces the content and leaves only the target", async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFile(target, "old\n");
  await writeFileAtomic(target, "new\n");
  assert.equal(await readFile(target, "utf8"), "new\n");
  assert.deepEqual(await readdir(root), ["revision.json"]);
});

test("the write keeps an existing target's mode", { skip: !posix }, async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFile(target, "old\n", { mode: 0o640 });
  await writeFileAtomic(target, "new\n", { mode: 0o600 });
  assert.equal((await stat(target)).mode & 0o777, 0o640);
});

test("a new target gets the requested mode", { skip: !posix }, async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFileAtomic(target, "new\n", { mode: 0o600 });
  assert.equal((await stat(target)).mode & 0o777, 0o600);
});

test("a rename Windows refuses while a reader holds the file is retried", async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFile(target, "old\n");
  const codes = ["EPERM", "EACCES", "EBUSY"];
  let calls = 0;
  await writeFileAtomic(target, "new\n", {
    renameFile: async (from, to) => {
      calls += 1;
      if (calls <= codes.length) throw refusal(codes[calls - 1]);
      return rename(from, to);
    },
  });
  assert.equal(calls, codes.length + 1);
  assert.equal(await readFile(target, "utf8"), "new\n");
  assert.deepEqual(await readdir(root), ["revision.json"]);
});

test("a rename refused on every attempt fails and removes the temporary file", async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFile(target, "old\n");
  let calls = 0;
  await assert.rejects(
    writeFileAtomic(target, "new\n", { renameFile: async () => { calls += 1; throw refusal("EPERM"); } }),
    { code: "EPERM" },
  );
  assert.equal(calls, RENAME_ATTEMPTS);
  assert.equal(await readFile(target, "utf8"), "old\n");
  assert.deepEqual(await readdir(root), ["revision.json"]);
});

test("any other rename error fails at once and removes the temporary file", async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  await writeFile(target, "old\n");
  let calls = 0;
  await assert.rejects(
    writeFileAtomic(target, "new\n", { renameFile: async () => { calls += 1; throw refusal("EXDEV"); } }),
    { code: "EXDEV" },
  );
  assert.equal(calls, 1);
  assert.deepEqual(await readdir(root), ["revision.json"]);
});

test("a concurrent reader sees only whole files", async (t) => {
  const root = await scratch(t);
  const target = join(root, "revision.json");
  // Large enough that a plain write is not one system call.
  const versions = ["a", "b"].map((fill) => `${JSON.stringify({ fill: fill.repeat(4 * 1024 * 1024) })}\n`);
  await writeFile(target, versions[0]);
  let writing = true;
  const reader = (async () => {
    let reads = 0;
    while (writing || reads === 0) {
      const text = await readFile(target, "utf8");
      assert.ok(versions.includes(text), `a read saw a partial file of ${text.length} bytes`);
      reads += 1;
    }
    return reads;
  })();
  for (let round = 0; round < 20; round += 1) await writeFileAtomic(target, versions[round % 2]);
  writing = false;
  assert.ok(await reader > 0);
});

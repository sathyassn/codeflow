import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";

const repo = resolve(import.meta.dirname, "../../../..");
test("new consumer stylesheets reject planted literals and accept tokens", async () => {
  const fixture = await mkdtemp(join(repo, "crates/codeflow-present/web/src/.token-lint-"));
  const sheet = join(fixture, "fixture.css");
  const check = () => spawnSync(process.execPath, ["scripts/design-tokens-sync.ts", "--check"], {
    cwd: repo, encoding: "utf8", timeout: 30_000,
  });
  try {
    await writeFile(sheet, "p { color: var(--cf-text); width: auto; }\n");
    const clean = check();
    assert.equal(clean.status, 0, clean.stderr);
    await writeFile(sheet, "p { color: papayawhip; width: 1cm; }\n");
    const planted = check();
    assert.notEqual(planted.status, 0, "an unregistered stylesheet must not escape lint");
    assert.match(planted.stderr, /Authored style literals.*fixture\.css: 1cm, papayawhip/);
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
});

test("token synchronization resolves its repository from the script location", () => {
  const result = spawnSync(process.execPath, [join(repo, "scripts/design-tokens-sync.ts"), "--check"], {
    cwd: join(repo, "crates"), encoding: "utf8", timeout: 30_000,
  });
  assert.equal(result.status, 0, result.stderr);
});

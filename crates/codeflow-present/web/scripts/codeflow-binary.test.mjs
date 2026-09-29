import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { codeflowBinary } from "./codeflow-binary.mjs";

const repo = resolve("/work/codeflow");

test("an explicit binary wins over the cargo target directory", () => {
  const env = { CF_PRESENT_CODEFLOW: "/opt/cf/codeflow", CARGO_TARGET_DIR: "/tmp/agent-target" };
  assert.equal(codeflowBinary(repo, env), resolve("/opt/cf/codeflow"));
});

test("the cargo target directory is used when it is set", () => {
  assert.equal(codeflowBinary(repo, { CARGO_TARGET_DIR: "/tmp/agent-target" }), join(resolve("/tmp/agent-target"), "debug", "codeflow"));
  assert.equal(codeflowBinary(repo, { CARGO_TARGET_DIR: "rel-target" }), join(resolve("rel-target"), "debug", "codeflow"));
});

test("the repository target directory is the fallback", () => {
  assert.equal(codeflowBinary(repo, {}), join(repo, "target", "debug", "codeflow"));
  assert.equal(codeflowBinary(repo, { CF_PRESENT_CODEFLOW: "", CARGO_TARGET_DIR: "" }), join(repo, "target", "debug", "codeflow"));
});

test("every browser check resolves its binary through the helper", async () => {
  const scripts = dirname(fileURLToPath(import.meta.url));
  for (const name of ["delivery-browser-check.mjs", "entity-browser-check.mjs", "figure-browser-check.mjs", "form-browser-check.mjs", "matrix-browser-check.mjs", "real-browser-check.mjs"]) {
    const source = await readFile(join(scripts, name), "utf8");
    assert.match(source, /codeflowBinary\(repoRoot\)/u, `${name} resolves its binary through codeflowBinary`);
    assert.doesNotMatch(source, /target\/debug\/codeflow/u, `${name} hard-codes the repository target directory`);
  }
});

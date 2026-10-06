import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { portalFixture } from "./portal-fixture.mjs";

test("fixture initialization starts no automatic Git maintenance", async () => {
  const scratch = await mkdtemp(path.join(os.tmpdir(), "portal-maintenance-"));
  const trace = path.join(scratch, "trace.jsonl");
  const previous = process.env.GIT_TRACE2_EVENT;
  let root;
  try {
    process.env.GIT_TRACE2_EVENT = trace;
    root = await portalFixture();
    const events = (await readFile(trace, "utf8")).trim().split("\n").map(JSON.parse);
    assert.ok(events.some((event) => event.event === "start"));
    const writers = events.filter((event) => event.event === "child_start"
      && event.argv.some((arg) => arg === "maintenance" || arg === "gc"));
    assert.deepEqual(writers, [], JSON.stringify(writers));
  } finally {
    if (previous === undefined) delete process.env.GIT_TRACE2_EVENT;
    else process.env.GIT_TRACE2_EVENT = previous;
    if (root) await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    await rm(scratch, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

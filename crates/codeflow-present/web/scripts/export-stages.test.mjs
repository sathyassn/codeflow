import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";
const scratch = await mkdtemp(join(tmpdir(), "cf-stage-unit-"));
const output = join(scratch, "stages.mjs");
await build({ entryPoints: ["src/export-stages.ts"], outfile: output, bundle: true, format: "esm", platform: "node", loader: { ".woff2": "dataurl" } });
const { frameLoad, sizeStage } = await import(pathToFileURL(output).href);
await rm(scratch, { recursive: true });
class Frame extends EventTarget { dataset = {}; }

test("readiness waits for every load and records a failed frame", async () => {
  const first = new Frame(); const second = new Frame();
  let ready = false;
  const settled = Promise.all([frameLoad(first, 500), frameLoad(second, 500)]).then((values) => { ready = true; return values; });
  first.dispatchEvent(new Event("load"));
  await Promise.resolve(); assert.equal(ready, false);
  second.dispatchEvent(new Event("error"));
  assert.deepEqual(await settled, [true, false]);
  assert.equal(second.dataset.cfStageStatus, "failed");
});
test("a missing load is bounded and a late event cannot erase failure", async () => {
  const frame = new Frame();
  assert.equal(await frameLoad(frame, 5), false);
  frame.dispatchEvent(new Event("load"));
  assert.equal(frame.dataset.cfStageStatus, "failed");
});
test("only matching width and appearance size the stage, and failure stays visible", () => {
  globalThis.document = { documentElement: { dataset: { cfTheme: "slate", cfModeResolved: "dark" } } };
  const notice = { hidden: false, textContent: "" };
  const values = new Map();
  const frame = { closest: () => ({ dataset: { cfBlockId: "nine-rows" } }), getBoundingClientRect: () => ({ width: 832 }),
    style: { setProperty: (key, value) => values.set(key, value) }, dataset: {}, parentElement: { querySelector: () => notice } };
  const report = { stage_id: "nine-rows", content_width: 832, height: 916, theme: "slate", mode: "dark" };
  sizeStage(frame, [report]); assert.equal(values.get("--cf-export-stage-height"), "916px"); assert.equal(notice.hidden, true);
  sizeStage(frame, [{ ...report, height: 48 }]); assert.equal(values.get("--cf-export-stage-height"), "48px");
  for (const content_width of [832 - 1 / 64, 832 + 1 / 64]) {
    sizeStage(frame, [{ ...report, content_width }]);
    assert.equal(values.get("--cf-export-stage-height"), "916px");
    assert.equal(notice.hidden, true);
  }
  for (const wrong of [{ ...report, content_width: 832 + 1 / 32 }, { ...report, content_width: 373 }, { ...report, mode: "light" }, { ...report, theme: "sage" }]) {
    sizeStage(frame, [wrong]); assert.equal(values.get("--cf-export-stage-height"), "28rem");
    assert.equal(notice.hidden, false); assert.match(notice.textContent, /height not measured at this width/u);
  }
  frame.dataset.cfStageStatus = "failed";
  sizeStage(frame, [report]); assert.equal(notice.hidden, false); assert.match(notice.textContent, /did not load/u);
});

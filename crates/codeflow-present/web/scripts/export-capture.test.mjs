import assert from "node:assert/strict";
import { test } from "node:test";
import { prepareExportCapture } from "./export-capture.mjs";

test("capture exposes the whole document at the measured width before waiting for paint", async () => {
  const calls = [];
  let evaluation = 0;
  const page = {
    waitForSelector: async (selector) => { calls.push(selector); },
    viewportSize: () => ({ width: 400, height: 900 }),
    evaluate: async () => { calls.push(++evaluation === 1 ? "extent" : "paint"); return 2500; },
    setViewportSize: async (size) => { calls.push(size); },
  };
  await prepareExportCapture(page);
  assert.deepEqual(calls, ["html[data-cf-ready='true']", "extent", { width: 400, height: 2500 }, "paint"]);
});

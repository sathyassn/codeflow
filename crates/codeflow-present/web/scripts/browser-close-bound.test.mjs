import assert from "node:assert/strict";
import test from "node:test";
import { closeWaitMs, MINIMUM_CLOSE_WAIT_MS } from "./browser-close-bound.mjs";

const CEILING = 45_000;

test("a close that starts early gets the whole ceiling", () => {
  assert.equal(closeWaitMs(CEILING, 170_000), CEILING);
  assert.equal(closeWaitMs(CEILING, 75_000), CEILING);
});

test("a close that starts later waits only until the teardown reserve", () => {
  assert.equal(closeWaitMs(CEILING, 60_000), 30_000);
  assert.equal(closeWaitMs(CEILING, 40_000), 10_000);
});

test("a close that starts near or past the deadline still gets its floor", () => {
  assert.equal(closeWaitMs(CEILING, 35_000), MINIMUM_CLOSE_WAIT_MS);
  assert.equal(closeWaitMs(CEILING, 1_000), MINIMUM_CLOSE_WAIT_MS);
  assert.equal(closeWaitMs(CEILING, -4_000), MINIMUM_CLOSE_WAIT_MS);
});

test("the wait never runs more than its floor past the qualification deadline", () => {
  for (let remaining = 0; remaining <= 180_000; remaining += 500) {
    const wait = closeWaitMs(CEILING, remaining);
    assert.ok(wait <= CEILING, `${wait} over the ceiling at ${remaining}`);
    assert.ok(
      wait - remaining <= MINIMUM_CLOSE_WAIT_MS,
      `a close started with ${remaining} ms left waits ${wait} ms, past the deadline by more than the floor`,
    );
  }
});

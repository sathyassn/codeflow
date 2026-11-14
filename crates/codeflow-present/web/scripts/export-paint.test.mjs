import assert from "node:assert/strict";
import { test } from "node:test";
import { hasTextInk } from "./export-paint.mjs";

test("blank paragraph and missing lower lanes fail in light and dark captures", () => {
  for (const ground of [246, 17]) {
    const pixels = new Uint8Array(20 * 40 * 4).fill(ground);
    const upperLane = { x: 2, y: 2, width: 10, height: 4 };
    const lowerLane = { ...upperLane, y: 20 };
    const paragraph = { ...upperLane, y: 30 };
    pixels[(3 * 20 + 3) * 4] = ground === 17 ? 220 : 27;
    assert.equal(hasTextInk(pixels, 20, upperLane, [ground, ground, ground]), true);
    assert.equal(hasTextInk(pixels, 20, lowerLane, [ground, ground, ground]), false);
    assert.equal(hasTextInk(pixels, 20, paragraph, [ground, ground, ground]), false);
    for (const box of [lowerLane, paragraph]) {
      pixels[((box.y + 1) * 20 + 3) * 4] = ground === 17 ? 220 : 27;
      assert.equal(hasTextInk(pixels, 20, box, [ground, ground, ground]), true);
    }
  }
});

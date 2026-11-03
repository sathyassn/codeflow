import { test } from 'node:test';
import assert from 'node:assert/strict';
import { segments, ringPath } from './stage-geometry.ts';
import floors from './floors.json' with {type: 'json'};
test('one of 300 is visible at wide and phone widths without changing labels', () => {
  for (const width of [1280, 390]) {
    const bar = segments([299, 1], width, floors.minimumSegmentPx);
    assert.ok(bar[1]!.extent >= floors.minimumSegmentPx);
    assert.equal(bar[1]!.count, 1);
    assert.ok(Math.abs(bar.reduce((n, s) => n + s.extent, 0) - width) < 1e-8);
    const ring = segments([299, 1], 360, floors.minimumArcDegrees);
    assert.ok(ring[1]!.extent >= floors.minimumArcDegrees);
  }
});
test('zero counts stay absent and a single positive count fills its chart', () => {
  assert.deepEqual(segments([0, 0], 390, 3).map(s => s.extent), [0, 0]);
  assert.deepEqual(segments([0, 7, 0], 360, 3).map(s => s.extent), [0, 360, 0]);
  assert.equal(ringPath(0, 0, 40), '');
  assert.equal((ringPath(0, 360, 40).match(/ A /g) || []).length, 2);
});
test('invalid or impossible geometry fails explicitly', () => {
  for (const counts of [[-1], [NaN], [.5], [Number.MAX_SAFE_INTEGER, 1]]) assert.throws(() => segments(counts, 390, 3));
  assert.throws(() => segments([1, 1], 5, 3)); assert.throws(() => ringPath(0, 361, 40));
});

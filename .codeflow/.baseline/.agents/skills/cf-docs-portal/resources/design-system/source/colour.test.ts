import { test } from 'node:test';
import assert from 'node:assert/strict';
import { contrast, deltaE, lab, separation, simulatedLab } from './colour.ts';

test('CIEDE2000 reproduces Sharma reference vectors, including hue wrap', () => {
  for (const [a, b, want] of [
    [[50, 2.6772, -79.7751], [50, 0, -82.7485], 2.0425],
    [[50, 3.1571, -77.2803], [50, 0, -82.7485], 2.8615],
    [[50, 0, 0], [50, -1, 2], 2.3669],
    [[50, 2.49, -.001], [50, -2.49, .001], 7.1792],
  ] as const) {
    assert.ok(Math.abs(deltaE(a, b) - want) < .0001);
    assert.ok(Math.abs(deltaE(b, a) - want) < .0001);
  }
});
test('contrast and D65 lightness endpoints reproduce reference values', () => {
  assert.equal(contrast('#000000', '#ffffff'), 21);
  assert.equal(contrast('#12766e', '#12766e'), 1);
  assert.ok(Math.abs(lab('#ffffff')[0] - 100) < .001);
  assert.equal(lab('#000000')[0], 0);
});
test('simulation has identity endpoints and collapses identical pairs', () => {
  for (const kind of ['protan', 'deutan', 'tritan'] as const) {
    assert.equal(separation('#123456', '#123456', kind), 0);
    assert.equal(simulatedLab('#000000', kind)[0], 0);
    assert.ok(Math.abs(simulatedLab('#ffffff', kind)[0] - 100) < .001);
  }
});

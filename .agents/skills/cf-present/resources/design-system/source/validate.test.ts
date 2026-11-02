import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { validateColours } from './validate.ts';
import type { Colours, Floors } from './validate.ts';
const source: Colours = JSON.parse(readFileSync(new URL('./colour.json', import.meta.url), 'utf8'));
const floors: Floors = JSON.parse(readFileSync(new URL('./floors.json', import.meta.url), 'utf8'));
for (const [name, rule, mutate] of [
  ['cast', 'neutral', (s: Colours) => { s.palettes['graphite-light']!.canvas = '#f4f3f4'; }],
  ['white', 'canvas', (s: Colours) => { s.palettes['graphite-light']!.canvas = '#ffffff'; }],
  ['black', 'canvas', (s: Colours) => { s.palettes['graphite-dark']!.canvas = '#000000'; }],
  ['Neutral', 'theme', (s: Colours) => { s.themes.Neutral = ['green', 'plum']; }],
  ['one accent', 'options', (s: Colours) => { s.themes.graphite = ['green']; }],
  ['contrast', 'contrast', (s: Colours) => { s.accents.green!.light = '#fafafa'; }],
  ['status hue', 'hue', (s: Colours) => { s.accents.green!.light = s.palettes['graphite-light']!.positive!; }],
] as const) test(`planted ${name} is rejected`, () => {
  const copy = structuredClone(source); mutate(copy);
  assert.ok(validateColours(copy, floors).some(f => f.rule === rule));
});
test('clean Graphite theme control passes every theme check', () => {
  const copy = structuredClone(source);
  copy.themes = { graphite: copy.themes.graphite! };
  assert.deepEqual(validateColours(copy, floors), []);
});

test('every shipped theme and accent passes the colour floors', () => {
  assert.deepEqual(validateColours(source, floors), []);
});

import { validateStages, validateCompanions } from './validate.ts';
test('near-identical dark stage inks fail normal and all simulated floors', () => {
  const failures = validateStages({ first: '#222222', second: '#232323' }, { surface: '#ffffff' }, floors);
  for (const name of ['normal', 'protan', 'deutan', 'tritan']) assert.ok(failures.some(f => f.rule === 'stage separation' && f.context.endsWith('/' + name)));
  assert.ok(!failures.some(f => f.rule === 'stage contrast'));
});
test('missed stage surface floor fails, with a clean palette control', () => {
  assert.ok(validateStages({ first: '#fafafa' }, { surface: '#ffffff' }, floors).some(f => f.rule === 'stage contrast'));
  assert.deepEqual(validateStages({ first: '#111111', second: '#777777' }, { surface: '#ffffff' }, floors), []);
});
test('all curated companions meet unchanged ordinary and simulated floors', () => {
  assert.deepEqual(validateCompanions(source, floors), []);
});

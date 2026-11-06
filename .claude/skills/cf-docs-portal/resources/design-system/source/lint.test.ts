import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { styleLiterals } from './lint.ts';
for (const [name, value] of [['hex', 'color:#fff'], ['functional', 'color:rgb(1 2 3)'], ['named', 'color:red'], ['named extended', 'color:papayawhip'], ['absolute size', 'width:1cm'], ['exponent size', 'width:1e3px'], ['size', 'width:14px'], ['radius', 'border-radius:8px'], ['fallback', 'color:var(--ink,#fff)'], ['named fallback', 'color:var(--ink,red)']])
  test(`planted ${name} literal fails lint`, () => assert.ok(styleLiterals(`a{${value}}`).length));
test('clean token control permits intrinsic values and CSS variables', () => {
  assert.deepEqual(styleLiterals('a{color:var(--ink);fill:currentColor;background:transparent;padding:0;border-radius:var(--radius)}'), []);
});
test('every authored style template is free of colour, size and radius literals', async () => {
  for (const name of await readdir(new URL('./styles/', import.meta.url))) {
    const source = await readFile(new URL('./styles/' + name, import.meta.url), 'utf8');
    assert.deepEqual(styleLiterals(source), [], name);
  }
});

test('pseudo-highlight selectors are not named colour declarations', () => assert.deepEqual(styleLiterals('::highlight(notes){color:var(--ink)}'), []));

test('value-named colour properties cannot disguise palette literals', () => {
  for (const css of ['a{color:var(--cf-colour-fff)}', ':root{--cf-colour-aabbcc:var(--cf-text)}'])
    assert.ok(styleLiterals(css).length, css);
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { generate } from './generate.ts';

test('system export fallback matches every resolved theme and mode palette', async () => {
  const outputs = await generate();
  const palettes = JSON.parse(outputs.get('tokens.json')!);
  const [light, dark] = outputs.get('export-fallback.css')!.split('@media (prefers-color-scheme: dark)');
  for (const [pair, roles] of Object.entries(palettes)) {
    const [theme, mode] = pair.split('-');
    const css = mode === 'dark' ? dark! : light!;
    const selector = `:root[data-cf-mode="system"][data-cf-theme="${theme}"]`;
    const body = css.slice(css.indexOf(selector) + selector.length).match(/^\s*\{([^}]*)\}/)?.[1];
    assert.ok(body, pair);
    const actual = Object.fromEntries([...body.matchAll(/--cf-([\w-]+):\s*([^;]+);/g)].map(([, key, value]) => [key, value]));
    assert.deepEqual(actual, roles, pair);
  }
});

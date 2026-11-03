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


test('root size scales inherited stage text and every rem type role', async () => {
  const outputs = await generate();
  for (const sheet of ['tokens.css', 'present.css', 'utility-tokens.css']) {
    const css = outputs.get(sheet)!;
    for (const [role, value] of Object.entries({ display: '2.35rem', h2: '1.65rem', h3: '1.25rem', lede: '1.18rem', body: '1rem', title: '1rem', ui: '0.88rem', caption: '0.82rem', micro: '0.76rem' })) {
      assert.ok(css.includes(`--cf-fs-${role}: ${value};`), `${sheet} ${role}`);
    }
    assert.ok(css.includes('font-size: var(--cf-root-size)'), `${sheet} root must consume the size`);
    for (const value of ['93.75%', '106.25%', '118.75%']) assert.ok(css.includes(`--cf-root-size: ${value}`), `${sheet} ${value}`);
    assert.ok(!css.includes('--cf-ui-scale'), `${sheet} must not double-scale tokens`);
  }
  assert.match(outputs.get('present.css')!, /\.cf-stage-host\s*\{[^}]*font-size: var\(--cf-fs-ui\)/);
  assert.match(outputs.get('figure.css')!, /\.cf-fig-number\s*\{[^}]*font-size: var\(--cf-fs-micro\)/);
});

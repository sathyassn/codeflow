import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { readFile, rm } from "node:fs/promises";
import path from "node:path";
import { portalSkinOrder, validatePortalConfig } from "../scripts/lib.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture } from "./portal-fixture.mjs";
import { inlineScripts } from "../scripts/runtime-scripts.mjs";
import { prePaintScript } from "../scripts/runtime-scripts.mjs";

function paint(theme, stored = {}) {
  const attributes = {};
  const element = { dataset: {}, style: { setProperty: (key, value) => { attributes[key] = value; } }, setAttribute: (key, value) => { attributes[key] = value; } };
  const matchMedia = () => ({ matches: false });
  vm.runInNewContext(prePaintScript(theme), { document: { documentElement: element }, localStorage: { getItem: (key) => stored[key] ?? null }, window: { matchMedia }, matchMedia });
  return attributes;
}

for (const [value, expected] of [['graphite', 'graphite'], ['slate', 'slate'], ['sage', 'sage'], ['signal', 'graphite'], ['folio', 'sage']]) {
  test(`configured ${value} paints ${expected} with independent Inter before runtime`, () => {
    const actual = paint(value);
    assert.equal(actual['data-cfp-skin'], expected);
    assert.equal(actual['data-cfp-typeface'], 'inter');
    assert.deepEqual(actual, paint(expected));
    const stored = paint(value, { 'cf-portal-typeface': 'plex', 'cf-portal-skin': 'slate', 'starlight-theme': 'dark' });
    assert.equal(stored['data-cfp-typeface'], 'plex');
    assert.equal(stored['data-cfp-skin'], 'slate');
    assert.equal(stored['data-theme'], 'dark');
  });
}
test('pre-paint restores bounded panel preferences and ignores malformed values', () => {
  assert.equal(paint('graphite', { 'cf.nav.width': '360', 'cf.nav.open': 'false' })['--cf-nav-w'], '360px');
  assert.equal(paint('graphite', { 'cf.nav.width': '99999' })['--cf-nav-w'], undefined);
  const actual = paint('graphite', { 'cf-portal-typeface': 'invalid', 'cf-portal-scale': 'invalid', 'cf.toc.open': 'false' });
  assert.equal(actual['data-cfp-typeface'], 'inter');
  assert.equal(actual['data-cfp-scale'], 'default');
  assert.equal(actual['data-cf-toc'], 'collapsed');
});


test('canonical and compatibility portal themes build the same skin order', { timeout: 120_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    const original = JSON.parse(await readFile(path.join(root, 'portal.config.json'), 'utf8'));
    for (const [theme, skin] of [['graphite', 'graphite'], ['slate', 'slate'], ['sage', 'sage'], ['signal', 'graphite'], ['folio', 'sage']]) {
      assert.equal(validatePortalConfig({ ...original, theme }).theme, theme);
      assert.deepEqual(portalSkinOrder(theme), portalSkinOrder(skin));
      await configureFixture(root, { theme });
      commitFixture(root, 'configure fixture palette');
      runLocalAdapter(root);
      buildFixture(root);
      const html = await readFile(path.join(root, 'dist/reference/seed/index.html'), 'utf8');
      assert.ok(inlineScripts(html).includes(prePaintScript(theme)), `${theme}: built page carries the configured pre-paint script`);
      assert.equal(paint(theme)['data-cfp-skin'], skin);
      assert.equal(paint(theme)['data-cfp-typeface'], 'inter');
    }
    assert.throws(() => validatePortalConfig({ ...original, theme: 'unknown' }), /theme must be/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

for (const [old, skin] of [['instrument', 'graphite'], ['editorial', 'slate'], ['ink', 'sage'], ['technical', 'graphite']]) {
  test(`saved ${old} skin normalizes before paint without selecting a font`, () => {
    assert.equal(paint('sage', { 'cf-portal-skin': old })['data-cfp-skin'], skin);
    assert.equal(paint('sage', { 'cf-portal-skin': old })['data-cfp-typeface'], 'inter');
    assert.equal(paint('sage', { 'cf-portal-skin': old, 'cf-portal-typeface': 'plex' })['data-cfp-typeface'], 'plex');
  });
}
for (const [old, face] of [['instrument', 'archivo'], ['editorial', 'inter'], ['plex', 'plex']]) {
  test(`saved ${old} font normalizes before paint without selecting a skin`, () => {
    const actual = paint('sage', { 'cf-portal-typeface': old });
    assert.equal(actual['data-cfp-typeface'], face);
    assert.equal(actual['data-cfp-skin'], 'sage');
  });
}

import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { portalSkinOrder, validatePortalConfig } from "../scripts/lib.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture } from "./portal-fixture.mjs";
import { inlineScripts, allowedInlineScripts, scriptSha256, prePaintHashes, runtimeScriptsFailure, PRE_PAINT_FIXTURE_FILE, REGENERATE } from "../scripts/runtime-scripts.mjs";
import { prePaintScript } from "../scripts/runtime-scripts.mjs";
import { PORTAL_THEMES } from "../scripts/lib.mjs";
import committedScripts from "../scripts/runtime-scripts.json" with { type: "json" };

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

// The committed pre-paint hashes are what this generator writes now, for
// every configurable theme, so a token or template change that was not
// regenerated fails here before `validate --portal` refuses every page. The
// fixture the Rust validator's tests hash is the generator's too.
test('the committed pre-paint hashes and fixture are the generator output for every theme', async () => {
  assert.equal(committedScripts.schema_version, 2);
  assert.deepEqual(Object.keys(committedScripts.pre_paint_sha256), [...PORTAL_THEMES]);
  assert.deepEqual(committedScripts.pre_paint_sha256, prePaintHashes(), `scripts/runtime-scripts.json is stale against the tokens; run: ${REGENERATE}`);
  const fixture = JSON.parse(await readFile(path.join(import.meta.dirname, '..', PRE_PAINT_FIXTURE_FILE), 'utf8'));
  assert.equal(fixture.script, prePaintScript(fixture.theme));
  assert.equal(fixture.sha256, scriptSha256(fixture.script));
  assert.equal(fixture.sha256, committedScripts.pre_paint_sha256[fixture.theme]);
  for (const theme of PORTAL_THEMES) assert.ok(allowedInlineScripts(theme).has(scriptSha256(prePaintScript(theme))), theme);
});

// The browser gate reads a runtime script list as the Rust validator does:
// both run these shared cases and must give exactly the recorded reason, so
// neither accepts a list the other refuses (figures.rs,
// runtime_script_list_fails_closed_like_the_starter_reader).
test('the runtime script list reader refuses what the Rust validator refuses, with the same reason', async () => {
  const { cases } = JSON.parse(await readFile(path.join(import.meta.dirname, 'fixtures/runtime-scripts-cases.json'), 'utf8'));
  assert.ok(cases.length >= 10);
  const at = (root, pointer) => pointer.split('/').slice(1).reduce((node, key) => node[key], root);
  for (const { name, theme, set = {}, remove = [], error } of cases) {
    const list = structuredClone(committedScripts);
    for (const [pointer, value] of Object.entries(set)) { const cut = pointer.lastIndexOf('/'); at(list, pointer.slice(0, cut))[pointer.slice(cut + 1)] = value; }
    for (const pointer of remove) { const cut = pointer.lastIndexOf('/'); delete at(list, pointer.slice(0, cut))[pointer.slice(cut + 1)]; }
    assert.equal(runtimeScriptsFailure(theme, list), error, name);
    if (error === null) assert.ok(allowedInlineScripts(theme, list).has(committedScripts.pre_paint_sha256[theme]), name);
    else assert.throws(() => allowedInlineScripts(theme, list), { message: `the runtime's inline script list scripts/runtime-scripts.json cannot be read: ${error}` }, name);
  }
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
      // The hash the validators look up is the hash of what the build inlined.
      assert.ok(inlineScripts(html).map(scriptSha256).includes(committedScripts.pre_paint_sha256[theme]), `${theme}: built pre-paint script has the committed hash`);
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

test('nested navigation and breadcrumbs share readable labels without recasing custom labels', { timeout: 60_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    await mkdir(path.join(root, 'docs/architecture/release_notes'), { recursive: true });
    await writeFile(path.join(root, 'docs/architecture/release_notes/entry.md'), '# Release entry\n');
    const config = JSON.parse(await readFile(path.join(root, 'portal.config.json'), 'utf8'));
    await configureFixture(root, { layers: config.layers.map(layer => layer.id === 'reference' ? { ...layer, label: 'API docs' } : layer) });
    commitFixture(root, 'add nested navigation fixture');
    runLocalAdapter(root);
    buildFixture(root);
    const html = await readFile(path.join(root, 'dist/reference/architecture/release_notes/entry/index.html'), 'utf8');
    const nav = html.match(/<nav[^>]*id="cf-nav"[\s\S]*?<\/nav>/)?.[0] ?? html;
    const crumbs = html.match(/<nav class="cf-crumbs"[\s\S]*?<\/nav>/)?.[0];
    assert.ok(crumbs, 'rendered breadcrumb');
    for (const label of ['API docs', 'Architecture', 'Release Notes']) {
      assert.match(nav, new RegExp(`class="cf-kicker"[^>]*>${label} <span class="cf-row-count">`));
      assert.ok(crumbs.includes(`>${label}</`), `breadcrumb label ${label}`);
    }
    assert.doesNotMatch(nav, /class="cf-kicker"[^>]*>(?:architecture|release_notes|Api Docs) /);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('built page resolves system mode before deferred runtime', { timeout: 120_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    runLocalAdapter(root);
    buildFixture(root);
    const html = await readFile(path.join(root, 'dist/reference/seed/index.html'), 'utf8');
    const config = JSON.parse(await readFile(path.join(root, 'portal.config.json'), 'utf8'));
    const allowed = allowedInlineScripts(config.theme);
    for (const script of inlineScripts(html)) assert.ok(allowed.has(scriptSha256(script)), 'built inline script is in the committed allowlist');
    const scripts = inlineScripts(html).filter(script => script.includes('document.documentElement'));
    assert.ok(scripts.length > 0, 'page emits its pre-paint theme owner');
    for (const dark of [false, true]) {
      for (const stored of ['system', 'auto', 'light', 'dark', null, 'invalid']) {
        const attributes = {};
        const element = {
          dataset: new Proxy({}, {
            get: (_, key) => attributes['data-' + key.replace(/[A-Z]/g, c => '-' + c.toLowerCase())],
            set: (_, key, value) => { attributes['data-' + key.replace(/[A-Z]/g, c => '-' + c.toLowerCase())] = value; return true; }
          }),
          style: { setProperty() {} },
          setAttribute(key, value) { attributes[key] = value; }
        };
        const matchMedia = query => ({ matches: query.includes('dark') ? dark : !dark });
        const context = vm.createContext({ document: { documentElement: element }, localStorage: { getItem: key => key === 'starlight-theme' ? stored : null }, window: { matchMedia }, matchMedia });
        for (const script of scripts) vm.runInContext(script, context);
        const expected = stored === 'light' || stored === 'dark' ? stored : dark ? 'dark' : 'light';
        assert.equal(attributes['data-theme'], expected, `stored ${stored}, system ${dark ? 'dark' : 'light'}`);
      }
    }
    assert.equal(scripts.length, 1, 'one owner resolves mode before first paint');
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('portal mode stores auto and reads legacy system while following media changes', async () => {
  const runtime = await readFile(new URL('../public/portal-chrome.js', import.meta.url), 'utf8');
  const { settings: choices } = await import('../public/tokens.js');
  const stored = new Map([['starlight-theme', 'system']]);
  const attributes = {};
  const media = { matches: false, addEventListener(_, handler) { this.change = handler; } };
  const CF = {};
  const context = vm.createContext({ choices, CF, accentLabel: value => value,
    store: { get: key => stored.get(key) ?? null, set: (key, value) => stored.set(key, value) },
    root: { getAttribute: key => attributes[key], setAttribute: (key, value) => { attributes[key] = value; } },
    mqDark: media, syncDisplay() {}, d: { dispatchEvent() {} }, CustomEvent: class {} });
  vm.runInContext(runtime.slice(runtime.indexOf('const PREFS ='), runtime.indexOf('let displayEl =')), context);
  assert.equal(CF.getPref('theme'), 'system');
  CF.setPref('theme', 'system');
  assert.equal(stored.get('starlight-theme'), 'auto');
  assert.equal(CF.getPref('theme'), 'system');
  assert.equal(attributes['data-theme'], 'light');
  media.matches = true; media.change();
  assert.equal(attributes['data-theme'], 'dark');
  CF.setPref('theme', 'light');
  media.change();
  assert.equal(attributes['data-theme'], 'light', 'explicit mode ignores system changes');
});

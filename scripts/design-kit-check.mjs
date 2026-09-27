// Browser check for the design system reference kit (TSK-070). Opens
// portal.reference.html and present.reference.html over file:// from the
// assets/base kit folder in every skin and mode pair at 1280 and 375 px,
// drives each named control, and reports one row per control.
//
// The check is repository-only: the kit's source folder exists only in the
// CodeFlow checkout, so it lives here rather than in docs-portal/scripts,
// whose files the starter mirror ships to every portal consumer. It borrows
// the portal's pinned Playwright, so run `npm run deps:install --prefix
// docs-portal` first, then `node scripts/design-kit-check.mjs` or
// `node --test scripts/design-kit-check.test.mjs` from the repository root.
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { hardenedChildEnvironment } from "../docs-portal/scripts/process-environment.mjs";

const REPOSITORY = fileURLToPath(new URL("..", import.meta.url));
const { chromium } = createRequire(path.join(REPOSITORY, "docs-portal/package.json"))("@playwright/test");
export const KIT_DIR = path.join(REPOSITORY, "assets/base/agents/skills/cf-present/resources/design-system");
let kitDir = KIT_DIR;
let PHASE = '';
const url = name => pathToFileURL(path.join(kitDir, name)).href;
export const PAIRS = [['graphite', 'light'], ['graphite', 'dark'], ['slate', 'light'], ['slate', 'dark'], ['sage', 'light'], ['sage', 'dark']];
export const WIDTHS = [1280, 375];
const PAUSE = 360;
const sleep = ms => new Promise(r => setTimeout(r, ms));

/* In-page probes */
const FIGCHECK = () => {
  const out = [];
  const svgs = [...document.querySelectorAll('.cf-fig-svg')].filter(s => s.getBoundingClientRect().width > 0);
  svgs.forEach(svg => {
    const fig = svg.closest('.cf-fig');
    const vb = svg.viewBox.baseVal, scale = svg.getBoundingClientRect().width / vb.width;
    const texts = [...svg.querySelectorAll('text')].map(t => ({ t, b: t.getBBox(), fs: parseFloat(getComputedStyle(t).fontSize) }));
    const minPx = Math.min(...texts.map(x => x.fs * scale));
    const marks = [...svg.querySelectorAll('path, line, polyline, circle, rect')].filter(m => !m.closest('defs') && !m.classList.contains('cf-f-hit'));
    const near = [];
    const boxDist = (b, x, y) => Math.hypot(Math.max(b.x - x, 0, x - (b.x + b.width)), Math.max(b.y - y, 0, y - (b.y + b.height)));
    texts.forEach(({ t, b }) => {
      let best = 1e9, who = '';
      marks.forEach(m => {
        const cs = getComputedStyle(m), sw = parseFloat(cs.strokeWidth) || 0, half = cs.stroke !== 'none' ? sw / 2 : 0;
        if (m.tagName === 'rect' || m.tagName === 'circle') {
          const mb = m.getBBox(), box = { x: mb.x - half, y: mb.y - half, width: mb.width + 2 * half, height: mb.height + 2 * half };
          const inside = b.x >= box.x && b.y >= box.y && b.x + b.width <= box.x + box.width && b.y + b.height <= box.y + box.height;
          const dd = inside ? Math.min(b.x - box.x, b.y - box.y, box.x + box.width - b.x - b.width, box.y + box.height - b.y - b.height) - sw
            : Math.hypot(Math.max(box.x - (b.x + b.width), 0, b.x - (box.x + box.width)), Math.max(box.y - (b.y + b.height), 0, b.y - (box.y + box.height)));
          if (dd < best) { best = dd; who = m.getAttribute('class'); }
        } else {
          const L = m.getTotalLength(), n = Math.max(2, Math.ceil(L / 2));
          for (let i = 0; i <= n; i++) { const p = m.getPointAtLength(L * i / n); const dd = boxDist(b, p.x, p.y) - half; if (dd < best) { best = dd; who = m.getAttribute('class'); } }
        }
      });
      if (best < 8) near.push(`${t.textContent.trim()} ${best.toFixed(1)} from ${who}`);
    });
    for (let i = 0; i < texts.length; i++) for (let j = i + 1; j < texts.length; j++) {
      const a = texts[i].b, c = texts[j].b;
      if (a.x < c.x + c.width && c.x < a.x + a.width && a.y < c.y + c.height && c.y < a.y + a.height) near.push(`text over text: ${texts[i].t.textContent.trim()} / ${texts[j].t.textContent.trim()}`);
    }
    const drawn = [...new Set([...svg.querySelectorAll('[data-state]')].map(e => e.dataset.state))].sort();
    const keyed = [...new Set([...fig.querySelectorAll('.cf-legend li[data-state]')].filter(li => getComputedStyle(li).display !== 'none').map(li => li.dataset.state))].sort();
    out.push({
      fig: fig.querySelector('.cf-fig-title').textContent.trim(),
      variant: svg.classList.contains('cf-fig-svg--narrow') ? 'narrow' : 'wide',
      minPx: +minPx.toFixed(2), near, drawn, keyed, legendOk: drawn.join() === keyed.join(),
      titleDesc: !!svg.querySelector(':scope > title') && !!svg.querySelector(':scope > desc') && svg.getAttribute('role') === 'img',
      twin: !!fig.querySelector('details.cf-fig-details table'), caption: !!fig.querySelector('figcaption.cf-fig-caption')
    });
  });
  return out;
};
const MOTION = () => {
  let max = 0, worst = '', loops = 0;
  for (const el of document.querySelectorAll('*')) for (const ps of [null, '::before', '::after']) {
    const cs = getComputedStyle(el, ps);
    const t = Math.max(...cs.transitionDuration.split(',').map(parseFloat));
    const a = cs.animationName !== 'none' ? Math.max(...cs.animationDuration.split(',').map(parseFloat)) : 0;
    if (cs.animationName !== 'none' && cs.animationIterationCount.includes('infinite')) loops++;
    if (Math.max(t, a) > max) { max = Math.max(t, a); worst = (el.className.baseVal ?? el.className) + (ps || ''); }
  }
  return { max, worst, loops };
};

async function newPage(browser, width, opts = {}) {
  const ctx = await browser.newContext({ viewport: { width, height: width === 375 ? 812 : 860 }, isMobile: width === 375, ...opts });
  await ctx.addInitScript(() => {
    new MutationObserver((ms, obs) => {
      if (document.body) { window.__atBody = { theme: document.documentElement.getAttribute('data-cf-theme'), skin: document.documentElement.getAttribute('data-cf-skin') }; obs.disconnect(); }
    }).observe(document, { childList: true, subtree: true });
  });
  const page = await ctx.newPage();
  page.errors = [];
  page.on('console', m => { if (m.type() === 'error') page.errors.push(m.text()); });
  page.on('pageerror', e => page.errors.push(String(e)));
  page.on('requestfailed', r => page.errors.push('request failed: ' + r.url()));
  return { ctx, page };
}
async function load(page, name, skin, theme, hash = '') {
  await page.goto(url(name));
  await page.evaluate(([s, t]) => { localStorage.clear(); localStorage.setItem('cf.skin', s); localStorage.setItem('cf.theme', t); }, [skin, theme]);
  await page.goto(url(name) + hash);
  await page.reload();
  await page.waitForFunction(() => window.CF && CF.view && !document.documentElement.hasAttribute('data-cf-preload'));
  await sleep(150);
}
const attr = (page, a) => page.evaluate(n => document.documentElement.getAttribute(n), a);
const blur = page => page.evaluate(() => { if (document.activeElement) document.activeElement.blur(); });

function recorder() {
  const rows = [];
  const rec = (name, ok, detail = '') => { rows.push({ name, ok: ok === 'n/a' ? 'n/a' : !!ok, detail: String(detail) }); };
  const step = async (name, fn) => { PHASE = ''; try { await fn(); } catch (e) { rec(name, false, `threw${PHASE ? ' at ' + PHASE : ''}: ` + e.message.split('\n')[0]); } };
  return { rows, rec, step };
}

async function commonChecks(page, width, skin, theme, rec, step) {
  const atBody = await page.evaluate(() => window.__atBody);
  rec('prepaint', atBody && atBody.theme === theme && atBody.skin === skin, `at <body>: ${atBody && atBody.skin}/${atBody && atBody.theme}`);
  const sw = await page.evaluate(() => document.documentElement.scrollWidth);
  rec('no-hscroll', sw === width, `scrollWidth ${sw}`);
  await step('display', async () => {
    const gear = page.locator('[data-cf-open="display"]:visible').first();
    await gear.click(); await sleep(PAUSE);
    const open = await page.evaluate(() => { const p = document.querySelector('.cf-display'); return !!p && p.classList.contains('is-open') && !p.hidden; });
    const sheet = await page.evaluate(() => document.querySelector('.cf-display').classList.contains('is-sheet'));
    rec('display.open', open, width === 375 ? (sheet ? 'bottom sheet' : 'popover') : (sheet ? 'bottom sheet' : 'popover'));
    const pill = (pref, v) => page.locator(`.cf-display [data-pref="${pref}"] .cf-pill[data-value="${v}"]`);
    await pill('typeface', 'archivo').click();
    rec('display.font', await attr(page, 'data-cf-typeface') === 'archivo' && await page.evaluate(() => localStorage.getItem('cf.typeface')) === 'archivo', await attr(page, 'data-cf-typeface'));
    await pill('scale', 'large').click();
    rec('display.size', await attr(page, 'data-cf-scale') === 'large', await attr(page, 'data-cf-scale'));
    const other = skin === 'sage' ? 'slate' : 'sage';
    await pill('skin', other).click();
    const switched = await attr(page, 'data-cf-skin');
    await pill('skin', skin).click();
    rec('display.palette', switched === other && await attr(page, 'data-cf-skin') === skin, `${other} then ${skin}`);
    const flip = theme === 'light' ? 'dark' : 'light';
    await pill('theme', flip).click();
    const flipped = await attr(page, 'data-cf-theme');
    await page.emulateMedia({ colorScheme: 'dark' });
    await pill('theme', 'system').click();
    const sysDark = await attr(page, 'data-cf-theme');
    await page.emulateMedia({ colorScheme: 'light' }); await sleep(50);
    const sysLight = await attr(page, 'data-cf-theme');
    await pill('theme', theme).click(); await sleep(PAUSE);
    rec('display.appearance', flipped === flip && sysDark === 'dark' && sysLight === 'light' && await attr(page, 'data-cf-theme') === theme, `${flip}; system follows OS live: dark ${sysDark}, light ${sysLight}`);
    const border = await page.evaluate(() => {
      const p = document.querySelector('.cf-display .cf-pill[aria-checked="true"]'), probe = document.createElement('i');
      probe.style.color = document.documentElement.getAttribute('data-cf-theme') === 'dark' ? 'var(--cf-border-strong)' : 'var(--cf-border)';
      document.body.appendChild(probe); const want = getComputedStyle(probe).color; probe.remove();
      return getComputedStyle(p).borderTopColor === want;
    });
    rec('display.selected-pill-border', border, theme === 'dark' ? 'border-strong in dark' : 'border in light');
    await page.locator('.cf-display [data-pref="scale"] .cf-pill[aria-checked="true"]').focus();
    await page.keyboard.press('ArrowLeft');
    rec('display.arrow-keys', await attr(page, 'data-cf-scale') === 'default', 'ArrowLeft from Large gives ' + await attr(page, 'data-cf-scale'));
    await page.keyboard.press('Escape'); await sleep(PAUSE);
    const closed = await page.evaluate(() => document.querySelector('.cf-display').hidden);
    const refocus = await page.evaluate(() => document.activeElement && document.activeElement.getAttribute('data-cf-open') === 'display');
    rec('display.esc', closed && refocus, 'closed, focus back on the gear');
    await page.reload(); await page.waitForFunction(() => window.CF && CF.view); await sleep(150);
    rec('display.remembered', await attr(page, 'data-cf-typeface') === 'archivo' && await attr(page, 'data-cf-skin') === skin && await attr(page, 'data-cf-theme') === theme, 'font survives reload');
    await page.evaluate(() => { localStorage.setItem('cf.typeface', 'inter'); localStorage.setItem('cf.scale', 'default'); });
    await page.reload(); await page.waitForFunction(() => window.CF && CF.view); await sleep(150);
  });
}

async function figureChecks(page, rec, width, prefix = '') {
  const figs = await page.evaluate(FIGCHECK);
  const expect = width === 375 ? 'narrow' : 'wide';
  figs.forEach(f => {
    const n = prefix + f.fig.replace(/^Figure (\d).*/, 'fig$1');
    rec(`${n}.variant`, f.variant === expect, f.variant);
    rec(`${n}.min-text`, f.minPx >= 12.5, f.minPx + ' px');
    rec(`${n}.clearance`, f.near.length === 0, f.near.join('; ') || 'no text within 8 px of a mark, no text over text');
    rec(`${n}.legend`, f.legendOk, `drawn ${f.drawn.join(' ')} | keyed ${f.keyed.join(' ')}`);
    rec(`${n}.title-desc-twin`, f.titleDesc && f.twin && f.caption, 'title, desc, caption, table twin');
  });
  return figs;
}

async function portalRun(browser, skin, theme, width) {
  const { rows, rec, step } = recorder();
  const { ctx, page } = await newPage(browser, width);
  await load(page, 'portal.reference.html', skin, theme);
  await commonChecks(page, width, skin, theme, rec, step);
  const figs = [];
  await step('figures', async () => {
    for (const layer of ['concept', 'architecture', 'technical']) {
      await page.click(`#cf-tab-${layer}`); await sleep(250);
      figs.push(...await figureChecks(page, rec, width));
    }
    rec('figures.count', figs.length === 6, figs.length + ' figures');
    await page.click('#cf-tab-concept'); await sleep(200);
  });
  await step('search', async () => {
    const trigger = width === 375 ? '.cf-search-icon' : '.cf-search-trigger';
    await page.click(trigger); await sleep(PAUSE);
    const isOpen = () => page.evaluate(() => { const b = document.querySelector('.cf-search'); return !!b && b.classList.contains('is-open') && document.activeElement === b.querySelector('input'); });
    rec('search.click', await isOpen(), trigger);
    await page.keyboard.press('Escape'); await sleep(PAUSE);
    rec('search.esc', await page.evaluate(t => document.querySelector('.cf-search').hidden && document.activeElement === document.querySelector(t), trigger), 'closed, focus back on the opener');
    for (const [key, name] of [['/', 'search.slash'], ['Control+k', 'search.ctrl-k'], ['Meta+k', 'search.cmd-k']]) {
      await blur(page); await page.keyboard.press(key); await sleep(PAUSE);
      rec(name, await isOpen(), key);
      await page.keyboard.press('Escape'); await sleep(PAUSE);
    }
    await blur(page); await page.keyboard.press('/'); await sleep(PAUSE);
    await page.keyboard.type('gate'); await sleep(200);
    const res = await page.evaluate(() => {
      const groups = [...document.querySelectorAll('.cf-search-group')].map(g => g.querySelector('.cf-search-group-title').textContent);
      const rows = [...document.querySelectorAll('.cf-hit')].map(h => h.closest('.cf-search-group').querySelector('.cf-search-group-title').textContent + '|' + h.querySelector('.cf-hit-title').textContent);
      const chip = document.querySelector('.cf-hit .cf-chip');
      return { groups, rows, dupes: rows.length - new Set(rows).size, marks: document.querySelectorAll('.cf-hit mark').length, status: document.querySelector('.cf-search-status').textContent, chipPx: chip && getComputedStyle(chip).fontSize, chipTrack: chip && getComputedStyle(chip).letterSpacing };
    });
    rec('search.results', res.groups.length >= 2 && res.marks > 0 && /results? for gate/.test(res.status), `${res.status}; groups: ${res.groups.join(', ')}; ${res.marks} highlighted`);
    rec('search.one-row-per-section', res.dupes === 0, `${res.rows.length} rows, ${res.dupes} duplicates`);
    rec('search.chip-micro', res.chipPx === '12.5px', `${res.chipPx}, tracking ${res.chipTrack}`);
    await page.keyboard.press('ArrowDown');
    rec('search.arrow-keys', await page.evaluate(() => document.querySelector('.cf-search-input').getAttribute('aria-activedescendant') === 'cf-opt-1'), 'ArrowDown selects the second row');
    await page.keyboard.press('ArrowUp');
    const target = await page.evaluate(() => [...document.querySelectorAll('.cf-hit')].findIndex(h => h.querySelector('.cf-hit-title').textContent === 'When a gate blocks'));
    for (let i = 0; i < target; i++) await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter'); await sleep(PAUSE + 300);
    const after = await page.evaluate(() => ({ closed: document.querySelector('.cf-search').hidden, tab: document.querySelector('.cf-tab[aria-selected="true"]').dataset.layer, hash: location.hash, focus: document.activeElement.id }));
    rec('search.enter-follows', after.closed && after.tab === 'technical' && after.focus === 'when-a-gate-blocks', `tab ${after.tab}, hash ${after.hash}, focus #${after.focus}`);
  });
  await step('tabs', async () => {
    await page.click('#cf-tab-architecture'); await sleep(250);
    const a = await page.evaluate(() => ({ hash: location.hash, shown: [...document.querySelectorAll('.cf-tabpanel')].filter(p => !p.hidden).map(p => p.dataset.layer) }));
    rec('tabs.click-writes-hash', a.hash.endsWith('#architecture') && a.shown.join() === 'architecture', `${a.hash}, visible ${a.shown}`);
    await page.focus('#cf-tab-architecture'); await page.keyboard.press('ArrowRight'); await sleep(250);
    const b = await page.evaluate(() => ({ hash: location.hash, sel: document.activeElement.dataset.layer }));
    rec('tabs.arrow-keys', b.hash.endsWith('#technical') && b.sel === 'technical', `${b.hash}, focus ${b.sel}`);
    await page.goBack(); await sleep(250);
    const c = await page.evaluate(() => document.querySelector('.cf-tab[aria-selected="true"]').dataset.layer);
    await page.goForward(); await sleep(250);
    const f = await page.evaluate(() => document.querySelector('.cf-tab[aria-selected="true"]').dataset.layer);
    rec('tabs.back-forward', c === 'architecture' && f === 'technical', `back ${c}, forward ${f}`);
    const ind = await page.evaluate(() => { const t = document.querySelector('.cf-tab[aria-selected="true"]'), i = document.querySelector('.cf-tabs-ind'); return Math.abs(i.getBoundingClientRect().left - t.getBoundingClientRect().left) < 1.5 && Math.abs(i.offsetWidth - t.offsetWidth) < 1.5; });
    rec('tabs.underline', ind, 'accent underline under the selected tab');
  });
  await step('toc', async () => {
    const heads = await page.evaluate(() => [...document.querySelectorAll('.cf-panel--toc .cf-toc-link')].map(a => a.dataset.target));
    rec('toc.built', heads.length >= 4, heads.join(' '));
    if (width === 1280) { await page.click('.cf-panel--toc .cf-toc-link[data-target="when-a-gate-blocks"]'); await sleep(900); }
    else { await page.evaluate(() => document.getElementById('when-a-gate-blocks').scrollIntoView({ block: 'start' })); await sleep(400); }
    const act = await page.evaluate(() => [...document.querySelectorAll('.cf-toc-link.is-active')].map(a => a.dataset.target));
    rec('toc.scroll-spy', act.length === 2 && act.every(t => t === 'when-a-gate-blocks'), `active in panel and sheet: ${act.join(', ')}`);
    const fill = await page.evaluate(() => { const a = document.querySelector('.cf-toc-link.is-active'), cs = getComputedStyle(a); return cs.borderLeftWidth === '0px' && cs.backgroundColor !== 'rgba(0, 0, 0, 0)'; });
    rec('toc.soft-fill', fill, 'active item has a fill and no border');
    await page.evaluate(() => scrollTo(0, 0)); await sleep(200);
  });
  await step('crumbs', async () => {
    const c = await page.evaluate(() => [...document.querySelectorAll('.cf-crumbs li')].filter(li => getComputedStyle(li).display !== 'none').map(li => li.textContent.trim()));
    rec('crumbs', width === 375 ? c.length === 2 : c.length === 3, c.join(' / '));
    const before = await page.evaluate(() => location.hash);
    await page.click('.cf-crumb-parent a'); await sleep(200);
    const toast = await page.evaluate(() => { const t = document.querySelector('.cf-toast'); return t && t.classList.contains('is-open') ? t.textContent : ''; });
    rec('crumbs.route-seam', /Outside this reference page: \/portal\/orient/.test(toast) && before === await page.evaluate(() => location.hash), toast);
  });
  await step('provenance', async () => {
    const p = await page.evaluate(() => {
      const parts = [...document.querySelectorAll('.cf-prov-part')];
      const seps = parts.map(e => getComputedStyle(e, '::before').content);
      const shape = document.querySelector('.cf-fresh .cf-shape--dot');
      const lines = parts.map(e => e.getBoundingClientRect().top);
      return { n: parts.length, seps, shape: !!shape, px: getComputedStyle(document.querySelector('.cf-prov')).fontSize, lines: [...new Set(lines.map(Math.round))].length };
    });
    rec('provenance', p.n === 3 && p.shape && p.px === '12.5px' && p.seps[0] === 'none' && p.seps.slice(1).every(s => s === '"·"'), `${p.n} parts on ${p.lines} line(s), separators ${p.seps.join(' ')}, ${p.px}, fresh dot`);
  });
  await step('nav', async () => {
    if (width === 1280) {
      await page.click('.cf-panel--nav [data-group="system"] .cf-nav-chev'); await sleep(PAUSE);
      const g = await page.evaluate(() => ({ c: document.querySelector('.cf-panel--nav [data-group="system"]').classList.contains('is-collapsed'), s: localStorage.getItem('cf.nav.groups') }));
      await page.click('.cf-panel--nav [data-group="system"] .cf-nav-chev'); await sleep(PAUSE);
      rec('nav.group-collapse', g.c && /"system":true/.test(g.s), `stored ${g.s}`);
      const toggle = page.locator('[data-cf-toggle="nav"]');
      await toggle.click(); await sleep(PAUSE);
      const col = await page.evaluate(() => ({ a: document.documentElement.getAttribute('data-cf-nav'), w: document.querySelector('.cf-panel--nav').getBoundingClientRect().width, s: localStorage.getItem('cf.nav.open') }));
      rec('nav.collapse', col.a === 'collapsed' && col.w <= 1 && col.s === 'false', `column ${col.w}px (transparent border), stored cf.nav.open=${col.s}`);
      const sheetOpen = () => page.evaluate(() => document.querySelector('[data-cf-sheet="nav"]').classList.contains('is-open'));
      await page.mouse.move(640, 400); await page.mouse.move(5, 420); await sleep(PAUSE);
      const hoverOpen = await sheetOpen();
      await page.mouse.move(700, 420); await sleep(600);
      rec('nav.peek-hover', hoverOpen && !(await sheetOpen()), 'opens on the left edge strip, closes after the pointer leaves');
      await toggle.focus(); await page.keyboard.press('Enter'); await sleep(PAUSE);
      const kb = await page.evaluate(() => ({ open: document.querySelector('[data-cf-sheet="nav"]').classList.contains('is-open'), inside: !!document.activeElement.closest('[data-cf-sheet="nav"]') }));
      await page.keyboard.press('Escape'); await sleep(PAUSE);
      const back = await page.evaluate(() => document.activeElement.getAttribute('data-cf-toggle'));
      rec('nav.peek-keyboard', kb.open && kb.inside && back === 'nav', 'Enter on the collapsed toggle opens with focus inside; Esc returns focus');
      await blur(page); await page.keyboard.press('['); await sleep(PAUSE);
      const b1 = await sheetOpen(); await page.keyboard.press('['); await sleep(PAUSE);
      rec('nav.peek-bracket', b1 && !(await sheetOpen()), '[ opens and closes');
      await toggle.click(); await sleep(PAUSE);
      rec('nav.restore', await page.evaluate(() => !document.documentElement.hasAttribute('data-cf-nav')), 'toggle restores the column');
    } else {
      await page.click('[data-cf-toggle="nav"]'); await sleep(PAUSE);
      const open = await page.evaluate(() => document.querySelector('[data-cf-sheet="nav"]').classList.contains('is-open'));
      await page.click('[data-cf-sheet="nav"] [data-group="system"] .cf-nav-chev'); await sleep(PAUSE);
      const g = await page.evaluate(() => document.querySelector('[data-cf-sheet="nav"] [data-group="system"]').classList.contains('is-collapsed'));
      await page.click('[data-cf-sheet="nav"] [data-group="system"] .cf-nav-chev'); await sleep(PAUSE);
      rec('nav.sheet', open, 'navigation opens as a sheet');
      rec('nav.group-collapse', g, 'group collapses inside the sheet');
      await page.keyboard.press('Escape'); await sleep(PAUSE);
      await blur(page); await page.keyboard.press('['); await sleep(PAUSE);
      const b1 = await page.evaluate(() => document.querySelector('[data-cf-sheet="nav"]').classList.contains('is-open'));
      await page.keyboard.press('Escape'); await sleep(PAUSE);
      rec('nav.peek-bracket', b1, '[ opens the sheet');
      rec('nav.collapse', 'n/a', 'column layout starts at 880 px');
      rec('nav.peek-hover', 'n/a', 'edge strips start at 880 px');
    }
  });
  await step('toc-peek', async () => {
    const sheetOpen = () => page.evaluate(() => document.querySelector('[data-cf-sheet="toc"]').classList.contains('is-open'));
    if (width === 1280) {
      await page.click('[data-cf-toggle="toc"]'); await sleep(PAUSE);
      const collapsed = await page.evaluate(() => document.documentElement.getAttribute('data-cf-toc') === 'collapsed' && localStorage.getItem('cf.toc.open') === 'false');
      await blur(page); await page.keyboard.press(']'); await sleep(PAUSE);
      const b1 = await sheetOpen(); await page.keyboard.press(']'); await sleep(PAUSE);
      const b2 = await sheetOpen();
      await page.mouse.move(640, 400); await page.mouse.move(1276, 420); await sleep(PAUSE);
      const hov = await sheetOpen(); await page.mouse.move(640, 420); await sleep(600);
      await page.click('[data-cf-toggle="toc"]'); await sleep(PAUSE);
      rec('toc.collapse', collapsed, 'stored cf.toc.open=false');
      rec('toc.peek-bracket', b1 && !b2, '] opens and closes');
      rec('toc.peek-hover', hov, 'right edge strip opens the outline');
    } else {
      await blur(page); await page.keyboard.press(']'); await sleep(PAUSE);
      const b1 = await sheetOpen(); await page.keyboard.press('Escape'); await sleep(PAUSE);
      rec('toc.peek-bracket', b1, '] opens the outline sheet');
      rec('toc.collapse', 'n/a', 'column layout starts at 1120 px');
    }
  });
  await step('grip', async () => {
    if (width !== 1280) { rec('grip', 'n/a', 'navigation is a sheet under 880 px'); return; }
    const box = await page.locator('.cf-grip').boundingBox();
    const x = box.x + box.width / 2, y = box.y + 200;
    const w = () => page.evaluate(() => Math.round(document.querySelector('.cf-panel--nav').getBoundingClientRect().width));
    await page.mouse.move(x, y); await page.mouse.down(); await page.mouse.move(x + 400, y, { steps: 6 }); await page.mouse.up(); await sleep(150);
    const hi = await w();
    const box2 = await page.locator('.cf-grip').boundingBox();
    await page.mouse.move(box2.x + 4, y); await page.mouse.down(); await page.mouse.move(box2.x - 400, y, { steps: 6 }); await page.mouse.up(); await sleep(150);
    const lo = await w();
    await page.focus('.cf-grip'); await page.keyboard.press('ArrowRight'); await sleep(100);
    const k = await w();
    const stored = await page.evaluate(() => localStorage.getItem('cf.nav.width'));
    await page.dblclick('.cf-grip'); await sleep(PAUSE);
    const reset = await w();
    rec('grip.drag-clamp', hi === 360 && lo === 220, `drag right ${hi}px, drag left ${lo}px`);
    rec('grip.keyboard', k === 228 && stored === '228', `ArrowRight ${k}px, stored ${stored}`);
    rec('grip.reset', reset === 264, `double-click ${reset}px`);
  });
  await step('preview-card', async () => {
    await page.click('#cf-tab-concept'); await sleep(250);
    const id = page.locator('.cf-tabpanel:not([hidden]) .cf-id').first();
    await id.scrollIntoViewIfNeeded(); await id.hover(); await sleep(PAUSE);
    const c = await page.evaluate(() => { const p = document.querySelector('.cf-preview'); const r = p.getBoundingClientRect(); return { open: p.classList.contains('is-open'), sheet: p.classList.contains('is-sheet-mode'), title: p.querySelector('.cf-preview-title')?.textContent, inView: r.left >= 0 && r.right <= innerWidth }; });
    rec('preview.hover', c.open && c.inView && (width === 375 ? c.sheet : !c.sheet), `${c.title}; ${c.sheet ? 'bottom sheet' : 'card below the link'}`);
    await page.mouse.move(5, 5); await sleep(PAUSE);
    await id.focus(); await sleep(150);
    const f = await page.evaluate(() => document.querySelector('.cf-preview').classList.contains('is-open'));
    await page.keyboard.press('Escape'); await sleep(PAUSE);
    const e = await page.evaluate(() => document.querySelector('.cf-preview').classList.contains('is-open'));
    rec('preview.focus-esc', f && !e, 'opens on focus, Esc dismisses');
    await blur(page);
  });
  await step('footer', async () => {
    const card = page.locator('.cf-footcard--next');
    await card.scrollIntoViewIfNeeded();
    const before = await card.evaluate(e => getComputedStyle(e).backgroundColor);
    await card.hover(); await sleep(PAUSE);
    const after = await card.evaluate(e => getComputedStyle(e).backgroundColor);
    const n = await page.locator('.cf-footcard').count();
    rec('footer.cards', n === 2 && before !== after, `${n} cards, hover fill ${before} to ${after}`);
  });
  rec('console-errors', page.errors.length === 0, page.errors.join(' | ') || '0 during the run');
  await ctx.close();
  return { rows };
}

async function presentRun(browser, skin, theme, width) {
  const { rows, rec, step } = recorder();
  const { ctx, page } = await newPage(browser, width);
  await load(page, 'present.reference.html', skin, theme);
  await commonChecks(page, width, skin, theme, rec, step);
  await step('topbar', async () => {
    const t = await page.evaluate(() => {
      const q = s => document.querySelector(s), vis = e => e && getComputedStyle(e).display !== 'none';
      return { title: q('.cf-topbar-title').textContent, rev: vis(q('.cf-topbar-rev')) ? q('.cf-topbar-rev').textContent : '(hidden)', sub: q('.cf-doc-meta').textContent, kicker: vis(q('.cf-topbar-titles > .cf-kicker')), px: getComputedStyle(q('.cf-topbar-title')).fontSize, clamp: getComputedStyle(q('.cf-topbar-title')).webkitLineClamp };
    });
    const ok = t.title === 'Landing paths figure' && !/revision/i.test(t.sub) && (width === 375 ? !t.kicker && t.px === '14px' && t.clamp === '2' : t.rev === 'Revision 3' && t.kicker);
    rec('topbar', ok, `title "${t.title}", meta "${t.rev}", subtitle "${t.sub}", kicker ${t.kicker ? 'shown' : 'hidden'}, ${t.px}`);
    const railHidden = await page.evaluate(() => getComputedStyle(document.querySelector('.cf-rail')).visibility === 'hidden');
    rec('rail.hidden-when-off', railHidden, 'rail hidden while Comment is off');
  });
  await step('route', async () => {
    if (width !== 1280) { rec('route', 'n/a', 'section route hides under 70rem'); return; }
    const active = () => page.evaluate(() => document.querySelector('.cf-route .cf-toc-link.is-active').dataset.target);
    await page.click('.cf-route .cf-toc-link[data-target="blk-ask"]'); await sleep(900);
    const a = await active(), y = await page.evaluate(() => scrollY);
    await page.evaluate(() => scrollTo(0, 0)); await sleep(300);
    const b = await active();
    rec('route', a === 'blk-ask' && y > 0 && b === 'blk-figure', `click Ask scrolls to ${y} and marks ${a}; back at top ${b}`);
  });
  await figureChecks(page, rec, width);
  await step('comment', async () => {
    const armed = () => page.evaluate(() => ({ a: document.querySelector('.cf-present-app').getAttribute('data-cf-comment') === 'armed', p: document.querySelector('.cf-comment-toggle').getAttribute('aria-pressed'), rail: getComputedStyle(document.querySelector('.cf-rail')).visibility, strip: getComputedStyle(document.querySelector('.cf-strip')).display }));
    await page.click('.cf-comment-toggle'); await sleep(PAUSE);
    const a1 = await armed();
    rec('comment.arm-click', a1.a && a1.p === 'true' && a1.rail === 'visible' && a1.strip === 'flex', `aria-pressed ${a1.p}, rail ${a1.rail}, strip ${a1.strip}`);
    await blur(page); await page.keyboard.press('c'); await sleep(PAUSE);
    const a2 = await armed(); await page.keyboard.press('c'); await sleep(PAUSE);
    const a3 = await armed();
    rec('comment.key-c', !a2.a && a3.a, 'C leaves and arms');
    const v0 = await page.evaluate(() => ({ v: document.querySelector('[data-cf-verdict]').value, b: document.querySelector('[data-cf-act="submit"]').disabled, t: document.querySelector('[data-cf-act="submit"]').textContent }));
    rec('verdict.default', v0.v === 'approve' && !v0.b, `${v0.v} at zero notes, "${v0.t}" enabled`);
    await sleep(PAUSE);
    await figureChecks(page, rec, width, 'armed.');
    if (width === 375) rec('no-hscroll.armed', await page.evaluate(() => document.documentElement.scrollWidth) === 375, 'scrollWidth with the rail sheet open ' + await page.evaluate(() => document.documentElement.scrollWidth));

    /* text gesture */
    PHASE = 'text gesture';
    await page.evaluate(() => {
      const p = document.querySelector('#blk-frame p'), n = p.firstChild, i = n.nodeValue.indexOf('ends the integrate path');
      const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + 'ends the integrate path'.length);
      const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    });
    await sleep(PAUSE);
    const fl = await page.evaluate(() => {
      const f = document.querySelector('.cf-float'), p = document.querySelector('#blk-frame p'), r = getSelection().getRangeAt(0).getClientRects()[0];
      const fr = f.getBoundingClientRect(), pr = p.getBoundingClientRect();
      return { shown: !f.hidden, text: f.textContent, clear: fr.top >= pr.bottom + 5 || fr.bottom <= r.top, fits: fr.left >= 0 && fr.right <= innerWidth };
    });
    rec('gesture.text-float', fl.shown && /Text/.test(fl.text) && fl.clear && fl.fits, `"${fl.text.replace('esc', '').trim()}", ${fl.clear ? 'clear of the paragraph lines' : 'covers the paragraph'}`);
    await page.click('.cf-float'); await sleep(200);
    const comp = await page.evaluate(() => { const c = document.querySelector('.cf-composer'), r = c.getBoundingClientRect(); return { open: !c.hidden, focus: document.activeElement === c.querySelector('textarea'), fits: r.left >= 0 && r.right <= innerWidth }; });
    rec('composer.open', comp.open && comp.focus && comp.fits, 'autofocus, inside the viewport');
    await page.keyboard.type('Name who merges on path B.');
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter'); await sleep(PAUSE);
    const m1 = await page.evaluate(() => ({ markers: [...document.querySelectorAll('.cf-marker')].filter(m => !m.hidden).map(m => m.textContent), v: document.querySelector('[data-cf-verdict]').value }));
    rec('composer.cmd-enter-saves', m1.markers.join() === '1', 'markers ' + m1.markers.join(','));
    rec('verdict.first-note', m1.v === 'approve-with-notes', m1.v);

    /* element gesture */
    PHASE = 'element gesture';
    const pt = await page.evaluate(() => {
      const g = [...document.querySelectorAll('#cf-present-document [data-cf-anchor="node-a-checks"]')].find(e => e.getBoundingClientRect().width > 0);
      g.scrollIntoView({ block: 'center' }); const r = g.querySelector('circle').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2];
    });
    await sleep(200);
    await page.mouse.move(pt[0], pt[1]); await sleep(150);
    const hov = await page.evaluate(() => !document.querySelector('.cf-hoverbox').hidden);
    await page.mouse.click(pt[0], pt[1]); await sleep(PAUSE);
    const ef = await page.evaluate(() => { const f = document.querySelector('.cf-float'); return { shown: !f.hidden, text: f.textContent }; });
    rec('gesture.element', hov && ef.shown && /Element/.test(ef.text), `hover outline ${hov}; "${ef.text.replace('esc', '').trim()}"`);
    await page.click('.cf-float'); await sleep(200);
    await page.keyboard.type('Say that a red required check stops travel here.');
    await page.click('.cf-composer [data-act="save"]'); await sleep(PAUSE);

    /* area gesture: find empty figure ground, then drag a box */
    PHASE = 'area gesture';
    const box = await page.evaluate(() => {
      const svg = [...document.querySelectorAll('#cf-present-document .cf-fig-svg')].find(s => s.getBoundingClientRect().width > 0);
      const r = svg.getBoundingClientRect();
      for (let fy = 0.15; fy < 0.9; fy += 0.05) for (let fx = 0.05; fx < 0.6; fx += 0.05) {
        const x = r.left + r.width * fx, y = r.top + r.height * fy;
        const ok = [0, 40, 80].every(dx => [0, 30].every(dy => document.elementFromPoint(x + dx, y + dy) === svg));
        if (ok) return [x, y];
      }
      return null;
    });
    if (!box) throw new Error('no empty figure ground found');
    await page.mouse.move(box[0], box[1]); await page.mouse.down(); await page.mouse.move(box[0] + 80, box[1] + 30, { steps: 6 }); await page.mouse.up(); await sleep(PAUSE);
    const af = await page.evaluate(() => { const f = document.querySelector('.cf-float'); return { shown: !f.hidden, text: f.textContent }; });
    rec('gesture.area', af.shown && /Area/.test(af.text), `"${af.text.replace('esc', '').trim()}"`);
    await page.click('.cf-float'); await sleep(200);
    await page.keyboard.type('Leave more room around the lanes.');
    await page.click('.cf-composer [data-act="save"]'); await sleep(PAUSE);

    const MARKS = () => {
      return [...document.querySelectorAll('.cf-marker')].map(m => {
        const t = m.style.transform.match(/translate\(([-\d.]+)px, ([-\d.]+)px\)/);
        return { n: m.textContent, hidden: m.hidden, x: +t[1], y: +t[2] };
      });
    };
    const EXPECT = () => {
      const wrap = document.querySelector('.cf-doc-wrap').getBoundingClientRect();
      const blk = document.querySelector('#blk-frame').getBoundingClientRect();
      const node = [...document.querySelectorAll('#cf-present-document [data-cf-anchor="node-a-checks"]')].find(e => e.getBoundingClientRect().width > 0).getBoundingClientRect();
      return { elementX: Math.round(node.right + 2 - wrap.left), elementY: Math.round(node.top - 18 - wrap.top), textX: Math.round(blk.left - 32 - wrap.left) };
    };
    const m3 = await page.evaluate(MARKS);
    rec('markers.numbered', m3.map(m => m.n).join() === '1,2,3' && m3.every(m => !m.hidden), 'markers ' + m3.map(m => m.n).join(','));
    const vp = page.viewportSize();
    await page.setViewportSize({ width: width === 1280 ? 1000 : 414, height: vp.height }); await sleep(600);
    const mr = await page.evaluate(MARKS), ex = await page.evaluate(EXPECT);
    await page.setViewportSize(vp); await sleep(600);
    const mb = await page.evaluate(MARKS), eb = await page.evaluate(EXPECT);
    const near = (a, b) => Math.abs(a - b) <= 1;
    const okResize = near(mr[1].x, ex.elementX) && near(mr[1].y, ex.elementY) && near(mr[0].x, ex.textX) && near(mb[1].x, eb.elementX) && near(mb[1].y, eb.elementY) && mr.every(m => !m.hidden);
    rec('markers.survive-resize', okResize, `after resize marker 2 at ${mr[1].x},${mr[1].y} expected ${ex.elementX},${ex.elementY}; restored ${mb[1].x},${mb[1].y} expected ${eb.elementX},${eb.elementY}`);

    /* rail, earlier feedback, tools */
    PHASE = 'rail, earlier feedback, tools';
    const rail = await page.evaluate(() => { document.querySelector('[data-cf-earlier]').open = true; const b = document.querySelector('[data-cf-earlier-body]').textContent; return { rows: document.querySelectorAll('.cf-note-row').length, badge: document.querySelector('[data-cf-badge]').textContent, orphan: /Unresolved: quote not found/.test(b), re: /Reanchored/.test(b) }; });
    rec('rail.notes', rail.rows === 3 && rail.badge === '3', `${rail.rows} rows, badge ${rail.badge}`);
    rec('rail.earlier-feedback', rail.orphan && rail.re, 'one reanchored note, one orphaned: quote not found');
    await page.evaluate(() => {
      const p = document.querySelector('#blk-evidence .cf-status-name'), r = document.createRange(); r.selectNodeContents(p);
      const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    });
    await sleep(PAUSE);
    await page.evaluate(() => { document.querySelectorAll('.cf-rail details').forEach(d => { d.open = true; }); });
    await page.click('.cf-rail [data-cf-tool="text"]'); await sleep(200);
    const tool = await page.evaluate(() => ({ open: !document.querySelector('.cf-composer').hidden, q: document.querySelector('.cf-composer-quote').textContent }));
    await page.evaluate(() => getSelection().removeAllRanges());
    await page.keyboard.press('Escape'); await sleep(PAUSE);
    const stillArmed = await page.evaluate(() => document.querySelector('.cf-present-app').getAttribute('data-cf-comment') === 'armed' && document.querySelector('.cf-composer').hidden);
    rec('tools.add-selected-text', tool.open && /Figure gate/.test(tool.q) && stillArmed, tool.q + '; Esc closes the composer only');

    /* verdict and submit */
    PHASE = 'verdict and submit';
    await page.selectOption('[data-cf-verdict]', 'request-changes');
    await page.click('[data-cf-act="submit"]'); await sleep(150);
    const req = await page.evaluate(() => ({ label: document.querySelector('[data-cf-summary-label]').textContent, err: !document.querySelector('[data-cf-summary-error]').hidden }));
    rec('verdict.request-changes', req.label === 'Required change' && req.err, `label "${req.label}", submit blocked until a required change is written`);
    await page.fill('[data-cf-summary]', 'Name who merges on path B.');
    await page.click('[data-cf-act="submit"]'); await sleep(PAUSE);
    const sub = await page.evaluate(() => {
      const t = document.querySelector('.cf-toast'), pre = document.querySelector('.cf-envelope pre');
      let env = null; try { env = JSON.parse(pre.textContent); } catch (e) { }
      return { toast: t.textContent, env, badge: document.querySelector('[data-cf-badge]').textContent, markers: document.querySelectorAll('.cf-marker').length, v: document.querySelector('[data-cf-verdict]').value, label: document.querySelector('.cf-envelope summary').textContent };
    });
    rec('submit', /Review submitted: 3 notes, request changes/.test(sub.toast) && sub.env && sub.env.notes.length === 3 && sub.env.revision === 3 && sub.badge === '0' && sub.markers === 0 && sub.v === 'approve',
      `toast "${sub.toast}"; ${sub.label}: ${sub.env ? sub.env.notes.map(n => n.anchor.type).join(', ') : 'no JSON'}; badge ${sub.badge}; verdict reset ${sub.v}`);

    /* Esc ladder: settings, composer, float, tool, exit */
    PHASE = 'Esc ladder';
    await page.click('.cf-rail [data-cf-tool="element"]'); await sleep(150);
    await page.evaluate(() => {
      const p = document.querySelector('#blk-frame p'), n = p.firstChild, r = document.createRange(); r.setStart(n, 0); r.setEnd(n, 10);
      const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    });
    await sleep(PAUSE);
    await page.click('.cf-float'); await sleep(150);
    await page.click('[data-cf-open="display"]'); await sleep(PAUSE);
    const ladder = [];
    const state = () => page.evaluate(() => ({
      display: !document.querySelector('.cf-display').hidden && document.querySelector('.cf-display').classList.contains('is-open'),
      composer: !document.querySelector('.cf-composer').hidden, float: !document.querySelector('.cf-float').hidden,
      tool: document.querySelector('.cf-present-app').getAttribute('data-cf-tool'), armed: document.querySelector('.cf-present-app').getAttribute('data-cf-comment') === 'armed'
    }));
    ladder.push(await state());
    for (let i = 0; i < 5; i++) { await page.keyboard.press('Escape'); await sleep(PAUSE); ladder.push(await state()); }
    const s0 = ladder[0], L = ladder;
    const okLadder = s0.display && s0.composer && s0.float && s0.tool === 'element' && s0.armed &&
      !L[1].display && L[1].composer && !L[2].composer && L[2].float && !L[3].float && L[3].tool === 'element' && !L[4].tool && L[4].armed && !L[5].armed;
    const focusBack = await page.evaluate(() => document.activeElement.classList.contains('cf-comment-toggle'));
    const railGone = await page.evaluate(() => getComputedStyle(document.querySelector('.cf-rail')).visibility === 'hidden');
    rec('esc-ladder', okLadder && focusBack && railGone, 'settings, composer, float, tool, exit; focus returns to Comment; rail hidden');
  });
  rec('console-errors', page.errors.length === 0, page.errors.join(' | ') || '0 during the run');
  await ctx.close();
  return { rows };
}

async function motionRun(browser, name, width) {
  const { ctx, page } = await newPage(browser, width, { reducedMotion: 'reduce' });
  await load(page, name, 'graphite', 'light');
  if (name.startsWith('portal')) {
    await page.click('[data-cf-open="display"]'); await sleep(100);
    await page.keyboard.press('Escape');
    await blur(page); await page.keyboard.press('/'); await sleep(100);
    await page.keyboard.type('gate'); await sleep(100);
  } else {
    await page.click('.cf-comment-toggle'); await sleep(100);
  }
  const m = await page.evaluate(MOTION);
  const smooth = await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches);
  await ctx.close();
  return { ...m, matched: smooth, errors: page.errors };
}

/** Run every page, pair and width plus the reduced-motion probes. */
export async function runDesignKitCheck({ kit = KIT_DIR, only = '', log = console.log } = {}) {
  kitDir = kit;
  const browser = await chromium.launch({ env: hardenedChildEnvironment() });
  const report = { browser: `chromium ${browser.version()}`, runs: [], motion: [] };
  try {
    for (const [skin, theme] of PAIRS) for (const width of WIDTHS) {
      if (only && !`${skin}-${theme}-${width}`.includes(only)) continue;
      for (const [name, fn] of [['portal', portalRun], ['present', presentRun]]) {
        const { rows } = await fn(browser, skin, theme, width);
        const errs = rows.filter(r => r.ok === false);
        report.runs.push({ page: name, skin, theme, width, rows });
        log(`\n${errs.length ? 'FAIL' : 'PASS'} ${name} ${skin}-${theme} ${width}px (${rows.filter(r => r.ok === true).length} pass, ${errs.length} fail, ${rows.filter(r => r.ok === 'n/a').length} n/a)`);
        log('  ' + rows.map(r => `${r.name} ${r.ok === true ? 'ok' : r.ok === 'n/a' ? 'n/a' : 'FAIL'}`).join(', '));
        errs.forEach(r => log(`  FAIL ${r.name}: ${r.detail}`));
      }
    }
    for (const name of ['portal.reference.html', 'present.reference.html']) for (const width of WIDTHS) {
      const m = await motionRun(browser, name, width);
      const ok = m.matched && m.max === 0 && m.loops === 0 && m.errors.length === 0;
      report.motion.push({ name, width, ok, ...m });
      log(`${ok ? 'PASS' : 'FAIL'} reduced motion ${name} ${width}px: max transition or animation ${m.max}s${m.max ? ' on ' + m.worst : ''}, infinite loops ${m.loops}`);
    }
  } finally {
    await browser.close();
  }
  report.failures = [
    ...report.runs.flatMap(run => run.rows.filter(r => r.ok === false).map(r => `${run.page} ${run.skin}-${run.theme} ${run.width}px ${r.name}: ${r.detail}`)),
    ...report.motion.filter(m => !m.ok).map(m => `reduced motion ${m.name} ${m.width}px: ${m.max}s on ${m.worst}, ${m.loops} loops, ${m.errors.length} errors`),
  ];
  report.consoleErrors = report.runs.filter(run => run.rows.some(r => r.name === 'console-errors' && r.ok === false)).length;
  return report;
}

function headSha() {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: REPOSITORY, env: hardenedChildEnvironment(), encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const only = (process.argv.find(a => a.startsWith('--only=')) || '').slice(7);
  const report = await runDesignKitCheck({ only });
  console.log(`\nKit ${path.relative(REPOSITORY, KIT_DIR)} at ${headSha()}. ${report.browser}. Runs: ${report.runs.length}. Control failures: ${report.failures.length}. Runs with console errors: ${report.consoleErrors}.`);
  process.exitCode = report.failures.length || report.consoleErrors ? 1 : 0;
}

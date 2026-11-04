#!/usr/bin/env node
// Drives prototype.html from file:// and checks the operator's design decisions that AC-5 names, one line per check.
// Usage: node audit-decisions.cjs   (exits 1 on any FAIL; AUDIT_PAGE names another built page)
const path = require('path'); const fs = require('fs'); const { pathToFileURL } = require('url');
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const pagePath = path.resolve(process.env.AUDIT_PAGE || path.join(__dirname, '../prototype.html')); const URL = pathToFileURL(pagePath).href;
const rows = []; const check = (src, what, ok, note = '') => rows.push([ok ? 'PASS' : 'FAIL', src, what, note]);
const KINDS = ['Status report', 'Epic close', 'Security review', 'Weekly workspace digest', 'Design review', 'Feature', 'Hand-off', 'Incident', 'Research or decision', 'Small project'];
(async () => {
  const browser = await chromium.launch();
  try {
    const open = async (w, h = 900) => { const c = await browser.newContext({ viewport: { width: w, height: h }, colorScheme: 'light' }); const p = await c.newPage(); await p.goto(URL + '#overview', { waitUntil: 'load' }); await p.waitForTimeout(300); return p; };
    const page = await open(1440);
    const html = fs.readFileSync(pagePath, 'utf8');

    // TSK-1120 AC-10: one menu control
    const tg = await page.evaluate(() => { const t = document.querySelectorAll('[data-menu-toggle]'); const b = t[0]; return { n: t.length, name: b.getAttribute('aria-label'), exp: b.getAttribute('aria-expanded'), text: b.textContent.trim() }; });
    check('TSK-1120 AC-10', 'one icon-only menu control with a name and aria-expanded', tg.n === 1 && !!tg.name && tg.exp !== null && !tg.text, JSON.stringify(tg));
    await page.click('[data-menu-toggle]'); await page.waitForTimeout(350);
    check('TSK-1120 AC-10', 'desktop: the control collapses the menu to its rail', await page.evaluate(() => document.querySelector('.shell').dataset.menu === 'rail'));
    await page.click('[data-menu-toggle]'); await page.waitForTimeout(350);
    check('TSK-1120 AC-10', 'desktop: the control expands it again', await page.evaluate(() => document.querySelector('.shell').dataset.menu === 'full'));
    const phone = await open(390, 844); await phone.click('[data-menu-toggle]'); await phone.waitForTimeout(350);
    const dOpen = await phone.evaluate(() => document.querySelector('.shell').dataset.drawer === 'open'); await phone.click('[data-menu-toggle]', { force: true }).catch(() => {}); await phone.waitForTimeout(350);
    check('TSK-1120 AC-10', 'phone: the control opens and closes the drawer', dOpen && await phone.evaluate(() => document.querySelector('.shell').dataset.drawer !== 'open'));

    // TSK-1120 AC-11: one gear
    const bar = await page.evaluate(() => { const b = document.querySelector('.bar') || document.querySelector('header'); return { gears: document.querySelectorAll('[data-gear]').length, name: document.querySelector('[data-gear]').getAttribute('aria-label'), display: [...b.querySelectorAll('button')].some(x => /display/i.test(x.textContent)), mode: [...b.querySelectorAll('button')].some(x => /\b(mode|dark|light|moon)\b/i.test(x.textContent + (x.getAttribute('aria-label') || ''))) }; });
    check('TSK-1120 AC-11', 'one named gear, no Display text button, no mode button', bar.gears === 1 && !!bar.name && !bar.display && !bar.mode, JSON.stringify(bar));
    await page.focus('[data-gear]'); await page.keyboard.press('Enter'); await page.waitForTimeout(250);
    const order = await page.evaluate(() => [...document.querySelectorAll('.pop .kicker')].map(k => k.textContent.trim()));
    check('TSK-1120 AC-11', 'the popover opens by keyboard and lists Mode, Theme, Accent, Size in order', /^Mode,Themes?,Accent,(Text )?size$/i.test(order.join()), order.join());
    await page.keyboard.press('Escape'); await page.waitForTimeout(250);
    check('TSK-1120 AC-11', 'Escape closes it and returns focus to the gear', await page.evaluate(() => !document.querySelector('.pop').classList.contains('is-open') && document.activeElement === document.querySelector('[data-gear]')));
    await page.click('[data-gear]'); await page.waitForTimeout(250); await page.mouse.click(800, 600); await page.waitForTimeout(250);
    check('TSK-1120 AC-11', 'an outside click closes it', await page.evaluate(() => !document.querySelector('.pop').classList.contains('is-open')));
    await page.click('[data-gear]'); await page.waitForTimeout(250);
    const darkPill = page.locator('.pop').getByRole('radio', { name: 'Dark' }).or(page.locator('.pop').getByRole('button', { name: 'Dark' })).first();
    await darkPill.click(); await page.waitForTimeout(150);
    const applied = await page.evaluate(() => document.documentElement.dataset.mode === 'dark');
    await page.reload({ waitUntil: 'domcontentloaded' });
    check('TSK-1120 AC-11', 'a choice applies at once and is kept before first render', applied && await page.evaluate(() => document.documentElement.dataset.mode === 'dark'));
    await page.evaluate(() => localStorage.clear()); await page.reload({ waitUntil: 'load' }); await page.waitForTimeout(300);

    // TSK-1120 AC-12: current view marked
    await page.click('.menu a[data-view-link="since"], .menu [data-view-link="since"]').catch(() => {}); await page.waitForTimeout(350);
    check('TSK-1120 AC-12', 'the current view is marked with aria-current', await page.evaluate(() => !!document.querySelector('.menu [data-view-link="since"][aria-current]')));

    // TSK-1120 AC-13: resizable separator
    await page.goto(URL + '#overview', { waitUntil: 'load' }); await page.waitForTimeout(300);
    const grip = await page.evaluate(() => { const g = document.querySelector('.grip'); return g && { role: g.getAttribute('role'), now: g.getAttribute('aria-valuenow'), min: g.getAttribute('aria-valuemin'), max: g.getAttribute('aria-valuemax'), tab: g.tabIndex }; });
    check('TSK-1120 AC-13', 'a focusable separator with role and aria-valuenow', !!grip && grip.role === 'separator' && grip.now !== null && grip.tab === 0, JSON.stringify(grip));
    await page.focus('.grip'); const w0 = await page.evaluate(() => document.querySelector('.menu').getBoundingClientRect().width); await page.keyboard.press('ArrowRight'); await page.keyboard.press('ArrowRight'); await page.waitForTimeout(350);
    const w1 = await page.evaluate(() => document.querySelector('.menu').getBoundingClientRect().width);
    check('TSK-1120 AC-13', 'arrow keys resize the menu', w1 > w0, `${w0} to ${w1}`);
    for (let i = 0; i < 80; i++) await page.keyboard.press('ArrowRight'); await page.waitForTimeout(350);
    const wMax = await page.evaluate(() => document.querySelector('.menu').getBoundingClientRect().width);
    check('TSK-1120 AC-13', 'the width stops at its maximum', wMax <= 361, String(wMax));
    await page.reload({ waitUntil: 'load' }); await page.waitForTimeout(350);
    check('TSK-1120 AC-13', 'the width is kept per viewer', Math.abs(await page.evaluate(() => document.querySelector('.menu').getBoundingClientRect().width) - wMax) < 2);
    await page.evaluate(() => localStorage.clear()); await page.reload({ waitUntil: 'load' }); await page.waitForTimeout(300);

    // TSK-1120 AC-14: motion properties
    const layoutMotion = await page.evaluate(() => { const out = []; const bad = /(^|[\s,])(width|left|right|top|bottom|height|margin[a-z-]*|padding[a-z-]*|grid-template-columns|all)(?=[\s,]|$)/; const walk = rs => { for (const r of rs) { if (r.cssRules && !r.style) walk(r.cssRules); else if (r.style) { const p = r.style.transitionProperty || ''; if (p && bad.test(p)) out.push(`${r.selectorText}: ${p}`); else if (r.style.transition && bad.test(r.style.transition)) out.push(`${r.selectorText}: ${r.style.transition}`); } } }; for (const s of document.styleSheets) walk(s.cssRules); return out; });
    const outsidePanel = layoutMotion.filter(x => !/^\.(shell|menu)\b/.test(x));
    check('TSK-1120 AC-14', 'no layout property animates outside the sidebar panel', outsidePanel.length === 0, outsidePanel.join(' ; ') || `panel rules: ${layoutMotion.join(' ; ')}`.slice(0, 300));
    check('TSK-1120 AC-14', 'reduced motion is honoured in the sheet', /prefers-reduced-motion/.test(html));

    // TSK-1097 AC-11: no thick left border, across every view
    const thick = await page.evaluate(() => { const out = []; for (const e of document.querySelectorAll('.view .card,.view .fig,.view .note,.view .callout,.view .more,.view blockquote,.view [class*="callout"],.view [class*="note"]')) { const v = e.closest('.view'); v.classList.add('is-active'); const c = getComputedStyle(e), l = parseFloat(c.borderLeftWidth); const d = c.display !== 'none' && (l > 1 || (l !== parseFloat(c.borderRightWidth) && l > 0)); v.classList.remove('is-active'); if (d) out.push(v.id + ' ' + e.className); } return out; });
    check('TSK-1097 AC-11', 'no card, figure, note or callout has a thick or uneven left border', thick.length === 0, thick.slice(0, 5).join(' ; '));

    // TSK-1097 AC-14 and AC-15: figures, never Mermaid
    check('TSK-1097 AC-15', 'no Mermaid fence, block or script', !/class="[^"]*mermaid|<script[^>]*mermaid|language-mermaid|```mermaid/i.test(html), `${(html.match(/mermaid/gi) || []).length} prose mentions`);
    const figs = await page.evaluate(() => { const v = [...document.querySelectorAll('.view')]; const none = v.filter(x => !x.querySelector('.fig') && /^(overview|o-|t-|k-|f-)/.test(x.id)).map(x => x.id); const allFigs = [...document.querySelectorAll('.fig')]; const svgInline = allFigs.filter(f => f.querySelector('svg')).length; return { none, total: allFigs.length, svgInline }; });
    check('TSK-1097 AC-14', 'overview, epic, theme and task views carry a figure', figs.none.filter(x => !x.startsWith('f-')).length === 0, `${figs.total} figures, ${figs.svgInline} with inline SVG`);

    // TSK-1097 AC-17: pills
    const pills = await page.evaluate(() => { const bad = []; for (const v of document.querySelectorAll('.view')) { v.classList.add('is-active'); for (const row of v.querySelectorAll('.chips')) { const by = {}; for (const c of row.querySelectorAll('.chip')) { const r = c.getBoundingClientRect(); if (!r.height) continue; const y = Math.round(r.top); (by[y] ||= []).push(r.height); const lh = parseFloat(getComputedStyle(c).lineHeight) || r.height; if (r.height > lh * 1.9 + 12) bad.push(v.id + ' wraps'); } for (const hs of Object.values(by)) if (Math.max(...hs) - Math.min(...hs) > .5) bad.push(v.id + ' heights ' + hs.join()); } v.classList.remove('is-active'); } return bad; });
    check('TSK-1097 AC-17', 'pills sit on one line and share height per row, every view at 1440', pills.length === 0, pills.slice(0, 4).join(' ; '));

    // AC-5 itself: the ten kinds, the portal layers, What changed
    const kinds = await page.evaluate(() => document.getElementById('kinds').textContent);
    const missing = KINDS.filter(k => !kinds.toLowerCase().includes(k.toLowerCase()));
    check('AC-5', 'the ten report types are named on the Report kinds view', missing.length === 0, missing.join());
    check('AC-5', 'nine kinds open as their own views; the status report is the overview', await page.evaluate(() => document.querySelectorAll('.view[id^="s-"]').length === 9 && !!document.getElementById('overview')));
    const portal = await page.evaluate(() => document.getElementById('portal').textContent.toLowerCase());
    check('AC-5', 'the portal view has its three layers', ['overview', 'area', 'reference'].every(x => portal.includes(x)));
    check('AC-5', 'a What changed page for each surface', await page.evaluate(() => !!document.getElementById('since') && !!document.getElementById('docs-since')));
    const tok = await page.evaluate(() => { const a = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(); return { a, portal: !!document.querySelector('#docs .fig') }; });
    check('AC-5', 'the docs surface draws on the same page tokens', !!tok.a && tok.portal, tok.a);

    // The operator's entity-page row of 2026-10-31 18:4x and TSK-1098 AC-13
    const ent = await page.evaluate(() => { const first = (v, sel) => { const a = v.querySelector('p.one'), b = v.querySelector(sel); return !!a && !!b && !!(a.compareDocumentPosition(b) & 4); }; const out = { epic: 0, theme: 0, task: 0, feature: 0, bad: [] }; for (const v of document.querySelectorAll('.view')) { const k = v.id.startsWith('o-') ? 'epic' : v.id.startsWith('t-') ? 'theme' : v.id.startsWith('k-') ? 'task' : v.id.startsWith('f-') ? 'feature' : null; if (!k) continue; out[k]++; if (!first(v, k === 'theme' ? '.fig' : k === 'task' ? '.fig' : 'dl.facts')) out.bad.push(v.id); if (/>\s*None\s*</.test(v.innerHTML)) out.bad.push(v.id + ' None'); if (k === 'task' && ![...v.querySelectorAll('details.more h4')].some(h => h.textContent === 'Context')) out.bad.push(v.id + ' no Context'); } return out; });
    check('operator 2026-10-31 18:4x, entity pages', 'epic, theme, task and feature views open with what the item is about; no None group; tasks show Context', ent.bad.length === 0 && ent.feature > 0, JSON.stringify({ ...ent, bad: ent.bad.slice(0, 5) }));
    const chips = await page.evaluate(() => { const a = [...document.querySelectorAll('.view[id^="o-"] .chips a[data-view-link^="f-CAP-"]')]; return { n: a.length, dead: a.filter(x => !document.getElementById(x.dataset.viewLink)).length, inMenu: document.querySelectorAll('.menu [data-view-link^="f-CAP-"]').length }; });
    check('operator 2026-10-31 18:4x, entity pages', 'every capability chip opens a feature view and no menu row was added', chips.n > 0 && !chips.dead && !chips.inMenu, JSON.stringify(chips));
    // A record's list, table or subheading stays one on the page: never one paragraph with its marks left inside. All five entity kinds.
    const runOn = await page.evaluate(() => { const LIST = /^[-*+] \S|\s[-*+] \S.*\s[-*+] \S|(^|\s)1[.)] \S.*\s2[.)] \S|^\d+[.)] \S/, TABLE = /\|\s*:?-{3,}/, HEAD = /^#{1,6} \S/; const kinds = { 'outcome and epic': 'o-', theme: 't-', task: 'k-', feature: 'f-' }; const out = { lists: 0, bad: [] }; for (const [kind, pre] of Object.entries(kinds)) { out[kind] = 0; for (const v of document.querySelectorAll(`.view[id^="${pre}"]`)) { out[kind]++; out.lists += v.querySelectorAll('.more-body ul.rec-list, .more-body ol.rec-list').length; for (const p of v.querySelectorAll('p, dd')) { const t = p.textContent.trim().replace(/"[^"]*"/g, '""'); if (LIST.test(t) || TABLE.test(t) || HEAD.test(t)) out.bad.push(`${v.id}: ${t.slice(0, 80)}`); } } } return out; });
    check('Coordinator 2026-11-03, list breaks', 'a record list, table or subheading is never a run-on paragraph, on epic, outcome, theme, task and feature views', runOn.bad.length === 0 && runOn.lists > 0, JSON.stringify({ ...runOn, n: runOn.bad.length, bad: runOn.bad.slice(0, 6) }));
  } finally { await browser.close(); }
  const fails = rows.filter(r => r[0] === 'FAIL').length;
  for (const r of rows) console.log(`${r[0]}  ${r[1]}: ${r[2]}${r[3] ? `  [${r[3]}]` : ''}`);
  console.log(`audit-decisions: ${rows.length - fails} of ${rows.length} pass`);
  process.exitCode = fails ? 1 : 0;
})().catch(err => { console.error(err); process.exitCode = 2; });

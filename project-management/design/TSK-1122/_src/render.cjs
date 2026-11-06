#!/usr/bin/env node
// TSK-1122 prototype: captures and journeys. Opens ../prototype.html from file:// (no server), drives the views at
// 1440, 1280, 1200 and 390 px in light and dark, captures the first screen of each to ../captures/ (plus a full page
// at 390 px for the overview, an epic and a task), and fails on horizontal scroll, a console error, a wrapped chip, a
// box whose text overflows it, or a capture not at the top. Then the pointer, keyboard and phone journeys.
// Usage: node render.cjs   (Chromium runs outside the Bash sandbox)
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const path = require('path'); const fs = require('fs');
const ROOT = path.resolve(__dirname, '..'), CAP = path.join(ROOT, 'captures'); fs.mkdirSync(CAP, { recursive: true });
const URL = 'file://' + path.join(ROOT, 'prototype.html');
const VIEWS = ['overview', 'o-EPC-022', 'o-EPC-022#order', 'o-EPC-022#issues', 'o-EPC-025', 't-EPC-022-0', 'k-TSK-1122', 'epics', 'blockers', 'since', 'lanes', 'evidence', 'inputs', 'script', 'kinds', 's-security-review', 's-epic-close', 'portal', 'docs', 'docs#architecture', 'docs#technical', 'docs-since'];
const SIZES = [[1440, 900], [1280, 800], [1200, 800], [390, 844]];
const CHECK = () => {
  const sw = document.documentElement.scrollWidth, cw = document.documentElement.clientWidth; const wraps = [], overflow = [];
  const vis = el => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden'; };
  const act = document.querySelector('.view.is-active');
  for (const el of act.querySelectorAll('.chip, .tab')) { if (!vis(el)) continue; const r = el.getBoundingClientRect(); if (r.height > (el.classList.contains('tab') ? 42 : 30)) wraps.push(el.textContent.trim().slice(0, 50)); }
  for (const el of act.querySelectorAll('.card, .facts > div, .vt > div > span, .fig, .opt, .hero, .bar-row, .stream')) { if (!vis(el)) continue; if (el.scrollWidth > el.clientWidth + 2 && getComputedStyle(el).overflowX !== 'auto') overflow.push({ el: el.className, text: el.textContent.trim().slice(0, 50) }); }
  const cb = document.querySelector('.comment-btn'); let cbOverlap = false;
  if (cb) { const b = cb.getBoundingClientRect(); for (const el of act.querySelectorAll('.fig, .card, .vt, .facts, .hero, p, h2, h3, .bar-l, .bar-n, li, dd')) { if (!vis(el) || el.closest('details:not([open])')) continue; const r = el.getBoundingClientRect(); if (r.left < b.right && r.right > b.left && r.top < b.bottom && r.bottom > b.top) { cbOverlap = true; break; } } }
  return { hscroll: sw > cw + 1, sw, cw, wraps, overflow, cbOverlap, title: act ? act.dataset.title : null, h: document.documentElement.scrollHeight, top: window.scrollY };
};
(async () => {
  const browser = await chromium.launch(); const fails = []; const rows = []; let captures = 0;
  const SKIP = !!process.env.JOURNEYS_ONLY;
  for (const [W, H] of SKIP ? [] : SIZES) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: mode, deviceScaleFactor: 1 }); const page = await ctx.newPage(); const errs = [];
    page.on('pageerror', e => errs.push(e.message.split('\n')[0]));
    for (const v of VIEWS) {
      const [id, tab] = v.split('#');
      await page.goto(`${URL}?v=${VIEWS.indexOf(v)}#${id}`, { waitUntil: 'load' }); await page.evaluate(() => { try { localStorage.clear(); } catch {} });
      await page.evaluate(m => { document.documentElement.setAttribute('data-mode', m); }, mode);
      if (tab) await page.click(`#${id}-tab-${tab}`);
      await page.waitForTimeout(450); await page.evaluate(() => window.scrollTo(0, 0)); await page.waitForTimeout(100);
      const r = await page.evaluate(CHECK);
      const file = path.join(CAP, `${v.replace('#', '-')}-${W}-${mode}.png`); await page.screenshot({ path: file }); captures++;
      if (W === 390 && ['overview', 'o-EPC-022', 'k-TSK-1122'].includes(v)) { await page.screenshot({ path: path.join(CAP, `${v}-${W}-${mode}-full.png`), fullPage: true }); captures++; }
      rows.push([v, W, mode, r.hscroll ? `HSCROLL ${r.sw}>${r.cw}` : 'ok', `${r.wraps.length} wrapped chips`, `${r.overflow.length} overflow`, errs.length ? `${errs.length} errors` : 'no errors', r.cbOverlap ? 'COMMENT OVERLAPS' : '', `top ${r.top}`, r.h]);
      if (r.hscroll) fails.push(`${v} ${W} ${mode}: horizontal scroll ${r.sw} > ${r.cw}`);
      if (r.top !== 0) fails.push(`${v} ${W} ${mode}: capture not at the top (${r.top})`);
      if (r.cbOverlap) fails.push(`${v} ${W} ${mode}: the Comment button overlaps content`);
      for (const w of r.wraps) fails.push(`${v} ${W} ${mode}: chip wraps: ${w}`);
      for (const w of r.overflow) fails.push(`${v} ${W} ${mode}: text overflows ${w.el}: ${w.text}`);
    }
    if (errs.length) fails.push(`${W} ${mode}: page errors: ${[...new Set(errs)].join(' | ')}`);
    await ctx.close();
  }
  // the three themes and the large text size at 1200 and 390, light and dark (fit only; one capture per theme)
  for (const theme of SKIP ? [] : ['graphite', 'slate', 'sage']) for (const mode of ['light', 'dark']) for (const size of ['default', 'large']) for (const W of [1200, 390]) {
    const ctx = await browser.newContext({ viewport: { width: W, height: 800 }, colorScheme: mode }); const page = await ctx.newPage();
    for (const v of ['overview', 'o-EPC-022', 'k-TSK-1122']) {
      await page.goto(`${URL}?t=${theme}${size}${mode}${W}#${v}`, { waitUntil: 'load' });
      await page.evaluate(([m, t, s]) => { const r = document.documentElement; r.setAttribute('data-mode', m); r.setAttribute('data-theme', t); r.setAttribute('data-size', s); }, [mode, theme, size]); await page.waitForTimeout(300);
      const r = await page.evaluate(CHECK); rows.push([`${v} ${theme} ${size}`, W, mode, r.hscroll ? 'HSCROLL' : 'ok', `${r.wraps.length} wrapped chips`, `${r.overflow.length} overflow`, '', r.cbOverlap ? 'COMMENT OVERLAPS' : '', '', r.h]);
      if (r.hscroll) fails.push(`${v} ${theme} ${size} ${W} ${mode}: horizontal scroll`); for (const w of r.wraps) fails.push(`${v} ${theme} ${size} ${W} ${mode}: chip wraps: ${w}`); for (const w of r.overflow) fails.push(`${v} ${theme} ${size} ${W} ${mode}: overflow ${w.el}: ${w.text}`);
      if (v === 'overview' && size === 'large' && mode === 'light' && W === 1200 && theme !== 'graphite') { await page.screenshot({ path: path.join(CAP, `overview-1200-${theme}-large-light.png`) }); captures++; }
    }
    await ctx.close();
  }
  // every view at 1440, 1280, 1200 and 390 px: the Comment button covers no visible content, at the top and at the end of the page
  const OVERLAP = () => { const cb = document.querySelector('.comment-btn'); if (!cb) return { views: 0, hits: ['no Comment button'] }; const hits = []; let views = 0;
    const vis = el => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden'; };
    for (const v of document.querySelectorAll('.view')) { document.querySelectorAll('.view.is-active').forEach(x => x.classList.remove('is-active')); v.classList.add('is-active'); views++;
      // at the top, the middle and the end of the page the button covers no element of the view; on desktop it also stays right of the page's content box
      const pg = document.querySelector('.page'); let hit = null;
      for (const f of [0, 0.5, 1]) { window.scrollTo(0, f * (document.documentElement.scrollHeight - innerHeight)); const b = cb.getBoundingClientRect();
        if (innerWidth >= 768) { const p = pg.getBoundingClientRect(); const contentRight = p.right - parseFloat(getComputedStyle(pg).paddingRight); if (b.left < contentRight - 1) { hit = `${v.id}: the button reaches into the content box by ${Math.round(contentRight - b.left)} px`; break; } }
        for (const el of v.querySelectorAll('.fig, .card, .vt, .facts, .hero, p, h2, h3, .bar-l, .bar-n, li, dd, .chip, .tab, .part, svg')) { if (!vis(el) || el.closest('details:not([open])')) continue; const r = el.getBoundingClientRect(); if (r.left < b.right && r.right > b.left && r.top < b.bottom && r.bottom > b.top) { hit = `${v.id}: ${el.className || el.tagName} "${el.textContent.trim().slice(0, 40)}" at scroll ${f}`; break; } }
        if (hit) break; }
      if (hit) hits.push(hit); }
    window.scrollTo(0, 0); return { views, hits }; };
  let overlapTotal = 0;
  for (const W of [1440, 1280, 1200, 390]) { const c = await browser.newContext({ viewport: { width: W, height: W === 390 ? 844 : 900 } }); const p = await c.newPage(); await p.goto(`${URL}?overlap${W}`, { waitUntil: 'load' }); await p.waitForTimeout(300); const r = await p.evaluate(OVERLAP); overlapTotal += r.hits.length; console.log(`${W} px: ${r.views} views, ${r.hits.length} views where the Comment button covers content`); for (const h of r.hits.slice(0, 10)) fails.push(`${W} px Comment overlap: ${h}`); await c.close(); }
  // journeys at 1440 light: pointer drill-down, then the keyboard
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } }); const page = await ctx.newPage();
  await page.goto(URL, { waitUntil: 'load' }); await page.waitForTimeout(300);
  const j = [];
  const active = () => page.evaluate(() => document.querySelector('.view.is-active').id);
  const focused = () => page.evaluate(() => { const a = document.activeElement; return a ? (a.id || a.className || a.tagName) + '|' + (a.textContent || '').trim().slice(0, 40) : null; });
  j.push(['the page opened from file:// with no server', page.url().startsWith('file://') && await page.evaluate(() => document.querySelectorAll('.view').length > 300)]);
  j.push(['overview: a figure is the main view of where the tasks are', await page.evaluate(() => { const v = document.getElementById('overview'); const f = v.querySelector('.fig'); return f && f.querySelector('.part') && f.querySelector('.fig-data table'); })]);
  await page.click('a.outcome[data-view-link="o-EPC-022"]'); await page.waitForTimeout(400); j.push(['outcome card opens the epic view', await active() === 'o-EPC-022']);
  j.push(['epic view: one line, then the strip of three facts, then the progress figure', await page.evaluate(() => { const v = document.querySelector('.view.is-active'); const o = v.querySelector('p.one'), s = v.querySelector('.facts'), f = v.querySelector('.fig'); return o && s && f && s.querySelectorAll(':scope > div').length === 3 && (o.compareDocumentPosition(s) & 4) && (s.compareDocumentPosition(f) & 4); })]);
  j.push(['view change moves focus to the new heading', /^H2\|/.test(await focused())]);
  await page.click('#o-EPC-022-tab-order'); j.push(['Order tab shows its panel', await page.evaluate(() => !document.getElementById('o-EPC-022-order').hidden && document.getElementById('o-EPC-022-progress').hidden)]);
  j.push(['tabs keep one tab stop (roving tabindex)', await page.evaluate(() => [...document.querySelectorAll('#o-EPC-022 [role=tab]')].map(t => t.tabIndex).join() === '-1,0,-1')]);
  await page.keyboard.press('ArrowRight'); j.push(['ArrowRight on a tab selects the next one', await page.evaluate(() => document.getElementById('o-EPC-022-tab-issues').getAttribute('aria-selected') === 'true' && document.activeElement.id === 'o-EPC-022-tab-issues')]);
  await page.click('#o-EPC-022-tab-progress'); await page.click('#o-EPC-022 .bars .bar-l a[data-view-link]'); await page.waitForTimeout(400); j.push(['theme bar opens the theme view', /^t-EPC-022-/.test(await active())]);
  await page.click('.view.is-active .vt.tasks a[data-view-link]'); await page.waitForTimeout(400); j.push(['task row opens the task view', /^k-TSK-/.test(await active())]);
  j.push(['task view: one line, the stage figure, then a strip of 8 facts', await page.evaluate(() => { const v = document.querySelector('.view.is-active'); const o = v.querySelector('p.one'), f = v.querySelector('.fig .path'), s = v.querySelector('.facts'); return o && f && s && (o.compareDocumentPosition(f) & 4) && (f.compareDocumentPosition(s) & 4) && s.querySelectorAll(':scope > div').length === 8; })]);
  j.push(['task view: the disclosure holds the record\u2019s Outcome and Context', await page.evaluate(() => { const hs = [...document.querySelectorAll('#k-TSK-1122 details.more h4')].map(h => h.textContent); return hs.includes('Outcome') && hs.includes('Context') && document.querySelector('#k-TSK-1122 details.more').textContent.includes('design pass of'); })]);
  j.push(['breadcrumb has 4 levels, each in words', await page.evaluate(() => { const c = [...document.querySelectorAll('.view.is-active .crumbs a, .view.is-active .crumbs [aria-current]')]; return c.length === 4 && c.every(x => !/^(TSK|EPC)-\d+$/.test(x.textContent.trim())); })]);
  await page.click('.view.is-active .crumbs a[data-view-link^="o-"]'); await page.waitForTimeout(400); j.push(['breadcrumb returns to the epic', await active() === 'o-EPC-022']);
  await page.click('#o-EPC-022 .chips a[data-view-link^="f-CAP-"]'); await page.waitForTimeout(400); j.push(['capability chip opens its feature view', /^f-CAP-/.test(await active())]);
  j.push(['feature view: what it is, then its state, with no empty group and no menu row', await page.evaluate(() => { const v = document.querySelector('.view.is-active'), o = v.querySelector('p.one'), s = v.querySelector('dl.facts'); return !!o && !!s && o.textContent.trim().length > 20 && !!(o.compareDocumentPosition(s) & 4) && !/\bNone\b/.test(v.textContent) && !document.querySelector(`#menu a[data-view-link="${v.id}"], .menu a[data-view-link="${v.id}"]`); })]);
  await page.click('.view.is-active .crumbs a[data-view-link="overview"]'); await page.waitForTimeout(400);
  await page.click('[data-view-link="o-EPC-025"]'); await page.waitForTimeout(400); j.push(['the coordination ledger epic shows for whom from its Outcome, not "not recorded"', await page.evaluate(() => { const dd = [...document.querySelectorAll('#o-EPC-025 .facts dd')].map(d => d.textContent); return dd.length === 3 && dd.every(t => !/not recorded/.test(t)) && /session/.test(dd[1]); })]);
  await page.click('.nav-area[data-area="act"]'); await page.waitForTimeout(200); j.push(['rail icon selects the area and the panel lists it', await page.evaluate(() => document.querySelector('.menu').dataset.area === 'act')]);
  await page.click('[data-view-link="inputs"]'); await page.waitForTimeout(400); await page.click('#decisions input[name=tree][value=approve]'); await page.click('[data-view-link="script"]'); await page.waitForTimeout(300);
  j.push(['an answer appears in the script', await page.evaluate(() => /tree: approve/.test(document.getElementById('script-text').textContent))]);
  await page.screenshot({ path: path.join(CAP, 'journey-script-1440-light.png') }); captures++;
  await page.focus('[data-gear]'); await page.keyboard.press('Enter'); await page.waitForTimeout(150); j.push(['gear opens the dialog and focus moves into it', await page.evaluate(() => document.querySelector('.pop').classList.contains('is-open') && document.querySelector('.pop').contains(document.activeElement))]);
  await page.keyboard.press('Escape'); await page.waitForTimeout(100); j.push(['Escape closes the dialog and returns focus to the gear', await page.evaluate(() => !document.querySelector('.pop').classList.contains('is-open') && document.activeElement === document.querySelector('[data-gear]'))]);
  // report kinds: navigable specimens with their own tabs
  await page.click('.nav-area[data-area="kinds"]'); await page.waitForTimeout(150); await page.click('[data-view-link="kinds"]'); await page.waitForTimeout(300);
  await page.click('#kinds a.kind[data-view-link="s-security-review"]'); await page.waitForTimeout(400); j.push(['a report kind card opens its specimen view', await active() === 's-security-review']);
  j.push(['the specimen has one tab per view kind and the first panel holds a figure or table, not a drawing of a page', await page.evaluate(() => { const v = document.getElementById('s-security-review'); const tabs = v.querySelectorAll('[role=tab]'); const p = v.querySelector('[role=tabpanel]:not([hidden])'); return tabs.length >= 3 && p && (p.querySelector('.fig, .vt, .facts, .form')); })]);
  const secondTab = await page.$('#s-security-review [role=tab]:nth-child(2)'); await secondTab.click(); j.push(['a specimen tab switches its panel', await page.evaluate(() => { const v = document.getElementById('s-security-review'); const open = [...v.querySelectorAll('[role=tabpanel]')].filter(p => !p.hidden); return open.length === 1 && open[0].id === v.querySelectorAll('[role=tab]')[1].getAttribute('aria-controls'); })]);
  j.push(['the nine other report kinds have specimen views with tabs', await page.evaluate(() => { const vs = document.querySelectorAll('.view[id^="s-"]'); return vs.length === 9 && [...vs].every(v => v.querySelectorAll('[role=tab]').length >= 2); })]);
  // the docs topic: three tabs, each opening with a figure and no prose before it
  await page.click('[data-view-link="docs"]'); await page.waitForTimeout(400);
  j.push(['docs topic: Concept, Architecture, Technical tabs', await page.evaluate(() => [...document.querySelectorAll('#docs [role=tab]')].map(t => t.textContent.trim()).join() === 'Concept,Architecture,Technical')]);
  j.push(['each docs tab opens with its figure as the first element', await page.evaluate(() => [...document.querySelectorAll('#docs [role=tabpanel]')].every(p => p.firstElementChild && p.firstElementChild.classList.contains('fig')))]);
  j.push(['every numbered figure has a title, a description or label, and a data table', await page.evaluate(() => [...document.querySelectorAll('.fig')].filter(f => /Figure \d/.test((f.querySelector('.fig-n') || {}).textContent || '')).every(f => (f.querySelector('svg title') && f.querySelector('svg desc') || f.querySelector('[role=img][aria-label], [role=group][aria-label]')) && f.querySelector('.fig-data table')))]);
  j.push(['every figure has an accessible name', await page.evaluate(() => [...document.querySelectorAll('.fig')].every(f => f.querySelector('svg title') || f.querySelector('[role=img][aria-label], [role=group][aria-label]')))]);
  j.push(['no visible time without a zone', await page.evaluate(() => { const w = document.createTreeWalker(document.querySelector('.page'), NodeFilter.SHOW_TEXT); const bad = []; let n; while ((n = w.nextNode())) { if (n.parentElement.closest('script, style, pre, code')) continue; const body = n.parentElement.closest('.more-body'), ctx = body && [...body.querySelectorAll('h4')].find(h => h.textContent === 'Context'); if (ctx && (ctx.compareDocumentPosition(n) & 4)) continue; /* Context is the record's own words, kept verbatim */ const t = n.textContent; for (const m of t.matchAll(/\b\d{1,2}:\d\d\b(?!\d)/g)) { const after = t.slice(m.index + m[0].length, m.index + m[0].length + 6); if (!/^\s*(EST|EDT|UTC|[+-]\d\d)/.test(after)) bad.push(t.trim().slice(0, 60)); } } return bad.length === 0; })]);
  await ctx.close();
  // the phone: drawer, labels on stacked tables and figure tables, the Comment button in the flow
  const ctx2 = await browser.newContext({ viewport: { width: 390, height: 844 } }); const p2 = await ctx2.newPage(); await p2.goto(URL, { waitUntil: 'load' }); await p2.waitForTimeout(300);
  await p2.click('[data-menu-toggle]'); await p2.waitForTimeout(400); j.push(['390 px: the toggle opens the drawer', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer === 'open')]);
  j.push(['390 px: the page behind the drawer is inert and focus is in the drawer', await p2.evaluate(() => document.querySelector('.main').hasAttribute('inert') && document.querySelector('.menu').contains(document.activeElement))]);
  await p2.screenshot({ path: path.join(CAP, 'drawer-390-light.png') }); captures++;
  await p2.keyboard.press('Escape'); await p2.waitForTimeout(400); j.push(['390 px: Escape closes it, the page is live again and focus returns to the toggle', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer !== 'open' && !document.querySelector('.main').hasAttribute('inert') && document.activeElement === document.querySelector('[data-menu-toggle]'))]);
  await p2.goto(`${URL}?p1#since`, { waitUntil: 'load' }); await p2.waitForTimeout(300);
  j.push(['390 px: a stacked visual-table row shows its column labels', await p2.evaluate(() => { const c = document.querySelector('#since .vt [role=row]:not(.vt-head) [role=cell][data-l="Now"]'); return c && getComputedStyle(c, '::before').content.includes('Now'); })]);
  await p2.goto(`${URL}?p2#docs-since`, { waitUntil: 'load' }); await p2.waitForTimeout(300); await p2.click('#docs-since .fig-data summary'); await p2.waitForTimeout(200);
  j.push(['390 px: an opened figure table keeps its field labels on each cell', await p2.evaluate(() => { const c = document.querySelector('#docs-since .fig-data td[data-l="Added"]'); return c && getComputedStyle(c, '::before').content.includes('Added') && c.getBoundingClientRect().height > 0; })]);
  await p2.screenshot({ path: path.join(CAP, 'docs-since-table-390-light.png') }); captures++;
  j.push(['390 px: the Comment button sits in the flow after the page, not over it', await p2.evaluate(() => { const cb = document.querySelector('.comment-btn'); return cb && getComputedStyle(cb).position === 'static'; })]);
  j.push(['every view at four widths: the Comment button covers no content', overlapTotal === 0]);
  await ctx2.close();
  // the collapsed rail: hovering opens the panel after one short delay and shows no tooltip; focus opens it at once; the pinned menu keeps its tooltip
  const ctx3 = await browser.newContext({ viewport: { width: 1440, height: 900 } }); const p3 = await ctx3.newPage(); await p3.goto(`${URL}?hover`, { waitUntil: 'load' }); await p3.waitForTimeout(300);
  const tipOn = () => p3.evaluate(() => !!document.querySelector('.nav-tip.is-on'));
  const peekOpen = () => p3.evaluate(() => document.querySelector('.shell').dataset.peek === 'open');
  const icon = async area => { const b = await p3.$(`.nav-area[data-area="${area}"]`); const r = await b.boundingBox(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
  let i = await icon('across'); await p3.mouse.move(i.x, i.y); await p3.waitForTimeout(80);
  j.push(['pinned menu: hovering a rail icon shows a tooltip and opens no panel', (await tipOn()) && !(await peekOpen())]);
  await p3.mouse.move(700, 500); await p3.click('[data-menu-toggle]'); await p3.waitForTimeout(500);
  j.push(['collapsed rail: the menu is a rail', await p3.evaluate(() => document.querySelector('.shell').dataset.menu === 'rail')]);
  i = await icon('across'); await p3.mouse.move(i.x, i.y); await p3.waitForTimeout(80);
  j.push(['collapsed rail: 80 ms after hover there is no tooltip and no panel yet', !(await tipOn()) && !(await peekOpen())]);
  await p3.mouse.move(i.x + 400, i.y); await p3.waitForTimeout(400);
  j.push(['collapsed rail: a passing mouse (gone before 200 ms) opens nothing', !(await peekOpen())]);
  i = await icon('across'); await p3.mouse.move(i.x, i.y); await p3.waitForTimeout(450);
  const frames = []; for (let k = 0; k < 6; k++) { frames.push([await tipOn(), await peekOpen()]); await p3.waitForTimeout(60); }
  j.push(['collapsed rail: after the delay the panel is open and at no point was a tooltip shown', (await peekOpen()) && frames.every(f => !f[0]) && frames.some(f => f[1])]);
  await p3.screenshot({ path: path.join(CAP, 'rail-hover-peek-1440-light.png') }); captures++;
  await p3.mouse.move(700, 500); await p3.waitForTimeout(500); j.push(['collapsed rail: leaving closes the panel', !(await peekOpen())]);
  await p3.focus('.nav-area[data-area="act"]'); await p3.waitForTimeout(30);
  j.push(['collapsed rail: keyboard focus opens the panel at once with no tooltip', (await peekOpen()) && !(await tipOn()) && await p3.evaluate(() => document.querySelector('.menu').dataset.area === 'act')]);
  await p3.focus('[data-gear]'); await p3.waitForTimeout(300); j.push(['collapsed rail: focus leaving the rail closes the panel', !(await peekOpen())]);
  await ctx3.close();
  // answers: Save writes answers.json through a directory handle, confirms with the file and the time; a draft survives a reload
  const FAKE_FS = () => { window.__written = {}; const dir = (p) => ({ kind: 'directory', getDirectoryHandle: async (n, o) => { if (p === '' && n === 'coordination') return dir('coordination'); if (!o || !o.create) { if (n === 'coordination') return dir(n); throw new Error('no ' + n); } return dir(p ? p + '/' + n : n); }, getFileHandle: async (n) => ({ createWritable: async () => ({ write: async t => { window.__written[(p ? p + '/' : '') + n] = t; }, close: async () => {} }) }), queryPermission: async () => 'granted', requestPermission: async () => 'granted' }); window.showDirectoryPicker = async () => dir(''); };
  const ctx4 = await browser.newContext({ viewport: { width: 1440, height: 900 } }); await ctx4.addInitScript(FAKE_FS); const p4 = await ctx4.newPage(); await p4.goto(`${URL}?save#inputs`, { waitUntil: 'load' }); await p4.waitForTimeout(300);
  await p4.click('#decisions input[name=tree][value=approve]'); await p4.fill('#changes', 'Keep the lane for the Comment button.'); await p4.waitForTimeout(100);
  await p4.reload({ waitUntil: 'load' }); await p4.waitForTimeout(300);
  j.push(['answers: a reload keeps the draft (the radio and the typed text)', await p4.evaluate(() => document.querySelector('#decisions input[name=tree][value=approve]').checked && document.getElementById('changes').value === 'Keep the lane for the Comment button.')]);
  await p4.click('[data-save="changes"]'); await p4.waitForTimeout(400);
  const written = await p4.evaluate(() => window.__written);
  const file = written['coordination/coordinators/visual-docs/published/status-prototype/answers.json']; let parsed = null; try { parsed = JSON.parse(file); } catch {}
  j.push(['answers: Save writes answers.json at the repository path through the directory handle', !!parsed]);
  j.push(['answers: the file carries id, label, text, page and time for each answered field', !!parsed && parsed.answers.length === 2 && parsed.answers.every(a => a.id && a.label && a.text && a.page && /^\d{4}-\d\d-\d\d \d\d:\d\d [A-Z]{2,5}$/.test(a.time)) && parsed.answers.some(a => a.id === 'changes' && a.text === 'Keep the lane for the Comment button.') && parsed.answers.some(a => a.id === 'tree' && a.value === 'approve')]);
  const conf = await p4.evaluate(() => document.getElementById('saved-changes').textContent);
  j.push(['answers: the confirmation names the file and the wall-clock time with a zone: ' + conf, /Saved to coordination\/coordinators\/visual-docs\/published\/status-prototype\/answers\.json at \d\d:\d\d [A-Z]{2,5}, \d{1,2} [A-Z][a-z]{2}\./.test(conf)]);
  await p4.evaluate(() => document.getElementById('saved-changes').scrollIntoView({ block: 'center' })); await p4.waitForTimeout(150); await p4.screenshot({ path: path.join(CAP, 'answers-saved-1440-light.png') }); captures++;
  await p4.click('[data-save="tree"]'); await p4.waitForTimeout(300);
  j.push(['answers: every field has its own Save button and confirmation', (await p4.evaluate(() => document.querySelectorAll('#decisions [data-save]').length)) === 5 && /Saved to/.test(await p4.evaluate(() => document.getElementById('saved-tree').textContent))]);
  // a draft with quotes, a backslash and two lines survives a reload and Save still works (the restore ran by name)
  const AWKWARD = 'Use "Overview" first\\ then \'Lanes\'\nsecond line';
  await p4.fill('#changes', AWKWARD); await p4.waitForTimeout(100); await p4.reload({ waitUntil: 'load' }); await p4.waitForTimeout(300);
  j.push(['answers: a reload keeps a draft with quotes, a backslash and a newline', await p4.evaluate(a => document.getElementById('changes').value === a && document.querySelector('#decisions input[name=tree][value=approve]').checked, AWKWARD)]);
  await p4.evaluate(() => { window.__written = {}; }); await p4.click('[data-save="changes"]'); await p4.waitForTimeout(400);
  const written2 = await p4.evaluate(() => window.__written); let parsed2 = null; try { parsed2 = JSON.parse(written2['coordination/coordinators/visual-docs/published/status-prototype/answers.json']); } catch {}
  j.push(['answers: after that reload Save still writes the file with the awkward text intact', !!parsed2 && parsed2.answers.some(a => a.id === 'changes' && a.text === AWKWARD)]);
  await ctx4.close();
  // the fallback: no File System Access API, so Save downloads answers.json and says where it went
  const ctx5 = await browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true }); await ctx5.addInitScript(() => { try { delete window.showDirectoryPicker; } catch {} Object.defineProperty(window, 'showDirectoryPicker', { value: undefined, configurable: true }); });
  const p5 = await ctx5.newPage(); await p5.goto(`${URL}?fallback#inputs`, { waitUntil: 'load' }); await p5.waitForTimeout(300);
  await p5.fill('#changes', 'Fallback path.'); const dl = p5.waitForEvent('download'); await p5.click('[data-save="changes"]'); const d = await dl; const dlText = fs.readFileSync(await d.path(), 'utf8'); let dlParsed = null; try { dlParsed = JSON.parse(dlText); } catch {}
  j.push(['fallback: Save downloads answers.json with the same content', d.suggestedFilename() === 'answers.json' && !!dlParsed && dlParsed.answers.some(a => a.id === 'changes' && a.text === 'Fallback path.')]);
  const conf5 = await p5.evaluate(() => document.getElementById('saved-changes').textContent);
  j.push(['fallback: the confirmation says the file was downloaded and names where to move it: ' + conf5, /downloaded to your browser's download folder at \d\d:\d\d [A-Z]{2,5}/.test(conf5) && /move it to coordination\/coordinators\/visual-docs\/published\/status-prototype\/answers\.json/.test(conf5)]);
  await p5.screenshot({ path: path.join(CAP, 'answers-fallback-1440-light.png') }); captures++;
  // the same awkward draft on the download path: reload, then Save still downloads
  await p5.fill('#changes', AWKWARD); await p5.waitForTimeout(100); await p5.reload({ waitUntil: 'load' }); await p5.waitForTimeout(300);
  const dl2 = p5.waitForEvent('download'); await p5.click('[data-save="changes"]'); const d2 = await dl2; let dlParsed2 = null; try { dlParsed2 = JSON.parse(fs.readFileSync(await d2.path(), 'utf8')); } catch {}
  j.push(['fallback: after a reload with the awkward draft Save still downloads it intact', !!dlParsed2 && dlParsed2.answers.some(a => a.id === 'changes' && a.text === AWKWARD)]);
  await ctx5.close(); await browser.close();
  console.log(rows.map(r => r.join('  ')).join('\n')); console.log(j.map(([k, ok]) => (ok ? 'PASS ' : 'FAIL ') + k).join('\n'));
  for (const [k, ok] of j) if (!ok) fails.push('journey: ' + k);
  if (fails.length) { console.log('\nFAILURES\n' + fails.join('\n')); process.exit(1); } console.log(`\n${rows.length} renders checked, ${captures} captures in ${CAP}, ${j.length} of ${j.length} journeys, 0 failures`);
})();

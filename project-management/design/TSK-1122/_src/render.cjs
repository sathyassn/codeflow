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
  if (cb && cw < 768) { const b = cb.getBoundingClientRect(); for (const el of act.querySelectorAll('.fig, .card, .vt, .facts, .hero, p, h2, h3')) { if (!vis(el) || el.closest('details:not([open])')) continue; const r = el.getBoundingClientRect(); if (r.left < b.right && r.right > b.left && r.top < b.bottom && r.bottom > b.top) { cbOverlap = true; break; } } }
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
  j.push(['breadcrumb has 4 levels, each in words', await page.evaluate(() => { const c = [...document.querySelectorAll('.view.is-active .crumbs a, .view.is-active .crumbs [aria-current]')]; return c.length === 4 && c.every(x => !/^(TSK|EPC)-\d+$/.test(x.textContent.trim())); })]);
  await page.click('.view.is-active .crumbs a[data-view-link^="o-"]'); await page.waitForTimeout(400); j.push(['breadcrumb returns to the epic', await active() === 'o-EPC-022']);
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
  j.push(['no visible time without a zone', await page.evaluate(() => { const w = document.createTreeWalker(document.querySelector('.page'), NodeFilter.SHOW_TEXT); const bad = []; let n; while ((n = w.nextNode())) { if (n.parentElement.closest('script, style, pre, code')) continue; const t = n.textContent; for (const m of t.matchAll(/\b\d{1,2}:\d\d\b(?!\d)/g)) { const after = t.slice(m.index + m[0].length, m.index + m[0].length + 6); if (!/^\s*(EST|EDT|UTC|[+-]\d\d)/.test(after)) bad.push(t.trim().slice(0, 60)); } } return bad.length === 0; })]);
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
  await ctx2.close(); await browser.close();
  console.log(rows.map(r => r.join('  ')).join('\n')); console.log(j.map(([k, ok]) => (ok ? 'PASS ' : 'FAIL ') + k).join('\n'));
  for (const [k, ok] of j) if (!ok) fails.push('journey: ' + k);
  if (fails.length) { console.log('\nFAILURES\n' + fails.join('\n')); process.exit(1); } console.log(`\n${rows.length} renders checked, ${captures} captures in ${CAP}, ${j.length} of ${j.length} journeys, 0 failures`);
})();

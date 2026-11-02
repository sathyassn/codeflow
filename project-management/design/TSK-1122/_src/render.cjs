#!/usr/bin/env node
// TSK-1122 prototype: captures and checks. Serves the folder, drives every view at 1440, 1200 and 390 px in light and
// dark, captures each to ../captures/ from the top of the page (desktop: the first screen; phone: the full page), and
// fails on horizontal scroll, a console error, a paragraph over 3 lines in a card, a chip that wraps to two lines, or
// a box whose text overflows it. Then drives the pointer and keyboard journeys: drill-down, tabs, rail, answers, drawer,
// gear focus, view-change focus. Usage: node render.cjs   (Chromium runs outside the Bash sandbox)
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const path = require('path'); const fs = require('fs'); const http = require('http');
const ROOT = path.resolve(__dirname, '..'), CAP = path.join(ROOT, 'captures'); fs.mkdirSync(CAP, { recursive: true });
const VIEWS = ['overview', 'o-EPC-022', 'o-EPC-022#order', 'o-EPC-022#issues', 'o-EPC-024', 't-EPC-022-0', 'k-TSK-1122', 'epics', 'blockers', 'since', 'lanes', 'evidence', 'inputs', 'script', 'kinds', 's-security-review', 's-incident', 'portal', 'docs', 'docs#architecture', 'docs#technical', 'docs-since'];
const SIZES = [[1440, 900], [1200, 800], [390, 844]];
const srv = http.createServer((q, r) => { const f = path.join(ROOT, decodeURIComponent(q.url.split('?')[0].split('#')[0]) === '/' ? 'prototype.html' : q.url.split('?')[0]); if (!f.startsWith(ROOT) || !fs.existsSync(f)) { r.writeHead(404); return r.end(); } r.writeHead(200, { 'content-type': f.endsWith('.html') ? 'text/html' : 'application/octet-stream' }); r.end(fs.readFileSync(f)); }).listen(0, '127.0.0.1');
const CHECK = () => {
  const sw = document.documentElement.scrollWidth, cw = document.documentElement.clientWidth; const walls = [], wraps = [], overflow = [];
  const vis = el => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden'; };
  const act = document.querySelector('.view.is-active');
  for (const el of act.querySelectorAll(':is(.card, .note, .strip, .vt, .fig) :is(p, li, dd)')) { if (!vis(el) || el.closest('.about, .about-rows, .fig-data, details:not([open])')) continue; const cs = getComputedStyle(el); const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.5; const lines = Math.round(el.getBoundingClientRect().height / lh); if (lines > 3) walls.push({ lines, text: el.textContent.trim().slice(0, 70) }); }
  for (const el of act.querySelectorAll('.chip, .pill, .tab')) { if (!vis(el)) continue; const r = el.getBoundingClientRect(); const max = el.classList.contains('tab') ? 42 : el.classList.contains('pill') ? 32 : 30; if (r.height > max) wraps.push(el.textContent.trim().slice(0, 50)); }
  for (const el of act.querySelectorAll('.card, .note, .strip > div, .vt > div > span, .fig, .opt, pre')) { if (!vis(el)) continue; if (el.scrollWidth > el.clientWidth + 2 && !el.closest('.scroll-x') && el.tagName !== 'PRE' && getComputedStyle(el).overflowX !== 'auto') overflow.push({ el: el.className, text: el.textContent.trim().slice(0, 50) }); }
  return { hscroll: sw > cw + 1, sw, cw, walls, wraps, overflow, title: act ? act.dataset.title : null, h: document.documentElement.scrollHeight, top: window.scrollY };
};
(async () => {
  await new Promise(r => srv.listening ? r() : srv.once('listening', r)); const port = srv.address().port;
  const browser = await chromium.launch(); const fails = []; const rows = [];
  for (const [W, H] of SIZES) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: mode, deviceScaleFactor: 1 }); const page = await ctx.newPage(); const errs = [];
    page.on('pageerror', e => errs.push(e.message.split('\n')[0]));
    for (const v of VIEWS) {
      const [id, tab] = v.split('#');
      await page.goto(`http://127.0.0.1:${port}/prototype.html?v=${VIEWS.indexOf(v)}#${id}`, { waitUntil: 'load' }); await page.evaluate(() => { try { localStorage.clear(); } catch {} });
      await page.evaluate(m => { document.documentElement.setAttribute('data-mode', m); }, mode);
      if (tab) await page.click(`#${id}-tab-${tab}`);
      await page.waitForTimeout(450); await page.evaluate(() => window.scrollTo(0, 0)); await page.waitForTimeout(100);
      const r = await page.evaluate(CHECK);
      const file = path.join(CAP, `${v.replace('#', '-')}-${W}-${mode}.png`); await page.screenshot({ path: file, fullPage: W === 390 });
      rows.push([v, W, mode, r.hscroll ? `HSCROLL ${r.sw}>${r.cw}` : 'ok', `${r.walls.length} walls`, `${r.wraps.length} wrapped chips`, `${r.overflow.length} overflow`, errs.length ? `${errs.length} errors` : 'no errors', `top ${r.top}`, r.h]);
      if (r.hscroll) fails.push(`${v} ${W} ${mode}: horizontal scroll ${r.sw} > ${r.cw}`);
      if (r.top !== 0) fails.push(`${v} ${W} ${mode}: capture not at the top (${r.top})`);
      for (const w of r.walls) fails.push(`${v} ${W} ${mode}: ${w.lines} lines: ${w.text}`);
      for (const w of r.wraps) fails.push(`${v} ${W} ${mode}: chip wraps: ${w}`);
      for (const w of r.overflow) fails.push(`${v} ${W} ${mode}: text overflows ${w.el}: ${w.text}`);
    }
    if (errs.length) fails.push(`${W} ${mode}: page errors: ${[...new Set(errs)].join(' | ')}`);
    await ctx.close();
  }
  // the three themes and the large text size on the overview and an outcome, light and dark, at 1200 (fit only, no captures)
  for (const theme of ['slate', 'sage']) for (const mode of ['light', 'dark']) for (const size of ['default', 'large']) {
    const ctx = await browser.newContext({ viewport: { width: 1200, height: 800 }, colorScheme: mode }); const page = await ctx.newPage();
    for (const v of ['overview', 'o-EPC-022', 'k-TSK-1122']) {
      await page.goto(`http://127.0.0.1:${port}/prototype.html?t=${theme}${size}${mode}#${v}`, { waitUntil: 'load' });
      await page.evaluate(([m, t, s]) => { const r = document.documentElement; r.setAttribute('data-mode', m); r.setAttribute('data-theme', t); r.setAttribute('data-size', s); }, [mode, theme, size]); await page.waitForTimeout(300);
      const r = await page.evaluate(CHECK); rows.push([`${v} ${theme} ${size}`, 1200, mode, r.hscroll ? 'HSCROLL' : 'ok', `${r.walls.length} walls`, `${r.wraps.length} wrapped chips`, `${r.overflow.length} overflow`, '', '', r.h]);
      if (r.hscroll) fails.push(`${v} ${theme} ${size} ${mode}: horizontal scroll`); for (const w of r.wraps) fails.push(`${v} ${theme} ${size} ${mode}: chip wraps: ${w}`); for (const w of r.overflow) fails.push(`${v} ${theme} ${size} ${mode}: overflow ${w.el}: ${w.text}`);
      if (v === 'overview' && size === 'large' && mode === 'light') await page.screenshot({ path: path.join(CAP, `overview-1200-${theme}-large-light.png`) });
    }
    await ctx.close();
  }
  // journeys at 1440 light: pointer drill-down, then the keyboard
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } }); const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/prototype.html`, { waitUntil: 'load' }); await page.waitForTimeout(300);
  const j = [];
  const active = () => page.evaluate(() => document.querySelector('.view.is-active').id);
  const focused = () => page.evaluate(() => { const a = document.activeElement; return a ? (a.id || a.className || a.tagName) + '|' + (a.textContent || '').trim().slice(0, 40) : null; });
  await page.click('a.outcome[data-view-link="o-EPC-022"]'); await page.waitForTimeout(400); j.push(['outcome card opens the topic view', await active() === 'o-EPC-022']);
  j.push(['the topic view opens with its about card before the strip', await page.evaluate(() => { const v = document.querySelector('.view.is-active'); const a = v.querySelector('.card.about'), s = v.querySelector('.strip'); return a && s && a.compareDocumentPosition(s) & Node.DOCUMENT_POSITION_FOLLOWING; })]);
  j.push(['view change moves focus to the new heading', /^H2\|/.test(await focused())]);
  await page.click('#o-EPC-022-tab-order'); j.push(['Order tab shows its panel', await page.evaluate(() => !document.getElementById('o-EPC-022-order').hidden && document.getElementById('o-EPC-022-progress').hidden)]);
  j.push(['tabs keep one tab stop (roving tabindex)', await page.evaluate(() => [...document.querySelectorAll('#o-EPC-022 [role=tab]')].map(t => t.tabIndex).join() === '-1,0,-1')]);
  await page.keyboard.press('ArrowRight'); j.push(['ArrowRight on a tab selects the next one', await page.evaluate(() => document.getElementById('o-EPC-022-tab-issues').getAttribute('aria-selected') === 'true' && document.activeElement.id === 'o-EPC-022-tab-issues')]);
  await page.click('#o-EPC-022-tab-progress'); await page.click('.vt.themes a[data-view-link]'); await page.waitForTimeout(400); j.push(['theme row opens the theme view', /^t-EPC-022-/.test(await active())]);
  await page.click('.view.is-active .vt.tasks a[data-view-link]'); await page.waitForTimeout(400); j.push(['task row opens the task view', /^k-TSK-/.test(await active())]);
  j.push(['task view: about card first, then a strip of 8 facts', await page.evaluate(() => { const v = document.querySelector('.view.is-active'); const a = v.querySelector('.card.about'), s = v.querySelector('.strip'); return a && s && (a.compareDocumentPosition(s) & Node.DOCUMENT_POSITION_FOLLOWING) && s.querySelectorAll(':scope > div').length === 8; })]);
  j.push(['task about is the whole Outcome section (no cut)', await page.evaluate(() => { const p = document.querySelector('.view.is-active .card.about p'); return p && !/…|\.\.\.$/.test(p.textContent) && p.textContent.trim().length > 100; })]);
  j.push(['breadcrumb has 4 levels, each in words', await page.evaluate(() => { const c = [...document.querySelectorAll('.view.is-active .crumbs a, .view.is-active .crumbs [aria-current]')]; return c.length === 4 && c.every(x => !/^(TSK|EPC)-\d+$/.test(x.textContent.trim())); })]);
  await page.click('.view.is-active .crumbs a[data-view-link^="o-"]'); await page.waitForTimeout(400); j.push(['breadcrumb returns to the outcome', await active() === 'o-EPC-022']);
  await page.click('.nav-area[data-area="act"]'); await page.waitForTimeout(200); j.push(['rail icon selects the area and the panel lists it', await page.evaluate(() => document.querySelector('.menu').dataset.area === 'act')]);
  await page.click('[data-view-link="inputs"]'); await page.waitForTimeout(400); await page.click('#decisions input[name=tree][value=approve]'); await page.click('[data-view-link="script"]'); await page.waitForTimeout(300);
  j.push(['an answer appears in the script', await page.evaluate(() => /tree: approve/.test(document.getElementById('script-text').textContent))]);
  await page.screenshot({ path: path.join(CAP, 'journey-script-1440-light.png') });
  // the gear: focus moves in on open and back on Escape
  await page.focus('[data-gear]'); await page.keyboard.press('Enter'); await page.waitForTimeout(150); j.push(['gear opens the dialog and focus moves into it', await page.evaluate(() => document.querySelector('.pop').classList.contains('is-open') && document.querySelector('.pop').contains(document.activeElement))]);
  await page.keyboard.press('Escape'); await page.waitForTimeout(100); j.push(['Escape closes the dialog and returns focus to the gear', await page.evaluate(() => !document.querySelector('.pop').classList.contains('is-open') && document.activeElement === document.querySelector('[data-gear]'))]);
  // the docs topic: three tabs, each opening with a figure
  await page.click('.nav-area[data-area="kinds"]'); await page.waitForTimeout(150); await page.click('[data-view-link="docs"]'); await page.waitForTimeout(400);
  j.push(['docs topic: Concept, Architecture, Technical tabs', await page.evaluate(() => [...document.querySelectorAll('#docs [role=tab]')].map(t => t.textContent.trim()).join() === 'Concept,Architecture,Technical')]);
  j.push(['each docs tab opens with a figure', await page.evaluate(() => [...document.querySelectorAll('#docs [role=tabpanel]')].every(p => { const first = p.querySelector('figure, .fig'); const lede = p.querySelector('.lede'); return first && (!lede || lede.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING); }))]);
  j.push(['every figure has a title, a description and a data table', await page.evaluate(() => [...document.querySelectorAll('.fig')].every(f => f.querySelector('svg title') && f.querySelector('svg desc') && f.querySelector('.fig-data table')))]);
  j.push(['the nine other report kinds have specimen views', await page.evaluate(() => document.querySelectorAll('.view[id^="s-"]').length === 9)]);
  await ctx.close();
  const ctx2 = await browser.newContext({ viewport: { width: 390, height: 844 } }); const p2 = await ctx2.newPage(); await p2.goto(`http://127.0.0.1:${port}/prototype.html`, { waitUntil: 'load' }); await p2.waitForTimeout(300);
  await p2.click('[data-menu-toggle]'); await p2.waitForTimeout(400); j.push(['390 px: the toggle opens the drawer', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer === 'open')]);
  j.push(['390 px: the page behind the drawer is inert and focus is in the drawer', await p2.evaluate(() => document.querySelector('.main').hasAttribute('inert') && document.querySelector('.menu').contains(document.activeElement))]);
  await p2.screenshot({ path: path.join(CAP, 'drawer-390-light.png') });
  await p2.keyboard.press('Escape'); await p2.waitForTimeout(400); j.push(['390 px: Escape closes it, the page is live again and focus returns to the toggle', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer !== 'open' && !document.querySelector('.main').hasAttribute('inert') && document.activeElement === document.querySelector('[data-menu-toggle]'))]);
  await p2.goto(`http://127.0.0.1:${port}/prototype.html#since`, { waitUntil: 'load' }); await p2.waitForTimeout(300);
  j.push(['390 px: a stacked table row shows its column labels', await p2.evaluate(() => { const c = document.querySelector('#since .vt [role=row]:not(.vt-head) [role=cell][data-l="Was"]'); return c && getComputedStyle(c, '::before').content.includes('Was'); })]);
  await ctx2.close(); await browser.close(); srv.close();
  console.log(rows.map(r => r.join('  ')).join('\n')); console.log(j.map(([k, ok]) => (ok ? 'PASS ' : 'FAIL ') + k).join('\n'));
  for (const [k, ok] of j) if (!ok) fails.push('journey: ' + k);
  if (fails.length) { console.log('\nFAILURES\n' + fails.join('\n')); process.exit(1); } console.log(`\n${rows.length} renders checked, ${rows.filter(r => r[8] !== '').length} captures in ${CAP}, ${j.length} of ${j.length} journeys, 0 failures`);
})();

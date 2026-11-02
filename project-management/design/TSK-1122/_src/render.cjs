#!/usr/bin/env node
// TSK-1122 prototype: captures and checks. Serves the folder, drives every view at 1440 and 390 px in light and dark,
// captures each to ../captures/, and fails on horizontal scroll, a console error, or a paragraph over 3 lines in a card.
// Usage: node render.cjs   (Chromium runs outside the Bash sandbox)
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const path = require('path'); const fs = require('fs'); const http = require('http');
const ROOT = path.resolve(__dirname, '..'), CAP = path.join(ROOT, 'captures'); fs.mkdirSync(CAP, { recursive: true });
const VIEWS = ['overview', 'o-EPC-022', 'o-EPC-022#order', 'o-EPC-022#issues', 't-EPC-022-0', 'k-TSK-1122', 'epics', 'blockers', 'since', 'lanes', 'evidence', 'inputs', 'script', 'kinds', 'portal'];
const srv = http.createServer((q, r) => { const f = path.join(ROOT, decodeURIComponent(q.url.split('?')[0].split('#')[0]) === '/' ? 'prototype.html' : q.url.split('?')[0]); if (!f.startsWith(ROOT) || !fs.existsSync(f)) { r.writeHead(404); return r.end(); } r.writeHead(200, { 'content-type': f.endsWith('.html') ? 'text/html' : 'application/octet-stream' }); r.end(fs.readFileSync(f)); }).listen(0, '127.0.0.1');
(async () => {
  await new Promise(r => srv.listening ? r() : srv.once('listening', r)); const port = srv.address().port;
  const browser = await chromium.launch(); const fails = []; const rows = [];
  for (const [W, H] of [[1440, 900], [390, 844]]) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: mode, deviceScaleFactor: 1 }); const page = await ctx.newPage(); const errs = [];
    page.on('pageerror', e => errs.push(e.message.split('\n')[0]));
    for (const v of VIEWS) {
      const [id, tab] = v.split('#');
      await page.goto(`http://127.0.0.1:${port}/prototype.html?v=${VIEWS.indexOf(v)}#${id}`, { waitUntil: 'load' }); await page.evaluate(() => { try { localStorage.clear(); } catch {} });
      await page.evaluate(m => { document.documentElement.setAttribute('data-mode', m); }, mode);
      if (tab) await page.click(`#${id}-tab-${tab}`);
      await page.waitForTimeout(450);
      const r = await page.evaluate(() => {
        const sw = document.documentElement.scrollWidth, cw = document.documentElement.clientWidth; const walls = [];
        const vis = el => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden'; };
        for (const el of document.querySelectorAll('.view.is-active :is(.card, .note, .strip, .vt, .fig) :is(p, li, dd)')) { if (!vis(el)) continue; const cs = getComputedStyle(el); const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.5; const lines = Math.round(el.getBoundingClientRect().height / lh); if (lines > 3) walls.push({ lines, text: el.textContent.trim().slice(0, 70) }); }
        const act = document.querySelector('.view.is-active'); return { hscroll: sw > cw + 1, sw, cw, walls, title: act ? act.dataset.title : null, h: document.documentElement.scrollHeight };
      });
      const file = path.join(CAP, `${v.replace('#', '-')}-${W}-${mode}.png`); await page.screenshot({ path: file, fullPage: W === 1440 ? false : true });
      rows.push([v, W, mode, r.hscroll ? `HSCROLL ${r.sw}>${r.cw}` : 'ok', r.walls.length ? `${r.walls.length} walls` : '0 walls', errs.length ? `${errs.length} errors` : 'no errors', r.h]);
      if (r.hscroll) fails.push(`${v} ${W} ${mode}: horizontal scroll ${r.sw} > ${r.cw}`);
      for (const w of r.walls) fails.push(`${v} ${W} ${mode}: ${w.lines} lines: ${w.text}`);
    }
    if (errs.length) fails.push(`${W} ${mode}: page errors: ${[...new Set(errs)].join(' | ')}`);
    await ctx.close();
  }
  // drill-down and interaction journey at 1440 light: overview -> outcome -> theme -> task -> breadcrumb back; tabs; drawer at 390
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } }); const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/prototype.html`, { waitUntil: 'load' }); await page.waitForTimeout(300);
  const j = [];
  const active = () => page.evaluate(() => document.querySelector('.view.is-active').id);
  await page.click('a.outcome[data-view-link="o-EPC-022"]'); await page.waitForTimeout(400); j.push(['outcome card opens the topic view', await active() === 'o-EPC-022']);
  await page.click('#o-EPC-022-tab-order'); j.push(['Order tab shows its panel', await page.evaluate(() => !document.getElementById('o-EPC-022-order').hidden && document.getElementById('o-EPC-022-progress').hidden)]);
  await page.click('#o-EPC-022-tab-progress'); await page.click('.vt.themes a[data-view-link]'); await page.waitForTimeout(400); j.push(['theme row opens the theme view', /^t-EPC-022-/.test(await active())]);
  await page.click('.view.is-active .vt.tasks a[data-view-link]'); await page.waitForTimeout(400); j.push(['task row opens the task view', /^k-TSK-/.test(await active())]);
  j.push(['task view has a context strip with 8 facts', await page.evaluate(() => document.querySelectorAll('.view.is-active .strip > div').length === 8)]);
  j.push(['breadcrumb has 4 levels', await page.evaluate(() => document.querySelectorAll('.view.is-active .crumbs a, .view.is-active .crumbs [aria-current]').length === 4)]);
  await page.click('.view.is-active .crumbs a[data-view-link^="o-"]'); await page.waitForTimeout(400); j.push(['breadcrumb returns to the outcome', await active() === 'o-EPC-022']);
  await page.click('.nav-area[data-area="act"]'); await page.waitForTimeout(200); j.push(['rail icon selects the area and the panel lists it', await page.evaluate(() => document.querySelector('.menu').dataset.area === 'act')]);
  await page.click('[data-view-link="inputs"]'); await page.waitForTimeout(400); await page.click('#decisions input[name=tree][value=approve]'); await page.click('[data-view-link="script"]'); await page.waitForTimeout(300);
  j.push(['an answer appears in the script', await page.evaluate(() => /tree: approve/.test(document.getElementById('script-text').textContent))]);
  await page.screenshot({ path: path.join(CAP, 'journey-script-1440-light.png') });
  await ctx.close();
  const ctx2 = await browser.newContext({ viewport: { width: 390, height: 844 } }); const p2 = await ctx2.newPage(); await p2.goto(`http://127.0.0.1:${port}/prototype.html`, { waitUntil: 'load' }); await p2.waitForTimeout(300);
  await p2.click('[data-menu-toggle]'); await p2.waitForTimeout(400); j.push(['390 px: the toggle opens the drawer', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer === 'open')]);
  await p2.screenshot({ path: path.join(CAP, 'drawer-390-light.png') });
  await p2.keyboard.press('Escape'); await p2.waitForTimeout(400); j.push(['390 px: Escape closes it', await p2.evaluate(() => document.querySelector('.shell').dataset.drawer !== 'open')]);
  await ctx2.close(); await browser.close(); srv.close();
  console.log(rows.map(r => r.join('  ')).join('\n')); console.log(j.map(([k, ok]) => (ok ? 'PASS ' : 'FAIL ') + k).join('\n'));
  for (const [k, ok] of j) if (!ok) fails.push('journey: ' + k);
  if (fails.length) { console.log('\nFAILURES\n' + fails.join('\n')); process.exit(1); } console.log(`\n${rows.length} captures in ${CAP}, 0 failures`);
})();

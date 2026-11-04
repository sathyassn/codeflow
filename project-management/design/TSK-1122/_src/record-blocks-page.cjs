#!/usr/bin/env node
// Opens the disclosures that hold a record's lists and tables, which axe.cjs and textlines.cjs leave closed, and checks them.
// Usage: node record-blocks-page.cjs   (exits 1 on any failure)
const path = require('path'); const { pathToFileURL } = require('url');
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const { AxeBuilder } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/@axe-core/playwright');
const URL = pathToFileURL(path.resolve(__dirname, '../prototype.html')).href;
const VIEWS = [['k-TSK-306', { table: true, olStart: true }], ['k-TSK-1122', { list: true }], ['f-CAP-001', { list: true }]];
(async () => {
  const b = await chromium.launch(); let fails = 0, n = 0;
  for (const [w, h] of [[1440, 900], [390, 844]]) for (const mode of ['light', 'dark']) {
    const c = await b.newContext({ viewport: { width: w, height: h }, colorScheme: mode }); const p = await c.newPage(); p.setDefaultTimeout(240000);
    for (const [id, want] of VIEWS) {
      await p.goto(`${URL}?${w}-${mode}-${id}#${id}`, { waitUntil: 'load' }); await p.waitForTimeout(300);
      await p.evaluate(v => { for (const d of document.querySelectorAll(`#${v} details.more`)) d.open = true; }, id); await p.waitForTimeout(100);
      const r = await p.evaluate(v => { const s = document.getElementById(v); const tw = [...s.querySelectorAll('.rec-table')]; return { lists: s.querySelectorAll('.rec-list').length, visibleList: [...s.querySelectorAll('.rec-list li')].some(li => li.getBoundingClientRect().height > 0 && getComputedStyle(li).listStyleType !== 'none'), tables: tw.length, olStart: !!s.querySelector('ol.rec-list[start]'), pageScroll: document.documentElement.scrollWidth - document.documentElement.clientWidth, tablesReachable: tw.every(t => t.scrollWidth <= t.clientWidth || t.tabIndex === 0) }; }, id);
      const ax = await new AxeBuilder({ page: p }).include(`#${id}`).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
      const bad = [];
      if (want.list && (!r.lists || !r.visibleList)) bad.push('no visible list');
      if (want.table && !r.tables) bad.push('no table');
      if (want.olStart && !r.olStart) bad.push('a numbered list lost its start');
      if (r.pageScroll > 0) bad.push(`page scrolls sideways by ${r.pageScroll}`);
      if (!r.tablesReachable) bad.push('a scrolling table cannot take focus');
      for (const v of ax.violations) bad.push(`axe ${v.id} (${v.impact}): ${v.nodes.length} nodes`);
      n++; fails += bad.length; console.log(`${bad.length ? 'FAIL' : 'PASS'} ${id} ${w} ${mode} ${JSON.stringify(r)} ${bad.join('; ')}`);
    }
    // Every view with a disclosure, opened: no page sideways scroll at this width.
    const sweep = await p.evaluate(() => { const out = { views: 0, bad: [] }; const was = document.querySelector('.view.is-active'); const de = document.documentElement; for (const v of document.querySelectorAll('.view[id^="k-"], .view[id^="f-"], .view[id^="o-"], .view[id^="t-"]')) { document.querySelectorAll('.view.is-active').forEach(x => x.classList.remove('is-active')); v.classList.add('is-active'); const ds = [...v.querySelectorAll('details.more')]; ds.forEach(d => { d.open = true; }); out.views++; const over = de.scrollWidth - de.clientWidth; if (over > 0) { const who = [...v.querySelectorAll('*')].find(e => !e.closest('.rec-table') && e.getBoundingClientRect().right > de.clientWidth + 1); out.bad.push(`${v.id} ${over} ${who ? who.tagName + ':' + who.textContent.trim().slice(0, 40) : ''}`); } ds.forEach(d => { d.open = false; }); v.classList.remove('is-active'); } if (was) was.classList.add('is-active'); return out; });
    n++; fails += sweep.bad.length; console.log(`${sweep.bad.length ? 'FAIL' : 'PASS'} sweep ${w} ${mode}: ${sweep.views} views opened, ${sweep.bad.length} scroll sideways ${sweep.bad.slice(0, 6).join(' | ')}`);
    await c.close();
  }
  await b.close(); console.log(`record-blocks-page: ${n} opened views, ${fails} failures`); process.exitCode = fails ? 1 : 0;
})().catch(e => { console.error(e); process.exitCode = 2; });

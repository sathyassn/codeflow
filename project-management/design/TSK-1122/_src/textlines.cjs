#!/usr/bin/env node
// TSK-1122 prototype: the two-line rule. Opens ../prototype.html from file:// at 1440 and 1280 px, makes every view
// active in turn (every tab panel shown), and measures the rendered line count of every visible text block: an
// element with its own text and only inline children, outside <pre>, <svg> and a closed <details>. Any block over two
// lines fails. Prints the count, which must be 0. Usage: node textlines.cjs   (Chromium runs outside the Bash sandbox)
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const path = require('path');
const URL = 'file://' + path.resolve(__dirname, '../prototype.html');
const MEASURE = () => {
  const BLOCK = /^(block|flex|grid|table|table-row|table-cell|list-item|flow-root)$/;
  const out = { blocks: 0, over: [], views: 0 };
  const lines = el => { const r = document.createRange(); r.selectNodeContents(el); const tops = []; for (const b of r.getClientRects()) { if (b.width < 1 || b.height < 1) continue; if (!tops.some(t => Math.abs(t - b.top) < 4)) tops.push(b.top); } return tops.length; };
  const ok = el => { const cs = getComputedStyle(el); if (cs.display === 'none' || cs.visibility === 'hidden') return false; const b = el.getBoundingClientRect(); return b.width > 2 && b.height > 2; };
  const scan = root => { for (const el of root.querySelectorAll('*')) {
    if (el.closest('pre, svg, script, style, textarea, details:not([open]) > :not(summary)')) continue;
    if (el.closest('details:not([open])') && el.tagName !== 'SUMMARY' && !el.closest('summary')) continue;
    const own = [...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim()); if (!own) continue;
    if ([...el.children].some(c => BLOCK.test(getComputedStyle(c).display))) continue;
    if (!ok(el)) continue;
    out.blocks++; const n = lines(el); if (n > 2) out.over.push({ view: root.id || root.className, tag: el.tagName.toLowerCase() + (el.className ? '.' + String(el.className).split(' ')[0] : ''), lines: n, text: el.textContent.trim().replace(/\s+/g, ' ').slice(0, 90) });
  } };
  for (const v of document.querySelectorAll('.view')) {
    document.querySelectorAll('.view.is-active').forEach(x => x.classList.remove('is-active')); v.classList.add('is-active');
    const hiddenPanels = [...v.querySelectorAll('[role=tabpanel][hidden]')]; hiddenPanels.forEach(p => p.hidden = false);
    scan(v); out.views++; hiddenPanels.forEach(p => p.hidden = true);
  }
  scan(document.querySelector('.menu')); scan(document.querySelector('.notice').parentElement.querySelector('.notice')); scan(document.querySelector('.topmeta'));
  return out;
};
(async () => {
  const browser = await chromium.launch(); let total = 0, blocks = 0; const fails = [];
  for (const W of [1440, 1280]) {
    const ctx = await browser.newContext({ viewport: { width: W, height: 900 } }); const page = await ctx.newPage();
    await page.goto(URL, { waitUntil: 'load' }); await page.waitForTimeout(400);
    const r = await page.evaluate(MEASURE); blocks += r.blocks; total += r.over.length;
    console.log(`${W} px: ${r.views} views, ${r.blocks} text blocks measured, ${r.over.length} over two lines`);
    for (const o of r.over) { fails.push(`${W} ${o.view} ${o.tag} ${o.lines} lines: ${o.text}`); }
    await ctx.close();
  }
  await browser.close();
  if (fails.length) console.log(fails.slice(0, 80).join('\n') + (fails.length > 80 ? `\n... and ${fails.length - 80} more` : ''));
  console.log(`\ntext blocks over two lines outside closed details: ${total} (of ${blocks} measured at two widths)`);
  process.exit(total ? 1 : 0);
})();

#!/usr/bin/env node
// TSK-1122 prototype: axe-core (WCAG 2.x A and AA rules) over the static views at 1440, 1280 and 390 px, light and
// dark, in Graphite; then the Slate and Sage themes and the large text size on three views at 1200 px. The page is
// opened from file:// with no server. Any violation of any impact fails the run; the summary prints all four counts.
// Usage: node axe.cjs   (Chromium runs outside the Bash sandbox)
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const { AxeBuilder } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/@axe-core/playwright');
const path = require('path');
const URL = 'file://' + path.resolve(__dirname, '../prototype.html');
const VIEWS = ['overview', 'o-EPC-022', 'o-EPC-025', 'k-TSK-1122', 't-EPC-022-0', 'epics', 'blockers', 'since', 'lanes', 'evidence', 'inputs', 'script', 'kinds', 's-security-review', 'portal', 'docs', 'docs-since'];
(async () => {
  const browser = await chromium.launch(); const tot = { critical: 0, serious: 0, moderate: 0, minor: 0 }; const seen = {}; let runs = 0;
  const check = async (page, label, v, W, mode, theme, size) => {
    await page.goto(`${URL}?${label}#${v}`, { waitUntil: 'load' });
    await page.evaluate(([m, t, s]) => { const r = document.documentElement; r.setAttribute('data-mode', m); if (t) r.setAttribute('data-theme', t); if (s) r.setAttribute('data-size', s); }, [mode, theme, size]); await page.waitForTimeout(350);
    const res = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).exclude('[inert]').analyze(); runs++;
    const by = { critical: 0, serious: 0, moderate: 0, minor: 0 }; for (const x of res.violations) { by[x.impact] = (by[x.impact] || 0) + 1; tot[x.impact] = (tot[x.impact] || 0) + 1; }
    console.log(`${v} ${W} ${mode}${theme ? ' ' + theme : ''}${size ? ' ' + size : ''}: ${by.critical} critical, ${by.serious} serious, ${by.moderate} moderate, ${by.minor} minor, ${res.passes.length} rules passed`);
    for (const x of res.violations) { const k = x.id + ' ' + x.nodes[0].target[0]; if (!seen[k]) { seen[k] = 1; console.log(`  ${x.impact} ${x.id}: ${x.help} (${x.nodes.length} nodes, first ${x.nodes[0].target[0]})`); } }
  };
  for (const [W, H] of [[1440, 900], [1280, 800], [390, 844]]) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: mode }); const page = await ctx.newPage();
    for (const v of VIEWS) await check(page, `${W}${mode}${v}`, v, W, mode, null, null);
    await ctx.close();
  }
  for (const theme of ['slate', 'sage']) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: 1200, height: 800 }, colorScheme: mode }); const page = await ctx.newPage();
    for (const v of ['overview', 'o-EPC-022', 'k-TSK-1122']) await check(page, `${theme}${mode}${v}`, v, 1200, mode, theme, null);
    await check(page, `${theme}${mode}large`, 'overview', 1200, mode, theme, 'large');
    await ctx.close();
  }
  await browser.close();
  const bad = tot.critical + tot.serious + tot.moderate + tot.minor;
  console.log(`\n${runs} view renders: ${tot.critical} critical, ${tot.serious} serious, ${tot.moderate} moderate, ${tot.minor} minor violations; the run fails on any`);
  process.exit(bad ? 1 : 0);
})();

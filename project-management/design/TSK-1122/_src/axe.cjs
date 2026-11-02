#!/usr/bin/env node
// TSK-1122 prototype: axe-core (WCAG 2.x A and AA rules) over every static view at 1440 and 390 px, light and dark.
// Usage: node axe.cjs   (Chromium runs outside the Bash sandbox). Fails on any serious or critical violation.
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const { AxeBuilder } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/@axe-core/playwright');
const path = require('path'); const fs = require('fs'); const http = require('http');
const ROOT = path.resolve(__dirname, '..');
const VIEWS = ['overview', 'o-EPC-022', 'k-TSK-1122', 'epics', 'blockers', 'since', 'lanes', 'evidence', 'inputs', 'script', 'kinds', 'portal'];
const srv = http.createServer((q, r) => { r.writeHead(200, { 'content-type': 'text/html' }); r.end(fs.readFileSync(path.join(ROOT, 'prototype.html'))); }).listen(0, '127.0.0.1');
(async () => {
  await new Promise(r => srv.listening ? r() : srv.once('listening', r)); const port = srv.address().port;
  const browser = await chromium.launch(); let bad = 0; const seen = {};
  for (const [W, H] of [[1440, 900], [390, 844]]) for (const mode of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: mode }); const page = await ctx.newPage();
    for (const v of VIEWS) {
      await page.goto(`http://127.0.0.1:${port}/?${W}${mode}${v}#${v}`, { waitUntil: 'load' }); await page.evaluate(m => document.documentElement.setAttribute('data-mode', m), mode); await page.waitForTimeout(350);
      const res = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).exclude('[inert]').analyze();
      const vio = res.violations.filter(x => x.impact === 'serious' || x.impact === 'critical');
      const minor = res.violations.filter(x => !(x.impact === 'serious' || x.impact === 'critical'));
      console.log(`${v} ${W} ${mode}: ${vio.length} serious or critical, ${minor.length} moderate or minor, ${res.passes.length} rules passed`);
      for (const x of [...vio, ...minor]) { const k = x.id + ' ' + x.nodes[0].target[0]; if (!seen[k]) { seen[k] = 1; console.log(`  ${x.impact} ${x.id}: ${x.help} (${x.nodes.length} nodes, first ${x.nodes[0].target[0]})`); } }
      bad += vio.length;
    }
    await ctx.close();
  }
  await browser.close(); srv.close(); console.log(bad ? `\n${bad} serious or critical violations` : '\n0 serious or critical violations'); process.exit(bad ? 1 : 0);
})();

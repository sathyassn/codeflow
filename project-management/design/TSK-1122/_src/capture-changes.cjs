#!/usr/bin/env node
// Before and after captures of the changes made after the operator's approval.
// Usage: node capture-changes.cjs before <approved-page.html> | node capture-changes.cjs after
// HTML stage marks paint with their background and SVG marks with their fill, so each is read by its own property.
const fs = require('fs'); const path = require('path'); const crypto = require('crypto');
const { pathToFileURL } = require('url');
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const [phase, given] = process.argv.slice(2);
if (!['before', 'after'].includes(phase) || (phase === 'before') !== !!given) throw new Error('Usage: capture-changes.cjs before <page> | after');
const root = path.resolve(__dirname, '..');
const pagePath = given ? path.resolve(given) : path.join(root, 'prototype.html');
const out = name => path.join(root, 'captures', `post-approval-${name}-${phase}.png`);
(async () => {
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', reducedMotion: 'reduce' });
    const page = await context.newPage();
    const url = pathToFileURL(pagePath).href;
    const go = async id => {
      await page.goto(`${url}?${phase}-${id}#${id}`, { waitUntil: 'load' });
      await page.evaluate(() => { const r = document.documentElement; r.dataset.mode = 'light'; r.dataset.theme = 'graphite'; delete r.dataset.accent; return document.fonts.ready; });
      await page.waitForTimeout(300);
    };
    await go('overview');
    const figure = page.locator('#overview .fig').filter({ has: page.locator('.st-line') }).first();
    await figure.screenshot({ path: out('stage-fill'), animations: 'disabled' });
    const colours = await figure.evaluate(el => {
      const paint = n => { const cs = getComputedStyle(n); return n instanceof SVGElement ? { element: 'svg', fill: cs.fill } : { element: 'html', background: cs.backgroundColor }; };
      return { main: paint(el.querySelector('.st-main')), line: paint(el.querySelector('.st-line')), lineMark: paint(document.querySelector('.st-line-f')) };
    });
    await go('k-TSK-1122');
    const task = page.locator('#k-TSK-1122');
    await task.locator('details.more').first().evaluate(d => { d.open = true; });
    await task.locator('details.more').first().screenshot({ path: out('task-context') });
    await go('o-EPC-022');
    await page.locator('#o-EPC-022 .chips').first().screenshot({ path: out('epic-chips') });
    let feature = null;
    if (phase === 'after') {
      feature = await page.locator('#o-EPC-022 .chips a[data-view-link^="f-CAP-"]').first().getAttribute('data-view-link');
      await go(feature);
      await page.screenshot({ path: out('feature-view') });
    }
    fs.writeFileSync(path.join(root, 'captures', `post-approval-${phase}.json`), JSON.stringify({ phase, page: path.basename(pagePath), sha256: crypto.createHash('sha256').update(fs.readFileSync(pagePath)).digest('hex'), viewport: '1440x900', theme: 'graphite', mode: 'light', colours, feature }, null, 2) + '\n');
    console.log(phase, JSON.stringify(colours), feature || '');
    await context.close();
  } finally { await browser.close(); }
})().catch(err => { console.error(err); process.exitCode = 1; });

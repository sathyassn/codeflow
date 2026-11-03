#!/usr/bin/env node
// Capture the same real overview stage figure before and after the floor fix.
// Usage: node capture-stage.cjs before|after (before must precede rebuilding the page).
const fs = require('fs'); const path = require('path'); const crypto = require('crypto');
const { pathToFileURL } = require('url');
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const phase = process.argv[2];
if (!['before', 'after'].includes(phase)) throw new Error('Usage: node capture-stage.cjs before|after');
const root = path.resolve(__dirname, '..');
const pagePath = path.join(root, 'prototype.html');
const stem = path.join(root, 'captures', `stage-bar-${phase}-1440-light`);
if (fs.existsSync(stem + '.png') || fs.existsSync(stem + '.json')) throw new Error('Capture already exists: ' + stem);
(async () => {
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', reducedMotion: 'reduce' });
    const page = await context.newPage();
    const bytes = fs.readFileSync(pagePath);
    await page.goto(pathToFileURL(pagePath).href + `?stage-${phase}#overview`, { waitUntil: 'load' });
    await page.evaluate(() => {
      const r = document.documentElement;
      r.dataset.mode = 'light'; r.dataset.theme = 'graphite'; delete r.dataset.accent;
    });
    await page.evaluate(() => document.fonts.ready);
    const figure = page.locator('#overview .fig').filter({ has: page.locator('.st-line') }).first();
    await figure.waitFor({ state: 'visible' });
    await figure.scrollIntoViewIfNeeded();
    const colours = await figure.evaluate(el => ({
      surface: getComputedStyle(document.documentElement).getPropertyValue('--surface').trim(),
      main: getComputedStyle(el.querySelector('.st-main')).fill,
      line: getComputedStyle(el.querySelector('.st-line')).fill,
      lineF: getComputedStyle(document.querySelector('.st-line-f')).fill,
      title: el.querySelector('svg title')?.textContent || el.textContent.trim().slice(0, 100),
    }));
    await figure.screenshot({ path: stem + '.png', animations: 'disabled' });
    fs.writeFileSync(stem + '.json', JSON.stringify({
      phase, capturedAt: new Date().toISOString(), page: 'prototype.html',
      sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
      viewport: { width: 1440, height: 900 }, theme: 'graphite', mode: 'light', accent: 'main', colours,
    }, null, 2) + '\n');
    console.log(`Captured ${stem}.png; rendered colours and page digest in ${stem}.json`);
    await context.close();
  } finally { await browser.close(); }
})().catch(err => { console.error(err); process.exitCode = 1; });

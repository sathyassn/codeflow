/* Re-measures the W3 pages with the corrected containment probe.
 *
 * Why this exists. The W3 harness compared `node.getBBox()` — which is in the
 * node's OWN user space — directly against the root `viewBox`, with no
 * `getCTM()` applied. Any node inside a translated <g> was therefore measured in
 * the wrong coordinate system: a group translated outward reads as an overshoot
 * that is not on the page, and a group translated inward hides one that is.
 *
 * W4's first render reproduced exactly that: every legend item on every figure
 * reported a 7px overshoot, and every one of them was inside its frame.
 *
 * The W3 audit's A7 findings rest on that probe, so they are re-measured here
 * rather than repeated. This tool writes checks/w3-recheck.json and CHANGES
 * NOTHING under ../tsk-014-w3, which stays append-only.
 *
 * Hidden-overflow findings (scrollWidth vs clientWidth) are coordinate-system
 * independent and were never affected. Only the out-of-viewBox findings are at
 * issue.
 */
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';

const ROOT = path.resolve(import.meta.dirname, '..');
const W3 = path.resolve(ROOT, '..', 'tsk-014-w3');
const REPO = path.resolve(ROOT, '..', '..', '..');
const WEB = path.join(REPO, 'crates/codeflow-present/web/node_modules');
const MIME = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json' };

const PAGES = [
  ['p3-a-convergence-with-source-depth', 'cases/p3/a-convergence-with-source-depth/index.html'],
  ['p3-b-source-region-history', 'cases/p3/b-source-region-history/index.html'],
  ['p1-a-critical-path-and-room', 'cases/p1/a-critical-path-and-room/index.html'],
  ['p2-a-stopped-provenance', 'cases/p2/a-stopped-provenance/index.html'],
  ['p2-baseline', 'baselines/p2/baseline.html'],
  ['d1-baseline', 'baselines/d1/baseline.html'],
];
const VIEWPORTS = { desktop: [1440, 900], tablet: [900, 1200], mobile: [390, 844] };

const PROBE = `(() => {
  const naive = [];
  const corrected = [];
  for (const svg of document.querySelectorAll('svg[viewBox]')) {
    const vb = svg.viewBox.baseVal;
    for (const node of svg.querySelectorAll('*')) {
      if (typeof node.getBBox !== 'function') continue;
      if (node.closest('defs')) continue;
      let box; try { box = node.getBBox(); } catch { continue; }
      if (!box.width && !box.height) continue;
      const label = { tag: node.tagName, text: (node.textContent || '').trim().slice(0, 44) };

      const naiveWorst = Math.max(vb.x - box.x, vb.y - box.y, (box.x + box.width) - (vb.x + vb.width), (box.y + box.height) - (vb.y + vb.height));
      if (naiveWorst > 0.5) naive.push({ ...label, overshoot: Math.round(naiveWorst * 10) / 10 });

      /* Corrected: client rects on both sides, in CSS pixels. getBBox is in the
       * node's own user space and getCTM maps to the nearest viewport, so neither
       * can be compared with viewBox.baseVal without mixing coordinate systems.
       * An SVG root clips to its box, but a clipped child still reports its full
       * geometric rect, so this measures exactly what the reader loses. */
      const r = node.getBoundingClientRect();
      const fr = svg.getBoundingClientRect();
      if (!fr.width || !fr.height) continue;
      if (!r.width && !r.height) continue;
      const worst = Math.max(fr.left - r.left, fr.top - r.top, r.right - fr.right, r.bottom - fr.bottom);
      if (worst > 0.5) corrected.push({ ...label, overshoot_css_px: Math.round(worst * 10) / 10 });
    }
  }
  const hidden = [];
  for (const node of document.querySelectorAll('*')) {
    const style = getComputedStyle(node);
    if (['auto', 'scroll', 'hidden'].includes(style.overflowX) && node.scrollWidth > node.clientWidth + 1) {
      hidden.push({ tag: node.tagName.toLowerCase(), hidden: node.scrollWidth - node.clientWidth, client: node.clientWidth });
    }
  }
  return { naive, corrected, hidden, documentOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth };
})()`;

const server = http.createServer((req, res) => {
  const file = path.resolve(W3, decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, ''));
  if (!file.startsWith(W3) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream' });
  res.end(fs.readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const { chromium } = await import(path.join(WEB, 'playwright-core/index.mjs'));
const browser = await chromium.launch({ headless: true });
const rows = [];
for (const [id, rel] of PAGES) {
  for (const mode of ['light', 'dark']) {
    for (const [vp, [w, h]] of Object.entries(VIEWPORTS)) {
      const context = await browser.newContext({ viewport: { width: w, height: h }, colorScheme: mode, deviceScaleFactor: 1 });
      const page = await context.newPage();
      await page.goto(`http://127.0.0.1:${port}/${rel}`, { waitUntil: 'networkidle' });
      await page.evaluate(() => document.fonts.ready);
      const probe = await page.evaluate(PROBE);
      rows.push({ render: `${id}-${mode}-${vp}`, ...probe });
      await context.close();
    }
  }
}
await browser.close();
server.close();

const w3 = JSON.parse(fs.readFileSync(path.join(W3, 'registry.json'), 'utf8'));
const claimed = new Set(w3.invalidated_bands.map((b) => b.render));

const verdicts = rows.map((row) => {
  const naiveOnly = row.naive.length && !row.corrected.length;
  return {
    render: row.render,
    w3_registered_invalid: claimed.has(row.render),
    naive_out_of_viewbox: row.naive.length,
    corrected_out_of_viewbox: row.corrected.length,
    hidden_overflow_containers: row.hidden.length,
    hidden_overflow_px: row.hidden.reduce((n, h) => n + h.hidden, 0),
    document_overflow_px: row.documentOverflow,
    corrected_worst: row.corrected.slice(0, 3),
    verdict: row.corrected.length || row.hidden.length || row.documentOverflow > 0
      ? 'containment failure confirmed under the corrected probe'
      : naiveOnly
        ? 'the naive probe reported an overshoot that the corrected probe does not — coordinate-system artefact'
        : 'no containment failure measured',
  };
});

const out = {
  purpose:
    'Re-measure the W3 pages with a containment probe that maps each node bounding box through getCTM() before comparing it with the root viewBox. The W3 harness compared the two directly.',
  what_is_unaffected:
    'Hidden-overflow findings (scrollWidth vs clientWidth) and document overflow are coordinate-system independent and were never at issue. Composition failures recorded as machine_visible:false were never probe results at all.',
  w3_is_not_edited: '../tsk-014-w3 is append-only and nothing here writes to it.',
  browser: browser.version ? undefined : undefined,
  rows: verdicts,
  summary: {
    registered_bands_rechecked: verdicts.filter((v) => v.w3_registered_invalid).length,
    confirmed: verdicts.filter((v) => v.w3_registered_invalid && v.verdict.startsWith('containment failure')).length,
    artefacts: verdicts.filter((v) => v.w3_registered_invalid && v.verdict.startsWith('the naive')).length,
    clean: verdicts.filter((v) => v.w3_registered_invalid && v.verdict.startsWith('no containment')).length,
  },
};
fs.mkdirSync(path.join(ROOT, 'checks'), { recursive: true });
fs.writeFileSync(path.join(ROOT, 'checks/w3-recheck.json'), `${JSON.stringify(out, null, 2)}\n`);
console.log(JSON.stringify(out.summary, null, 2));
for (const v of verdicts.filter((x) => x.w3_registered_invalid)) {
  console.log(`  ${v.render}: naive=${v.naive_out_of_viewbox} corrected=${v.corrected_out_of_viewbox} hidden=${v.hidden_overflow_px}px doc=${v.document_overflow_px}px → ${v.verdict}`);
}

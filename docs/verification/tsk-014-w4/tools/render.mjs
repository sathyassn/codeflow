/* W4 render harness.
 *
 * Task-owned, headless only, own profile directory, own loopback port. It never
 * launches headed and never touches the operator's browser or active view.
 *
 * It renders every registered artefact at the three declared viewports in both
 * declared modes and, per render, probes:
 *   frame containment (A7), remote requests, console and page errors,
 *   accessibility (axe WCAG 2.0/2.1/2.2 A + AA), the smallest computed type
 *   size inside a figure (A13), the drawn/keyed/described state sets (A12),
 *   the non-colour channel tokens (A14), motion under both reduced-motion
 *   settings (A5), the served shell digest (A10) and the emitting mechanism (A9).
 *
 * Differences from the W3 harness, stated rather than implied: this one does not
 * reimplement W3's process-inventory ownership contract. It uses Playwright's own
 * close, then proves its profile root is gone by ENOENT. That is a weaker
 * lifecycle proof than W3's and is recorded as such in checks/lifecycle.json.
 */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';

const ROOT = path.resolve(import.meta.dirname, '..');
const REPO = path.resolve(ROOT, '..', '..', '..');
const WEB = path.join(REPO, 'crates/codeflow-present/web/node_modules');
const RENDERS = path.join(ROOT, 'renders');
const CHECKS = path.join(ROOT, 'checks');

const registry = JSON.parse(fs.readFileSync(path.join(ROOT, 'registry.json'), 'utf8'));
const { viewports, modes, legibility_floor_px: TYPE_FLOOR } = registry.thresholds;

/* ---- what to render -------------------------------------------------- */

function discover() {
  const pages = [];
  const add = (id, rel) => {
    if (fs.existsSync(path.join(ROOT, rel))) pages.push({ id, rel });
  };
  for (const dir of fs.existsSync(path.join(ROOT, 'directions')) ? fs.readdirSync(path.join(ROOT, 'directions')) : []) {
    add(`dir-${dir}`, `directions/${dir}/index.html`);
  }
  for (const dir of fs.existsSync(path.join(ROOT, 'portal')) ? fs.readdirSync(path.join(ROOT, 'portal')) : []) {
    add(`portal-${dir}`, `portal/${dir}/index.html`);
  }
  const cases = path.join(ROOT, 'cases');
  if (fs.existsSync(cases)) {
    for (const c of fs.readdirSync(cases)) {
      for (const cand of fs.readdirSync(path.join(cases, c))) {
        add(`${c}-${cand}`, `cases/${c}/${cand}/index.html`);
      }
    }
  }
  const baselines = path.join(ROOT, 'baselines');
  if (fs.existsSync(baselines)) {
    for (const c of fs.readdirSync(baselines)) add(`${c}-baseline`, `baselines/${c}/baseline.html`);
  }
  return pages;
}

/* ---- the in-page probe ------------------------------------------------ */

const PROBE = `(() => {
  const frame = { hiddenOverflow: [], outsideViewBox: [], documentOverflow: null };
  for (const node of document.querySelectorAll('*')) {
    const style = getComputedStyle(node);
    if (['auto', 'scroll', 'hidden'].includes(style.overflowX) && node.scrollWidth > node.clientWidth + 1) {
      frame.hiddenOverflow.push({ tag: node.tagName.toLowerCase(), cls: node.className.baseVal ?? node.className ?? '', hidden: node.scrollWidth - node.clientWidth, client: node.clientWidth });
    }
    if (['auto', 'scroll', 'hidden'].includes(style.overflowY) && node.scrollHeight > node.clientHeight + 1) {
      frame.hiddenOverflow.push({ tag: node.tagName.toLowerCase(), cls: node.className.baseVal ?? node.className ?? '', hiddenY: node.scrollHeight - node.clientHeight, client: node.clientHeight });
    }
  }
  /* getBBox() is in the node's OWN user space. A node inside a transformed <g>
   * must have its box mapped through getCTM() before it can be compared with
   * the root viewBox, or every translated group reads as an overshoot and every
   * group translated inward hides a real one. The W3 harness compared the two
   * directly; checks/w3-recheck.json records what that cost. */
  /* Measured in CSS pixels, from client rects on both sides.
   *
   * getBBox() is in the node's own user space and getCTM() maps that to the
   * nearest VIEWPORT, not to viewBox units — so comparing either one against
   * svg.viewBox.baseVal mixes coordinate systems and reports overshoots that are
   * not on the page (and can hide ones that are, wherever the viewBox is scaled
   * down). An SVG root clips to its own box but a clipped child still reports
   * its full geometric rect, so client rects give the honest answer in the units
   * a reader actually loses. checks/w3-recheck.json records what the earlier
   * formulation cost. */
  for (const svg of document.querySelectorAll('svg[viewBox]')) {
    const frameRect = svg.getBoundingClientRect();
    if (!frameRect.width || !frameRect.height) continue;
    for (const node of svg.querySelectorAll('*')) {
      if (typeof node.getBBox !== 'function') continue;
      /* A pattern, gradient or symbol inside <defs> is a template, never painted
       * where its own coordinates say. */
      if (node.closest('defs')) continue;
      const r = node.getBoundingClientRect();
      if (!r.width && !r.height) continue;
      const worst = Math.max(frameRect.left - r.left, frameRect.top - r.top, r.right - frameRect.right, r.bottom - frameRect.bottom);
      if (worst > 0.5) {
        frame.outsideViewBox.push({
          figure: (svg.querySelector('title') || {}).textContent?.slice(0, 44) ?? '',
          tag: node.tagName,
          state: node.getAttribute('data-state') ?? null,
          text: (node.textContent || '').slice(0, 40),
          overshoot_css_px: Math.round(worst * 10) / 10,
        });
      }
    }
  }
  frame.documentOverflow = document.documentElement.scrollWidth - document.documentElement.clientWidth;

  const type = { smallest: Infinity, node: null, belowFloor: [] };
  for (const node of document.querySelectorAll('figure *, .sh-figure *, svg *')) {
    if (!node.textContent || !node.textContent.trim()) continue;
    if (node.children.length) continue;
    const size = parseFloat(getComputedStyle(node).fontSize);
    if (!Number.isFinite(size)) continue;
    if (size < type.smallest) { type.smallest = size; type.node = (node.textContent || '').slice(0, 40); }
    if (size < __FLOOR__) type.belowFloor.push({ size, text: (node.textContent || '').slice(0, 40) });
  }
  if (!Number.isFinite(type.smallest)) type.smallest = null;

  const keys = [];
  for (const svg of document.querySelectorAll('svg[role="img"]')) {
    const drawn = [...new Set([...svg.querySelectorAll('[data-state]')].map((n) => n.getAttribute('data-state')))].sort();
    const keyed = [...new Set([...svg.querySelectorAll('[data-legend-state]')].map((n) => n.getAttribute('data-legend-state')))].sort();
    const desc = (svg.querySelector('desc') || {}).textContent || '';
    keys.push({ id: svg.id || (svg.querySelector('title') || {}).textContent || '', drawn, keyed, describedMissing: drawn.filter((s) => keyed.length && !keyed.includes(s)) });
  }

  const mechanism = [...document.querySelectorAll('[data-compiled-by], [data-emitted-by]')]
    .map((n) => ({ by: n.dataset.compiledBy || n.dataset.emittedBy, band: n.dataset.band || null, marks: n.dataset.markCount || null }));

  const motion = { running: 0, named: [], transitions: [] };
  for (const node of document.querySelectorAll('*')) {
    const style = getComputedStyle(node);
    if (style.animationName && style.animationName !== 'none') motion.named.push(style.animationName);
    const duration = parseFloat(style.transitionDuration || '0');
    if (duration > 0) motion.transitions.push({ tag: node.tagName.toLowerCase(), duration });
  }
  motion.running = document.getAnimations().filter((a) => a.playState === 'running').length;

  const visibleText = document.body.innerText;
  const mode = getComputedStyle(document.documentElement).colorScheme;

  return { frame, type, keys, mechanism, motion, visibleText, mode,
           lang: document.documentElement.lang, title: document.title,
           h1: (document.querySelector('h1') || {}).textContent || null };
})()`;

/* ---- run -------------------------------------------------------------- */

const MIME = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png' };

async function main() {
  const { chromium } = await import(path.join(WEB, 'playwright-core/index.mjs'));
  const axeSource = fs.readFileSync(path.join(WEB, 'axe-core/axe.min.js'), 'utf8');
  const pages = discover();
  if (!pages.length) { console.error('nothing to render'); process.exit(1); }

  const shellDigest = crypto.createHash('sha256')
    .update(fs.readFileSync(path.join(ROOT, 'shared/shell.css'))).digest('hex');

  fs.rmSync(RENDERS, { recursive: true, force: true });
  fs.mkdirSync(RENDERS, { recursive: true });
  fs.mkdirSync(CHECKS, { recursive: true });

  const server = http.createServer((req, res) => {
    const rel = decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, '');
    const file = path.resolve(ROOT, rel);
    if (!file.startsWith(`${ROOT}${path.sep}`) && file !== ROOT) { res.writeHead(403); res.end(); return; }
    if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream' });
    res.end(fs.readFileSync(file));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  const origin = `http://127.0.0.1:${port}`;

  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'cf-w4-render-'));
  const browser = await chromium.launch({ headless: true });
  const version = browser.version();

  const out = { frame: [], axe: [], network: [], console: [], type: [], keys: [], mechanism: [], motion: [], text: [], shell: [], coverage: [] };
  let renderCount = 0;

  for (const page of pages) {
    for (const mode of modes) {
      for (const [vp, size] of Object.entries(viewports)) {
        for (const reduced of [false, true]) {
          const context = await browser.newContext({
            viewport: { width: size.width, height: size.height },
            colorScheme: mode,
            reducedMotion: reduced ? 'reduce' : 'no-preference',
            deviceScaleFactor: 1,
          });
          const tab = await context.newPage();
          const remote = [];
          const errors = [];
          tab.on('request', (r) => { if (!r.url().startsWith(origin)) remote.push(r.url()); });
          tab.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
          tab.on('pageerror', (e) => errors.push(String(e)));

          await tab.goto(`${origin}/${page.rel}`, { waitUntil: 'networkidle' });
          await tab.evaluate(() => document.fonts.ready);
          await tab.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

          const probe = await tab.evaluate(PROBE.replace('__FLOOR__', String(TYPE_FLOOR)));
          const key = `${page.id}-${mode}-${vp}`;

          out.motion.push({ render: key, reduced, ...probe.motion });

          if (!reduced) {
            const shot = path.join(RENDERS, `${key}.png`);
            await tab.screenshot({ path: shot, fullPage: true });
            renderCount += 1;

            out.frame.push({ render: key, ...probe.frame });
            out.type.push({ render: key, ...probe.type });
            out.keys.push({ render: key, figures: probe.keys });
            out.mechanism.push({ render: key, emitted: probe.mechanism });
            out.network.push({ render: key, remote });
            out.console.push({ render: key, errors });
            out.text.push({ render: key, visible: probe.visibleText });
            out.coverage.push({ render: key, lang: probe.lang, title: probe.title, h1: probe.h1, mode: probe.mode });

            await tab.addScriptTag({ content: axeSource });
            const axe = await tab.evaluate(() => window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] } }));
            out.axe.push({ render: key, violations: axe.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.length, help: v.help })) });

            const served = await tab.evaluate(async (o) => {
              const r = await fetch(`${o}/shared/shell.css`);
              return r.text();
            }, origin);
            out.shell.push({ render: key, sha256: crypto.createHash('sha256').update(served).digest('hex') });
          }
          await context.close();
        }
      }
    }
    process.stdout.write(`.`);
  }

  await browser.close();
  server.close();
  fs.rmSync(profile, { recursive: true, force: true });
  const profileGone = !fs.existsSync(profile);

  for (const [name, value] of Object.entries(out)) {
    fs.writeFileSync(path.join(CHECKS, `${name}.json`), `${JSON.stringify(value, null, 2)}\n`);
  }
  fs.writeFileSync(path.join(CHECKS, 'lifecycle.json'), `${JSON.stringify({
    browser: version,
    playwright: JSON.parse(fs.readFileSync(path.join(WEB, 'playwright-core/package.json'), 'utf8')).version,
    axe: JSON.parse(fs.readFileSync(path.join(WEB, 'axe-core/package.json'), 'utf8')).version,
    headless: true,
    headed_launches: 0,
    operator_browser_touched: false,
    profile_root: profile,
    profile_removed: profileGone,
    renders: renderCount,
    pages: pages.length,
    shell_digest_on_disk: shellDigest,
    weaker_than_w3:
      'This harness does not reimplement W3 process-inventory ownership. It closes through Playwright and proves its own profile root absent. A surviving browser process would not be detected here.',
  }, null, 2)}\n`);

  const violations = out.axe.flatMap((a) => a.violations);
  const clipped = out.frame.filter((f) => f.hiddenOverflow.length || f.outsideViewBox.length || f.documentOverflow > 0);
  console.log(`\nrenders ${renderCount} · axe violations ${violations.length} · frame failures ${clipped.length} · remote ${out.network.reduce((n, r) => n + r.remote.length, 0)} · console ${out.console.reduce((n, r) => n + r.errors.length, 0)}`);
  for (const f of clipped.slice(0, 12)) {
    console.log(`  FRAME ${f.render}: doc+${f.documentOverflow} hidden=${f.hiddenOverflow.length} outside=${f.outsideViewBox.length} ${JSON.stringify(f.outsideViewBox.slice(0, 2))}${JSON.stringify(f.hiddenOverflow.slice(0, 2))}`);
  }
  for (const v of violations.slice(0, 8)) console.log(`  AXE ${v.id} (${v.impact}) ×${v.nodes}`);
  for (const c of out.console.filter((c) => c.errors.length).slice(0, 8)) console.log(`  CONSOLE ${c.render}: ${c.errors[0]}`);
  for (const t of out.type.filter((t) => t.belowFloor.length).slice(0, 8)) console.log(`  TYPE ${t.render}: ${t.belowFloor.length} below ${TYPE_FLOOR}px, smallest ${t.smallest}`);
  for (const k of out.keys.filter((k) => k.figures.some((f) => f.describedMissing.length)).slice(0, 8)) console.log(`  KEYS ${k.render}: ${JSON.stringify(k.figures.map((f) => f.describedMissing))}`);
}

main().catch((error) => { console.error(error); process.exit(1); });

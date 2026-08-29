/* W5 render harness (carried from W4, with the W5 probes added).
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

  /* ---- collisions (new in W5) ------------------------------------------
   * Every blocking Part D defect the blind observer returned was a collision or
   * an overprint that every W4 gate passed: a row label printed on a glyph
   * strip, rotated headers colliding into a block, a key landing on the content
   * under it. None of them is visible to an overflow check, because nothing
   * overflowed — two things were simply drawn in the same place.
   *
   * So: every pair of rendered text runs inside a figure is compared by client
   * rect. Overlap area is reported in CSS px, with a small tolerance for the
   * antialiasing slop of adjacent baselines. This is the one gate that would
   * have caught D06 before an observer had to. */
  const collisions = [];
  /* An axis-aligned rect is the wrong instrument for rotated text: two lane
   * heads rotated 52 degrees have bounding boxes that overlap heavily while
   * their ink never touches, so an AABB test reports a collision on every
   * rotated header strip and then has to be muted with a tolerance — which is
   * how a real collision hides behind a false one. So each run is reduced to
   * its ORIENTED box: getBBox() in the node's own user space, four corners
   * pushed through getScreenCTM(), then a convex clip for the true overlap
   * area. Rotation is handled exactly instead of being forgiven. */
  const corners = (node) => {
    const b = node.getBBox();
    const m = node.getScreenCTM();
    if (!m || !b.width || !b.height) return null;
    return [[b.x, b.y], [b.x + b.width, b.y], [b.x + b.width, b.y + b.height], [b.x, b.y + b.height]]
      .map(([x, y]) => [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f]);
  };
  /* Sutherland-Hodgman: clip one convex polygon by the other's edges. */
  const clipArea = (subject, clip) => {
    let out = subject;
    for (let i = 0; i < clip.length; i += 1) {
      const a = clip[i]; const b = clip[(i + 1) % clip.length];
      const side = (p) => (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
      const input = out;
      out = [];
      for (let k = 0; k < input.length; k += 1) {
        const cur = input[k]; const prv = input[(k + input.length - 1) % input.length];
        const dc = side(cur); const dp = side(prv);
        if (dc <= 0) {
          if (dp > 0) {
            const t = dp / (dp - dc);
            out.push([prv[0] + t * (cur[0] - prv[0]), prv[1] + t * (cur[1] - prv[1])]);
          }
          out.push(cur);
        } else if (dp <= 0) {
          const t = dp / (dp - dc);
          out.push([prv[0] + t * (cur[0] - prv[0]), prv[1] + t * (cur[1] - prv[1])]);
        }
      }
      if (!out.length) return { area: 0, depth: 0 };
    }
    let area = 0;
    for (let i = 0; i < out.length; i += 1) {
      const a = out[i]; const b = out[(i + 1) % out.length];
      area += a[0] * b[1] - b[0] * a[1];
    }
    /* Area alone is the wrong verdict. Two stacked lines whose em boxes touch by
     * a fifth of a pixel across 200px of width produce 40px2 of "overlap" and
     * nothing a reader could see, while a short label printed squarely on top of
     * a glyph strip produces less area and ruins the figure. What matters is how
     * DEEP the overlap runs, so the intersection's minor axis is reported beside
     * its area and the gate is decided on depth. */
    const xs = out.map((q) => q[0]); const ys = out.map((q) => q[1]);
    const depth = Math.min(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
    return { area: Math.abs(area) / 2, depth };
  };
  /* winding has to be consistent for the half-plane test above */
  const cw = (poly) => {
    let sum = 0;
    for (let i = 0; i < poly.length; i += 1) {
      const a = poly[i]; const b = poly[(i + 1) % poly.length];
      sum += (b[0] - a[0]) * (b[1] + a[1]);
    }
    return sum > 0 ? poly : [...poly].reverse();
  };
  for (const svg of document.querySelectorAll('svg')) {
    const runs = [];
    for (const t of svg.querySelectorAll('text')) {
      if (!(t.textContent || '').trim()) continue;
      const c = corners(t);
      if (c) runs.push({ poly: cw(c), text: (t.textContent || '').slice(0, 40) });
    }
    for (let i = 0; i < runs.length; i += 1) {
      for (let j = i + 1; j < runs.length; j += 1) {
        /* the em box is taller than the ink; a couple of px of vertical kiss
         * between stacked lines is not an overprint */
        const hit = clipArea(runs[i].poly, runs[j].poly);
        if (hit.area > 12 && hit.depth > 1.5) {
          collisions.push({
            figure: (svg.querySelector('title') || {}).textContent?.slice(0, 40) ?? '',
            a: runs[i].text, b: runs[j].text,
            area_css_px: Math.round(hit.area), depth_css_px: Math.round(hit.depth * 10) / 10,
          });
        }
      }
    }
  }

  /* ---- channels and the mark floor (new in W5) --------------------------
   * A14 asked whether a channel that is not colour exists. A15 asks whether the
   * mark carrying it is big enough to read. Every element that carries a
   * data-state is measured on its smallest information-bearing dimension. */
  const marks = [];
  for (const svg of document.querySelectorAll('svg')) {
    for (const owner of svg.querySelectorAll('[data-state]')) {
      /* a state may now name a GROUP that holds several marks; measure the marks,
       * not the box around them, or wrapping a mark would silently soften the floor */
      const nodes = owner.tagName.toLowerCase() === 'g'
        ? [...owner.querySelectorAll('rect, circle, ellipse, path, line, polygon, polyline')]
        : [owner];
      for (const node of nodes) {
      const r = node.getBoundingClientRect();
      if (!r.width && !r.height) continue;
      /* Both dimensions, because which one carries the information depends on
       * what kind of mark it is. A horizontal rule has zero height and is not
       * therefore a zero-sized mark: its length is the datum. The verifier picks
       * the right dimension per tag; the probe does not guess for it. */
      marks.push({
        figure: (svg.querySelector('title') || {}).textContent?.slice(0, 40) ?? '',
        state: owner.getAttribute('data-state'),
        tag: node.tagName.toLowerCase(),
        w: Math.round(r.width * 10) / 10,
        h: Math.round(r.height * 10) / 10,
        stroke_css_px: Math.round((parseFloat(getComputedStyle(node).strokeWidth) || 0) * 10) / 10,
      });
      }
    }
  }

  /* ---- channels (A15, rendered) -----------------------------------------
   * A15 requires checks/channels.json to record, for every distinction the
   * registry marks matters:true, that each declared channel is PRESENT IN THE
   * RENDERED MARKUP. The first W5 verifier did not do this: it read
   * registry.channels and asserted that the strings channel_a and channel_b
   * were non-empty, which is a declaration checking itself and could not fail
   * while the strings existed.
   *
   * So every channel token below is derived from what the browser actually
   * painted, and from properties that carry no hue:
   *
   *   interior  the KIND of paint in the shape's fill — empty, solid, or a
   *             pattern classified by the structure of its own children and by
   *             the angle it is laid at. Any colour maps to 'solid', so the
   *             token cannot change when the palette does.
   *   edge      the computed stroke-dasharray, normalised to a ratio, plus
   *             whether there is a stroke at all.
   *   form      the primary shape's tag and its user-space geometry, including
   *             whether a path curves or runs straight.
   *   overlay   the additional marks drawn inside the same state element,
   *             classified by their own geometry.
   *
   * No token reads a colour value, and none reads a data-* attribute. The only
   * thing data-state supplies is WHICH state a mark claims to be — the label
   * being tested, not the evidence.
   *
   * Hue-independence is then proved empirically rather than asserted: the
   * verifier requires every token to be identical between the light and dark
   * render of the same viewport. Light and dark change every colour on the
   * page, so a token that survives both cannot be carrying hue. */
  const patternKind = (url) => {
    const hash = url.indexOf('#');
    if (hash < 0) return 'pattern:unknown';
    const id = url.slice(hash + 1).split(')')[0].split('"').join('').split("'").join('').trim();
    const node = document.getElementById(id);
    if (!node) return 'pattern:unknown';
    const kids = [...node.children].map((k) => k.tagName.toLowerCase());
    const shape = kids.includes('circle') ? 'dots' : kids.includes('line') ? 'lines' : kids.join('+') || 'empty';
    const pt = node.getAttribute('patternTransform') || '';
    const at = pt.indexOf('rotate(');
    const rot = at < 0 ? 0 : Math.round(parseFloat(pt.slice(at + 7)) || 0);
    return 'pattern:' + shape + '@' + rot;
  };
  const interiorOf = (node) => {
    const fill = getComputedStyle(node).fill;
    if (!fill || fill === 'none') return 'empty';
    if (fill.startsWith('url(')) return patternKind(fill);
    return 'solid';               /* any colour, deliberately: hue is not a channel */
  };
  const edgeOf = (node) => {
    const cs = getComputedStyle(node);
    if (!cs.stroke || cs.stroke === 'none') return 'no-edge';
    const raw = (cs.strokeDasharray || '').trim();
    if (!raw || raw === 'none') return 'solid';
    const nums = raw.split(',').join(' ').split(' ').filter(Boolean)
      .map((v) => parseFloat(v)).filter((v) => !Number.isNaN(v));
    if (!nums.length) return 'solid';
    /* normalise to the ratio, so the same pattern at a different scale is the
     * same token and a genuinely different pattern is a different one */
    const unit = Math.min(...nums);
    return 'dash:' + nums.map((v) => Math.round((v / unit) * 2) / 2).join('-');
  };
  const formOf = (node) => {
    const tag = node.tagName.toLowerCase();
    let shape = tag;
    if (tag === 'path') {
      const d = node.getAttribute('d') || '';
      shape = 'CcSsQqTtAa'.split('').some((ch) => d.includes(ch)) ? 'path:curved' : 'path:straight';
    }
    let box = null;
    try { box = node.getBBox(); } catch { box = null; }
    const w = box ? Math.round(box.width * 2) / 2 : 0;
    const h = box ? Math.round(box.height * 2) / 2 : 0;
    return shape + ':' + w + 'x' + h;
  };
  const overlayOf = (node) => {
    const tag = node.tagName.toLowerCase();
    if (tag === 'line') {
      const y1 = parseFloat(node.getAttribute('y1')); const y2 = parseFloat(node.getAttribute('y2'));
      const x1 = parseFloat(node.getAttribute('x1')); const x2 = parseFloat(node.getAttribute('x2'));
      if (Math.abs(y1 - y2) < 0.6) return 'strike:horizontal';
      if (Math.abs(x1 - x2) < 0.6) return 'strike:vertical';
      return 'strike:diagonal';
    }
    return formOf(node);
  };
  const DRAWN = ['rect', 'circle', 'ellipse', 'path', 'line', 'polygon', 'polyline'];
  const channels = [];
  for (const svg of document.querySelectorAll('svg')) {
    for (const group of svg.querySelectorAll('[data-distinction][data-state]')) {
      /* a band that is not displayed is not evidence: its marks have no box and
       * are not on the page a reader sees */
      const gb = group.getBoundingClientRect();
      if (!gb.width && !gb.height) continue;
      const drawn = [...group.querySelectorAll('*')].filter((n) => DRAWN.includes(n.tagName.toLowerCase()));
      if (!drawn.length) {
        channels.push({
          figure: (svg.querySelector('title') || {}).textContent?.slice(0, 40) ?? '',
          distinction: group.getAttribute('data-distinction'),
          state: group.getAttribute('data-state'),
          measurable: false, reason: 'the element naming this state draws nothing',
        });
        continue;
      }
      /* the primary shape is the largest drawn descendant; the rest are overlay */
      const area = (n) => { try { const b = n.getBBox(); return b.width * b.height; } catch { return 0; } };
      const sorted = [...drawn].sort((a, b) => area(b) - area(a));
      const primary = sorted[0];
      const extras = sorted.slice(1);
      /* the shape vocabulary the whole mark is built from, which is what the
       * registry means by a geometry channel: an arc against a straight entry
       * is a difference in WHICH shapes are present, not in one shape's size */
      const geometry = [...new Set(drawn.map((n) => {
        const tag = n.tagName.toLowerCase();
        if (tag !== 'path') return tag;
        const d = n.getAttribute('d') || '';
        return 'CcSsQqTtAa'.split('').some((ch) => d.includes(ch)) ? 'curved' : 'straight';
      }))].sort().join('+');
      /* Marks that name a structural role are measured as their own channel, so
       * a claim about "the terminal" is tested against the terminal and not
       * against whatever else happens to sit in the same group. The role says
       * which mark; the token is still measured off the render. */
      const roles = {};
      for (const named of group.querySelectorAll('[data-channel]')) {
        const role = named.getAttribute('data-channel');
        const tag = named.tagName.toLowerCase();
        let shape = tag;
        if (tag === 'path') {
          const d = named.getAttribute('d') || '';
          shape = 'CcSsQqTtAa'.split('').some((ch) => d.includes(ch)) ? 'path:curved' : 'path:straight';
        }
        roles[role] = roles[role] ? roles[role] + '+' + shape : shape;
      }
      channels.push({
        figure: (svg.querySelector('title') || {}).textContent?.slice(0, 40) ?? '',
        distinction: group.getAttribute('data-distinction'),
        state: group.getAttribute('data-state'),
        measurable: true,
        interior: interiorOf(primary),
        edge: edgeOf(primary),
        form: formOf(primary),
        geometry,
        terminal: roles.terminal ?? 'absent',
        entry: roles.entry ?? 'absent',
        overlay: extras.length ? extras.map(overlayOf).sort().join('+') : 'none',
        marks: drawn.length,
      });
    }
  }

  /* ---- fidelity (new in W5) ---------------------------------------------
   * A18. The one defect no composition gate could reach was a figure that drew
   * the repository's own traceability spine in the wrong order. So the drawing
   * publishes what it drew, and the verifier re-derives the truth from the
   * shared source and compares the two. */
  const fidelity = {
    spine: [...document.querySelectorAll('[data-spine-node]')].map((el) => el.getAttribute('data-spine-node')),
    authorities: [...new Set([...document.querySelectorAll('[data-authority]')].map((el) => el.getAttribute('data-authority')))].sort(),
    footLanes: [...document.querySelectorAll('[data-foot-lane]')].map((el) => el.getAttribute('data-foot-lane')),
    claims: [...new Set([...document.querySelectorAll('[data-claim]')].map((el) => el.getAttribute('data-claim')))].sort(),
  };

  const visibleText = document.body.innerText;
  const mode = getComputedStyle(document.documentElement).colorScheme;

  return { frame, type, keys, mechanism, motion, visibleText, mode, collisions, marks, fidelity, channels,
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

  const out = { frame: [], axe: [], network: [], console: [], type: [], keys: [], mechanism: [], motion: [], text: [], shell: [], coverage: [], collisions: [], marks: [], geometry: [], fidelity: [], channels: [] };
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
            out.coverage.push({ render: key, page: page.id, mode, viewport: vp, lang: probe.lang, title: probe.title, h1: probe.h1, scheme: probe.mode });
            out.collisions.push({ render: key, hits: probe.collisions });
            out.marks.push({ render: key, marks: probe.marks });
            out.fidelity.push({ render: key, page: page.id, mode, viewport: vp, ...probe.fidelity });
            out.channels.push({ render: key, page: page.id, mode, viewport: vp, states: probe.channels });
            const box = await tab.evaluate(() => ({ h: document.documentElement.scrollHeight, w: document.documentElement.scrollWidth }));
            out.geometry.push({ render: key, page: page.id, mode, viewport: vp, height: box.h, width: box.w });

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
  const hits = out.collisions.filter((c) => c.hits.length);
  console.log(`  collisions ${hits.reduce((n, c) => n + c.hits.length, 0)} across ${hits.length} renders`);
  for (const c of hits.slice(0, 10)) console.log(`  COLLIDE ${c.render}: "${c.hits[0].a}" x "${c.hits[0].b}" depth ${c.hits[0].depth_css_px}px (${c.hits.length} in this render)`);
  for (const k of out.keys.filter((k) => k.figures.some((f) => f.describedMissing.length)).slice(0, 8)) console.log(`  KEYS ${k.render}: ${JSON.stringify(k.figures.map((f) => f.describedMissing))}`);
}

main().catch((error) => { console.error(error); process.exit(1); });

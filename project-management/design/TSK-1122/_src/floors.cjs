#!/usr/bin/env node
// TSK-1122 prototype floors: stage fills come from Chromium's computed styles on prototype.html.
// Other token pairs remain source-derived. Floors: 3:1 for stage fills and marks, 4.5:1 for text.
// Line/main separation is mandatory: Euclidean OKLab distance ×100 >= 15, including .st-line-f.
// Usage: node floors.cjs   (prints a table; exits 1 under any floor)
const fs = require('fs'); const path = require('path');
const { STAGE_LINE_PERCENT, STAGE_LINE_FILL, STAGE_SEPARATION_FLOOR } = require('./stage-colours.cjs');
const { pathToFileURL } = require('url');
const crypto = require('crypto');
const { chromium } = require('/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/docs-portal/node_modules/playwright');
const css = fs.readFileSync(path.join(__dirname, 'kit', 'cf.css'), 'utf8');
const rules = []; for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) { const d = {}; for (const x of m[2].matchAll(/--([a-z0-9-]+):\s*([^;]+);/g)) d[x[1]] = x[2].trim(); if (Object.keys(d).length) rules.push({ sel: m[1].replace(/\s+/g, ' ').trim(), d }); }
const pick = sels => { const out = {}; for (const r of rules) if (sels.some(s => r.sel.split(',').map(x => x.trim()).includes(s))) Object.assign(out, r.d); return out; };
// the selector lists that define each theme and mode (the base light tokens apply under every theme)
const BASE_L = [':root', ':root[data-theme="graphite"]'], BASE_D = [':root[data-mode="dark"]'];
const THEMES = {
  graphite: { light: pick(BASE_L), dark: { ...pick(BASE_L), ...pick(BASE_D) } },
  slate: { light: { ...pick(BASE_L), ...pick([':root[data-theme="slate"]']) }, dark: { ...pick(BASE_L), ...pick(BASE_D), ...pick([':root[data-mode="dark"][data-theme="slate"]']) } },
  sage: { light: { ...pick(BASE_L), ...pick([':root[data-theme="sage"]']) }, dark: { ...pick(BASE_L), ...pick(BASE_D), ...pick([':root[data-mode="dark"][data-theme="sage"]']) } },
};
const ACCENT2 = { graphite: 'plum', slate: 'violet', sage: 'plum' };
const hex = h => { h = h.replace('#', ''); if (h.length === 3) h = h.split('').map(c => c + c).join(''); return [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16)); };
const lum = ([r, g, b]) => { const f = c => { c /= 255; return c <= .03928 ? c / 12.92 : Math.pow((c + .055) / 1.055, 2.4); }; return .2126 * f(r) + .7152 * f(g) + .0722 * f(b); };
const ratio = (a, b) => { const la = lum(a), lb = lum(b); return (Math.max(la, lb) + .05) / (Math.min(la, lb) + .05); };
const oklab = rgb => {
  const [r, g, b] = rgb.map(v => { const c = v / 255; return c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; });
  const l = Math.cbrt(.4122214708 * r + .5363325363 * g + .0514459929 * b);
  const m = Math.cbrt(.2119034982 * r + .6806995451 * g + .1073969566 * b);
  const s = Math.cbrt(.0883024619 * r + .2817188376 * g + .6299787005 * b);
  return [.2104542553 * l + .793617785 * m - .0040720468 * s, 1.9779984951 * l - 2.428592205 * m + .4505937099 * s, .0259040371 * l + .7827717662 * m - .808675766 * s];
};
const separation = (a, b) => { const x = oklab(a), y = oklab(b); return 100 * Math.hypot(...x.map((v, i) => v - y[i])); };
const fmt = r => r.toFixed(2);
let bad = 0; const rows = []; let worstSeparation = null;
for (const [theme, modes] of Object.entries(THEMES)) for (const [mode, t0] of Object.entries(modes)) {
  const alt = pick(mode === 'light' ? [`:root[data-theme="${theme}"][data-accent="${ACCENT2[theme]}"]`, `:root[data-accent="${ACCENT2[theme]}"]`] : [`:root[data-mode="dark"][data-theme="${theme}"][data-accent="${ACCENT2[theme]}"]`, `:root[data-mode="dark"][data-accent="${ACCENT2[theme]}"]`]);
  for (const [accentName, t] of [['main', t0], [ACCENT2[theme], { ...t0, ...alt }]]) {
    const c = k => hex(t[k]); const surface = c('surface'), raised = c('raised'), ground = c('ground');
    const pairs = [
      ['stage todo fill on the figure surface', c('stage-todo'), surface, 3], ['stage todo fill on the page ground', c('stage-todo'), ground, 3], ['stage todo fill on a raised card', c('stage-todo'), raised, 3],
      // Actual stage fills and line/main pairs are measured from the page below.
      ['outline ring (unk-ring) on the surface', c('fig-outline'), surface, 3], ['ok mark on the surface', c('ok'), surface, 3], ['warn mark on the surface', c('warn-mark'), surface, 3], ['figure secondary ink (ink2) on the surface', c('fig-ink-2'), surface, 3],
      ['accent as text (accent-ink) on the surface', c('accent-ink'), surface, 4.5], ['accent as text on the accent wash', c('accent-ink'), c('accent-wash'), 4.5], ['text on the surface', c('text'), surface, 4.5], ['secondary text on the surface', c('text-2'), surface, 4.5], ['tertiary text on the surface', c('text-3'), surface, 4.5],
      ['on-accent text on the accent', c('on-accent') || (mode === 'light' ? [246, 251, 250] : [16, 32, 29]), c('accent'), 4.5], ['focus ring on the surface', c('focus'), surface, 3],
    ];
    for (const [what, a, b, floor] of pairs) { const r = ratio(a, b); const ok = r >= floor; if (!ok) bad++; rows.push([theme, mode, accentName, what, fmt(r), floor + ':1', ok ? 'ok' : 'UNDER']); }
  }
}
// Preserve fractional sRGB channels, as the rendered-colour probe does.
const renderedRgb = raw => {
  if (/^#[0-9a-f]{6}$/i.test(raw)) return hex(raw);
  let m = raw.match(/^color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\)$/);
  if (m) return m.slice(1).map(v => Number(v) * 255);
  m = raw.match(/^rgb\(\s*([\d.]+)[, ]+([\d.]+)[, ]+([\d.]+)\s*\)$/);
  if (m) return m.slice(1).map(Number);
  throw new Error('Unsupported or non-opaque rendered colour: ' + raw);
};
(async () => {
  const pagePath = path.resolve(__dirname, '../prototype.html');
  const bytes = fs.readFileSync(pagePath);
  if (!bytes.toString().includes(`.st-line{background:${STAGE_LINE_FILL};`)) throw new Error('prototype.html has a stale line mix; run build.cjs before floors.cjs');
  console.log(`Page SHA256 ${crypto.createHash('sha256').update(bytes).digest('hex')}; shared line mix ${STAGE_LINE_PERCENT}%`);
  const tokenCount = rows.length;
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({ colorScheme: 'light', reducedMotion: 'reduce' });
    const page = await context.newPage();
    await page.goto(pathToFileURL(pagePath).href, { waitUntil: 'load' });
    for (const theme of Object.keys(THEMES)) for (const mode of ['light', 'dark']) for (const accent of ['main', ACCENT2[theme]]) {
      const samples = await page.evaluate(({ theme, mode, accent }) => {
        const root = document.documentElement;
        root.dataset.theme = theme; root.dataset.mode = mode;
        if (accent === 'main') delete root.dataset.accent; else root.dataset.accent = accent;
        const surface = getComputedStyle(root).getPropertyValue('--surface').trim();
        const found = new Map(); const variants = new Set();
        // Inspect every actual stage element, including inactive views. Deduplicate only
        // after resolving its styles in its own ancestor and tag context.
        for (const el of document.querySelectorAll('.st-main, .st-line, .st-line-f, .st-pr, .st-todo, .st-block, .st-now')) {
          const stage = [...el.classList].find(c => /^st-(main|line|line-f|pr|todo|block|now)$/.test(c));
          const svg = el instanceof SVGElement;
          const property = stage === 'st-now' && svg ? 'stroke' : svg ? 'fill' : 'backgroundColor';
          const colour = getComputedStyle(el)[property];
          const variant = `${svg ? 'SVG' : 'HTML'} .${stage}`;
          let main = null;
          if (stage === 'st-line' || stage === 'st-line-f') {
            variants.add(variant);
            // Resolve .st-main with the same tag and ancestors: SVG uses its actual
            // kit cascade, while HTML uses its own background rule. No colour override.
            const peer = el.cloneNode(false);
            peer.classList.remove(stage); peer.classList.add('st-main');
            el.after(peer);
            try { main = getComputedStyle(peer)[property]; } finally { peer.remove(); }
          }
          // A line mark's visible boundary is its 1 px accent outline: an inset shadow on HTML, a stroke on SVG. SVG .st-line draws no outline.
          let outline = null;
          if (stage === 'st-line' || stage === 'st-line-f') {
            const cs = getComputedStyle(el);
            if (svg) { if (parseFloat(cs.strokeWidth) > 0 && cs.stroke !== 'none') outline = cs.stroke; }
            else { const m = cs.boxShadow.match(/^(rgba?\([^)]*\)|color\([^)]*\))/); if (m) outline = m[1]; }
          }
          const sample = { variant, property, colour, main, surface, outline };
          const key = JSON.stringify(sample);
          if (!found.has(key)) found.set(key, { ...sample, view: el.closest('.view')?.id || 'page', count: 0 });
          found.get(key).count++;
        }
        for (const variant of ['HTML .st-line', 'SVG .st-line', 'SVG .st-line-f']) {
          if (!variants.has(variant)) throw new Error('Missing required stage variant: ' + variant);
        }
        return [...found.values()];
      }, { theme, mode, accent });
      for (const sample of samples) {
        console.log('RENDERED ' + JSON.stringify({ theme, mode, accent, ...sample }));
        const colour = renderedRgb(sample.colour), surface = renderedRgb(sample.surface);
        const r = ratio(colour, surface);
        if (sample.outline) {
          // WCAG 1.4.11: a mark is identified by its boundary, so the outline carries 3:1 and the fill is reported beside it.
          const o = ratio(renderedRgb(sample.outline), surface); if (o < 3) bad++;
          rows.push([theme, mode, accent, `${sample.variant} 1 px outline on surface (rendered)`, fmt(o), '3:1', o >= 3 ? 'ok' : 'UNDER']);
          rows.push([theme, mode, accent, `${sample.variant} fill on surface (reported)`, fmt(r), 'none', 'info']);
        } else {
          if (r < 3) bad++;
          rows.push([theme, mode, accent, `${sample.variant} ${sample.property} on surface (rendered)`, fmt(r), '3:1', r >= 3 ? 'ok' : 'UNDER']);
        }
        if (sample.main !== null) {
          const d = separation(colour, renderedRgb(sample.main));
          const ok = d >= STAGE_SEPARATION_FLOOR; if (!ok) bad++;
          rows.push([theme, mode, accent, `${sample.variant} vs matching .st-main (OKLab ×100)`, fmt(d), String(STAGE_SEPARATION_FLOOR), ok ? 'ok' : 'UNDER']);
          if (!worstSeparation || d < worstSeparation.value) worstSeparation = { value: d, theme, mode, accent, variant: sample.variant, colour: sample.colour, main: sample.main };
        }
      }
    }
    await context.close();
  } finally { await browser.close(); }
  const w = [9, 6, 7, 66, 6, 6, 6];
  console.log(['theme', 'mode', 'accent', 'pair', 'value', 'floor', 'result'].map((h, i) => h.padEnd(w[i])).join(' '));
  for (const r of rows) console.log(r.map((x, i) => String(x).padEnd(w[i])).join(' '));
  console.log(`\n${tokenCount} source-derived token pairs; ${rows.length - tokenCount} rendered stage pairs; ${bad} under the floor`);
  console.log('Worst rendered line/main OKLab distance ×100 (floor 15): ' + JSON.stringify(worstSeparation));
  process.exitCode = bad ? 1 : 0;
})().catch(err => { console.error(err); process.exitCode = 1; });

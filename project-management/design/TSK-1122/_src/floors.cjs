#!/usr/bin/env node
// TSK-1122 prototype: token floors. Reads kit/cf.css, resolves the tokens of each theme and mode, and computes the
// WCAG 2.x sRGB contrast of every meaningful colour pair against its floor: 3:1 for stage fills and marks (non-text),
// 4.5:1 for text on its surface. These are source-derived calculations, not measurements of the rendered page.
// Line/main separation is mandatory: Euclidean OKLab distance ×100 >= 15, including .st-line-f.
// Usage: node floors.cjs   (prints a table; exits 1 under any floor)
const fs = require('fs'); const path = require('path');
const { STAGE_LINE_PERCENT, STAGE_SEPARATION_FLOOR } = require('./stage-colours.cjs');
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
// CSS color-mix keeps fractional sRGB channels; do not round before measuring.
const mix = (a, b, p) => a.map((x, i) => x * p + b[i] * (1 - p));
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
    const line = mix(c('accent'), surface, STAGE_LINE_PERCENT / 100);
    const pairs = [
      ['stage todo fill on the figure surface', c('stage-todo'), surface, 3], ['stage todo fill on the page ground', c('stage-todo'), ground, 3], ['stage todo fill on a raised card', c('stage-todo'), raised, 3],
      // the stage fills emitted by build.cjs: .st-main = accent, .st-line and .st-line-f use the shared accent mix
      // mixed into the surface, .st-pr = warn-mark, .st-todo = stage-todo, .st-block = danger, .st-now = surface with
      // an accent stroke; figure strokes are removed), each a non-text fill against the figure surface at 3:1
      ['stage main (accent) fill on the surface', c('accent'), surface, 3], [`stage line (${STAGE_LINE_PERCENT}% accent) fill on the surface`, line, surface, 3],
      [`stage line-f (${STAGE_LINE_PERCENT}% accent) fill on the surface`, line, surface, 3],
      ['stage pull request (warn-mark) fill on the surface', c('warn-mark'), surface, 3], ['stage blocked (danger) fill on the surface', c('danger'), surface, 3],
      ['stage now ring (accent stroke) on the surface', c('accent'), surface, 3],
      ['outline ring (unk-ring) on the surface', c('fig-outline'), surface, 3], ['ok mark on the surface', c('ok'), surface, 3], ['warn mark on the surface', c('warn-mark'), surface, 3], ['figure secondary ink (ink2) on the surface', c('fig-ink-2'), surface, 3],
      ['accent as text (accent-ink) on the surface', c('accent-ink'), surface, 4.5], ['accent as text on the accent wash', c('accent-ink'), c('accent-wash'), 4.5], ['text on the surface', c('text'), surface, 4.5], ['secondary text on the surface', c('text-2'), surface, 4.5], ['tertiary text on the surface', c('text-3'), surface, 4.5],
      ['on-accent text on the accent', c('on-accent') || (mode === 'light' ? [246, 251, 250] : [16, 32, 29]), c('accent'), 4.5], ['focus ring on the surface', c('focus'), surface, 3],
    ];
    for (const [what, a, b, floor] of pairs) { const r = ratio(a, b); const ok = r >= floor; if (!ok) bad++; rows.push([theme, mode, accentName, what, fmt(r), floor + ':1', ok ? 'ok' : 'UNDER']); }
    // Surface gaps do not waive the mandatory normal-vision separation floor.
    for (const name of ['stage line', 'stage line-f']) {
      const d = separation(line, c('accent')); const ok = d >= STAGE_SEPARATION_FLOOR;
      if (!ok) bad++;
      rows.push([theme, mode, accentName, `${name} beside stage main (OKLab distance ×100)`, fmt(d), String(STAGE_SEPARATION_FLOOR), ok ? 'ok' : 'UNDER']);
      if (!worstSeparation || d < worstSeparation.value) worstSeparation = { value: d, theme, mode, accent: accentName, pair: name };
    }
    rows.push([theme, mode, accentName, 'stage todo beside stage main: adjacent fills, separated by a 2 px gap of the surface', fmt(ratio(c('stage-todo'), c('accent'))), 'n/a', 'info']);
  }
}
const w = [9, 6, 7, 66, 6, 6, 6];
console.log(['theme', 'mode', 'accent', 'pair', 'value', 'floor', 'result'].map((h, i) => h.padEnd(w[i])).join(' '));
for (const r of rows) console.log(r.map((x, i) => String(x).padEnd(w[i])).join(' '));
console.log(`\n${rows.filter(r => r[6] !== 'info').length} pairs over ${Object.keys(THEMES).length} themes, 2 modes, 2 accents each; ${bad} under the floor`);
console.log(`Line mix ${STAGE_LINE_PERCENT}%; worst line/main OKLab distance ×100: ${worstSeparation.value.toFixed(4)} (${worstSeparation.theme}/${worstSeparation.mode}/${worstSeparation.accent}, ${worstSeparation.pair}); floor ${STAGE_SEPARATION_FLOOR}`);
process.exit(bad ? 1 : 0);

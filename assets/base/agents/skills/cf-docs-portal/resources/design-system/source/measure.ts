/** Read-only evidence report for the authored palette, including every ink pair. */
import { readFile } from 'node:fs/promises';
import { validateColours, validateCompanions, measurePairs } from './validate.ts';
import type { Colours, Floors } from './validate.ts';
const colours: Colours = JSON.parse(await readFile(new URL('./colour.json', import.meta.url), 'utf8'));
const floors: Floors = JSON.parse(await readFile(new URL('./floors.json', import.meta.url), 'utf8'));
const worst = [];
for (const [theme, options] of Object.entries(colours.themes)) for (const mode of ['light', 'dark']) for (const accent of options) {
  const roles = colours.palettes[`${theme}-${mode}`]!;
  const inks = Object.fromEntries(['fig-line', 'fig-cat1', 'fig-cat2', 'fig-warn', 'fig-stop'].map(role => [role, roles[role]!]));
  inks['fig-accent'] = colours.accents[accent]![mode]!;
  const pairs = measurePairs(inks);
  for (const simulation of ['normal', 'protan', 'deutan', 'tritan']) {
    const pair = pairs.filter(pair => pair.simulation === simulation).sort((a, b) => a.distance - b.distance)[0];
    worst.push({ theme, mode, accent, ...pair });
  }
}
const findings = [...validateColours(colours, floors), ...validateCompanions(colours, floors)];
console.log(JSON.stringify({ worst, findings }, null, 2));
process.exitCode = findings.length ? 1 : 0;

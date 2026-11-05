import { contrast, hueGap, lab, rgb, separation } from './colour.ts';
import { accentFamily } from './accent.ts';
import type { Deficiency } from './colour.ts';
export interface Colours {
  defaultAccent: string;
  themes: Record<string, string[]>;
  accents: Record<string, Record<string, string>>;
  palettes: Record<string, Record<string, string>>;
}
export interface Floors {
  graphiteLightness: Record<string, number[]>;
  textContrast: number; uiContrast: number; stageContrast: number;
  stageSeparation: number; simulatedSeparation: number; accentStatusHueGap: number;
  companionSeparation: number; companionStopSeparation: number;
  skinCanvasSeparation: number; skinSurfaceSeparation: number;
  minimumSegmentPx: number; minimumArcDegrees: number;
}
export interface Finding { context: string; rule: string; measured?: number; floor?: number; }
export function validateColours(source: Colours, floors: Floors): Finding[] {
  const findings: Finding[] = [];
  const below = (context: string, rule: string, measured: number, floor: number): void => {
    if (measured < floor) findings.push({ context, rule, measured, floor });
  };
  for (const [theme, options] of Object.entries(source.themes)) {
    if (!['graphite', 'slate', 'sage'].includes(theme)) findings.push({ context: theme, rule: 'theme' });
    if (new Set(options).size < 2) findings.push({ context: theme, rule: 'options' });
    if (!options.includes(source.defaultAccent)) findings.push({ context: theme, rule: 'default' });
    for (const mode of ['light', 'dark']) {
      const context = `${theme}/${mode}`, roles = source.palettes[`${theme}-${mode}`];
      if (!roles) { findings.push({ context, rule: 'palette' }); continue; }
      if (theme === 'graphite') {
        for (const role of ['canvas', 'surface', 'surface-raised', 'surface-subtle', 'paper', 'panel', 'chrome', 'text', 'text-muted', 'border', 'border-strong']) {
          const [r, g, b] = rgb(roles[role]!);
          if (r !== g || g !== b) findings.push({ context: `${context}/${role}`, rule: 'neutral' });
        }
        const canvas = roles.canvas!, lightness = lab(canvas)[0], band = floors.graphiteLightness[mode]!;
        if (['#ffffff', '#000000'].includes(canvas.toLowerCase()) || lightness < band[0]! || lightness > band[1]!) findings.push({ context, rule: 'canvas', measured: lightness });
      }
      for (const option of options) {
        const base = source.accents[option]?.[mode];
        if (!base) { findings.push({ context: `${context}/${option}`, rule: 'accent' }); continue; }
        for (const surface of ['canvas', 'surface', 'surface-raised', 'paper', 'panel', 'chrome']) {
          below(`${context}/${option}/${surface}`, 'contrast', contrast(base, roles[surface]!), floors.textContrast);
          below(`${context}/${option}/focus/${surface}`, 'contrast', contrast(base, roles[surface]!), floors.uiContrast);
        }
        const family = accentFamily(base, mode, roles.surface!, floors.textContrast);
        below(`${context}/${option}/on-accent`, 'contrast', contrast(family['on-accent']!, base), floors.textContrast);
        below(`${context}/${option}/accent-wash`, 'contrast', contrast(base, family['accent-soft']!), floors.textContrast);
        for (const role of ['positive', 'warning', 'danger']) below(`${context}/${option}/${role}`, 'hue', hueGap(base, roles[role]!), floors.accentStatusHueGap);
      }
    }
  }
  const themes = Object.keys(source.themes).filter(theme => ['graphite', 'slate', 'sage'].includes(theme));
  for (const mode of ['light', 'dark']) for (let i = 0; i < themes.length; i++) for (const other of themes.slice(i + 1)) {
    for (const [role, floor] of [['canvas', floors.skinCanvasSeparation], ['surface', floors.skinSurfaceSeparation]] as const) {
      const first = source.palettes[`${themes[i]}-${mode}`]?.[role], second = source.palettes[`${other}-${mode}`]?.[role];
      if (first && second) below(`${mode}/${themes[i]}/${other}/${role}`, 'skin separation', separation(first, second), floor);
    }
  }
  return findings;
}

export interface PairMeasurement { first: string; second: string; simulation: 'normal' | Deficiency; distance: number; }
export function measurePairs(roles: Record<string, string>): PairMeasurement[] {
  const entries = Object.entries(roles), measurements: PairMeasurement[] = [];
  for (let i = 0;i < entries.length;i++)for (let j = i + 1;j < entries.length;j++) {
    const [first, a] = entries[i]!, [second, b] = entries[j]!;
    for (const simulation of ['normal', 'protan', 'deutan', 'tritan'] as const)
      measurements.push({ first, second, simulation, distance: separation(a, b, simulation === 'normal' ? undefined : simulation) });
  }
  return measurements;
}

export function validateStages(roles: Record<string, string>, surfaces: Record<string, string>, floors: Floors): Finding[] {
  const findings: Finding[] = [];
  for (const [role, colour] of Object.entries(roles)) for (const [surface, ground] of Object.entries(surfaces)) {
    const measured = contrast(colour, ground);
    if (measured < floors.stageContrast) findings.push({ context: `${role}/${surface}`, rule: 'stage contrast', measured, floor: floors.stageContrast });
  }
  for (const pair of measurePairs(roles)) {
    const floor = pair.simulation === 'normal' ? floors.stageSeparation : floors.simulatedSeparation;
    if (pair.distance < floor) findings.push({ context: `${pair.first}/${pair.second}/${pair.simulation}`, rule: 'stage separation', measured: pair.distance, floor });
  }
  return findings;
}

/** The kit's established companion protections also apply to curated accents. */
export function validateCompanions(source: Colours, floors: Floors): Finding[] {
  const findings: Finding[] = [];
  for (const [theme, options] of Object.entries(source.themes)) for (const mode of ['light', 'dark']) {
    const palette = source.palettes[`${theme}-${mode}`]; if (!palette) continue;
    for (const option of options) {
      const accent = source.accents[option]?.[mode]; if (!accent) continue;
      const roles: Record<string, string> = { ...palette, 'fig-accent': accent };
      for (const [first, second, floor] of [
        ['fig-cat1', 'fig-cat2', floors.companionSeparation],
        ['fig-cat1', 'fig-accent', floors.companionSeparation],
        ['fig-cat2', 'fig-accent', floors.companionSeparation],
        ['fig-cat1', 'fig-warn', floors.companionSeparation],
        ['fig-cat2', 'fig-warn', floors.companionSeparation],
        ['fig-cat1', 'fig-stop', floors.companionSeparation],
        ['fig-cat2', 'fig-stop', floors.companionStopSeparation],
      ] as const) {
        const measured = separation(roles[first]!, roles[second]!);
        if (measured < floor) findings.push({ context: `${theme}/${mode}/${option}/${first}/${second}`, rule: 'companion separation', measured, floor });
      }
      const inks = Object.fromEntries(['fig-line', 'fig-accent', 'fig-cat1', 'fig-cat2', 'fig-warn', 'fig-stop'].map(role => [role, roles[role]!]));
      for (const pair of measurePairs(inks)) if (pair.simulation !== 'normal' && pair.distance < floors.simulatedSeparation)
        findings.push({ context: `${theme}/${mode}/${option}/${pair.first}/${pair.second}/${pair.simulation}`, rule: 'simulated separation', measured: pair.distance, floor: floors.simulatedSeparation });
    }
  }
  return findings;
}

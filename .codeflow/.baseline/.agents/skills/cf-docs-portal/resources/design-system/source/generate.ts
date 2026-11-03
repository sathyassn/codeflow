/** Run with Node 26: node source/generate.ts [--check].
 * Values are authored only in source/*.json. Outputs are reproducible and
 * self-contained so an installed kit never needs the CodeFlow checkout. */
import { readFile, writeFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { fileURLToPath } from 'node:url';
import { styleLiterals } from './lint.ts';
import { accentFamily } from './accent.ts';
import { validateColours, validateCompanions, validateStages } from './validate.ts';
import type { Colours, Floors } from './validate.ts';
interface TokenBlock { selector: string; media?: string; values: Record<string, string>; }
const here = new URL('./', import.meta.url);
function javascript(source: string): string {
  return stripTypeScriptTypes(source).split('\n').map(line => line.trimEnd()).join('\n').trimEnd() + '\n';
}
async function json<T>(name: string): Promise<T> { return JSON.parse(await readFile(new URL(name, here), 'utf8')) as T; }
function declarations(values: Record<string, string>): string {
  return Object.entries(values).map(([name, value]) => `  --cf-${name}: ${value};`).join('\n');
}
function selectors(theme: string, mode: string): string[] {
  return [`[data-cf-skin="${theme}"][data-cf-theme="${mode}"]`];
}
function consumerTokens(css: string, consumer: string): string {
  css = css.replace(/^  --cf-length-[^\n]+\n/gm, '');
  if (consumer === 'present') return css.replaceAll('data-cf-theme=', 'data-cf-mode-resolved=').replaceAll('data-cf-skin=', 'data-cf-theme=');
  if (consumer === 'utility-tokens') return css.replaceAll('data-cf-theme=', 'data-theme=').replaceAll('data-cf-skin=', 'data-cfp-skin=');
  return css;
}
export async function generate(): Promise<Map<string, string>> {
  const colours = await json<Colours>('colour.json'), floors = await json<Floors>('floors.json');
  const stageRoles = await json<Record<string, string>>('stages.json');
  const findings = [...validateColours(colours, floors), ...validateCompanions(colours, floors)];
  for (const [pair, roles] of Object.entries(colours.palettes)) {
    const [theme, mode] = pair.split('-') as [string, string];
    for (const option of colours.themes[theme]!) {
      const palette: Record<string, string> = { ...roles, 'fig-accent': colours.accents[option]![mode]! };
      const stages = Object.fromEntries(Object.entries(stageRoles).map(([stage, role]) => [stage, palette[role]!]));
      const grounds = Object.fromEntries(['canvas', 'surface', 'paper', 'panel', 'chrome'].map(role => [role, roles[role]!]));
      findings.push(...validateStages(stages, grounds, floors).map(f => ({ ...f, context: `${pair}/${option}/${f.context}` })));
    }
  }
  if (findings.length) throw new Error(`Token floors failed:\n${JSON.stringify(findings, null, 2)}`);
  const geometry = await json<Record<string, number>>('geometry.json');
  const layout = await json<Record<string, string>>('layout.json');
  const stages = await json<Record<string, string>>('stages.json');
  const blocks = (await Promise.all(['type', 'space', 'radius', 'elevation', 'motion'].map(name => json<TokenBlock[]>(`${name}.json`)))).flat();
  let css = '/* Generated from source/*.json. Run node source/generate.ts; do not edit. */\n';
  const light = colours.palettes['graphite-light']!;
  const defaults = { ...light, ...accentFamily(colours.accents[colours.defaultAccent]!.light!, 'light', light.surface!, floors.textContrast) };
  for (const [stage, role] of Object.entries(stages)) defaults[`stage-${stage}`] = defaults[role]!;
  const common = Object.assign({}, layout, ...blocks.filter(block => block.selector === ':root' && !block.media).map(block => block.values), defaults);
  css += `:root {\n${declarations(common)}\n}\n`;
  for (const block of blocks) {
    if (block.selector === ':root' && !block.media) continue;
    const selector = block.selector.includes('data-cf-typeface') || block.selector.includes('data-cf-scale')
      ? `${block.selector}, ${block.selector.replaceAll('data-cf-typeface', 'data-cfp-typeface').replaceAll('data-cf-scale', 'data-cfp-scale')}` : block.selector;
    const rule = `${selector} {\n${declarations(block.values)}\n}\n`;
    css += block.media ? `@media ${block.media} {\n${rule}}\n` : rule;
  }
  const resolved: Record<string, Record<string, string>> = {};
  for (const [pair, roles] of Object.entries(colours.palettes)) {
    const [theme, mode] = pair.split('-') as [string, string];
    const family = accentFamily(colours.accents[colours.defaultAccent]![mode]!, mode, roles.surface!, floors.textContrast);
    const values = { ...roles, ...family };
    for (const [stage, role] of Object.entries(stages)) values[`stage-${stage}`] = values[role]!;
    resolved[pair] = values;
    // Separate blocks preserve the kit's established selector contract.
    for (const selector of selectors(theme, mode)) css += `${selector} {\n${declarations(values)}\n}\n`;
    for (const option of colours.themes[theme]!) {
      const accent = accentFamily(colours.accents[option]![mode]!, mode, roles.surface!, floors.textContrast);
      for (const [stage, role] of Object.entries(stages)) if (role in accent) accent[`stage-${stage}`] = accent[role]!;
      for (const selector of selectors(theme, mode)) css += `${selector}[data-cf-accent="${option}"] {\n${declarations(accent)}\n}\n`;
    }
  }
  const exportTokens = ['light', 'dark'].map(mode => {
    const rules = Object.entries(resolved).filter(([pair]) => pair.endsWith('-' + mode)).map(([pair, roles]) => {
      const theme = pair.slice(0, -mode.length - 1);
      const selector = `:root[data-cf-mode="system"][data-cf-theme="${theme}"]`;
      let rule = `${selector} {\n${declarations(roles)}\n}\n`;
      for (const option of colours.themes[theme]!) {
        const family = accentFamily(colours.accents[option]![mode]!, mode, roles.surface!, floors.textContrast);
        for (const [stage, role] of Object.entries(stages)) if (role in family) family[`stage-${stage}`] = family[role]!;
        rule += `${selector}[data-cf-accent="${option}"] {\n${declarations(family)}\n}\n`;
      }
      return rule;
    }).join('');
    return mode === 'dark' ? `@media (prefers-color-scheme: dark) {\n${rules}}\n` : rules;
  }).join('');
  const settings = { themes: colours.themes, modes: ['system', 'light', 'dark'], sizes: ['compact', 'default', 'large'], defaultTheme: 'graphite', defaultAccent: colours.defaultAccent };
  const lengths = Object.fromEntries([2, 4, 8, 10, 12, 16, 20, 24, 26, 40, 46, 48, 64, 240, 280, 360].map(n => [`px${n}`, Number.parseFloat(layout[`length-${n}px`]!)]));
  const runtime = { lengths, minimumMarkerRem: Number.parseFloat(layout['length-2-25rem']!), paper: layout['print-paper'], ink: colours.palettes['graphite-light']!.text, compactQuery: `(max-width: ${layout['length-879-98px']})`, tocQuery: `(max-width: ${layout['length-1119-98px']})`, phoneQuery: `(max-width: ${layout['length-599-98px']})`, cardQuery: `(max-width: ${layout['length-639-98px']})` };
  const typed = `// Generated from source/*.json. Do not edit.\nexport const runtime = ${JSON.stringify(runtime)} as const;\nexport const tokenBlocks = ${JSON.stringify(blocks, null, 2)} as const;\nexport const layout = ${JSON.stringify(layout, null, 2)} as const;\nexport const settings = ${JSON.stringify(settings, null, 2)} as const;\nexport const palettes = ${JSON.stringify(resolved, null, 2)} as const;\nexport const floors = ${JSON.stringify(floors, null, 2)} as const;\nexport const geometry = ${JSON.stringify(geometry, null, 2)} as const;\nexport const stageNames = ${JSON.stringify(Object.keys(stages))} as const;\nexport type Theme = keyof typeof settings.themes;\nexport type Mode = typeof settings.modes[number];\nexport type Accent = typeof settings.themes[Theme][number];\n`;
  const outputs = new Map([['tokens.css', css], ['tokens.json', JSON.stringify(resolved, null, 2) + '\n'], ['tokens.ts', typed]]);
  const chartSource = await readFile(new URL('count-charts.ts', here), 'utf8');
  outputs.set('count-charts.js', javascript(chartSource).replace("../tokens.ts", "./tokens.js").replace("./stage-geometry.ts", "./stage-geometry.js"));
  outputs.set('settings.js', `// Generated display choices.\nglobalThis.CF_DISPLAY_SETTINGS = ${JSON.stringify(settings)};\nglobalThis.CF_RUNTIME_LAYOUT = ${JSON.stringify(runtime)};\n`);
  outputs.set('tokens.js', javascript(typed));
  outputs.set('stage-geometry.js', javascript(await readFile(new URL('stage-geometry.ts', here), 'utf8')));
  for (const name of ['chrome', 'figure', 'present', 'export-fallback', 'portal', 'utility-tokens', 'figure-roles']) {
    const template = await readFile(new URL(`styles/${name}.css.in`, here), 'utf8');
    const literals = styleLiterals(template);
    if (literals.length) throw new Error(`${name} has authored style literals: ${literals.join(', ')}`);
    let output = template.replace(/\{\{([\w-]+)\}\}/g, (_match, key: string) => {
      if (key === 'export-tokens') return exportTokens.trimEnd();
      if (key === 'tokens') return consumerTokens(css, name);
      if (!(key in layout)) throw new Error(`Unknown layout token ${key}`);
      return layout[key]!;
    });
    // Resolve fixed layout values at generation time; semantic tokens remain
    // custom properties. This avoids repeating long variable names per rule.
    output = output.replace(/var\(--cf-(length-[\w-]+)\)/g, (match, key: string) => layout[key] ?? match);
    outputs.set(`${name}.css`, '/* Generated from source/styles and source/*.json. Do not edit. */\n' + output);
  }
  return outputs;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const [name, bytes] of await generate()) {
    const target = new URL('../' + name, here);
    if (process.argv.includes('--check')) {
      if (await readFile(target, 'utf8') !== bytes) throw new Error(`${name} drifted; regenerate tokens`);
    } else await writeFile(target, bytes);
  }
}

/** Synchronize generated consumers, skill mirrors and manifest entries. */
import { cp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { styleLiterals } from '../assets/base/agents/skills/cf-present/resources/design-system/source/lint.ts';
import { generate } from '../assets/base/agents/skills/cf-present/resources/design-system/source/generate.ts';
process.chdir(fileURLToPath(new URL('..', import.meta.url)));
const canonical = 'assets/base/agents/skills/cf-present/resources/design-system';
const check = process.argv.includes('--check');
async function put(path: string, bytes: string | Buffer): Promise<void> {
  if (check) { if (!Buffer.from(await readFile(path)).equals(Buffer.from(bytes))) throw new Error(`Token consumer drift: ${path}`); }
  else { await mkdir(dirname(path), { recursive: true }); await writeFile(path, bytes); }
}
const outputs = await generate();
for (const [name, bytes] of outputs) await put(join(canonical, name), bytes);
const paths: Record<string, string[]> = {
  'present.css': ['crates/codeflow-present/web/src/styles.css'],
  'export-fallback.css': ['crates/codeflow-present/web/src/export-fallback.css'],
  'figure.css': ['crates/codeflow-present/web/src/figure.css', 'assets/docs-portal/starter/src/styles/figure.css', 'docs-portal/src/styles/figure.css'],
  'tokens.ts': ['crates/codeflow-present/web/src/tokens.ts', 'assets/docs-portal/starter/src/tokens.ts', 'docs-portal/src/tokens.ts'],
  'tokens.js': ['assets/docs-portal/starter/public/tokens.js', 'docs-portal/public/tokens.js'],
};
for (const name of ['portal', 'utility-tokens', 'figure-roles']) paths[`${name}.css`] = ['assets/docs-portal/starter', 'docs-portal'].map(p => `${p}/src/styles/${name}.css`);
for (const [name, targets] of Object.entries(paths)) for (const target of targets) await put(target, await readFile(join(canonical, name)));
async function walk(path: string, prefix = ''): Promise<string[]> {
  const files = [];
  for (const entry of await readdir(path, { withFileTypes: true })) files.push(...entry.isDirectory() ? await walk(join(path, entry.name), prefix + entry.name + '/') : [prefix + entry.name]);
  return files.sort();
}
let manifest = await readFile('assets/base/scaffold-manifest.toml', 'utf8');
const installed = JSON.parse(await readFile('.codeflow/manifest.json', 'utf8'));
for (const skill of ['cf-present', 'cf-docs-portal']) for (const relative of await walk(canonical)) {
  const source = `agents/skills/${skill}/resources/design-system/${relative}`, bytes = await readFile(join(canonical, relative));
  if (skill !== 'cf-present') await put(`assets/base/${source}`, bytes);
  for (const harness of ['.agents', '.claude']) {
    const dest = `${harness}/skills/${skill}/resources/design-system/${relative}`;
    await put(dest, bytes); await put(`.codeflow/.baseline/${dest}`, bytes);
    if (!manifest.includes(`dest = "${dest}"`)) manifest += `\n[[entry]]\nsrc = "${source}"\ndest = "${dest}"\nownership = "managed"\ntiers = ["standard", "full"]\n`;
    installed.files[dest] = { src: source, ownership: 'managed', sha256: createHash('sha256').update(bytes).digest('hex'), exec: false };
  }
}
await put('assets/base/scaffold-manifest.toml', manifest);
// Match InstalledManifest's BTreeMap serialization so update stays a no-op.
installed.files = Object.fromEntries(Object.keys(installed.files).sort().map(path => [path, installed.files[path]]));
await put('.codeflow/manifest.json', JSON.stringify(installed, null, 2) + '\n');

// Refresh only the adopted files whose bytes equal this starter revision.
const portalManifest = JSON.parse(await readFile('assets/docs-portal/manifest.json', 'utf8'));
const portalState = JSON.parse(await readFile('.codeflow/docs-portal.json', 'utf8'));
for (const entry of portalManifest.files) {
  if (entry.ownership !== 'managed') continue;
  const bytes = await readFile(join('assets/docs-portal/starter', entry.path));
  const actual = await readFile(join('docs-portal', entry.path));
  if (bytes.equals(actual)) portalState.files[entry.path] = { ownership: entry.ownership, pristine_sha256: createHash('sha256').update(bytes).digest('hex') };
}
await put('.codeflow/docs-portal.json', JSON.stringify(portalState, null, 2) + '\n');

// The generated file set is explicit. New consumer sheets cannot opt out by
// claiming to be generated in a comment.
const kitRoots = ['assets/base/agents/skills', '.agents/skills', '.claude/skills', '.codeflow/.baseline/.agents/skills', '.codeflow/.baseline/.claude/skills']
  .flatMap(root => ['cf-present', 'cf-docs-portal'].map(skill => `${root}/${skill}/resources/design-system`));
const generatedSheets = new Set([...kitRoots.flatMap(root => [...outputs.keys()].filter(name => name.endsWith('.css')).map(name => join(root, name))),
  ...Object.entries(paths).filter(([name]) => name.endsWith('.css')).flatMap(([, targets]) => targets.map(target => join(target)))]);
for (const root of [...kitRoots, 'crates/codeflow-present/web/src', 'docs-portal/src', 'assets/docs-portal/starter/src']) {
  for (const relative of await walk(root)) {
    const path = join(root, relative);
    if (!path.endsWith('.css') || generatedSheets.has(path)) continue;
    const literals = styleLiterals(await readFile(path, 'utf8'));
    if (literals.length) throw new Error(`Authored style literals in ${path}: ${literals.join(', ')}`);
  }
}

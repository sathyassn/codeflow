/* W4 verifier — the pre-registered gates, run against the recorded evidence.
 *
 * It judges no composition. It decides whether the observation gates in
 * rubric.v4.md are capable of meaning anything, and it fails closed.
 *
 *   node tools/verify.mjs             compare, fail on drift
 *   node tools/verify.mjs --update    refresh SHA256SUMS, but only if every
 *                                     other check passed first
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const ROOT = path.resolve(import.meta.dirname, '..');
const W3 = path.resolve(ROOT, '..', 'tsk-014-w3');
const REPO = path.resolve(ROOT, '..', '..', '..');
const UPDATE = process.argv.includes('--update');

const registry = JSON.parse(fs.readFileSync(path.join(ROOT, 'registry.json'), 'utf8'));
const amendments = JSON.parse(fs.readFileSync(path.join(ROOT, 'amendments.json'), 'utf8'));
const dispositions = JSON.parse(fs.readFileSync(path.join(ROOT, 'dispositions.json'), 'utf8'));
const check = (name) => JSON.parse(fs.readFileSync(path.join(ROOT, 'checks', `${name}.json`), 'utf8'));

const results = [];
const ok = (gate, what) => results.push({ gate, pass: true, what });
const bad = (gate, what) => results.push({ gate, pass: false, what });
const sha = (f) => crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const read = (f) => fs.readFileSync(f, 'utf8');

/* ---- locks and ordering ---------------------------------------------- */

for (const [file, lockFile] of [['rubric.v4.md', 'rubric.v4.lock.json'], ['registry.json', 'registry.lock.json']]) {
  const lock = JSON.parse(read(path.join(ROOT, lockFile)));
  const now = sha(path.join(ROOT, file));
  if (now === lock.sha256) ok('lock', `${file} matches ${lockFile}`);
  else bad('lock', `${file} digest ${now.slice(0, 12)} does not match its lock ${lock.sha256.slice(0, 12)} — a pinned file was edited`);
}

const authored = [];
const walk = (dir) => {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else authored.push(full);
  }
};
for (const dir of ['cases', 'directions', 'portal', 'baselines', 'renders', 'shared']) walk(path.join(ROOT, dir));
const pinnedMtime = Math.max(
  fs.statSync(path.join(ROOT, 'rubric.v4.md')).mtimeMs,
  fs.statSync(path.join(ROOT, 'registry.json')).mtimeMs,
);
const earlier = authored.filter((f) => fs.statSync(f).mtimeMs < pinnedMtime);
if (!earlier.length) ok('ordering', `both pinned files precede all ${authored.length} authored artefacts and renders in this working tree (mtime; not durable across a checkout)`);
else bad('ordering', `${earlier.length} authored files predate a pinned file, e.g. ${path.relative(ROOT, earlier[0])}`);

/* ---- W3 is append-only ------------------------------------------------ */

const w3Lock = JSON.parse(read(path.join(W3, 'rubric.v2.lock.json')));
const w3Now = sha(path.join(W3, 'rubric.v2.md'));
if (w3Now === (w3Lock.sha256 ?? w3Lock.digest ?? w3Lock.rubric_sha256)) ok('w3-append-only', 'W3 rubric.v2.md still matches its own lock');
else bad('w3-append-only', 'W3 rubric.v2.md no longer matches its own lock — this study must not edit W3');

/* ---- A1 answer-channel separation ------------------------------------- */

const text = check('text');
let leaks = 0;
for (const row of text) {
  const caseId = row.render.split('-')[0];
  const spec = registry.cases.find((c) => c.id === caseId);
  if (!spec) continue;
  for (const phrase of spec.forbidden_phrases) {
    if (row.visible.toLowerCase().includes(phrase.toLowerCase())) { leaks += 1; bad('A1', `${row.render} shows the forbidden phrase "${phrase}"`); }
  }
  for (const pattern of spec.forbidden_patterns) {
    if (new RegExp(pattern.pattern, 'iu').test(row.visible)) { leaks += 1; bad('A1', `${row.render} matches forbidden pattern ${pattern.pattern}`); }
  }
}
if (!leaks) ok('A1', `no registered forbidden phrase or pattern appears in the visible text captured off ${text.length} renders`);

/* ---- A2 declared distinctness ------------------------------------------ */

let clashes = 0;
for (const spec of registry.cases) {
  const siblings = registry.candidates.filter((c) => c.case === spec.id);
  for (let i = 0; i < siblings.length; i += 1) {
    for (let j = i + 1; j < siblings.length; j += 1) {
      const a = siblings[i]; const b = siblings[j];
      if (a.primary_unit === b.primary_unit && a.primary_axis === b.primary_axis) {
        clashes += 1; bad('A2', `${a.id} and ${b.id} share unit and axis`);
      }
    }
  }
}
if (!clashes) ok('A2', `no two siblings share unit and axis across ${registry.candidates.length} registered candidates`);

/* ---- A3 the intermediate context is real ------------------------------- */

const tabletWidth = registry.thresholds.viewports.tablet.width;
for (const candidate of registry.candidates) {
  const page = path.join(ROOT, candidate.carried_from ? `cases/${candidate.case}/${candidate.id}/index.html` : `cases/${candidate.case}/${candidate.id}/index.html`);
  if (!fs.existsSync(page)) { bad('A3', `${candidate.id} has no page`); continue; }
  const declared = [...read(page).matchAll(/max-width:\s*(\d+)px/g)].map((m) => Number(m[1])).sort((a, b) => a - b);
  const amendment = amendments.entries.find((e) => e.artefacts.some((a) => a.endsWith(candidate.id)) && e.field.includes('breakpoint'));
  const matchesDeclaration = declared.includes(candidate.wide_breakpoint) && declared.includes(candidate.narrow_breakpoint);
  const real = declared.filter((w) => w < 1240);
  const straddles = real.some((w) => w < tabletWidth) && real.some((w) => w > tabletWidth);
  if (matchesDeclaration && straddles) ok('A3', `${candidate.id} declares both breakpoints and the intermediate width falls between them`);
  else if (straddles && amendment) ok('A3', `${candidate.id}: the intermediate width falls strictly between the page's real breakpoints (${real.join(', ')}); the registered numbers differ and the divergence is recorded as ${amendment.id} rather than edited away`);
  else bad('A3', `${candidate.id}: no real media query straddles the intermediate width, and no amendment records why`);
}

/* ---- A4 carrier honesty ------------------------------------------------ */

const schema = JSON.parse(read(path.join(REPO, '.codeflow/schemas/present/document-v1.schema.json')));
const liveBlocks = new Set([...new Set(JSON.stringify(schema).match(/"const":\s*"([a-z_]+)"/g) ?? [])].map((m) => m.split('"')[3]));
for (const block of registry.carrier_contracts.cf_present.closed_block_types) {
  if (!liveBlocks.has(block)) bad('A4', `registry names block "${block}" which the live schema does not define`);
}
const sanitiser = read(path.join(REPO, registry.carrier_contracts.cf_present.verbatim_constraint_source));
if (sanitiser.includes(registry.carrier_contracts.cf_present.verbatim_constraint)) {
  ok('A4', `the carrier constraint "${registry.carrier_contracts.cf_present.verbatim_constraint}" is verbatim in ${registry.carrier_contracts.cf_present.verbatim_constraint_source}`);
} else {
  bad('A4', 'the quoted carrier constraint is not present verbatim in the source that imposes it');
}
for (const candidate of registry.candidates) {
  if (candidate.carrier.verdict !== 'native' && !candidate.carrier.loss) bad('A4', `${candidate.id} is non-native and states no material loss`);
}
if (liveBlocks.size) ok('A4', `all ${registry.carrier_contracts.cf_present.closed_block_types.length} registered block types exist in the live schema`);

/* ---- A5 motion --------------------------------------------------------- */

const motion = check('motion');
const moving = motion.filter((m) => m.running > 0 || m.named.length || m.transitions.length);
if (registry.contexts.motion.policy === 'declared-per-artefact') {
  const caseMoving = moving.filter((m) => !m.render.startsWith('dir-') && !m.render.startsWith('portal-'));
  if (!caseMoving.length) ok('A5', `no case artefact animates, under either prefers-reduced-motion setting, across ${motion.length} probes`);
  else bad('A5', `${caseMoving.length} case probes found motion where none is declared`);
  const directionMoving = moving.filter((m) => m.render.startsWith('dir-'));
  if (!directionMoving.length) ok('A5', 'every direction declares none-authored motion and every probe agrees');
  else bad('A5', `${directionMoving.length} direction probes found motion against a none-authored declaration`);
}

/* ---- A7 frame containment ---------------------------------------------- */

const frame = check('frame');
const failed = frame.filter((f) => f.hiddenOverflow.length || f.outsideViewBox.length || f.documentOverflow > 0);
if (!failed.length) ok('A7', `all ${frame.length} renders carry what their page contains — no hidden overflow, nothing outside its own frame, no document overflow`);
else for (const f of failed) bad('A7', `${f.render}: hidden ${f.hiddenOverflow.length}, outside ${f.outsideViewBox.length}, document +${f.documentOverflow}px`);

/* ---- A8 registration honesty ------------------------------------------- */

if (registry.pre_registration_evidence?.what_the_files_cannot_show && registry.pre_registration_evidence?.standing) {
  ok('A8', 'the registry states what its priority can and cannot be shown to be');
} else bad('A8', 'the registry carries no honest pre-registration account');
if (Array.isArray(amendments.entries)) {
  ok('A8', `${amendments.entries.length} divergences recorded in amendments.json; registry.json itself is byte-frozen under its lock`);
} else bad('A8', 'no amendments record');

/* ---- A9 the demonstration is produced by its own mechanism -------------- */

const mechanism = check('mechanism');
for (const direction of registry.directions) {
  const rows = mechanism.filter((m) => m.render.startsWith(`dir-${direction.id}`));
  if (!rows.length) { bad('A9', `${direction.id} has no render`); continue; }
  const emitters = [...new Set(rows.flatMap((r) => r.emitted.map((e) => e.by)))];
  const expected = direction.id === 'e-raster' ? null : direction.id;
  if (!expected) { ok('A9', `${direction.id} emits no compiled figure, which is the direction: the payload is a raster`); continue; }
  if (emitters.length && emitters.every((e) => e && e.includes(expected.split('-')[1]))) {
    ok('A9', `${direction.id} figures are emitted by ${emitters.join(', ')}`);
  } else {
    bad('A9', `${direction.id}: emitters ${JSON.stringify(emitters)} do not name its own mechanism`);
  }
}

/* ---- A10 the shell does not decide the contest -------------------------- */

const shell = check('shell');
const shellDigest = sha(path.join(ROOT, 'shared/shell.css'));
const served = [...new Set(shell.map((s) => s.sha256))];
if (served.length === 1 && served[0] === shellDigest) ok('A10', `every page was served the byte-identical shell (${shellDigest.slice(0, 12)})`);
else bad('A10', `the shell served differs across pages or from disk: ${served.map((s) => s.slice(0, 12)).join(', ')}`);

/* ---- A11 every failed W3 band has a disposition ------------------------- */

const w3 = JSON.parse(read(path.join(W3, 'registry.json')));
const registered = w3.invalidated_bands.map((b) => b.render);
const covered = dispositions.entries.map((e) => e.w3_render);
const missing = registered.filter((r) => !covered.includes(r));
if (missing.length) bad('A11', `${missing.length} W3 bands have no disposition: ${missing.join(', ')}`);
else ok('A11', `all ${registered.length} W3 invalidated bands carry a disposition (${dispositions.counts.repaired} repaired, ${dispositions.counts.replaced} replaced, ${dispositions.counts.withdrawn} withdrawn)`);
for (const entry of dispositions.entries) {
  const target = path.join(ROOT, entry.after.now);
  if (!fs.existsSync(target)) bad('A11', `${entry.w3_render} names ${entry.after.now}, which does not exist`);
}
const renders = fs.readdirSync(path.join(ROOT, 'renders'));
for (const entry of dispositions.entries) {
  if (!renders.includes(`${entry.w3_render}.png`)) bad('A11', `${entry.w3_render} has no re-rendered W4 counterpart`);
}

/* ---- A12 every drawn mark is keyed -------------------------------------- */

const keys = check('keys');
const unkeyed = keys.flatMap((k) => k.figures.filter((f) => f.describedMissing.length).map((f) => `${k.render}: ${f.describedMissing.join(', ')}`));
if (!unkeyed.length) ok('A12', `every state drawn with a declared channel is keyed, across ${keys.length} renders`);
else for (const u of unkeyed) bad('A12', u);

/* ---- A13 legible at the size it renders --------------------------------- */

const type = check('type');
const small = type.filter((t) => t.belowFloor.length);
if (!small.length) ok('A13', `no figure text renders below the declared ${registry.thresholds.legibility_floor_px}px floor on any of ${type.length} renders`);
else for (const t of small) bad('A13', `${t.render}: ${t.belowFloor.length} nodes below floor, smallest ${t.smallest}px`);

/* ---- A14 non-colour channel --------------------------------------------- */

let missingChannel = 0;
for (const candidate of registry.candidates) {
  for (const state of candidate.states ?? []) {
    if (!state.non_colour_marker) { missingChannel += 1; bad('A14', `${candidate.id} state "${state.id}" declares no non-colour channel`); }
  }
}
if (!missingChannel) ok('A14', 'every declared state on every candidate names a channel that is not colour');

/* ---- deterministic integrity -------------------------------------------- */

const axe = check('axe');
const violations = axe.flatMap((a) => a.violations);
if (!violations.length) ok('axe', `zero WCAG 2.0/2.1/2.2 A and AA violations across ${axe.length} renders`);
else for (const v of violations) bad('axe', `${v.id} (${v.impact}) ×${v.nodes}`);

const network = check('network');
const remote = network.flatMap((r) => r.remote);
if (!remote.length) ok('network', `zero requests outside the study's own loopback origin across ${network.length} renders`);
else bad('network', `${remote.length} remote requests, e.g. ${remote[0]}`);

const consoleRows = check('console');
const errors = consoleRows.flatMap((c) => c.errors);
if (!errors.length) ok('console', `zero console or page errors across ${consoleRows.length} renders`);
else bad('console', `${errors.length} errors, e.g. ${errors[0]}`);

const coverage = check('coverage');
const badMeta = coverage.filter((c) => !c.lang || !c.title || !c.h1);
if (!badMeta.length) ok('coverage', `every render declares a language, a title and an h1 (${coverage.length} renders)`);
else bad('coverage', `${badMeta.length} renders missing lang, title or h1`);

const lifecycle = JSON.parse(read(path.join(ROOT, 'checks/lifecycle.json')));
if (lifecycle.headless && lifecycle.headed_launches === 0 && lifecycle.profile_removed && !lifecycle.operator_browser_touched) {
  ok('lifecycle', `headless only, ${lifecycle.headed_launches} headed launches, task-owned profile removed, operator browser untouched (Chrome ${lifecycle.browser})`);
} else bad('lifecycle', 'the render harness did not prove its own cleanup');

/* ---- checksums ----------------------------------------------------------- */

const inventory = [];
const collect = (dir) => {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) collect(full);
    else inventory.push(path.relative(ROOT, full));
  }
};
for (const dir of ['cases', 'directions', 'portal', 'baselines', 'shared', 'tools', 'renders']) collect(path.join(ROOT, dir));
for (const file of ['rubric.v4.md', 'rubric.v4.lock.json', 'registry.json', 'registry.lock.json', 'amendments.json', 'dispositions.json', 'board.html']) {
  if (fs.existsSync(path.join(ROOT, file))) inventory.push(file);
}
const lines = inventory.sort().map((rel) => `${sha(path.join(ROOT, rel))}  ${rel}`);
const sumsPath = path.join(ROOT, 'SHA256SUMS');
const failures = results.filter((r) => !r.pass);

if (UPDATE) {
  if (failures.length) {
    bad('SHA256SUMS', `refused to refresh while ${failures.length} checks are failing; the recorded inventory is left byte-identical`);
  } else {
    fs.writeFileSync(sumsPath, `${lines.join('\n')}\n`);
    ok('SHA256SUMS', `refreshed over ${lines.length} files`);
  }
} else if (!fs.existsSync(sumsPath)) {
  bad('SHA256SUMS', 'no recorded inventory — run with --update once the board is settled');
} else {
  const recorded = read(sumsPath).trim().split('\n');
  const drift = lines.filter((l, i) => l !== recorded[i]);
  if (recorded.length === lines.length && !drift.length) ok('SHA256SUMS', `${lines.length} files match the recorded inventory`);
  else bad('SHA256SUMS', `${Math.abs(recorded.length - lines.length)} file count difference and ${drift.length} digest differences`);
}

/* ---- report -------------------------------------------------------------- */

const finalFailures = results.filter((r) => !r.pass);
for (const r of results) console.log(`${r.pass ? 'PASS' : 'FAIL'}  ${r.gate.padEnd(14)} ${r.what}`);
console.log(`\n${results.length - finalFailures.length} passed, ${finalFailures.length} failed`);
console.log('\nA clean pass says the study is internally consistent, accessible to the automated checks,');
console.log('reproducible and admissible. It says nothing about whether a composition communicates:');
console.log('G1 to G10 need an observer who authored nothing here and has read no answer.');
process.exit(finalFailures.length ? 1 : 0);

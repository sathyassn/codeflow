// Writes the digest locks for rubric.v5.md and registry.json.
//
// The first draft of this tool recorded "these directories did not exist when I
// pinned", which stopped being true the moment the W4 seed copies were laid
// down. Absence is a weak proof anyway: it expires. So this version proves the
// stronger and more durable thing — that at pin time every file under the
// watched directories was byte-identical to its counterpart on a previous
// board, which is what "no W5 artefact has been authored yet" actually means.
//
// A seed file that stops matching its source is an authored file. Re-running
// this tool after authoring will list it under authored_since_seed, which is the
// point: the lock cannot be re-pinned into a lie without saying what changed.
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const PRIOR = ['../tsk-014-w4', '../tsk-014-w3', '../tsk-014-w2'];
const WATCH = ['cases', 'directions', 'portal', 'baselines', 'renders', 'checks', 'shared', 'rejected'];

const sha = (f) => crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const full = path.join(dir, e.name);
    return e.isDirectory() ? walk(full) : [path.relative(ROOT, full)];
  });
}

/* A content index of every prior board, keyed by digest rather than by path.
 * Path-keyed matching would call a carried-forward W4 render "authored" the
 * moment it is filed under a different name — which is exactly what the
 * rejection records do. The question is whether these bytes are new, not
 * whether they sit where they used to. */
const priorIndex = new Map();
for (const board of PRIOR) {
  const dir = path.resolve(ROOT, board);
  if (!fs.existsSync(dir)) continue;
  for (const rel of walk(dir)) {
    const abs = path.resolve(ROOT, rel);
    const digest = sha(abs);
    if (!priorIndex.has(digest)) {
      priorIndex.set(digest, { board: board.replace('../', ''), at: path.relative(dir, abs) });
    }
  }
}

const files = WATCH.flatMap((d) => walk(path.join(ROOT, d)));
const seeded = {};
const carriedUnderNewName = [];
const authored = [];
for (const rel of files) {
  const hit = priorIndex.get(sha(path.join(ROOT, rel)));
  if (!hit) {
    authored.push(rel);
    continue;
  }
  seeded[hit.board] = (seeded[hit.board] ?? 0) + 1;
  if (hit.at !== rel) carriedUnderNewName.push({ now: rel, was: `${hit.board}/${hit.at}` });
}

function lock(name, extra) {
  const file = path.join(ROOT, name);
  return {
    file: name,
    sha256: sha(file),
    bytes: fs.statSync(file).size,
    mtime_iso: fs.statSync(file).mtime.toISOString(),
    pinned_at_state:
      authored.length === 0
        ? 'no W5 artefact had been authored. Every file under the watched directories was byte-identical to its counterpart on a previous board — a starting point, not authorship.'
        : `${authored.length} file(s) under the watched directories no longer match any previous board. This lock was written AFTER authoring began and is not pre-authorship evidence.`,
    seed_files_byte_identical_to: seeded,
    seed_files_carried_under_a_new_name: carriedUnderNewName,
    authored_since_seed: authored,
    what_this_proves:
      'the bytes, and the seed state. The digests survive a checkout, a copy and a commit; the mtime does not, and is recorded because it is the only ordering witness that will not.',
    what_this_does_not_prove:
      'that the author intended this before drawing. Only that these bytes preceded every authored artefact. A later change to the pinned file is a lock failure, not a silent edit.',
    ...extra,
  };
}

fs.writeFileSync(
  path.join(ROOT, 'rubric.v5.lock.json'),
  `${JSON.stringify(
    lock('rubric.v5.md', {
      governs: 'the W5 board',
      supersedes_nothing:
        'rubric.md, rubric.v2.md, rubric.v3.md and rubric.v4.md stay byte-identical under their own locks in ../tsk-014-w2/, ../tsk-014-w3/ and ../tsk-014-w4/',
    }),
    null,
    2,
  )}\n`,
);

fs.writeFileSync(
  path.join(ROOT, 'registry.lock.json'),
  `${JSON.stringify(
    lock('registry.json', {
      governs: 'every declaration the W5 gates check against',
      fixes:
        'W3 registry.json carried no lock at all, and its inode postdated seven of eleven candidate pages. The W5 first draft carried a lock whose stated proof (directory absence) had already expired when it was written.',
      re_pinned:
        'This is a corrected re-pin. The first W5 draft of the pinned files misstated the observer evidence and recorded planned repairs as completed outcomes; it was corrected before any W5 artefact was authored. See registry.json correction_history.',
    }),
    null,
    2,
  )}\n`,
);

console.log(
  `pinned. seed files: ${JSON.stringify(seeded)}; authored since seed: ${authored.length}${
    authored.length ? `\n  ${authored.join('\n  ')}` : ''
  }`,
);

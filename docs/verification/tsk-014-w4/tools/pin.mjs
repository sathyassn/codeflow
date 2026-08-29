// Writes the digest locks for rubric.v4.md and registry.json.
// Run once, before any artefact exists; re-running after authoring records the
// same digests only if the pinned files were never touched.
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const WATCH = ['cases', 'directions', 'portal', 'baselines', 'renders', 'checks', 'shared'];

const sha = (f) => crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const absent = WATCH.filter((d) => !fs.existsSync(path.join(ROOT, d)));

function lock(name, extra) {
  const file = path.join(ROOT, name);
  return {
    file: name,
    sha256: sha(file),
    bytes: fs.statSync(file).size,
    mtime_iso: fs.statSync(file).mtime.toISOString(),
    pinned_at_state:
      'the only files in this directory were rubric.v4.md, registry.json and tools/pin.mjs; no authored candidate, direction demonstration, portal page, baseline or render existed',
    directories_absent_when_pinned: absent,
    what_this_proves:
      'the bytes. The digest survives a checkout, a copy and a commit; the mtime does not, and is recorded here because this lock is the only witness that will survive one.',
    what_this_does_not_prove:
      'that the author intended this before drawing. Only that these bytes preceded every artefact. A later change to the pinned file is a lock failure, not a silent edit.',
    ...extra,
  };
}

fs.writeFileSync(
  path.join(ROOT, 'rubric.v4.lock.json'),
  `${JSON.stringify(
    lock('rubric.v4.md', {
      governs: 'the W4 board',
      supersedes_nothing:
        'rubric.v2.md and rubric.v3.md stay byte-identical under their own locks in ../tsk-014-w3/',
    }),
    null,
    2,
  )}\n`,
);

fs.writeFileSync(
  path.join(ROOT, 'registry.lock.json'),
  `${JSON.stringify(
    lock('registry.json', {
      governs: 'every declaration the W4 gates check against',
      fixes:
        'W3 registry.json carried no lock at all, and its inode postdated seven of eleven candidate pages',
    }),
    null,
    2,
  )}\n`,
);

console.log('pinned. absent when pinned:', absent.join(', ') || '(none)');

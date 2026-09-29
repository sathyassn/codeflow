import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, copyFile, writeFile, readFile, rm, access } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';

async function fixture(action) {
  const root = await mkdtemp(join(tmpdir(), 'portal-verify-test-'));
  try {
    await mkdir(join(root, 'scripts'));
    await mkdir(join(root, 'node_modules/astro/bin'), { recursive: true });
    for (const file of ['workflow.mjs', 'publication.mjs', 'child-lifecycle.mjs', 'process-environment.mjs']) await copyFile(resolve('docs-portal/scripts', file), join(root, 'scripts', file));
    await writeFile(join(root, 'scripts/lib.mjs'), `export {compareDeterministicText, portablePathKey, safeRelative} from ${JSON.stringify(pathToFileURL(resolve('docs-portal/scripts/lib.mjs')).href)};`);
    const record = `import {readFileSync,appendFileSync} from 'node:fs';\nconst owner=JSON.parse(readFileSync('.portal/workflow.lock'));\nappendFileSync('sequence.jsonl',JSON.stringify({stage:STAGE,token:owner.token})+'\\n');\n`;
    for (const stage of ['adapter', 'evidence']) await writeFile(join(root, `scripts/${stage}.mjs`), record.replace('STAGE', JSON.stringify(stage)));
    await writeFile(join(root, 'node_modules/astro/bin/astro.mjs'), record.replace('STAGE', 'process.argv[2]') + "if(process.argv[2]==='build' && requireFailure()) process.exit(1); function requireFailure(){try{readFileSync('fail-build');return true;}catch{return false;}}\n");
    await action(root);
  } finally { await rm(root, { recursive: true, force: true }); }
}

test('verify runs adapter once and check build evidence under one lease', async () => {
  await fixture(async root => {
    const result = spawnSync(process.execPath, ['scripts/workflow.mjs', 'verify'], { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const rows = (await readFile(join(root, 'sequence.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
    assert.deepEqual(rows.map(r => r.stage), ['adapter', 'check', 'build', 'evidence']);
    assert.equal(new Set(rows.map(r => r.token)).size, 1);
    await assert.rejects(access(join(root, '.portal/workflow.lock')));
  });
});

test('verify stops on build failure and releases its lease', async () => {
  await fixture(async root => {
    await writeFile(join(root, 'fail-build'), 'fail');
    const result = spawnSync(process.execPath, ['scripts/workflow.mjs', 'verify'], { cwd: root, encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    const rows = (await readFile(join(root, 'sequence.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
    assert.deepEqual(rows.map(r => r.stage), ['adapter', 'check', 'build']);
    await assert.rejects(access(join(root, '.portal/workflow.lock')));
  });
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, copyFile, rm, readFile, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const script = resolve('scripts/check-present-binary-delta.mjs');
async function fixture(action) {
  const root = await mkdtemp(join(tmpdir(), 'cf-binary-delta-test-'));
  try {
    for (const path of ['scripts', 'crates/codeflow-present/src', 'crates/codeflow-present/assets/service', 'crates/codeflow-present/assets/export', 'crates/codeflow-cli/src', 'assets', 'support']) await mkdir(join(root, path), { recursive: true });
    await copyFile(script, join(root, 'scripts/check-present-binary-delta.mjs'));
    await writeFile(join(root, 'crates/codeflow-present/src/limits.rs'), 'pub const MAX_SERVICE_BINARY_DELTA_BYTES: u64 = 9_999_999;\npub const MAX_COMBINED_BINARY_DELTA_BYTES: u64 = 9_999_999;\n');
    await writeFile(join(root, 'crates/codeflow-present/assets/manifest.json'), '{}');
    for (const dir of ['service', 'export']) await writeFile(join(root, `crates/codeflow-present/assets/${dir}/payload`), 'payload');
    await writeFile(join(root, 'support/included.txt'), 'included from outside crates\n');
    await writeFile(join(root, 'Cargo.toml'), '[workspace]\nmembers = ["crates/codeflow-cli", "crates/codeflow-present"]\nresolver = "2"\n');
    await writeFile(join(root, 'crates/codeflow-cli/Cargo.toml'), '[package]\nname="codeflow-cli"\nversion="0.1.0"\nedition="2021"\n[[bin]]\nname="codeflow"\npath="src/main.rs"\n');
    await writeFile(join(root, 'crates/codeflow-cli/src/main.rs'), 'fn main() { println!("{} {}", include_str!("../../../support/included.txt"), env!("CODEFLOW_SOURCE_REVISION")); }');
    await writeFile(join(root, 'crates/codeflow-present/Cargo.toml'), '[package]\nname="codeflow-present"\nversion="0.1.0"\nedition="2021"\n');
    await writeFile(join(root, 'crates/codeflow-present/src/lib.rs'), 'pub mod limits;');
    await writeFile(join(root, 'Cargo.lock'), 'version = 4\n[[package]]\nname = "codeflow-cli"\nversion = "0.1.0"\n[[package]]\nname = "codeflow-present"\nversion = "0.1.0"\n');
    const git = (...args) => execFileSync('git', args, { cwd: root, env: { ...process.env, GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.com', GIT_COMMITTER_NAME: 'Test', GIT_COMMITTER_EMAIL: 'test@example.com' } });
    git('init', '-q'); git('add', '.'); git('commit', '-qm', 'chore: fixture');
    await action(root, git);
  } finally { await rm(root, { recursive: true, force: true }); }
}

test('archive includes source outside crates and passes revision to repeat builds', async () => {
  await fixture(async (root, git) => {
    const revision = git('rev-parse', 'HEAD').toString().trim();
    const done = spawnSync(process.execPath, ['scripts/check-present-binary-delta.mjs', '--with-repeat'], { cwd: root, encoding: 'utf8', timeout: 120_000 });
    assert.equal(done.status, 0, done.stderr);
    const evidence = JSON.parse(done.stdout);
    assert.equal(evidence.source_revision, revision);
    assert.equal(evidence.exact_repeat, 'pass');
    const target = evidence.target_directory;
    assert.ok(target.startsWith(join(await realpath(root), 'target/binary-delta-')));
    assert.match(execFileSync(join(target, 'release', process.platform === 'win32' ? 'codeflow.exe' : 'codeflow'), { encoding: 'utf8' }), new RegExp(revision));
  });
});

test('an absent included source fails the archive build', async () => {
  await fixture(async (root, git) => {
    git('rm', 'support/included.txt'); git('commit', '-qm', 'test: absent include');
    const done = spawnSync(process.execPath, ['scripts/check-present-binary-delta.mjs'], { cwd: root, encoding: 'utf8', timeout: 120_000 });
    assert.notEqual(done.status, 0);
    assert.match(done.stderr, /included.txt/);
  });
});

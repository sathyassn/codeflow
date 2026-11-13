// TSK-443: the portal build and present check enforce the same figure faults.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { configureFixture, commitFixture, selfContainedPortalFixture, starterRoot } from './portal-fixture.mjs';
import { specimen } from './page-shapes.mjs';

test('portal build and present check refuse the same fact-kind and width faults', { timeout: 300_000 }, async () => {
  const binary = process.env.CF_PRESENT_CODEFLOW;
  assert.ok(binary, 'CF_PRESENT_CODEFLOW must name the worktree binary for the cross-surface check');
  const root = await selfContainedPortalFixture();
  const evidence = process.env.CF_PARITY_EVIDENCE;
  if (evidence) await mkdir(evidence, { recursive: true });
  try {
    // Use the installed Astro package; its dependencies resolve in starterRoot.
    await cp(path.join(starterRoot, 'node_modules/astro'), path.join(root, 'node_modules/astro'), { recursive: true });
    await mkdir(path.join(root, 'figures'));
    await writeFile(path.join(root, 'facts.json'), JSON.stringify({ step: 'review' }));
    await writeFile(path.join(root, 'docs/seed.md'), '# Review handoff\n\nThe author sends a review request.\n');
    await configureFixture(root, { page_classes: [{ source: 'docs/seed.md', class: 'illustrated' }], figures: [{ declaration: 'figures/review.json', route: 'reference/seed' }] });
    for (const [family, filename, message] of [
      ['sequence', '04-sequence.json', 'fact 1: a JSON source takes the json check'],
      ['coverage', '06-coverage.json', 'wide.width must be 120 to 720 units'],
    ]) {
      const good = await specimen(filename);
      good.figure.facts = [{ claim: 'Review is required', source: 'facts.json', derive: 'Read the required step', check: { kind: 'json', select: 'step' }, value: 'review' }];
      for (const invalid of [true, false]) {
        const declaration = structuredClone(good);
        if (invalid && family === 'sequence') {
          declaration.figure.facts[0].check = { kind: 'contains', text: 'review' };
          declaration.figure.facts[0].value = true;
        }
        if (invalid && family === 'coverage') declaration.figure.wide.width = 760;
        await writeFile(path.join(root, 'figures/review.json'), JSON.stringify(declaration));
        const input = path.join(root, 'present.json');
        await writeFile(input, JSON.stringify({ schema_version: 2, title: 'Review handoff', blocks: [{ type: 'figure', id: 'review', declaration }] }));
        commitFixture(root, `${family} ${invalid ? 'refusal' : 'control'}`);
        const present = spawnSync(binary, ['present', 'check', '--file', input], { cwd: root, encoding: 'utf8', timeout: 30_000 });
        const portal = spawnSync(process.execPath, ['scripts/workflow.mjs', 'build'], { cwd: root, encoding: 'utf8', timeout: 110_000 });
        const name = `${family}-${invalid ? 'invalid' : 'valid'}`;
        for (const [surface, result] of [['present', present], ['portal', portal]]) {
          const output = `${result.stdout}\n${result.stderr}`;
          if (evidence) await writeFile(path.join(evidence, `${name}-${surface}.log`), output);
          assert.equal(result.error, undefined, `${name} ${surface}: ${result.error}`);
          if (invalid) {
            assert.notEqual(result.status, 0, `${name} ${surface} must fail`);
            assert.ok(output.includes(message), `${name} ${surface}: ${output}`);
          } else assert.equal(result.status, 0, `${name} ${surface}: ${output}`);
        }
      }
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

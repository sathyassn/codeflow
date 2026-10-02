#!/usr/bin/env python3
"""Targeted structural checks for the candidate gate and CI trust boundaries."""
import json
import os
import re
from pathlib import Path
import unittest

ROOT = Path(os.environ.get('CODEFLOW_GATE_CONTRACT_ROOT', Path(__file__).resolve().parents[1]))
def read(path):
    return (ROOT / path).read_text()

class GateContracts(unittest.TestCase):
    def test_publication_requires_the_five_existing_floors(self):
        checks = json.loads(read('.release/config.json'))['required_publication_checks']
        self.assertEqual(set(checks), {'codeflow gates', 'windows (build + test + clippy)', 'secret scan', 'security review', 'release state'})
        self.assertEqual(len(checks), 5)

    def test_windows_job_is_a_gate(self):
        # 3.0.1 restores native Windows (TSK-197): a Windows failure fails
        # the workflow run, and publication requires the job by its name.
        workflow = read('.github/workflows/codeflow-ci.yml')
        job = re.split(r'\n  [A-Za-z0-9_-]+:\n', workflow.split('\n  windows:\n', 1)[1], maxsplit=1)[0]
        self.assertIn('journey-gate.py', job)
        self.assertIn('runs-on: windows-latest', job)
        self.assertNotIn('continue-on-error', job)
        name = job.split('name: ', 1)[1].split('\n', 1)[0]
        checks = json.loads(read('.release/config.json'))['required_publication_checks']
        self.assertIn(name, checks)

    def test_one_target_source_build_judges_policy_and_registry_without_cancellation(self):
        workflow = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('pull_request_target:', workflow)
        self.assertIn('github.event.pull_request.base.sha', workflow)
        self.assertEqual(workflow.count('cargo build --release'), 1)
        self.assertIn('codeflow ids check --base', workflow)
        self.assertIn('codeflow ci --base', workflow)
        self.assertNotIn('concurrency:', workflow, 'registry runs must not replace queued events')
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertNotIn('github.event.pull_request.head.sha }}\n          fetch-depth', workflow)
        self.assertFalse((ROOT / '.github/workflows/codeflow-registry.yml').exists())

    def test_release_reuses_cache_and_only_cancels_pr_events(self):
        workflow = read('.github/workflows/codeflow-release.yml')
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", workflow)
        self.assertIn("github.event_name == 'pull_request' && github.ref || github.run_id", workflow)
        self.assertIn('cargo build --release --locked -p codeflow-cli', workflow)

    def test_release_fixture_bodies_name_a_task(self):
        source = read('scripts/fixtures/release_impact_cases.json')
        self.assertNotIn('Task: none', source)
        self.assertIn('Task: TSK-', source)

    def test_pipeline_verifies_targeted_and_quick_and_names_candidate_owner(self):
        for path in ['assets/base/claude/workflows/pipeline.workflow.js', '.claude/workflows/pipeline.workflow.js']:
            source = read(path)
            self.assertIn('affected targeted checks', source)
            self.assertIn('codeflow test --mode quick --strict', source)
            self.assertIn('primary runs the full gate once on the exact landing candidate', source)
        self.assertEqual(read('assets/base/claude/workflows/pipeline.workflow.js'), read('.claude/workflows/pipeline.workflow.js'))

    def test_browser_targets_keep_each_unique_proof_once(self):
        first = read('crates/codeflow-present/web/scripts/browser-check.mjs')
        real = read('crates/codeflow-present/web/scripts/real-browser-check.mjs')
        cleanup = read('crates/codeflow-present/web/scripts/real-browser-cleanup-check.mjs')
        self.assertTrue('globalThis.axe.run' in first, 'fixture browser pass keeps shared-chrome WCAG')
        self.assertEqual(real.count('globalThis.axe.run'), 1, 'real pass checks the actual exported artifact once')
        for proof in ['cleanup', 'feedback', 'profile']:
            self.assertTrue(proof in real, proof)
        self.assertIn('cleanup', cleanup)
        self.assertIn('check:real-browser-cleanup', read('.codeflow/test-config.json'))

    def test_own_config_publishes_the_producer_graph_and_quality_floors(self):
        cfg = json.loads(read('.codeflow/test-config.json'))
        targets = {t['name']: t for t in cfg['targets']}
        self.assertTrue(cfg['execution']['parallel'])
        self.assertGreater(cfg['execution']['max_parallel'], 0)
        def closure(name):
            found = set()
            def visit(node):
                if node in found: return
                found.add(node)
                for dep in targets[node].get('requires', []): visit(dep)
            visit(name)
            return found
        for name in ['rust-build', 'codeflow-bin', 'rust-coverage', 'rust-doctest', 'rustdoc', 'docs-validation', 'docs-portal', 'journey-gate', 'cf-present-qualification', 'present-browser', 'read-benchmark']:
            self.assertIn('present-web-build', closure(name), name)
        # The quick gate stays light: no quick target pulls in a build producer
        # (fmt and clippy need no web build; there is no present build.rs).
        quick = {name for name, t in targets.items() if 'quick' in t['modes']}
        self.assertEqual(quick, {'rust-format', 'rust-clippy', 'gate-parity', 'skill-triggers', 'herdr-delivery'})
        for name in quick:
            self.assertLessEqual(closure(name), quick, name)
        for name in ['present-browser', 'read-benchmark', 'present-web-build']:
            self.assertTrue(targets[name]['exclusive'], name)
        self.assertEqual(targets['journey-gate']['requires'], ['rust-coverage'])
        self.assertIn('--fail-under-lines 90', targets['rust-coverage']['modes']['full']['command'])
        self.assertIn('nextest', targets['rust-coverage']['modes']['full']['command'])
        self.assertNotIn('full', targets['rust-workspace']['modes'])
        self.assertEqual(targets['rust-doctest']['modes']['full']['command'], 'cargo test --workspace --doc')
        self.assertIn('npm run supply-chain', targets['present-supply-chain']['modes']['full']['command'])
        self.assertIn('check-present-binary-delta', targets['present-binary-delta']['modes']['full']['command'])
        self.assertIn('structural', cfg['_description'])

if __name__ == '__main__':
    unittest.main()

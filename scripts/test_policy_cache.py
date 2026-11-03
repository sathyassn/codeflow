#!/usr/bin/env python3
"""Check the pinned main-only cache proposal, not the withdrawn dispatcher."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / 'operator-patches/TSK-1240-codeflow-policy.proposed.yml'
PROPOSED_SHA256 = '97b8d65e39038077d57fb00e09c870fc790440b256273825e0328040fc341066'
NAMESPACE = 'codeflow-policy-main-binary-v2-'
DEPS_NAMESPACE = 'codeflow-policy-main-deps-v2'
MAIN_PUSH = "github.event_name == 'push' && github.ref == 'refs/heads/main'"


def proposed_workflow():
    data = WORKFLOW.read_bytes()
    assert hashlib.sha256(data).hexdigest() == PROPOSED_SHA256, 'proposed workflow drift'
    return data.decode()


def step(workflow, name):
    marker = f'      - name: {name}\n'
    assert marker in workflow, f'missing cache step: {name}'
    return workflow.split(marker, 1)[1].split('\n      - ', 1)[0]


def script(body):
    run = body.split('        run: |\n', 1)[1]
    return '\n'.join(line[10:] for line in run.splitlines() if line.startswith('          '))


def cache_contract(workflow):
    assert workflow.split('\npermissions:\n', 1)[1].split('\nconcurrency:', 1)[0] == '  contents: read\n'
    assert ': write' not in workflow
    for withdrawn in ('warm-target-cache:', 'workflow_dispatch:', 'build_sha:', 'dispatch-target'):
        assert withdrawn not in workflow
    key = step(workflow, 'Identify the target build')
    assert "ref: ${{ github.event_name == 'pull_request_target' && github.event.pull_request.base.sha || '' }}" in workflow
    assert workflow.count('uses: actions/checkout@') == 1
    assert 'git checkout' not in workflow
    assert 'sha=$(git rev-parse HEAD)' in key
    assert 'git diff --quiet' in key and 'git diff --cached --quiet' in key
    assert 'rustc -vV' in key and 'cargo -V' in key
    assert 'ImageVersion' in key and 'RUNNER_ARCH' in key
    assert 'pull_request.head' not in key and 'github.sha' not in key
    assert 'push:main|pull_request_target:main|schedule:main) allowed=true ;;' in key
    assert key.count('allowed=true') == 1
    assert 'integration/' not in key and 'master' not in key and 'workflow_dispatch' not in key
    assert f'key={NAMESPACE}$sha-$identity' in key
    restore = step(workflow, 'Restore the target binary')
    save = step(workflow, 'Save the verified target binary')
    assert "if: steps.target-build.outputs.allowed == 'true'" in restore
    assert 'actions/cache/restore@' in restore
    assert 'restore-keys:' not in workflow
    assert 'lookup-only: ${{ ' + MAIN_PUSH + ' }}' in restore
    assert 'key: ${{ steps.target-build.outputs.key }}' in restore
    assert "if: " + MAIN_PUSH + " && steps.target-build.outputs.allowed == 'true' && steps.target-binary.outputs.built == 'true' && steps.target-cache.outputs.cache-hit != 'true'" in save
    assert 'actions/cache/save@' in save
    assert 'key: ${{ steps.target-build.outputs.key }}' in save
    assert workflow.count('actions/cache/save@') == 1
    assert workflow.index('Verify or rebuild the target binary') < workflow.index('Save the verified target binary') < workflow.index('Fetch the pull request head as data')
    deps = workflow.split('      - uses: Swatinem/rust-cache@', 1)[1].split('\n      - ', 1)[0]
    assert 'prefix-key: ' + DEPS_NAMESPACE in deps
    assert 'save-if: ${{ ' + MAIN_PUSH + ' }}' in deps
    verify = step(workflow, 'Verify or rebuild the target binary')
    assert 'CACHE_READ: ${{ github.event_name != \'push\' }}' in verify
    assert '[ "$CACHE_READ" != true ] || [ "$CACHE_HIT" != true ] || ! matches_target' in verify
    assert 'cargo build --release --locked -p codeflow-cli' in verify
    assert 'cache-mode:' not in workflow


class PolicyCache(unittest.TestCase):
    def setUp(self):
        self.workflow = proposed_workflow()

    def test_pinned_proposal_matches_operator_patch(self):
        patch = (ROOT / 'operator-patches/TSK-1240-codeflow-policy.patch').read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / '.github/workflows/codeflow-policy.yml'
            target.parent.mkdir(parents=True)
            target.write_bytes(subprocess.check_output(
                ['git', '-C', str(ROOT), 'show',
                 '59c9ae96547aaf8241e0c66029c809b80d748f51:.github/workflows/codeflow-policy.yml']))
            subprocess.run(['git', 'apply', '--check', '-'], input=patch, cwd=root, check=True)
            subprocess.run(['git', 'apply', '-'], input=patch, cwd=root, check=True)
            self.assertEqual(target.read_text(), self.workflow)
        actual = (ROOT / '.github/workflows/codeflow-policy.yml').read_bytes()
        baseline = subprocess.check_output(['git', '-C', str(ROOT), 'show',
            '59c9ae96547aaf8241e0c66029c809b80d748f51:.github/workflows/codeflow-policy.yml'])
        self.assertIn(actual, (baseline, self.workflow.encode()),
                      'real workflow must be the pinned line or identical operator-applied proposal')

    def test_cache_authority(self):
        cache_contract(self.workflow)

    def test_sibling_workflows_cannot_use_policy_namespaces(self):
        for folder in (ROOT / '.github/workflows', ROOT / 'assets/base/ci'):
            for path in folder.rglob('*'):
                if path.suffix not in ('.yml', '.yaml') or path == ROOT / '.github/workflows/codeflow-policy.yml':
                    continue
                for namespace in (NAMESPACE, DEPS_NAMESPACE):
                    self.assertNotIn(namespace, path.read_text(), str(path))

    def test_head_integration_dispatch_and_extra_permissions_are_rejected(self):
        mutations = (
            (MAIN_PUSH, "github.event_name == 'push'"),
            (MAIN_PUSH, "github.event_name == 'pull_request'"),
            ('push:main|', 'push:main|push:integration/*|'),
            ('push:main|', 'push:main|push:master|'),
            ('push:main|', 'push:main|workflow_dispatch:main|'),
            ('sha=$(git rev-parse HEAD)', 'sha=${{ github.event.pull_request.head.sha }}'),
            ('key: ${{ steps.target-build.outputs.key }}', 'key: ${{ github.sha }}'),
            ('actions/cache/restore@', 'actions/cache@'),
            ("if: steps.target-build.outputs.allowed == 'true'", "if: always()"),
            ('permissions:\n  contents: read', 'permissions:\n  contents: write'),
            ('      contents: read', '      contents: read\n      pull-requests: write'),
            ('\njobs:\n', '\njobs:\n  warm-target-cache:\n    permissions:\n      contents: write\n'),
            ('lookup-only: ${{ ' + MAIN_PUSH + ' }}', 'lookup-only: false'),
            ('prefix-key: ' + DEPS_NAMESPACE, 'prefix-key: shared'),
        )
        for before, after in mutations:
            with self.subTest(mutation=after):
                self.assertIn(before, self.workflow)
                with self.assertRaises(AssertionError):
                    cache_contract(self.workflow.replace(before, after))

    @unittest.skipIf(os.name == "nt", "the target-cache script runs in Linux bash")
    def test_identity_mismatch_miss_and_exact_hit(self):
        run = script(step(self.workflow, 'Verify or rebuild the target binary'))
        sha = 'a' * 40
        for hit, cached, dirty, rebuilt, read_cache in (
            ('true', sha, 'false', False, 'true'), ('true', 'b' * 40, 'false', True, 'true'),
            ('false', sha, 'false', True, 'true'), ('', None, 'false', True, 'true'),
            ('true', sha, 'false', True, 'false'),
            ('true', 'malformed', 'false', True, 'true'), ('true', sha, 'true', True, 'true'),
        ):
            with self.subTest(hit=hit, cached=cached), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / 'cache' / 'codeflow'
                binary.parent.mkdir()
                if cached:
                    binary.write_text(f'#!/bin/sh\necho "codeflow 4.0.0 source={cached} dirty={dirty} inputs=abc"\n')
                    binary.chmod(0o755)
                cargo = root / 'cargo'
                cargo.write_text('#!/bin/sh\nmkdir -p target/release\nprintf build >> built\n'
                                 f'printf \'#!/bin/sh\\necho "codeflow 4.0.0 source={sha} dirty=false inputs=abc"\\n\' > target/release/codeflow\n'
                                 'chmod +x target/release/codeflow\n')
                cargo.chmod(0o755)
                output = root / 'output'
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           TARGET_SHA=sha, CACHE_HIT=hit, CACHE_READ=read_cache, BINARY_DIR=str(binary.parent),
                           GITHUB_OUTPUT=str(output), GITHUB_PATH=str(root / 'path'))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=root,
                                        env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((root / 'built').exists(), rebuilt)
                self.assertIn(f'built={str(rebuilt).lower()}', output.read_text())
                self.assertIn(f'source={sha}', subprocess.check_output([str(binary), '--version'], text=True))

    @unittest.skipIf(os.name == "nt", "the target-cache script runs in Linux bash")
    def test_a_rebuild_with_wrong_identity_fails_closed(self):
        run = script(step(self.workflow, 'Verify or rebuild the target binary'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / 'cargo'
            cargo.write_text('#!/bin/sh\nmkdir -p target/release\n'
                             'printf \'#!/bin/sh\\necho "codeflow 4.0.0 source=wrong dirty=false inputs=abc"\\n\' > target/release/codeflow\n'
                             'chmod +x target/release/codeflow\n')
            cargo.chmod(0o755)
            output = root / 'output'
            path = root / 'path'
            env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                       TARGET_SHA='a' * 40, CACHE_HIT='', CACHE_READ='true', BINARY_DIR=str(root / 'cache'),
                       GITHUB_OUTPUT=str(output), GITHUB_PATH=str(path))
            result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=root,
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('::error::target binary source identity does not match', result.stdout)
            self.assertFalse(output.exists(), 'a failed identity check must not authorize a save')
            self.assertFalse(path.exists(), 'a failed binary must not enter PATH')

    @unittest.skipIf(os.name == "nt", "the target-cache script runs in Linux bash")
    def test_key_changes_with_target_toolchain_and_image(self):
        run = script(step(self.workflow, 'Identify the target build'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, content in {'git': 'if [ "$1" = rev-parse ]; then echo "$TEST_SHA"; '
                                  'elif [ "$1" = diff ]; then '
                                  'if [ "$TEST_DIRTY" = unstaged ] && [ "$2" = --quiet ]; then exit 1; fi; '
                                  'if [ "$TEST_DIRTY" = staged ] && [ "$2" = --cached ]; then exit 1; fi; fi',
                                  'rustc': 'echo "$TEST_RUST"', 'cargo': 'echo cargo',
                                  'cc': 'echo compiler'}.items():
                path = root / name
                path.write_text('#!/bin/sh\n' + content + '\n')
                path.chmod(0o755)
            def key(sha='a' * 40, rust='rustc 1.94.0', image='20261102', event='pull_request_target', ref='main', dirty=''):
                output = root / 'output'
                output.write_text('')
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           TEST_SHA=sha, TEST_RUST=rust, TEST_DIRTY=dirty, ImageOS='ubuntu24', ImageVersion=image,
                           RUNNER_ARCH='X64', EVENT=event, TARGET_REF=ref, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], env=env,
                                        capture_output=True, text=True)
                if dirty:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(output.read_text(), '', 'a dirty checkout must not emit a cache key')
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                return output.read_text()
            key(dirty='unstaged')
            key(dirty='staged')
            original = key()
            self.assertIn('allowed=true', original)
            self.assertNotEqual(original, key(sha='b' * 40))
            self.assertNotEqual(original, key(rust='rustc 1.95.0'))
            self.assertNotEqual(original, key(image='20261103'))
            for event, ref in [('pull_request', 'main'), ('workflow_dispatch', 'main'),
                               ('push', 'integration/EPC-022'), ('pull_request_target', 'integration/EPC-022'),
                               ('push', 'master'), ('push', 'task/head'), ('pull_request_target', 'task/head')]:
                self.assertIn('allowed=false', key(event=event, ref=ref))


if __name__ == '__main__':
    unittest.main()

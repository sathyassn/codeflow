#!/usr/bin/env python3
"""Exercise target binary reuse and pin the workflow's cache authority."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / '.github/workflows/codeflow-policy.yml'


def step(workflow, name):
    marker = f'      - name: {name}\n'
    assert marker in workflow, f'missing cache step: {name}'
    return workflow.split(marker, 1)[1].split('\n      - ', 1)[0]


def script(body):
    run = body.split('        run: |\n', 1)[1]
    return '\n'.join(line[10:] for line in run.splitlines() if line.startswith('          '))


def cache_contract(workflow):
    """Assertions deliberately reject changes that give PR heads cache authority."""
    key = step(workflow, 'Identify the target build')
    assert "ref: ${{ github.event_name == 'pull_request_target' && github.event.pull_request.base.sha || '' }}" in workflow
    assert 'sha=$(git rev-parse HEAD)' in key
    assert 'git diff --quiet' in key
    assert 'rustc -vV' in key and 'cargo -V' in key
    assert 'ImageVersion' in key and 'RUNNER_ARCH' in key
    assert 'pull_request.head' not in key
    assert workflow.count('uses: actions/checkout@') == 1
    assert 'github.sha' not in key
    restore = step(workflow, 'Restore the target binary')
    save = step(workflow, 'Save the verified target binary')
    assert "if: steps.target-build.outputs.allowed == 'true'" in restore
    assert 'actions/cache/restore@' in restore
    assert 'restore-keys:' not in restore
    assert "lookup-only: ${{ github.event_name == 'workflow_dispatch' }}" in restore
    assert workflow.index('Verify the dispatched target') < workflow.index('Install Rust (rust-toolchain.toml)')
    assert "if: github.event_name != 'workflow_dispatch' || steps.target-cache.outputs.cache-hit != 'true'" in step(workflow, 'Verify or rebuild the target binary')
    assert 'key: ${{ steps.target-build.outputs.key }}' in restore
    assert "if: (github.event_name == 'push' || (github.event_name == 'workflow_dispatch' && steps.dispatch-target.outputs.verified == 'true')) && steps.target-build.outputs.allowed == 'true' && steps.target-binary.outputs.built == 'true' && steps.target-cache.outputs.cache-hit != 'true'" in save
    assert 'actions/cache/save@' in save
    assert 'key: ${{ steps.target-build.outputs.key }}' in save
    assert workflow.index('Verify or rebuild the target binary') < workflow.index('Save the verified target binary') < workflow.index('Fetch the pull request head as data')
    assert workflow.count('actions/cache/save@') == 1
    assert "save-if: ${{ github.event_name == 'push' && steps.target-build.outputs.allowed == 'true' }}" in workflow
    assert 'cache-mode: write' not in workflow


class PolicyCache(unittest.TestCase):
    def setUp(self):
        self.workflow = WORKFLOW.read_text()


    def test_only_integration_pushes_can_dispatch_to_main(self):
        job = self.workflow.split('  warm-target-cache:\n', 1)[1].split('  commit-lint:\n', 1)[0]
        self.assertIn("if: github.event_name == 'push' && startsWith(github.ref, 'refs/heads/integration/')", job)
        self.assertIn('      actions: write\n', job)
        self.assertEqual(self.workflow.count('actions: write'), 1)
        self.assertIn('BUILD_SHA: ${{ github.sha }}', job)
        self.assertIn('gh workflow run codeflow-policy.yml --repo "$GH_REPO" --ref main -f "build_sha=$BUILD_SHA"', job)
        self.assertNotIn('uses: actions/checkout', job)
        self.assertNotIn('pull_request.head', job)

    @unittest.skipIf(os.name == "nt", "the target-cache script runs in Linux bash")
    def test_dispatch_requires_a_fetched_trusted_branch_containing_the_sha(self):
        run = script(step(self.workflow, 'Verify the dispatched target'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            remote = root / 'remote'
            checkout = root / 'checkout'
            def git(*args, cwd=checkout):
                return subprocess.check_output(['git', *args], cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()
            remote.mkdir()
            git('init', '-b', 'main', cwd=remote)
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '--allow-empty', '-m', 'chore: initial', cwd=remote)
            main = git('rev-parse', 'HEAD', cwd=remote)
            git('checkout', '-b', 'integration/fixture', cwd=remote)
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '--allow-empty', '-m', 'chore: trusted target', cwd=remote)
            target = git('rev-parse', 'HEAD', cwd=remote)
            git('checkout', '-b', 'task/untrusted', cwd=remote)
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '--allow-empty', '-m', 'chore: untrusted head', cwd=remote)
            head = git('rev-parse', 'HEAD', cwd=remote)
            checkout.mkdir()
            git('init', '-b', 'main')
            git('remote', 'add', 'origin', str(remote))
            def verify(sha, workflow_ref='refs/heads/main'):
                output = root / 'output'
                output.write_text('')
                env = dict(os.environ, BUILD_SHA=sha, WORKFLOW_REF=workflow_ref, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=checkout,
                                        env=env, text=True, capture_output=True)
                return result, output.read_text()
            for sha in (main, target):
                result, output = verify(sha)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(git('rev-parse', 'HEAD'), sha)
                self.assertIn('verified=true', output)
            for sha, ref in ((head, 'refs/heads/main'), ('0' * 40, 'refs/heads/main'),
                             ('main', 'refs/heads/main'), ('$(touch injected)', 'refs/heads/main'),
                             (target, 'refs/heads/task/head')):
                result, output = verify(sha, ref)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('verified=true', output)
            # An obsolete remote-tracking branch must not authorize a writer.
            git('branch', '-D', 'integration/fixture', cwd=remote)
            result, output = verify(target)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('verified=true', output)
            self.assertFalse((checkout / 'injected').exists())
            # Failure to refresh the authority must fail closed, even for main.
            git('remote', 'set-url', 'origin', str(root / 'absent'))
            result, output = verify(main)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('verified=true', output)

    def test_cache_authority(self):
        cache_contract(self.workflow)

    def test_head_written_or_head_selected_cache_is_rejected(self):
        cache_contract(self.workflow)
        mutations = (
            ("if: (github.event_name == 'push' ||", "if: (github.event_name == 'pull_request' ||"),
            ("steps.dispatch-target.outputs.verified == 'true'", "true"),
            ('sha=$(git rev-parse HEAD)', 'sha=${{ github.event.pull_request.head.sha }}'),
            ('key: ${{ steps.target-build.outputs.key }}', 'key: ${{ github.sha }}'),
            ('actions/cache/restore@', 'actions/cache@'),
            ("if: steps.target-build.outputs.allowed == 'true'", "if: always()"),
        )
        for old, new in mutations:
            with self.subTest(new=new), self.assertRaises(AssertionError):
                cache_contract(self.workflow.replace(old, new))

    @unittest.skipIf(os.name == "nt", "the target-cache script runs in Linux bash")
    def test_identity_mismatch_miss_and_exact_hit(self):
        run = script(step(self.workflow, 'Verify or rebuild the target binary'))
        sha = 'a' * 40
        for hit, cached, dirty, rebuilt in (
            ('true', sha, 'false', False), ('true', 'b' * 40, 'false', True),
            ('false', sha, 'false', True), ('', None, 'false', True),
            ('true', 'malformed', 'false', True), ('true', sha, 'true', True),
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
                           TARGET_SHA=sha, CACHE_HIT=hit, BINARY_DIR=str(binary.parent),
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
                       TARGET_SHA='a' * 40, CACHE_HIT='', BINARY_DIR=str(root / 'cache'),
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
            for name, content in {'git': 'if [ "$1" = rev-parse ]; then echo "$TEST_SHA"; fi',
                                  'rustc': 'echo "$TEST_RUST"', 'cargo': 'echo cargo',
                                  'cc': 'echo compiler'}.items():
                path = root / name
                path.write_text('#!/bin/sh\n' + content + '\n')
                path.chmod(0o755)
            def key(sha='a' * 40, rust='rustc 1.94.0', image='20261102', event='pull_request_target', ref='integration/EPC-022', verified=''):
                output = root / 'output'
                output.write_text('')
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           TEST_SHA=sha, TEST_RUST=rust, ImageOS='ubuntu24', ImageVersion=image,
                           RUNNER_ARCH='X64', EVENT=event, TARGET_REF=ref, VERIFIED_DISPATCH=verified, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], env=env,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return output.read_text()
            self.assertIn('allowed=true', key(event='workflow_dispatch', ref='main', verified='true'))
            self.assertIn('allowed=false', key(event='workflow_dispatch', ref='task/head', verified='true'))
            original = key()
            self.assertIn('allowed=true', original)
            self.assertNotEqual(original, key(sha='b' * 40))
            self.assertNotEqual(original, key(rust='rustc 1.95.0'))
            self.assertNotEqual(original, key(image='20261103'))
            for event, ref in [('pull_request', 'main'), ('workflow_dispatch', 'main'),
                               ('push', 'task/head'), ('pull_request_target', 'task/head')]:
                self.assertIn('allowed=false', key(event=event, ref=ref))


if __name__ == '__main__':
    unittest.main()

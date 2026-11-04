#!/usr/bin/env python3
"""Check the operator-applied main-only cache workflows (TSK-1240).

The verdict jobs in codeflow-policy.yml only restore the policy binary. The
job `cache-main-build` in codeflow-registry.yml is the one writer, and it runs
for a push to the default branch alone.
"""
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / '.github/workflows/codeflow-policy.yml'
REGISTRY = ROOT / '.github/workflows/codeflow-registry.yml'
POLICY_SHA256 = '64eea3e571e2aa2bdca1650cbaead5e8ee6d8add19dfada7aaac03625aab417c'
REGISTRY_SHA256 = '35fd2901a905cca647c374ce8dd2b1d32563e329d87e4c769409effd6e7e6ae2'
NAMESPACE = 'codeflow-policy-main-binary-v3-'
DEPS_NAMESPACE = 'codeflow-policy-main-deps-v3'
MAIN_PUSH = "github.event_name == 'push' && github.ref == 'refs/heads/main'"
WRITER_JOB = 'cache-main-build'
POLICY_REL = '.github/workflows/codeflow-policy.yml'
REGISTRY_REL = '.github/workflows/codeflow-registry.yml'
IDENTITY = r'^codeflow\ [^[:space:]]+\ source=$AUTHORITY_SHA\ dirty=false\ inputs=[0-9a-f]+$'


def pinned(path, digest):
    data = path.read_bytes()
    assert hashlib.sha256(data).hexdigest() == digest, f'operator-applied workflow drift: {path.name}'
    return data.decode()


def policy_workflow():
    return pinned(WORKFLOW, POLICY_SHA256)


def registry_workflow():
    return pinned(REGISTRY, REGISTRY_SHA256)


def jobs_of(workflow):
    parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
    return dict(zip(parts[1::2], parts[2::2]))


def steps_of(job):
    body = job.split('\n    steps:\n', 1)[1]
    return ['      - ' + text for text in re.split(r'(?m)^      - ', body)[1:]]


def find_step(job, name):
    for text in steps_of(job):
        if text.startswith(f'      - name: {name}\n'):
            return text
    raise AssertionError(f'missing step: {name}')


def uses_steps(job, action):
    return [text for text in steps_of(job) if f'uses: {action}@' in text]


def script(step):
    run = step.split('        run: |\n', 1)[1]
    return '\n'.join(line[10:] for line in run.splitlines() if line.startswith('          '))


def workflow_permissions(text):
    return re.search(r'\npermissions:\n((?:  .+\n)+)', text).group(1)


def cache_contract(policy, registry):
    for text in (policy, registry):
        assert workflow_permissions(text) == '  contents: read\n'
        assert ': write' not in text
        assert 'restore-keys:' not in text
    triggers = policy.split('\njobs:\n', 1)[0]
    assert 'workflow_dispatch' not in triggers and '\n  push:' not in triggers and 'schedule:' not in triggers
    assert 'actions/cache/save@' not in policy and 'actions/cache@' not in policy
    assert 'lookup-only' not in policy
    for withdrawn in ('warm-target-cache:', 'build_sha:', 'dispatch-target'):
        assert withdrawn not in policy and withdrawn not in registry

    # The verdict jobs read the cache and never write it.
    verdict_jobs = jobs_of(policy)
    assert set(verdict_jobs) == {'commit-lint', 'release-impact'}
    keys, verifies = [], []
    for job in verdict_jobs.values():
        key = find_step(job, 'Identify the authority build')
        keys.append(script(key))
        assert 'AUTHORITY_SHA: ${{ github.sha }}' in key
        assert 'pull_request' not in key and 'github.event' not in script(key)
        assert '[ "$sha" = "$AUTHORITY_SHA" ]' in script(key)
        assert 'git diff --quiet' in script(key) and 'git diff --cached --quiet' in script(key)
        assert 'rustc -vV' in key and 'cargo -V' in key and 'cc --version' in key
        assert 'ImageVersion' in key and 'RUNNER_ARCH' in key
        assert f'key={NAMESPACE}$sha-$identity' in script(key)
        restore = uses_steps(job, 'actions/cache/restore')
        assert len(restore) == 1 and len(uses_steps(job, 'actions/cache')) == 0
        assert 'id: authority-cache' in restore[0]
        assert 'key: ${{ steps.authority-build.outputs.key }}' in restore[0]
        deps = uses_steps(job, 'Swatinem/rust-cache')
        assert len(deps) == 1
        assert 'save-if: false' in deps[0] and 'prefix-key: ' + DEPS_NAMESPACE in deps[0]
        assert "if: steps.authority-cache.outputs.cache-hit != 'true'" in deps[0]
        verify = find_step(job, 'Verify or rebuild the authority binary')
        verifies.append(script(verify))
        assert '[ -f "$binary" ] && [ -x "$binary" ] && [ ! -L "$binary" ] || return 1' in verify
        assert IDENTITY in verify
        assert '[ "$CACHE_HIT" != true ] || ! matches_authority' in verify
        assert job.count('cargo build --release --locked -p codeflow-cli') == 1
        assert job.count('cargo build') == 1
        order = [job.index(find_step(job, 'Identify the authority build')), job.index(restore[0]),
                 job.index(deps[0]), job.index(verify), job.index("      - name: Fetch the pull request's base and head as data")]
        assert order == sorted(order)
    assert keys[0] == keys[1] and verifies[0] == verifies[1]

    # One writer: its own job in the registry workflow.
    registry_jobs = jobs_of(registry)
    assert set(registry_jobs) == {'registry-check', WRITER_JOB}
    check = registry_jobs['registry-check']
    assert 'actions/cache' not in check and 'codeflow-policy-main' not in check
    writer = registry_jobs[WRITER_JOB]
    assert re.search(r'^    if: ' + re.escape(MAIN_PUSH) + r'$', writer, re.M)
    assert '    timeout-minutes: ' in writer
    assert '    permissions:\n      contents: read\n    steps:\n' in writer
    assert 'secrets.' not in writer and 'environment:' not in writer and 'needs:' not in writer
    assert 'id-token' not in writer and 'GITHUB_TOKEN' not in writer and 'github.token' not in writer
    assert registry.count('actions/cache/save@') == 1 and writer.count('actions/cache/save@') == 1
    assert len(uses_steps(writer, 'actions/cache')) == 0
    key = find_step(writer, 'Identify the main build')
    assert script(key) == keys[0]
    assert 'id: main-build' in key and 'AUTHORITY_SHA: ${{ github.sha }}' in key
    look = find_step(writer, 'Look for the saved main binary')
    assert 'actions/cache/restore@' in look and 'lookup-only: true' in look
    assert 'key: ${{ steps.main-build.outputs.key }}' in look and 'id: main-cache' in look
    deps = uses_steps(writer, 'Swatinem/rust-cache')
    assert len(deps) == 1
    assert 'save-if: ${{ ' + MAIN_PUSH + ' }}' in deps[0] and 'prefix-key: ' + DEPS_NAMESPACE in deps[0]
    assert "if: steps.main-cache.outputs.cache-hit != 'true'" in deps[0]
    build = find_step(writer, 'Build and verify the main binary')
    assert "if: steps.main-cache.outputs.cache-hit != 'true'" in build and 'id: main-binary' in build
    assert writer.count('cargo build --release --locked -p codeflow-cli') == 1
    assert IDENTITY in build
    save = find_step(writer, 'Save the verified main binary')
    assert 'actions/cache/save@' in save
    assert 'if: ' + MAIN_PUSH + " && steps.main-binary.outputs.built == 'true' && steps.main-cache.outputs.cache-hit != 'true'" in save
    assert 'key: ${{ steps.main-build.outputs.key }}' in save
    order = [writer.index(key), writer.index(look), writer.index(deps[0]), writer.index(build), writer.index(save)]
    assert order == sorted(order)
    assert registry.count('cargo build --release') == 2
    assert 'run: codeflow ids check\n' in check


def writers_contract(files):
    saves = sorted(path for path, text in files.items() if 'actions/cache/save@' in text)
    assert saves == [REGISTRY_REL], f'a second cache writer: {saves}'
    for path, text in files.items():
        if path in (POLICY_REL, REGISTRY_REL):
            continue
        for name in (NAMESPACE, DEPS_NAMESPACE, 'codeflow-policy-main'):
            assert name not in text, f'{path} names the policy cache namespace'


def workflow_files():
    files = {}
    for folder in ('.github/workflows', 'assets/base/ci', '.codeflow/.baseline/.github/workflows'):
        base = ROOT / folder
        if not base.is_dir():
            continue
        for path in sorted(base.rglob('*')):
            if path.suffix in ('.yml', '.yaml'):
                files[path.relative_to(ROOT).as_posix()] = path.read_text()
    return files


class PolicyCache(unittest.TestCase):
    def setUp(self):
        self.policy = policy_workflow()
        self.registry = registry_workflow()

    def test_cache_authority(self):
        cache_contract(self.policy, self.registry)

    def test_one_writer_across_every_workflow(self):
        files = workflow_files()
        files[POLICY_REL] = self.policy
        files[REGISTRY_REL] = self.registry
        writers_contract(files)
        for path in list(files):
            if path in (POLICY_REL, REGISTRY_REL):
                continue
            with self.subTest(second_writer=path):
                mutated = dict(files)
                mutated[path] = files[path] + '\n      - uses: actions/cache/save@x\n'
                with self.assertRaises(AssertionError):
                    writers_contract(mutated)
            with self.subTest(namespace=path):
                mutated = dict(files)
                mutated[path] = files[path] + f'\n# key: {NAMESPACE}$sha\n'
                with self.assertRaises(AssertionError):
                    writers_contract(mutated)
            with self.subTest(deps=path):
                mutated = dict(files)
                mutated[path] = files[path] + f'\n# prefix-key: {DEPS_NAMESPACE}\n'
                with self.assertRaises(AssertionError):
                    writers_contract(mutated)

    def test_writer_and_verdict_jobs_reject_every_other_save_and_any_write_permission(self):
        writer_permissions = '    timeout-minutes: 30\n    permissions:\n      contents: read\n'
        mutations = (
            ('registry', MAIN_PUSH, "github.event_name == 'push'"),
            ('registry', MAIN_PUSH, "github.event_name == 'pull_request'"),
            ('registry', MAIN_PUSH, "github.event_name == 'push' && github.ref == 'refs/heads/integration/x'"),
            ('registry', MAIN_PUSH, "github.event_name != 'pull_request'"),
            ('registry', writer_permissions, writer_permissions.replace('contents: read', 'contents: write')),
            ('registry', writer_permissions, writer_permissions + '      actions: write\n'),
            ('registry', writer_permissions, writer_permissions + '      pull-requests: write\n'),
            ('registry', writer_permissions, writer_permissions + '      packages: write\n'),
            ('registry', writer_permissions, writer_permissions + '      id-token: write\n'),
            ('registry', writer_permissions, writer_permissions + '      actions: read\n'),
            ('registry', writer_permissions, writer_permissions + '      contents: write\n'),
            ('registry', '    permissions:\n      contents: read\n    steps:\n      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n        with:\n          ref: ${{ github.sha }}',
             '    steps:\n      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n        with:\n          ref: ${{ github.sha }}'),
            ('registry', '          BINARY_DIR: ${{ runner.temp }}/codeflow-policy-binary\n        run: |\n          set -euo pipefail\n          binary="$BINARY_DIR/codeflow"\n          cargo build',
             '          BINARY_DIR: ${{ runner.temp }}/codeflow-policy-binary\n          TOKEN: ${{ secrets.PUBLISH }}\n        run: |\n          set -euo pipefail\n          binary="$BINARY_DIR/codeflow"\n          cargo build'),
            ('registry', '          lookup-only: true', '          lookup-only: false'),
            ('registry', '          lookup-only: true', '          lookup-only: true\n          restore-keys: codeflow-policy-main-binary-v3-'),
            ('registry', '          save-if: ${{ ' + MAIN_PUSH + ' }}', '          save-if: true'),
            ('registry', " && steps.main-binary.outputs.built == 'true'", ''),
            ('registry', " && steps.main-cache.outputs.cache-hit != 'true'\n        uses: actions/cache/save@", '\n        uses: actions/cache/save@'),
            ('registry', '      - name: Fetch the shared registry\n',
             '      - name: Save elsewhere\n        uses: actions/cache/save@x\n        with:\n          key: k\n          path: p\n\n      - name: Fetch the shared registry\n'),
            ('registry', '    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n', '    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n    needs: registry-check\n'),
            ('policy', 'actions/cache/restore@', 'actions/cache/save@'),
            ('policy', 'actions/cache/restore@', 'actions/cache@'),
            ('policy', 'save-if: false', 'save-if: true'),
            ('policy', 'save-if: false', 'save-if: ${{ ' + MAIN_PUSH + ' }}'),
            ('policy', 'permissions:\n  contents: read', 'permissions:\n  contents: write'),
            ('policy', '      contents: read\n      pull-requests: read', '      contents: read\n      pull-requests: write'),
            ('policy', '      actions: read\n', '      actions: write\n'),
            ('policy', 'key: ${{ steps.authority-build.outputs.key }}', 'key: ${{ github.event.pull_request.head.sha }}'),
            ('policy', '          key: ${{ steps.authority-build.outputs.key }}\n',
             '          key: ${{ steps.authority-build.outputs.key }}\n          restore-keys: codeflow-policy-main-binary-v3-\n'),
            ('policy', '          key: ${{ steps.authority-build.outputs.key }}\n',
             '          key: ${{ steps.authority-build.outputs.key }}\n          lookup-only: true\n'),
            ('policy', 'sha=$(git rev-parse HEAD)', 'sha=${{ github.event.pull_request.head.sha }}'),
            ('policy', '          [ "$sha" = "$AUTHORITY_SHA" ]\n', ''),
            ('policy', 'rustc -vV\n', ''),
            ('policy', '[ -f "$binary" ] && ', ''),
            ('policy', '[ "$CACHE_HIT" != true ] || ! matches_authority', '[ "$CACHE_HIT" != true ]'),
            ('policy', 'codeflow-policy-main-deps-v3', 'shared-deps'),
            ('policy', '\njobs:\n', '\njobs:\n  save-cache:\n    permissions:\n      contents: write\n'),
        )
        for target, before, after in mutations:
            text = self.registry if target == 'registry' else self.policy
            self.assertIn(before, text, before)
            offset = text.find(before)
            while offset != -1:
                with self.subTest(target=target, mutation=after[:60], offset=offset):
                    mutated = text[:offset] + after + text[offset + len(before):]
                    pair = (self.policy, mutated) if target == 'registry' else (mutated, self.registry)
                    with self.assertRaises(AssertionError):
                        cache_contract(*pair)
                offset = text.find(before, offset + len(before))

    def test_a_job_that_gains_a_write_permission_in_either_file_is_rejected(self):
        # Any added permission line, in the writer or beside it, fails the contract.
        for key in ('actions', 'checks', 'contents', 'deployments', 'issues', 'packages', 'pull-requests', 'statuses'):
            with self.subTest(key=key):
                mutated = self.registry.replace(
                    '    permissions:\n      contents: read\n    steps:\n      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n        with:\n          ref: ${{ github.sha }}',
                    f'    permissions:\n      contents: read\n      {key}: write\n    steps:\n      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n        with:\n          ref: ${{{{ github.sha }}}}')
                self.assertNotEqual(mutated, self.registry)
                with self.assertRaises(AssertionError):
                    cache_contract(self.policy, mutated)

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_identity_mismatch_miss_and_exact_hit(self):
        run = script(find_step(jobs_of(self.policy)['commit-lint'], 'Verify or rebuild the authority binary'))
        sha = 'a' * 40
        cases = (
            ('true', sha, 'false', False, False), ('true', 'b' * 40, 'false', True, False),
            ('false', sha, 'false', True, False), ('', None, 'false', True, False),
            ('true', 'malformed', 'false', True, False), ('true', sha, 'true', True, False),
            ('true', sha, 'false', True, True),
        )
        for hit, cached, dirty, rebuilt, symlink in cases:
            with self.subTest(hit=hit, cached=cached, symlink=symlink), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / 'cache' / 'codeflow'
                binary.parent.mkdir()
                if cached:
                    text = f'#!/bin/sh\necho "codeflow 4.0.0 source={cached} dirty={dirty} inputs=abc"\n'
                    if symlink:
                        real = root / 'real'
                        real.write_text(text)
                        real.chmod(0o755)
                        binary.symlink_to(real)
                    else:
                        binary.write_text(text)
                        binary.chmod(0o755)
                cargo = root / 'cargo'
                cargo.write_text('#!/bin/sh\nmkdir -p target/release\nprintf build >> built\n'
                                 f'printf \'#!/bin/sh\\necho "codeflow 4.0.0 source={sha} dirty=false inputs=abc"\\n\' > target/release/codeflow\n'
                                 'chmod +x target/release/codeflow\n')
                cargo.chmod(0o755)
                path_file = root / 'path'
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           AUTHORITY_SHA=sha, CACHE_HIT=hit, BINARY_DIR=str(binary.parent),
                           GITHUB_WORKSPACE=str(root), GITHUB_PATH=str(path_file))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=root,
                                        env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((root / 'built').exists(), rebuilt)
                self.assertIn(f'rebuilt={str(rebuilt).lower()}', result.stdout)
                self.assertIn(f'source={sha}', subprocess.check_output([str(binary), '--version'], text=True))
                self.assertFalse(binary.is_symlink())
                self.assertIn(f'source={sha}', subprocess.check_output([str(root / 'target/release/codeflow'), '--version'], text=True))
                self.assertEqual(path_file.read_text().strip(), f'{root}/target/release')

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_a_rebuild_with_wrong_identity_fails_closed(self):
        run = script(find_step(jobs_of(self.policy)['release-impact'], 'Verify or rebuild the authority binary'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / 'cargo'
            cargo.write_text('#!/bin/sh\nmkdir -p target/release\n'
                             'printf \'#!/bin/sh\\necho "codeflow 4.0.0 source=wrong dirty=false inputs=abc"\\n\' > target/release/codeflow\n'
                             'chmod +x target/release/codeflow\n')
            cargo.chmod(0o755)
            path_file = root / 'path'
            env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                       AUTHORITY_SHA='a' * 40, CACHE_HIT='', BINARY_DIR=str(root / 'cache'),
                       GITHUB_WORKSPACE=str(root), GITHUB_PATH=str(path_file))
            result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=root,
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('::error::authority binary source identity does not match', result.stdout)
            self.assertFalse(path_file.exists(), 'a failed binary must not enter PATH')

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_the_writer_saves_only_a_verified_build(self):
        run = script(find_step(jobs_of(self.registry)[WRITER_JOB], 'Build and verify the main binary'))
        sha = 'a' * 40
        for built, expect_ok in ((sha, True), ('wrong', False), ('', False)):
            with self.subTest(built=built), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cargo = root / 'cargo'
                version = f'codeflow 4.0.0 source={built} dirty=false inputs=abc' if built else 'garbled'
                cargo.write_text('#!/bin/sh\nmkdir -p target/release\n'
                                 f'printf \'#!/bin/sh\\necho "{version}"\\n\' > target/release/codeflow\n'
                                 'chmod +x target/release/codeflow\n')
                cargo.chmod(0o755)
                output = root / 'output'
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           AUTHORITY_SHA=sha, BINARY_DIR=str(root / 'cache'), GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], cwd=root,
                                        env=env, capture_output=True, text=True)
                if expect_ok:
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertEqual(output.read_text().strip(), 'built=true')
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(output.exists(), 'a wrong identity must not enable the save')

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_key_changes_with_commit_toolchain_and_image_and_refuses_a_moved_or_dirty_checkout(self):
        scripts = [script(find_step(job, 'Identify the authority build')) for job in jobs_of(self.policy).values()]
        scripts.append(script(find_step(jobs_of(self.registry)[WRITER_JOB], 'Identify the main build')))
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

            def key(run, sha='a' * 40, authority='a' * 40, rust='rustc 1.94.0', image='20261102', dirty=''):
                output = root / 'output'
                output.write_text('')
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}',
                           TEST_SHA=sha, TEST_RUST=rust, TEST_DIRTY=dirty, ImageOS='ubuntu24', ImageVersion=image,
                           RUNNER_ARCH='X64', AUTHORITY_SHA=authority, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', run], env=env,
                                        capture_output=True, text=True)
                if dirty or sha != authority:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(output.read_text(), '', 'a moved or dirty checkout must not emit a cache key')
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                return output.read_text()

            results = []
            for run in scripts:
                key(run, dirty='unstaged')
                key(run, dirty='staged')
                key(run, sha='b' * 40)
                original = key(run)
                self.assertIn(f'key={NAMESPACE}{"a" * 40}-', original)
                self.assertNotEqual(original, key(run, sha='b' * 40, authority='b' * 40))
                self.assertNotEqual(original, key(run, rust='rustc 1.95.0'))
                self.assertNotEqual(original, key(run, image='20261103'))
                results.append(original)
            self.assertEqual(len(set(results)), 1, 'the writer and both readers compute the same key')


if __name__ == '__main__':
    unittest.main()

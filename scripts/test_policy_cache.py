#!/usr/bin/env python3
"""Check the operator-applied main-only cache workflows (TSK-1240).

The verdict jobs in codeflow-policy.yml only restore the policy binary, and
run it only when its build attestation verifies. The job `cache-main-build` in
codeflow-registry.yml is the one writer: it runs for a push to the default
branch alone, builds, attests and saves.

The limit of the design: the attestation covers the bytes that run. It does not
make restoring an entry safe, because the cache action extracts an archive
before any check can run.
"""
import hashlib
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / '.github/workflows/codeflow-policy.yml'
REGISTRY = ROOT / '.github/workflows/codeflow-registry.yml'
POLICY_SHA256 = '5ceab01371a51eeca3be7087f25e83de3f5af4597bb0b117a8647a0bc6b5128d'
REGISTRY_SHA256 = '0e3c77ac0ac9dd21d4f064020cd8f25f0044d6f7f14d92a080c34eece6621a5c'
NAMESPACE = 'codeflow-policy-main-binary-v3-'
DEPS_NAMESPACE = 'codeflow-policy-main-deps-v3'
MAIN_PUSH = "github.event_name == 'push' && github.ref == 'refs/heads/main'"
WRITER_JOB = 'cache-main-build'
POLICY_REL = '.github/workflows/codeflow-policy.yml'
REGISTRY_REL = '.github/workflows/codeflow-registry.yml'
IDENTITY = r'^codeflow\ [^[:space:]]+\ source=$AUTHORITY_SHA\ dirty=false\ inputs=[0-9a-f]+$'
ATTEST = 'actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2'
WRITER_PERMISSIONS = '    permissions:\n      contents: read\n      id-token: write\n      attestations: write\n'
VERIFY_FLAGS = (
    '--repo "$GITHUB_REPOSITORY"',
    '--cert-identity "https://github.com/${GITHUB_REPOSITORY}/.github/workflows/codeflow-registry.yml@refs/heads/main"',
    '--source-ref refs/heads/main',
    '--source-digest "$AUTHORITY_SHA"',
    '--signer-digest "$AUTHORITY_SHA"',
    '--deny-self-hosted-runners',
)
BINARY_PATH = '${{ runner.temp }}/codeflow-main-binary'


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


# The policy workflow's jobs that build or restore the authority binary. The
# `authority` job (TSK-1177) only concludes checks from them.
VERDICT_JOBS = ('commit-lint', 'release-impact')


def verdict_jobs_of(workflow):
    jobs = jobs_of(workflow)
    return {name: jobs[name] for name in VERDICT_JOBS}


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
        assert 'restore-keys:' not in text
    assert ': write' not in policy
    # The only write permissions in the registry workflow are the two the writer's attestation needs.
    assert registry.count(': write') == 2
    triggers = policy.split('\njobs:\n', 1)[0]
    assert 'workflow_dispatch' not in triggers and '\n  push:' not in triggers and 'schedule:' not in triggers
    assert 'actions/cache/save@' not in policy and 'actions/cache@' not in policy
    assert 'attest-build-provenance' not in policy and 'id-token' not in policy
    assert 'lookup-only' not in policy
    for withdrawn in ('warm-target-cache:', 'build_sha:', 'dispatch-target'):
        assert withdrawn not in policy and withdrawn not in registry

    # The verdict jobs read the cache and never write it. The authority job
    # builds nothing and touches no cache.
    jobs = jobs_of(policy)
    assert set(jobs) == {*VERDICT_JOBS, 'authority'}
    for word in ('rust-cache', 'actions/cache', 'authority-cache', 'cargo build'):
        assert word not in jobs['authority'], word
    verdict_jobs = verdict_jobs_of(policy)
    keys, verifies, builds = [], [], []
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
        assert 'path: ' + BINARY_PATH + '\n' in restore[0]
        verify_step = find_step(job, 'Verify the authority binary')
        verify = script(verify_step)
        verifies.append(verify)
        assert 'id: authority-verify' in verify_step
        assert 'GH_TOKEN: ${{ github.token }}' in verify_step and 'secrets.' not in verify_step
        assert 'CACHE_HIT: ${{ steps.authority-cache.outputs.cache-hit }}' in verify_step
        assert 'BINARY_DIR: ' + BINARY_PATH + '\n' in verify_step
        assert '[ -f "$binary" ] && [ ! -L "$binary" ] && [ -x "$binary" ]' in verify
        assert 'timeout 60 gh attestation verify "$binary"' in verify
        for flag in VERIFY_FLAGS:
            assert flag in verify, flag
        assert 'timeout 30 env -i "$binary" --version' in verify
        assert IDENTITY in verify
        assert 'verified=false' in verify and 'echo "verified=$verified" >> "$GITHUB_OUTPUT"' in verify
        assert 'rm -rf "$BINARY_DIR"' in verify
        assert '::warning::' in verify
        # Nothing from the cache runs before the attestation verifies.
        assert verify.index('gh attestation verify') < verify.index('--version') < verify.index('cp "$binary" target/release/codeflow')
        assert verify.count('--version') == 1
        assert 'GITHUB_PATH' in verify and verify.index('GITHUB_PATH') > verify.index('verified=true')
        deps = uses_steps(job, 'Swatinem/rust-cache')
        assert len(deps) == 1
        assert 'save-if: false' in deps[0] and 'prefix-key: ' + DEPS_NAMESPACE in deps[0]
        assert "if: steps.authority-verify.outputs.verified != 'true'" in deps[0]
        # The two caches stay separable: the dependency step reads only the verified flag.
        assert 'authority-cache' not in deps[0] and 'authority-build' not in deps[0] and NAMESPACE not in deps[0]
        build = find_step(job, 'Build the authority binary')
        builds.append(script(build))
        assert "if: steps.authority-verify.outputs.verified != 'true'" in build
        assert 'authority-cache' not in build and 'codeflow-main-binary' not in build
        assert IDENTITY in build and 'target/release/codeflow --version' in build
        assert 'AUTHORITY_SHA: ${{ github.sha }}' in build
        assert 'exit 1' in build and build.index('--version') < build.index('GITHUB_PATH')
        assert job.count('cargo build --release --locked -p codeflow-cli') == 1
        assert job.count('cargo build') == 1
        order = [job.index(key), job.index(restore[0]), job.index(verify_step), job.index(deps[0]), job.index(build),
                 job.index("      - name: Fetch the pull request's base and head as data")]
        assert order == sorted(order)
    assert keys[0] == keys[1] and verifies[0] == verifies[1] and builds[0] == builds[1]

    # One writer: its own job in the registry workflow.
    registry_jobs = jobs_of(registry)
    assert set(registry_jobs) == {'registry-check', WRITER_JOB}
    check = registry_jobs['registry-check']
    assert 'actions/cache' not in check and 'codeflow-policy-main' not in check and 'attest-build-provenance' not in check
    assert ': write' not in check
    writer = registry_jobs[WRITER_JOB]
    assert re.search(r'^    if: ' + re.escape(MAIN_PUSH) + r'$', writer, re.M)
    assert '    timeout-minutes: ' in writer
    assert '    permissions:\n      contents: read\n      id-token: write\n      attestations: write\n    steps:\n' in writer
    assert writer.count(': write') == 2
    assert 'secrets.' not in writer and 'environment:' not in writer and 'needs:' not in writer
    assert 'GITHUB_TOKEN' not in writer and 'github.token' not in writer
    assert 'persist-credentials: false' in writer
    assert registry.count('actions/cache/save@') == 1 and writer.count('actions/cache/save@') == 1
    assert registry.count('actions/attest-build-provenance@') == 1 and writer.count('actions/attest-build-provenance@') == 1
    assert len(uses_steps(writer, 'actions/cache')) == 0
    key = find_step(writer, 'Identify the main build')
    assert script(key) == keys[0]
    assert 'id: main-build' in key and 'AUTHORITY_SHA: ${{ github.sha }}' in key
    look = find_step(writer, 'Look for the saved main binary')
    assert 'actions/cache/restore@' in look and 'lookup-only: true' in look
    assert 'key: ${{ steps.main-build.outputs.key }}' in look and 'id: main-cache' in look
    assert 'path: ' + BINARY_PATH + '\n' in look
    deps = uses_steps(writer, 'Swatinem/rust-cache')
    assert len(deps) == 1
    assert 'save-if: ${{ ' + MAIN_PUSH + ' }}' in deps[0] and 'prefix-key: ' + DEPS_NAMESPACE in deps[0]
    assert "if: steps.main-cache.outputs.cache-hit != 'true'" in deps[0]
    build = find_step(writer, 'Build and verify the main binary')
    assert "if: steps.main-cache.outputs.cache-hit != 'true'" in build and 'id: main-binary' in build
    assert writer.count('cargo build --release --locked -p codeflow-cli') == 1
    assert IDENTITY in build
    gate = 'if: ' + MAIN_PUSH + " && steps.main-binary.outputs.built == 'true' && steps.main-cache.outputs.cache-hit != 'true'"
    attest = find_step(writer, 'Attest the verified main binary')
    assert gate in attest and 'uses: ' + ATTEST + '\n' in attest
    assert 'subject-path: ' + BINARY_PATH + '/codeflow\n' in attest
    assert 'push-to-registry' not in attest and 'create-storage-record' not in attest
    save = find_step(writer, 'Save the verified main binary')
    assert 'actions/cache/save@' in save
    assert gate in save
    assert 'key: ${{ steps.main-build.outputs.key }}' in save
    order = [writer.index(key), writer.index(look), writer.index(deps[0]), writer.index(build), writer.index(attest), writer.index(save)]
    assert order == sorted(order)
    assert registry.count('cargo build --release') == 2
    assert 'run: codeflow ids check\n' in check


def writers_contract(files):
    saves = sorted(path for path, text in files.items() if 'actions/cache/save@' in text)
    assert saves == [REGISTRY_REL], f'a second cache writer: {saves}'
    attests = sorted(path for path, text in files.items() if 'actions/attest-build-provenance@' in text)
    assert attests == [REGISTRY_REL], f'a second attesting workflow: {attests}'
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


# Longer than the step's own limits (60 s for the attestation, 30 s for the version), so the step's
# `timeout` fires first and this bound only catches a step that ignores its own limits.
RUN_BASH_SECONDS = 120
DRAIN_SECONDS = 5


def session_pids(session):
    # Every live process in the given session. GNU timeout moves its command into a new process
    # group, so a process group kill of bash misses it; the session id still matches.
    # A zombie is already dead and only waits to be reaped, so it is skipped.
    candidates = []
    if Path('/proc/self/stat').is_file():
        for entry in Path('/proc').iterdir():
            if not entry.name.isdigit():
                continue
            try:
                state = (entry / 'stat').read_text().rsplit(')', 1)[1].split()[0]
            except (OSError, IndexError):
                continue
            if state != 'Z':
                candidates.append(int(entry.name))
    else:
        listing = subprocess.run(['ps', '-A', '-o', 'pid=,stat='], stdin=subprocess.DEVNULL, capture_output=True,
                                 text=True, timeout=DRAIN_SECONDS, check=True)
        for line in listing.stdout.splitlines():
            fields = line.split()
            if len(fields) == 2 and not fields[1].startswith('Z'):
                candidates.append(int(fields[0]))
    found = []
    for pid in candidates:
        try:
            if os.getsid(pid) == session:
                found.append(pid)
        except (ProcessLookupError, PermissionError):
            continue
    return found


def kill_session(process):
    # Kill every process in the step's session; repeat in case one forked while the list was read.
    # Bash is reaped here and orphans are reaped by init, so a dead process leaves the list.
    session = process.pid
    for _ in range(20):
        pids = session_pids(session)
        if not pids:
            return
        for pid in pids:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        time.sleep(0.1)
        process.poll()
    raise AssertionError(f'processes in session {session} survived SIGKILL: {session_pids(session)}')


def run_bash(run, cwd, env):
    # stdin is closed so nothing in the step can wait on input, and the run is bounded: a hang
    # fails the test instead of the whole gate. The step runs in its own session so a timeout
    # kills every process it started, including those `timeout` moved into another process group,
    # and the wait after the kill has its own deadline.
    process = subprocess.Popen(['bash', '-euo', 'pipefail', '-c', run], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=RUN_BASH_SECONDS)
    except subprocess.TimeoutExpired:
        failure = f'the step did not finish in {RUN_BASH_SECONDS} s'
        try:
            # Bash's own group first, which needs no process listing; then the rest of the session.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            kill_session(process)
        except (AssertionError, OSError, subprocess.SubprocessError) as error:
            failure += f'; cleaning up its processes failed: {error}'
        try:
            stdout, stderr = process.communicate(timeout=DRAIN_SECONDS)
        except subprocess.TimeoutExpired:
            stdout, stderr = '', ''
            failure += f'; its output pipes were still open {DRAIN_SECONDS} s after the kill'
        raise AssertionError(f'{failure}\n{stdout}{stderr}') from None
    return subprocess.CompletedProcess(process.args, process.returncode, stdout, stderr)


def real_timeout():
    # Resolve the real `timeout` on the caller's PATH, before the fake folder is put in front of it.
    # A bare `timeout` inside the fake would find the fake again and exec itself forever.
    found = shutil.which('timeout', path=os.environ.get('PATH', os.defpath))
    if found is None:
        raise AssertionError('the cache step tests need a `timeout` command on PATH (GNU coreutils)')
    return found


def fake_version(source, dirty='false'):
    return f'#!/bin/sh\necho "codeflow 4.0.0 source={source} dirty={dirty} inputs=abc"\n'


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
            with self.subTest(second_attester=path):
                mutated = dict(files)
                mutated[path] = files[path] + '\n      - uses: actions/attest-build-provenance@x\n'
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

    def test_mutations_of_either_workflow_are_rejected(self):
        verify_attest = 'timeout 60 gh attestation verify "$binary"'
        mutations = (
            ('registry', MAIN_PUSH, "github.event_name == 'push'"),
            ('registry', MAIN_PUSH, "github.event_name == 'pull_request'"),
            ('registry', MAIN_PUSH, "github.event_name == 'push' && github.ref == 'refs/heads/integration/x'"),
            ('registry', MAIN_PUSH, "github.event_name != 'pull_request'"),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS.replace('contents: read', 'contents: write')),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS + '      actions: write\n'),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS + '      pull-requests: write\n'),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS + '      packages: write\n'),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS + '      contents: write\n'),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS + '      actions: read\n'),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS.replace('      attestations: write\n', '')),
            ('registry', WRITER_PERMISSIONS, WRITER_PERMISSIONS.replace('      id-token: write\n', '')),
            ('registry', WRITER_PERMISSIONS, '    permissions:\n      contents: read\n'),
            ('registry', '          persist-credentials: false\n', ''),
            ('registry', '          ref: ${{ github.sha }}\n          fetch-depth: 0\n          persist-credentials: false\n',
             '          ref: ${{ github.sha }}\n          fetch-depth: 0\n          persist-credentials: false\n          token: ${{ secrets.PUBLISH }}\n'),
            ('registry', '        run: |\n          set -euo pipefail\n          binary="$BINARY_DIR/codeflow"\n          cargo build',
             '          TOKEN: ${{ secrets.PUBLISH }}\n        run: |\n          set -euo pipefail\n          binary="$BINARY_DIR/codeflow"\n          cargo build'),
            ('registry', '          lookup-only: true', '          lookup-only: false'),
            ('registry', '          lookup-only: true', '          lookup-only: true\n          restore-keys: codeflow-policy-main-binary-v3-'),
            ('registry', '          save-if: ${{ ' + MAIN_PUSH + ' }}', '          save-if: true'),
            ('registry', " && steps.main-binary.outputs.built == 'true'", ''),
            ('registry', " && steps.main-cache.outputs.cache-hit != 'true'\n        uses: " + ATTEST, '\n        uses: ' + ATTEST),
            ('registry', "        uses: " + ATTEST + "\n        with:\n          subject-path: " + BINARY_PATH + "/codeflow\n",
             "        uses: " + ATTEST + "\n        with:\n          subject-path: " + BINARY_PATH + "/codeflow\n          push-to-registry: true\n"),
            ('registry', "subject-path: " + BINARY_PATH + "/codeflow", "subject-path: target/release/codeflow"),
            ('registry', '      - name: Fetch the shared registry\n',
             '      - name: Save elsewhere\n        uses: actions/cache/save@x\n        with:\n          key: k\n          path: p\n\n      - name: Fetch the shared registry\n'),
            ('registry', '      - name: Fetch the shared registry\n',
             '      - name: Attest elsewhere\n        uses: actions/attest-build-provenance@x\n\n      - name: Fetch the shared registry\n'),
            ('registry', '    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n', '    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n    needs: registry-check\n'),
            ('policy', 'actions/cache/restore@', 'actions/cache/save@'),
            ('policy', 'actions/cache/restore@', 'actions/cache@'),
            ('policy', 'save-if: false', 'save-if: true'),
            ('policy', 'save-if: false', 'save-if: ${{ ' + MAIN_PUSH + ' }}'),
            ('policy', 'permissions:\n  contents: read', 'permissions:\n  contents: write'),
            ('policy', '      contents: read\n      pull-requests: read', '      contents: read\n      id-token: write\n      pull-requests: read'),
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
            ('policy', verify_attest, 'timeout 60 true "$binary"'),
            ('policy', verify_attest, 'gh attestation verify "$binary"'),
            ('policy', '                --source-ref refs/heads/main \\\n', ''),
            ('policy', '                --source-digest "$AUTHORITY_SHA" \\\n', ''),
            ('policy', '                --signer-digest "$AUTHORITY_SHA" \\\n', ''),
            ('policy', '                --deny-self-hosted-runners > /dev/null', '                > /dev/null'),
            ('policy', '                --repo "$GITHUB_REPOSITORY" \\\n', ''),
            ('policy', 'timeout 30 env -i "$binary" --version', '"$binary" --version'),
            ('policy', '[ ! -L "$binary" ] && ', ''),
            ('policy', "if: steps.authority-verify.outputs.verified != 'true'\n        with:", "if: steps.authority-cache.outputs.cache-hit != 'true'\n        with:"),
            ('policy', "if: steps.authority-verify.outputs.verified != 'true'\n        env:", "if: steps.authority-cache.outputs.cache-hit != 'true'\n        env:"),
            ('policy', "          echo \"verified=$verified\" >> \"$GITHUB_OUTPUT\"\n", "          echo \"verified=true\" >> \"$GITHUB_OUTPUT\"\n"),
            ('policy', "          else\n            rm -rf \"$BINARY_DIR\"\n          fi\n", "          fi\n"),
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

    def test_no_ref_but_main_can_save_the_binary_or_dependency_cache(self):
        # AC-3: every place that decides a save (the writer job, the dependency save, the attestation and
        # the binary save) must name exactly a push to refs/heads/main. A condition that lets a pull request
        # event, an integration push, a schedule, a dispatch or any other ref through is rejected.
        offsets = [m.start() for m in re.finditer(re.escape(MAIN_PUSH), self.registry)]
        self.assertEqual(len(offsets), 4, 'the writer job, dependency save, attestation and binary save')
        looser = (
            "github.event_name == 'push'",
            "github.event_name == 'push' && startsWith(github.ref, 'refs/heads/')",
            "github.event_name == 'push' && github.ref == 'refs/heads/integration/x'",
            "github.event_name == 'push' && github.ref_name == 'main'",
            "github.event_name != 'pull_request' && github.ref == 'refs/heads/main'",
            "github.event_name == 'schedule' || " + MAIN_PUSH,
            "github.event_name == 'workflow_dispatch' || " + MAIN_PUSH,
            "github.ref == 'refs/heads/main'",
            "true",
            "always()",
        )
        for offset in offsets:
            for condition in looser:
                with self.subTest(offset=offset, condition=condition):
                    mutated = self.registry[:offset] + condition + self.registry[offset + len(MAIN_PUSH):]
                    with self.assertRaises(AssertionError):
                        cache_contract(self.policy, mutated)
        for before, after in (('save-if: ${{ ' + MAIN_PUSH + ' }}', 'save-if: true'),
                              ('save-if: ${{ ' + MAIN_PUSH + ' }}', "save-if: ${{ github.event_name == 'push' }}")):
            with self.subTest(after=after):
                with self.assertRaises(AssertionError):
                    cache_contract(self.policy, self.registry.replace(before, after))
        # A verdict job, which runs on pull_request_target, can never save.
        for after in ('save-if: true', "save-if: ${{ github.event_name == 'pull_request_target' }}"):
            with self.subTest(verdict=after):
                with self.assertRaises(AssertionError):
                    cache_contract(self.policy.replace('save-if: false', after), self.registry)

    def test_a_job_that_gains_a_write_permission_in_either_file_is_rejected(self):
        # Any added write permission line beyond the writer's two fails the contract.
        for key in ('actions', 'checks', 'contents', 'deployments', 'issues', 'packages', 'pull-requests', 'statuses', 'security-events'):
            with self.subTest(key=key):
                mutated = self.registry.replace(WRITER_PERMISSIONS, WRITER_PERMISSIONS + f'      {key}: write\n')
                self.assertNotEqual(mutated, self.registry)
                with self.assertRaises(AssertionError):
                    cache_contract(self.policy, mutated)
            with self.subTest(verdict=key):
                mutated = self.policy.replace('      contents: read\n      pull-requests: read', f'      contents: read\n      {key}: write\n      pull-requests: read')
                self.assertNotEqual(mutated, self.policy)
                with self.assertRaises(AssertionError):
                    cache_contract(mutated, self.registry)

    def verify_environment(self, root, hit, source='a' * 40, version_source=None, symlink=False,
                           gh_exit=0, write_binary=True, dirty='false', sleep_version=False):
        binary_dir = root / 'cache'
        binary_dir.mkdir()
        binary = binary_dir / 'codeflow'
        version_source = source if version_source is None else version_source
        if write_binary:
            text = fake_version(version_source, dirty)
            if sleep_version:
                text = '#!/bin/sh\nsleep 5\n'
            if symlink:
                real = root / 'real'
                real.write_text(text)
                real.chmod(0o755)
                binary.symlink_to(real)
            else:
                binary.write_text(text)
                binary.chmod(0o755)
        gh = root / 'gh'
        gh.write_text(f'#!/bin/sh\necho "$@" >> "{root}/gh-args"\nexit {gh_exit}\n')
        gh.chmod(0o755)
        timeout = root / 'timeout'
        timeout.write_text(f'#!/bin/sh\necho "$1" >> "{root}/timeouts"\nlimit=$1\nshift\nif [ "$limit" = 30 ] && [ -n "$FAST_TIMEOUT" ]; then limit=1; fi\nexec {shlex.quote(real_timeout())} "$limit" "$@"\n')
        timeout.chmod(0o755)
        path_file = root / 'path'
        output = root / 'output'
        env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}', AUTHORITY_SHA=source, CACHE_HIT=hit,
                   BINARY_DIR=str(binary_dir), GITHUB_WORKSPACE=str(root), GITHUB_PATH=str(path_file),
                   GITHUB_OUTPUT=str(output), GITHUB_REPOSITORY='sathyassn/codeflow', GH_TOKEN='token',
                   FAST_TIMEOUT='1' if sleep_version else '')
        return binary, path_file, output, env

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_only_a_verified_attestation_lets_a_restored_binary_run(self):
        run = script(find_step(jobs_of(self.policy)['commit-lint'], 'Verify the authority binary'))
        sha = 'a' * 40
        cases = {
            'verified': dict(hit='true', ok=True),
            'miss': dict(hit='false', ok=False, warning=False),
            'unset': dict(hit='', ok=False, warning=False),
            'attestation fails': dict(hit='true', ok=False, gh_exit=1),
            'attestations unavailable for the repository': dict(hit='true', ok=False, gh_exit=1),
            'wrong version source': dict(hit='true', ok=False, version_source='b' * 40),
            'malformed version source': dict(hit='true', ok=False, version_source='malformed'),
            'dirty': dict(hit='true', ok=False, dirty='true'),
            'symlink': dict(hit='true', ok=False, symlink=True),
            'no file': dict(hit='true', ok=False, write_binary=False),
            'version hangs': dict(hit='true', ok=False, sleep_version=True),
        }
        for name, case in cases.items():
            case = dict(case)
            ok = case.pop('ok')
            warning = case.pop('warning', not ok)
            with self.subTest(case=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary, path_file, output, env = self.verify_environment(root, source=sha, **case)
                result = run_bash(run, root, env)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(output.read_text().strip(), f'verified={str(ok).lower()}')
                self.assertEqual(('::warning::' in result.stdout), warning, result.stdout)
                if ok:
                    self.assertTrue(binary.exists())
                    self.assertIn(f'source={sha}', subprocess.check_output([str(root / 'target/release/codeflow'), '--version'], text=True))
                    self.assertEqual(path_file.read_text().strip(), f'{root}/target/release')
                    args = (root / 'gh-args').read_text()
                    for part in ('attestation verify', '--repo sathyassn/codeflow',
                                 '--cert-identity https://github.com/sathyassn/codeflow/.github/workflows/codeflow-registry.yml@refs/heads/main',
                                 '--source-ref refs/heads/main', f'--source-digest {sha}', f'--signer-digest {sha}',
                                 '--deny-self-hosted-runners'):
                        self.assertIn(part, args)
                    self.assertEqual((root / 'timeouts').read_text().split(), ['60', '30'])
                else:
                    self.assertFalse(binary.parent.exists(), 'a binary that did not verify is deleted')
                    self.assertFalse((root / 'target/release/codeflow').exists())
                    self.assertFalse(path_file.exists(), 'a binary that did not verify must not enter PATH')
                if hit_is_false(case):
                    self.assertFalse((root / 'gh-args').exists(), 'no restored file means nothing to verify')
                if name == 'attestation fails' or name == 'attestations unavailable for the repository':
                    self.assertFalse((root / 'timeouts').read_text().split() == ['60', '30'], 'the binary never runs')

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_a_binary_whose_attestation_fails_never_runs(self):
        run = script(find_step(jobs_of(self.policy)['release-impact'], 'Verify the authority binary'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary, _path, output, env = self.verify_environment(root, hit='true', gh_exit=1)
            ran = root / 'ran'
            binary.write_text(f'#!/bin/sh\ntouch "{ran}"\necho "codeflow 4.0.0 source={"a" * 40} dirty=false inputs=abc"\n')
            result = run_bash(run, root, env)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(ran.exists(), 'an unverified binary must not be executed')
            self.assertEqual(output.read_text().strip(), 'verified=false')

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_the_rebuild_checks_the_embedded_identity(self):
        run = script(find_step(jobs_of(self.policy)['release-impact'], 'Build the authority binary'))
        sha = 'a' * 40
        for source, dirty, ok in ((sha, 'false', True), ('wrong', 'false', False), (sha, 'true', False), ('', 'false', False)):
            with self.subTest(source=source, dirty=dirty), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cargo = root / 'cargo'
                version = f'codeflow 4.0.0 source={source} dirty={dirty} inputs=abc' if source else 'garbled'
                cargo.write_text('#!/bin/sh\nmkdir -p target/release\n'
                                 f'printf \'#!/bin/sh\\necho "{version}"\\n\' > target/release/codeflow\n'
                                 'chmod +x target/release/codeflow\n')
                cargo.chmod(0o755)
                path_file = root / 'path'
                env = dict(os.environ, PATH=f'{root}{os.pathsep}{os.environ["PATH"]}', AUTHORITY_SHA=sha,
                           GITHUB_WORKSPACE=str(root), GITHUB_PATH=str(path_file))
                result = run_bash(run, root, env)
                if ok:
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertEqual(path_file.read_text().strip(), f'{root}/target/release')
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('::error::the rebuilt authority binary does not report', result.stdout)
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
                result = run_bash(run, root, env)
                if expect_ok:
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertEqual(output.read_text().strip(), 'built=true')
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(output.exists(), 'a wrong identity must not enable the save')

    def test_removing_the_binary_restore_leaves_the_dependency_cache(self):
        # The removal recipe: delete the restore and verify steps and the writer's attest and save steps,
        # and the dependency steps still stand on their own with no reference to the binary entry.
        for job in verdict_jobs_of(self.policy).values():
            deps = uses_steps(job, 'Swatinem/rust-cache')[0]
            self.assertNotIn('authority-cache', deps)
            self.assertNotIn(NAMESPACE, deps)
            self.assertIn('prefix-key: ' + DEPS_NAMESPACE, deps)
        writer = jobs_of(self.registry)[WRITER_JOB]
        self.assertNotIn('main-cache', uses_steps(writer, 'Swatinem/rust-cache')[0].replace("if: steps.main-cache.outputs.cache-hit != 'true'", ''))
        self.assertNotIn(NAMESPACE, uses_steps(writer, 'Swatinem/rust-cache')[0])

    @unittest.skipIf(os.name == "nt", "the cache scripts run in Linux bash")
    def test_key_changes_with_commit_toolchain_and_image_and_refuses_a_moved_or_dirty_checkout(self):
        scripts = [script(find_step(job, 'Identify the authority build')) for job in verdict_jobs_of(self.policy).values()]
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
                result = run_bash(run, root, env)
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


def hit_is_false(case):
    return case.get('hit') != 'true'


if __name__ == '__main__':
    unittest.main()

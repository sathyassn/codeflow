#!/usr/bin/env python3
"""Targeted structural checks for the candidate gate and CI trust boundaries."""
import importlib.util
import json
import os
import re
import subprocess
import tomllib
from pathlib import Path
import unittest

ROOT = Path(os.environ.get('CODEFLOW_GATE_CONTRACT_ROOT', Path(__file__).resolve().parents[1]))
_SPEC = importlib.util.spec_from_file_location('gate_parity', Path(__file__).resolve().parent / 'gate-parity.py')
PARITY = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(PARITY)
def read(path):
    return (ROOT / path).read_text()

class GateContracts(unittest.TestCase):
    def test_publication_requires_the_five_existing_floors(self):
        checks = json.loads(read('.release/config.json'))['required_publication_checks']
        self.assertEqual(set(checks), {'codeflow gates', 'windows', 'secret scan', 'security review', 'release state'})
        self.assertEqual(len(checks), 5)

    def test_windows_jobs_are_a_gate_behind_one_verdict(self):
        # Native Windows is a release target (TSK-197). TSK-203 splits the
        # referee into test partitions, the other checks and the journey gate
        # over every partition's results. TSK-265 builds the suite once into
        # an archive the partitions run, and runs the Python journeys beside
        # the partitions. None of them may forgive a failure, and one
        # `windows` verdict, never skipped, passes only when all of them
        # succeeded, or when the push reuse decision named a run and all were
        # skipped. Publication requires that verdict by its name.
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        jobs = dict(zip(parts[1::2], parts[2::2]))
        windows = {key: text for key, text in jobs.items() if key.startswith('windows')}
        gated = ('windows', 'windows-build', 'windows-tests', 'windows-journeys', 'windows-release-journey')
        self.assertEqual(set(windows), {*gated, 'windows-verdict'})
        checks = json.loads(read('.release/config.json'))['required_publication_checks']
        for key in gated:
            with self.subTest(job=key):
                job = windows[key]
                self.assertIn('runs-on: windows-latest', job)
                self.assertNotIn('continue-on-error', job)
                name = job.split('name: ', 1)[1].split('\n', 1)[0]
                self.assertNotIn('advisory', name)
                self.assertNotIn(name, checks)
        # AC-1: the job after the partitions reads results only; the Python
        # journeys run in a job that starts with the run.
        journeys = windows['windows-journeys']
        self.assertIn('journey-gate.py --results target/nextest/partitions --skip-benchmark --part results', journeys)
        self.assertIn('windows-partitions.py check --list', journeys)
        self.assertNotIn('test_release.py', journeys)
        self.assertIn('needs: windows-tests', journeys)
        self.assertIn('pattern: windows-junit-*', journeys)
        release = windows['windows-release-journey']
        self.assertIn('journey-gate.py --part python --skip-benchmark', release)
        self.assertIn('    needs: reuse\n', release)
        # AC-2: one build, and partitions that run the archive without compiling.
        self.assertIn('cargo nextest archive --workspace --profile codeflow', windows['windows-build'])
        tests = windows['windows-tests']
        self.assertIn('name: windows-junit-${{ matrix.partition }}', tests)
        self.assertIn('    needs: windows-build\n', tests)
        self.assertIn('cargo nextest run --archive-file', tests)
        self.assertNotIn('rust-cache', tests)
        self.assertNotIn('cargo build', tests)
        self.assertNotIn('--workspace --no-fail-fast', tests)
        # The verdict is pinned exactly in scripts/gate-parity.py, as
        # `codeflow gates` is: one accepted body, so no edit can add another
        # way for it to pass.
        self.assertEqual(PARITY.windows_verdict_problems(workflow), [])
        self.assertEqual(windows['windows-verdict'].count('exit 0'), 1)
        self.assertEqual([check for check in checks if 'windows' in check], ['windows'])

    def test_an_extra_way_for_the_windows_verdict_to_pass_is_refused(self):
        workflow = read('.github/workflows/codeflow-ci.yml')
        guard = '          if [ "$CHECKS" != success ] || [ "$BUILD" != success ]'
        self.assertEqual(workflow.count(guard), 1)
        controls = {
            'an inserted exit 0': workflow.replace(guard, '          if [ "$TESTS" = failure ]; then\n'
                                                          '            exit 0\n          fi\n' + guard),
            'reuse without skipped jobs': workflow.replace(
                '[ "$REUSE" = success ] && [ "$CHECKS $BUILD $TESTS $JOURNEYS $RELEASE" = '
                '"skipped skipped skipped skipped skipped" ]', '[ "$REUSE" = success ]'),
            'a dropped job result': workflow.replace(' || [ "$RELEASE" != success ]', ''),
            'continue-on-error': workflow.replace('    name: windows\n', '    name: windows\n    continue-on-error: true\n'),
        }
        for label, changed in controls.items():
            with self.subTest(label):
                self.assertNotEqual(changed, workflow)
                problems = PARITY.windows_verdict_problems(changed)
                self.assertTrue(any('must be exactly the Windows verdict job' in p for p in problems), problems)
        renamed = workflow.replace('    name: windows\n', '    name: windows verdict\n')
        self.assertTrue(any('exactly one job must be named' in p for p in PARITY.windows_verdict_problems(renamed)))

    def test_only_a_push_reuses_a_verdict_and_the_heavy_jobs_follow_it(self):
        # TSK-265 AC-4: pull request runs never reuse; every gate and Windows
        # job before the verdicts is skipped exactly when the decision names
        # a run, and the decision's token reads only.
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        jobs = dict(zip(parts[1::2], parts[2::2]))
        reuse = jobs['reuse']
        # TSK-1001 AC-3: besides a push, only a ready pull request's `edited`
        # run that keeps its base asks the decision; a base change runs all.
        self.assertTrue(reuse.startswith(
            "    if: github.event_name == 'push' || github.event_name == 'pull_request' && "
            "github.event.action == 'edited' && !github.event.pull_request.draft && "
            "github.event.changes.base.ref.from == ''\n"), reuse)
        self.assertIn('python3 -I scripts/ci-reuse.py edit --repo', reuse)
        self.assertIn('    permissions:\n      contents: read\n      actions: read\n      pull-requests: read\n', reuse)
        # The one write permission is the provenance job's OIDC token
        # request (TSK-1001); nothing writes to the repository.
        self.assertEqual(re.findall(r'^ +([a-z-]+): write$', workflow, re.M), ['id-token'])
        self.assertIn('      contents: read\n      id-token: write\n', jobs['provenance'])
        self.assertIn('python3 -I scripts/ci-reuse.py', reuse)
        for key in ('gates', 'windows', 'windows-build', 'windows-release-journey'):
            with self.subTest(job=key):
                self.assertIn("    if: ${{ !cancelled() && needs.reuse.outputs.run == '' && (", jobs[key])
                self.assertIn('    needs: [reuse, gates-build]\n' if key == 'gates' else '    needs: reuse\n', jobs[key])
        self.assertIn("needs.windows-build.result == 'success'", jobs['windows-tests'])
        self.assertIn("needs.windows-tests.result != 'skipped'", jobs['windows-journeys'])

    # TSK-254: pull request runs share one group per pull request and cancel
    # the run they supersede; push, schedule and dispatch runs each get a
    # group of their own run id and are never cancelled, so a registry run is
    # never replaced. The exact text is pinned: a group keyed on the ref, or a
    # cancel that is not limited to pull_request_target, would cancel
    # registry runs.
    POLICY_CONCURRENCY = (
        "\nconcurrency:\n"
        "  group: ${{ github.event_name == 'pull_request_target' && format('{0}-pr-{1}', github.workflow, "
        "github.event.pull_request.number) || format('{0}-run-{1}', github.workflow, github.run_id) }}\n"
        "  cancel-in-progress: ${{ github.event_name == 'pull_request_target' }}\n\n"
    )
    # The judging run reads the body when it runs, so the newest body is
    # judged whichever event's run survives the group.
    POLICY_BODY = (
        "          curl -fsSL --retry 3 --max-time 60 \\\n"
        "            -H \"Authorization: Bearer ${GH_TOKEN}\" -H \"Accept: application/vnd.github+json\" \\\n"
        "            \"${GITHUB_API_URL}/repos/${GITHUB_REPOSITORY}/pulls/${PR_NUMBER}\" -o \"${RUNNER_TEMP}/pull.json\"\n"
    )

    def assert_policy_cancels_only_superseded_pull_request_runs(self, workflow, admission=False):
        self.assertEqual(workflow.count('concurrency:'), 1, 'one workflow-level group, no job-level one')
        self.assertIn(self.POLICY_CONCURRENCY, workflow)
        self.assertLess(workflow.index(self.POLICY_CONCURRENCY), workflow.index('\njobs:\n'))
        self.assertIn(self.POLICY_BODY, workflow)
        self.assertIn('          GH_TOKEN: ${{ github.token }}\n', workflow)
        self.assertIn('          CODEFLOW_PR_BODY: ${{ steps.body.outputs.text }}\n', workflow)
        self.assertNotIn('github.event.pull_request.body', workflow)
        actions = '      actions: read\n' if admission else ''
        self.assertIn(f'    permissions:\n      contents: read\n      pull-requests: read\n{actions}    steps:\n', workflow)
        self.assertNotIn(': write', workflow)
        self.assertIn('    timeout-minutes: 45\n' if admission else '    timeout-minutes: 20\n', workflow)

    def test_one_target_source_build_judges_policy_and_registry_and_cancels_only_pr_runs(self):
        workflow = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('pull_request_target:', workflow)
        self.assertIn('github.event.pull_request.base.sha', workflow)
        self.assertEqual(workflow.count('cargo build --release'), 1)
        self.assertIn('codeflow ids check --base', workflow)
        self.assertIn('codeflow ci --base', workflow)
        self.assert_policy_cancels_only_superseded_pull_request_runs(workflow, admission=True)
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertNotIn('github.event.pull_request.head.sha }}\n          fetch-depth', workflow)
        self.assertFalse((ROOT / '.github/workflows/codeflow-registry.yml').exists())

    def test_commit_standards_admits_only_the_exact_heads_ready_run(self):
        # TSK-449 AC-6: merge admission is the required `commit standards`
        # check. It reruns when a draft is made ready, reads run records with
        # a read-only token from the trusted target checkout, and refuses a
        # draft at once (no wait) while a ready head waits for its run.
        workflow = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('    types: [opened, synchronize, reopened, edited, ready_for_review]\n', workflow)
        step = workflow.split("      - name: the exact head's ready codeflow-ci run passed\n", 1)[1]
        self.assertIn("        if: github.event_name == 'pull_request_target'\n", step)
        self.assertIn('python3 -I scripts/ci-reuse.py admit --repo "$REPOSITORY" --head "$HEAD_SHA" --pull "$PR_NUMBER"', step)
        self.assertIn('          PR_NUMBER: ${{ github.event.pull_request.number }}\n', step)
        self.assertIn('--workflow .github/workflows/codeflow-ci.yml \\\n', step)
        self.assertIn('--require "codeflow gates" --require windows --wait "$wait"', step)
        self.assertIn('          HEAD_SHA: ${{ github.event.pull_request.head.sha }}\n', step)
        self.assertIn('          if [ "$DRAFT" = true ]; then wait=0; fi\n', step)
        wait = int(re.search(r'^          wait=(\d+)$', step, re.M).group(1))
        self.assertLessEqual(wait, 40 * 60, 'the wait stays inside the 45-minute job bound')
        self.assertNotIn('pull_request.head.ref', step)

    def run_verdict(self, **env):
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        job = dict(zip(parts[1::2], parts[2::2]))['gates-verdict']
        lines = []
        for line in job.split('        run: |\n', 1)[1].splitlines():
            if line.strip() and not line.startswith(' ' * 10):
                break
            lines.append(line[10:])
        script = '\n'.join(lines)
        values = {'PARTS': 'success', 'RELEASE': 'success', 'SHARDS': 'success', 'REPORT': 'success',
                  'REUSE': 'skipped', 'REUSED': '', 'RELEASE_OWED': 'false', 'GITHUB_SERVER_URL': 'https://x',
                  'GITHUB_REPOSITORY': 'o/r', 'PATH': os.environ.get('PATH', '')}
        values.update(env)
        return subprocess.run(['bash', '-c', script], env=values, capture_output=True, text=True)

    def test_the_verdict_refuses_a_skipped_release_part_the_event_owed(self):
        # TSK-1001 AC-4: the release parts are owed on the release path;
        # there a skipped, failed or cancelled release job fails the verdict,
        # while an integration pull request may skip them.
        workflow = read('.github/workflows/codeflow-ci.yml')
        owed = re.search(r'^          RELEASE_OWED: \$\{\{ (.+) \}\}$', workflow, re.M).group(1)
        release_if = re.search(r'^  gates-release:\n    if: \$\{\{ (.+) \}\}$', workflow, re.M).group(1)
        self.assertIn(owed.replace("github.event_name == 'pull_request' && ",
                                   "github.event_name == 'pull_request' && !github.event.pull_request.draft && "),
                      release_if, 'the verdict owes the release parts exactly where gates-release runs')
        for result in ('skipped', 'failure', 'cancelled'):
            with self.subTest(result=result):
                done = self.run_verdict(RELEASE=result, RELEASE_OWED='true')
                self.assertEqual(done.returncode, 1, done.stdout)
                self.assertIn('owes the release parts', done.stdout)
        self.assertEqual(self.run_verdict(RELEASE='skipped').returncode, 0)
        self.assertEqual(self.run_verdict(RELEASE='success', RELEASE_OWED='true').returncode, 0)
        self.assertEqual(self.run_verdict(RELEASE='failure').returncode, 1)
        reused = self.run_verdict(REUSE='success', REUSED='77', PARTS='skipped', RELEASE='skipped',
                                  SHARDS='skipped', REPORT='skipped', RELEASE_OWED='true')
        self.assertEqual(reused.returncode, 0, reused.stdout)

    def test_runs_carry_their_base_and_a_retarget_reruns_the_gates(self):
        # TSK-1001 AC-2, AC-3: the run title names the pull request, base
        # branch and base commit as admission parses it; `edited` reruns
        # on a retarget, and only an edit that keeps the base waits instead
        # of cancelling the running gate.
        workflow = read('.github/workflows/codeflow-ci.yml')
        spec = importlib.util.spec_from_file_location('ci_reuse', ROOT / 'scripts' / 'ci-reuse.py')
        reuse = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(reuse)
        # Double-quoted, or YAML reads ` #` as a comment and GitHub refuses
        # the file before any job starts (run 40179230387).
        self.assertIn('run-name: "${{ github.event_name == \'pull_request\' && format(\'codeflow-ci pull request #{0} '
                      'into {1} at {2}\', github.event.pull_request.number, github.event.pull_request.base.ref, '
                      'github.event.pull_request.base.sha) || \'\' }}"\n', workflow)
        # No top-level or job-level plain scalar of the workflow holds ` #`
        # outside a comment line or a quoted value.
        for number, line in enumerate(workflow.splitlines(), 1):
            text = line.lstrip()
            if text.startswith('#') or ': ' not in text:
                continue
            value = text.split(': ', 1)[1]
            if value[:1] in ('"', "'", '|', '>'):
                continue
            self.assertNotIn(' #', value.split(' # ', 1)[0] if '${{' not in value else value,
                             f'line {number}: a plain value holds " #", which YAML reads as a comment')
        title = 'codeflow-ci pull request #{0} into {1} at {2}'.format(5, 'integration/EPC-1-x', 'a' * 40)
        self.assertEqual(reuse.run_title({'display_title': title}), (5, 'integration/EPC-1-x', 'a' * 40))
        self.assertIn('    types: [opened, synchronize, reopened, ready_for_review, edited]\n', workflow)
        self.assertIn("  cancel-in-progress: ${{ github.event_name == 'pull_request' && (github.event.action != "
                      "'edited' || github.event.changes.base.ref.from != '') }}\n", workflow)
        policy = read('.github/workflows/codeflow-policy.yml')
        step = policy.split("      - name: the exact head's ready codeflow-ci run passed\n", 1)[1]
        self.assertIn('--trusted-file .github/workflows/codeflow-ci.yml --trusted-file scripts/ci-reuse.py', step)

    def test_the_adopter_gate_runs_quick_only_targets(self):
        # TSK-1001 AC-14: a target that defines `quick` but not the mode the
        # hosted gate runs lost its only automatic run with the pre-push
        # quick gate; the shipped workflow runs exactly those after the gate.
        import tempfile
        workflow = read('assets/base/ci/codeflow-ci.yml')
        line = re.search(r"^          codeflow_quick_only=\$\(python3 -I -c '(.+)' \"\$codeflow_mode\" 2>/dev/null \|\| true\)$",
                         workflow, re.M)
        self.assertIsNotNone(line)
        self.assertIn('          if [ -n "$codeflow_quick_only" ]; then\n'
                      '            "$codeflow_bin" test --strict --mode quick --only "$codeflow_quick_only"\n', workflow)
        self.assertLess(workflow.index('"$codeflow_bin" test --strict --mode "$codeflow_mode"'),
                        workflow.index('codeflow_quick_only='))
        self.assertEqual(read('.codeflow/.baseline/.github/workflows/codeflow-ci.yml'), workflow)
        config = {'targets': [
            {'name': 'lint', 'modes': {'quick': {}}},
            {'name': 'unit', 'modes': {'quick': {}, 'essential': {}, 'full': {}}},
            {'name': 'slow', 'modes': {'full': {}}},
            {'name': 'off', 'enabled': False, 'modes': {'quick': {}}},
            {'name': 'smoke', 'modes': {'quick': {}, 'full': {}}}]}
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, '.codeflow').mkdir()
            Path(directory, '.codeflow/test-config.json').write_text(json.dumps(config))
            def names(mode):
                return subprocess.run(['python3', '-I', '-c', line.group(1), mode], cwd=directory,
                                      capture_output=True, text=True, check=True).stdout.strip()
            self.assertEqual(names('essential'), 'lint,smoke')
            self.assertEqual(names('full'), 'lint')

    def test_release_impact_judges_the_candidate_with_the_targets_definitions(self):
        # TSK-1001: `release impact (trusted)` runs on pull_request_target
        # from the target's base commit, builds only that checkout's
        # codeflow, runs only its release.py, fetches the candidate as git
        # data, and pins and rechecks head, base and body. During the
        # transition the existing `pull_request` job keeps reporting
        # `release impact` unchanged; TSK-1015 removes it once main carries
        # the trusted job.
        workflow = read('.github/workflows/codeflow-release.yml')
        self.assertIn('  pull_request_target:\n    types: [opened, synchronize, reopened, edited, ready_for_review]\n',
                      workflow)
        self.assertIn('  pull_request:\n    types: [opened, synchronize, reopened, edited]\n', workflow)
        legacy = workflow.split('  release-impact:\n', 1)[1].split('\n  release-impact-trusted:\n', 1)[0]
        self.assertIn('    name: release impact\n', legacy)
        self.assertIn("    if: github.event_name == 'pull_request'\n", legacy)
        job = workflow.split('  release-impact-trusted:\n', 1)[1].split('\n  release-state:\n', 1)[0]
        self.assertIn('    name: release impact (trusted)\n', job)
        self.assertIn("    if: github.event_name == 'pull_request_target'\n", job)
        checkouts = re.findall(r'uses: actions/checkout@[0-9a-f]{40}[^\n]*\n        with:\n          ref: ([^\n]+)\n', job)
        self.assertEqual(checkouts, ['${{ github.event.pull_request.base.sha }}'])
        self.assertEqual(job.count('uses: actions/checkout@'), 1)
        self.assertNotIn('pull_request.head.ref', job)
        self.assertNotIn('github.event.pull_request.body', job)
        self.assertNotIn(': write', job)
        self.assertIn('"+refs/pull/${PR_NUMBER}/head:refs/codeflow/pr-head"', job)
        # Nothing of the candidate is checked out, built or run.
        for forbidden in ('git checkout', 'git switch', 'git worktree', 'refs/codeflow/pr-head:', '--root',
                          '--config', 'cd '):
            self.assertNotIn(forbidden, job.replace('refs/codeflow/pr-head"', ''), forbidden)
        self.assertEqual(job.count('cargo build --release --locked -p codeflow-cli'), 1)
        steps = [name for name in re.findall(r'^      - name: (.+)$', job, re.M)]
        self.assertEqual(steps[-3:], ['Pin the head, base and body judged', 'Validate reviewed release intent',
                                      'Recheck the pinned snapshot before reporting'])
        self.assertIn('python3 -I scripts/release.py pr-snapshot', job)
        self.assertIn('--expect "$RUNNER_TEMP/release-snapshot.json"', job)
        self.assertIn('--body-file "$RUNNER_TEMP/release-body.md"', job)
        self.assertIn('--codeflow-bin target/release/codeflow', job)
        # Merge admission binds the base and body the policy job judged.
        policy = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('--base-ref "$BASE_REF" --base-sha "$BASE_SHA" --body-sha256 "$BODY_SHA256"', policy)
        self.assertIn('          BODY_SHA256: ${{ steps.body.outputs.sha256 }}\n', policy)

    def test_every_ready_run_keeps_its_signed_provenance(self):
        # TSK-1001: each ready pull_request run asks GitHub for its OIDC
        # token with the admission audience and keeps it as an artifact
        # named for its attempt, which admission and edit reuse verify.
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        job = dict(zip(parts[1::2], parts[2::2]))['provenance']
        self.assertIn("    if: ${{ github.event_name == 'pull_request' && !github.event.pull_request.draft }}\n", job)
        self.assertIn('&audience=codeflow-admission', job)
        self.assertIn('name: codeflow-provenance-${{ github.run_attempt }}', job)
        spec = importlib.util.spec_from_file_location('ci_reuse', ROOT / 'scripts' / 'ci-reuse.py')
        reuse = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(reuse)
        self.assertEqual(reuse.PROVENANCE_AUDIENCE, 'codeflow-admission')
        self.assertEqual(reuse.PROVENANCE_ARTIFACT.format(attempt=2), 'codeflow-provenance-2')
        reuse_job = dict(zip(parts[1::2], parts[2::2]))['reuse']
        self.assertIn('--trusted-file .github/workflows/codeflow-ci.yml --trusted-file scripts/ci-reuse.py', reuse_job)
        policy = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('          CODEFLOW_PR_IS_DRAFT: ${{ steps.body.outputs.draft }}\n', policy)
        self.assertIn('          DRAFT: ${{ steps.body.outputs.draft }}\n', policy)
        self.assertIn('--draft "$DRAFT"', policy)

    def test_release_tier_targets_leave_the_hosted_landing_gate(self):
        # TSK-449 AC-4: prose-exhaustive, read-benchmark, present-browser and
        # present-binary-delta run in gates-release, which runs on the release
        # candidate (a pull request into main or master), pushes to main or
        # master, the weekly schedule and manual runs, and never on a pull
        # request into integration/**. The landing gate parts name none of them.
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        jobs = dict(zip(parts[1::2], parts[2::2]))
        release_targets = ('prose-exhaustive', 'read-benchmark', 'present-browser', 'present-binary-delta')
        def named(job):
            return {n for line in re.findall(r'^\s*only:\s*(\S+)$', jobs[job], re.M) for n in line.split(',')}
        self.assertFalse(set(release_targets) & named('gates'))
        self.assertEqual(named('gates-release'), set(release_targets))
        condition = re.search(r'^    if: (.+)$', jobs['gates-release'], re.M).group(1)
        self.assertNotIn('integration/', condition)
        self.assertIn("contains(fromJSON('[\"main\",\"master\"]'), github.event.pull_request.base.ref)", condition)
        for event in ("github.event_name == 'schedule'", "github.event_name == 'workflow_dispatch'"):
            self.assertIn(event, condition)
        self.assertIn('    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n',
                      jobs['gates-verdict'])
        # The local full gate keeps them: the test config still gives them a
        # full mode, since a schedule tier would change the adopter schema.
        config = json.loads(read('.codeflow/test-config.json'))
        modes = {t['name']: set(t['modes']) for t in config['targets']}
        for name in release_targets:
            self.assertEqual(modes[name], {'full'}, name)

    def test_the_release_binary_is_built_once_and_windows_skips_fmt(self):
        # TSK-449 AC-5: one release build per workflow, shared by every gate
        # part; Windows runs clippy, doctests, rustdoc and the partitions,
        # and no fmt.
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        jobs = dict(zip(parts[1::2], parts[2::2]))
        builds = [name for name, text in jobs.items() if re.search(r'^\s*run: cargo build --release', text, re.M)]
        self.assertEqual(builds, ['gates-build'])
        self.assertIn('name: codeflow-release', jobs['gates'])
        self.assertNotIn('cargo build', jobs['gates'])
        windows = [name for name in jobs if name.startswith('windows')]
        runs = '\n'.join(re.findall(r'^\s*run: (.+)$', ''.join(jobs[n] for n in windows), re.M))
        self.assertNotIn('cargo fmt', runs)
        for command in ('cargo clippy --workspace --all-targets -- -D warnings', 'cargo test --workspace --doc',
                        'cargo nextest run --archive-file'):
            self.assertIn(command, runs)

    def test_the_adopter_policy_template_cancels_only_pr_runs_the_same_way(self):
        self.assert_policy_cancels_only_superseded_pull_request_runs(read('assets/base/ci/codeflow-policy.yml'))
        # Marking a draft ready reruns the standards, so a check that reads
        # the pull request at ready (its Testing citation) judges it then.
        self.assertIn('    types: [opened, synchronize, reopened, edited, ready_for_review]\n',
                      read('assets/base/ci/codeflow-policy.yml'))

    def test_release_reuses_cache_and_only_cancels_pr_events(self):
        workflow = read('.github/workflows/codeflow-release.yml')
        self.assertIn('Swatinem/rust-cache@', workflow)
        # Release impact runs on pull_request_target (TSK-1001): one group per
        # pull request, cancelled when superseded; push runs are never.
        # One group per pull request and event (the two release jobs never
        # cancel each other), cancelled when superseded; push runs never.
        self.assertIn("cancel-in-progress: ${{ github.event_name != 'push' }}", workflow)
        self.assertIn("format('{0}-{1}-pr-{2}', github.workflow, github.event_name, github.event.pull_request.number) "
                      "|| format('{0}-run-{1}', github.workflow, github.run_id)", workflow)
        self.assertIn('cargo build --release --locked -p codeflow-cli', workflow)

    def test_every_workflow_action_is_pinned_to_a_commit(self):
        # Issue 148: a tag can be moved, so a workflow with repository write
        # or pull request permissions must not run whatever a tag points at.
        # Every `uses:` in this repository's workflows and in the workflows
        # CodeFlow ships to adopters names a full 40-character commit, and a
        # shipped one names its release in a trailing comment.
        checked = {}
        for directory, shipped in (('.github/workflows', False), ('assets/base/ci', True)):
            for path in sorted((ROOT / directory).glob('*.y*ml')):
                text = path.read_text()
                if shipped and not re.search(r'^jobs:', text, re.M):
                    continue
                name = path.relative_to(ROOT).as_posix()
                checked[name] = text
                with self.subTest(workflow=name):
                    self.assertEqual(PARITY.pin_problems(text, name, shipped), [])
        for name in ('.github/workflows/codeflow-ci.yml', '.github/workflows/codeflow-policy.yml',
                     '.github/workflows/release.yml', 'assets/base/ci/codeflow-ci.yml',
                     'assets/base/ci/codeflow-policy.yml'):
            self.assertIn(name, checked)
            self.assertRegex(checked[name], r'uses: actions/checkout@[0-9a-f]{40}')
        # The check fails on the tag form the managed workflows used to carry.
        tagged = checked['assets/base/ci/codeflow-policy.yml'].replace(
            re.search(r'actions/checkout@[0-9a-f]{40} # v\d+\.\d+\.\d+', checked['assets/base/ci/codeflow-policy.yml']).group(0),
            'actions/checkout@v6')
        self.assertTrue(PARITY.pin_problems(tagged, 'assets/base/ci/codeflow-policy.yml', True))

    def test_release_fixture_bodies_name_a_task(self):
        source = read('scripts/fixtures/release_impact_cases.json')
        self.assertNotIn('Task: none', source)
        self.assertIn('Task: TSK-', source)

    def test_pipeline_verifies_targeted_and_quick_and_names_candidate_owner(self):
        for path in ['assets/base/claude/workflows/pipeline.workflow.js', '.claude/workflows/pipeline.workflow.js']:
            source = read(path)
            self.assertIn('affected targeted checks', source)
            # TSK-263: review keeps the quick gate; verify runs essential
            # where the config defines it, so the unattended path runs the
            # suite a crates change owes.
            self.assertIn('codeflow test --mode quick --strict', source)
            self.assertIn('codeflow test --mode essential --strict', source)
            # Pin each command to its stage, so swapping the two prompts fails.
            review = source[source.index("review: gate('review',"):source.index("qa: gate('qa',")]
            verify = source[source.index("qa: gate('qa',"):source.index('Then check every acceptance criterion')]
            self.assertIn('codeflow test --mode quick --strict', review)
            self.assertNotIn('--mode essential', review)
            self.assertIn(
                '`codeflow test --mode essential --strict` when a target in `.codeflow/test-config.json` '
                'defines an `essential` mode, else `codeflow test --mode quick --strict`',
                verify,
            )
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

    def test_essential_rust_keeps_doctests_and_full_gate_membership(self):
        targets = {t['name']: t for t in json.loads(read('.codeflow/test-config.json'))['targets']}
        workspace = targets['rust-workspace']
        self.assertEqual(set(workspace['modes']), {'essential'})
        command = workspace['modes']['essential']['command']
        self.assertEqual(workspace.get('shell'), 'sh')
        self.assertTrue(command.startswith('run_tmp=$(mktemp -d "${TMPDIR:-/tmp}/cf.XXXXXX") || exit; '))
        self.assertIn('trap \'status=$?; rm -rf -- "$run_tmp" || exit 1; exit "$status"\' EXIT;', command)
        self.assertIn('export TMPDIR="$run_tmp";', command)
        self.assertTrue(command.endswith('cargo nextest run --workspace --no-fail-fast --profile essential && cargo test --workspace --doc'))
        self.assertEqual(workspace['report'], {'format': 'junit', 'path': 'target/nextest/essential/junit.xml'})
        self.assertEqual(targets['rust-coverage']['modes'], {'full': {'command':
            'cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 90 --profile codeflow'}})
        self.assertEqual(targets['rust-doctest']['modes'], {'full': {'command': 'cargo test --workspace --doc'}})
        nextest = tomllib.loads(read('.config/nextest.toml'))
        profiles = nextest['profile']
        self.assertEqual(nextest['test-groups']['serial']['max-threads'], 1)
        self.assertNotIn('test-threads', profiles['essential'])
        self.assertEqual(json.loads(read('.codeflow/test-config.json'))['execution']['worker_budget'], 9)
        self.assertEqual(workspace['workers'], 9)
        self.assertEqual(profiles['essential']['overrides'], profiles['codeflow']['overrides'])
        self.assertFalse(profiles['essential']['fail-fast'])
        self.assertEqual(profiles['essential']['junit']['path'], 'junit.xml')
        self.assertNotIn('default-filter', profiles['essential'])
        for name in ('read-benchmark', 'prose-exhaustive'):
            self.assertEqual(set(targets[name]['modes']), {'full'})

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
        self.assertIn('rust-build', closure('codeflow-bin'))
        # The Python release journeys read PR bodies with the built binary at
        # import time, so a cold checkout needs its producer too.
        for name in ['rust-doctest', 'rustdoc', 'journey-python']:
            self.assertIn('codeflow-bin', closure(name), name)
        self.assertEqual(targets['rust-clippy']['env']['CARGO_TARGET_DIR'], 'target/clippy')
        for name in ['rust-build', 'codeflow-bin', 'rust-clippy', 'rustdoc', 'rust-doctest', 'repository-update-noop', 'contract-scans', 'present-binary-delta']:
            self.assertEqual(targets[name]['workers'], cfg['execution']['worker_budget'], name)
        # Quick keeps its membership and does not pull in the web build.
        # Its Cargo checks declare their need for heavy admission.
        quick = {name for name, t in targets.items() if 'quick' in t['modes']}
        self.assertEqual(quick, {'rust-format', 'rust-clippy', 'repository-update-noop', 'contract-scans', 'gate-parity', 'skill-triggers', 'herdr-delivery'})
        for name in quick:
            self.assertLessEqual(closure(name), quick, name)
        for name in ['rust-clippy', 'repository-update-noop', 'contract-scans']:
            self.assertTrue(targets[name]['heavy'], name)
        # TSK-256: the push set replays `codeflow update` on this repository, so a
        # shipped asset changed without a resync fails before review. The replay
        # runs from the quick mode only (essential and full already reach it
        # through the workspace test run) and requires no producer.
        replay = targets['repository-update-noop']
        self.assertEqual(replay['modes'], {'quick': {'command': 'cargo test -p codeflow-core --test repository_update_noop --quiet'}})
        self.assertEqual(replay.get('requires', []), [])
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/repository_update_noop.rs').is_file())
        # TSK-385: preserve one target while discovering core repository
        # contracts and checking the remedy catalogue and derived lookups.
        # Its runner controls use real Cargo and run in gate-parity too.
        scans = targets['contract-scans']
        self.assertEqual(scans['modes'], {'quick': {'command': 'python3 -B scripts/contract-scans.py'}})
        self.assertEqual(scans.get('requires', []), [])
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/git_spawn_contract.rs').is_file())
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/artifact_budget_contract.rs').is_file())
        for mode in ['quick', 'essential', 'full']:
            self.assertIn('python3 -B scripts/test_contract_scans.py',
                          targets['gate-parity']['modes'][mode]['command'])
        for name in ['present-browser', 'read-benchmark', 'present-web-build']:
            self.assertTrue(targets[name]['exclusive'], name)
        self.assertEqual(targets['journey-gate']['requires'], ['rust-coverage'])
        self.assertIn('--fail-under-lines 90', targets['rust-coverage']['modes']['full']['command'])
        self.assertIn('nextest', targets['rust-coverage']['modes']['full']['command'])
        self.assertNotIn('full', targets['rust-workspace']['modes'])
        # TSK-263: an essential run checks figure fidelity, so a capability
        # wording change fails before review; every prerequisite defines the
        # mode, or the config is refused at load.
        self.assertIn('workflow.mjs verify', targets['docs-portal']['modes']['essential']['command'])
        for name in closure('docs-portal'):
            self.assertIn('essential', targets[name]['modes'], name)
        self.assertEqual(targets['rust-doctest']['modes']['full']['command'], 'cargo test --workspace --doc')
        self.assertIn('npm run supply-chain', targets['present-supply-chain']['modes']['full']['command'])
        self.assertIn('check-present-binary-delta', targets['present-binary-delta']['modes']['full']['command'])
        self.assertIn('structural', cfg['_description'])

if __name__ == '__main__':
    unittest.main()

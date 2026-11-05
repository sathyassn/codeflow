#!/usr/bin/env python3
"""Targeted structural checks for the candidate gate and CI trust boundaries."""
import ast
import importlib.util
import json
import os
import re
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
        self.assertEqual(set(windows), {*gated, 'windows-verdict', 'windows-image', 'windows-tier', 'windows-tier-journeys'})
        checks = json.loads(read('.release/config.json'))['required_publication_checks']
        # TSK-1242: the tier only applies to pull requests. Neither job is a
        # publication check; `scripts/gate-parity.py` pins their text.
        for key in ('windows-tier', 'windows-tier-journeys'):
            self.assertNotIn(windows[key].split('name: ', 1)[1].split('\n', 1)[0], checks)
        self.assertIn("    if: github.event_name == 'pull_request'\n", windows['windows-tier'])
        # TSK-359: `windows-image` only reads the push's Windows runner image
        # for the reuse decision. It runs on pushes alone and is no gate.
        image = windows['windows-image']
        self.assertIn("    if: github.event_name == 'push'\n", image)
        self.assertNotIn('needs:', image)
        self.assertNotIn('continue-on-error', image)
        self.assertNotIn(image.split('name: ', 1)[1].split('\n', 1)[0], checks)
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
        # Two ways to pass: the reduced tier (TSK-1242) and a reused or full run.
        self.assertEqual(windows['windows-verdict'].count('exit 0'), 2)
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
        self.assertTrue(reuse.startswith("    if: github.event_name == 'push'\n"), reuse)
        self.assertIn('    permissions:\n      contents: read\n      actions: read\n      pull-requests: read\n', reuse)
        self.assertNotIn(': write', workflow)
        self.assertIn('python3 -I scripts/ci-reuse.py', reuse)
        for key in ('gates', 'windows', 'windows-release-journey'):
            with self.subTest(job=key):
                self.assertIn("    if: ${{ !cancelled() && needs.reuse.outputs.run == '' && (", jobs[key])
                self.assertIn('    needs: [reuse, gates-build]\n' if key == 'gates' else '    needs: reuse\n', jobs[key])
        # TSK-1242: the build also follows the tier, and still follows the reuse decision.
        self.assertIn("    if: ${{ !cancelled() && needs.windows-tier.outputs.tier != 'reduced' && needs.reuse.outputs.run == '' && (", jobs['windows-build'])
        self.assertIn('    needs: [reuse, windows-tier]\n', jobs['windows-build'])
        self.assertIn("needs.windows-build.result == 'success'", jobs['windows-tests'])
        self.assertIn("needs.windows-tests.result != 'skipped'", jobs['windows-journeys'])

    # TSK-1040: pull request runs share one group per pull request and queue
    # (`queue: max`, no cancellation), so no event cancels or replaces a run.
    # The policy workflow runs only on pull_request_target (TSK-336); push,
    # schedule and dispatch start codeflow-registry.yml, which has no group.
    # The exact text is pinned: a group keyed on the ref, or any cancellation,
    # would cancel the run that judges a pushed head.
    POLICY_CONCURRENCY = (
        "\nconcurrency:\n"
        "  group: ${{ format('{0}-pr-{1}', github.workflow, github.event.pull_request.number) }}\n"
        "  cancel-in-progress: false\n"
        "  queue: max\n\n"
    )
    # The judging run reads the body when it runs, so a run that waited in
    # the queue judges the newest body, whichever event started it.
    POLICY_BODY = (
        "          curl -fsSL --retry 3 --max-time 60 \\\n"
        "            -H \"Authorization: Bearer ${GH_TOKEN}\" -H \"Accept: application/vnd.github+json\" \\\n"
        "            \"${GITHUB_API_URL}/repos/${GITHUB_REPOSITORY}/pulls/${PR_NUMBER}\" -o \"${RUNNER_TEMP}/pull.json\"\n"
    )

    def assert_policy_queues_every_pull_request_run(self, workflow, admission=False):
        self.assertEqual(workflow.count('concurrency:'), 1, 'one workflow-level group, no job-level one')
        self.assertIn(self.POLICY_CONCURRENCY, workflow)
        self.assertLess(workflow.index(self.POLICY_CONCURRENCY), workflow.index('\njobs:\n'))
        self.assertIn(self.POLICY_BODY, workflow)
        self.assertIn('          GH_TOKEN: ${{ github.token }}\n', workflow)
        self.assertIn('          BODY_FILE: ${{ runner.temp }}/pr-body.md\n', workflow)
        self.assertIn('--pr-body-file "$BODY_FILE"', workflow)
        self.assertNotIn('CODEFLOW_PR_BODY', workflow)
        self.assertNotIn('github.event.pull_request.body', workflow)
        actions = '      actions: read\n' if admission else ''
        self.assertIn(f'    permissions:\n      contents: read\n      pull-requests: read\n{actions}    steps:\n', workflow)
        self.assertNotIn(': write', workflow)
        self.assertIn('    timeout-minutes: 45\n' if admission else '    timeout-minutes: 20\n', workflow)

    def test_each_verdict_job_builds_the_authority_and_the_registry_runs_apart(self):
        # TSK-336: each trusted verdict job builds the default branch's own
        # source once; push, schedule and dispatch check the registry in their
        # own workflow, which builds the event commit once and has no group.
        workflow = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('pull_request_target:', workflow)
        self.assertIn('github.event.pull_request.base.sha', workflow)
        self.assertEqual(workflow.count('cargo build --release'), 2, 'trusted: commit standards and release impact')
        self.assertIn('codeflow ids check --base', workflow)
        self.assertIn('codeflow ci --base', workflow)
        self.assert_policy_queues_every_pull_request_run(workflow, admission=True)
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertNotIn('github.event.pull_request.head.sha }}\n          fetch-depth', workflow)
        for event in ('  push:', '  schedule:', '  workflow_dispatch:'):
            self.assertNotIn(event, workflow.split('\njobs:\n', 1)[0])
        registry = read('.github/workflows/codeflow-registry.yml')
        self.assertEqual(registry.count('cargo build --release'), 1)
        self.assertNotIn('concurrency:', registry)
        # Its one job is the registry check, never a `trusted:` verdict; its
        # comments may name the prefix to say why.
        self.assertEqual(re.findall(r'^    name: (.+)$', registry, re.M), ['registry check'])
        self.assertIn('run: codeflow ids check\n', registry)

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
        self.assertIn('--workflow .github/workflows/codeflow-ci.yml --require "codeflow gates" --require windows', step)
        self.assertIn('          HEAD_SHA: ${{ github.event.pull_request.head.sha }}\n', step)
        self.assertIn('          if [ "$DRAFT" = true ]; then wait=0; fi\n', step)
        wait = int(re.search(r'^          wait=(\d+)$', step, re.M).group(1))
        self.assertLessEqual(wait, 40 * 60, 'the wait stays inside the 45-minute job bound')
        self.assertNotIn('pull_request.head.ref', step)

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
        self.assert_policy_queues_every_pull_request_run(read('assets/base/ci/codeflow-policy.yml'))
        # Marking a draft ready reruns the standards, so a check that reads
        # the pull request at ready (its Testing citation) judges it then.
        self.assertIn('    types: [opened, synchronize, reopened, edited, ready_for_review]\n',
                      read('assets/base/ci/codeflow-policy.yml'))

    def test_release_reuses_cache_and_only_cancels_pr_events(self):
        workflow = read('.github/workflows/codeflow-release.yml')
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", workflow)
        self.assertIn("github.event_name == 'pull_request' && github.ref || github.run_id", workflow)
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

UNITTEST_CASES = {'TestCase', 'IsolatedAsyncioTestCase'}
UNITTEST_CLASS_DECORATORS = {'skip', 'skipIf', 'skipUnless', 'expectedFailure'}
SCOPES = (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)


def collection_problems(source, label):
    """Tests a module defines that unittest's loader might not run, read from
    the source alone (TSK-359). Nothing is imported or executed, so checking
    a file has no side effect such as a build.

    The check fails closed: a test file must use one plain layout, and
    anything outside it is refused rather than resolved.
    - A `test*` name outside a function body is bound only by one plain
      `def` directly in the body of a direct top-level class.
    - Each top-level class name is bound once in the whole module, at any
      depth, and is never the first argument of `setattr` or `delattr`.
    - No attribute of a top-level class is assigned or deleted anywhere,
      and no attribute named `test*` outside a function body.
    The one exception is `test_x = None` directly in a top-level class
    body, which drops a test that class inherits from another class in the
    file and does not define itself.
    - A class decorator is `unittest.skip*` or `unittest.expectedFailure`,
      and a class takes no keywords such as a metaclass.
    - No `load_tests` and no `from ... import *` anywhere.
    Within that layout, a test counts as collected when a top-level class
    whose bases reach `unittest.TestCase` resolves the test's name to that
    very definition through its method resolution order, so a mixin
    inherited by a concrete TestCase counts. Loader errors are not judged
    here: each file runs through unittest.main, which fails on one."""
    tree = ast.parse(source)
    problems = []

    def refuse(node, why):
        problems.append(f'{label}:{node.lineno} {why}')

    parents = {}
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            parents[child] = node

    def in_function(node):
        node = parents.get(node)
        while node is not None and not isinstance(node, SCOPES):
            node = parents.get(node)
        return node is not None

    def located(node):
        while not hasattr(node, 'lineno'):
            node = parents[node]
        return node

    unittest_modules, case_names = {'unittest'}, set()
    for node in tree.body:
        if isinstance(node, ast.Import):
            unittest_modules |= {a.asname or a.name for a in node.names if a.name == 'unittest'}
        elif isinstance(node, ast.ImportFrom) and node.module == 'unittest':
            case_names |= {a.asname or a.name for a in node.names if a.name in UNITTEST_CASES}

    classes = {}
    for stmt in tree.body:
        if isinstance(stmt, ast.ClassDef) and stmt.name not in classes:
            classes[stmt.name] = stmt
    class_nodes = {id(stmt) for stmt in tree.body if isinstance(stmt, ast.ClassDef)}

    def defined_tests(stmt):
        return {item.name for item in stmt.body
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and item.name.startswith('test')}

    def inherited_tests(stmt, seen=()):
        names = set()
        for expr in stmt.bases:
            if isinstance(expr, ast.Name) and expr.id in classes and expr.id not in seen:
                base = classes[expr.id]
                names |= defined_tests(base) | inherited_tests(base, seen + (expr.id,))
        return names

    # The one allowed exception: `test_x = None` directly in a top-level
    # class body drops a test the class inherits from a class in this file
    # and does not define itself. test_release.py uses it where a carrier
    # has no case to run.
    opt_outs = {}
    for stmt in classes.values():
        for item in stmt.body:
            if (isinstance(item, ast.Assign) and len(item.targets) == 1
                    and isinstance(item.targets[0], ast.Name)
                    and isinstance(item.value, ast.Constant) and item.value.value is None
                    and item.targets[0].id in inherited_tests(stmt) - defined_tests(stmt)):
                opt_outs[id(item.targets[0])] = item.targets[0].id

    def bound_names(node):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            return [node.name]
        if isinstance(node, ast.Name) and isinstance(node.ctx, (ast.Store, ast.Del)):
            return [node.id]
        if isinstance(node, ast.alias):
            return [node.asname or node.name.split('.')[0]]
        if isinstance(node, (ast.Global, ast.Nonlocal)):
            return list(node.names)
        if isinstance(node, (ast.ExceptHandler, ast.MatchAs, ast.MatchStar)):
            return [node.name] if node.name else []
        if isinstance(node, ast.MatchMapping):
            return [node.rest] if node.rest else []
        return []

    def plain_method(node):
        parent = parents.get(node)
        return (isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
                and id(parent) in class_nodes and node in parent.body)

    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and any(a.name == '*' for a in node.names):
            refuse(node, 'imports * and may rebind any name')
        if isinstance(node, ast.Attribute) and isinstance(node.ctx, (ast.Store, ast.Del)) and (
                (isinstance(node.value, ast.Name) and node.value.id in classes)
                or (node.attr.startswith('test') and not in_function(node))):
            refuse(node, f'assigns or deletes attribute {node.attr}')
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) \
                and node.func.id in {'setattr', 'delattr'} and node.args \
                and isinstance(node.args[0], ast.Name) and node.args[0].id in classes:
            refuse(node, f'{node.func.id} changes class {node.args[0].id}')
        for name in bound_names(node):
            if name == 'load_tests':
                refuse(located(node), 'binds load_tests; the guard cannot judge its collection')
            elif name in classes and node is not classes[name]:
                refuse(located(node), f'rebinds top-level class {name}')
            elif name.startswith('test') and not in_function(node) and not plain_method(node) \
                    and id(node) not in opt_outs:
                refuse(located(node), f'{name} is not a plain method of a top-level class')

    def is_unittest_case(expr):
        if isinstance(expr, ast.Attribute) and isinstance(expr.value, ast.Name):
            return expr.value.id in unittest_modules and expr.attr in UNITTEST_CASES
        return isinstance(expr, ast.Name) and expr.id in case_names

    def allowed_decorator(expr):
        target = expr.func if isinstance(expr, ast.Call) else expr
        return (isinstance(target, ast.Attribute) and isinstance(target.value, ast.Name)
                and target.value.id in unittest_modules and target.attr in UNITTEST_CLASS_DECORATORS)

    base_case = type('TestCaseRoot', (), {})
    built = {}
    for name, stmt in classes.items():
        for expr in stmt.decorator_list:
            if not allowed_decorator(expr):
                refuse(expr, f'class {name} has a decorator that may replace it')
        if stmt.keywords:
            refuse(stmt, f'class {name} takes keywords such as a metaclass')
        methods = {}
        for item in stmt.body:
            if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and item.name.startswith('test'):
                if item.name in methods:
                    refuse(item, f'{name}.{item.name} replaces an earlier definition')
                methods[item.name] = item
            elif isinstance(item, ast.Assign) and id(item.targets[0]) in opt_outs:
                methods[item.targets[0].id] = None
        bases = [base_case if is_unittest_case(expr) else built[expr.id]
                 for expr in stmt.bases
                 if is_unittest_case(expr) or (isinstance(expr, ast.Name) and expr.id in built)]
        try:
            built[name] = type(name, tuple(dict.fromkeys(bases)) or (object,), methods)
        except TypeError as error:
            refuse(stmt, f'class {name} has no consistent order: {error}')
    collected = set()
    for cls in built.values():
        if issubclass(cls, base_case):
            collected |= {id(getattr(cls, n)) for n in dir(cls) if n.startswith('test')}
    for name, stmt in classes.items():
        for item in stmt.body:
            if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) \
                    and item.name.startswith('test') and id(item) not in collected:
                refuse(item, f'{name}.{item.name} is never collected')
    return sorted(set(problems), key=lambda p: (int(p.split(':')[1].split()[0]), p))


def probe_count(source):
    """The number of tests unittest's own loader collects from probe text."""
    namespace = {'__name__': 'probe'}
    exec(compile(source, 'probe.py', 'exec'), namespace)
    module = type(unittest)('probe')
    module.__dict__.update(namespace)
    return unittest.defaultTestLoader.loadTestsFromModule(module).countTestCases()


CASE = 'import unittest\nclass Case(unittest.TestCase):\n'


class TestCollection(unittest.TestCase):
    """Every scripts/test_*.py file keeps the plain layout the collection
    guard accepts, and the guard refuses every other layout probed here
    (TSK-359). Each refused probe collects fewer tests than it defines when
    unittest's own loader reads it."""

    def problems(self, source):
        return collection_problems(source, 'probe.py')

    def assertRefused(self, source, expected, defined=1):
        self.assertEqual(self.problems(source), expected)
        self.assertLess(probe_count(source), defined)

    def test_every_scripts_test_file_keeps_the_plain_layout(self):
        files = sorted(Path(__file__).resolve().parent.glob('test_*.py'))
        self.assertTrue(files)
        for path in files:
            with self.subTest(file=path.name):
                self.assertEqual(collection_problems(path.read_text(), path.name), [])

    def test_a_test_under_a_plain_class_is_refused(self):
        self.assertRefused('import unittest\nclass Helper:\n    def test_lost(self):\n        pass\n',
                           ['probe.py:3 Helper.test_lost is never collected'])

    def test_a_nested_test_case_is_refused(self):
        source = CASE + ('    def test_runs(self):\n        pass\n    class Inner(unittest.TestCase):\n'
                         '        def test_lost(self):\n            pass\n')
        self.assertRefused(source, ['probe.py:6 test_lost is not a plain method of a top-level class'], 2)

    def test_a_class_inside_a_conditional_is_refused(self):
        source = 'import unittest\nif True:\n    class Helper:\n        def test_lost(self):\n            pass\n'
        self.assertRefused(source, ['probe.py:4 test_lost is not a plain method of a top-level class'])

    def test_a_class_replaced_by_a_later_one_of_the_same_name_is_refused(self):
        source = (CASE + '    def test_lost(self):\n        pass\n'
                  'class Case(unittest.TestCase):\n    def test_kept(self):\n        pass\n')
        self.assertRefused(source, ['probe.py:5 rebinds top-level class Case'], 2)

    def test_a_method_replaced_in_its_own_class_is_refused(self):
        source = CASE + '    def test_x(self):\n        pass\n    def test_x(self):\n        pass\n'
        self.assertRefused(source, ['probe.py:3 Case.test_x is never collected',
                                    'probe.py:5 Case.test_x replaces an earlier definition'], 2)

    def test_a_test_name_rebound_in_the_class_body_is_refused(self):
        source = CASE + '    def test_lost(self):\n        pass\n    test_lost = None\n'
        self.assertRefused(source, ['probe.py:5 test_lost is not a plain method of a top-level class'])

    def test_an_inherited_test_dropped_with_none_is_the_one_exception(self):
        base = CASE + '    def test_x(self):\n        pass\n'
        dropped = base + 'class Child(Case):\n    test_x = None\n'
        self.assertEqual(self.problems(dropped), [])
        self.assertEqual(probe_count(dropped), 1)
        self.assertEqual(self.problems(base + 'class Child(Case):\n    test_y = None\n'),
                         ['probe.py:6 test_y is not a plain method of a top-level class'])
        self.assertEqual(self.problems(base + 'class Child(Case):\n    test_x = 1\n'),
                         ['probe.py:6 test_x is not a plain method of a top-level class'])

    def test_a_class_attribute_assigned_from_outside_is_refused(self):
        source = CASE + '    def test_x(self):\n        pass\ndef f():\n    Case.test_x = None\n'
        self.assertEqual(self.problems(source), ['probe.py:6 assigns or deletes attribute test_x'])

    def test_a_class_rebound_at_top_level_is_refused(self):
        source = CASE + '    def test_lost(self):\n        pass\nCase = None\n'
        self.assertRefused(source, ['probe.py:5 rebinds top-level class Case'])

    def test_a_class_rebound_inside_a_conditional_is_refused(self):
        source = CASE + '    def test_lost(self):\n        pass\nif True:\n    Case = None\n'
        self.assertRefused(source, ['probe.py:6 rebinds top-level class Case'])

    def test_a_test_inside_an_except_handler_is_refused(self):
        source = ('import unittest\ntry:\n    raise ValueError\nexcept ValueError:\n    pass\nelse:\n    pass\n'
                  'try:\n    pass\nexcept ValueError:\n    class Case(unittest.TestCase):\n'
                  '        def test_lost(self):\n            pass\n')
        self.assertRefused(source, ['probe.py:12 test_lost is not a plain method of a top-level class'])

    def test_a_test_inside_a_match_arm_is_refused(self):
        source = ('import unittest\nmatch 0:\n    case 1:\n        class Case(unittest.TestCase):\n'
                  '            def test_lost(self):\n                pass\n')
        self.assertRefused(source, ['probe.py:5 test_lost is not a plain method of a top-level class'])

    def test_a_module_level_test_function_is_refused(self):
        self.assertRefused('def test_lost():\n    pass\n',
                           ['probe.py:1 test_lost is not a plain method of a top-level class'])

    def test_load_tests_is_refused(self):
        source = CASE + '    def test_x(self):\n        pass\ndef load_tests(loader, tests, pattern):\n    return tests\n'
        self.assertEqual(self.problems(source), ['probe.py:5 binds load_tests; the guard cannot judge its collection'])

    def test_a_replacing_class_decorator_or_setattr_is_refused(self):
        source = ('import unittest\ndef drop(cls):\n    return None\n@drop\nclass Case(unittest.TestCase):\n'
                  '    def test_lost(self):\n        pass\nsetattr(Case, "x", 1)\n')
        self.assertEqual(self.problems(source), ['probe.py:4 class Case has a decorator that may replace it',
                                                 'probe.py:8 setattr changes class Case'])

    def test_the_plain_layouts_in_use_are_accepted(self):
        sources = [
            ('import unittest\nclass Mixin:\n    def test_shared(self):\n        pass\n'
             'class Concrete(Mixin, unittest.TestCase):\n    pass\n'),
            ('import unittest as ut\nfrom unittest import TestCase as Base\nclass Root(ut.TestCase):\n    pass\n'
             'class Child(Root):\n    def test_a(self):\n        pass\nclass Other(Base):\n'
             '    def test_b(self):\n        pass\n'),
            ('import unittest\n@unittest.skipIf(False, "reason")\nclass Case(unittest.TestCase):\n'
             '    def test_x(self):\n        def test_inner():\n            pass\n'),
        ]
        for source in sources:
            with self.subTest(source=source[:40]):
                self.assertEqual(self.problems(source), [])
                self.assertGreater(probe_count(source), 0)


if __name__ == '__main__':
    unittest.main()

#!/usr/bin/env python3
"""Targeted structural checks for the candidate gate and CI trust boundaries."""
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
        self.assertTrue(reuse.startswith("    if: github.event_name == 'push'\n"), reuse)
        self.assertIn('    permissions:\n      contents: read\n      actions: read\n      pull-requests: read\n', reuse)
        self.assertNotIn(': write', workflow)
        self.assertIn('python3 -I scripts/ci-reuse.py', reuse)
        for key in ('gates', 'windows', 'windows-build', 'windows-release-journey'):
            with self.subTest(job=key):
                self.assertIn("    if: ${{ !cancelled() && needs.reuse.outputs.run == '' && (", jobs[key])
                self.assertIn('    needs: reuse\n', jobs[key])
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

    def assert_policy_cancels_only_superseded_pull_request_runs(self, workflow):
        self.assertEqual(workflow.count('concurrency:'), 1, 'one workflow-level group, no job-level one')
        self.assertIn(self.POLICY_CONCURRENCY, workflow)
        self.assertLess(workflow.index(self.POLICY_CONCURRENCY), workflow.index('\njobs:\n'))
        self.assertIn(self.POLICY_BODY, workflow)
        self.assertIn('          GH_TOKEN: ${{ github.token }}\n', workflow)
        self.assertIn('          CODEFLOW_PR_BODY: ${{ steps.body.outputs.text }}\n', workflow)
        self.assertNotIn('github.event.pull_request.body', workflow)
        self.assertIn('    permissions:\n      contents: read\n      pull-requests: read\n    steps:\n', workflow)
        self.assertNotIn(': write', workflow)
        self.assertIn('    timeout-minutes: 20\n', workflow)

    def test_one_target_source_build_judges_policy_and_registry_and_cancels_only_pr_runs(self):
        workflow = read('.github/workflows/codeflow-policy.yml')
        self.assertIn('pull_request_target:', workflow)
        self.assertIn('github.event.pull_request.base.sha', workflow)
        self.assertEqual(workflow.count('cargo build --release'), 1)
        self.assertIn('codeflow ids check --base', workflow)
        self.assertIn('codeflow ci --base', workflow)
        self.assert_policy_cancels_only_superseded_pull_request_runs(workflow)
        self.assertIn('Swatinem/rust-cache@', workflow)
        self.assertNotIn('github.event.pull_request.head.sha }}\n          fetch-depth', workflow)
        self.assertFalse((ROOT / '.github/workflows/codeflow-registry.yml').exists())

    def test_the_adopter_policy_template_cancels_only_pr_runs_the_same_way(self):
        self.assert_policy_cancels_only_superseded_pull_request_runs(read('assets/base/ci/codeflow-policy.yml'))

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
        self.assertEqual(workspace['modes'], {'essential': {'command':
            'cargo nextest run --workspace --no-fail-fast --profile essential && cargo test --workspace --doc'}})
        self.assertEqual(workspace['report'], {'format': 'junit', 'path': 'target/nextest/essential/junit.xml'})
        self.assertEqual(targets['rust-coverage']['modes'], {'full': {'command':
            'cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 90 --profile codeflow'}})
        self.assertEqual(targets['rust-doctest']['modes'], {'full': {'command': 'cargo test --workspace --doc'}})
        profiles = tomllib.loads(read('.config/nextest.toml'))['profile']
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
        # The quick gate stays light: no quick target pulls in a build producer
        # (fmt and clippy need no web build; there is no present build.rs).
        quick = {name for name, t in targets.items() if 'quick' in t['modes']}
        self.assertEqual(quick, {'rust-format', 'rust-clippy', 'repository-update-noop', 'contract-scans', 'gate-parity', 'skill-triggers', 'herdr-delivery'})
        for name in quick:
            self.assertLessEqual(closure(name), quick, name)
        # TSK-256: the push set replays `codeflow update` on this repository, so a
        # shipped asset changed without a resync fails before review. The replay
        # runs from the quick mode only (essential and full already reach it
        # through the workspace test run) and requires no producer.
        replay = targets['repository-update-noop']
        self.assertEqual(replay['modes'], {'quick': {'command': 'cargo test -p codeflow-core --test repository_update_noop --quiet'}})
        self.assertEqual(replay.get('requires', []), [])
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/repository_update_noop.rs').is_file())
        # TSK-261: the push set runs the cross-cutting source scans: the one that
        # judges every git spawn and the one that checks every instruction
        # artifact reference. A branch that holds a refused spawn or a stale
        # reference fails before push, not in hosted CI after a merge. Quick mode
        # only, no producer, like the replay.
        scans = targets['contract-scans']
        self.assertEqual(scans['modes'], {'quick': {'command': 'cargo test -p codeflow-core --test git_spawn_contract --test artifact_budget_contract --quiet'}})
        self.assertEqual(scans.get('requires', []), [])
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/git_spawn_contract.rs').is_file())
        self.assertTrue((ROOT / 'crates/codeflow-core/tests/artifact_budget_contract.rs').is_file())
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

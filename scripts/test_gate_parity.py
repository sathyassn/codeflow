#!/usr/bin/env python3
"""Controls for scripts/gate-parity.py (TSK-134, TSK-203).

Each control mutates a copy of the real test config and checks the guard
against the real CI workflow. The coverage and doctest pair stands for CI's
raw nextest plus doctests only when both checks run in CI and run the whole
suite, so every way of running less must read as drift.
"""

import copy
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)

CONFIG = json.loads(parity.CONFIG.read_text())
WORKFLOW = parity.WORKFLOW.read_text()
CI = parity.ci_rust_job_commands(WORKFLOW)
PORTAL = "docs-portal"
PRESENT = "present-browser"


def target(cfg: dict, name: str) -> dict:
    return next(t for t in cfg["targets"] if t["name"] == name)


def full_command(cfg: dict, name: str) -> str:
    return target(cfg, name).get("modes", {}).get("full", {}).get("command", "")


def mutated(change) -> dict:
    cfg = copy.deepcopy(CONFIG)
    change(cfg)
    return cfg


def coverage_name() -> str:
    return next(t["name"] for t in CONFIG["targets"]
                if full_command(CONFIG, t["name"]).startswith("cargo llvm-cov "))


def doctest_name() -> str:
    return next(t["name"] for t in CONFIG["targets"]
                if full_command(CONFIG, t["name"]) == parity.DOCTESTS)


class GateParityControls(unittest.TestCase):
    def assert_parity(self, cfg: dict) -> None:
        self.assertEqual(parity.local_rust_commands(cfg), CI)

    def assert_drift(self, cfg: dict) -> list[str]:
        notes: list[str] = []
        local = parity.local_rust_commands(cfg, notes)
        self.assertNotEqual(local, CI)
        self.assertNotIn(parity.SUITE, local)
        return notes

    def test_the_committed_config_is_at_parity(self):
        self.assert_parity(CONFIG)

    def test_removing_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: c["targets"].remove(target(c, n))))

    def test_disabling_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: target(c, n).update(enabled=False)))

    def test_skipping_either_half_in_ci_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: target(c, n).update(
                    ci_skip=True, ci_skip_reason="control")))

    def test_a_narrowed_coverage_selection_is_drift(self):
        for suffix in (" --lib", " -- --skip git_guard", " --features extra",
                       " --exclude codeflow-cli", " --no-default-features"):
            with self.subTest(suffix=suffix):
                def narrow(c, s=suffix):
                    target(c, coverage_name())["modes"]["full"]["command"] += s
                notes = self.assert_drift(mutated(narrow))
                self.assertTrue(any(parity.COVERAGE_FORM in n for n in notes), notes)

    def test_a_cwd_or_env_on_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            for field, value in (("cwd", "crates/codeflow-core"),
                                 ("env", {"RUSTFLAGS": "--cfg skip"})):
                with self.subTest(name=name, field=field):
                    self.assert_drift(mutated(
                        lambda c, n=name, f=field, v=value: target(c, n).update({f: v})))

    def test_a_changed_line_threshold_keeps_parity(self):
        def threshold(c):
            target(c, coverage_name())["modes"]["full"]["command"] = (
                "cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 80 --profile codeflow")
        self.assert_parity(mutated(threshold))

    def test_any_disabled_rust_check_is_drift(self):
        cfg = mutated(lambda c: target(c, "rust-clippy").update(enabled=False))
        self.assertNotEqual(parity.local_rust_commands(cfg), CI)


class NodePinControls(unittest.TestCase):
    """TSK-142 AC-1: each Node target runs on the version its version file
    pins, and every CI job that installs Node for that directory pins the
    same version, including the job that runs the full gate."""

    def problems(self, cfg: dict, workflow: str = WORKFLOW, root: Path = parity.ROOT):
        return parity.node_pin_problems(cfg, workflow, root)

    def assert_problem(self, problems: list[str], *needles: str) -> None:
        self.assertTrue(any(all(n in p for n in needles) for p in problems),
                        f"{needles} not in {problems}")

    def test_the_committed_config_and_workflow_agree(self):
        self.assertEqual(self.problems(CONFIG), [])

    def test_cmd_keeps_each_node_launcher_and_chain_together(self):
        for name in (PORTAL, PRESENT):
            with self.subTest(name=name):
                command = full_command(CONFIG, name)
                self.assertEqual(parity.cmd_segments(command), [command])
                pin, inner = parity.with_node_parts(command)
                self.assertIsNotNone(pin)
                self.assertIn(" && ", inner)

    def test_cmd_split_model_handles_quotes_operators_and_caret(self):
        self.assertEqual(parity.cmd_segments('echo "a&&b|c" ^& d'),
                         ['echo "a&&b|c" ^& d'])
        self.assertEqual(parity.cmd_segments('echo a && echo b || echo c | more'),
                         ['echo a', 'echo b', 'echo c', 'more'])

    def test_single_quoted_node_chain_is_drift(self):
        for name in (PORTAL, PRESENT):
            def old_form(cfg, n=name):
                command = full_command(cfg, n)
                pin, inner = parity.with_node_parts(command)
                self.assertIsNotNone(pin)
                target(cfg, n)["modes"]["full"]["command"] = (
                    f"python3 -B scripts/with-node.py {pin} '{inner}'")
            with self.subTest(name=name):
                problems = self.problems(mutated(old_form))
                self.assert_problem(problems, name, "double quotes")

    @unittest.skipUnless(os.name == "nt", "requires cmd.exe and the Windows CI Node install")
    def test_docs_portal_target_runs_through_cmd_on_windows(self):
        # The target ends with `$CODEFLOW_BIN validate --portal`, which
        # `codeflow test` supplies; run directly, the caller must.
        if not os.environ.get("CODEFLOW_BIN"):
            self.fail("set CODEFLOW_BIN to a built codeflow: the docs-portal target validates the portal with it")
        command = full_command(CONFIG, PORTAL)
        run = subprocess.run(f'cmd.exe /D /S /C "{command}"', cwd=ROOT,
                             capture_output=True, text=True, check=False)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("with-node: Node 24.18.0", run.stderr)

    def test_each_node_target_runs_through_its_version_file(self):
        for name, pin in ((PORTAL, "docs-portal/.node-version"),
                          (PRESENT, "crates/codeflow-present/web/.node-version")):
            with self.subTest(name=name):
                self.assertEqual(parity.node_pin(full_command(CONFIG, name)), pin)

    def test_a_node_target_without_a_pin_is_refused(self):
        for name in (PORTAL, PRESENT):
            def unpin(c, n=name):
                modes = target(c, n)["modes"]["full"]
                modes["command"] = parity.with_node_parts(modes["command"])[1]
            with self.subTest(name=name):
                self.assert_problem(self.problems(mutated(unpin)), name, "without a Node pin")

    def test_a_ci_pin_that_differs_from_the_version_file_is_refused(self):
        workflow = WORKFLOW.replace("node-version: 24.18.0", "node-version: 24.17.0")
        self.assert_problem(self.problems(CONFIG, workflow), "24.17.0",
                            "docs-portal/.node-version", "24.18.0")

    def test_a_full_gate_job_without_the_pinned_node_is_refused(self):
        gates, rest = WORKFLOW.split("\n  windows:\n", 1)
        step = gates[gates.index("      - uses: actions/setup-node@"):]
        step = step[:step.index("\n      - ", 1) + 1]
        self.assertIn("docs-portal/package-lock.json", step)
        workflow = gates.replace(step, "") + "\n  windows:\n" + rest
        self.assert_problem(self.problems(CONFIG, workflow), "gates",
                            "24.18.0", "docs-portal")

    def test_a_missing_or_malformed_version_file_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs-portal").mkdir()
            (root / "crates/codeflow-present/web").mkdir(parents=True)
            (root / "crates/codeflow-present/web/.node-version").write_text("26.4.0\n")
            self.assert_problem(self.problems(CONFIG, WORKFLOW, root),
                                "docs-portal/.node-version", "does not exist")
            (root / "docs-portal/.node-version").write_text("lts/*\n")
            self.assert_problem(self.problems(CONFIG, WORKFLOW, root),
                                "docs-portal/.node-version", "one exact Node version")

    def test_an_exact_engines_pin_must_match_the_version_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            web = root / "crates/codeflow-present/web"
            web.mkdir(parents=True)
            (root / "docs-portal").mkdir()
            (root / "docs-portal/.node-version").write_text("24.18.0\n")
            (web / ".node-version").write_text("26.4.0\n")
            (web / "package.json").write_text(json.dumps({"engines": {"node": "26.3.0"}}))
            self.assert_problem(self.problems(CONFIG, WORKFLOW, root),
                                "engines", "26.3.0", "26.4.0")


class GateBinaryControls(unittest.TestCase):
    """TSK-142 AC-2: the real-browser check runs the binary the gate built,
    wherever CARGO_TARGET_DIR points."""

    def test_the_committed_config_passes_the_gate_binary(self):
        self.assertEqual(parity.gate_binary_problems(CONFIG), [])

    def test_gate_binary_helper_follows_custom_target_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, CARGO_TARGET_DIR=tmp)
            run = subprocess.run([sys.executable, "-B", "scripts/gate-binary.py"],
                                 cwd=ROOT, env=env, capture_output=True, text=True,
                                 check=False)
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertEqual(run.stdout.strip(),
                             str((Path(tmp) / "debug" / "codeflow").resolve()))

    def test_a_real_browser_check_without_the_gate_binary_is_refused(self):
        for field, value in (("requires", []), ("outputs", ["elsewhere/codeflow"])):
            def drop(c, f=field, v=value):
                target(c, PRESENT if f == "requires" else "codeflow-bin")[f] = v
            with self.subTest(field=field):
                problems = parity.gate_binary_problems(mutated(drop))
                self.assertTrue(any(PRESENT in p and "CARGO_TARGET_DIR" in p for p in problems), problems)


class WindowsRefereeControls(unittest.TestCase):
    def test_every_raw_rust_floor_is_required(self):
        for command in ["cargo fmt --all -- --check", parity.SUITE, parity.DOCTESTS,
                        "cargo clippy --workspace --all-targets -- -D warnings",
                        'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps']:
            with self.subTest(command=command):
                self.assertNotEqual(parity.local_rust_commands(CONFIG), parity.ci_rust_job_commands(WORKFLOW.replace("run: " + command, "run: echo removed")))


class WindowsPartitionControls(unittest.TestCase):
    """TSK-203 AC-2: the partitioned Windows suite stands for the whole
    suite only when its matrix runs every partition from 1 to N once."""

    PARTITIONS = "partition: [1, 2, 3, 4, 5, 6]"
    RUN = "--partition count:${{ matrix.partition }}/6"

    def test_the_committed_partitions_run_the_whole_suite(self):
        self.assertIn(self.PARTITIONS, WORKFLOW)
        self.assertIn(self.RUN, WORKFLOW)
        self.assertIn(parity.SUITE, CI)

    def test_a_partition_set_that_misses_or_exceeds_n_is_drift(self):
        for changed in (("partition: [1, 2, 3, 4, 5]", self.RUN),
                        ("partition: [1, 2, 3, 4, 5, 6, 7]", self.RUN),
                        ("partition: [1, 1, 2, 3, 4, 5]", self.RUN),
                        (self.PARTITIONS, "--partition count:${{ matrix.partition }}/7"),
                        (self.PARTITIONS, "--partition hash:${{ matrix.partition }}/6")):
            with self.subTest(changed=changed):
                workflow = (WORKFLOW.replace(self.PARTITIONS, changed[0])
                            .replace(self.RUN, changed[1]))
                self.assertNotIn(parity.SUITE, parity.ci_rust_job_commands(workflow))
                self.assertNotEqual(parity.local_rust_commands(CONFIG),
                                    parity.ci_rust_job_commands(workflow))


class GatePartControls(unittest.TestCase):
    """TSK-203 AC-1: the parts of the Linux full gate together run every
    full-mode target with the same strictness, and one `codeflow gates` job
    judges them all and is never skipped."""

    def problems(self, workflow: str = WORKFLOW, cfg: dict = CONFIG) -> list[str]:
        return parity.gate_part_problems(cfg, workflow)

    def assert_problem(self, workflow: str, *needles: str, cfg: dict = CONFIG) -> None:
        self.assertNotEqual(workflow, WORKFLOW, "the control must change the workflow")
        problems = self.problems(workflow, cfg)
        self.assertTrue(any(all(n in p for n in needles) for p in problems),
                        f"{needles} not in {problems}")

    def test_the_committed_parts_run_every_full_mode_target(self):
        self.assertEqual(self.problems(), [])

    def test_a_target_no_part_names_is_refused(self):
        self.assert_problem(WORKFLOW.replace("only: read-benchmark", "only: rust-format"),
                            "no gate part runs", "read-benchmark")
        self.assert_problem(WORKFLOW.replace(",journey-gate\n", "\n"),
                            "no gate part runs", "journey-gate")

    def test_a_new_full_mode_target_must_join_a_part(self):
        def add(c):
            c["targets"].append({"name": "new-check", "runner": "custom",
                                 "modes": {"full": {"command": "true"}}})
        problems = self.problems(cfg=mutated(add))
        self.assertTrue(any("new-check" in p for p in problems), problems)

    def test_an_unknown_or_twice_named_target_is_refused(self):
        self.assert_problem(WORKFLOW.replace("only: read-benchmark", "only: read-benchmark,rust-lint"),
                            "no enabled full mode", "rust-lint")
        self.assert_problem(WORKFLOW.replace("only: read-benchmark", "only: read-benchmark,rust-format"),
                            "more than one gate part", "rust-format")

    def test_a_part_with_less_strictness_is_refused(self):
        for weaker in ("codeflow test --mode full --all --only ${{ matrix.only }}",
                       "codeflow test --mode full --strict --only ${{ matrix.only }}",
                       "codeflow test --mode essential --strict --all --only ${{ matrix.only }}"):
            with self.subTest(weaker=weaker):
                self.assert_problem(WORKFLOW.replace(parity.PART_RUN, weaker), "must run exactly")

    VERDICT_IF = ("    if: always() && (github.event_name == 'pull_request' && contains(fromJSON('[\"main\",\"master\"]'), "
                  "github.event.pull_request.base.ref) || github.event_name == 'push')\n    name: codeflow gates\n")
    EXACT = "must be exactly the verdict job"

    def test_the_verdict_must_exist_need_the_parts_and_never_be_skipped(self):
        self.assertIn(self.VERDICT_IF, WORKFLOW)
        self.assert_problem(WORKFLOW.replace("    name: codeflow gates\n", "    name: gates verdict\n"),
                            "exactly one job must be named")
        self.assert_problem(WORKFLOW.replace("    needs: gates\n", "    needs: windows\n"), self.EXACT)
        self.assert_problem(WORKFLOW.replace(self.VERDICT_IF,
                                             "    if: github.event_name == 'push'\n    name: codeflow gates\n"),
                            self.EXACT)
        self.assert_problem(WORKFLOW.replace('if [ "$PARTS" != success ]; then', 'if false; then'), self.EXACT)

    def test_a_verdict_that_could_pass_a_failed_part_is_refused(self):
        """The four forms the TSK-203 review showed a substring check accepting."""
        controls = {
            "a condition that never runs": WORKFLOW.replace(
                "    if: always() && (github.event_name", "    if: always() && false && (github.event_name"),
            "a forgiven exit": WORKFLOW.replace(
                "            exit 1\n          fi\n          echo \"every part",
                "            exit 0\n          fi\n          echo \"every part"),
            "continue-on-error on the verdict": WORKFLOW.replace(
                "    needs: gates\n", "    needs: gates\n    continue-on-error: true\n"),
        }
        for label, workflow in controls.items():
            with self.subTest(label):
                self.assert_problem(workflow, self.EXACT)

    STEP = "      - name: codeflow test --mode full --strict\n"
    GATE_STEP_EXACT = "the gate step must be exactly"
    OWN_KEYS = "own keys must be exactly"
    TOP_LEVEL = "top level must be exactly"
    STRATEGY = "the gates strategy must be"

    def assert_all(self, controls: dict) -> None:
        for label, (workflow, needle) in controls.items():
            with self.subTest(label):
                self.assert_problem(workflow, needle)

    def test_a_skippable_forgiven_or_moved_gate_step_is_refused(self):
        """Key order cannot hide a key (TSK-203 review round two)."""
        step = self.STEP
        self.assertIn(step + "        shell: bash\n", WORKFLOW)
        self.assert_all({
            "if": (WORKFLOW.replace(step, step + "        if: false\n"), self.GATE_STEP_EXACT),
            "if first": (WORKFLOW.replace(step, "      - if: false\n        name: codeflow test --mode full --strict\n"),
                         self.GATE_STEP_EXACT),
            "continue-on-error first": (WORKFLOW.replace(step, "      - continue-on-error: true\n"
                                                               "        name: codeflow test --mode full --strict\n"),
                                        self.GATE_STEP_EXACT),
            "step continue-on-error": (WORKFLOW.replace(step, step + "        continue-on-error: true\n"),
                                       self.GATE_STEP_EXACT),
            "working directory": (WORKFLOW.replace(step, step + "        working-directory: docs\n"),
                                  self.GATE_STEP_EXACT),
            "no pinned shell": (WORKFLOW.replace(step + "        shell: bash\n", step), self.GATE_STEP_EXACT),
            "job continue-on-error": (WORKFLOW.replace("    timeout-minutes: 45\n    strategy:\n",
                                                       "    timeout-minutes: 45\n    continue-on-error: true\n    strategy:\n"),
                                      self.OWN_KEYS),
        })

    def test_alternate_yaml_spellings_are_refused(self):
        """Quoted, spaced or escaped keys decode to the same key (TSK-203 review round three)."""
        step = self.STEP
        self.assert_all({
            "quoted if": (WORKFLOW.replace(step, step + "        'if': false\n"), self.GATE_STEP_EXACT),
            "spaced if": (WORKFLOW.replace(step, step + "        if : false\n"), self.GATE_STEP_EXACT),
            "quoted continue-on-error": (WORKFLOW.replace(step, step + "        'continue-on-error': true\n"),
                                         self.GATE_STEP_EXACT),
            "quoted job key": (WORKFLOW.replace("    timeout-minutes: 45\n    strategy:\n",
                                                "    timeout-minutes: 45\n    'continue-on-error': true\n    strategy:\n"),
                               self.OWN_KEYS),
            "escaped BASH_ENV": (WORKFLOW.replace("\njobs:\n", "\nenv:\n  \"BASH\\u005fENV\": ./skip.sh\n\njobs:\n"),
                                 self.TOP_LEVEL),
        })

    def test_an_indented_root_cannot_hide_a_workflow_env(self):
        """Indenting every root key by one space still parses, and left a
        column-zero check nothing to read (TSK-203 review round four)."""
        lines = WORKFLOW.splitlines(keepends=True)
        shifted = "".join(" " + line if re.match(r"^[a-z-]+:", line) else line for line in lines)
        shifted = shifted.replace("\n jobs:\n", "\n env:\n  SHELLOPTS: noexec\n jobs:\n")
        self.assertIn(" env:\n  SHELLOPTS: noexec\n", shifted)
        self.assert_problem(shifted, self.TOP_LEVEL)
        missing = WORKFLOW.replace("\nconcurrency:\n", "\n# concurrency removed\nx-concurrency:\n")
        self.assert_problem(missing, self.TOP_LEVEL)

    def test_a_continued_matrix_value_is_refused(self):
        """A continuation line joins `|| true` onto the command (round three)."""
        only = "            only: rust-coverage,journey-gate\n"
        self.assertIn(only, WORKFLOW)
        self.assert_problem(WORKFLOW.replace(only, only + "              || true\n"), self.STRATEGY)
        self.assert_problem(WORKFLOW.replace(only, "            only: 'rust-coverage,journey-gate || true'\n"),
                            self.STRATEGY)

    def test_a_second_command_in_a_script_is_refused(self):
        """A heredoc can carry the expected text while bash runs something else (round three)."""
        step = self.STEP
        decoy = ("      - name: decoy\n        shell: bash\n        run: |\n          cat <<'X'\n"
                 "          run: codeflow test --mode full --strict --all --only ${{ matrix.only }}\n"
                 "          X\n          true\n")
        self.assert_problem(WORKFLOW.replace(step, decoy + step), self.GATE_STEP_EXACT)

    def test_nothing_in_the_workflow_may_reshape_the_gate_or_verdict(self):
        """`defaults.run.shell: bash -n {0}` made the verdict exit 0 on a failed
        part without changing its text (round two)."""
        top = "\njobs:\n"
        self.assertIn(top, WORKFLOW)
        self.assert_all({
            "workflow defaults": (WORKFLOW.replace(top, "\ndefaults:\n  run:\n    shell: bash -n {0}\n" + top),
                                  self.TOP_LEVEL),
            "workflow env": (WORKFLOW.replace(top, "\nenv:\n  BASH_ENV: ./skip.sh\n" + top), self.TOP_LEVEL),
            "gates job defaults": (WORKFLOW.replace("    timeout-minutes: 45\n    strategy:\n",
                                                    "    timeout-minutes: 45\n    defaults:\n      run:\n"
                                                    "        shell: bash -n {0}\n    strategy:\n"),
                                   self.OWN_KEYS),
            "merge key": (WORKFLOW.replace("        shell: bash\n        env:\n          LLVM",
                                           "        <<: *skipped\n        shell: bash\n        env:\n          LLVM"),
                          self.GATE_STEP_EXACT),
            "flow mapping": (WORKFLOW.replace("        env:\n          LLVM_PROFILE_FILE_NAME: codeflow-%4m.profraw\n",
                                              "        env: {LLVM_PROFILE_FILE_NAME: codeflow-%4m.profraw}\n"),
                             self.GATE_STEP_EXACT),
            "verdict without its shell": (WORKFLOW.replace(
                "      - name: Every part of the full gate passed\n        shell: bash\n",
                "      - name: Every part of the full gate passed\n"), self.EXACT),
        })

    def test_comments_and_blank_lines_in_the_verdict_are_allowed(self):
        workflow = WORKFLOW.replace("    needs: gates\n", "    needs: gates\n\n    # the parts\n")
        self.assertNotEqual(workflow, WORKFLOW)
        self.assertEqual(self.problems(workflow), [])


class SharedInstallControls(unittest.TestCase):
    """TSK-203 review: one target installs the portal workspace, and every
    other target on its Node pin waits for it, so no two `npm ci` runs overlap."""

    @staticmethod
    def target(cfg: dict, name: str) -> dict:
        return next(t for t in cfg["targets"] if t["name"] == name)

    def test_the_committed_config_installs_each_workspace_once(self):
        self.assertEqual(parity.shared_install_problems(CONFIG), [])

    def test_a_second_installer_is_refused(self):
        def reinstall(c):
            full = self.target(c, "visual-eval-controls")["modes"]["full"]
            full["command"] = full["command"].replace('"node --test', '"npm run deps:install --prefix docs-portal && node --test')
        problems = parity.shared_install_problems(mutated(reinstall))
        self.assertTrue(any("more than one target installs docs-portal" in p for p in problems), problems)

    def test_a_portal_target_that_does_not_wait_for_the_install_is_refused(self):
        def unordered(c):
            self.target(c, "docs-portal")["requires"] = ["codeflow-bin"]
        problems = parity.shared_install_problems(mutated(unordered))
        self.assertTrue(any("'docs-portal'" in p and "does not require 'docs-portal-deps'" in p for p in problems),
                        problems)


class TimeoutAndPlaywrightControls(unittest.TestCase):
    """TSK-254: every job has its own timeout, and only the present part
    installs Playwright, with a bounded, retried system-library install."""

    INSTALL_TIMEOUT = "        if: matrix.part == 'present'\n        timeout-minutes: 25\n"
    BOUNDED = 'sudo timeout --kill-after=30s 10m "$node" "$cli" install-deps chromium firefox webkit'

    def problems(self, workflow: str) -> list[str]:
        return parity.timeout_problems(workflow) + parity.playwright_problems(workflow)

    def assert_problem(self, workflow: str, *needles: str) -> None:
        self.assertNotEqual(workflow, WORKFLOW, "the control must change the workflow")
        problems = self.problems(workflow)
        self.assertTrue(any(all(n in p for n in needles) for p in problems),
                        f"{needles} not in {problems}")

    def test_the_committed_workflow_bounds_every_job_and_confines_playwright(self):
        self.assertEqual(self.problems(WORKFLOW), [])
        jobs = parity.jobs_section(WORKFLOW)
        self.assertIn("gates", jobs)
        self.assertNotIn("pull_request", jobs, "trigger keys are not jobs")

    def test_a_job_without_its_own_whole_minute_timeout_is_refused(self):
        journeys = "    runs-on: windows-latest\n    timeout-minutes: 15\n"
        self.assertEqual(WORKFLOW.count(journeys), 1)
        self.assert_problem(WORKFLOW.replace(journeys, "    runs-on: windows-latest\n"),
                            "job 'windows-journeys'", "timeout-minutes")
        for value in ("0", "${{ 360 }}", "'30'", "30.5"):
            with self.subTest(value=value):
                self.assert_problem(WORKFLOW.replace(journeys, f"    runs-on: windows-latest\n    timeout-minutes: {value}\n"),
                                    "job 'windows-journeys'")
        self.assert_problem(WORKFLOW.replace(journeys, journeys + "    timeout-minutes: 15\n"),
                            "job 'windows-journeys'", "exactly one")

    def test_a_timeout_above_the_ac1_bound_is_refused(self):
        """AC-1: a hung job frees its runner inside 45 minutes, so GitHub's
        six-hour default of 360 must not pass as a "positive whole number"."""
        journeys = "    runs-on: windows-latest\n    timeout-minutes: 15\n"
        self.assertEqual(parity.MAX_TIMEOUT_MINUTES, 45)
        for value in ("360", "46", "100"):
            with self.subTest(value=value):
                workflow = WORKFLOW.replace(journeys, f"    runs-on: windows-latest\n    timeout-minutes: {value}\n")
                self.assert_problem(workflow, "job 'windows-journeys'", "45 minutes")
        at_bound = WORKFLOW.replace(journeys, "    runs-on: windows-latest\n    timeout-minutes: 45\n")
        self.assertNotEqual(at_bound, WORKFLOW)
        self.assertEqual(self.problems(at_bound), [], "45 itself is allowed")

    def test_every_committed_workflow_and_template_is_within_the_bound(self):
        self.assertEqual(parity.all_timeout_problems(), [])
        values = []
        for relative in parity.workflow_files():
            text = (ROOT / relative).read_text()
            values += [int(v) for v in re.findall(r"^ {4}timeout-minutes: (\d+)$", text, re.M)]
        self.assertTrue(values)
        self.assertLessEqual(max(values), parity.MAX_TIMEOUT_MINUTES)

    def test_a_playwright_step_timeout_or_install_limit_above_the_bound_is_refused(self):
        self.assertEqual(WORKFLOW.count(self.INSTALL_TIMEOUT), 1)
        self.assertEqual(WORKFLOW.count(self.BOUNDED), 1)
        self.assert_problem(WORKFLOW.replace(self.INSTALL_TIMEOUT, self.INSTALL_TIMEOUT.replace("25", "360")),
                            "install-deps", "twice")
        for limit in ("11m", "3h", "601s", "600", "1h", "0m"):
            with self.subTest(limit=limit):
                self.assert_problem(WORKFLOW.replace(self.BOUNDED, self.BOUNDED.replace("10m", limit)),
                                    "install-deps", "10 minutes")
        within = WORKFLOW.replace(self.BOUNDED, self.BOUNDED.replace("10m", "600s"))
        self.assertEqual(self.problems(within), [], "600 seconds is 10 minutes")

    BROWSERS = 'if timeout --kill-after=30s 10m "$node" "$cli" install chromium firefox webkit'

    def test_the_browser_install_is_bounded_like_install_deps(self):
        """AC-2: each attempt is bounded at 10 minutes, the browser download
        included, so a hang there reaches the retry and not the step timeout."""
        self.assertEqual(WORKFLOW.count(self.BROWSERS), 1)
        unwrapped = self.BROWSERS.replace("timeout --kill-after=30s 10m ", "")
        self.assert_problem(WORKFLOW.replace(self.BROWSERS, unwrapped), "browser `install`", "10 minutes")
        for limit in ("11m", "3h", "601s", "600", "1h", "0m"):
            with self.subTest(limit=limit):
                self.assert_problem(WORKFLOW.replace(self.BROWSERS, self.BROWSERS.replace("10m", limit)),
                                    "browser `install`", "10 minutes")
        within = WORKFLOW.replace(self.BROWSERS, self.BROWSERS.replace("10m", "600s"))
        self.assertEqual(self.problems(within), [], "600 seconds is 10 minutes")
        # A browser install with no install-deps still has to be bounded.
        only_browsers = WORKFLOW.replace(self.BROWSERS, unwrapped).replace(self.BOUNDED, "true")
        self.assertNotIn("install-deps chromium", only_browsers)
        self.assert_problem(only_browsers, "browser `install`")

    def test_every_spelling_of_the_browser_install_is_bounded(self):
        """An `npx playwright install` or `playwright-core install` line is
        the same download as the committed `"$cli" install`, so it needs the
        same 10 minute attempt bound."""
        self.assertEqual(WORKFLOW.count(self.BROWSERS), 1)
        for spelling in ("npx playwright install chromium firefox webkit",
                         "npx playwright-core install chromium",
                         "npx playwright install",
                         "npx playwright@1.49.0 install",
                         "npx playwright-core@1.49.0 install",
                         'npx "playwright" install',
                         "node node_modules/playwright/cli.js install",
                         "node node_modules/playwright-core/cli.js install",
                         '"${cli}" install chromium',
                         "$cli install chromium",
                         'node -e "require(\'child_process\').execSync(\'npx playwright install\')"'):
            with self.subTest(spelling=spelling):
                extra = WORKFLOW.replace(self.BROWSERS, self.BROWSERS + "\n            " + spelling)
                self.assert_problem(extra, "browser `install`", "10 minutes")
                bounded = WORKFLOW.replace(
                    self.BROWSERS, self.BROWSERS + "\n            timeout --kill-after=30s 10m " + spelling)
                self.assertEqual(self.problems(bounded), [], spelling)

    def test_a_playwright_step_on_every_part_is_refused(self):
        cache = "        if: matrix.part == 'present'\n        with:\n          path: ~/.cache/ms-playwright\n"
        self.assertEqual(WORKFLOW.count(cache), 1)
        self.assert_problem(WORKFLOW.replace(cache, cache.replace("        if: matrix.part == 'present'\n", "")),
                            "touches Playwright on every part")
        self.assertEqual(WORKFLOW.count(self.INSTALL_TIMEOUT), 1)
        self.assert_problem(WORKFLOW.replace(self.INSTALL_TIMEOUT, "        timeout-minutes: 25\n"),
                            "touches Playwright on every part")

    def test_an_unbounded_or_single_attempt_install_is_refused(self):
        self.assertEqual(WORKFLOW.count(self.BOUNDED), 1)
        for name, workflow in {
            "no step timeout": WORKFLOW.replace(self.INSTALL_TIMEOUT, "        if: matrix.part == 'present'\n"),
            "apt-get outside a timeout": WORKFLOW.replace(self.BOUNDED, self.BOUNDED.replace("sudo timeout --kill-after=30s 10m ", "sudo ")),
            "one attempt": WORKFLOW.replace("for attempt in 1 2; do", "for attempt in 1; do"),
        }.items():
            with self.subTest(name=name):
                self.assert_problem(workflow, "install-deps", "twice")

    def test_with_deps_is_refused(self):
        self.assert_problem(WORKFLOW.replace('"$cli" install chromium', '"$cli" install --with-deps chromium'),
                            "--with-deps")

    def test_playwright_outside_the_gates_job_is_refused(self):
        windows = "      - name: Install nextest\n        uses: taiki-e/install-action"
        self.assertEqual(WORKFLOW.count(windows), 1)
        self.assert_problem(WORKFLOW.replace(windows, "      - name: Install Playwright\n        run: npx playwright-core install\n" + windows),
                            "job 'windows-tests'", "Playwright step")


class EveryWorkflowTimeoutControls(unittest.TestCase):
    """TSK-254: the timeout rule covers every workflow the repository runs
    or ships, not only codeflow-ci.yml."""

    OWN = {
        ".github/workflows/codeflow-ci.yml", ".github/workflows/codeflow-policy.yml",
        ".github/workflows/codeflow-release.yml", ".github/workflows/codeflow-release-integration.yml",
        ".github/workflows/portal-pages.yml", ".github/workflows/release.yml",
        ".github/workflows/release-main-recheck.yml", ".github/workflows/release-plan-authority.yml",
        ".github/workflows/release-post-announce.yml",
    }
    SHIPPED = {"assets/base/ci/codeflow-ci.yml", "assets/base/ci/codeflow-policy.yml"}

    def files(self) -> dict[str, str]:
        return {path.as_posix(): (parity.ROOT / path).read_text() for path in parity.workflow_files()}

    def problems(self, path: str, text: str) -> list[str]:
        return parity.timeout_problems(text, path, parity.UNBOUNDED_BY_DESIGN.get(path))

    def test_every_workflow_file_is_found_and_none_is_unbounded(self):
        files = self.files()
        self.assertEqual(set(files), self.OWN | self.SHIPPED, "bitbucket and generic CI files are not workflows")
        self.assertEqual(parity.all_timeout_problems(), [])

    def test_dropping_any_jobs_timeout_in_any_workflow_is_refused(self):
        checked = 0
        for path, text in self.files().items():
            for name, job in parity.jobs_section(text).items():
                line = next((l for l in job.splitlines() if parity.TIMEOUT_KEY.match(l)), None)
                if line is None:
                    continue
                with self.subTest(path=path, job=name):
                    mutated_text = text.replace(job, job.replace(line + "\n", "", 1), 1)
                    self.assertNotEqual(mutated_text, text)
                    problems = self.problems(path, mutated_text)
                    self.assertTrue(any(f"job '{name}'" in p and path in p for p in problems), problems)
                    checked += 1
        self.assertGreaterEqual(checked, 25, "every capped job in every workflow is exercised")

    def test_reusable_workflow_callers_are_checked_through_their_called_workflow(self):
        release = self.files()[".github/workflows/release.yml"]
        callers = [n for n, j in parity.jobs_section(release).items() if parity.CALLER_LINE.search(j)]
        self.assertEqual(sorted(callers), ["custom-release-main-recheck", "custom-release-plan-authority",
                                           "custom-release-post-announce"])
        self.assertEqual(self.problems(".github/workflows/release.yml", release), [])
        for called in ("release-main-recheck", "release-plan-authority", "release-post-announce"):
            text = self.files()[f".github/workflows/{called}.yml"]
            self.assertEqual(len(parity.jobs_section(text)), 1)
            self.assertEqual(self.problems(called, text), [])
            self.assertTrue(self.problems(called, text.replace("    timeout-minutes: 10\n", "")))

    CALLER = "name: x\non: push\njobs:\n  call:\n    uses: {target}\n"
    BOUNDED = "on: workflow_call\njobs:\n  one:\n    timeout-minutes: 5\n    runs-on: ubuntu-latest\n    steps:\n      - run: true\n"
    UNBOUNDED = BOUNDED.replace("    timeout-minutes: 5\n", "")

    def caller_problems(self, target: str, callee_files: dict[str, str] | None = None) -> list[str]:
        """Check a one-job caller of `target` against a scratch tree holding `callee_files`."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / "assets" / "base" / "ci").mkdir(parents=True)
            (root / "scripts").mkdir()
            for name, text in (callee_files or {}).items():
                (root / name).write_text(text)
            original = parity.ROOT
            parity.ROOT = root
            try:
                return parity.timeout_problems(self.CALLER.format(target=target), "caller.yml")
            finally:
                parity.ROOT = original

    def test_a_caller_is_exempt_only_when_it_calls_a_bounded_local_workflow(self):
        ok = {".github/workflows/ok.yml": self.BOUNDED}
        self.assertEqual(self.caller_problems("./.github/workflows/ok.yml", ok), [])
        self.assertEqual(self.caller_problems("'./.github/workflows/ok.yml'  # bounded", ok), [])

        remote = self.caller_problems("some-org/some-repo/.github/workflows/ci.yml@v1")
        self.assertTrue(any("job 'call' in caller.yml" in p and "cannot open and bound" in p for p in remote), remote)
        outside = self.caller_problems("./scripts/hang.yml", {"scripts/hang.yml": self.UNBOUNDED})
        self.assertTrue(any("./scripts/hang.yml" in p for p in outside), outside)
        pinned = self.caller_problems("./.github/workflows/ok.yml@main", ok)
        self.assertTrue(pinned, "a local path with a ref is not a local call")
        sideways = self.caller_problems("./.github/workflows/../../scripts/hang.yml", {"scripts/hang.yml": self.BOUNDED})
        self.assertTrue(sideways, "a path that climbs out of .github/workflows is refused")
        missing = self.caller_problems("./.github/workflows/gone.yml")
        self.assertTrue(missing, "a callee that does not exist cannot be shown bounded")

        hang = self.caller_problems("./.github/workflows/hang.yml", {".github/workflows/hang.yml": self.UNBOUNDED})
        self.assertTrue(any("job 'call' in caller.yml" in p and "job 'one' in .github/workflows/hang.yml" in p
                            for p in hang), hang)

        loop = self.caller_problems("./.github/workflows/loop.yml", {
            ".github/workflows/loop.yml": self.CALLER.format(target="./.github/workflows/loop.yml")})
        self.assertTrue(any("calls itself again" in p for p in loop), loop)

    def test_the_adopter_template_exempts_only_its_named_jobs(self):
        path = "assets/base/ci/codeflow-ci.yml"
        text = self.files()[path]
        self.assertEqual(self.problems(path, text), [])
        self.assertTrue(self.problems(path, text.replace("    timeout-minutes: 30\n", "", 1)))
        self.assertTrue(parity.timeout_problems(text), "without the exemptions the unbounded jobs are named")
        stale = parity.timeout_problems("jobs:\n  other:\n    timeout-minutes: 5\n", path, {"gates": "x"})
        self.assertTrue(any("exempts job 'gates'" in p for p in stale), stale)

    def test_a_new_workflow_without_timeouts_is_found_and_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / "assets" / "base" / "ci").mkdir(parents=True)
            (root / ".github" / "workflows" / "nightly.yml").write_text(
                "name: nightly\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: true\n")
            (root / "assets" / "base" / "ci" / "bitbucket-pipelines.yml").write_text("pipelines:\n  default: []\n")
            original = parity.ROOT
            parity.ROOT = root
            try:
                self.assertEqual([p.as_posix() for p in parity.workflow_files()], [".github/workflows/nightly.yml"])
                problems = parity.all_timeout_problems()
            finally:
                parity.ROOT = original
        self.assertTrue(any("job 'build' in .github/workflows/nightly.yml" in p for p in problems), problems)

    def test_the_dist_config_must_allow_the_hand_added_release_timeouts(self):
        self.assertEqual(parity.dist_problems(), [])
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "dist-workspace.toml"
            for body in ('[dist]\nci = "github"\n', '[dist]\nallow-dirty = ["msi"]\n'):
                config.write_text(body)
                self.assertTrue(any("allow-dirty" in p for p in parity.dist_problems(config)), body)
            # A string is not the list cargo-dist honors, and `in` on a string
            # is a substring test.
            for body in ('[dist]\nallow-dirty = "ci"\n', '[dist]\nallow-dirty = "preci"\n',
                         '[dist]\nallow-dirty = ["preci"]\n', '[dist]\nallow-dirty = 5\n',
                         '[dist]\nallow-dirty = []\n'):
                config.write_text(body)
                self.assertTrue(any("allow-dirty" in p for p in parity.dist_problems(config)), body)
            config.write_text('[dist]\nallow-dirty = ["ci"]\n')
            self.assertEqual(parity.dist_problems(config), [])
            config.write_text('[dist]\nallow-dirty = ["msi", "ci"]\n')
            self.assertEqual(parity.dist_problems(config), [])


class JobHeaderControls(unittest.TestCase):
    """TSK-254 review: a job GitHub runs must never be skipped by the line
    reader. A header with a trailing comment or a quoted id is a job, and a
    line at the job indent that is not a header is refused, not read past."""

    PAGES = ".github/workflows/portal-pages.yml"
    RELEASE = ".github/workflows/release.yml"
    CI = ".github/workflows/codeflow-ci.yml"

    def files(self) -> dict[str, str]:
        return {path.as_posix(): (parity.ROOT / path).read_text() for path in parity.workflow_files()}

    def problems(self, path: str, text: str) -> list[str]:
        return parity.timeout_problems(text, path, parity.UNBOUNDED_BY_DESIGN.get(path))

    def drop_timeout(self, text: str, job: str, header: str | None = None) -> str:
        """`text` with the job's header rewritten as `header` and its timeout removed."""
        body = parity.jobs_section(text)[job]
        line = next(l for l in body.splitlines() if parity.TIMEOUT_KEY.match(l))
        mutated = text.replace(f"  {job}:\n" + body, (header or f"  {job}:") + "\n" + body.replace(line + "\n", "", 1), 1)
        self.assertNotEqual(mutated, text)
        return mutated

    def test_the_reviewers_three_mutations_name_the_unbounded_job(self):
        files = self.files()
        nightly = (files[self.PAGES].rstrip("\n")
                   + "\n\n  nightly: # weekly\n    runs-on: ubuntu-latest\n    steps:\n      - run: true\n")
        cases = {
            "nightly appended with a trailing comment": (self.PAGES, nightly, "nightly"),
            "plan header with a comment, timeout deleted": (
                self.RELEASE, self.drop_timeout(files[self.RELEASE], "plan", "  plan: # first job"), "plan"),
            "secret-scan header with a comment, timeout deleted": (
                self.CI, self.drop_timeout(files[self.CI], "secret-scan", "  secret-scan: # scan"), "secret-scan"),
        }
        for name, (path, text, job) in cases.items():
            with self.subTest(name):
                self.assertEqual(self.problems(path, files[path]), [])
                found = self.problems(path, text)
                self.assertTrue(any(f"job '{job}'" in p and "timeout-minutes" in p for p in found), found)

    def test_a_quoted_job_id_is_a_job(self):
        files = self.files()
        for header in ('  "plan":', "  'plan':", '  "plan": # quoted'):
            with self.subTest(header=header):
                kept = files[self.RELEASE].replace("  plan:\n", header + "\n", 1)
                self.assertIn("plan", parity.jobs_section(kept))
                self.assertEqual(self.problems(self.RELEASE, kept), [], "a bounded quoted job passes")
                found = self.problems(self.RELEASE, self.drop_timeout(files[self.RELEASE], "plan", header))
                self.assertTrue(any("job 'plan'" in p and "timeout-minutes" in p for p in found), found)

    def test_a_commented_out_header_cannot_hide_its_job(self):
        """The body of `# plan:` folds into the job above, or under no job,
        and repeats keys the job above already has."""
        files = self.files()
        for path, job in ((self.RELEASE, "plan"), (self.CI, "secret-scan")):
            with self.subTest(job=job):
                found = self.problems(path, self.drop_timeout(files[path], job, f"  # {job}:"))
                self.assertTrue(any("repeats the key" in p or "under no job" in p for p in found), found)

    def test_a_line_at_the_job_indent_that_is_not_a_header_is_refused(self):
        base = "jobs:\n  ok:\n    timeout-minutes: 5\n    runs-on: x\n"
        for line in ("  nightly: {runs-on: ubuntu-latest}", "  nightly:#weekly", "  nightly: &anchor",
                     "  - nightly:", "  ? nightly", "  night.ly:", '  "night ly":', "  nightly"):
            with self.subTest(line=line):
                found = parity.timeout_problems(base + line + "\n    runs-on: x\n")
                self.assertTrue(any("is not a job id" in p for p in found), found)
        found = parity.timeout_problems(base + " stray:\n")
        self.assertTrue(any("job ids are indented two spaces" in p for p in found), found)
        found = parity.timeout_problems(base + "\t  nightly:\n")
        self.assertTrue(any("tab" in p for p in found), found)
        found = parity.timeout_problems("jobs:\n    ok:\n      timeout-minutes: 5\n")
        self.assertTrue(any("under no job id" in p for p in found), found)
        self.assertEqual(parity.timeout_problems(base + "\n  # a comment\n\n"), [])

    def test_a_repeated_job_or_top_level_jobs_key_is_refused(self):
        job = "  ok:\n    timeout-minutes: 5\n    runs-on: x\n"
        found = parity.timeout_problems("jobs:\n" + job + job)
        self.assertTrue(any("declared twice" in p for p in found), found)
        found = parity.timeout_problems("jobs:\n" + job + "jobs:\n" + job)
        self.assertTrue(any("more than one top-level `jobs:`" in p for p in found), found)
        self.assertEqual(parity.timeout_problems("jobs: # all\n" + job), [])
        self.assertEqual(parity.timeout_problems("jobs:\n" + job + "env:\n  a: b\n"), [])

    def test_keys_under_a_job_are_read_or_refused(self):
        job = "jobs:\n  ok:\n    timeout-minutes: 5\n    runs-on: x\n"
        cases = {
            "a repeated key": job + "    runs-on: y\n",
            "a sequence at the key indent": job + "    - run: true\n",
            "a flow mapping line": job + "    {a: b}\n",
            "a key indented three spaces": job + "   steps:\n",
            "inline steps": job + "    steps: [{run: true}]\n",
            "a quoted step key": job + '    steps:\n      - "run": true\n',
            "a quoted second step key": job + "    steps:\n      - name: x\n        'uses': ./a\n",
            "a second timeout": job + "    timeout-minutes: 6\n",
            "a quoted timeout key": job.replace("    timeout-minutes: 5\n", '    "timeout-minutes": 5\n'),
        }
        for name, text in cases.items():
            with self.subTest(name):
                self.assertTrue(parity.timeout_problems(text), name)
        self.assertEqual(parity.timeout_problems(job + "    steps: # block\n      - run: true\n"), [])
        self.assertEqual(parity.timeout_problems(job.replace("5\n", "5 # sized from the runs\n", 1)), [])

    def test_a_column_zero_comment_does_not_end_a_job(self):
        windows = "      - name: Install nextest\n        uses: taiki-e/install-action"
        self.assertEqual(WORKFLOW.count(windows), 1)
        step = "      - name: Install Playwright\n        run: npx playwright-core install\n"
        for comment in ("# note", "#"):
            with self.subTest(comment=comment):
                found = parity.playwright_problems(WORKFLOW.replace(windows, f"{comment}\n" + step + windows))
                self.assertTrue(any("job 'windows-tests'" in p and "Playwright step" in p for p in found), found)

    def test_a_quoted_setup_node_value_is_still_a_setup_node_step(self):
        job = '      - uses: "actions/setup-node@v4"\n        with:\n          node-version: 24.18.0\n'
        self.assertEqual(parity.setup_node_steps("    steps:\n" + job), [("24.18.0", [])])
        self.assertEqual(parity.setup_node_steps("    steps:\n" + job.replace('"', "'")), [("24.18.0", [])])
        named = job.replace("- uses", "- name: n\n        uses")
        self.assertEqual(parity.setup_node_steps("    steps:\n" + named), [("24.18.0", [])])

    def test_a_template_with_a_quoted_jobs_key_is_still_a_workflow(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / "assets" / "base" / "ci").mkdir(parents=True)
            (root / "assets" / "base" / "ci" / "quoted.yml").write_text('"jobs":\n  a:\n    runs-on: x\n')
            original = parity.ROOT
            parity.ROOT = root
            try:
                self.assertEqual([p.as_posix() for p in parity.workflow_files()], ["assets/base/ci/quoted.yml"])
            finally:
                parity.ROOT = original


if __name__ == "__main__":
    unittest.main()

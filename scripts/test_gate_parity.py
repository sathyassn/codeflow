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

    def test_the_verdict_must_exist_need_the_parts_and_never_be_skipped(self):
        self.assert_problem(WORKFLOW.replace("    name: codeflow gates\n", "    name: gates verdict\n"),
                            "exactly one job must be named")
        self.assert_problem(WORKFLOW.replace("    needs: gates\n", "    needs: windows\n"),
                            "must need the gates job")
        self.assert_problem(WORKFLOW.replace("    if: always() && (github.event_name == 'pull_request' && contains(fromJSON('[\"main\",\"master\"]'), github.event.pull_request.base.ref) || github.event_name == 'push')\n    name: codeflow gates\n",
                                             "    if: github.event_name == 'push'\n    name: codeflow gates\n"),
                            "always()")
        self.assert_problem(WORKFLOW.replace('if [ "$PARTS" != success ]; then', 'if false; then'),
                            "needs.gates.result is success")


if __name__ == "__main__":
    unittest.main()

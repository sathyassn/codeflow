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
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)

CONFIG = json.loads(parity.CONFIG.read_text(encoding="utf-8"))
WORKFLOW = parity.WORKFLOW.read_text(encoding="utf-8")
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


def gate_event_condition(workflow: str) -> str:
    gate = parity.workflow_jobs(workflow)["gates"]
    condition = re.search(r"^ {4}if:\s*(.+?)\s*$", gate, re.M).group(1)
    return parity.GATES_IF.match(condition).group(1)


class GateParityControls(unittest.TestCase):
    def assert_parity(self, cfg: dict) -> None:
        self.assertEqual(parity.windows_owed(parity.local_rust_commands(cfg)), CI)

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

    def test_model_eval_requires_the_binary_in_each_mode(self):
        for mode in ("essential", "full"):
            for change in ("missing dependency", "wrong dependency", "missing mode", "disabled", "wrong output"):
                with self.subTest(mode=mode, change=change):
                    cfg = copy.deepcopy(CONFIG)
                    consumer = target(cfg, "model-eval-kit")
                    consumer["modes"] = {mode: consumer["modes"][mode]}
                    producer = target(cfg, "codeflow-bin")
                    if change == "missing dependency":
                        consumer["requires"] = []
                    elif change == "wrong dependency":
                        consumer["requires"] = ["release-automation"]
                        target(cfg, "release-automation")["requires"] = []
                    elif change == "missing mode":
                        del producer["modes"][mode]
                    elif change == "disabled":
                        producer["enabled"] = False
                    else:
                        producer["outputs"] = ["target/release/codeflow"]
                    self.assertTrue(any("model-eval-kit" in p and mode in p
                                        for p in parity.gate_binary_problems(cfg)))

    def test_a_direct_binary_path_needs_a_matching_producer(self):
        cfg = copy.deepcopy(CONFIG)
        consumer = target(cfg, "model-eval-kit")
        consumer["requires"] = ["codeflow-bin"]
        consumer["modes"] = {"essential": {"command": "target/release/codeflow status"}}
        self.assertTrue(any("target/release/codeflow" in p for p in parity.gate_binary_problems(cfg)))
        target(cfg, "codeflow-bin")["outputs"] = ["target/release/codeflow"]
        self.assertEqual([p for p in parity.gate_binary_problems(cfg) if "model-eval-kit" in p], [])

    def test_a_longer_binary_name_is_not_a_codeflow_input(self):
        for name in ("codeflow-present", "codeflow.exe-wrapper", "codeflow_extra", "codeflow/helper"):
            with self.subTest(name=name):
                cfg = copy.deepcopy(CONFIG)
                consumer = target(cfg, "model-eval-kit")
                consumer["requires"] = []
                consumer["modes"] = {"full": {"command": f"target/debug/{name} status"}}
                self.assertFalse(any("model-eval-kit" in p for p in parity.gate_binary_problems(cfg)))

    def test_an_unlisted_script_using_the_binary_needs_a_producer(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / "scripts" / "new-consumer.py"
            script.parent.mkdir()
            script.write_text('import os\nprint(os.environ["CODEFLOW_BIN"])\n')
            cfg = copy.deepcopy(CONFIG)
            consumer = target(cfg, "model-eval-kit")
            consumer["modes"] = {"full": {"command": f'python3 -B "{script}"'}}
            consumer["requires"] = []
            self.assertTrue(any("model-eval-kit" in p for p in parity.gate_binary_problems(cfg)))
            consumer["requires"] = ["codeflow-bin"]
            self.assertFalse(any("model-eval-kit" in p for p in parity.gate_binary_problems(cfg)))

    def test_windows_script_operands_keep_the_drive_and_separators(self):
        for script in (r'C:\repo\scripts\consumer.py', 'C:/repo/scripts/consumer.py',
                       r'\\server\share\consumer.py', r'\\?\C:\repo\consumer.py',
                       r'scripts\consumer.py'):
            for command in (f'python3 -B "{script}"', f"python3 -B '{script}'",
                            f'python3 -B {script}'):
                with self.subTest(command=command):
                    self.assertIn(script, parity.literal_script_paths(command))

    def test_quoted_script_with_spaces_reads_the_real_binary_dependency(self):
        with tempfile.TemporaryDirectory() as temp:
            script = Path(temp) / "new consumer.py"
            cfg = copy.deepcopy(CONFIG)
            consumer = target(cfg, "model-eval-kit")
            consumer["requires"] = []
            for quote in ('"', "'"):
                with self.subTest(quote=quote):
                    consumer["modes"] = {"full": {"command": f'python3 -B {quote}{script}{quote}'}}
                    # CRLF and UTF-8 are real bytes read by the discovery.
                    script.write_bytes('# \u201d\r\nprint(CODEFLOW_BIN)\r\n'.encode("utf-8"))
                    self.assertTrue(any("model-eval-kit" in p for p in parity.gate_binary_problems(cfg)))
                    script.write_bytes(b'print("ordinary script")\r\n')
                    self.assertFalse(any("model-eval-kit" in p for p in parity.gate_binary_problems(cfg)))

    def test_script_operands_after_colons_and_questions_keep_binary_discovery(self):
        cases = (
            ("python3 scripts/a.py:scripts/b.py", "scripts/b.py"),
            ("X=lib:scripts/x.py python3 scripts/x.py", "scripts/x.py"),
            ("node --cfg=file:scripts/x.js", "scripts/x.js"),
            ("python3 a:scripts/x.py", "scripts/x.py"),
            ("node --cfg=file?scripts/x.js", "scripts/x.js"),
        )
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            for command, script in cases:
                with self.subTest(command=command):
                    (root / script).write_text("print(CODEFLOW_BIN)\n", encoding="utf-8")
                    # Inspect the prefixed operand itself, so the second
                    # explicit x.py in the assignment case cannot mask a miss.
                    operand = next(word for word in command.split() if ":" in word or "?" in word)
                    self.assertIn(script, parity.literal_script_paths(operand))
                    self.assertIn(script, parity.literal_script_paths(command))
                    self.assertTrue(parity.script_uses_codeflow_bin(command, root))

    def test_utf8_script_discovery_with_windows_default_decoding(self):
        # Exercise a real byte read with the cp1252 default from hosted Windows.
        # Explicit encodings still reach Path.read_text unchanged.
        original = Path.read_text

        def windows_read(path, encoding=None, errors=None):
            return original(path, encoding=encoding or "cp1252", errors=errors)

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / "consumer.py"
            script.write_bytes('# \u201d\nprint(CODEFLOW_BIN)\n'.encode("utf-8"))
            with mock.patch.object(Path, "read_text", windows_read):
                self.assertTrue(parity.script_uses_codeflow_bin("python consumer.py", root))
                script.write_bytes('# \u201d\nprint("ordinary script")\n'.encode("utf-8"))
                self.assertFalse(parity.script_uses_codeflow_bin("python consumer.py", root))

    def test_invalid_utf8_script_names_the_file_and_refuses(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / "malformed.py"
            script.write_bytes(b"# \xff\nprint(CODEFLOW_BIN)\n")
            with self.assertRaisesRegex(ValueError, r"malformed\.py: repository text must be UTF-8"):
                parity.script_uses_codeflow_bin("python malformed.py", root)

    def test_windows_portal_check_has_its_own_binary_prerequisite(self):
        portal = target(CONFIG, "docs-portal-windows")
        self.assertEqual(set(portal["modes"]), {"essential", "full"})
        self.assertEqual(portal["requires"], ["docs-portal"])
        self.assertTrue({"codeflow-bin", "docs-portal-deps"} <=
                        parity.requires_closure(CONFIG, {"docs-portal-windows"}))
        for settings in portal["modes"].values():
            self.assertEqual(settings["command"], "python3 -B scripts/test_gate_portal_windows.py")
        self.assertIn('python scripts/test_gate_parity.py && CODEFLOW_BIN=', WORKFLOW)
        self.assertIn('python scripts/test_gate_portal_windows.py', WORKFLOW)


ARCHIVED_RUN = ('cargo nextest run --archive-file target/windows-tests.tar.zst --workspace-remap . --extract-to . '
                '--extract-overwrite --no-fail-fast --profile codeflow '
                '-E "$FILTER"')
PARTITION_FILTER = "FILTER: ${{ fromJSON(needs.judge.outputs.partitions)[format('{0}', matrix.partition)] }}"
ARCHIVE = "cargo nextest archive --workspace --profile codeflow --archive-file target/windows-tests.tar.zst"


class WindowsRefereeControls(unittest.TestCase):
    def test_every_raw_rust_floor_is_required(self):
        for command in [ARCHIVED_RUN, ARCHIVE, parity.DOCTESTS,
                        "cargo clippy --workspace --all-targets -- -D warnings",
                        'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps']:
            with self.subTest(command=command):
                self.assertIn("run: " + command, WORKFLOW)
                self.assertNotEqual(parity.local_rust_commands(CONFIG), parity.ci_rust_job_commands(WORKFLOW.replace("run: " + command, "run: echo removed")))

    def test_windows_runs_no_fmt_and_the_parity_needs_none(self):
        # TSK-449 AC-5: formatting is platform independent, so only the Linux
        # rust-format target runs it; Windows keeps the other raw floors.
        windows = parity.workflow_jobs(WORKFLOW)["windows"]
        self.assertNotIn("cargo fmt", windows)
        self.assertIn("cargo fmt --all -- --check", parity.local_rust_commands(CONFIG))
        self.assertEqual(parity.rust_parity(), 0)


class WindowsPartitionControls(unittest.TestCase):
    """TSK-203 AC-2, TSK-265 AC-2 and AC-3: the archived Windows partitions
    stand for the whole suite only when the archive holds the whole
    workspace and the matrix runs every planned partition from 1 to N once."""

    PARTITIONS = "partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]"

    def test_the_committed_partitions_run_the_whole_suite(self):
        self.assertIn(self.PARTITIONS, WORKFLOW)
        self.assertIn("run: " + ARCHIVED_RUN, WORKFLOW)
        self.assertEqual(parity.planned_partitions(), 12)
        self.assertIn(parity.SUITE, CI)

    def test_a_partition_set_that_misses_or_exceeds_the_plan_is_drift(self):
        for matrix in ("partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]",
                       "partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]",
                       "partition: [1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]"):
            with self.subTest(matrix=matrix):
                workflow = WORKFLOW.replace(self.PARTITIONS, matrix)
                self.assertNotIn(parity.SUITE, parity.ci_rust_job_commands(workflow))
        self.assertNotIn(parity.SUITE, parity.ci_rust_job_commands(WORKFLOW, planned=13))

    def test_a_partial_archive_another_file_or_a_count_split_is_drift(self):
        for old, new in ((ARCHIVE, ARCHIVE.replace("--workspace", "-p codeflow-core")),
                         (ARCHIVE, ARCHIVE.replace("--profile codeflow ", "")),
                         (ARCHIVED_RUN, ARCHIVED_RUN.replace("--archive-file target/windows-tests.tar.zst",
                                                             "--archive-file target/other.tar.zst")),
                         (ARCHIVED_RUN, ARCHIVED_RUN.replace("--no-fail-fast ", "")),
                         (ARCHIVED_RUN, ARCHIVED_RUN.replace('-E "$FILTER"',
                                                             "--partition count:${{ matrix.partition }}/12"))):
            with self.subTest(new=new):
                workflow = WORKFLOW.replace("run: " + old, "run: " + new)
                self.assertNotEqual(workflow, WORKFLOW)
                self.assertNotIn(parity.SUITE, parity.ci_rust_job_commands(workflow))
                self.assertNotEqual(parity.local_rust_commands(CONFIG), parity.ci_rust_job_commands(workflow))

    def test_a_filter_the_judge_job_did_not_select_is_drift(self):
        # TSK-1001: the partition filter is the judge job's output for this
        # partition, refused when empty; a filter the partition job computes
        # itself, after checking out the pull request, is not the base's.
        self.assertIn(PARTITION_FILTER, WORKFLOW)
        for old, new in ((PARTITION_FILTER, 'FILTER: ${{ matrix.partition }}'),
                         (PARTITION_FILTER, PARTITION_FILTER.replace("outputs.partitions", "outputs.shards")),
                         ("    needs: [windows-build, judge]\n", "    needs: windows-build\n"),
                         ('        run: test -n "$FILTER"\n\n      - name: Fetch the test archive',
                          '        run: true\n\n      - name: Fetch the test archive')):
            with self.subTest(new=new):
                workflow = WORKFLOW.replace(old, new)
                self.assertNotEqual(workflow, WORKFLOW)
                self.assertNotIn(parity.SUITE, parity.ci_rust_job_commands(workflow))


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
                            "no gate part, release part or coverage job runs", "read-benchmark")
        self.assert_problem(WORKFLOW.replace(",journey-python,", ","),
                            "no gate part, release part or coverage job runs", "journey-python")

    def test_release_tier_targets_run_only_in_the_release_job(self):
        # TSK-449 AC-4: the four release-tier targets leave the hosted
        # landing gate; a target neither the landing gate nor the release
        # path runs is refused, and so is a release job on landing events.
        landing = parity.workflow_jobs(WORKFLOW)["gates"]
        release = parity.workflow_jobs(WORKFLOW)["gates-release"]
        for name in parity.RELEASE_TARGETS:
            self.assertNotIn(name, landing)
            self.assertIn(name, release)
        self.assertNotIn("integration/", re.search(r"^ {4}if:\s*(.+?)\s*$", release, re.M).group(1))
        self.assert_problem(WORKFLOW.replace("only: present-browser,present-binary-delta", "only: present-browser"),
                            "no gate part, release part or coverage job runs", "present-binary-delta")
        self.assert_problem(WORKFLOW.replace("only: present-supply-chain,cf-present-qualification",
                                             "only: present-supply-chain,cf-present-qualification,read-benchmark"),
                            "landing gate parts name release-tier targets")
        self.assert_problem(WORKFLOW.replace(parity.RELEASE_IF, parity.RELEASE_IF.replace(
            "github.event_name == 'schedule'", "github.event_name == 'pull_request' || github.event_name == 'schedule'")),
                            "gates-release job's condition")

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

    DRAFT_NEUTRAL = "${{ always() && !(github.event_name == 'pull_request' && github.event.pull_request.draft) }}"
    VERDICT_IF = (f"    if: {DRAFT_NEUTRAL}\n"
                  "    name: codeflow gates\n")
    EXACT = "must be exactly the verdict job"

    def test_the_verdict_must_exist_need_the_parts_and_never_be_skipped(self):
        self.assertIn(self.VERDICT_IF, WORKFLOW)
        self.assert_problem(WORKFLOW.replace("    name: codeflow gates\n", "    name: gates verdict\n"),
                            "exactly one job must be named")
        self.assert_problem(WORKFLOW.replace("    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n", "    needs: windows\n"), self.EXACT)
        self.assert_problem(WORKFLOW.replace("    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n", "    needs: [reuse, gates]\n"), self.EXACT)
        self.assert_problem(WORKFLOW.replace("    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n", "    needs: [reuse, gates, gates-coverage, gates-coverage-report]\n"), self.EXACT)
        self.assert_problem(WORKFLOW.replace(self.VERDICT_IF,
                                             "    if: github.event_name == 'push'\n    name: codeflow gates\n"),
                            self.EXACT)
        self.assert_problem(WORKFLOW.replace('if [ "$PARTS" != success ] || [ "$SHARDS" != success ] || '
                                             '[ "$REPORT" != success ]; then', 'if false; then'), self.EXACT)
        self.assert_problem(WORKFLOW.replace(' || [ "$REPORT" != success ]; then', '; then'), self.EXACT)

    def test_a_verdict_that_could_pass_a_failed_part_is_refused(self):
        """The four forms the TSK-203 review showed a substring check accepting."""
        controls = {
            "a condition that never runs": WORKFLOW.replace(
                self.VERDICT_IF, self.VERDICT_IF.replace("always() && ", "always() && false && ")),
            "a forgiven exit": WORKFLOW.replace(
                "            exit 1\n          fi\n          echo \"every part",
                "            exit 0\n          fi\n          echo \"every part"),
            "continue-on-error on the verdict": WORKFLOW.replace(
                "    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n", "    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n    continue-on-error: true\n"),
            "a reuse that needs no skipped part": WORKFLOW.replace(
                '[ "$REUSE" = success ] && [ "$PARTS $RELEASE $SHARDS $REPORT" = "skipped skipped skipped skipped" ]',
                '[ "$REUSE" = success ]'),
            "a reuse that needs only the parts skipped": WORKFLOW.replace(
                '[ "$PARTS $RELEASE $SHARDS $REPORT" = "skipped skipped skipped skipped" ]', '[ "$PARTS" = skipped ]'),
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
        only = "            only: prose-exhaustive\n"
        self.assertIn(only, WORKFLOW)
        self.assert_problem(WORKFLOW.replace(only, only + "              || true\n"), self.STRATEGY)
        self.assert_problem(WORKFLOW.replace(only, "            only: 'prose-exhaustive || true'\n"),
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
        workflow = WORKFLOW.replace("    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n", "    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]\n\n    # the parts\n")
        self.assertNotEqual(workflow, WORKFLOW)
        self.assertEqual(self.problems(workflow), [])


def verdict_script(workflow: str = WORKFLOW, name: str = "gates-verdict") -> str:
    """The `codeflow gates` verdict's bash script, as the runner sees it."""
    job = parity.workflow_jobs(workflow)[name]
    lines = job.splitlines()
    start = lines.index("        run: |") + 1
    script = []
    for line in lines[start:]:
        if line.strip() and not line.startswith(" " * 10):
            break
        script.append(line[10:])
    return "\n".join(script)


# The verdict runs on ubuntu-latest; on Windows `bash` may be WSL's, which
# cannot see this checkout's paths.
@unittest.skipIf(os.name == "nt", "the verdict script runs in Linux bash")
class VerdictOutcomeControls(unittest.TestCase):
    """TSK-270 AC-3: the verdict fails when a part, a coverage shard or the
    coverage report failed, was cancelled or was skipped, and passes only
    when all succeeded or the reuse decision named a run and all were
    skipped. Each case runs the committed script in bash."""

    def verdict(self, parts, shards, report, reuse="skipped", reused="", release="skipped"):
        env = {"PATH": os.environ.get("PATH", ""), "PARTS": parts, "SHARDS": shards, "REPORT": report,
               "RELEASE": release,
               "REUSE": reuse, "REUSED": reused, "GITHUB_SERVER_URL": "https://github.com",
               "GITHUB_REPOSITORY": "sathyassn/codeflow"}
        return subprocess.run(["bash", "-c", verdict_script()], env=env, capture_output=True, text=True).returncode

    def test_it_passes_only_when_every_job_passed(self):
        self.assertEqual(self.verdict("success", "success", "success"), 0)

    def test_the_release_parts_pass_when_run_or_skipped_and_fail_otherwise(self):
        # TSK-449 AC-4: the release parts are skipped by their own condition
        # on a landing pull request; a failed or cancelled run fails.
        for release in ("success", "skipped"):
            self.assertEqual(self.verdict("success", "success", "success", release=release), 0, release)
        for release in ("failure", "cancelled"):
            self.assertEqual(self.verdict("success", "success", "success", release=release), 1, release)

    def test_a_failed_cancelled_or_skipped_job_fails_it(self):
        for outcome in ("failure", "cancelled", "skipped"):
            for which in range(3):
                results = ["success"] * 3
                results[which] = outcome
                with self.subTest(outcome=outcome, job=("parts", "shards", "report")[which]):
                    self.assertEqual(self.verdict(*results), 1)

    def test_both_verdicts_fail_clearly_when_the_gates_did_not_run(self):
        env = {"PATH": os.environ.get("PATH", ""), "REUSE": "skipped", "REUSED": ""}
        env.update({key: "skipped" for key in (
            "PARTS", "SHARDS", "REPORT", "CHECKS", "BUILD", "TESTS", "JOURNEYS", "RELEASE")})
        for name in ("gates-verdict", "windows-verdict"):
            with self.subTest(verdict=name):
                result = subprocess.run(["bash", "-c", verdict_script(name=name)], env=env,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 1)
                self.assertIn("gates did not run", result.stdout)

    def test_reuse_passes_only_when_every_job_was_skipped(self):
        self.assertEqual(self.verdict("skipped", "skipped", "skipped", "success", "123"), 0)
        for results in (("skipped", "success", "skipped"), ("skipped", "skipped", "failure"),
                        ("success", "skipped", "skipped")):
            with self.subTest(results=results):
                self.assertEqual(self.verdict(*results, "success", "123"), 1)
        self.assertEqual(self.verdict("skipped", "skipped", "skipped", "success", ""), 1)
        self.assertEqual(self.verdict("skipped", "skipped", "skipped", "failure", "123"), 1)


class HostedEventParityControls(unittest.TestCase):
    """Ready integration pull requests must keep both verdict contracts intact."""

    READY = ("github.event_name == 'pull_request' && !github.event.pull_request.draft && "
             "(contains(fromJSON('[\"main\",\"master\"]'), github.event.pull_request.base.ref) || "
             "startsWith(github.event.pull_request.base.ref, 'integration/')) || github.event_name == 'push' || "
             "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'")

    def test_both_verdicts_run_for_every_event_but_a_draft(self):
        # TSK-449 AC-6: a draft's verdicts are skipped (neutral); every
        # other event gets a verdict, and a weaker or missing guard is drift.
        for name in ("gates-verdict", "windows-verdict"):
            job = parity.workflow_jobs(WORKFLOW)[name]
            condition = re.search(r"^ {4}if:\s*(.+?)\s*$", job, re.M).group(1)
            self.assertEqual(condition, GatePartControls.DRAFT_NEUTRAL)
        for old, new in (("    if: ${{ always() && !(github.event_name == 'pull_request' && github.event.pull_request.draft) }}\n    name: windows\n",
                          "    if: always()\n    name: windows\n"),
                         ("    if: ${{ always() && !(github.event_name == 'pull_request' && github.event.pull_request.draft) }}\n    name: windows\n",
                          "    if: ${{ always() && !github.event.pull_request.draft }}\n    name: windows\n")):
            self.assertIn(old, WORKFLOW)
            problems = parity.windows_verdict_problems(WORKFLOW.replace(old, new))
            self.assertTrue(any("must be exactly the Windows verdict job" in p for p in problems), problems)

    def test_a_draft_skips_every_gate_job(self):
        # Every gate job's pull request clause requires a non-draft, whatever
        # the base, so a draft runs no gate and spends no runner on one.
        self.assertTrue(self.READY.startswith(
            "github.event_name == 'pull_request' && !github.event.pull_request.draft && ("))

    def test_schedule_and_dispatch_keep_unconditional_security_jobs(self):
        trigger = WORKFLOW.split("permissions:", 1)[0]
        self.assertIn("  schedule:\n", trigger)
        self.assertIn("  workflow_dispatch:", trigger)
        for name in ("secret-scan", "gitleaks-canary", "security-review"):
            job = parity.workflow_jobs(WORKFLOW)[name]
            self.assertIsNone(re.search(r"^ {4}if:", job, re.M), name)
            self.assertNotIn("    needs:", job, name)

    def test_schedule_and_dispatch_select_every_expensive_job(self):
        for name in ("gates", "gates-coverage", "gates-coverage-report", "windows-build",
                     "windows-tests", "windows", "windows-release-journey", "windows-journeys"):
            job = parity.workflow_jobs(WORKFLOW)[name]
            condition = re.search(r"^ {4}if:\s*(.+?)\s*$", job, re.M).group(1)
            for event in ("schedule", "workflow_dispatch"):
                self.assertIn(f"github.event_name == '{event}'", condition, name)

    def ready_workflow(self):
        return WORKFLOW.replace(gate_event_condition(WORKFLOW), self.READY)

    def test_ready_integration_conditions_keep_both_verdict_contracts(self):
        self.assertEqual(gate_event_condition(WORKFLOW), self.READY)
        workflow = self.ready_workflow()
        self.assertEqual(parity.gate_part_problems(CONFIG, workflow), [])
        self.assertEqual(parity.windows_verdict_problems(workflow), [])

    def test_a_windows_verdict_with_a_different_event_condition_is_refused(self):
        workflow = self.ready_workflow()
        job = parity.workflow_jobs(workflow)["windows-verdict"]
        narrowed = job.replace("    if: ${{ always() && ", "    if: ${{ always() && github.event_name == 'push' && ")
        self.assertNotEqual(job, narrowed)
        problems = parity.windows_verdict_problems(workflow.replace(job, narrowed))
        self.assertTrue(any("must be exactly the Windows verdict job" in p for p in problems), problems)


class CoverageShardControls(unittest.TestCase):
    """TSK-270 AC-1 to AC-3: the shards run the rust-coverage command on
    every planned shard once, after its prerequisite, and the report reads
    the floor from the config and the journeys from the shards' results."""

    EXACT = "must be exactly this, built from .codeflow/test-config.json"

    def problems(self, workflow: str = WORKFLOW, cfg: dict = CONFIG, data: Path | None = None) -> list[str]:
        jobs = parity.workflow_jobs(workflow)
        guarded = parity.GATES_IF.match(re.search(r"^ {4}if:\s*(.+?)\s*$", jobs["gates"], re.M).group(1))
        return parity.coverage_job_problems(cfg, jobs, guarded.group(1), data)

    def assert_problem(self, needle: str, workflow: str = WORKFLOW, cfg: dict = CONFIG, data=None) -> None:
        problems = self.problems(workflow, cfg, data)
        self.assertTrue(any(needle in p for p in problems), f"{needle} not in {problems}")

    def test_the_committed_jobs_match_the_config_and_plan(self):
        self.assertEqual(self.problems(), [])
        self.assertEqual(parity.planned_partitions(parity.SHARD_DATA), 2)
        self.assertIn("        shard: [1, 2]\n", WORKFLOW)

    def test_a_shard_that_runs_less_or_another_command_is_refused(self):
        run = 'cargo llvm-cov nextest --workspace --no-fail-fast --no-report --profile codeflow -E'
        self.assertIn(run, WORKFLOW)
        for changed in (run.replace("--workspace", "-p codeflow-core"), run.replace("--no-fail-fast ", ""),
                        run.replace("--profile codeflow", "--profile default"),
                        run.replace(" -E", " --lib -E")):
            with self.subTest(changed=changed):
                self.assert_problem(self.EXACT, WORKFLOW.replace(run, changed))
        # The filter is the judge job's selection for this shard (TSK-1001):
        # another slot, a filter computed here or no emptiness check is refused.
        shard = "FILTER: ${{ fromJSON(needs.judge.outputs.shards)[format('{0}', matrix.shard)] }}"
        self.assertIn(shard, WORKFLOW)
        self.assert_problem(self.EXACT, WORKFLOW.replace(shard, shard.replace("matrix.shard", "'1'")))
        self.assert_problem(self.EXACT, WORKFLOW.replace(
            shard, 'FILTER: $(python3 -B scripts/coverage-shards.py filter ${{ matrix.shard }})'))
        self.assert_problem(self.EXACT, WORKFLOW.replace("    needs: [reuse, judge]\n", "    needs: reuse\n"))
        self.assert_problem(self.EXACT, WORKFLOW.replace('        run: test -n "$FILTER"\n      - name: Remove old',
                                                         '        run: true\n      - name: Remove old'))
        self.assert_problem(self.EXACT, WORKFLOW.replace("        run: cargo llvm-cov clean --workspace\n",
                                                         "        run: true\n"))
        self.assert_problem(self.EXACT, WORKFLOW.replace("          LLVM_PROFILE_FILE_NAME: codeflow-%4m.profraw\n"
                                                         "        run: cargo llvm-cov",
                                                         "          LLVM_PROFILE_FILE_NAME: codeflow-%p.profraw\n"
                                                         "        run: cargo llvm-cov"))

    def test_a_shard_set_that_misses_or_exceeds_the_plan_is_refused(self):
        for matrix in ("        shard: [1]\n", "        shard: [1, 2, 3]\n", "        shard: [1, 1]\n"):
            with self.subTest(matrix=matrix):
                self.assert_problem(self.EXACT, WORKFLOW.replace("        shard: [1, 2]\n", matrix))
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory) / "times.json"
            data.write_text(json.dumps({"partitions": 3}))
            self.assert_problem(self.EXACT, data=data)

    def test_the_config_commands_drive_the_jobs(self):
        def narrowed(c):
            target(c, "rust-coverage")["modes"]["full"]["command"] += " --lib"
        self.assert_problem("the rust-coverage target must run", cfg=mutated(narrowed))
        def threshold(c):
            target(c, "rust-coverage")["modes"]["full"]["command"] = (
                "cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 80 --profile codeflow")
        self.assertEqual(self.problems(cfg=mutated(threshold)), [])
        def web(c):
            target(c, "present-web-build")["modes"]["full"]["command"] += " && true"
        self.assert_problem(self.EXACT, cfg=mutated(web))
        def journey(c):
            target(c, "journey-gate")["modes"]["full"]["command"] = "python3 -B scripts/journey-gate.py --list"
        self.assert_problem(self.EXACT, cfg=mutated(journey))
        def requires(c):
            target(c, "rust-coverage")["requires"] = ["present-web-build", "rust-build"]
        self.assert_problem("must require exactly present-web-build", cfg=mutated(requires))

    def test_a_report_that_could_pass_without_every_shard_or_the_floor_is_refused(self):
        merge = '--floor-from "$CODEFLOW_JUDGE/.codeflow/test-config.json" --shards 2 target/coverage/data'
        self.assertIn(merge, WORKFLOW)
        controls = {
            "a fixed floor": WORKFLOW.replace(merge, "--floor 80 --shards 2 target/coverage/data"),
            "one shard": WORKFLOW.replace(merge, merge.replace("--shards 2", "--shards 1")),
            "the pull request's floors": WORKFLOW.replace(merge, "--floor-from .codeflow/test-config.json --shards 2 "
                                                                 "target/coverage/data"),
            "a forgiven merge": WORKFLOW.replace(merge, merge + " || true"),
            "a skipped journey step": WORKFLOW.replace(
                "      - name: journey gate\n        if: ${{ !cancelled() }}\n",
                "      - name: journey gate\n        if: false\n"),
            "one shard's list": WORKFLOW.replace(" target/coverage/results/coverage-shard-2/tests.json", ""),
            "a report that runs on skipped shards": WORKFLOW.replace(
                "    if: always() && needs.gates-coverage.result != 'skipped' && (",
                "    if: always() && ("),
            "continue-on-error on the report": WORKFLOW.replace(
                "    needs: gates-coverage\n", "    needs: gates-coverage\n    continue-on-error: true\n"),
        }
        for label, workflow in controls.items():
            with self.subTest(label):
                self.assertNotEqual(workflow, WORKFLOW)
                self.assert_problem(self.EXACT, workflow)

    def test_a_part_that_names_a_coverage_target_is_refused(self):
        workflow = WORKFLOW.replace("only: prose-exhaustive\n", "only: prose-exhaustive,rust-coverage\n")
        problems = parity.gate_part_problems(CONFIG, workflow)
        self.assertTrue(any("which the coverage jobs run" in p for p in problems), problems)

    def test_without_the_coverage_jobs_their_targets_go_unrun(self):
        start = WORKFLOW.index("  gates-coverage:\n")
        end = WORKFLOW.index("  gates-verdict:\n")
        problems = parity.gate_part_problems(CONFIG, WORKFLOW[:start] + WORKFLOW[end:])
        self.assertTrue(any("`gates-coverage` job must be exactly" in p for p in problems), problems)


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
    touches Playwright, through the pinned steps (PLAYWRIGHT_STEPS)."""

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
        journeys = "    runs-on: windows-latest\n    timeout-minutes: 10\n"
        self.assertEqual(WORKFLOW.count(journeys), 1)
        self.assert_problem(WORKFLOW.replace(journeys, "    runs-on: windows-latest\n"),
                            "job 'windows-journeys'", "timeout-minutes")
        for value in ("0", "${{ 360 }}", "'30'", "30.5"):
            with self.subTest(value=value):
                self.assert_problem(WORKFLOW.replace(journeys, f"    runs-on: windows-latest\n    timeout-minutes: {value}\n"),
                                    "job 'windows-journeys'")
        self.assert_problem(WORKFLOW.replace(journeys, journeys + "    timeout-minutes: 10\n"),
                            "job 'windows-journeys'", "exactly one")

    def test_a_timeout_above_the_ac1_bound_is_refused(self):
        """AC-1: a hung job frees its runner inside 45 minutes, so GitHub's
        six-hour default of 360 must not pass as a "positive whole number"."""
        journeys = "    runs-on: windows-latest\n    timeout-minutes: 10\n"
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
            text = (ROOT / relative).read_text(encoding="utf-8")
            values += [int(v) for v in re.findall(r"^ {4}timeout-minutes: (\d+)$", text, re.M)]
        self.assertTrue(values)
        self.assertLessEqual(max(values), parity.MAX_TIMEOUT_MINUTES)

    INSTALL_HEAD = "      - name: Install Playwright browsers from the lockfile\n"
    BROWSERS = 'if timeout --kill-after=30s 10m "$node" "$cli" install chromium firefox webkit'
    CLI = "          cli=crates/codeflow-present/web/node_modules/playwright-core/cli.js\n"

    def install_step(self) -> str:
        start = WORKFLOW.index(self.INSTALL_HEAD)
        end = WORKFLOW.index("          exit 1\n", start) + len("          exit 1\n")
        return WORKFLOW[start:end]

    def assert_unpinned(self, workflow: str) -> None:
        """Refused, and the message names the pin a deliberate change edits."""
        self.assert_problem(workflow, "PLAYWRIGHT_STEPS", "scripts/gate-parity.py")

    def test_a_line_broken_install_without_a_step_timeout_is_refused(self):
        """Round 9: the package and `install` on two lines, with no step
        timeout, matched no same-line spelling and passed."""
        broken = (self.INSTALL_HEAD + "        if: matrix.part == 'present'\n        run: |\n"
                  "          npx playwright \\\n            install chromium firefox webkit\n")
        self.assert_unpinned(WORKFLOW.replace(self.install_step(), broken))

    def test_a_timeout_that_wraps_another_command_is_refused(self):
        """Round 9: `timeout` wrapped `echo` on the install's own line."""
        self.assertEqual(WORKFLOW.count(self.BROWSERS), 1)
        self.assert_unpinned(WORKFLOW.replace(
            self.BROWSERS, "if timeout --kill-after=30s 10m echo skip; npx playwright install chromium"))

    def test_an_added_install_through_a_variable_is_refused(self):
        self.assertEqual(WORKFLOW.count(self.CLI), 1)
        self.assert_unpinned(WORKFLOW.replace(
            self.CLI, self.CLI + '          PW=playwright\n          npx "$PW" install chromium\n'))

    def test_a_new_step_naming_playwright_is_refused(self):
        step = ("      - name: Warm the Playwright cache\n        if: matrix.part == 'present'\n"
                "        run: ls ~/.cache/ms-playwright\n")
        self.assert_unpinned(WORKFLOW.replace(self.INSTALL_HEAD, step + self.INSTALL_HEAD))
        upper = step.replace("Playwright", "PLAYWRIGHT").replace("ms-playwright", "ms-browsers")
        self.assert_unpinned(WORKFLOW.replace(self.INSTALL_HEAD, upper + self.INSTALL_HEAD))
        self.assert_unpinned(WORKFLOW.replace(self.install_step(), self.install_step() * 2))

    def test_any_edit_to_a_pinned_step_is_refused(self):
        """The forms the earlier text checks chased are each a change to the
        pinned text, so each is refused the same way."""
        install = self.install_step()
        root = "sudo timeout --kill-after=30s 10m "
        cases = {
            "step timeout 360": install.replace("timeout-minutes: 25", "timeout-minutes: 360"),
            "no step timeout": install.replace("        timeout-minutes: 25\n", ""),
            "browser install unbounded": install.replace(
                self.BROWSERS, self.BROWSERS.replace("timeout --kill-after=30s 10m ", "")),
            "attempt limit 11m": install.replace(" 10m ", " 11m ", 1),
            "apt-get outside a timeout": install.replace(root, root.split()[0] + " "),
            "one attempt": install.replace("for attempt in 1 2; do", "for attempt in 1; do"),
            "with-deps": install.replace('"$cli" install chromium', '"$cli" install --with-deps chromium'),
            "every part": install.replace("        if: matrix.part == 'present'\n", ""),
            "an extra spelling": install.replace(
                self.BROWSERS, self.BROWSERS + "\n            npx playwright-core@1.49.0 install"),
            "a bounded extra spelling": install.replace(
                self.BROWSERS, self.BROWSERS + "\n            timeout --kill-after=30s 10m npx playwright install"),
        }
        for name, step in cases.items():
            with self.subTest(name=name):
                self.assertNotEqual(step, install)
                self.assert_unpinned(WORKFLOW.replace(install, step))
        cache = "        if: matrix.part == 'present'\n        with:\n          path: ~/.cache/ms-playwright\n"
        self.assertEqual(WORKFLOW.count(cache), 1)
        self.assert_unpinned(WORKFLOW.replace(cache, cache.replace("        if: matrix.part == 'present'\n", "")))
        noted = install.replace("          exit 1\n", "          # a note\n\n          exit 1   \n")
        self.assertEqual(parity.playwright_problems(WORKFLOW.replace(install, noted)), [],
                         "comments, blank lines and trailing spaces are not part of the pin")

    def test_the_pin_keeps_the_ac2_bound(self):
        """A deliberate edit to the pin still keeps AC-2: present only, no
        --with-deps, a step timeout within the job bound, two attempts, and
        each install under a timeout of at most 10 minutes."""
        steps = parity.PLAYWRIGHT_STEPS
        self.assertEqual(len(steps), 4)
        self.assertTrue(all("        if: matrix.part == 'present'\n" in s for s in steps))
        installs = [s for s in steps if s.startswith(self.INSTALL_HEAD)]
        self.assertEqual(len(installs), 1)
        install = installs[0]
        self.assertNotIn("--with-deps", "".join(steps))
        limit = re.search(r"^ {8}timeout-minutes: (\d+)$", install, re.M)
        self.assertTrue(limit and int(limit.group(1)) <= parity.MAX_TIMEOUT_MINUTES)
        self.assertIn("for attempt in 1 2; do", install)
        self.assertIn(self.BROWSERS, install)
        self.assertIn('sudo timeout --kill-after=30s 10m "$node" "$cli" install-deps chromium firefox webkit', install)
        self.assertEqual([s for s in steps if s is not install and " install" in s], [],
                         "only the install step installs")

    def test_playwright_outside_the_gates_job_is_refused(self):
        windows = "      - name: Fetch the test archive\n"
        self.assertEqual(WORKFLOW.count(windows), 1)
        self.assert_problem(WORKFLOW.replace(windows, "      - name: Install Playwright\n        run: npx playwright-core install\n" + windows),
                            "job 'windows-tests'", "Playwright step")


class EveryWorkflowTimeoutControls(unittest.TestCase):
    """TSK-254: the timeout rule covers every workflow the repository runs
    or ships, not only codeflow-ci.yml."""

    OWN = {
        ".github/workflows/codeflow-ci.yml", ".github/workflows/codeflow-policy.yml",
        ".github/workflows/codeflow-release.yml", ".github/workflows/codeflow-release-trusted.yml",
        ".github/workflows/portal-pages.yml", ".github/workflows/release.yml",
        ".github/workflows/release-main-recheck.yml", ".github/workflows/release-plan-authority.yml",
        ".github/workflows/release-post-announce.yml",
    }
    SHIPPED = {"assets/base/ci/codeflow-ci.yml", "assets/base/ci/codeflow-policy.yml"}

    def files(self) -> dict[str, str]:
        return {path.as_posix(): (parity.ROOT / path).read_text(encoding="utf-8") for path in parity.workflow_files()}

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
        return {path.as_posix(): (parity.ROOT / path).read_text(encoding="utf-8") for path in parity.workflow_files()}

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
        windows = "      - name: Fetch the test archive\n"
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


class EveryActionIsPinnedByCommit(unittest.TestCase):
    """Issue 148: a workflow references an action by a full commit, never a
    tag or a branch, in this repository's workflows and the shipped ones."""

    SHA = "d23441a48e516b6c34aea4fa41551a30e30af803"

    def problems(self, line: str, shipped: bool = False) -> list[str]:
        return parity.pin_problems(f"jobs:\n  a:\n    steps:\n{line}\n", "x.yml", shipped)

    def test_the_real_workflows_and_templates_are_pinned(self):
        self.assertEqual(parity.all_pin_problems(), [])

    def test_a_tag_a_branch_or_a_short_ref_is_refused(self):
        for ref in ("v6", "v6.1.0", "main", "d23441a", self.SHA[:39], self.SHA.upper()):
            for shipped in (False, True):
                with self.subTest(ref=ref, shipped=shipped):
                    problems = self.problems(f"      - uses: actions/checkout@{ref} # v6.1.0", shipped)
                    self.assertEqual(len(problems), 1, problems)
                    self.assertIn("x.yml:4", problems[0])
                    self.assertIn("issue 148", problems[0])

    def test_every_spelling_of_the_key_is_read(self):
        for line in ("        uses: actions/cache@v6", "      - 'uses': actions/cache@v6",
                     '      - "uses": "actions/cache@v6"', "      - uses: 'actions/cache@v6' # v6.1.0",
                     "      -   uses:   actions/cache@v6", "        uses: github/codeql-action/init@v4"):
            with self.subTest(line=line):
                self.assertEqual(len(self.problems(line)), 1, line)

    def test_a_commit_passes_with_or_without_a_comment_outside_a_template(self):
        for line in (f"      - uses: actions/checkout@{self.SHA} # v6.1.0",
                     f"      - uses: actions/checkout@{self.SHA}",
                     f'      - uses: "actions/checkout@{self.SHA}"',
                     f"        uses: actions/cache/restore@{self.SHA} # v6.1.0",
                     "      - uses: ./.github/actions/prepare",
                     "    uses: ./.github/workflows/release-plan-authority.yml",
                     "      - uses: docker://alpine@sha256:" + "a" * 64):
            with self.subTest(line=line):
                self.assertEqual(self.problems(line), [], line)

    def test_a_docker_image_is_pinned_by_digest(self):
        self.assertEqual(len(self.problems("      - uses: docker://alpine:3.20")), 1)
        self.assertEqual(len(self.problems("      - uses: docker://alpine@sha256:abc")), 1)

    def test_a_shipped_template_also_names_the_release(self):
        for comment in ("", " # v6", " # main", " #"):
            with self.subTest(comment=comment):
                problems = self.problems(f"      - uses: actions/checkout@{self.SHA}{comment}", shipped=True)
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("release comment", problems[0])
        self.assertEqual(self.problems(f"      - uses: actions/checkout@{self.SHA} # v6.1.0", shipped=True), [])
        self.assertEqual(self.problems("      - uses: ./.github/actions/prepare", shipped=True), [])

    def test_the_tag_form_of_each_real_workflow_is_refused(self):
        # The 3.1.0 workflows used `@v6`; putting it back on any pinned line
        # of any real file must fail.
        checked = 0
        for relative in parity.workflow_files():
            text = (parity.ROOT / relative).read_text(encoding="utf-8")
            shipped = relative.parts[0] != ".github"
            for number, line in enumerate(text.splitlines()):
                found = parity.USES_LINE.match(line)
                if found is None or found.group("value").startswith("./"):
                    continue
                with self.subTest(file=relative.as_posix(), line=number + 1):
                    lines = text.splitlines()
                    lines[number] = re.sub(r"@[0-9a-f]{40}", "@v6", line)
                    self.assertTrue(parity.pin_problems("\n".join(lines), relative.as_posix(), shipped),
                                    line)
                    checked += 1
        self.assertGreaterEqual(checked, 50)


if __name__ == "__main__":
    unittest.main()

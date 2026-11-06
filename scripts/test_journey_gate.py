#!/usr/bin/env python3
"""Controls for scripts/journey-gate.py (TSK-110 R110-2).

Each control runs the gate's own Python runner on a scratch unittest script.
Only a run where every listed class executed and nothing was skipped or
expected to fail is a passing journey.
"""

import importlib.util
import tempfile
import textwrap
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("journey_gate", ROOT / "scripts" / "journey-gate.py")
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

SCRIPT = textwrap.dedent(
    '''
    import unittest


    class Passing(unittest.TestCase):
        def test_one(self):
            pass

        def test_documented(self):
            """A docstring puts the status on its own line."""


    class AlsoPassing(unittest.TestCase):
        def test_one(self):
            pass


    @unittest.skip("platform")
    class SkippedClass(unittest.TestCase):
        def test_one(self):
            pass


    class SkippedMethod(unittest.TestCase):
        def test_runs(self):
            pass

        @unittest.skip("platform")
        def test_skipped(self):
            pass


    class SkippedSetup(unittest.TestCase):
        @classmethod
        def setUpClass(cls):
            raise unittest.SkipTest("platform")

        def test_one(self):
            pass


    class ExpectedFailure(unittest.TestCase):
        @unittest.expectedFailure
        def test_known(self):
            self.fail("known")


    class Failing(unittest.TestCase):
        def test_one(self):
            self.fail("broken")


    class Empty(unittest.TestCase):
        pass


    if __name__ == "__main__":
        unittest.main()
    '''
)


class NextestFilterTests(unittest.TestCase):
    """The reduced Windows tier runs exactly the journey tests by this filter."""

    JOURNEYS = [
        {"text": "both", "control": ["codeflow-core --test id_registry first", "codeflow-cli --lib second"],
         "fault": ["python scripts/test_release.py Release", "codeflow-core --test id_registry first"]},
        {"text": "unix", "platform": "unix", "control": ["codeflow-core --test unix_only third"], "fault": []},
        {"text": "later", "pending": "TSK-1", "control": ["codeflow-core --test later fourth"], "fault": []},
        {"text": "bench", "benchmark": True, "control": ["codeflow-core --test bench fifth"], "fault": []},
    ]

    def test_selects_each_rust_test_by_binary_and_exact_name(self):
        groups, _, _ = gate.plan(self.JOURNEYS, on_unix=False, with_benchmark=False)
        self.assertEqual(gate.nextest_filter(groups),
                         "(binary_id(=codeflow-core::id_registry) & (test(=first))) | "
                         "(binary_id(=codeflow-cli) & (test(=second)))")

    def test_a_platform_keeps_its_own_journeys(self):
        groups, _, _ = gate.plan(self.JOURNEYS, on_unix=True, with_benchmark=False)
        self.assertIn("binary_id(=codeflow-core::unix_only)", gate.nextest_filter(groups))

    def test_no_rust_journey_is_refused(self):
        groups, _, _ = gate.plan([self.JOURNEYS[0] | {"control": [], "fault": ["python a.py B"]}],
                                 on_unix=False, with_benchmark=False)
        with self.assertRaises(ValueError):
            gate.nextest_filter(groups)

    def test_the_real_map_gives_a_filter_with_no_python_clause(self):
        groups, _, _ = gate.plan(gate.load_journeys(), on_unix=False, with_benchmark=False)
        selected = gate.nextest_filter(groups)
        names = sum(len(names) for key, names in groups.items() if key[0] != "python")
        self.assertEqual(selected.count("test(="), names)
        self.assertNotIn("python", selected)


class PythonJourneyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.TemporaryDirectory()
        cls.script = Path(cls.dir.name) / "journeys.py"
        cls.script.write_text(SCRIPT)

    @classmethod
    def tearDownClass(cls):
        cls.dir.cleanup()

    def verdict(self, *names):
        with redirect_stdout(StringIO()):
            return gate.run(("python", str(self.script)), list(names), None)

    def test_listed_classes_that_all_ran_pass(self):
        self.assertIsNone(self.verdict("Passing", "AlsoPassing"))

    def test_a_skipped_class_fails(self):
        failure = self.verdict("Passing", "SkippedClass")
        self.assertIsNotNone(failure)
        self.assertIn("skipped=", failure)

    def test_a_skipped_method_fails(self):
        self.assertIn("skipped=", self.verdict("SkippedMethod") or "")

    def test_a_class_skipped_in_its_setup_fails(self):
        self.assertIn("skipped=", self.verdict("SkippedSetup") or "")

    def test_an_expected_failure_fails(self):
        self.assertIn("expected failures=", self.verdict("ExpectedFailure") or "")

    def test_a_failing_class_fails(self):
        self.assertIn("exit", self.verdict("Passing", "Failing") or "")

    def test_a_listed_class_with_no_test_fails(self):
        failure = self.verdict("Passing", "Empty")
        self.assertIsNotNone(failure)
        self.assertIn("no test ran in Empty", failure)


class RustResultTests(unittest.TestCase):
    def verdict(self, xml, names=None):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            if xml is not None:
                report.write_text(xml)
            return gate.result_failure(("cargo", "codeflow-cli", "--test", "journeys", False), names or ["passes"], report)

    def test_exact_package_target_and_name_pass(self):
        self.assertIsNone(self.verdict('<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"/></testsuite></testsuites>'))

    def test_missing_renamed_wrong_target_skipped_failed_duplicate_or_truncated_fail(self):
        for xml in [None, '<testsuites/>', '<testsuites><testsuite name="codeflow-cli::other"><testcase name="passes"/></testsuite></testsuites>',
                    '<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="renamed"/></testsuite></testsuites>',
                    '<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"><skipped/></testcase></testsuite></testsuites>',
                    '<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"><failure/></testcase></testsuite></testsuites>',
                    '<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"/><testcase name="passes"/></testsuite></testsuites>',
                    '<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"/>']:
            with self.subTest(xml=xml):
                self.assertIsNotNone(self.verdict(xml))

    def test_stale_results_fail(self):
        import os
        import time
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text('<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"/></testsuite></testsuites>')
            os.utime(report, (1, 1))
            with patch.dict(os.environ, {"CODEFLOW_GATE_STARTED_AT": str(time.time())}):
                self.assertIn("stale", gate.result_failure(("cargo", "codeflow-cli", "--test", "journeys", False), ["passes"], report))

    def test_results_never_spawn_cargo(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text('<testsuites><testsuite name="codeflow-cli::journeys"><testcase name="passes"/></testsuite></testsuites>')
            with patch.object(gate.subprocess, "run", side_effect=AssertionError("cargo was rerun")):
                self.assertIsNone(gate.run(("cargo", "codeflow-cli", "--test", "journeys", False), ["passes"], None, report))


class PartTests(unittest.TestCase):
    """TSK-265: the Windows Python journeys run beside the partitions and the
    results part runs after them; together they run each journey once."""

    def groups(self, on_unix):
        groups, _, _ = gate.plan(gate.load_journeys(), on_unix=on_unix, with_benchmark=False)
        return groups

    def test_the_two_parts_run_every_group_exactly_once(self):
        for on_unix in (True, False):
            groups = self.groups(on_unix)
            python, python_other = gate.part(groups, "python")
            results, results_other = gate.part(groups, "results")
            with self.subTest(on_unix=on_unix):
                self.assertTrue(python and results)
                self.assertEqual(dict(python), dict(results_other))
                self.assertEqual(dict(results), dict(python_other))
                self.assertFalse(set(python) & set(results))
                self.assertEqual(set(python) | set(results), set(groups))
                self.assertTrue(all(key[0] == "python" for key in python))
                self.assertTrue(all(key[0] == "cargo" for key in results))

    def test_no_part_runs_everything_and_an_unknown_part_is_refused(self):
        groups = self.groups(True)
        everything, elsewhere = gate.part(groups, None)
        self.assertEqual(everything, groups)
        self.assertFalse(elsewhere)
        with self.assertRaises(ValueError):
            gate.part(groups, "rust")

    def test_the_python_part_needs_no_results(self):
        from unittest.mock import patch
        groups, _ = gate.part(self.groups(False), "python")
        with patch.object(gate, "result_failure", side_effect=AssertionError("results were read")), \
                patch.object(gate.subprocess, "run", return_value=gate.subprocess.CompletedProcess([], 0, "OK\n")), \
                patch.object(gate, "python_failure", return_value=None), redirect_stdout(StringIO()):
            for key, names in groups.items():
                self.assertIsNone(gate.run(key, names, None, None))


class PendingCheckTests(unittest.TestCase):
    """A pending journey names a task that is not complete (TSK-1151).

    The check reads task records, so a records-only landing runs it through
    the docs-validation record check.
    """

    def check(self, status: str | None, control: list[str] | None = None, record: str | None = None) -> list[str]:
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            tasks = root / "project-management" / "tasks"
            tasks.mkdir(parents=True)
            if record is not None:
                (tasks / "TSK-900.md").write_text(record)
            elif status is not None:
                (tasks / "TSK-900.md").write_text(f"---\nid: TSK-900\nstatus: {status}  # note\n---\n")
            journeys = [
                {"text": "a shipped journey", "control": ["a"], "fault": ["b"]},
                {"text": "a later journey", "pending": "TSK-900", "control": control or [], "fault": []},
            ]
            return gate.pending_problems(journeys, root)

    def test_an_open_pending_task_passes(self):
        for status in ("todo", "blocked", "in_progress", "'todo'", '"in_progress"'):
            with self.subTest(status=status):
                self.assertEqual(self.check(status), [])

    def test_a_quoted_complete_status_fails(self):
        # The record parser reads the field as YAML, so a quoted spelling is
        # complete there too (Grok review of a9ad817e6, finding 1).
        for status in ('"complete"', "'complete'"):
            with self.subTest(status=status):
                problems = self.check(status)
                self.assertEqual(len(problems), 1)
                self.assertIn("pending on TSK-900, which is complete", problems[0])

    def test_a_record_without_a_status_line_fails_closed(self):
        problems = self.check(None, record="---\nid: TSK-900\n---\n")
        self.assertEqual(len(problems), 1)
        self.assertIn("no status line", problems[0])

    def test_an_unreadable_status_fails_closed(self):
        cases = {
            "an unknown word": "---\nid: TSK-900\nstatus: done\n---\n",
            "an escape": '---\nid: TSK-900\nstatus: "compl\\x65te"\n---\n',
            "an unclosed quote": "---\nid: TSK-900\nstatus: 'complete\n---\n",
            "a tag": "---\nid: TSK-900\nstatus: !!str complete\n---\n",
            "two status lines": "---\nid: TSK-900\nstatus: todo\nstatus: complete\n---\n",
            "no front matter": "status: complete\n",
        }
        for name, record in cases.items():
            with self.subTest(case=name):
                problems = self.check(None, record=record)
                self.assertEqual(len(problems), 1)
                self.assertIn("whose status cannot be read", problems[0])

    def test_a_completed_pending_task_fails(self):
        problems = self.check("complete")
        self.assertEqual(len(problems), 1)
        self.assertIn("pending on TSK-900, which is complete", problems[0])

    def test_a_pending_journey_that_names_tests_fails(self):
        problems = self.check("todo", control=["codeflow-cli --test x y"])
        self.assertEqual(len(problems), 1)
        self.assertIn("names no tests", problems[0])

    def test_a_missing_pending_record_fails_closed(self):
        problems = self.check(None)
        self.assertEqual(len(problems), 1)
        self.assertIn("TSK-900", problems[0])

    def test_the_live_map_has_no_stale_pending_journey(self):
        self.assertEqual(gate.pending_problems(gate.load_journeys(), gate.ROOT), [])


if __name__ == "__main__":
    unittest.main()

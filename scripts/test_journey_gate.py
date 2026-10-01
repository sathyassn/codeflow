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


if __name__ == "__main__":
    unittest.main()

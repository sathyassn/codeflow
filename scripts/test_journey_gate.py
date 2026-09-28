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


if __name__ == "__main__":
    unittest.main()

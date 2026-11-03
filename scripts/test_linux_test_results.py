#!/usr/bin/env python3
"""Controls for whole-suite membership before the Linux coverage split."""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("linux-test-results.py")
SPEC = importlib.util.spec_from_file_location("linux_test_results", SCRIPT)
results = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(results)


class LinuxTestResultsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.listing = self.root / "tests.json"
        self.report = self.root / "junit.xml"
        self.listing.write_text(json.dumps({"rust-suites": {
            "core": {"testcases": {"one": {"ignored": False}, "ignored": {"ignored": True}}},
            "cli::journey": {"testcases": {"one": {"ignored": False}}},
        }}))
        self.xml()

    def xml(self, core='<testcase name="one"/>', cli='<testcase name="one"/>'):
        self.report.write_text('<testsuites><testsuite name="core">' + core
                               + '</testsuite><testsuite name="cli::journey">' + cli
                               + '</testsuite></testsuites>')

    def test_full_suite_passes_with_binary_identity_and_ignored_test_excluded(self):
        self.assertEqual(results.check(self.listing, self.report), [])

    def test_missing_test_is_refused(self):
        self.xml(cli="")
        self.assertIn("cli::journey one has 0 results", "\n".join(results.check(self.listing, self.report)))

    def test_duplicate_test_is_refused(self):
        self.xml(core='<testcase name="one"/><testcase name="one"/>')
        self.assertIn("core one has 2 results", "\n".join(results.check(self.listing, self.report)))

    def test_unlisted_test_is_refused(self):
        self.xml(core='<testcase name="one"/><testcase name="extra"/>')
        self.assertIn("core extra ran but is not in the list", "\n".join(results.check(self.listing, self.report)))

    def test_failure_skip_and_retry_are_refused(self):
        for tag in ("failure", "error", "skipped", "flakyFailure", "flakyError"):
            with self.subTest(tag=tag):
                self.xml(core=f'<testcase name="one"><{tag}/></testcase>')
                self.assertIn("did not pass on its first attempt", "\n".join(results.check(self.listing, self.report)))

    def test_empty_listing_cannot_pass(self):
        self.listing.write_text('{"rust-suites": {}}')
        self.assertEqual(results.check(self.listing, self.report),
                         ["the nextest list contains no non-ignored tests"])

    def test_single_suite_root_is_supported(self):
        self.listing.write_text('{"rust-suites": {"core": {"testcases": {"one": {}}}}}')
        self.report.write_text('<testsuite name="core"><testcase name="one"/></testsuite>')
        self.assertEqual(results.check(self.listing, self.report), [])

    def test_cli_refuses_missing_and_malformed_evidence(self):
        for value in (None, '<testsuites>', '<unexpected/>'):
            with self.subTest(value=value):
                if value is None:
                    self.report.unlink(missing_ok=True)
                else:
                    self.report.write_text(value)
                done = subprocess.run([sys.executable, "-B", str(SCRIPT), "--list", str(self.listing),
                                       "--results", str(self.report)], capture_output=True, text=True)
                self.assertEqual(done.returncode, 1)
                self.assertIn("FAILED", done.stdout)


if __name__ == "__main__":
    unittest.main()

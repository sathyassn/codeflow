#!/usr/bin/env python3
"""Controls for scripts/partition-check.py (TSK-1321 AC-2).

The hash must place each test where `cargo nextest run --partition
hash:<n>/<count>` runs it, and `check` must fail a run where a listed test
ran in no partition, in two, outside its hash partition or without being
listed. A partition without a report must be named as not finished, never
read as failing tests.
"""

import importlib.util
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("partition_check", ROOT / "scripts" / "partition-check.py")
pc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pc)

# Observed with cargo-nextest 0.9.129: `cargo nextest list --partition
# hash:<n>/16` on a scratch crate placed each of these tests in partition n.
NEXTEST_16 = {
    "case_33_name": 1, "case_52_name": 1, "case_56_name": 2, "nested::deep_x": 9,
    "quick_neighbour": 15, "sleeps_past_the_bound": 15,
    "outer::inner::a_long_test_name_that_is_well_past_thirty_two_bytes_one": 1,
    "outer::inner::a_long_test_name_that_is_well_past_thirty_two_bytes_two_and_then_some_more_words": 8,
    "outer::inner::a_failure_of_the_kill_on_close_job_ends_the_suspended_target": 12,
}
COUNT = 4
TESTS = [("core", f"unit::case_{i}") for i in range(24)] + [("cli::journey", f"flow_{i}") for i in range(8)]


def listing(tests, ignored=()):
    suites = {}
    for binary, name in [*tests, *ignored]:
        suites.setdefault(binary, {"testcases": {}})["testcases"][name] = {"ignored": (binary, name) in ignored}
    return json.dumps({"rust-suites": suites})


def junit(tests, seconds=60.0):
    suites = {}
    for binary, name in tests:
        suites.setdefault(binary, []).append(name)
    body = "".join(f'<testsuite name="{binary}">' + "".join(f'<testcase name="{n}" time="1.5"/>' for n in names)
                   + "</testsuite>" for binary, names in suites.items())
    return f'<testsuites time="{seconds}">{body}</testsuites>'


def by_partition(tests, count=COUNT):
    out = {n: [] for n in range(1, count + 1)}
    for binary, name in tests:
        out[pc.partition_of(name, count)].append((binary, name))
    return out


class RunDirectory:
    """A results directory as download-artifact writes it."""

    def __init__(self, test: unittest.TestCase):
        self.dir = tempfile.TemporaryDirectory()
        test.addCleanup(self.dir.cleanup)
        self.root = Path(self.dir.name)
        self.results = self.root / "results"
        self.results.mkdir()
        self.list = self.root / "tests.json"
        self.list.write_text(listing(TESTS), encoding="utf-8")

    def write(self, partition, tests, prefix="windows-junit", seconds=60.0):
        folder = self.results / f"{prefix}-{partition}"
        folder.mkdir()
        (folder / "junit.xml").write_text(junit(tests, seconds), encoding="utf-8")

    def run(self, *args):
        out = StringIO()
        with redirect_stdout(out):
            code = pc.main(list(args))
        return code, out.getvalue()


class HashTests(unittest.TestCase):
    def test_xxh64_matches_the_published_vectors(self):
        self.assertEqual(pc.xxh64(b""), 0xEF46DB3751D8E999)
        self.assertEqual(pc.xxh64(b"a"), 0xD24EC4F1A98C6E5B)
        self.assertEqual(pc.xxh64(b"abc"), 0x44BC2CF5AD770999)
        self.assertEqual(pc.xxh64(b"Nobody inspects the spammish repetition"), 0xFBCEA83C8A378BF1)

    def test_the_partition_is_the_one_nextest_runs_the_test_in(self):
        for name, partition in NEXTEST_16.items():
            self.assertEqual(pc.partition_of(name, 16), partition, name)

    def test_every_partition_is_inside_the_count(self):
        self.assertEqual({pc.partition_of(f"t{i}", 16) for i in range(400)}, set(range(1, 17)))


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.run_dir = RunDirectory(self)
        self.planned = by_partition(TESTS)

    def write_all(self, plan=None, seconds=None):
        for partition, tests in (plan or self.planned).items():
            self.run_dir.write(partition, tests, seconds=(seconds or {}).get(partition, 60.0))

    def check(self, margin="24", setup="2"):
        return self.run_dir.run("check", "--count", str(COUNT), "--prefix", "windows-junit",
                                "--margin-minutes", margin, "--setup-minutes", setup,
                                "--list", str(self.run_dir.list), "--results", str(self.run_dir.results))

    def test_a_partition_past_the_24_minute_margin_fails_and_one_at_it_passes(self):
        # TSK-1063: 22 test minutes plus 2 of setup is the margin itself; 22.5 passes it.
        self.write_all(seconds={1: 22 * 60, 2: 22.5 * 60})
        code, out = self.check()
        self.assertEqual(code, 1, out)
        self.assertIn("partition 2 took 22.5 test minutes, about 24.5 job minutes with 2 of setup, past the 24 "
                      "minute margin of the 30 minute job limit (TSK-1063)", out)
        self.assertNotIn("partition 1 took", out)

    def test_the_margin_and_setup_are_required_and_checked(self):
        self.write_all()
        for args in (("--setup-minutes", "2"), ("--margin-minutes", "24")):
            with self.subTest(args=args), self.assertRaises(SystemExit), redirect_stderr(StringIO()):
                self.run_dir.run("check", "--count", str(COUNT), "--prefix", "windows-junit", *args,
                                 "--list", str(self.run_dir.list), "--results", str(self.run_dir.results))
        for margin, setup in (("0", "2"), ("24", "-1")):
            with self.subTest(margin=margin, setup=setup), self.assertRaises(SystemExit) as refused:
                self.check(margin, setup)
            self.assertIn("--margin-minutes must be above 0", str(refused.exception.code))

    def test_a_run_by_hash_passes_and_prints_each_partitions_minutes_and_the_slowest_test(self):
        self.write_all()
        code, out = self.check()
        self.assertEqual(code, 0, out)
        self.assertIn(f"{len(TESTS)} listed tests, {len(TESTS)} ran in {COUNT} of {COUNT} partitions", out)
        self.assertIn("test minutes per partition {1: 1.0, 2: 1.0, 3: 1.0, 4: 1.0}", out)
        self.assertIn("slowest test", out)

    def test_a_listed_test_that_ran_nowhere_fails(self):
        plan = by_partition(TESTS)
        gone = plan[2].pop()
        self.write_all(plan)
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{gone[0]} {gone[1]} ran in no partition", out)

    def test_a_test_that_ran_in_two_partitions_fails(self):
        plan = by_partition(TESTS)
        twice = plan[1][0]
        plan[3].append(twice)
        self.write_all(plan)
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{twice[0]} {twice[1]} ran in partitions [1, 3]", out)

    def test_a_test_outside_its_hash_partition_fails(self):
        plan = by_partition(TESTS)
        moved = plan[1].pop()
        plan[2].append(moved)
        self.write_all(plan)
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{moved[0]} {moved[1]} ran in partition 2; its hash gives partition 1", out)

    def test_a_test_missing_from_the_list_fails(self):
        plan = by_partition(TESTS)
        extra = ("core", "unit::not_listed")
        plan[pc.partition_of(extra[1], COUNT)].append(extra)
        self.write_all(plan)
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("core unit::not_listed ran but is not in the list of tests", out)

    def test_an_ignored_test_is_not_owed(self):
        self.run_dir.list.write_text(listing(TESTS, ignored=[("core", "unit::ignored")]), encoding="utf-8")
        self.write_all()
        code, out = self.check()
        self.assertEqual(code, 0, out)

    def test_a_missing_report_is_named_once_and_its_tests_are_not_called_failures(self):
        for partition, tests in self.planned.items():
            if partition != 3:
                self.run_dir.write(partition, tests)
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"no JUnit report from partition(s) 3: the job ended without writing its report "
                      f"(a timeout or a cancellation), so its {len(self.planned[3])} listed tests did not finish", out)
        self.assertNotIn("ran in no partition", out)

    def test_lists_that_disagree_are_refused(self):
        other = self.run_dir.root / "other.json"
        other.write_text(listing(TESTS[1:]), encoding="utf-8")
        self.write_all()
        with self.assertRaises(SystemExit) as refused:
            self.run_dir.run("check", "--count", str(COUNT), "--prefix", "windows-junit",
                             "--margin-minutes", "24", "--setup-minutes", "2", "--list",
                             str(self.run_dir.list), str(other), "--results", str(self.run_dir.results))
        self.assertIn("lists other tests", str(refused.exception.code))

    def test_two_reports_for_one_partition_are_refused(self):
        self.write_all()
        (self.run_dir.results / "junit.xml").write_text(junit(self.planned[2]), encoding="utf-8")
        with self.assertRaises(SystemExit) as refused:
            self.check()
        self.assertIn("are both partition 2's report", str(refused.exception.code))


class ReportsTests(unittest.TestCase):
    def setUp(self):
        self.run_dir = RunDirectory(self)
        self.planned = by_partition(TESTS)
        self.output = self.run_dir.root / "github-output"

    def reports(self):
        with patch.dict(os.environ, {"GITHUB_OUTPUT": str(self.output)}):
            return self.run_dir.run("reports", "--count", str(COUNT), "--prefix", "windows-junit",
                                    "--results", str(self.run_dir.results))

    def test_every_report_present_passes_and_writes_an_empty_list(self):
        for partition, tests in self.planned.items():
            self.run_dir.write(partition, tests)
        code, out = self.reports()
        self.assertEqual(code, 0, out)
        self.assertEqual(self.output.read_text(encoding="utf-8"), "missing=\n")

    def test_the_missing_partitions_are_named_for_the_verdict(self):
        for partition in (1, 3):
            self.run_dir.write(partition, self.planned[partition])
        code, out = self.reports()
        self.assertEqual(code, 1)
        self.assertEqual(self.output.read_text(encoding="utf-8"), "missing=2 4\n")
        self.assertIn("::error::partition(s) 2 4 ended without a JUnit report", out)

    def test_a_sole_surviving_report_is_numbered_by_the_hash_of_its_tests(self):
        (self.run_dir.results / "junit.xml").write_text(junit(self.planned[3]), encoding="utf-8")
        code, _ = self.reports()
        self.assertEqual(code, 1)
        self.assertEqual(self.output.read_text(encoding="utf-8"), "missing=1 2 4\n")

    def test_a_sole_report_whose_tests_hash_to_several_partitions_is_refused(self):
        (self.run_dir.results / "junit.xml").write_text(junit(self.planned[1] + self.planned[2]), encoding="utf-8")
        with self.assertRaises(SystemExit) as refused:
            self.reports()
        self.assertIn("cannot tell which partition wrote", str(refused.exception.code))

    def test_no_report_names_every_partition(self):
        code, _ = self.reports()
        self.assertEqual(code, 1)
        self.assertEqual(self.output.read_text(encoding="utf-8"), "missing=1 2 3 4\n")


class CoverageShardTests(unittest.TestCase):
    """The coverage shards write each shard's own list beside its report."""

    def setUp(self):
        self.run_dir = RunDirectory(self)
        self.planned = by_partition(TESTS, 2)

    def shard(self, number):
        folder = self.run_dir.results / f"coverage-shard-{number}"
        folder.mkdir()
        (folder / "junit.xml").write_text(junit(self.planned[number]), encoding="utf-8")
        (folder / "tests.json").write_text(listing(TESTS), encoding="utf-8")

    def check(self):
        lists = [str(self.run_dir.results / f"coverage-shard-{n}" / "tests.json") for n in (1, 2)]
        return self.run_dir.run("check", "--count", "2", "--prefix", "coverage-shard",
                                "--margin-minutes", "24", "--setup-minutes", "6", "--list", *lists,
                                "--results", str(self.run_dir.results))

    def test_both_shards_pass(self):
        self.shard(1)
        self.shard(2)
        code, out = self.check()
        self.assertEqual(code, 0, out)

    def test_a_shard_past_the_margin_with_its_setup_fails(self):
        # A coverage shard takes about 6 minutes to build and set up, so 18.5
        # test minutes passes the 24 minute margin (TSK-1063).
        self.shard(1)
        folder = self.run_dir.results / "coverage-shard-2"
        folder.mkdir()
        (folder / "junit.xml").write_text(junit(self.planned[2], 18.5 * 60), encoding="utf-8")
        (folder / "tests.json").write_text(listing(TESTS), encoding="utf-8")
        code, out = self.check()
        self.assertEqual(code, 1, out)
        self.assertIn("partition 2 took 18.5 test minutes, about 24.5 job minutes with 6 of setup", out)

    def test_a_sole_surviving_shard_is_numbered_and_the_other_named(self):
        (self.run_dir.results / "junit.xml").write_text(junit(self.planned[2]), encoding="utf-8")
        (self.run_dir.results / "tests.json").write_text(listing(TESTS), encoding="utf-8")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("no JUnit report from partition(s) 1", out)
        self.assertNotIn("ran in no partition", out)

    def test_no_surviving_list_is_refused_with_the_cause(self):
        with self.assertRaises(SystemExit) as refused:
            self.check()
        self.assertIn("none of the test lists exists", str(refused.exception.code))


if __name__ == "__main__":
    unittest.main(verbosity=1)

#!/usr/bin/env python3
"""Controls for scripts/windows-partitions.py (TSK-265 AC-3).

The plan must give every test exactly one partition, the filtersets must say
the same, and `check` must fail a run where a listed test ran in no
partition, in two, or outside its planned partition.
"""

import importlib.util
import json
import re
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("windows_partitions", ROOT / "scripts" / "windows-partitions.py")
wp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wp)

DATA = {
    "partitions": 3,
    "threads": 2,
    "binaries": {"cli::fast": [10.0, 0.0], "cli::slow": [100.0, 0.0], "core": [40.0, 1.0],
                 "cli::present_cli": [30.0, 30.0]},
    "split": {"cli::remedy": {"rest": [20.0, 0.0], "tests": {"a::one": 90.0, "two": 60.0, "three": 50.0}}},
}


def evaluate(expression: str, binary: str, test: str) -> bool:
    """A small evaluator for the filterset forms the plan writes, so the
    tests can check the filtersets without nextest."""
    python = expression
    python = re.sub(r"binary_id\(=([^)]+)\)", lambda m: repr(m.group(1) == binary), python)
    python = re.sub(r"test\(=([^)]+)\)", lambda m: repr(m.group(1) == test), python)
    python = python.replace("&", " and ").replace("|", " or ")
    return eval(python, {"__builtins__": {}})  # noqa: S307 - only True, False, and, or, not, ()


def junit(tests, seconds=60.0):
    suites = {}
    for binary, name in tests:
        suites.setdefault(binary, []).append(name)
    body = "".join(f'<testsuite name="{binary}">' + "".join(f'<testcase name="{n}" time="1.0"/>' for n in names)
                   + "</testsuite>" for binary, names in suites.items())
    return f'<testsuites time="{seconds}">{body}</testsuites>'


class PlanTests(unittest.TestCase):
    def setUp(self):
        self.plan = wp.Plan(DATA)
        self.tests = [("cli::fast", "x"), ("cli::slow", "y"), ("core", "z"), ("cli::present_cli", "p"),
                      ("cli::remedy", "a::one"), ("cli::remedy", "two"), ("cli::remedy", "three"),
                      ("cli::remedy", "new_test"), ("cli::brand_new", "anything")]

    def test_every_test_is_in_exactly_one_filterset_and_it_is_the_planned_one(self):
        expressions = {p: self.plan.filterset(p) for p in range(1, self.plan.count + 1)}
        for binary, test in self.tests:
            with self.subTest(binary=binary, test=test):
                matched = [p for p, expression in expressions.items() if evaluate(expression, binary, test)]
                self.assertEqual(matched, [self.plan.partition_of(binary, test)])

    def test_the_largest_units_spread_across_partitions(self):
        slow = self.plan.partition_of("cli::slow", "y")
        self.assertNotEqual(slow, self.plan.partition_of("cli::remedy", "a::one"))
        self.assertNotEqual(self.plan.partition_of("cli::remedy", "a::one"),
                            self.plan.partition_of("cli::remedy", "two"))

    def test_the_real_data_is_balanced_and_every_filterset_is_short_enough(self):
        plan = wp.Plan(wp.load())
        mean = sum(plan.estimates) / plan.count
        self.assertLessEqual(max(plan.estimates), mean * 1.10)
        for partition in range(1, plan.count + 1):
            # Windows limits a whole command line to 32,767 characters;
            # this leaves room for the rest of the command and for growth.
            self.assertLess(len(plan.filterset(partition)), 20000)

    def test_a_partition_outside_the_count_is_refused(self):
        for partition in (0, self.plan.count + 1):
            with self.assertRaises(SystemExit):
                self.plan.filterset(partition)

    def test_a_name_a_filterset_cannot_hold_is_refused(self):
        for data in [{**DATA, "binaries": {"bad name)": [1.0, 0.0]}},
                     {**DATA, "split": {"cli::remedy": {"rest": [1.0, 0.0], "tests": {"a,b": 1.0}}}},
                     {**DATA, "split": {"core": {"rest": [1.0, 0.0], "tests": {}}}}]:
            with tempfile.TemporaryDirectory() as directory, self.subTest(data=data):
                path = Path(directory) / "times.json"
                path.write_text(json.dumps(data))
                with self.assertRaises(SystemExit):
                    wp.load(path)


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.plan = wp.Plan(DATA)
        self.listed = {("cli::fast", "x"), ("cli::slow", "y"), ("cli::remedy", "two"), ("cli::remedy", "new_test")}

    def problems(self, layout):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(StringIO()):
            for partition, tests in layout.items():
                report = Path(directory) / f"windows-junit-{partition}" / "junit.xml"
                report.parent.mkdir()
                report.write_text(junit(tests))
            return wp.check(self.plan, self.listed, wp.junit_reports(Path(directory)))

    def planned(self):
        layout = {p: [] for p in range(1, self.plan.count + 1)}
        for test in sorted(self.listed):
            layout[self.plan.partition_of(*test)].append(test)
        return layout

    def test_a_run_as_planned_passes(self):
        self.assertEqual(self.problems(self.planned()), [])

    def test_a_test_in_no_partition_fails(self):
        layout = self.planned()
        layout[self.plan.partition_of("cli::fast", "x")].remove(("cli::fast", "x"))
        self.assertIn("ran in no partition", "\n".join(self.problems(layout)))

    def test_a_test_in_two_partitions_fails(self):
        layout = self.planned()
        other = next(p for p in layout if p != self.plan.partition_of("cli::slow", "y"))
        layout[other].append(("cli::slow", "y"))
        self.assertIn("ran in partitions", "\n".join(self.problems(layout)))

    def test_a_test_outside_its_planned_partition_fails(self):
        layout = self.planned()
        home = self.plan.partition_of("cli::remedy", "two")
        layout[home].remove(("cli::remedy", "two"))
        layout[home % self.plan.count + 1].append(("cli::remedy", "two"))
        self.assertIn("the plan gives it", "\n".join(self.problems(layout)))

    def test_an_unlisted_test_or_a_missing_report_fails(self):
        layout = self.planned()
        layout[1].append(("cli::fast", "not_listed"))
        self.assertIn("not in the archive's list", "\n".join(self.problems(layout)))
        layout = self.planned()
        del layout[2]
        self.assertIn("no JUnit report from partition(s) [2]", "\n".join(self.problems(layout)))

    def test_the_list_reader_skips_ignored_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "list.json"
            path.write_text(json.dumps({"rust-suites": {"cli::read_benchmark": {"testcases": {
                "bench": {"ignored": True}, "unit": {"ignored": False}}}}}))
            self.assertEqual(wp.listed_tests(path), {("cli::read_benchmark", "unit")})


class MeasureTests(unittest.TestCase):
    def test_medians_whole_binaries_and_split_large_ones(self):
        with tempfile.TemporaryDirectory() as directory:
            runs = []
            for run, scale in (("old", 1.0), ("new", 3.0)):
                report = Path(directory) / run / "windows-junit-1" / "junit.xml"
                report.parent.mkdir(parents=True)
                cases = "".join(f'<testcase name="t{i}" time="{10 * scale}"/>' for i in range(10))
                gone = '<testcase name="removed" time="5"/>' if run == "old" else ""
                report.write_text(f'<testsuites><testsuite name="big">{cases}{gone}</testsuite>'
                                  f'<testsuite name="small"><testcase name="s" time="{scale}"/></testsuite></testsuites>')
                runs.append(report.parents[1])
            data = wp.measure(runs, partitions=4, threads=2)
        self.assertEqual(data["binaries"], {"small": [2.0, 0.0]})
        self.assertIn("big", data["split"])
        split = data["split"]["big"]
        self.assertNotIn("removed", split["tests"])
        self.assertAlmostEqual(sum(split["tests"].values()) + split["rest"][0], 200.0)


if __name__ == "__main__":
    unittest.main()

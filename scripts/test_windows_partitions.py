#!/usr/bin/env python3
"""Controls for scripts/windows-partitions.py (TSK-265 AC-3, TSK-1063).

The plan must give every test exactly one partition, the filtersets must say
the same, and `check` must fail a run where a listed test ran in no
partition, in two, or outside its planned partition. Binaries the data does
not know must spread over the partitions, `drift` must fail when the data
stops covering the suite, `check` must fail a partition past the time
margin, and a partition without a report must be named, not read as failing
tests (TSK-1063).
"""

import importlib.util
import json
import os
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch
from io import StringIO
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("windows_partitions", ROOT / "scripts" / "windows-partitions.py")
wp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wp)

DATA = {
    "partitions": 3,
    "threads": 2,
    "default_seconds": 2.0,
    "names": {"cli::fast": ["x"], "cli::slow": ["y"], "core": ["z"], "cli::present_cli": ["p"],
              "cli::remedy": ["a::one", "two", "three", "rest_test"]},
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
        self.assertIn("not in the list of tests", "\n".join(self.problems(layout)))
        layout = self.planned()
        del layout[2]
        self.assertIn("no JUnit report from partition(s) [2]", "\n".join(self.problems(layout)))

    def test_the_list_reader_skips_ignored_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "list.json"
            path.write_text(json.dumps({"rust-suites": {"cli::read_benchmark": {"testcases": {
                "bench": {"ignored": True}, "unit": {"ignored": False}}}}}))
            self.assertEqual(wp.listed_tests(path), {("cli::read_benchmark", "unit")})


def many_new_tests(binaries=6, tests=10):
    """Tests of binaries the data does not know, as a nextest list would name them."""
    return {(f"cli::new_{b}", f"t{t}") for b in range(binaries) for t in range(tests)}


class UnmeasuredTests(unittest.TestCase):
    """TSK-1063 AC-2: binaries the data does not know are placed by a rule
    that spreads them, from the build's test list."""

    def setUp(self):
        self.listed = {("cli::fast", "x"), ("cli::slow", "y"), ("core", "z"), ("cli::present_cli", "p"),
                       ("cli::remedy", "two"), ("cli::remedy", "new_test")} | many_new_tests()

    def test_without_the_list_every_unmeasured_binary_runs_in_one_partition(self):
        plan = wp.Plan(DATA)
        self.assertEqual(len({plan.partition_of(b, t) for b, t in many_new_tests()}), 1)

    def test_with_the_list_unmeasured_binaries_spread_over_the_partitions(self):
        plan = wp.Plan(wp.with_unmeasured(DATA, self.listed))
        partitions = {plan.partition_of(b, t) for b, t in many_new_tests()}
        self.assertGreater(len(partitions), 1)
        loads = [plan.estimates[p - 1] for p in range(1, plan.count + 1)]
        self.assertLess(max(loads) - min(loads), max(loads) * 0.75)

    def test_every_listed_test_is_in_exactly_one_filterset_and_it_is_the_planned_one(self):
        plan = wp.Plan(wp.with_unmeasured(DATA, self.listed))
        expressions = {p: plan.filterset(p) for p in range(1, plan.count + 1)}
        for binary, test in sorted(self.listed):
            with self.subTest(binary=binary, test=test):
                matched = [p for p, expression in expressions.items() if evaluate(expression, binary, test)]
                self.assertEqual(matched, [plan.partition_of(binary, test)])

    def test_the_placement_is_the_same_on_every_run_of_the_same_list(self):
        first = wp.Plan(wp.with_unmeasured(DATA, self.listed))
        second = wp.Plan(wp.with_unmeasured(DATA, set(reversed(sorted(self.listed)))))
        for binary, test in self.listed:
            self.assertEqual(first.partition_of(binary, test), second.partition_of(binary, test))

    def test_a_listed_binary_id_a_filterset_cannot_hold_is_refused(self):
        with self.assertRaises(SystemExit):
            wp.with_unmeasured(DATA, {("bad name)", "t")})

    def assert_each_test_runs_once_where_planned(self, plan, listed):
        expressions = {p: plan.filterset(p) for p in range(1, plan.count + 1)}
        for binary, test in sorted(listed):
            matched = [p for p, expression in expressions.items() if evaluate(expression, binary, test)]
            self.assertEqual(matched, [plan.partition_of(binary, test)], (binary, test))

    def measured(self):
        return {("cli::fast", "x"), ("cli::slow", "y"), ("core", "z"), ("cli::present_cli", "p"),
                ("cli::remedy", "a::one"), ("cli::remedy", "two"), ("cli::remedy", "three"),
                ("cli::remedy", "rest_test")}

    def test_new_tests_in_one_whole_binary_spread_and_add_to_the_estimate(self):
        new = {("cli::slow", f"added_{i}") for i in range(12)}
        listed = self.measured() | new
        before = wp.Plan(DATA)
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        self.assertEqual(len({plan.partition_of(*test) for test in new}), plan.count)
        self.assertGreater(sum(plan.seconds), sum(before.seconds))
        self.assertAlmostEqual(sum(plan.seconds) - sum(before.seconds), 12 * DATA["default_seconds"])
        # The measured test stays with its binary's rest.
        self.assertEqual(plan.partition_of("cli::slow", "y"), before.partition_of("cli::slow", "y"))
        self.assert_each_test_runs_once_where_planned(plan, listed)

    def test_new_tests_in_a_split_binary_spread_and_add_to_the_estimate(self):
        new = {("cli::remedy", f"added_{i}") for i in range(12)}
        listed = self.measured() | new
        before = wp.Plan(DATA)
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        self.assertEqual(len({plan.partition_of(*test) for test in new}), plan.count)
        self.assertAlmostEqual(sum(plan.seconds) - sum(before.seconds), 12 * DATA["default_seconds"])
        self.assertEqual(plan.partition_of("cli::remedy", "rest_test"), before.partition_of("cli::remedy", "rest_test"))
        self.assert_each_test_runs_once_where_planned(plan, listed)

    def test_a_large_new_binary_is_split_by_test_over_the_partitions(self):
        # 300 tests at 2 seconds are 600 seconds, past a partition's share of the 330 seconds measured.
        new = {("cli::huge", f"t{i}") for i in range(300)}
        listed = self.measured() | new
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        self.assertEqual(len({plan.partition_of(*test) for test in new}), plan.count)
        loads = [sum(1 for test in new if plan.partition_of(*test) == p) for p in range(1, plan.count + 1)]
        self.assertLess(max(loads) - min(loads), 0.3 * max(loads))
        self.assert_each_test_runs_once_where_planned(plan, listed)
        # Each partition lists only its own tests of the binary, never the others'.
        for partition in range(1, plan.count + 1):
            self.assertNotIn("not (test(=t", plan.filterset(partition))

    def test_a_small_new_binary_spreads_too(self):
        new = {("cli::tiny", f"t{i}") for i in range(6)}
        listed = self.measured() | new
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        self.assertGreater(len({plan.partition_of(*test) for test in new}), 1)
        self.assert_each_test_runs_once_where_planned(plan, listed)

    def test_a_small_new_binary_under_every_drift_trigger_still_spreads(self):
        """59 new tests in one binary on the committed measurements: 1.1 guessed
        minutes if they gather in one partition, which passes `drift`. They must
        spread over the partitions all the same, and each run in one filterset."""
        data = wp.load()
        measured = {(binary, name) for binary, names in data["names"].items() for name in names}
        new = {("codeflow-cli::brand_new_suite", f"t{i}") for i in range(59)}
        listed = measured | new
        with redirect_stdout(StringIO()):
            self.assertEqual(wp.drift(data, listed), [])  # under both triggers
        plan = wp.Plan(wp.with_unmeasured(data, listed))
        placed = [plan.partition_of(*test) for test in sorted(new)]
        self.assertGreater(len(set(placed)), 6, "the new tests must spread over the partitions")
        self.assertLessEqual(max(placed.count(p) for p in set(placed)), 10)
        before = wp.Plan(data)
        self.assertAlmostEqual(sum(plan.seconds) - sum(before.seconds), 59 * data["default_seconds"], places=2)
        expressions = {p: plan.filterset(p) for p in range(1, plan.count + 1)}
        for binary, test in sorted(new):
            matched = [p for p, expression in expressions.items() if evaluate(expression, binary, test)]
            self.assertEqual(matched, [plan.partition_of(binary, test)])

    def test_a_replaced_test_is_unmeasured_and_the_old_name_costs_nothing(self):
        listed = (self.measured() - {("cli::slow", "y")}) | {("cli::slow", "y_renamed")}
        self.assertEqual(wp.unmeasured_tests(DATA, listed), {("cli::slow", "y_renamed")})
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        self.assert_each_test_runs_once_where_planned(plan, listed)

    def test_a_listed_test_name_a_filterset_cannot_hold_is_refused(self):
        with self.assertRaises(SystemExit):
            wp.with_unmeasured(DATA, self.measured() | {("cli::slow", "a,b")})

    def test_a_filterset_past_the_limit_is_refused_with_how_to_refresh(self):
        listed = self.measured() | {("cli::huge", f"t{i}") for i in range(300)}
        plan = wp.Plan(wp.with_unmeasured(DATA, listed))
        with patch.object(wp, "FILTERSET_LIMIT", 200), self.assertRaises(SystemExit) as refused:
            for partition in range(1, plan.count + 1):
                plan.filterset(partition)
        self.assertIn("characters, past 200", str(refused.exception))
        self.assertIn("32767", str(refused.exception))
        self.assertIn("scripts/windows-partitions.py measure", str(refused.exception))

    def test_the_limit_leaves_room_for_the_rest_of_the_command(self):
        self.assertLessEqual(wp.FILTERSET_LIMIT + 600, wp.COMMAND_LIMIT)
        self.assertEqual(wp.COMMAND_LIMIT, 32767)


class DriftTests(unittest.TestCase):
    """TSK-1063 AC-3: the data must cover the suite and the plan must stay
    inside the margin, before any partition runs."""

    def listing(self, extra=0):
        # DATA measured 8 tests; `extra` more are new tests in new binaries.
        listed = {("cli::fast", "x"), ("cli::slow", "y"), ("core", "z"), ("cli::present_cli", "p"),
                  ("cli::remedy", "a::one"), ("cli::remedy", "two"), ("cli::remedy", "three"),
                  ("cli::remedy", "rest_test")}
        return listed | {("cli::drifted", f"t{i}") for i in range(extra)}

    def run_drift(self, data, listed):
        with redirect_stdout(StringIO()) as out:
            problems = wp.drift(data, listed, plan_minutes=60)
        return problems, out.getvalue()

    def test_data_that_covers_the_suite_passes(self):
        problems, out = self.run_drift(DATA, self.listing())
        self.assertEqual(problems, [])
        self.assertIn("0 of 8 listed tests are outside the measurements", out)

    def test_unmeasured_tests_beyond_the_share_fail_and_say_how_to_refresh(self):
        problems, _ = self.run_drift(DATA, self.listing(extra=1))
        self.assertEqual(len(problems), 1)
        self.assertIn("1 of 9 listed tests (11.1 percent) are outside the measurements", problems[0])
        self.assertIn("scripts/windows-partitions.py measure", problems[0])

    def test_unmeasured_tests_within_the_share_pass(self):
        measured = [f"s{i}" for i in range(30)]
        data = {**DATA, "names": {**DATA["names"], "cli::slow": ["y", *measured]}}
        listed = self.listing() | {("cli::slow", name) for name in measured} | {("cli::slow", "brand_new")}
        problems, out = self.run_drift(data, listed)  # 1 of 39 listed
        self.assertEqual(problems, [])
        self.assertIn("1 of 39 listed tests are outside", out)

    def test_a_new_test_in_a_known_binary_counts_as_unmeasured(self):
        listed = self.listing() | {("core", "brand_new")}
        problems, _ = self.run_drift(DATA, listed)
        self.assertTrue(any("outside the measurements" in problem for problem in problems))

    def test_a_replaced_test_counts_as_unmeasured_though_the_binary_is_the_same_size(self):
        listed = (self.listing() - {("cli::remedy", "rest_test")}) | {("cli::remedy", "renamed_test")}
        self.assertEqual(len(listed), len(self.listing()))
        self.assertEqual(wp.unmeasured_tests(DATA, listed), {("cli::remedy", "renamed_test")})
        problems, _ = self.run_drift(DATA, listed)
        self.assertTrue(any("1 of 8 listed tests (12.5 percent)" in problem for problem in problems))

    def test_guessed_minutes_in_one_partition_fail_though_the_share_is_small(self):
        """A few expensive-looking additions dominate a partition while the
        count of unmeasured tests stays small."""
        measured = [f"s{i}" for i in range(60)]
        data = {**DATA, "default_seconds": 200.0, "names": {**DATA["names"], "cli::slow": ["y", *measured]}}
        listed = self.listing() | {("cli::slow", name) for name in measured} | {("cli::slow", "brand_new")}
        problems, out = self.run_drift(data, listed)  # 1 of 69 listed, 100 seconds on 2 threads
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("holds 1.7 planned minutes of tests the data has not measured", problems[0])
        self.assertIn("scripts/windows-partitions.py measure", problems[0])
        self.assertIn("guessed test minutes per partition", out)

    def test_a_plan_past_the_planned_minutes_fails(self):
        with redirect_stdout(StringIO()):
            problems = wp.drift(DATA, self.listing(), plan_minutes=1)
        self.assertEqual(len(problems), 1)
        self.assertIn("the plan puts a partition at", problems[0])
        self.assertIn("refresh the data", problems[0])

    def test_a_filterset_past_the_command_line_limit_fails_before_any_partition_runs(self):
        with patch.object(wp, "FILTERSET_LIMIT", 120):
            problems, _ = self.run_drift(DATA, self.listing())
        self.assertTrue(any("filterset of partition" in problem and "characters, past 120" in problem
                            for problem in problems), problems)

    def test_data_without_names_cannot_prove_coverage(self):
        data = {key: value for key, value in DATA.items() if key != "names"}
        problems, _ = self.run_drift(data, self.listing())
        self.assertIn("records no measured test names", problems[0])

    def test_the_real_data_covers_the_listed_suite_and_the_plan_is_inside_the_margin(self):
        data = wp.load()
        listed = {(binary, name) for binary, names in data["names"].items() for name in names}
        with redirect_stdout(StringIO()):
            self.assertEqual(wp.drift(data, listed), [])
        plan = wp.Plan(data)
        self.assertLessEqual(max(plan.estimates) / 60, wp.PLAN_MINUTES)
        # Planned time plus setup, at the slowest runner factor read in TSK-1063's findings.
        self.assertLessEqual(max(plan.estimates) / 60 * 1.7 + wp.SETUP_MINUTES, wp.MARGIN_MINUTES)


class MarginTests(unittest.TestCase):
    """TSK-1063 AC-3 and AC-6: a run whose partition would pass the margin fails
    `check` while it still finishes."""

    def setUp(self):
        self.plan = wp.Plan(DATA)
        self.listed = {("cli::fast", "x"), ("cli::slow", "y")}

    def problems(self, seconds_by_partition):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(StringIO()) as out:
            for partition in range(1, self.plan.count + 1):
                tests = [t for t in sorted(self.listed) if self.plan.partition_of(*t) == partition]
                report = Path(directory) / f"windows-junit-{partition}" / "junit.xml"
                report.parent.mkdir()
                report.write_text(junit(tests, seconds_by_partition.get(partition, 60.0)))
            return wp.check(self.plan, self.listed, wp.junit_reports(Path(directory))), out.getvalue()

    def test_partitions_inside_the_margin_pass(self):
        problems, out = self.problems({1: 20 * 60})  # 20 test minutes + 2 setup = 22 < 24
        self.assertEqual(problems, [])
        self.assertIn("measured of planned", out)

    def test_a_partition_past_the_margin_fails_and_says_how_to_refresh(self):
        problems, _ = self.problems({2: 23 * 60})  # 23 + 2 setup = 25 > 24
        self.assertEqual(len(problems), 1)
        self.assertIn("partition 2 took 23.0 test minutes, about 25.0 job minutes", problems[0])
        self.assertIn("past the 24 minute margin of the 30 minute job limit", problems[0])
        self.assertIn("refresh it with `scripts/windows-partitions.py measure`", problems[0])

    def test_the_margin_leaves_a_fifth_of_the_job_limit(self):
        self.assertLessEqual(wp.MARGIN_MINUTES, wp.JOB_LIMIT_MINUTES * 0.8)


class MissingReportTests(unittest.TestCase):
    """TSK-1063 AC-7: a partition that ended without its report (a timeout or
    a cancellation) is named as not finished, never read as failing tests."""

    def setUp(self):
        self.plan = wp.Plan(DATA)
        self.listed = {("cli::fast", "x"), ("cli::slow", "y"), ("cli::remedy", "two"), ("core", "z")}

    def layout(self, directory, skip):
        for partition in range(1, self.plan.count + 1):
            if partition == skip:
                continue
            tests = [t for t in sorted(self.listed) if self.plan.partition_of(*t) == partition]
            report = Path(directory) / f"windows-junit-{partition}" / "junit.xml"
            report.parent.mkdir()
            report.write_text(junit(tests))

    def test_check_names_the_partition_and_does_not_list_its_tests_as_failures(self):
        skip = self.plan.partition_of("cli::slow", "y")
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(StringIO()):
            self.layout(directory, skip)
            problems = wp.check(self.plan, self.listed, wp.junit_reports(Path(directory)))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn(f"no JUnit report from partition(s) [{skip}]", problems[0])
        self.assertIn("a timeout or a cancellation", problems[0])
        self.assertIn("none of them is known to have failed", problems[0])
        self.assertNotIn("ran in no partition", "\n".join(problems))

    def test_the_reports_command_names_the_missing_partitions_and_writes_the_output(self):
        with tempfile.TemporaryDirectory() as directory:
            self.layout(directory, 2)
            output = Path(directory) / "github-output"
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            with patch.object(sys, "argv", ["windows-partitions.py", "reports", "--results", directory]), \
                    patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), redirect_stdout(StringIO()) as out:
                code = wp.main(data_path=data)
            self.assertEqual(code, 1)
            self.assertIn("partition(s) 2 ended without a JUnit report", out.getvalue())
            self.assertIn("none is known to have failed", out.getvalue())
            self.assertEqual(output.read_text(), "missing=2\n")

    def test_the_reports_command_passes_when_every_report_exists(self):
        with tempfile.TemporaryDirectory() as directory:
            self.layout(directory, 0)
            output = Path(directory) / "github-output"
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            with patch.object(sys, "argv", ["windows-partitions.py", "reports", "--results", directory]), \
                    patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), redirect_stdout(StringIO()):
                self.assertEqual(wp.main(data_path=data), 0)
            self.assertEqual(output.read_text(), "missing=\n")

    def sole_report(self, directory, partition, tests=None):
        """What download-artifact leaves when one artifact survives: the report
        in the results directory itself, with no numbered directory."""
        tests = tests if tests is not None else [t for t in sorted(self.listed) if self.plan.partition_of(*t) == partition]
        report = Path(directory) / "junit.xml"
        report.write_text(junit(tests))
        return report

    def write_list(self, directory):
        suites = {}
        for binary, name in sorted(self.listed):
            suites.setdefault(binary, {"testcases": {}})["testcases"][name] = {"ignored": False}
        path = Path(directory) / "list.json"
        path.write_text(json.dumps({"rust-suites": suites}))
        return path

    def test_a_sole_surviving_report_is_numbered_by_the_tests_the_plan_gives_it(self):
        survivor = self.plan.partition_of("cli::slow", "y")
        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "results"
            results.mkdir()
            self.sole_report(results, survivor)
            reports = wp.junit_reports(results, identify=wp.identifier(self.plan, self.listed))
        self.assertEqual(list(reports), [survivor])

    def test_the_reports_command_names_every_other_partition_when_one_report_survives(self):
        survivor = self.plan.partition_of("cli::slow", "y")
        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "results"
            results.mkdir()
            self.sole_report(results, survivor)
            output = Path(directory) / "github-output"
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            argv = ["windows-partitions.py", "reports", "--results", str(results), "--list", str(self.write_list(directory))]
            with patch.object(sys, "argv", argv), patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), \
                    redirect_stdout(StringIO()) as out:
                code = wp.main(data_path=data)
            others = " ".join(str(p) for p in range(1, self.plan.count + 1) if p != survivor)
            self.assertEqual(code, 1)
            self.assertEqual(output.read_text(), f"missing={others}\n")
            self.assertIn(f"partition(s) {others} ended without a JUnit report", out.getvalue())

    def test_the_reports_command_names_every_partition_when_no_report_survives(self):
        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "never-created"
            output = Path(directory) / "github-output"
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            argv = ["windows-partitions.py", "reports", "--results", str(results), "--list", str(self.write_list(directory))]
            with patch.object(sys, "argv", argv), patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), \
                    redirect_stdout(StringIO()):
                self.assertEqual(wp.main(data_path=data), 1)
            self.assertEqual(output.read_text(), "missing=" + " ".join(map(str, range(1, self.plan.count + 1))) + "\n")

    def test_check_with_one_surviving_report_names_the_missing_partitions_only(self):
        survivor = self.plan.partition_of("cli::slow", "y")
        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "results"
            results.mkdir()
            self.sole_report(results, survivor)
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            argv = ["windows-partitions.py", "check", "--results", str(results), "--list", str(self.write_list(directory))]
            with patch.object(sys, "argv", argv), redirect_stdout(StringIO()) as out:
                code = wp.main(data_path=data)
        others = [p for p in range(1, self.plan.count + 1) if p != survivor]
        self.assertEqual(code, 1)
        self.assertIn(f"no JUnit report from partition(s) {others}", out.getvalue())
        self.assertNotIn("ran in no partition", out.getvalue())

    def test_a_sole_report_whose_tests_the_plan_cannot_place_is_refused(self):
        slow, fast = ("cli::slow", "y"), ("cli::remedy", "two")
        self.assertNotEqual(self.plan.partition_of(*slow), self.plan.partition_of(*fast))
        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "results"
            results.mkdir()
            self.sole_report(results, 0, tests=[slow, fast])  # one test of each of two partitions: a tie
            with self.assertRaises(SystemExit) as refused:
                wp.junit_reports(results, identify=wp.identifier(self.plan, self.listed))
            self.assertIn("cannot tell which partition wrote", str(refused.exception))

    def test_a_report_outside_a_numbered_directory_without_the_list_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            self.sole_report(directory, 1)
            with self.assertRaises(SystemExit) as refused:
                wp.junit_reports(Path(directory))
            self.assertIn("--list", str(refused.exception))

    def test_two_reports_for_one_partition_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            for name in ("a", "b"):
                report = Path(directory) / "windows-junit-3" / f"{name}.xml"
                report.parent.mkdir(exist_ok=True)
                report.write_text(junit([]))
            with self.assertRaises(SystemExit):
                wp.junit_reports(Path(directory))


def workflow_jobs(workflow):
    return dict(zip(*[iter(re.split(r"^  ([A-Za-z0-9_-]+):\n", workflow.split("\njobs:\n", 1)[1], flags=re.M)[1:])] * 2))


def workflow_steps(job):
    """The steps of a job as dicts of their top-level keys, for the fixed
    shape of this repository's workflow (a key at eight spaces, a step at six)."""
    steps = []
    for line in job.split("    steps:\n", 1)[1].splitlines():
        if re.match(r"^      - ", line):
            steps.append({"text": ""})
            line = "        " + line[8:]
        elif not steps:
            continue
        steps[-1]["text"] += line + "\n"
        key = re.match(r"^        ([A-Za-z][A-Za-z0-9_-]*):\s*(.*)$", line)
        if key:
            steps[-1][key.group(1)] = key.group(2)
    return steps


class WorkflowStructureTests(unittest.TestCase):
    """TSK-1063 AC-7: deleting the reports step, or letting a later step run
    without it, must fail here, not only change a string."""

    @classmethod
    def setUpClass(cls):
        cls.jobs = workflow_jobs((ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text(encoding="utf-8"))

    def test_the_journeys_job_has_one_reports_step_that_always_gates_what_follows(self):
        steps = workflow_steps(self.jobs["windows-journeys"])
        reports = [s for s in steps if s.get("id") == "reports"]
        self.assertEqual(len(reports), 1, "the reports step must exist once")
        step = reports[0]
        self.assertEqual(step["run"], "python -B scripts/windows-partitions.py reports --results "
                         "target/nextest/partitions --list target/nextest/list/windows-tests.json")
        for forbidden in ("if", "continue-on-error"):
            self.assertNotIn(forbidden, step, f"the reports step must not carry `{forbidden}`")
        for other in steps:
            self.assertNotIn("continue-on-error", other, "no journeys step may forgive its failure")
        names = [s.get("name") for s in steps]
        at = steps.index(step)
        self.assertLess(at, names.index("journey gate"))
        self.assertLess(at, names.index("Every test ran in exactly one partition"))
        self.assertNotIn("if", steps[names.index("journey gate")], "the journey gate must run only after the reports step passed")
        placement = steps[names.index("Every test ran in exactly one partition")]
        # The whole condition, so `... || true` or a dropped `!cancelled()` fails here.
        self.assertEqual(placement["if"], "${{ !cancelled() && steps.reports.outcome == 'success' }}")
        self.assertIn("windows-partitions.py check", placement["text"])

    def test_the_journeys_job_hands_the_missing_partitions_to_the_verdict(self):
        job = self.jobs["windows-journeys"]
        self.assertRegex(job, r"outputs:\n\s+missing: \$\{\{ steps\.reports\.outputs\.missing \}\}\n")
        verdict = self.jobs["windows-verdict"]
        self.assertRegex(verdict, r"needs: \[[^\]]*windows-journeys[^\]]*\]")
        self.assertIn("MISSING: ${{ needs.windows-journeys.outputs.missing }}", verdict)
        self.assertIn('[ "$JOURNEYS" != success ]', verdict)
        self.assertIn("exit 1", verdict.split('[ "$JOURNEYS" != success ]', 1)[1])

    def test_the_partitions_run_their_filterset_from_the_builds_list(self):
        steps = workflow_steps(self.jobs["windows-tests"])
        run = [s for s in steps if s.get("name", "").startswith("cargo nextest run")]
        self.assertEqual(len(run), 1)
        self.assertIn('filter ${{ matrix.partition }} --list "$RUNNER_TEMP/windows-test-list/windows-tests.json"', run[0]["run"])


CS_SPEC = importlib.util.spec_from_file_location("coverage_shards", ROOT / "scripts" / "coverage-shards.py")
cs = importlib.util.module_from_spec(CS_SPEC)
CS_SPEC.loader.exec_module(cs)


class CoverageShardTests(unittest.TestCase):
    """TSK-270 AC-2: the Linux coverage shards use this planner with their
    own measurements, results directories and per-shard test lists."""

    def test_the_real_coverage_data_is_balanced_over_two_shards(self):
        plan = wp.Plan(wp.load(cs.DATA))
        self.assertEqual(plan.count, 2)
        mean = sum(plan.estimates) / plan.count
        self.assertLessEqual(max(plan.estimates), mean * 1.05)

    def test_the_real_coverage_data_is_inside_the_margin_and_covers_its_names(self):
        data = wp.load(cs.DATA)
        plan = wp.Plan(data)
        self.assertLessEqual(max(plan.estimates) / 60, cs.PLAN_MINUTES)
        self.assertLessEqual(cs.PLAN_MINUTES + cs.SETUP_MINUTES, wp.MARGIN_MINUTES)
        listed = {(binary, name) for binary, names in data["names"].items() for name in names}
        with redirect_stdout(StringIO()):
            self.assertEqual(wp.drift(data, listed, cs.REFRESH, cs.PLAN_MINUTES, spread=False), [])

    def test_a_shard_filter_ignores_the_list_so_it_matches_what_check_expects(self):
        """The shards run before any list exists, so an unmeasured binary stays
        in the rest shard; a list given to `filter` or `check` must not move it."""
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory) / "times.json"
            data.write_text(json.dumps(DATA))
            listing = self.write_list(Path(directory) / "list.json", sorted(many_new_tests()))
            outputs = []
            for extra in ([], ["--list", str(listing)]):
                with patch.object(sys, "argv", ["coverage-shards.py", "filter", "1", *extra]), \
                        redirect_stdout(StringIO()) as out:
                    self.assertEqual(wp.main(data_path=data, spread=False), 0)
                outputs.append(out.getvalue())
        self.assertEqual(outputs[0], outputs[1])

    def write_list(self, path: Path, tests) -> Path:
        suites = {}
        for binary, name in tests:
            suites.setdefault(binary, {"testcases": {}})["testcases"][name] = {"ignored": False}
        path.write_text(json.dumps({"rust-suites": suites}))
        return path

    def test_check_reads_the_shard_directories_and_every_shards_list(self):
        plan = wp.Plan(DATA)
        listed = [("cli::fast", "x"), ("cli::slow", "y"), ("cli::remedy", "two")]
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(StringIO()):
            root = Path(directory)
            for partition in range(1, plan.count + 1):
                report = root / f"coverage-shard-{partition}" / "junit.xml"
                report.parent.mkdir()
                report.write_text(junit([t for t in listed if plan.partition_of(*t) == partition]))
                self.write_list(report.parent / "tests.json", listed)
            lists = [root / f"coverage-shard-{p}" / "tests.json" for p in range(1, plan.count + 1)]
            self.assertEqual(wp.check(plan, wp.listed_once(lists), wp.junit_reports(root, cs.RESULTS)), [])
            with self.assertRaises(SystemExit):
                wp.junit_reports(root)
            self.write_list(lists[-1], listed[:-1])
            with self.assertRaises(SystemExit):
                wp.listed_once(lists)


class CoverageSoleShardTests(unittest.TestCase):
    """M2 for the Linux sibling: one surviving shard artifact is written into
    the results directory itself, with its list, and the dead shards are named."""

    def setUp(self):
        self.plan = wp.Plan(DATA)
        self.listed = [("cli::fast", "x"), ("cli::slow", "y"), ("cli::remedy", "two")]

    def run_check(self, directory, survivor):
        root = Path(directory) / "results"
        root.mkdir()
        data = Path(directory) / "times.json"
        data.write_text(json.dumps(DATA))
        if survivor:
            (root / "junit.xml").write_text(
                junit([t for t in self.listed if self.plan.partition_of(*t) == survivor]))
            suites = {}
            for binary, name in self.listed:
                suites.setdefault(binary, {"testcases": {}})["testcases"][name] = {"ignored": False}
            (root / "tests.json").write_text(json.dumps({"rust-suites": suites}))
        lists = [str(root / f"coverage-shard-{p}" / "tests.json") for p in range(1, self.plan.count + 1)]
        argv = ["coverage-shards.py", "check", "--list", *lists, "--results", str(root)]
        with patch.object(sys, "argv", argv), redirect_stdout(StringIO()) as out:
            return wp.main(data_path=data, prefix=cs.RESULTS, spread=False), out.getvalue()

    def test_the_dead_shards_are_named_when_one_shard_survives(self):
        survivor = self.plan.partition_of("cli::slow", "y")
        with tempfile.TemporaryDirectory() as directory:
            code, out = self.run_check(directory, survivor)
        others = [p for p in range(1, self.plan.count + 1) if p != survivor]
        self.assertEqual(code, 1)
        self.assertIn(f"no JUnit report from partition(s) {others}", out)
        self.assertNotIn("ran in no partition", out)

    def test_no_surviving_shard_is_refused_with_the_cause(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(SystemExit) as refused:
            self.run_check(directory, None)
        self.assertIn("a timeout or a cancellation", str(refused.exception))


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
        self.assertEqual(data["names"], {"big": sorted(f"t{i}" for i in range(10)), "small": ["s"]})
        self.assertAlmostEqual(data["default_seconds"], (10 * 20.0 + 2.0) / 11, places=2)
        self.assertEqual(data["binaries"], {"small": [2.0, 0.0]})
        self.assertIn("big", data["split"])
        split = data["split"]["big"]
        self.assertNotIn("removed", split["tests"])
        self.assertAlmostEqual(sum(split["tests"].values()) + split["rest"][0], 200.0)


if __name__ == "__main__":
    unittest.main()

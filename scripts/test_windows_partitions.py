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
    "counts": {"cli::fast": 1, "cli::slow": 1, "core": 1, "cli::present_cli": 1, "cli::remedy": 4},
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


class DriftTests(unittest.TestCase):
    """TSK-1063 AC-3: the data must cover the suite and the plan must stay
    inside the margin, before any partition runs."""

    def listing(self, extra=0):
        # DATA measured 8 tests; `extra` more are new tests in new binaries.
        listed = {("cli::fast", "x"), ("cli::slow", "y"), ("core", "z"), ("cli::present_cli", "p"),
                  ("cli::remedy", "a"), ("cli::remedy", "b"), ("cli::remedy", "c"), ("cli::remedy", "d")}
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
        listed = self.listing() | {("core", f"more{i}") for i in range(0)}
        data = {**DATA, "counts": {**DATA["counts"], "core": 1}}
        listed |= {("core", "extra")}  # 1 of 9 would fail; 12 measured tests make it 1 of 12
        data["counts"]["cli::remedy"] = 4
        listed |= {("cli::slow", f"s{i}") for i in range(3)}
        data["counts"]["cli::slow"] = 4
        problems, _ = self.run_drift(data, listed)
        self.assertEqual(problems, [])

    def test_a_new_test_in_a_known_binary_counts_as_unmeasured(self):
        listed = self.listing() | {("core", "brand_new")}
        problems, _ = self.run_drift({**DATA, "counts": {**DATA["counts"], "cli::remedy": 0}}, listed)
        self.assertTrue(any("outside the measurements" in problem for problem in problems))

    def test_a_plan_past_the_planned_minutes_fails(self):
        with redirect_stdout(StringIO()):
            problems = wp.drift(DATA, self.listing(), plan_minutes=1)
        self.assertEqual(len(problems), 1)
        self.assertIn("the plan puts a partition at", problems[0])
        self.assertIn("refresh the data", problems[0])

    def test_data_without_counts_cannot_prove_coverage(self):
        data = {key: value for key, value in DATA.items() if key != "counts"}
        problems, _ = self.run_drift(data, self.listing())
        self.assertIn("records no test counts", problems[0])

    def test_the_real_data_covers_the_listed_suite_and_the_plan_is_inside_the_margin(self):
        data = wp.load()
        counts = data["counts"]
        listed = {(binary, f"t{i}") for binary, n in counts.items() for i in range(n)}
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

    def test_the_workflow_hands_the_missing_partitions_to_the_verdict(self):
        workflow = (ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text(encoding="utf-8")
        self.assertIn("windows-partitions.py reports --results target/nextest/partitions", workflow)
        self.assertIn("missing: ${{ steps.reports.outputs.missing }}", workflow)
        self.assertIn("MISSING: ${{ needs.windows-journeys.outputs.missing }}", workflow)


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

    def test_the_real_coverage_data_is_inside_the_margin_and_covers_its_counts(self):
        data = wp.load(cs.DATA)
        plan = wp.Plan(data)
        self.assertLessEqual(max(plan.estimates) / 60, cs.PLAN_MINUTES)
        self.assertLessEqual(cs.PLAN_MINUTES + cs.SETUP_MINUTES, wp.MARGIN_MINUTES)
        listed = {(binary, f"t{i}") for binary, n in data["counts"].items() for i in range(n)}
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
        self.assertEqual(data["counts"], {"big": 10, "small": 1})
        self.assertAlmostEqual(data["default_seconds"], (10 * 20.0 + 2.0) / 11, places=2)
        self.assertEqual(data["binaries"], {"small": [2.0, 0.0]})
        self.assertIn("big", data["split"])
        split = data["split"]["big"]
        self.assertNotIn("removed", split["tests"])
        self.assertAlmostEqual(sum(split["tests"].values()) + split["rest"][0], 200.0)


if __name__ == "__main__":
    unittest.main()

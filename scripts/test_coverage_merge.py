#!/usr/bin/env python3
"""Controls for scripts/coverage-merge.py (TSK-270 AC-2).

The merged line coverage must be the number llvm-cov gives one run of the
whole suite, and the floor must fail the merge below it. The fixtures in
scripts/fixtures/coverage-merge are real `cargo llvm-cov report --json`
output, cut down to the fields the merge reads, from a two-crate workspace:
crate a defines a generic `twice` and a `sign` with three branches, crate b
calls both. Shard 1 ran crate a's unit tests, shard 2 crate b's tests, and
the single run ran both. Two instantiations of `twice` each cover its one
line, so a sum of lcov line hits counts 11 of 14 lines where llvm-cov counts
10; the merge must give 10.
"""

import copy
import importlib.util
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("coverage_merge", ROOT / "scripts" / "coverage-merge.py")
cm = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cm)

FIXTURES = ROOT / "scripts" / "fixtures" / "coverage-merge"


def fixture(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text())


def synthetic(covered: set[int], lines: int = 1000) -> dict:
    """A report of one file with `lines` one-line functions, each covered
    when its number is in `covered`, with llvm-cov's summary for it."""
    functions = [{"name": f"f{i}", "count": 1 if i in covered else 0, "filenames": ["src/lib.rs"],
                  "regions": [[i + 1, 1, i + 1, 20, 1 if i in covered else 0, 0, 0, 0]]}
                 for i in range(lines)]
    hit = len([i for i in range(lines) if i in covered])
    return {"data": [{"files": [{"filename": "src/lib.rs", "summary": {"lines": {"count": lines, "covered": hit}}}],
                      "functions": functions, "totals": {"lines": {"count": lines, "covered": hit}}}]}


class Shards:
    """A directory laid out as the report job downloads the shards."""

    def __init__(self, *reports):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)
        for number, report in enumerate(reports, 1):
            if report is None:
                continue
            target = self.path / f"coverage-data-{number}" / "coverage.json"
            target.parent.mkdir()
            target.write_text(report if isinstance(report, str) else json.dumps(report))

    def __enter__(self):
        return self.path

    def __exit__(self, *exc):
        self.temp.cleanup()


def run(directory: Path, shards: int, floor: float = 90.0) -> tuple[int, str]:
    argv = ["coverage-merge.py", "--floor", str(floor), "--shards", str(shards), str(directory)]
    out = StringIO()
    with mock.patch("sys.argv", argv), redirect_stdout(out):
        status = cm.main()
    return status, out.getvalue()


class RealReportTests(unittest.TestCase):
    def test_each_shard_is_counted_as_llvm_cov_counted_it(self):
        for name in ("shard-1.json", "shard-2.json", "single-run.json"):
            with self.subTest(name=name):
                data = fixture(name)["data"][0]
                counts = [[r[4] for r in f["regions"]] for f in data["functions"]]
                mine = cm.file_lines(data["functions"], counts)
                # The report leaves out test files (b/tests/journey.rs), and so do the totals.
                self.assertIn("b/tests/journey.rs", mine)
                self.assertEqual({name: mine[name] for name in cm.reported(data)}, cm.reported(data))

    def test_the_merged_shards_equal_the_single_run(self):
        single = fixture("single-run.json")["data"][0]["totals"]["lines"]
        with Shards(fixture("shard-1.json"), fixture("shard-2.json")) as directory:
            lines, hit, _ = cm.merge(cm.shard_reports(directory, 2))
        self.assertEqual((lines, hit), (single["count"], single["covered"]))
        self.assertEqual((lines, hit), (14, 10))

    def test_the_shards_alone_cover_less(self):
        for name, hit in (("shard-1.json", 7), ("shard-2.json", 8)):
            with self.subTest(name=name):
                self.assertEqual(fixture(name)["data"][0]["totals"]["lines"]["covered"], hit)


class FloorTests(unittest.TestCase):
    def test_89_9_percent_fails_and_90_0_passes(self):
        for covered, status in ((899, 1), (900, 0)):
            with self.subTest(covered=covered), \
                    Shards(synthetic(set(range(0, covered, 2))), synthetic(set(range(1, covered, 2)))) as directory:
                code, out = run(directory, 2)
                self.assertEqual(code, status, out)
                self.assertIn(f"cover {covered} of 1000 lines", out)
                if status:
                    self.assertIn("is under the 90 percent floor", out)

    def test_the_floor_is_the_rust_coverage_targets(self):
        self.assertEqual(cm.floor_from(ROOT / ".codeflow" / "test-config.json"), 90.0)
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "test-config.json"
            config.write_text(json.dumps({"targets": [{"name": "rust-coverage", "modes": {"full": {
                "command": "cargo llvm-cov nextest --workspace --no-fail-fast --profile codeflow"}}}]}))
            with self.assertRaises(cm.Problem):
                cm.floor_from(config)


class RefusalTests(unittest.TestCase):
    def assert_refused(self, directory: Path, shards: int, needle: str) -> None:
        code, out = run(directory, shards, floor=0.0)
        self.assertEqual(code, 1, out)
        self.assertIn(needle, out)

    def test_a_missing_shard_is_refused(self):
        with Shards(fixture("shard-1.json"), None) as directory:
            self.assert_refused(directory, 2, "no coverage report from shard(s) [2]")
        with Shards(fixture("shard-1.json"), fixture("shard-2.json")) as directory:
            self.assert_refused(directory, 3, "no coverage report from shard(s) [3]")
            self.assert_refused(directory, 1, "outside 1 to 1")

    def test_an_empty_or_summary_only_shard_is_refused(self):
        summary = {"data": [{"files": [], "totals": {"lines": {"count": 0, "covered": 0}}}]}
        for report in ("", "{}", summary):
            with self.subTest(report=report), Shards(fixture("shard-1.json"), report) as directory:
                self.assert_refused(directory, 2, "coverage-data-2")

    def test_a_file_or_function_only_one_shard_reports_is_refused(self):
        extra_file = copy.deepcopy(fixture("shard-2.json"))
        extra_file["data"][0]["files"].append(
            {"filename": "c/src/lib.rs", "summary": {"lines": {"count": 1, "covered": 0}}})
        missing_function = copy.deepcopy(fixture("shard-2.json"))
        missing_function["data"][0]["functions"].pop()
        moved_region = copy.deepcopy(fixture("shard-2.json"))
        moved_region["data"][0]["functions"][0]["regions"][0][0] += 1
        for label, report in (("a file", extra_file), ("a function", missing_function), ("a region", moved_region)):
            with self.subTest(label), Shards(fixture("shard-1.json"), report) as directory:
                self.assert_refused(directory, 2, "not built from the same code")

    def test_a_count_this_merge_would_not_reproduce_is_refused(self):
        wrong = copy.deepcopy(fixture("shard-2.json"))
        wrong["data"][0]["files"][0]["summary"]["lines"]["covered"] += 1
        with Shards(fixture("shard-1.json"), wrong) as directory:
            self.assert_refused(directory, 2, "differently from llvm-cov")

    def test_functions_in_another_order_still_merge(self):
        reordered = copy.deepcopy(fixture("shard-2.json"))
        reordered["data"][0]["functions"].reverse()
        with Shards(fixture("shard-1.json"), reordered) as directory:
            self.assertEqual(cm.merge(cm.shard_reports(directory, 2))[:2], (14, 10))


class SegmentTests(unittest.TestCase):
    """Small cases of llvm-cov's line rules, for a reader of the port."""

    def lines(self, *regions):
        return cm.line_counts(cm.segments([list(r) for r in regions]))

    def test_a_region_over_three_lines_maps_three(self):
        self.assertEqual(self.lines((1, 1, 3, 2, 5, 0, 0, cm.CODE)), (3, 3))
        self.assertEqual(self.lines((1, 1, 3, 2, 0, 0, 0, cm.CODE)), (3, 0))

    def test_a_line_that_starts_inside_an_unrun_branch_is_not_covered(self):
        # The body on lines 1 to 6 runs; the branch from line 3 column 5 to
        # line 4 never does. Line 3 starts in the body, so it counts as run;
        # line 4 starts in the branch, so it does not.
        self.assertEqual(self.lines((1, 1, 6, 2, 1, 0, 0, cm.CODE), (3, 5, 4, 6, 0, 0, 0, cm.CODE)), (6, 5))

    def test_a_skipped_region_maps_no_line(self):
        self.assertEqual(self.lines((1, 1, 2, 2, 1, 0, 0, cm.CODE), (4, 1, 5, 2, 0, 0, 0, cm.SKIPPED)), (2, 2))

    def test_a_group_counts_its_best_instantiation(self):
        one = {"name": "f<u8>", "filenames": ["x.rs"], "regions": [[1, 1, 3, 2, 1, 0, 0, cm.CODE],
                                                                    [2, 5, 2, 9, 0, 0, 0, cm.CODE]]}
        two = {"name": "f<String>", "filenames": ["x.rs"], "regions": [[1, 1, 3, 2, 1, 0, 0, cm.CODE],
                                                                        [2, 5, 2, 9, 1, 0, 0, cm.CODE]]}
        counts = [[r[4] for r in f["regions"]] for f in (one, two)]
        self.assertEqual(cm.file_lines([one, two], counts), {"x.rs": (3, 3)})
        self.assertEqual(cm.file_lines([one], counts[:1]), {"x.rs": (3, 3)})


if __name__ == "__main__":
    unittest.main()

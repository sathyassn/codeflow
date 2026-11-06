#!/usr/bin/env python3
"""Controls for scripts/disk-margin.py (TSK-1157 AC-11 to AC-13).

The coverage shards record their lowest free disk and fail under the margin.
A low point under the margin fails, one at or above it passes, and a missing
or unreadable record fails: a shard whose watch never ran has no measured
margin.
"""

import importlib.util
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("disk_margin", ROOT / "scripts" / "disk-margin.py")
dm = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dm)

GIB = dm.GIB


def samples(*free_gib: float):
    values = iter(free_gib)
    return lambda path: {"free": int(next(values) * GIB), "total": 72 * GIB, "at": 100.0}


class Watch(unittest.TestCase):
    def test_the_record_keeps_the_lowest_sample_and_the_latest_time(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "low.json"
            times = iter([100.0, 105.0, 110.0, 115.0])
            def timed(path, base=samples(20, 9, 15, 12)):
                return {**base(path), "at": next(times)}
            with mock.patch.object(dm, "sample", timed), mock.patch.object(dm.time, "sleep"):
                dm.watch(out, Path(directory), 1.0, samples=4)
            low = json.loads(out.read_text())
            self.assertEqual(low["free"], 9 * GIB)
            self.assertEqual(low["at"], 105.0)
            self.assertEqual(low["latest"], 115.0)
            self.assertEqual(low["samples"], 4)
            self.assertFalse(out.with_name("low.json.partial").exists())

    def test_a_real_sample_reads_this_file_system(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "low.json"
            dm.watch(out, Path(directory), 0.01, samples=2)
            low = json.loads(out.read_text())
            self.assertGreater(low["total"], 0)
            self.assertLessEqual(low["free"], low["total"])


NOW = 1000.0


def live(free_gib: float):
    return lambda path: {"free": int(free_gib * GIB), "total": 72 * GIB, "at": NOW}


class Report(unittest.TestCase):
    """The live free space and the record are each judged: either under the
    margin fails, a stale or missing record fails, and both at the margin pass
    (Grok's security review of 148ea3709, finding 1)."""

    def run_report(self, low: dict | None, margin: float = 10.0, raw: str | None = None, now_gib: float = 30.0):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "low.json"
            if raw is not None:
                path.write_text(raw)
            elif low is not None:
                path.write_text(json.dumps(low))
            sized = Path(directory) / "tree"
            sized.mkdir()
            (sized / "a").write_bytes(b"x" * 10000)
            with mock.patch.object(dm, "sample", live(now_gib)):
                return dm.report(path, margin, Path(directory), [sized, Path(directory) / "absent"], 5.0)

    def low(self, free_gib: float, latest: float = NOW - 2) -> dict:
        return {"free": int(free_gib * GIB), "total": 72 * GIB, "at": NOW - 300, "samples": 30, "latest": latest}

    def test_a_low_point_under_the_margin_fails(self):
        problems, lines = self.run_report(self.low(9.9))
        self.assertEqual(len(problems), 1)
        self.assertIn("lowest free disk was 9.9 GiB, under the 10 GiB margin", problems[0])
        self.assertTrue(any("lowest free 9.9 GiB" in line for line in lines), lines)

    def test_a_live_disk_under_the_margin_fails_whatever_the_record_says(self):
        problems, _ = self.run_report(self.low(20), now_gib=1)
        self.assertEqual(len(problems), 1)
        self.assertIn("free disk now is 1.0 GiB, under the 10 GiB margin", problems[0])

    def test_a_stale_latest_sample_fails(self):
        for latest in (NOW - 16, 0.0):
            with self.subTest(latest=latest):
                problems, _ = self.run_report(self.low(40, latest=latest))
                self.assertEqual(len(problems), 1)
                self.assertIn("the watch stopped", problems[0])
        self.assertEqual(self.run_report(self.low(40, latest=NOW - 15))[0], [])

    def test_a_low_point_and_live_disk_at_or_above_the_margin_pass(self):
        for free, now in ((10.0, 10.0), (30.0, 30.0), (10.0, 50.0)):
            with self.subTest(free=free, now=now):
                problems, lines = self.run_report(self.low(free), now_gib=now)
                self.assertEqual(problems, [])
                self.assertTrue(any("tree takes" in line for line in lines), lines)
                self.assertTrue(any("absent does not exist" in line for line in lines), lines)

    def test_a_missing_or_broken_record_fails(self):
        record = self.low(40)
        cases = (("missing", None), ("not json", "{"), ("no free", "{}"),
                 ("no latest", json.dumps({k: v for k, v in record.items() if k != "latest"})),
                 ("wrong type", json.dumps({**record, "free": "a"})),
                 ("a boolean", json.dumps({**record, "latest": True})))
        for label, raw in cases:
            with self.subTest(label):
                problems, _ = self.run_report(None, raw=raw)
                self.assertEqual(len(problems), 1)
                self.assertIn("the margin is not measured", problems[0])

    def test_the_command_fails_under_the_margin_and_refuses_bad_options(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "low.json"
            path.write_text(json.dumps(self.low(5)))
            with redirect_stdout(StringIO()) as out, mock.patch.object(dm, "sample", live(30)):
                self.assertEqual(dm.main(["report", "--low", str(path), "--min-free-gib", "10", "--path", directory]), 1)
            self.assertIn("disk-margin: FAILED", out.getvalue())
            with redirect_stdout(StringIO()), mock.patch.object(dm, "sample", live(30)):
                self.assertEqual(dm.main(["report", "--low", str(path), "--min-free-gib", "4", "--path", directory]), 0)
            for argv in (["report", "--low", str(path), "--min-free-gib", "0"],
                         ["report", "--low", str(path), "--min-free-gib", "nan"],
                         ["report", "--low", str(path), "--min-free-gib", "10", "--interval", "0"],
                         ["watch", "--out", str(path), "--interval", "0"]):
                with self.subTest(argv=argv), self.assertRaises(SystemExit):
                    dm.main(argv)

    def test_a_real_watch_then_report_passes_and_a_killed_watch_goes_stale(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "low.json"
            dm.watch(out, Path(directory), 0.01, samples=2)
            free = json.loads(out.read_text())["free"] / GIB
            margin = max(free / 2, 0.001)
            self.assertEqual(dm.report(out, margin, Path(directory), [], 5.0)[0], [])
            with mock.patch.object(dm.time, "time", return_value=dm.time.time() + 60):
                problems, _ = dm.report(out, margin, Path(directory), [], 5.0)
            self.assertTrue(any("the watch stopped" in p for p in problems), problems)


class TreeSize(unittest.TestCase):
    def test_it_counts_the_files_below(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "d").mkdir()
            (root / "d" / "f").write_bytes(b"x" * 100000)
            self.assertGreaterEqual(dm.tree_size(root), 100000)


if __name__ == "__main__":
    unittest.main()

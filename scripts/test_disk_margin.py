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
    def test_the_record_keeps_the_lowest_sample(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "low.json"
            with mock.patch.object(dm, "sample", samples(20, 9, 15, 12)), mock.patch.object(dm.time, "sleep"):
                dm.watch(out, Path(directory), 1.0, samples=4)
            low = json.loads(out.read_text())
            self.assertEqual(low["free"], 9 * GIB)
            self.assertEqual(low["samples"], 4)
            self.assertFalse(out.with_name("low.json.partial").exists())

    def test_a_real_sample_reads_this_file_system(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "low.json"
            dm.watch(out, Path(directory), 0.01, samples=2)
            low = json.loads(out.read_text())
            self.assertGreater(low["total"], 0)
            self.assertLessEqual(low["free"], low["total"])


class Report(unittest.TestCase):
    def run_report(self, low: dict | None, margin: float = 10.0, raw: str | None = None):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "low.json"
            if raw is not None:
                path.write_text(raw)
            elif low is not None:
                path.write_text(json.dumps(low))
            sized = Path(directory) / "tree"
            sized.mkdir()
            (sized / "a").write_bytes(b"x" * 10000)
            return dm.report(path, margin, Path(directory), [sized, Path(directory) / "absent"])

    def low(self, free_gib: float) -> dict:
        return {"free": int(free_gib * GIB), "total": 72 * GIB, "at": 0.0, "samples": 30}

    def test_a_low_point_under_the_margin_fails(self):
        problems, lines = self.run_report(self.low(9.9))
        self.assertEqual(len(problems), 1)
        self.assertIn("under the 10 GiB margin", problems[0])
        self.assertTrue(any("lowest free 9.9 GiB" in line for line in lines), lines)

    def test_a_low_point_at_or_above_the_margin_passes(self):
        for free in (10.0, 30.0):
            with self.subTest(free=free):
                problems, lines = self.run_report(self.low(free))
                self.assertEqual(problems, [])
                self.assertTrue(any("tree takes" in line for line in lines), lines)
                self.assertTrue(any("absent does not exist" in line for line in lines), lines)

    def test_a_missing_or_broken_record_fails(self):
        for label, low, raw in (("missing", None, None), ("not json", None, "{"), ("no free", None, "{}"),
                                ("wrong type", None, '{"free": "a", "total": 1, "at": 0, "samples": 1}')):
            with self.subTest(label):
                problems, _ = self.run_report(low, raw=raw)
                self.assertEqual(len(problems), 1)
                self.assertIn("the margin is not measured", problems[0])

    def test_the_command_fails_under_the_margin_and_refuses_bad_options(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "low.json"
            path.write_text(json.dumps(self.low(5)))
            with redirect_stdout(StringIO()) as out:
                self.assertEqual(dm.main(["report", "--low", str(path), "--min-free-gib", "10", "--path", directory]), 1)
            self.assertIn("disk-margin: FAILED", out.getvalue())
            with redirect_stdout(StringIO()):
                self.assertEqual(dm.main(["report", "--low", str(path), "--min-free-gib", "4", "--path", directory]), 0)
            for argv in (["report", "--low", str(path), "--min-free-gib", "0"],
                         ["watch", "--out", str(path), "--interval", "0"]):
                with self.subTest(argv=argv), self.assertRaises(SystemExit):
                    dm.main(argv)


class TreeSize(unittest.TestCase):
    def test_it_counts_the_files_below(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "d").mkdir()
            (root / "d" / "f").write_bytes(b"x" * 100000)
            self.assertGreaterEqual(dm.tree_size(root), 100000)


if __name__ == "__main__":
    unittest.main()

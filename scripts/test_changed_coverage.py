#!/usr/bin/env python3
"""Changed-line fixtures; no processes, repository walks or network calls.

The PR journey runs the actual report entry point against a miniature diff,
coverage report and binding record. Its explicit revisions model an event;
the fixture does not claim to be a hosted GitHub run.
"""

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("changed_coverage", Path(__file__).with_name("changed-coverage.py"))
cc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc)

BASE, HEAD = "a" * 40, "b" * 40
DIFF = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n-old\n+covered\n+untested\n"
LCOV = b"SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n"
XML = b'<coverage><packages><package><classes><class filename="src/lib.rs"><lines><line number="1" hits="1"/><line number="2" hits="0"/></lines></class></classes></package></packages></coverage>'


def binding(payload=LCOV, **changes):
    result = {"version": 1, "revision": HEAD, "run": "42", "attempt": "1", "target": "rust-coverage",
              "shard": 1, "shards": 2, "status": "success", "format": "lcov", "source_root": "/checkout",
              "sha256": hashlib.sha256(payload).hexdigest()}
    result.update(changes)
    return result


class CoverageTests(unittest.TestCase):
    def report(self, **changes):
        args = {"event": "pull_request", "base": BASE, "head": HEAD, "diff": DIFF,
                "inventory": ["src/lib.rs"], "coverage": {"src/lib.rs": {1: True, 2: False}},
                "excluded": {}, "suffixes": [".rs"]}
        args.update(changes)
        return cc.report(**args)

    def test_pr_with_base_under_floor_fails_and_names_the_untested_line(self):
        status, text = self.report()
        self.assertEqual(status, 1)
        for expected in ("floor 80%", "target 90%", "2: uncovered", "1/2", "50.00%"):
            self.assertIn(expected, text)

    def test_floor_is_80_and_target_is_90(self):
        diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -0,0 +1,10 @@\n" + "+line\n" * 10
        for hit, status in ((7, 1), (8, 0), (9, 0)):
            with self.subTest(hit=hit):
                self.assertEqual(self.report(diff=diff, coverage={"src/lib.rs": {i: i <= hit for i in range(1, 11)}})[0], status)

    def test_pr_without_resolvable_base_fails(self):
        for base in (None, "unknown", "missing", "0" * 40):
            with self.subTest(base=base), self.assertRaisesRegex(cc.Problem, "base.*no resolvable"):
                self.report(base=base)

    def test_local_without_base_enforces_nothing(self):
        status, text = self.report(event="local", base=None)
        self.assertEqual(status, 0)
        self.assertIn("unavailable: no comparison base; nothing enforced", text)
        self.assertNotIn("50.00", text)

    def test_push_without_base_fails(self):
        with self.assertRaises(cc.Problem):
            self.report(event="push", base=None)

    def test_empty_changed_set_for_code_range_fails(self):
        for diff in ("", "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n"):
            with self.subTest(diff=diff), self.assertRaisesRegex(cc.Problem, "empty changed set"):
                self.report(diff=diff)

    def test_added_code_missing_coverage_fails(self):
        with self.assertRaisesRegex(cc.Problem, "no coverage file"):
            self.report(coverage={})
        with self.assertRaisesRegex(cc.Problem, "empty changed set"):
            self.report(coverage={"src/lib.rs": {30: True}})

    def test_malformed_and_truncated_diff_fail(self):
        for diff in (DIFF[:-10], DIFF.replace("@@ -1 +1,2 @@", "@@ invalid @@"), DIFF + "+extra\n"):
            with self.subTest(diff=diff), self.assertRaises(cc.Problem):
                self.report(diff=diff)

    def test_rename_uses_the_head_path(self):
        diff = "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 60%\nrename from src/old.rs\nrename to src/new.rs\n--- a/src/old.rs\n+++ b/src/new.rs\n@@ -1 +1 @@\n-old\n+new\n"
        self.assertEqual(self.report(diff=diff, inventory=["src/new.rs"], coverage={"src/new.rs": {1: True}})[0], 0)

    def test_pure_rename_has_no_added_lines(self):
        diff = "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 100%\nrename from src/old.rs\nrename to src/new.rs\n"
        status, text = self.report(diff=diff, inventory=["src/new.rs"], coverage={})
        self.assertEqual(status, 0)
        self.assertIn("No added executable", text)

    def test_deleted_file_and_deletion_only_edit_have_no_head_lines(self):
        deleted = "diff --git a/src/lib.rs b/src/lib.rs\ndeleted file mode 100644\n--- a/src/lib.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-one\n-two\n"
        self.assertEqual(self.report(diff=deleted, inventory=[], coverage={})[0], 0)
        edited = deleted.replace("+++ /dev/null", "+++ b/src/lib.rs")
        self.assertEqual(self.report(diff=edited, coverage={})[0], 0)

    def test_quoted_unicode_and_space_path(self):
        self.assertEqual(cc.git_path('"b/src/\\303\\251 space.rs"'), "src/é space.rs")
        self.assertEqual(cc.git_path('"b/src/é\\tspace.rs"'), "src/é\tspace.rs")

    def test_lcov_and_cobertura(self):
        for kind, payload in (("lcov", LCOV), ("cobertura", XML)):
            with self.subTest(kind=kind):
                self.assertEqual(cc.line_map(payload, kind, "/checkout"), {"src/lib.rs": {1: True, 2: False}})

    def test_llvm_regions(self):
        payload = {"data": [{"files": [{"filename": "/checkout/src/lib.rs"}], "functions": [
            {"filenames": ["/checkout/src/lib.rs"], "regions": [[1, 1, 2, 5, 0, 0, 0, 0]]}]}]}
        self.assertEqual(cc.line_map(json.dumps(payload).encode(), "llvm", "/checkout"), {"src/lib.rs": {1: False, 2: False}})

    def test_malformed_coverage_and_xml_entities_fail(self):
        for kind, payload in (("lcov", b"DA:1,1"), ("lcov", b"SF:a.rs\nDA:0,1"),
                              ("cobertura", b'<!DOCTYPE coverage [<!ENTITY x "value">]><coverage/>')):
            with self.subTest(kind=kind), self.assertRaises(cc.Problem):
                cc.line_map(payload, kind, "/checkout")


class ShardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.roster = {("rust-coverage", 1): 2, ("rust-coverage", 2): 2}
        self.put(1, binding())
        self.put(2, binding(shard=2))

    def put(self, folder, record, payload=LCOV):
        directory = self.root / f"coverage-data-{folder}"
        directory.mkdir(exist_ok=True)
        (directory / cc.FILES[record["format"]]).write_bytes(payload)
        (directory / "record.json").write_text(json.dumps(record))

    def read(self):
        return cc.shard_coverage(self.root, self.roster, HEAD, "42", "1")

    def test_valid_roster(self):
        self.assertEqual(self.read(), {"src/lib.rs": {1: True, 2: False}})

    def test_stale_revision_run_and_attempt(self):
        for field, value in (("revision", BASE), ("run", "41"), ("attempt", "2")):
            self.put(1, binding(**{field: value}))
            with self.subTest(field=field), self.assertRaisesRegex(cc.Problem, field):
                self.read()

    def test_failed_shard(self):
        self.put(1, binding(status="failed"))
        with self.assertRaisesRegex(cc.Problem, "failed shard"):
            self.read()

    def test_duplicate_shard(self):
        self.put(3, binding())
        with self.assertRaisesRegex(cc.Problem, "duplicate shard"):
            self.read()

    def test_missing_shard(self):
        (self.root / "coverage-data-2" / "record.json").unlink()
        with self.assertRaisesRegex(cc.Problem, "missing coverage shard"):
            self.read()

    def test_target_roster_mismatch(self):
        for changes in ({"target": "other"}, {"shards": 3}, {"shard": 3}):
            self.put(1, binding(**changes))
            with self.subTest(changes=changes), self.assertRaisesRegex(cc.Problem, "roster"):
                self.read()

    def test_digest_mismatch(self):
        self.put(1, binding(sha256="0" * 64))
        with self.assertRaisesRegex(cc.Problem, "digest mismatch"):
            self.read()

    def test_missing_binding_record(self):
        (self.root / "coverage-data-3").mkdir()
        with self.assertRaisesRegex(cc.Problem, "no binding record"):
            self.read()

    def test_symlinked_report_fails(self):
        report = self.root / "coverage-data-1" / "coverage.info"
        report.unlink()
        report.symlink_to(self.root / "coverage-data-2" / "coverage.info")
        with self.assertRaisesRegex(cc.Problem, "regular report"):
            self.read()

    def test_bind_command_creates_a_digest_bound_record_and_refuses_overwrite(self):
        directory = self.root / "producer"
        directory.mkdir()
        (directory / "coverage.info").write_bytes(LCOV)
        argv = ["changed-coverage.py", "bind", "--format", "lcov", "--directory", str(directory),
                "--source-root", "/checkout", "--head", HEAD, "--run", "42", "--attempt", "1",
                "--shard", "1", "--shards", "2"]
        with patch.object(sys, "argv", argv), redirect_stdout(StringIO()):
            self.assertEqual(cc.main(), 0)
            self.assertEqual(cc.main(), 1)
        self.assertEqual(json.loads((directory / "record.json").read_text()), binding())

    def test_configured_target_and_shard_roster(self):
        config, plan = self.root / "config.json", self.root / "plan.json"
        config.write_text(json.dumps({"targets": [{"name": "rust-coverage", "modes": {"full": {"command": "coverage"}}}]}))
        plan.write_text('{"partitions":2}')
        self.assertEqual(cc.configured_roster(config, plan), self.roster)
        config.write_text('{"targets":[]}')
        with self.assertRaisesRegex(cc.Problem, "configured coverage targets"):
            cc.configured_roster(config, plan)

    def test_pr_fixture_journey(self):
        config, plan = self.root / "config.json", self.root / "plan.json"
        config.write_text(json.dumps({"targets": [{"name": "rust-coverage", "modes": {"full": {"command": "coverage"}}}]}))
        plan.write_text(json.dumps({"partitions": 2}))
        excluded = self.root / "exemptions.json"
        excluded.write_text('{"version":1,"exemptions":[]}')
        diff, paths, output = self.root / "range.diff", self.root / "paths", self.root / "report.txt"
        diff.write_text(DIFF)
        paths.write_bytes(b"src/lib.rs\x00")
        argv = ["changed-coverage.py", "report", "--event", "pull_request", "--base", BASE,
                "--head", HEAD, "--run", "42", "--attempt", "1", "--directory", str(self.root),
                "--config", str(config), "--plan", str(plan), "--exemptions", str(excluded),
                "--diff", str(diff), "--paths", str(paths), "--output", str(output)]
        out = StringIO()
        with patch.object(sys, "argv", argv), redirect_stdout(out):
            status = cc.main()
        self.assertEqual(status, 1)
        self.assertEqual(out.getvalue(), output.read_text())
        self.assertIn("'src/lib.rs':2: uncovered", output.read_text())
        self.assertIn("50.00%", output.read_text())


if __name__ == "__main__":
    unittest.main()

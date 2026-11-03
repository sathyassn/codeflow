"""TSK-450: the severity table and shared exception validator."""
import datetime
import contextlib
import io
import importlib.util
import json
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("dependency_audit", Path(__file__).with_name("dependency_audit.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
TODAY = datetime.date(2026, 11, 3)


def finding(severity=2, path="Cargo.lock"):
    names = ["informational", "low", "medium", "high", "critical"]
    report = {"results": [{"source": {"path": path}, "packages": [{
        "package": {"name": "demo", "ecosystem": "crates.io", "version": "1.0"},
        "vulnerabilities": [{"id": "GHSA-test", "aliases": ["CVE-test"],
                             "database_specific": {"severity": names[severity]}}]}]}]}
    return audit.findings(report, Path("/scan"))[0]


def entry(**changes):
    value = {"id": "GHSA-test", "package": "demo", "ecosystem": "crates.io", "owner": "team",
             "rationale": "tracked remediation", "expiry": "2099-01-01"}
    value.update(changes)
    return value


class Precedence(unittest.TestCase):
    def test_table_and_overlaps_at_both_levels(self):
        unreachable = entry(reachability="not-reachable", evidence="no call path; reviewed source")
        suppression = {"id": "GHSA-test", "directory": ".", "expiry": TODAY}
        cases = [
            ("new low", [], finding(1), [], [], False),
            ("new informational", [], finding(0), [], [], False),
            ("new medium triaged", [], finding(), [entry()], [], False),
            ("old low", [finding(1)], finding(1), [], [], True),
            ("old informational", [finding(0)], finding(0), [], [], True),
            ("medium untriaged", [finding()], finding(), [], [], False),
            ("medium triaged", [finding()], finding(), [entry()], [], True),
            ("high unknown reachability", [finding(3)], finding(3), [], [], False),
            ("high unreachable", [finding(3)], finding(3), [unreachable], [], True),
            ("critical unreachable", [finding(4)], finding(4), [unreachable], [], True),
            ("new high unreachable", [], finding(3), [unreachable], [], False),
            ("new suppressed", [], finding(3), [], [suppression], True),
            ("expired suppression", [finding()], finding(), [], [{**suppression, "expiry": datetime.date(2000, 1, 1)}], False),
            ("dev medium", [finding(path="dev/Cargo.lock")], finding(path="dev/Cargo.lock"), [], [], False),
            ("new ecosystem", [{**finding(1), "ecosystem": "npm"}], finding(1), [], [], False),
            ("new package", [{**finding(1), "package": "other"}], finding(1), [], [], False),
        ]
        for name, base, head, triage, suppressions, passes in cases:
            for level in ("block", "warn"):
                with self.subTest(name=name, level=level):
                    code, messages = audit.judge(base, [head], triage, suppressions, level, TODAY)
                    self.assertEqual(code, int(not passes and level == "block"))
                    self.assertTrue(messages)
                    if not passes:
                        self.assertIn("::error::" if level == "block" else "::warning::", messages[0])

    def test_invalid_entries_fail_at_warn_and_block(self):
        for bad in [entry(expiry="2000-01-01"), entry(owner=""), entry(rationale=""),
                    entry(expiry="never"), entry(reachability="not-reachable", evidence="")]:
            for level in ("block", "warn"):
                with self.subTest(entry=bad, level=level), self.assertRaises(audit.AuditError):
                    audit.judge([finding()], [finding()], [bad], [], level, TODAY)
        with self.assertRaises(audit.AuditError):
            audit.judge([finding(3)], [finding(3)], [entry()], [], "warn", TODAY)

    def test_hook_and_file_share_validation(self):
        valid = entry()
        hook = audit.parse_hook(json.dumps(valid) + "\n")
        self.assertEqual(audit.validate_entries(hook, TODAY), audit.validate_entries([valid], TODAY))
        for line in ["GHSA-test\tdemo\t1.0\tcrates.io\tCargo.lock", "[]", "", "not json"]:
            if not line:
                continue  # no output accepts no entries
            with self.assertRaisesRegex(audit.AuditError, "Replace bare TSV"):
                audit.parse_hook(line)
        with self.assertRaises(audit.AuditError):
            audit.validate_entries(audit.parse_hook(json.dumps(entry(expiry="2000-01-01"))), TODAY, [finding()])

    def test_expired_entry_present_fails_even_at_warn(self):
        for level in ("block", "warn"):
            with self.assertRaisesRegex(audit.AuditError, "expired triage"):
                audit.judge([finding()], [finding()], [entry(expiry="2000-01-01")], [], level, TODAY)

    def test_expired_entry_absent_passes_with_named_notice(self):
        for level in ("block", "warn"):
            code, messages = audit.judge([finding()], [], [entry(expiry="2000-01-01")], [], level, TODAY)
            self.assertEqual(code, 0)
            self.assertEqual(len(messages), 1)
            self.assertIn("::notice::expired triage entry", messages[0])
            self.assertIn("GHSA-test", messages[0])
            self.assertIn("2000-01-01", messages[0])

    def test_invalid_entries_fail_even_when_absent(self):
        for bad in [entry(expiry="2000-01-01", owner=""), entry(expiry="never"),
                    entry(reachability="not-reachable", evidence=""), "bare finding"]:
            for level in ("block", "warn"):
                with self.assertRaises(audit.AuditError):
                    audit.judge([], [], [bad], [], level, TODAY)
        with self.assertRaises(audit.AuditError):
            audit.parse_hook("bare finding")
        with self.assertRaises(audit.AuditError):
            audit.judge([], [], [entry(), entry()], [], "warn", TODAY)

    def test_failed_or_unparseable_scan_never_supplies_absence(self):
        for level in ("block", "warn"):
            for failure in [audit.AuditError("scan failed"), audit.AuditError("invalid OSV JSON report")]:
                with tempfile.TemporaryDirectory() as temp:
                    with patch.object(audit.subprocess, "run"), \
                         patch.object(audit, "dependency_tree", return_value=Path(temp)), \
                         patch.object(audit, "scan", side_effect=[[finding()], failure]), \
                         patch.object(audit, "trusted_exceptions") as exceptions:
                        with self.assertRaises(audit.AuditError):
                            audit.run(Path("/scanner"), "base", level, Path(temp))
                        exceptions.assert_not_called()

    def test_osv_json_and_unknown_severity(self):
        report = {"results": [{"source": {"path": "/scan/dev/Cargo.lock"}, "packages": [{
            "package": {"name": "demo", "ecosystem": "crates.io", "version": "1.0"},
            "vulnerabilities": [{"id": "GHSA-test", "aliases": []}],
            "groups": [{"ids": ["GHSA-test"], "max_severity": "7.5"}]}]}]}
        rows = audit.findings(report, Path("/scan"))
        self.assertEqual(rows[0]["severity"], 3)
        self.assertEqual(rows[0]["path"], "dev/Cargo.lock")
        report["results"][0]["packages"][0]["groups"] = []
        self.assertEqual(audit.findings(report, Path("/scan"))[0]["severity"], 3)
        with self.assertRaises(audit.AuditError):
            audit.findings({}, Path("/scan"))

    def test_scan_root_alias_accepts_same_tree_and_refuses_outside(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp)
            tree = parent / "tree"
            tree.mkdir()
            alias = parent / "alias"
            alias.symlink_to(tree, target_is_directory=True)
            report = {"results": [{"source": {"path": str(tree.resolve() / "Cargo.lock")},
                                   "packages": [{"package": {"name": "demo", "ecosystem": "crates.io"},
                                                 "vulnerabilities": [{"id": "GHSA-test"}]}]}]}
            self.assertEqual(audit.findings(report, alias)[0]["path"], "Cargo.lock")
            report["results"][0]["source"]["path"] = str(alias / "Cargo.lock")
            self.assertEqual(audit.findings(report, tree)[0]["path"], "Cargo.lock")
            for outside in [parent / "Cargo.lock", parent / "tree-other" / "Cargo.lock"]:
                report["results"][0]["source"]["path"] = str(outside)
                with self.assertRaises(audit.AuditError):
                    audit.findings(report, alias)
            link = tree / "outside"
            link.symlink_to(parent / "Cargo.lock")
            report["results"][0]["source"]["path"] = str(link)
            with self.assertRaises(audit.AuditError):
                audit.findings(report, tree)

    def test_scan_errors_and_incomplete_reports_cannot_reach_the_table(self):
        from subprocess import CompletedProcess
        cases = [(128, "", ""), (128, "unexpected", ""),
                 (128, "unexpected", "No package sources found, --help for usage information."),
                 (127, "", "failed"), (128, "Error during extraction\n" + "x" * 2097152, ""),
                 (1, '{"results":[]}', ""), (0, '{"results":[]}', "read failed"),
                 (0, '{}', ""), (0, 'not json', "")]
        for status, out, err in cases:
            for level in ("block", "warn"):
                with self.subTest(status=status, level=level):
                    with patch.object(audit.subprocess, "run", return_value=CompletedProcess([], status, out, err)):
                        with self.assertRaises(audit.AuditError):
                            audit.scan(Path("/scanner"), Path("/scan"), Path("/config"))
        with patch.object(audit.subprocess, "run", side_effect=AssertionError("off must not scan")):
            self.assertEqual(audit.run(Path("/scanner"), "base", "off", Path("/work")), 0)

    def test_missing_base_inputs_warn_without_changing_expiry_verdict(self):
        for level in ("warn", "block"):
            with tempfile.TemporaryDirectory() as temp:
                work = Path(temp)
                base = work / "base-inputs"
                head = work / "head-inputs"
                base.mkdir()
                head.mkdir()
                for name in ["Cargo.lock", "yarn.lock", "go.mod"]:
                    (base / name).write_text("dependency input")
                # Delete Cargo.lock, rename yarn.lock, retain go.mod.
                for name in ["package-lock.json", "go.mod"]:
                    (head / name).write_text("dependency input")
                output = io.StringIO()
                with patch.object(audit.subprocess, "run"), \
                     patch.object(audit, "dependency_tree", side_effect=[base, head]), \
                     patch.object(audit, "scan", return_value=[]), \
                     patch.object(audit, "trusted_exceptions", return_value=([entry(expiry="2000-01-01")], [])), \
                     contextlib.redirect_stdout(output):
                    self.assertEqual(audit.run(Path("/scanner"), "base", level, work), 0)
                warnings = [line for line in output.getvalue().splitlines() if line.startswith("::warning::")]
                self.assertEqual(len(warnings), 2)
                self.assertIn('"Cargo.lock"', warnings[0])
                self.assertIn('"yarn.lock"', warnings[1])
                self.assertIn("::notice::expired triage entry", output.getvalue())

    def test_workflows_embed_the_same_judge(self):
        root = Path(__file__).resolve().parent.parent
        expected = Path(__file__).with_name("dependency_audit.py").read_text()
        for path in ["assets/base/ci/codeflow-ci.yml", ".codeflow/.baseline/.github/workflows/codeflow-ci.yml"]:
            text = (root / path).read_text()
            embedded = text.split("<<'CODEFLOW_AUDIT_PY'\n", 1)[1].split("          CODEFLOW_AUDIT_PY\n", 1)[0]
            self.assertEqual("\n".join(line[10:] for line in embedded.splitlines()) + "\n", expected)

    def test_scan_tree_contains_only_committed_dependency_inputs(self):
        from subprocess import CompletedProcess
        listing = (b"100644 blob a\tCargo.lock\0"
                   b"100644 blob b\tosv-scanner.toml\0"
                   b"100644 blob c\t.codeflow/audit-triage.json\0"
                   b"100755 blob d\tosv-scanner\0"
                   b"100644 blob e\t.gitignore\0")
        with tempfile.TemporaryDirectory() as temp:
            tree = Path(temp) / "inputs"
            with patch.object(audit.subprocess, "run", side_effect=[
                    CompletedProcess([], 0, listing), CompletedProcess([], 0, b"locked dependencies")]) as run:
                audit.dependency_tree("HEAD", tree)
                self.assertEqual(run.call_count, 2)
                self.assertEqual((tree / "Cargo.lock").read_bytes(), b"locked dependencies")
                self.assertEqual([p.name for p in tree.iterdir()], ["Cargo.lock"])
        for listing in [b"120000 blob a\tCargo.lock\0", b"160000 commit a\tvendor\0"]:
            with tempfile.TemporaryDirectory() as temp:
                with patch.object(audit.subprocess, "run", return_value=CompletedProcess([], 0, listing)):
                    with self.assertRaises(audit.AuditError):
                        audit.dependency_tree("HEAD", Path(temp) / "inputs")


if __name__ == "__main__":
    unittest.main()

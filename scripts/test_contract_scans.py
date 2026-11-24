#!/usr/bin/env python3
"""Exercise the repository scan runner with real Cargo and Rust test failures."""

from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class ContractScanJourneys(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="contract-scans-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.write("Cargo.toml", '[workspace]\nmembers=["crates/codeflow-core", "crates/codeflow-cli"]\nresolver="2"\n')
        self.write("crates/codeflow-core/Cargo.toml", '[package]\nname="codeflow-core"\nversion="0.0.0"\nedition="2021"\n')
        self.write("crates/codeflow-core/src/lib.rs", "")
        self.write("crates/codeflow-core/tests/existing_contract.rs", "#[test] fn existing_contract() {}\n")
        self.write("crates/codeflow-cli/Cargo.toml", '[package]\nname="codeflow-cli"\nversion="0.0.0"\nedition="2021"\n[[bin]]\nname="codeflow"\npath="src/main.rs"\n')
        self.write("crates/codeflow-cli/src/main.rs", "fn main() {}\n#[test] fn derived_lookup_fixture() {}\n")
        self.write("crates/codeflow-cli/tests/remedy_clearing.rs", "#[test] fn every_catalogued_row_is_proven_or_excluded_at_a_boundary() {}\n")
        self.write("crates/codeflow-cli/tests/test_gate_cli.rs", "#[test] fn every_tracked_path_is_classified_by_the_conservative_partition() {}\n")
        self.write("scripts/contract-scans.py", (ROOT / "scripts/contract-scans.py").read_text(encoding="utf-8"))

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def run_scans(self):
        env = os.environ.copy()
        # Respect the selected Cargo build directory; default to this worktree.
        control_target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "contract-scan-controls"
        env["CARGO_TARGET_DIR"] = str(control_target)
        return subprocess.run([sys.executable, "-B", "scripts/contract-scans.py"],
                              cwd=self.root, env=env, capture_output=True, text=True,
                              encoding="utf-8", timeout=120)

    def assert_control(self):
        result = self.run_scans()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def assert_failure(self, needle):
        result = self.run_scans()
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(needle, result.stdout + result.stderr)

    def test_a_new_scan_is_discovered_and_its_failure_reaches_the_caller(self):
        self.assert_control()
        for relative in ["new_repository_scan.rs", "new_repository_scan/main.rs"]:
            with self.subTest(relative=relative):
                name = "crates/codeflow-core/tests/" + relative
                self.write(name, '#[test] fn newly_added_scan() { panic!("new scan ran"); }\n')
                try:
                    self.assert_failure("new scan ran")
                finally:
                    (self.root / name).unlink()

    def test_a_missing_remedy_proof_fails_the_runner(self):
        self.assert_control()
        self.write("crates/codeflow-cli/tests/remedy_clearing.rs",
                   '#[test] fn every_catalogued_row_is_proven_or_excluded_at_a_boundary() { panic!("missing remedy proof"); }\n')
        self.assert_failure("missing remedy proof")

    def test_a_stale_derived_lookup_fails_the_runner(self):
        self.assert_control()
        self.write("crates/codeflow-cli/src/main.rs",
                   'fn main() {}\n#[test] fn derived_lookup_fixture() { panic!("stale lookup"); }\n')
        self.assert_failure("stale lookup")

    def test_an_unclassified_path_fails_the_runner(self):
        self.assert_control()
        self.write("crates/codeflow-cli/tests/test_gate_cli.rs",
                   '#[test] fn every_tracked_path_is_classified_by_the_conservative_partition() { panic!("unclassified path"); }\n')
        self.assert_failure("unclassified path")

    def test_an_empty_scan_inventory_fails_instead_of_passing_no_tests(self):
        self.assert_control()
        shutil.rmtree(self.root / "crates/codeflow-core/tests")
        self.assert_failure("no core integration scans discovered")

    def test_a_renamed_catalogue_test_cannot_silently_pass_an_empty_filter(self):
        self.assert_control()
        self.write("crates/codeflow-cli/tests/remedy_clearing.rs",
                   "#[test] fn renamed_catalogue_test() {}\n")
        self.assert_failure("ran no passing tests")

    def test_passing_run_exits_zero_and_prints_summary(self):
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0, output)
        self.assertIn("contract-scans: summary: 4 targets ran, 0 failed", output)

    def test_two_failing_targets_both_run_and_appear_in_summary(self):
        self.write(
            "crates/codeflow-core/tests/alpha_failing_scan.rs",
            '#[test] fn alpha_contract_fails() { panic!("alpha failure"); }\n',
        )
        self.write(
            "crates/codeflow-core/tests/zeta_failing_scan.rs",
            '#[test] fn zeta_contract_fails() { panic!("zeta failure"); }\n',
        )
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("alpha failure", output)
        self.assertIn("zeta failure", output)
        self.assertIn("contract-scans: summary: 6 targets ran, 2 failed", output)
        self.assertIn("contract-scans: failing targets: alpha_failing_scan, zeta_failing_scan", output)
        self.assertIn("contract-scans: failing tests: alpha_contract_fails, zeta_contract_fails", output)

    def test_core_failure_still_runs_subsequent_cli_commands(self):
        self.write(
            "crates/codeflow-core/tests/alpha_failing_scan.rs",
            '#[test] fn alpha_contract_fails() { panic!("alpha failure"); }\n',
        )
        self.write(
            "crates/codeflow-cli/tests/remedy_clearing.rs",
            '#[test] fn every_catalogued_row_is_proven_or_excluded_at_a_boundary() { panic!("remedy failure"); }\n',
        )
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("alpha failure", output)
        self.assertIn("remedy failure", output)
        self.assertIn("test_gate_cli", output)
        self.assertIn("derived_lookup", output)
        self.assertIn("contract-scans: summary: 5 targets ran, 2 failed", output)
        self.assertIn("contract-scans: failing targets: alpha_failing_scan, remedy_clearing", output)
        self.assertIn(
            "contract-scans: failing tests: alpha_contract_fails, "
            "every_catalogued_row_is_proven_or_excluded_at_a_boundary",
            output,
        )

    def test_empty_core_target_alongside_passing_target_is_named_and_refused(self):
        self.write("crates/codeflow-core/tests/empty_scan.rs", "")
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("ran no passing tests", output)
        self.assertIn("contract-scans: summary: 5 targets ran, 1 failed", output)
        self.assertIn("contract-scans: failing targets: empty_scan", output)

    def test_multiple_empty_core_targets_are_all_named_and_refused(self):
        self.write("crates/codeflow-core/tests/empty_alpha.rs", "")
        self.write("crates/codeflow-core/tests/empty_zeta.rs", "")
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("ran no passing tests", output)
        self.assertIn("contract-scans: summary: 6 targets ran, 2 failed", output)
        self.assertIn("contract-scans: failing targets: empty_alpha, empty_zeta", output)

    def test_compile_failure_reports_unrun_targets_without_fabricating_failed_target_count(self):
        self.write(
            "crates/codeflow-core/tests/broken_compile.rs",
            "fn not_valid_rust( { }\n",
        )
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("derived_lookup", output)
        self.assertIn(
            "contract-scans: summary: 3 targets ran, 0 failed, 1 command failed (2 targets not run)",
            output,
        )
        self.assertIn("contract-scans: targets not run: broken_compile, existing_contract", output)
        self.assertNotIn("contract-scans: failing targets:", output)

    def test_aborted_test_binary_is_named_and_counted_as_failed(self):
        self.write(
            "crates/codeflow-core/tests/aborting_scan.rs",
            "#[test] fn aborts_process() { std::process::abort(); }\n",
        )
        result = self.run_scans()
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn("contract-scans: summary: 5 targets ran, 1 failed", output)
        self.assertIn("contract-scans: failing targets: aborting_scan", output)


if __name__ == "__main__":
    unittest.main()

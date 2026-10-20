#!/usr/bin/env python3
"""Run this repository's cheap contracts through Cargo, including new scans.

TSK-385 selected whole-repository suites by the median of three warm runs
under 15 seconds on the reference host. Measurements live in its PR. No scan
was over the bound. New top-level core integration suites join by default;
classify a fixture-only suite here, or record measurements and its owning
mode before excluding a slow scan. Never skip a scan based on runtime speed.
"""

from pathlib import Path
import re
import subprocess
import sys
import time


# These are behavioral fixtures, not repository scans. Essential runs them
# through rust-workspace; full runs them through rust-coverage, except the
# explicitly qualified managed-secret-scan journey noted below.
OUTSIDE_SCANS = {
    "doctor_trust_probes": "doctor trust fixtures (essential/full)",
    "history_overlays": "synthetic history fixtures (essential/full)",
    "id_registry": "local remote and registry journeys (essential/full)",
    "integrate_merge": "merge and gate lifecycle fixtures (essential/full)",
    "integration_testing_setup": "test setup and runner fixtures (essential/full)",
    "managed_secret_scan": "ignored tool-backed journey (CI scaffold secret scan)",
    "model_catalog_surfaces": "fictional model catalogue fixtures (essential/full)",
    "permission_presets_update": "older-preset update fixtures (essential/full)",
    "prose_differential": "prose differential corpus (essential/full; prose-exhaustive)",
    "repository_update_noop": "already runs in quick repository-update-noop (essential/full)",
    "rule_reinjection_update": "older hook-wiring update fixtures (essential/full)",
    "scaffold_test": "scaffold engine fixtures (essential/full)",
    "shell_resolution_differential": "native shell differential corpus (essential/full)",
}


def commands(root: Path) -> list[list[str]]:
    tests = root / "crates/codeflow-core/tests"
    discovered = {path.stem for path in tests.glob("*.rs")}
    discovered.update(path.parent.name for path in tests.glob("*/main.rs"))
    scans = sorted(discovered - OUTSIDE_SCANS.keys())
    if not scans:
        raise ValueError("no core integration scans discovered")
    core = ["cargo", "test", "-p", "codeflow-core"]
    for scan in scans:
        core.extend(["--test", scan])
    return [
        core + ["--quiet"],
        ["cargo", "test", "-p", "codeflow-cli", "--test", "remedy_clearing",
         "every_catalogued_row_is_proven_or_excluded_at_a_boundary", "--", "--exact"],
        ["cargo", "test", "-p", "codeflow-cli", "--bin", "codeflow", "derived_lookup"],
    ]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        selected = commands(root)
    except ValueError as error:
        print(f"contract-scans: {error}", file=sys.stderr)
        return 1
    for command in selected:
        print("contract-scans: " + " ".join(command), flush=True)
        start = time.monotonic()
        result = subprocess.run(command, cwd=root, check=False,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        sys.stdout.buffer.write(result.stdout)
        sys.stdout.flush()
        print(f"contract-scans: exit {result.returncode}, {time.monotonic() - start:.3f}s",
              flush=True)
        if result.returncode:
            return 1
        if (not re.search(rb"test result: ok\. [1-9][0-9]* passed;", result.stdout)
                or b"test result: ok. 0 passed;" in result.stdout):
            print("contract-scans: a selected suite or filter ran no passing tests",
                  file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

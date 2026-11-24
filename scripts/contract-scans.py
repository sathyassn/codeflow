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
# explicitly qualified managed-leak-scan journey noted below.
OUTSIDE_SCANS = {
    "doctor_trust_probes": "doctor trust fixtures (essential/full)",
    "history_overlays": "synthetic history fixtures (essential/full)",
    "id_registry": "local remote and registry journeys (essential/full)",
    "integrate_merge": "merge and gate lifecycle fixtures (essential/full)",
    "integration_testing_setup": "test setup and runner fixtures (essential/full)",
    "managed_leak_scan": "ignored tool-backed journey (CI scaffold secret scan)",
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
    core = ["cargo", "test", "-p", "codeflow-core", "--no-fail-fast"]
    for scan in scans:
        core.extend(["--test", scan])
    return [
        core + ["--quiet"],
        ["cargo", "test", "-p", "codeflow-cli", "--no-fail-fast", "--test", "remedy_clearing",
         "every_catalogued_row_is_proven_or_excluded_at_a_boundary", "--", "--exact"],
        ["cargo", "test", "-p", "codeflow-cli", "--no-fail-fast", "--test", "test_gate_cli",
         "every_tracked_path_is_classified_by_the_conservative_partition", "--", "--exact"],
        ["cargo", "test", "-p", "codeflow-cli", "--no-fail-fast", "--bin", "codeflow", "derived_lookup"],
    ]


def command_targets(command: list[str]) -> list[str]:
    targets: list[str] = []
    index = 0
    while index < len(command) - 1:
        if command[index] in ("--test", "--bin"):
            targets.append(command[index + 1])
            index += 2
            continue
        index += 1
    return targets


def parse_command_output(
    output: bytes,
    selected_targets: list[str],
    returncode: int,
) -> tuple[int, int, list[str], list[str], bool, bool]:
    text = output.decode("utf-8", errors="replace")
    lines = text.splitlines()
    targets_ran = 0
    targets_failed = 0
    failing_targets: list[str] = []
    failing_tests: list[str] = []
    has_passing = False
    ran_no_passing = False

    for index, line in enumerate(lines):
        if re.match(r"^test result: ok\. [1-9][0-9]* passed;", line):
            targets_ran += 1
            has_passing = True
        elif re.match(r"^test result: ok\. 0 passed;", line):
            targets_ran += 1
            targets_failed += 1
            ran_no_passing = True
        elif line.startswith("test result: FAILED."):
            targets_ran += 1
            targets_failed += 1
            cursor = index - 1
            while cursor >= 0 and lines[cursor].strip() == "":
                cursor -= 1
            block: list[str] = []
            while cursor >= 0 and lines[cursor].startswith("    "):
                name = lines[cursor].strip()
                if name:
                    block.append(name)
                cursor -= 1
            if cursor >= 0 and lines[cursor].strip() == "failures:":
                for name in reversed(block):
                    if name not in failing_tests:
                        failing_tests.append(name)
        else:
            match = re.match(
                r"^(?:error: test failed, to rerun pass |\s+)[`']-p \S+ --(?:test|bin) (\S+)[`']$",
                line,
            )
            if match:
                target_name = match.group(1)
                if target_name not in failing_targets:
                    failing_targets.append(target_name)

    if returncode == 0 and not has_passing:
        ran_no_passing = True

    command_failed = returncode != 0 or ran_no_passing
    if command_failed:
        if targets_failed == 0:
            targets_failed = max(1, len(failing_targets))
        elif targets_failed < len(failing_targets):
            targets_failed = len(failing_targets)
        if not failing_targets and len(selected_targets) == 1:
            failing_targets.append(selected_targets[0])

    return (
        targets_ran,
        targets_failed,
        failing_targets,
        failing_tests,
        command_failed,
        ran_no_passing,
    )


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        selected = commands(root)
    except ValueError as error:
        print(f"contract-scans: {error}", file=sys.stderr)
        return 1

    total_ran = 0
    total_failed = 0
    all_failing_targets: list[str] = []
    all_failing_tests: list[str] = []
    any_failed = False

    for command in selected:
        print("contract-scans: " + " ".join(command), flush=True)
        start = time.monotonic()
        result = subprocess.run(command, cwd=root, check=False,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        sys.stdout.buffer.write(result.stdout)
        sys.stdout.flush()
        print(f"contract-scans: exit {result.returncode}, {time.monotonic() - start:.3f}s",
              flush=True)
        (
            ran,
            failed,
            failing_targets,
            failing_tests,
            command_failed,
            ran_no_passing,
        ) = parse_command_output(result.stdout, command_targets(command), result.returncode)
        total_ran += ran
        total_failed += failed
        for target in failing_targets:
            if target not in all_failing_targets:
                all_failing_targets.append(target)
        for test_name in failing_tests:
            if test_name not in all_failing_tests:
                all_failing_tests.append(test_name)
        if ran_no_passing:
            print("contract-scans: a selected suite or filter ran no passing tests",
                  file=sys.stderr)
        if command_failed:
            any_failed = True

    print(f"contract-scans: summary: {total_ran} targets ran, {total_failed} failed",
          flush=True)
    if all_failing_targets:
        print("contract-scans: failing targets: " + ", ".join(all_failing_targets),
              flush=True)
    if all_failing_tests:
        print("contract-scans: failing tests: " + ", ".join(all_failing_tests),
              flush=True)
    return 1 if any_failed else 0


if __name__ == "__main__":
    sys.exit(main())

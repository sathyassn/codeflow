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
        core + ["--", "--quiet"],
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
        if command[index] == "--":
            break
        if command[index] in ("--test", "--bin"):
            targets.append(command[index + 1])
            index += 2
            continue
        index += 1
    return targets


def parse_running_target(line: str, selected_targets: list[str]) -> str | None:
    match = re.match(r"^\s*Running tests/([^/\s]+)(?:/main)?\.rs \(.+\)$", line)
    if match:
        return match.group(1)
    match = re.match(r"^\s*Running unittests .+ \(.+[/\\]([^/\\\s]+)-[0-9a-f]+(?:\.exe)?\)$", line)
    if match:
        return match.group(1)
    if re.match(r"^\s*Running ", line) and len(selected_targets) == 1:
        return selected_targets[0]
    return None


def parse_command_output(
    output: bytes,
    selected_targets: list[str],
    returncode: int,
) -> tuple[int, int, list[str], list[str], list[str], bool, bool, bool]:
    text = output.decode("utf-8", errors="replace")
    lines = text.splitlines()
    target_outcomes: dict[str, str] = {}
    failing_tests: list[str] = []
    rerun_targets: list[str] = []
    current_target: str | None = None
    ran_no_passing = False

    def record_outcome(outcome: str) -> None:
        nonlocal current_target
        target = current_target
        if target is None and len(selected_targets) == 1:
            target = selected_targets[0]
        if target is not None:
            target_outcomes[target] = outcome
        current_target = None

    for index, line in enumerate(lines):
        started = parse_running_target(line, selected_targets)
        if started is not None:
            if current_target is not None and target_outcomes.get(current_target) == "started":
                target_outcomes[current_target] = "aborted"
            current_target = started
            target_outcomes[current_target] = "started"
            continue

        if re.match(r"^test result: ok\. [1-9][0-9]* passed;", line):
            record_outcome("passed")
        elif re.match(r"^test result: ok\. 0 passed;", line):
            ran_no_passing = True
            record_outcome("empty")
        elif line.startswith("test result: FAILED."):
            record_outcome("failed")
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
                if target_name not in rerun_targets:
                    rerun_targets.append(target_name)

    if current_target is not None and target_outcomes.get(current_target) == "started":
        target_outcomes[current_target] = "aborted"

    for target_name in rerun_targets:
        if target_outcomes.get(target_name) not in ("failed", "aborted", "empty"):
            target_outcomes[target_name] = "aborted"

    targets_ran = len(target_outcomes)
    failing_targets = [
        target for target, outcome in target_outcomes.items()
        if outcome in ("failed", "aborted", "empty")
    ]
    targets_failed = len(failing_targets)
    targets_not_run = [target for target in selected_targets if target not in target_outcomes]

    if returncode == 0 and not any(outcome == "passed" for outcome in target_outcomes.values()):
        ran_no_passing = True

    command_failed_without_results = (
        bool(targets_not_run)
        or (returncode != 0 and targets_failed == 0)
        or (returncode == 0 and targets_ran == 0)
    )
    any_failed = returncode != 0 or ran_no_passing or targets_failed > 0 or command_failed_without_results

    return (
        targets_ran,
        targets_failed,
        failing_targets,
        failing_tests,
        targets_not_run,
        any_failed,
        command_failed_without_results,
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
    command_failures = 0
    all_failing_targets: list[str] = []
    all_failing_tests: list[str] = []
    all_targets_not_run: list[str] = []
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
            targets_not_run,
            cmd_any_failed,
            cmd_failed_without_results,
            ran_no_passing,
        ) = parse_command_output(result.stdout, command_targets(command), result.returncode)
        total_ran += ran
        total_failed += failed
        if cmd_failed_without_results:
            command_failures += 1
        for target in failing_targets:
            if target not in all_failing_targets:
                all_failing_targets.append(target)
        for test_name in failing_tests:
            if test_name not in all_failing_tests:
                all_failing_tests.append(test_name)
        for target in targets_not_run:
            if target not in all_targets_not_run:
                all_targets_not_run.append(target)
        if ran_no_passing:
            print("contract-scans: a selected suite or filter ran no passing tests",
                  file=sys.stderr)
        if cmd_any_failed:
            any_failed = True

    summary = f"contract-scans: summary: {total_ran} targets ran, {total_failed} failed"
    if command_failures or all_targets_not_run:
        cmd_word = "command" if command_failures == 1 else "commands"
        summary += f", {command_failures} {cmd_word} failed"
        if all_targets_not_run:
            summary += f" ({len(all_targets_not_run)} targets not run)"
    print(summary, flush=True)
    if all_failing_targets:
        print("contract-scans: failing targets: " + ", ".join(all_failing_targets),
              flush=True)
    if all_failing_tests:
        print("contract-scans: failing tests: " + ", ".join(all_failing_tests),
              flush=True)
    if all_targets_not_run:
        print("contract-scans: targets not run: " + ", ".join(all_targets_not_run),
              flush=True)
    return 1 if any_failed else 0


if __name__ == "__main__":
    sys.exit(main())

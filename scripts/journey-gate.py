#!/usr/bin/env python3
"""Journey gate (TSK-110 AC-1, SPC-013 R-104, R-107).

Runs every deterministic journey that crates/codeflow-cli/tests/journey_gate.toml
maps from R-104: each journey's passing control and fault tests. Tests of one
cargo target run in one `cargo test ... -- --exact <names>` call, and the gate
fails unless exactly that many tests ran and passed, so a renamed or vanished
journey test cannot pass silently. The benchmark runs in release mode with
`--include-ignored`, so its budget check runs beside it. A journey marked
`platform = "unix"` is skipped on Windows and said so; a `pending` journey is
reported with the task that ships it.

The map itself is checked against R-104 and the code by
crates/codeflow-cli/tests/journey_gate.rs.

Usage: python3 scripts/journey-gate.py [--list] [--skip-benchmark]
                                       [--test-threads N]
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tomllib
from collections import OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAP = ROOT / "crates" / "codeflow-cli" / "tests" / "journey_gate.toml"
RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed")
RAN = re.compile(r"^Ran (\d+) tests? in ", re.MULTILINE)


def load_journeys() -> list[dict]:
    with MAP.open("rb") as handle:
        return tomllib.load(handle)["journey"]


def plan(journeys: list[dict], on_unix: bool, with_benchmark: bool):
    """Group the tests to run by command. Returns (groups, skipped, pending)."""
    groups: "OrderedDict[tuple, list[str]]" = OrderedDict()
    skipped: list[str] = []
    pending: list[str] = []
    for journey in journeys:
        if journey.get("pending"):
            pending.append(f"{journey['text']} (pending on {journey['pending']})")
            continue
        if journey.get("platform") == "unix" and not on_unix:
            skipped.append(f"{journey['text']} (unix only)")
            continue
        benchmark = bool(journey.get("benchmark"))
        if benchmark and not with_benchmark:
            skipped.append(f"{journey['text']} (benchmark skipped)")
            continue
        for reference in journey["control"] + journey["fault"]:
            parts = reference.split()
            if parts[0] == "python":
                key = ("python", parts[1])
                name = parts[2]
            elif parts[1] == "--lib":
                key = ("cargo", parts[0], "--lib", None, False)
                name = parts[2]
            else:
                key = ("cargo", parts[0], "--test", parts[2], benchmark)
                name = parts[3]
            names = groups.setdefault(key, [])
            if name not in names:
                names.append(name)
    return groups, skipped, pending


def command(key: tuple, names: list[str], test_threads: str | None) -> list[str]:
    if key[0] == "python":
        return [sys.executable, "-B", key[1], *names]
    _, package, kind, target, benchmark = key
    cmd = ["cargo", "test"]
    if benchmark:
        cmd.append("--release")
    cmd += ["-p", package, kind]
    if target:
        cmd.append(target)
    cmd += ["--", "--exact"]
    if benchmark:
        cmd.append("--include-ignored")
    if test_threads:
        cmd += ["--test-threads", test_threads]
    return cmd + names


def run(key: tuple, names: list[str], test_threads: str | None) -> str | None:
    """Run one group; return the failure, or None when every test passed."""
    cmd = command(key, names, test_threads)
    print("journey gate: " + " ".join(cmd[:8]) + (" ..." if len(cmd) > 8 else ""), flush=True)
    done = subprocess.run(
        cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    )
    output = done.stdout
    if done.returncode != 0:
        sys.stdout.write(output)
        return f"{' '.join(cmd[:6])}: exit {done.returncode}"
    if key[0] == "python":
        ran = sum(int(count) for count in RAN.findall(output))
        if ran == 0:
            sys.stdout.write(output)
            return f"{key[1]} {' '.join(names)}: no test ran"
        return None
    results = RESULT.findall(output)
    passed = sum(int(count) for _, count, _ in results)
    failed = sum(int(count) for _, _, count in results)
    if failed or passed != len(names):
        sys.stdout.write(output)
        return (
            f"{' '.join(cmd[:6])}: {passed} of {len(names)} listed tests ran and passed; "
            "a journey test was renamed or removed"
        )
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--list", action="store_true", help="print the commands only")
    parser.add_argument("--skip-benchmark", action="store_true")
    parser.add_argument("--test-threads", default=os.environ.get("JOURNEY_GATE_TEST_THREADS"))
    args = parser.parse_args()

    journeys = load_journeys()
    groups, skipped, pending = plan(
        journeys, on_unix=os.name != "nt", with_benchmark=not args.skip_benchmark
    )
    if args.list:
        for key, names in groups.items():
            print(" ".join(command(key, names, args.test_threads)))
        return 0
    failures = [
        failure
        for key, names in groups.items()
        if (failure := run(key, names, args.test_threads)) is not None
    ]
    tests = sum(len(names) for names in groups.values())
    print(
        f"journey gate: {len(journeys)} journeys, {tests} tests in {len(groups)} runs; "
        f"{len(pending)} pending, {len(skipped)} skipped here"
    )
    for line in pending + skipped:
        print(f"  not run: {line}")
    for failure in failures:
        print(f"journey gate: FAILED {failure}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

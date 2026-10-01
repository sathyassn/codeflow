#!/usr/bin/env python3
"""Journey gate (TSK-110 AC-1, SPC-013 R-104, R-107).

Reads every mapped Rust journey from the candidate's nextest JUnit report,
keyed by package, test target and full test name. Each must occur exactly
once and pass. Python classes still run verbosely; a skipped or expected
failure is not proof. The ignored read benchmark has its own gate target.

Usage: python3 scripts/journey-gate.py --results <junit file or directory>
       python3 scripts/journey-gate.py --list
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET
from collections import OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAP = ROOT / "crates" / "codeflow-cli" / "tests" / "journey_gate.toml"
RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed")
# A verbose unittest line: `test_name (module.Class.test_name)`, or
# `(module.Class)` before Python 3.11. A docstring puts the status on the
# next line, so only the name is matched.
VERBOSE = re.compile(r"^(test\w*) \(([\w.]+)\)", re.MULTILINE)
SUMMARY = re.compile(r"^(OK|FAILED|NO TESTS RAN)(?: \((.*)\))?$", re.MULTILINE)


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
        if benchmark:
            skipped.append(f"{journey['text']} (own read-benchmark target)")
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
        return [sys.executable, "-B", key[1], "-v", *names]
    _, package, kind, target, _ = key
    suite = package if kind == "--lib" else f"{package}::{target}"
    return ["results", suite, *names]


def result_failure(key: tuple, names: list[str], results: Path | None) -> str | None:
    """Require one passing occurrence per exact nextest binary/name identity."""
    _, package, kind, target, _ = key
    suite = package if kind == "--lib" else f"{package}::{target}"
    if results is None:
        return f"{suite}: missing --results; a journey test was renamed or removed"
    reports = sorted(results.rglob("*.xml")) if results.is_dir() else [results]
    found = {name: [] for name in names}
    if not reports:
        return f"{suite}: missing results"
    for report in reports:
        try:
            started = os.environ.get("CODEFLOW_GATE_STARTED_AT")
            if started and report.stat().st_mtime < float(started):
                return f"{suite}: stale results: {report}"
            root = ET.parse(report).getroot()
            if root.tag not in ("testsuites", "testsuite"):
                return f"{suite}: malformed results: {report}"
            suites = [root] if root.tag == "testsuite" else root.findall("testsuite")
            for entry in suites:
                if entry.get("name") != suite:
                    continue
                for case in entry.findall("testcase"):
                    name = case.get("name")
                    if name in found:
                        found[name].append(not any(case.find(tag) is not None for tag in ("failure", "error", "skipped", "flakyFailure", "flakyError")))
        except (OSError, ET.ParseError, ValueError) as error:
            return f"{suite}: missing, malformed or truncated results: {error}"
    bad = [name for name, states in found.items() if states != [True]]
    if bad:
        return f"{suite}: {len(names) - len(bad)} of {len(names)} listed tests ran and passed; a journey test was renamed or removed (missing, duplicated, skipped or failed: {', '.join(bad)})"
    return None


def run(key: tuple, names: list[str], test_threads: str | None, results: Path | None = None) -> str | None:
    """Run one group; return the failure, or None when every test passed."""
    if key[0] != "python":
        print(f"journey gate: results {key[1]} {key[3] or '--lib'} ({len(names)} exact tests)", flush=True)
        return result_failure(key, names, results)
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
        failure = python_failure(names, output)
        if failure:
            sys.stdout.write(output)
            return f"{key[1]}: {failure}"
        return None
    return None


def python_failure(names: list[str], output: str) -> str | None:
    """Why a verbose unittest run does not prove every listed class, or None."""
    summaries = SUMMARY.findall(output)
    if not summaries:
        return "no unittest summary"
    status, detail = summaries[-1]
    if status != "OK" or detail:
        shown = f"{status} ({detail})" if detail else status
        return f"{shown}; a skipped or expected-failing test is not a passing journey"
    ran = set()
    for method, dotted in VERBOSE.findall(output):
        parts = dotted.split(".")
        ran.add(parts[-2] if parts[-1] == method and len(parts) > 1 else parts[-1])
    missing = [name for name in names if name not in ran]
    if missing:
        return f"no test ran in {', '.join(missing)}"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--results", type=Path, help="JUnit file or directory from this candidate run")
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
        if (failure := run(key, names, args.test_threads, args.results)) is not None
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

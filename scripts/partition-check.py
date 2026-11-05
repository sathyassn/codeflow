#!/usr/bin/env python3
"""Prove that hash-partitioned nextest jobs ran every listed test once (TSK-1321).

The Windows test partitions and the Linux coverage shards run
`cargo nextest run --partition hash:<n>/<count>`. nextest puts a test in
partition n when `xxh64(test name, seed 0) % count == n - 1`, and keeps that
rule fixed for the whole cargo-nextest 0.9 series (nextest-runner's
partition.rs). A test's partition depends on its name only, so no
measurements are needed, a new test never moves another one, and a rename
moves only the renamed test.

`reports` names the partitions whose JUnit report is missing: a job that
timed out or was cancelled writes none, so its tests did not finish and none
of them is known to have failed. It writes `missing=<numbers>` to
GITHUB_OUTPUT for the verdict.

`check` compares the union of the reports with the test list (the Windows
archive's list, or each coverage shard's list, which must agree) and fails
when a listed test ran in no partition, ran in two, ran outside the partition
its hash gives it, or ran without being listed. It prints each partition's
test minutes and the slowest test.

download-artifact writes a sole matching artifact into the results directory
itself, without its `<prefix>-<n>` directory. Such a report is numbered by
the hash of its tests, which must all agree.

Usage: partition-check.py reports --count <n> --prefix <name> --results <dir> [--list <json>...]
       partition-check.py check --count <n> --prefix <name> --list <json>... --results <dir>
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

NAME = "partition-check"
MASK = 0xFFFFFFFFFFFFFFFF
PRIME1 = 11400714785074694791
PRIME2 = 14029467366897019727
PRIME3 = 1609587929392839161
PRIME4 = 9650029242287828579
PRIME5 = 2870177450012600261


def _rotl(value: int, bits: int) -> int:
    return ((value << bits) | (value >> (64 - bits))) & MASK


def _round(acc: int, lane: int) -> int:
    acc = (acc + lane * PRIME2) & MASK
    return (_rotl(acc, 31) * PRIME1) & MASK


def _merge(acc: int, value: int) -> int:
    acc ^= _round(0, value)
    return (acc * PRIME1 + PRIME4) & MASK


def xxh64(data: bytes, seed: int = 0) -> int:
    """XXH64, the hash nextest's `hash:` partitioner uses."""
    size = len(data)
    at = 0
    if size >= 32:
        lanes = [(seed + PRIME1 + PRIME2) & MASK, (seed + PRIME2) & MASK, seed, (seed - PRIME1) & MASK]
        while at + 32 <= size:
            for index in range(4):
                start = at + index * 8
                lanes[index] = _round(lanes[index], int.from_bytes(data[start:start + 8], "little"))
            at += 32
        value = (_rotl(lanes[0], 1) + _rotl(lanes[1], 7) + _rotl(lanes[2], 12) + _rotl(lanes[3], 18)) & MASK
        for lane in lanes:
            value = _merge(value, lane)
    else:
        value = (seed + PRIME5) & MASK
    value = (value + size) & MASK
    while at + 8 <= size:
        value ^= _round(0, int.from_bytes(data[at:at + 8], "little"))
        value = (_rotl(value, 27) * PRIME1 + PRIME4) & MASK
        at += 8
    if at + 4 <= size:
        value ^= (int.from_bytes(data[at:at + 4], "little") * PRIME1) & MASK
        value = (_rotl(value, 23) * PRIME2 + PRIME3) & MASK
        at += 4
    while at < size:
        value ^= (data[at] * PRIME5) & MASK
        value = (_rotl(value, 11) * PRIME1) & MASK
        at += 1
    value ^= value >> 33
    value = (value * PRIME2) & MASK
    value ^= value >> 29
    value = (value * PRIME3) & MASK
    value ^= value >> 32
    return value


def partition_of(name: str, count: int) -> int:
    """The partition, 1 to count, that `--partition hash:<n>/<count>` runs a test in."""
    return xxh64(name.encode("utf-8")) % count + 1


def listed_tests(path: Path) -> set[tuple[str, str]]:
    """The non-ignored tests of a `cargo nextest list --message-format json`."""
    listing = json.loads(path.read_text(encoding="utf-8"))
    tests = set()
    for binary, suite in listing["rust-suites"].items():
        for name, case in suite["testcases"].items():
            if not case.get("ignored"):
                tests.add((binary, name))
    return tests


def listed_once(paths: list[Path]) -> set[tuple[str, str]]:
    """The tests every list names; lists that disagree come from different builds."""
    lists = [listed_tests(path) for path in paths]
    for path, tests in zip(paths[1:], lists[1:]):
        if tests != lists[0]:
            raise SystemExit(f"{NAME}: {path} lists other tests than {paths[0]}; the partitions ran different builds")
    return lists[0]


def ran(report: Path) -> tuple[list[tuple[str, str, float]], float]:
    """The tests a JUnit report holds with their seconds, and the report's run time."""
    root = ET.parse(report).getroot()
    suites = [root] if root.tag == "testsuite" else root.findall("testsuite")
    tests = [(suite.get("name"), case.get("name"), float(case.get("time") or 0))
             for suite in suites for case in suite.findall("testcase")]
    return tests, float(root.get("time") or 0)


def number_by_hash(report: Path, count: int) -> int:
    """The partition that wrote a report found outside its numbered directory:
    the one every test in it hashes to."""
    tests, _ = ran(report)
    numbers = sorted({partition_of(name, count) for _, name, _ in tests})
    if len(numbers) != 1:
        raise SystemExit(f"{NAME}: cannot tell which partition wrote {report}: its {len(tests)} tests hash to "
                         f"partitions {numbers}")
    return numbers[0]


def junit_reports(results: Path, prefix: str, count: int) -> dict[int, Path]:
    """Each partition's report, keyed by the number in `<prefix>-<n>`."""
    reports: dict[int, Path] = {}
    for report in sorted(results.rglob("*.xml")):
        found = re.search(rf"{re.escape(prefix)}-(\d+)(?:/|$)", report.relative_to(results).as_posix())
        number = int(found.group(1)) if found else number_by_hash(report, count)
        if number in reports:
            raise SystemExit(f"{NAME}: {report} and {reports[number]} are both partition {number}'s report")
        reports[number] = report
    return reports


def missing_partitions(reports: dict[int, Path], count: int) -> list[int]:
    return sorted(set(range(1, count + 1)) - set(reports))


def missing_message(missing: list[int], tests: int) -> str:
    """A partition without a report did not finish; none of its tests failed."""
    return (f"no JUnit report from partition(s) {' '.join(map(str, missing))}: the job ended without writing its "
            f"report (a timeout or a cancellation), so its {tests} listed tests did not finish; none of them is "
            "known to have failed")


def check(listed: set[tuple[str, str]], reports: dict[int, Path], count: int) -> tuple[list[str], list[str]]:
    """(problems, summary lines) for one run's reports against the test list."""
    problems = []
    missing = missing_partitions(reports, count)
    if missing:
        problems.append(missing_message(missing, sum(1 for _, name in listed if partition_of(name, count) in missing)))
    extra = sorted(set(reports) - set(range(1, count + 1)))
    if extra:
        problems.append(f"JUnit reports from partition(s) {extra}, outside 1 to {count}")
    where: dict[tuple[str, str], list[int]] = {}
    times: dict[int, float] = {}
    slowest = (0.0, "", "", 0)
    for partition, report in sorted(reports.items()):
        tests, times[partition] = ran(report)
        for binary, name, seconds in tests:
            where.setdefault((binary, name), []).append(partition)
            slowest = max(slowest, (seconds, binary, name, partition))
    for test in sorted(listed):
        found = where.get(test, [])
        home = partition_of(test[1], count)
        if not found:
            # A missing partition is named once above, not once per test.
            if home not in missing:
                problems.append(f"{test[0]} {test[1]} ran in no partition")
        elif len(found) > 1:
            problems.append(f"{test[0]} {test[1]} ran in partitions {found}")
        elif found[0] != home:
            problems.append(f"{test[0]} {test[1]} ran in partition {found[0]}; its hash gives partition {home}")
    for test in sorted(set(where) - listed):
        problems.append(f"{test[0]} {test[1]} ran but is not in the list of tests")
    counts = {p: sum(1 for found in where.values() for at in found if at == p) for p in sorted(reports)}
    summary = [f"{len(listed)} listed tests, {sum(counts.values())} ran in {len(reports)} of {count} partitions"]
    if times:
        mean = sum(times.values()) / len(times)
        shown = ", ".join(f"{p}: {seconds / 60:.1f}" for p, seconds in sorted(times.items()))
        summary.append(f"test minutes per partition {{{shown}}}; mean {mean / 60:.1f}, longest "
                       f"{max(times.values()) / 60:.1f} ({max(times.values()) / mean:.2f} of the mean)")
    if slowest[1]:
        summary.append(f"slowest test {slowest[1]} {slowest[2]}: {slowest[0]:.1f} s in partition {slowest[3]}")
    return problems, summary


def write_github(name: str, line: str) -> None:
    target = os.environ.get(name)
    if target:
        with open(target, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    # Each subcommand and flag is spelled out: scripts/windows-daily.py proves a
    # line runnable by finding what the daily runner passes in this text.
    gone = commands.add_parser("reports", help="name the partitions whose JUnit report is missing")
    proof = commands.add_parser("check", help="prove every listed test ran in exactly the partition its hash gives")
    for sub in (gone, proof):
        sub.add_argument("--count", type=int, required=True)
        sub.add_argument("--prefix", required=True, help="the results directory name before -<n>")
        sub.add_argument("--results", type=Path, required=True)
    gone.add_argument("--list", type=Path, nargs="+")
    proof.add_argument("--list", type=Path, nargs="+", required=True)
    args = parser.parse_args(argv)
    if args.count < 1:
        raise SystemExit(f"{NAME}: --count must be at least 1")
    reports = junit_reports(args.results, args.prefix, args.count)
    if args.command == "reports":
        missing = missing_partitions(reports, args.count)
        write_github("GITHUB_OUTPUT", "missing=" + " ".join(map(str, missing)))
        if missing:
            line = (f"partition(s) {' '.join(map(str, missing))} ended without a JUnit report (a timeout or a "
                    "cancellation); their tests did not finish, and none is known to have failed")
            print(f"::error::{line}")
            print(f"{NAME}: FAILED {line}")
            return 1
        print(f"{NAME}: all {args.count} partitions wrote a report")
        return 0
    # A sole surviving shard's list is written into the results directory
    # itself, so the lists that exist stand for all of them (they agree).
    paths = [path for path in [*args.list, args.results / "tests.json"] if path.exists()]
    if not paths:
        raise SystemExit(f"{NAME}: none of the test lists exists; the jobs that wrote them ended without their "
                         "results (a timeout or a cancellation)")
    problems, summary = check(listed_once(paths), reports, args.count)
    for line in summary:
        print(f"{NAME}: {line}")
        write_github("GITHUB_STEP_SUMMARY", f"- {line}")
    for problem in problems:
        print(f"{NAME}: FAILED {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

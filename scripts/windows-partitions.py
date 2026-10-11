#!/usr/bin/env python3
"""Split the Windows nextest suite into partitions by measured duration (TSK-265).

nextest's own partitioners (count, hash, slice) split by test count, so a
partition that drew the slow journeys ran twice as long as one that did not.
This script assigns tests to partitions from the durations that hosted runs
measured, and each partition runs the filterset this script prints for it.

The measurements live in scripts/windows-test-times.json:

- `binaries`: a whole test binary, with its total and serial seconds;
- `split`: a binary with a test of at least NAMED_SECONDS, or too large for
  one partition: its slow tests by name, with their seconds, and the total
  and serial seconds of the rest.

The assignment places the largest unit first, each into the partition whose
estimated time grows least. A partition's estimate is the larger of its test
seconds over the runner's threads and its serial-group seconds, which run one
at a time. Every test lands in exactly one partition by construction: each
known binary belongs to one partition, or, when split, each named test to one
partition and every other test of that binary to the partition of its rest;
a binary the measurements do not know goes to the rest partition. A new test
therefore runs without a data refresh, only less evenly.

`check` proves it on a run: it reads the archive's test list and every
partition's JUnit report, and fails when a listed test ran in no partition,
ran in two, or ran outside the partition this plan gives it.

Refresh the data from a few green runs (artifacts are kept seven days):

    gh run download <run id> --repo sathyassn/codeflow -p 'windows-junit-*' -D runs/<run id>
    python3 -B scripts/windows-partitions.py measure runs/<oldest> ... runs/<newest>

Usage: windows-partitions.py filter <partition>
       windows-partitions.py plan
       windows-partitions.py check --list <nextest list json> --results <dir>
       windows-partitions.py measure <junit dir>...
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "scripts" / "windows-test-times.json"
# A filterset matcher ends at `)` and splits at `,`; names outside these
# characters would need escaping, so they are refused instead.
BINARY_ID = re.compile(r"^[A-Za-z0-9_:/.-]+$")
TEST_NAME = re.compile(r"^[A-Za-z0-9_:]+$")
# A binary larger than this share of one partition's seconds is split by test.
SPLIT_SHARE = 0.6
# A split binary names its slowest tests until the rest is this share of a
# partition or less.
REST_SHARE = 0.25
# A test at least this slow (median seconds) is always its own unit.
NAMED_SECONDS = 20.0
# Defaults for `measure`; the data file records the values in use.
PARTITIONS = 12
THREADS = 4


def serial(binary: str, test: str) -> bool:
    """Whether .config/nextest.toml puts a test in the serial group. Used only
    for the time estimate; membership never depends on it."""
    return (binary == "codeflow-present" or binary.startswith("codeflow-present::")
            or binary.split("::")[-1] in ("present_cli", "init_e2e", "test_gate_cli")
            or "process_group" in test)


def load(path: Path = DATA) -> dict:
    data = json.loads(path.read_text())
    for binary in [*data["binaries"], *data["split"]]:
        if not BINARY_ID.match(binary):
            raise SystemExit(f"windows-partitions: binary id {binary!r} cannot be written in a filterset")
    for binary, split in data["split"].items():
        if binary in data["binaries"]:
            raise SystemExit(f"windows-partitions: {binary} is both whole and split")
        for name in split["tests"]:
            if not TEST_NAME.match(name):
                raise SystemExit(f"windows-partitions: test name {binary} {name!r} cannot be written in a filterset")
    return data


class Plan:
    """Which partition runs which test, and the filterset of each partition."""

    def __init__(self, data: dict):
        self.count = int(data["partitions"])
        threads = int(data["threads"])
        # A unit is a whole binary, a split binary's rest (name None) or one
        # named test of a split binary.
        units = [((binary, None), float(seconds), float(serial_seconds), False)
                 for binary, (seconds, serial_seconds) in data["binaries"].items()]
        for binary, split in data["split"].items():
            seconds, serial_seconds = split["rest"]
            units.append(((binary, None), float(seconds), float(serial_seconds), True))
            units += [((binary, name), float(value), float(value) if serial(binary, name) else 0.0, True)
                      for name, value in split["tests"].items()]
        units.sort(key=lambda unit: (-unit[1], unit[0][0], unit[0][1] or ""))
        self.seconds = [0.0] * self.count
        self.serial = [0.0] * self.count
        self.whole: dict[str, int] = {}
        self.tests: dict[tuple[str, str], int] = {}
        self.home: dict[str, int] = {}

        def estimate(index: int, seconds: float = 0.0, serial_seconds: float = 0.0) -> float:
            return max((self.seconds[index] + seconds) / threads, self.serial[index] + serial_seconds)

        for (binary, name), seconds, serial_seconds, split in units:
            index = min(range(self.count),
                        key=lambda i: (estimate(i, seconds, serial_seconds), self.seconds[i], i))
            self.seconds[index] += seconds
            self.serial[index] += serial_seconds
            if not split:
                self.whole[binary] = index
            elif name is None:
                self.home[binary] = index
            else:
                self.tests[(binary, name)] = index
        self.estimates = [estimate(i) for i in range(self.count)]
        # Binaries the data does not know run where the estimate is least.
        self.rest = min(range(self.count), key=lambda i: (self.estimates[i], i))

    def partition_of(self, binary: str, test: str) -> int:
        """The one partition (1-based) that runs a test."""
        if binary in self.whole:
            return self.whole[binary] + 1
        if binary in self.home:
            return self.tests.get((binary, test), self.home[binary]) + 1
        return self.rest + 1

    def filterset(self, partition: int) -> str:
        """The nextest filterset of a partition (1-based)."""
        if not 1 <= partition <= self.count:
            raise SystemExit(f"windows-partitions: partition {partition} is not 1 to {self.count}")
        index = partition - 1
        terms = [f"binary_id(={binary})" for binary, at in sorted(self.whole.items()) if at == index]
        for binary in sorted(self.home):
            named = sorted(name for (owner, name), at in self.tests.items() if owner == binary and at == index)
            if self.home[binary] == index:
                elsewhere = sorted(name for (owner, name), at in self.tests.items()
                                   if owner == binary and at != index)
                excluded = " | ".join(f"test(={name})" for name in elsewhere)
                terms.append(f"(binary_id(={binary}) & not ({excluded}))" if excluded else f"binary_id(={binary})")
            elif named:
                terms.append(f"(binary_id(={binary}) & ({' | '.join(f'test(={name})' for name in named)}))")
        if index == self.rest:
            known = " | ".join(f"binary_id(={binary})" for binary in sorted([*self.whole, *self.home]))
            terms.append(f"not ({known})")
        if not terms:
            raise SystemExit(f"windows-partitions: partition {partition} runs no test; lower the partition count")
        return " | ".join(terms)


def listed_tests(path: Path) -> set[tuple[str, str]]:
    """The non-ignored tests of a `cargo nextest list --message-format json`."""
    listing = json.loads(path.read_text())
    tests = set()
    for binary, suite in listing["rust-suites"].items():
        for name, case in suite["testcases"].items():
            if not case.get("ignored"):
                tests.add((binary, name))
    return tests


def junit_reports(results: Path) -> dict[int, Path]:
    """Each partition's report, keyed by the number in `windows-junit-<n>`."""
    reports = {}
    for report in sorted(results.rglob("*.xml")):
        found = re.search(r"windows-junit-(\d+)", report.as_posix())
        if not found:
            raise SystemExit(f"windows-partitions: {report} is not under a windows-junit-<n> directory")
        reports[int(found.group(1))] = report
    return reports


def ran(report: Path) -> tuple[list[tuple[str, str]], float]:
    """The tests a JUnit report holds, and the partition's recorded run time."""
    root = ET.parse(report).getroot()
    suites = [root] if root.tag == "testsuite" else root.findall("testsuite")
    tests = [(suite.get("name"), case.get("name")) for suite in suites for case in suite.findall("testcase")]
    return tests, float(root.get("time") or 0)


def check(plan: Plan, listed: set[tuple[str, str]], reports: dict[int, Path]) -> list[str]:
    problems = []
    missing = sorted(set(range(1, plan.count + 1)) - set(reports))
    if missing:
        problems.append(f"no JUnit report from partition(s) {missing}")
    extra = sorted(set(reports) - set(range(1, plan.count + 1)))
    if extra:
        problems.append(f"JUnit reports from partition(s) {extra}, outside 1 to {plan.count}")
    where: dict[tuple[str, str], list[int]] = {}
    times = {}
    for partition, report in sorted(reports.items()):
        tests, times[partition] = ran(report)
        for test in tests:
            where.setdefault(test, []).append(partition)
    for test in sorted(listed):
        found = where.get(test, [])
        if not found:
            problems.append(f"{test[0]} {test[1]} ran in no partition")
        elif len(found) > 1:
            problems.append(f"{test[0]} {test[1]} ran in partitions {found}")
        elif found[0] != plan.partition_of(*test):
            problems.append(f"{test[0]} {test[1]} ran in partition {found[0]}; the plan gives it {plan.partition_of(*test)}")
    for test in sorted(set(where) - listed):
        problems.append(f"{test[0]} {test[1]} ran but is not in the archive's list of tests")
    counts = {p: sum(1 for found in where.values() for at in found if at == p) for p in sorted(reports)}
    print(f"windows-partitions: {len(listed)} listed tests, {sum(counts.values())} ran; per partition {counts}")
    if times:
        mean = sum(times.values()) / len(times)
        shown = ", ".join(f"{p}: {seconds / 60:.1f}" for p, seconds in sorted(times.items()))
        print(f"windows-partitions: test minutes per partition {{{shown}}}; "
              f"mean {mean / 60:.1f}, longest {max(times.values()) / mean:.2f} of the mean")
    return problems


def measure(directories: list[Path], partitions: int, threads: int) -> dict:
    """Median seconds per test across runs; whole binaries unless too large."""
    samples: dict[tuple[str, str], list[float]] = {}
    runs = []
    for directory in directories:
        reports = sorted(directory.rglob("*.xml"))
        if not reports:
            raise SystemExit(f"windows-partitions: no JUnit report under {directory}")
        runs.append(directory.name)
        for report in reports:
            root = ET.parse(report).getroot()
            for suite in [root] if root.tag == "testsuite" else root.findall("testsuite"):
                for case in suite.findall("testcase"):
                    samples.setdefault((suite.get("name"), case.get("name")), []).append(float(case.get("time")))
    # The last run named is the newest; tests it no longer has are dropped.
    newest = set()
    for report in sorted(directories[-1].rglob("*.xml")):
        tests, _ = ran(report)
        newest.update(tests)
    seconds = {test: round(statistics.median(values), 3) for test, values in samples.items() if test in newest}
    share = sum(seconds.values()) / partitions * SPLIT_SHARE
    serial_share = sum(seconds.values()) / partitions / threads * SPLIT_SHARE
    by_binary: dict[str, dict[str, float]] = {}
    for (binary, name), value in seconds.items():
        by_binary.setdefault(binary, {})[name] = value
    whole, split = {}, {}
    for binary, tests in sorted(by_binary.items()):
        total = sum(tests.values())
        serial_total = sum(v for name, v in tests.items() if serial(binary, name))
        # Every slow test is its own unit, so slow tests spread across the
        # partitions instead of travelling together inside one binary: run
        # together, the slow scaffold and update tests slowed each other to
        # two or three times their measured time (run 38101487324). A
        # binary too large for one partition also names its slowest tests
        # until the rest is small enough to place anywhere.
        named, rest, rest_serial = {}, total, serial_total
        for name, value in sorted(tests.items(), key=lambda item: (-item[1], item[0])):
            large = rest > share * REST_SHARE or rest_serial > serial_share * REST_SHARE
            if value < NAMED_SECONDS and not (large and (total > share or serial_total > serial_share)):
                break
            named[name] = value
            rest -= value
            rest_serial -= value if serial(binary, name) else 0.0
        if not named:
            whole[binary] = [round(total, 3), round(serial_total, 3)]
            continue
        split[binary] = {"rest": [round(max(rest, 0.0), 3), round(max(rest_serial, 0.0), 3)],
                         "tests": dict(sorted(named.items()))}
    return {
        "_source": "median seconds per test on windows-latest from the windows-junit artifacts of codeflow-ci "
                   "runs " + ", ".join(runs) + "; regenerate with `scripts/windows-partitions.py measure`",
        "partitions": partitions,
        "threads": threads,
        "binaries": whole,
        "split": split,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    one = commands.add_parser("filter", help="print one partition's nextest filterset")
    one.add_argument("partition", type=int)
    commands.add_parser("plan", help="print each partition's estimated minutes")
    proof = commands.add_parser("check", help="prove every listed test ran in exactly its one partition")
    proof.add_argument("--list", type=Path, required=True)
    proof.add_argument("--results", type=Path, required=True)
    data = commands.add_parser("measure", help="write the measurements from JUnit reports")
    data.add_argument("directories", type=Path, nargs="+")
    data.add_argument("--partitions", type=int)
    data.add_argument("--threads", type=int)
    args = parser.parse_args()
    if args.command == "measure":
        try:
            current = json.loads(DATA.read_text())
        except (OSError, ValueError):
            current = {}
        out = measure(args.directories, args.partitions or current.get("partitions", PARTITIONS),
                      args.threads or current.get("threads", THREADS))
        # One line per binary or test keeps a refresh's diff readable.
        text = re.sub(r"\[\s+([^\[\]\s,]+),\s+([^\[\]\s,]+)\s+\]", r"[\1, \2]", json.dumps(out, indent=1))
        DATA.write_text(text + "\n")
        print(f"windows-partitions: wrote {DATA.relative_to(ROOT)}")
        return 0
    plan = Plan(load())
    if args.command == "filter":
        # No line ending: Windows Python would end the line with CR, which a
        # shell's command substitution keeps.
        sys.stdout.write(plan.filterset(args.partition))
        return 0
    if args.command == "plan":
        for index, estimate in enumerate(plan.estimates):
            print(f"partition {index + 1}: about {estimate / 60:.1f} minutes "
                  f"({plan.seconds[index] / 60:.1f} test minutes, {plan.serial[index] / 60:.1f} serial)")
        return 0
    problems = check(plan, listed_tests(args.list), junit_reports(args.results))
    for problem in problems:
        print(f"windows-partitions: FAILED {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

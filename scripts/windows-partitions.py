#!/usr/bin/env python3
"""Split the Windows nextest suite into partitions by measured duration (TSK-265).

nextest's own partitioners (count, hash, slice) split by test count, so a
partition that drew the slow journeys ran twice as long as one that did not.
This script assigns tests to partitions from the durations that hosted runs
measured, and each partition runs the filterset this script prints for it.
The Linux coverage shards use the same planner through
scripts/coverage-shards.py, with their own measurements (TSK-270).

The measurements live in scripts/windows-test-times.json:

- `binaries`: a whole test binary, with its total and serial seconds;
- `split`: a binary with a test of at least NAMED_SECONDS, or too large for
  one partition: its slow tests by name, with their seconds, and the total
  and serial seconds of the rest;
- `names`: every measured test by binary, so a listed test the data has not
  measured is known by name (a new test, or a renamed one);
- `default_seconds`: the mean measured test.

The assignment places the largest unit first, each into the partition whose
estimated time grows least. A partition's estimate is the larger of its test
seconds over the runner's threads and its serial-group seconds, which run one
at a time. Every test lands in exactly one partition by construction: each
known binary belongs to one partition, or, when split, each named test to one
partition and every other test of that binary to the partition of its rest;
a binary the measurements do not know is placed by the rule below. A new test
therefore runs without a data refresh, only less evenly.

A test the data has not measured is placed from the build's test list
(`--list`), which the partition jobs and `check` read, and sized at
`default_seconds`. An unmeasured test of a binary the data knows is its own
unit, so a binary that gained many tests spreads them (a whole binary becomes
a split one for the run). A binary the data does not know is split into one
unit per test, so even a small new binary spreads. Without a list, such
tests run with their binary (or its home partition when split), or in the
rest partition, the one with the least planned time. A filterset that would pass FILTERSET_LIMIT characters is
refused, because Windows limits a command line to 32,767.

Two checks keep the plan honest (TSK-1063):

- `drift` fails when more than UNMEASURED_SHARE of the listed tests are
  outside the measurements, when the plan puts more than
  UNMEASURED_PARTITION_MINUTES of guessed time in one partition, when the plan
  with the unmeasured tests sized as above puts a partition past
  PLAN_MINUTES, or when a filterset is too long. It runs in the build job,
  before any partition starts.
- `check` also fails a run when a partition's test minutes plus SETUP_MINUTES
  pass MARGIN_MINUTES, the 80 percent bound of the 30 minute job limit, and
  names a partition whose report is missing (a timeout or a cancellation)
  as not run, never as failing tests. `reports` names the missing
  partitions on their own, for the job that reads the reports. When exactly
  one report survives, download-artifact writes it without its numbered
  directory, so a report outside a `<prefix>-<n>` directory is matched to
  the partition that the plan gives most of its tests.

The margin: PLAN_MINUTES of planned test time plus SETUP_MINUTES of job setup
is 14 job minutes on a typical runner, so a runner 1.7 times slower (the worst
spread in TSK-1063's findings) still finishes at 22.4, under MARGIN_MINUTES.

`check` proves it on a run: it reads the test list (the archive's, or each
coverage shard's, which must agree) and every partition's JUnit report, and
fails when a listed test ran in no partition, ran in two, or ran outside the
partition this plan gives it.

Refresh the data from a few green runs of these partitions (artifacts are
kept seven days). Leave out runs of the older compiling partitions: they
ran the slow tests about 10 percent faster in total, and a few of them in
half the time (TSK-265):

    gh run download <run id> --repo sathyassn/codeflow -p 'windows-junit-*' -D runs/<run id>
    python3 -B scripts/windows-partitions.py measure runs/<oldest> ... runs/<newest>

Usage: windows-partitions.py filter <partition> [--list <nextest list json>...]
       windows-partitions.py plan [--list <nextest list json>...]
       windows-partitions.py drift --list <nextest list json>...
       windows-partitions.py reports --results <dir> [--list <nextest list json>...]
       windows-partitions.py check --list <nextest list json>... --results <dir>
       windows-partitions.py measure <junit dir>...
"""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "scripts" / "windows-test-times.json"
# The directory each partition's results are downloaded to ends in -<n>.
RESULTS = "windows-junit"
SOURCE = "on windows-latest from the windows-junit artifacts"
REFRESH = "scripts/windows-partitions.py measure"
# A filterset matcher ends at `)` and splits at `,`; names outside these
# characters would need escaping, so they are refused instead.
BINARY_ID = re.compile(r"^[A-Za-z0-9_:/.-]+$")
TEST_NAME = re.compile(r"^[A-Za-z0-9_:]+$")
# A binary larger than this share of one partition's seconds is split by test.
SPLIT_SHARE = 0.6
# A split binary names its slowest tests until the rest is this share of a
# partition or less. A larger share names fewer tests: the home partition's
# filterset excludes every test named elsewhere, and Windows limits a command
# line to 32,767 characters (at 0.25 the suite's 3000-test unit binary alone
# needed 31,000; at this share the longest filterset is about 13,000).
REST_SHARE = 0.75
# A test at least this slow (median seconds) is always its own unit.
NAMED_SECONDS = 20.0
# Defaults for `measure`; the data file records the values in use.
PARTITIONS = 12
THREADS = 4
# The job limit is 30 minutes. A partition should stay at 80 percent of it or
# less on a typical runner, so a run whose job would pass MARGIN_MINUTES fails
# `check` while it still finishes. SETUP_MINUTES is a job's time outside the
# test run (checkout, shells, downloads): median 1.1 and largest 1.9 over the
# 72 jobs of six runs. A plan keeps every partition's estimate at PLAN_MINUTES
# or less. More than UNMEASURED_SHARE of the listed tests outside the data
# makes the plan unreliable, so `drift` fails.
JOB_LIMIT_MINUTES = 30
MARGIN_MINUTES = 24
SETUP_MINUTES = 2
PLAN_MINUTES = 12
UNMEASURED_SHARE = 0.10
# The most guessed time a partition may carry: 10 percent of PLAN_MINUTES.
UNMEASURED_PARTITION_MINUTES = 1.2
# A command line is limited to 32,767 characters on Windows. The rest of the
# partition command (nextest, the archive and its options) takes under 600.
COMMAND_LIMIT = 32767
FILTERSET_LIMIT = COMMAND_LIMIT - 600


def serial(binary: str, test: str) -> bool:
    """Whether .config/nextest.toml puts a test in the serial group. Used only
    for the time estimate; membership never depends on it."""
    return (binary == "codeflow-present" or binary.startswith("codeflow-present::")
            or binary.split("::")[-1] in ("present_cli", "init_e2e", "test_gate_cli")
            or "process_group" in test)


def load(path: Path = DATA) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    for binary in [*data["binaries"], *data["split"], *data.get("names", {})]:
        if not BINARY_ID.match(binary):
            raise SystemExit(f"windows-partitions: binary id {binary!r} cannot be written in a filterset")
    for binary, split in data["split"].items():
        if binary in data["binaries"]:
            raise SystemExit(f"windows-partitions: {binary} is both whole and split")
        for name in split["tests"]:
            if not TEST_NAME.match(name):
                raise SystemExit(f"windows-partitions: test name {binary} {name!r} cannot be written in a filterset")
    return data


def unmeasured_tests(data: dict, listed: set[tuple[str, str]]) -> set[tuple[str, str]] | None:
    """The listed tests the measurements do not cover: a test whose name the
    data has not measured, in a binary it knows or not (a renamed test counts,
    its old name is simply no longer listed). None when the data records no
    names (it was written before they were recorded)."""
    names = data.get("names")
    if names is None:
        return None
    return {(binary, name) for binary, name in listed if name not in names.get(binary, ())}


def with_unmeasured(data: dict, listed: set[tuple[str, str]]) -> dict:
    """The data plus a unit for each listed test it does not cover, sized by
    `default_seconds`, so the planner places them like any other and they
    spread over the partitions:

    - an unmeasured test of a known binary is its own named test (a whole
      binary becomes a split one, its measured rest staying where it is);
    - a binary the data does not know is one unit per listed test, however
      small, so new tests cannot gather in one partition.

    Data without `default_seconds`, or a test list, is returned as it is."""
    default = float(data.get("default_seconds", 0))
    missing = unmeasured_tests(data, listed)
    if not default or not missing:
        return data
    binaries = dict(data["binaries"])
    split = {binary: {"rest": list(entry["rest"]), "tests": dict(entry["tests"]),
                      **({"listed": True} if entry.get("listed") else {})}
             for binary, entry in data["split"].items()}
    by_binary: dict[str, list[str]] = {}
    for binary, name in sorted(missing):
        if not BINARY_ID.match(binary):
            raise SystemExit(f"windows-partitions: listed binary id {binary!r} cannot be written in a filterset")
        by_binary.setdefault(binary, []).append(name)
    for binary, tests in by_binary.items():
        known = binary in binaries or binary in split
        for name in tests:
            if not TEST_NAME.match(name):
                raise SystemExit(f"windows-partitions: listed test {binary} {name!r} cannot be written in a filterset")
        if binary in binaries:
            split[binary] = {"rest": binaries.pop(binary), "tests": {}}
        elif not known:
            split[binary] = {"rest": [0.0, 0.0], "tests": {}, "listed": True}
        split[binary]["tests"].update({name: default for name in tests})
    return {**data, "binaries": binaries, "split": split}


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
        # A binary whose every test is named (its rest is empty): no partition
        # needs to exclude the others' tests, so each lists only its own.
        self.listed = {binary for binary, split in data["split"].items() if split.get("listed")}

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
            if binary in self.listed:
                if named:
                    terms.append(f"(binary_id(={binary}) & ({' | '.join(f'test(={name})' for name in named)}))")
            elif self.home[binary] == index:
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
        text = " | ".join(terms)
        if len(text) > FILTERSET_LIMIT:
            raise SystemExit(f"windows-partitions: the filterset of partition {partition} is {len(text)} characters, "
                             f"past {FILTERSET_LIMIT} (a Windows command line holds {COMMAND_LIMIT}); too many tests "
                             "are outside the measurements: refresh the data with "
                             "`scripts/windows-partitions.py measure` on a few recent green runs")
        return text


def listed_tests(path: Path) -> set[tuple[str, str]]:
    """The non-ignored tests of a `cargo nextest list --message-format json`."""
    listing = json.loads(path.read_text(encoding="utf-8"))
    tests = set()
    for binary, suite in listing["rust-suites"].items():
        for name, case in suite["testcases"].items():
            if not case.get("ignored"):
                tests.add((binary, name))
    return tests


def junit_reports(results: Path, prefix: str = RESULTS, identify=None) -> dict[int, Path]:
    """Each partition's report, keyed by the number in `<prefix>-<n>`. A report
    outside such a directory (download-artifact writes a sole artifact into
    the results directory itself) is numbered by `identify`."""
    reports = {}
    for report in sorted(results.rglob("*.xml")):
        found = re.search(rf"{re.escape(prefix)}-(\d+)", report.as_posix())
        if found:
            number = int(found.group(1))
        elif identify is not None:
            number = identify(report)
        else:
            raise SystemExit(f"windows-partitions: {report} is not under a {prefix}-<n> directory, and without the "
                             "test list (--list) the partition that wrote it cannot be told")
        if number in reports:
            raise SystemExit(f"windows-partitions: {report} and {reports[number]} are both partition {number}'s report")
        reports[number] = report
    return reports


def identifier(plan: "Plan", listed: set[tuple[str, str]]):
    """A function that names the partition of a report found outside its
    numbered directory: the one the plan gives most of the report's tests."""
    def identify(report: Path) -> int:
        tests, _ = ran(report)
        votes: dict[int, int] = {}
        for test in tests:
            if test in listed:
                at = plan.partition_of(*test)
                votes[at] = votes.get(at, 0) + 1
        ranked = sorted(votes.items(), key=lambda item: -item[1])
        if not ranked or ranked[0][1] * 2 <= len(tests) or (len(ranked) > 1 and ranked[0][1] == ranked[1][1]):
            raise SystemExit(f"windows-partitions: cannot tell which partition wrote {report}: its {len(tests)} tests "
                             f"belong to partitions {dict(ranked)} in the plan")
        return ranked[0][0]
    return identify


def ran(report: Path) -> tuple[list[tuple[str, str]], float]:
    """The tests a JUnit report holds, and the partition's recorded run time."""
    root = ET.parse(report).getroot()
    suites = [root] if root.tag == "testsuite" else root.findall("testsuite")
    tests = [(suite.get("name"), case.get("name")) for suite in suites for case in suite.findall("testcase")]
    return tests, float(root.get("time") or 0)


def missing_message(missing: list[int], planned: int) -> str:
    """A partition without a report did not finish; none of its tests failed."""
    return (f"no JUnit report from partition(s) {missing}: the job ended without writing its report "
            f"(a timeout or a cancellation), so its {planned} planned tests did not finish; "
            "none of them is known to have failed")


def check(plan: Plan, listed: set[tuple[str, str]], reports: dict[int, Path], refresh: str = REFRESH,
          setup: float = SETUP_MINUTES) -> list[str]:
    problems = []
    missing = sorted(set(range(1, plan.count + 1)) - set(reports))
    if missing:
        planned = sum(1 for test in listed if plan.partition_of(*test) in missing)
        problems.append(missing_message(missing, planned))
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
            # A missing partition is named once above, not once per test.
            if plan.partition_of(*test) not in missing:
                problems.append(f"{test[0]} {test[1]} ran in no partition")
        elif len(found) > 1:
            problems.append(f"{test[0]} {test[1]} ran in partitions {found}")
        elif found[0] != plan.partition_of(*test):
            problems.append(f"{test[0]} {test[1]} ran in partition {found[0]}; the plan gives it {plan.partition_of(*test)}")
    for test in sorted(set(where) - listed):
        problems.append(f"{test[0]} {test[1]} ran but is not in the list of tests")
    counts = {p: sum(1 for found in where.values() for at in found if at == p) for p in sorted(reports)}
    print(f"windows-partitions: {len(listed)} listed tests, {sum(counts.values())} ran; per partition {counts}")
    if times:
        mean = sum(times.values()) / len(times)
        shown = ", ".join(f"{p}: {seconds / 60:.1f} of {plan.estimates[p - 1] / 60:.1f}"
                          for p, seconds in sorted(times.items()) if 1 <= p <= plan.count)
        print(f"windows-partitions: test minutes per partition, measured of planned {{{shown}}}; "
              f"mean {mean / 60:.1f}, longest {max(times.values()) / mean:.2f} of the mean")
    for partition, seconds in sorted(times.items()):
        job = seconds / 60 + setup
        if job > MARGIN_MINUTES:
            problems.append(f"partition {partition} took {seconds / 60:.1f} test minutes, about {job:.1f} job minutes "
                            f"with setup, past the {MARGIN_MINUTES} minute margin of the {JOB_LIMIT_MINUTES} minute job "
                            f"limit; the plan's data has drifted from the suite: refresh it with `{refresh}` "
                            "on a few recent green runs (see the header of this script)")
    return problems


def drift(data: dict, listed: set[tuple[str, str]], refresh: str = REFRESH,
          plan_minutes: float = PLAN_MINUTES, spread: bool = True, setup: float = SETUP_MINUTES) -> list[str]:
    """Problems found before any partition runs: the data no longer covers the
    suite, too much of a partition's time is guessed, the plan it gives passes
    the planned-minutes margin, or a filterset cannot be run."""
    problems = []
    missing = unmeasured_tests(data, listed)
    if missing is None:
        problems.append("the data records no measured test names, so its coverage of the suite cannot be read: "
                        f"refresh it with `{refresh}`")
        missing = set()
    else:
        share = len(missing) / max(1, len(listed))
        print(f"windows-partitions: {len(missing)} of {len(listed)} listed tests are outside the measurements "
              f"({share * 100:.1f} percent; at most {UNMEASURED_SHARE * 100:.0f} percent)")
        if share > UNMEASURED_SHARE:
            problems.append(f"{len(missing)} of {len(listed)} listed tests ({share * 100:.1f} percent) are outside the "
                            f"measurements, more than {UNMEASURED_SHARE * 100:.0f} percent: refresh the data with "
                            f"`{refresh}` on a few recent green runs (see the header of this script)")
    plan = Plan(with_unmeasured(data, listed) if spread else data)
    default, threads = float(data.get("default_seconds", 0)), int(data["threads"])
    # The tests the data has not measured are guessed at `default` seconds each.
    guessed = [0.0] * plan.count
    for test in missing:
        guessed[plan.partition_of(*test) - 1] += default / threads
    estimates = list(plan.estimates) if spread else [e + g for e, g in zip(plan.estimates, guessed)]
    if missing:
        most = max(range(plan.count), key=guessed.__getitem__)
        print(f"windows-partitions: guessed test minutes per partition, most {guessed[most] / 60:.1f} in partition "
              f"{most + 1} (at most {UNMEASURED_PARTITION_MINUTES:g})")
        if guessed[most] / 60 > UNMEASURED_PARTITION_MINUTES:
            problems.append(f"partition {most + 1} holds {guessed[most] / 60:.1f} planned minutes of tests the data has "
                            f"not measured, more than {UNMEASURED_PARTITION_MINUTES:g}: refresh the data with "
                            f"`{refresh}` on a few recent green runs (see the header of this script)")
    longest = max(estimates) / 60
    print(f"windows-partitions: planned test minutes per partition, longest {longest:.1f} "
          f"(at most {plan_minutes:g}); setup adds up to {setup:g} job minutes, against the {MARGIN_MINUTES} minute margin")
    if longest > plan_minutes:
        problems.append(f"the plan puts a partition at {longest:.1f} test minutes, past {plan_minutes:g}: "
                        f"refresh the data with `{refresh}`, or raise the partition count when the suite itself grew")
    longest_filterset = 0
    for partition in range(1, plan.count + 1):
        try:
            longest_filterset = max(longest_filterset, len(plan.filterset(partition)))
        except SystemExit as refused:
            problems.append(str(refused.code).removeprefix("windows-partitions: "))
    print(f"windows-partitions: longest filterset {longest_filterset} characters (at most {FILTERSET_LIMIT})")
    return problems


def reports_missing(results: Path, count: int, prefix: str = RESULTS, identify=None) -> list[int]:
    """The partitions (1 to count) with no JUnit report under the results."""
    return sorted(set(range(1, count + 1)) - set(junit_reports(results, prefix, identify)))


def measure(directories: list[Path], partitions: int, threads: int, source: str = SOURCE,
            refresh: str = REFRESH) -> dict:
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
        "_source": f"median seconds per test {source} of codeflow-ci runs " + ", ".join(runs)
                   + f"; regenerate with `{refresh}`",
        "partitions": partitions,
        "threads": threads,
        # The mean test, which sizes a binary the data does not know.
        "default_seconds": round(sum(seconds.values()) / max(1, len(seconds)), 3),
        # Every measured test by binary, which `drift` compares with the list.
        "names": {binary: sorted(tests) for binary, tests in sorted(by_binary.items())},
        "binaries": whole,
        "split": split,
    }


def listed_once(paths: list[Path]) -> set[tuple[str, str]]:
    """The tests every list names; lists that disagree come from different builds."""
    lists = [listed_tests(path) for path in paths]
    for path, tests in zip(paths[1:], lists[1:]):
        if tests != lists[0]:
            raise SystemExit(f"windows-partitions: {path} lists other tests than {paths[0]}; "
                             "the partitions ran different builds")
    return lists[0]


def main(data_path: Path = DATA, prefix: str = RESULTS, source: str = SOURCE, refresh: str = REFRESH,
         description: str = __doc__, setup: float = SETUP_MINUTES, plan_minutes: float = PLAN_MINUTES,
         spread: bool = True) -> int:
    """`spread` places binaries the data does not know from the test list. A
    caller whose jobs cannot read the list before they run (the coverage
    shards) leaves it off, and then every command uses the rest-partition rule."""
    parser = argparse.ArgumentParser(description=description.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    one = commands.add_parser("filter", help="print one partition's nextest filterset")
    one.add_argument("partition", type=int)
    one.add_argument("--list", type=Path, nargs="+", help="the build's test list, to place unmeasured binaries")
    show = commands.add_parser("plan", help="print each partition's estimated minutes; fails past the plan margin")
    show.add_argument("--list", type=Path, nargs="+")
    gap = commands.add_parser("drift", help="fail when the data no longer covers the suite or the plan is too long")
    gap.add_argument("--list", type=Path, nargs="+", required=True)
    gone = commands.add_parser("reports", help="name the partitions whose JUnit report is missing")
    gone.add_argument("--results", type=Path, required=True)
    gone.add_argument("--list", type=Path, nargs="+", help="the build's test list, to number a sole report")
    proof = commands.add_parser("check", help="prove every listed test ran in exactly its one partition")
    proof.add_argument("--list", type=Path, nargs="+", required=True)
    proof.add_argument("--results", type=Path, required=True)
    data = commands.add_parser("measure", help="write the measurements from JUnit reports")
    data.add_argument("directories", type=Path, nargs="+")
    data.add_argument("--partitions", type=int)
    data.add_argument("--threads", type=int)
    args = parser.parse_args()
    if args.command == "measure":
        try:
            current = json.loads(data_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            current = {}
        out = measure(args.directories, args.partitions or current.get("partitions", PARTITIONS),
                      args.threads or current.get("threads", THREADS), source, refresh)
        # One line per binary or test keeps a refresh's diff readable.
        text = re.sub(r"\[\s+([^\[\]\s,]+),\s+([^\[\]\s,]+)\s+\]", r"[\1, \2]", json.dumps(out, indent=1))
        data_path.write_text(text + "\n")
        print(f"windows-partitions: wrote {data_path.relative_to(ROOT)}")
        return 0
    base = load(data_path)
    paths = getattr(args, "list", None)
    if paths and args.command == "check":
        # A sole surviving shard's list is written into the results directory
        # itself, so the lists that exist stand for all of them (they agree).
        paths = [path for path in [*paths, args.results / "tests.json"] if path.exists()]
        if not paths:
            raise SystemExit("windows-partitions: none of the test lists exists; the jobs that wrote them ended "
                             "without their results (a timeout or a cancellation)")
    listed = listed_once(paths) if paths else None
    plan = Plan(with_unmeasured(base, listed) if listed is not None and spread else base)
    if args.command == "filter":
        # No line ending: Windows Python would end the line with CR, which a
        # shell's command substitution keeps.
        sys.stdout.write(plan.filterset(args.partition))
        return 0
    if args.command == "plan":
        for index, estimate in enumerate(plan.estimates):
            print(f"partition {index + 1}: about {estimate / 60:.1f} minutes "
                  f"({plan.seconds[index] / 60:.1f} test minutes, {plan.serial[index] / 60:.1f} serial)")
        if max(plan.estimates) / 60 > plan_minutes:
            print(f"windows-partitions: FAILED a partition is planned past {plan_minutes:g} minutes; "
                  f"refresh the data with `{refresh}`")
            return 1
        return 0
    if args.command == "drift":
        problems = drift(base, listed, refresh, plan_minutes, spread, setup)
    elif args.command == "reports":
        missing = reports_missing(args.results, plan.count, prefix,
                                  identifier(plan, listed) if listed is not None else None)
        github_output = os.environ.get("GITHUB_OUTPUT")
        if github_output:
            with open(github_output, "a", encoding="utf-8") as handle:
                handle.write("missing=" + " ".join(map(str, missing)) + "\n")
        if missing:
            line = (f"partition(s) {' '.join(map(str, missing))} ended without a JUnit report (a timeout or a "
                    "cancellation); their tests did not finish, and none is known to have failed")
            print(f"::error::windows tests: {line}")
            print(f"windows-partitions: FAILED {line}")
            return 1
        print(f"windows-partitions: all {plan.count} partitions wrote a report")
        return 0
    else:
        problems = check(plan, listed, junit_reports(args.results, prefix, identifier(plan, listed)), refresh, setup)
    for problem in problems:
        print(f"windows-partitions: FAILED {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

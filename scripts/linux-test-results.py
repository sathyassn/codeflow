#!/usr/bin/env python3
"""Check that the uninstrumented Linux suite ran every non-ignored test once.

The list comes from the same nextest build as the JUnit report. Journey
membership remains in journey-gate.py; this checks the whole suite.
"""

import argparse
import importlib.util
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "windows_partitions", Path(__file__).with_name("windows-partitions.py"))
partitions = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(partitions)


def check(listing: Path, report: Path) -> list[str]:
    """Require exact membership and one passing result per listed test."""
    listed = partitions.listed_tests(listing)
    if not listed:
        return ["the nextest list contains no non-ignored tests"]
    root = ET.parse(report).getroot()
    if root.tag not in ("testsuites", "testsuite"):
        return [f"{report}: expected a JUnit testsuites or testsuite root"]
    suites = [root] if root.tag == "testsuite" else root.findall("testsuite")
    counts = Counter()
    problems = []
    for suite in suites:
        for case in suite.findall("testcase"):
            identity = (suite.get("name"), case.get("name"))
            counts[identity] += 1
            if any(case.find(tag) is not None for tag in
                   ("failure", "error", "skipped", "flakyFailure", "flakyError")):
                problems.append(f"{identity[0]} {identity[1]} did not pass on its first attempt")
    for binary, name in sorted(listed):
        count = counts[(binary, name)]
        if count != 1:
            problems.append(f"{binary} {name} has {count} results; expected exactly one")
    for binary, name in counts.keys() - listed:
        problems.append(f"{binary} {name} ran but is not in the list of tests")
    print(f"linux-test-results: {len(listed)} listed tests, {sum(counts.values())} results")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", type=Path, required=True)
    parser.add_argument("--results", type=Path, required=True)
    args = parser.parse_args()
    try:
        problems = check(args.list, args.results)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, ET.ParseError) as error:
        problems = [f"missing or malformed test evidence: {error}"]
    for problem in problems:
        print(f"linux-test-results: FAILED {problem}")
    return int(bool(problems))


if __name__ == "__main__":
    sys.exit(main())

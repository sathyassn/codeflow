#!/usr/bin/env python3
"""Split the Linux instrumented suite into coverage shards (TSK-270).

CI runs `cargo llvm-cov nextest` as shards on separate runners, and each
shard runs the filterset this script prints for it. The plan is the one the
Windows partitions use (scripts/windows-partitions.py): tests are placed by
measured duration, each test lands in exactly one shard by construction, and
a test the measurements do not know runs in the rest shard. Only the
measurements differ: scripts/coverage-test-times.json holds the instrumented
run's seconds on the Linux runner, which are not the Windows ones.

`check` reads each shard's own test list (the shards must list the same
tests, or they ran different builds) and each shard's JUnit report under a
coverage-shard-<n> directory, and fails when a listed test ran in no shard,
in two, or outside the shard the plan gives it.

Refresh the data from a few green runs (the result artifacts are kept seven
days):

    gh run download <run id> --repo sathyassn/codeflow -p 'coverage-shard-*' -D runs/<run id>
    python3 -B scripts/coverage-shards.py measure runs/<oldest> ... runs/<newest>

Usage: coverage-shards.py filter <shard>
       coverage-shards.py plan
       coverage-shards.py check --list <nextest list json>... --results <dir>
       coverage-shards.py measure <junit dir>...
"""

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("windows_partitions", HERE / "windows-partitions.py")
planner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(planner)

DATA = HERE / "coverage-test-times.json"
RESULTS = "coverage-shard"
SOURCE = "on ubuntu-24.04 under cargo llvm-cov from the coverage-shard results"
REFRESH = "scripts/coverage-shards.py measure"

if __name__ == "__main__":
    sys.exit(planner.main(DATA, RESULTS, SOURCE, REFRESH, __doc__))

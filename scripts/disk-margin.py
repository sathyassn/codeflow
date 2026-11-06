#!/usr/bin/env python3
"""Record the lowest free disk of a hosted coverage shard and fail under a margin (TSK-1157).

The coverage shards build the whole instrumented workspace and write its
profiles on the runner's own disk. One shard ran out of disk in the link step
(job 125339596386, "No space left on device"). This script measures how
close each shard comes.

`watch` runs in the background from the start of the job. It samples the
free space of the file system holding `--path` every `--interval` seconds and
rewrites `--out` (JSON) after every sample, with the low point and the time of
the latest sample. The background process ends with the job.

`report` runs at the end of the job. It prints the free space now, the low
point and when it was seen, and the size of each `--size` directory. It fails
when the free space now or the recorded low point is under `--min-free-gib`,
when the watch left no record (it did not run, or it could not write), and
when the latest sample is older than STALE_INTERVALS times `--interval` (the
watch died or stopped writing). A missing or stale measurement is never a
pass, and the live sample is judged on its own, so a record that says more
than the disk has cannot pass a disk under the margin.

Usage: disk-margin.py watch --out <json> [--path <dir>] [--interval <seconds>]
       disk-margin.py report --low <json> --min-free-gib <n> [--path <dir>] [--size <dir>...] [--interval <seconds>]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

NAME = "disk-margin"
GIB = 1024 ** 3
# A record whose latest sample is older than this many intervals is stale.
STALE_INTERVALS = 3


def sample(path: Path) -> dict:
    usage = shutil.disk_usage(path)
    return {"free": usage.free, "total": usage.total, "at": time.time()}


def write_low(out: Path, low: dict) -> None:
    """Replace the record in one step, so a reader never sees half of it."""
    partial = out.with_name(out.name + ".partial")
    partial.write_text(json.dumps(low), encoding="utf-8")
    os.replace(partial, out)


def watch(out: Path, path: Path, interval: float, samples: int | None = None) -> None:
    """Keep the lowest sample in `out`. `samples` bounds the loop for tests."""
    low = None
    taken = 0
    while samples is None or taken < samples:
        now = sample(path)
        taken += 1
        if low is None or now["free"] < low["free"]:
            low = {**now, "path": str(path)}
        low["samples"] = taken
        low["latest"] = now["at"]
        write_low(out, low)
        if samples is None or taken < samples:
            time.sleep(interval)


def tree_size(root: Path) -> int:
    """Bytes allocated to a directory tree, as `du` counts them."""
    total = 0
    for directory, _, files in os.walk(root):
        for name in files:
            try:
                stat = os.lstat(os.path.join(directory, name))
            except OSError:
                continue
            total += getattr(stat, "st_blocks", 0) * 512 or stat.st_size
    return total


def gib(value: float) -> str:
    return f"{value / GIB:.1f} GiB"


def report(low_file: Path, min_free_gib: float, path: Path, sizes: list[Path],
           interval: float = 5.0) -> tuple[list[str], list[str]]:
    """(problems, lines) for the end of a job."""
    lines, problems = [], []
    margin = min_free_gib * GIB
    remedy = ("free disk before the build (unused preinstalled toolchains) or shrink the instrumented build, so "
              "the next run does not link on a disk this full")
    now = sample(path)
    lines.append(f"free now {gib(now['free'])} of {gib(now['total'])} on the file system of {path}")
    if now["free"] < margin:
        problems.append(f"the free disk now is {gib(now['free'])}, under the {min_free_gib:g} GiB margin: {remedy}")
    for directory in sizes:
        if directory.is_dir():
            lines.append(f"{directory} takes {gib(tree_size(directory))}")
        else:
            lines.append(f"{directory} does not exist")
    try:
        low = json.loads(low_file.read_text(encoding="utf-8"))
        values = [low[key] for key in ("free", "total", "at", "samples", "latest")]
        if not all(isinstance(value, (int, float)) and not isinstance(value, bool) for value in values):
            raise TypeError("a field is not a number")
        free, total, at, count, latest = (float(value) for value in values)
    except (OSError, ValueError, KeyError, TypeError) as error:
        problems.append(f"no low-point record at {low_file} ({error}): the watch did not run or could not write, "
                        "so the margin is not measured")
        return problems, lines
    share = free / total * 100 if total else 0.0
    age = now["at"] - latest
    lines.append(f"lowest free {gib(free)} ({share:.1f} percent of {gib(total)}), "
                 f"{now['at'] - at:.0f} s before this report, over {count:.0f} samples, the latest {age:.0f} s ago; "
                 f"the margin is {min_free_gib:g} GiB")
    if age > STALE_INTERVALS * interval:
        problems.append(f"the latest sample is {age:.0f} s old, more than {STALE_INTERVALS} intervals of "
                        f"{interval:g} s: the watch stopped, so the low point since then is not measured")
    if free < margin:
        problems.append(f"the lowest free disk was {gib(free)}, under the {min_free_gib:g} GiB margin: {remedy}")
    return problems, lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    keep = commands.add_parser("watch", help="keep the lowest free disk in a file until stopped")
    keep.add_argument("--out", type=Path, required=True)
    keep.add_argument("--path", type=Path, default=Path("."))
    keep.add_argument("--interval", type=float, default=5.0)
    show = commands.add_parser("report", help="print the low point and fail under the margin")
    show.add_argument("--low", type=Path, required=True)
    show.add_argument("--min-free-gib", type=float, required=True)
    show.add_argument("--path", type=Path, default=Path("."))
    show.add_argument("--size", type=Path, nargs="*", default=[])
    show.add_argument("--interval", type=float, default=5.0, help="the watch's interval, which sets when a sample is stale")
    args = parser.parse_args(argv)
    if args.command == "watch":
        if args.interval <= 0:
            raise SystemExit(f"{NAME}: --interval must be above 0")
        watch(args.out, args.path, args.interval)
        return 0
    if not args.min_free_gib > 0 or not args.interval > 0:
        raise SystemExit(f"{NAME}: --min-free-gib and --interval must be above 0")
    problems, lines = report(args.low, args.min_free_gib, args.path, args.size, args.interval)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    for line in lines:
        print(f"{NAME}: {line}")
        if summary:
            with open(summary, "a", encoding="utf-8") as handle:
                handle.write(f"- {line}\n")
    for problem in problems:
        print(f"{NAME}: FAILED {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

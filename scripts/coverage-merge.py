#!/usr/bin/env python3
"""Merge the coverage shards' line coverage and apply the floor (TSK-270).

CI runs the instrumented suite as shards, and each shard exports its own
`cargo llvm-cov report --json`. This script adds up the shards' counters and
computes the aggregate line coverage the way llvm-cov computes the number
that `cargo llvm-cov --fail-under-lines` reads, then fails under the floor
that the `rust-coverage` target of .codeflow/test-config.json names.

Summing the shards' lcov line hits would not give that number: llvm-cov
counts a generic function by its best single instantiation, while lcov
records a line as hit when any instantiation ran it. So the merge works on
the region counters of each function instantiation, which add up exactly,
and repeats llvm-cov's line count from them:

- each instantiation's regions in its own file become segments as llvm-cov's
  SegmentBuilder makes them, and each line is mapped and covered as its
  LineCoverageStats says;
- the instantiations of one function (the same file and start) form a group,
  which counts the most covered lines and mapped lines of any one of them;
- a file counts its groups, and the total counts the files of the report.

Before it trusts the merge, the script recomputes every shard's own numbers
from that shard's counters and compares them, file by file, with what
llvm-cov reported for it. A difference means this computation and llvm-cov
disagree, and the run fails instead of judging the floor on a number the
local gate would not produce. It also fails when a shard is missing or empty,
or when the shards were built from different code.

Usage: coverage-merge.py --floor-from <test-config.json> --shards <n> <directory>
       coverage-merge.py --floor <percent> --shards <n> <directory>

The directory holds coverage-data-<n>/coverage.json, each shard's
`cargo llvm-cov report --json`, for every shard from 1 to n.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".codeflow" / "test-config.json"
FLOOR = re.compile(r"--fail-under-lines (\d+(?:\.\d+)?)(?:\s|$)")

# llvm::coverage::CounterMappingRegion::RegionKind
CODE, EXPANSION, SKIPPED, GAP = 0, 1, 2, 3


class Problem(Exception):
    """A reason the merged number cannot be trusted or does not pass."""


def floor_from(config: Path) -> float:
    """The `--fail-under-lines` value of the rust-coverage target's full command."""
    targets = {t.get("name"): t for t in json.loads(config.read_text()).get("targets", [])}
    command = targets.get("rust-coverage", {}).get("modes", {}).get("full", {}).get("command", "")
    found = FLOOR.search(command)
    if not found:
        raise Problem(f"{config} has no rust-coverage full command with --fail-under-lines <N>")
    return float(found.group(1))


# A region is (line start, column start, line end, column end, count, file
# id, expanded file id, kind), the order llvm-cov's JSON export writes.
def start(region):
    return (region[0], region[1])


def end(region):
    return (region[2], region[3])


class Segment:
    __slots__ = ("line", "col", "count", "has_count", "entry", "gap")

    def __init__(self, line, col, count, has_count, entry, gap):
        self.line, self.col, self.count = line, col, count
        self.has_count, self.entry, self.gap = has_count, entry, gap


def combine(regions: list[list]) -> list[list]:
    """Sort nested regions and fold those covering the same area, as
    SegmentBuilder::sortNestedRegions and combineRegions do."""
    regions = sorted(regions, key=lambda r: (start(r), (-r[2], -r[3]), r[7]))
    out: list[list] = []
    for region in regions:
        if out and start(out[-1]) == start(region) and end(out[-1]) == end(region):
            if out[-1][7] == region[7]:
                out[-1][4] += region[4]
            continue
        out.append(list(region))
    return out


def segments(regions: list[list]) -> list[Segment]:
    """SegmentBuilder::buildSegments for one function's regions in its file."""
    built: list[Segment] = []
    active: list[list] = []

    def emit(region, loc, entry, skipped=False):
        has_count = not skipped and region[7] != SKIPPED
        if built and not entry and not skipped:
            last = built[-1]
            if last.has_count == has_count and last.count == region[4] and not last.entry:
                return
        if has_count:
            built.append(Segment(loc[0], loc[1], region[4], True, entry, region[7] == GAP))
        else:
            built.append(Segment(loc[0], loc[1], 0, False, entry, False))

    def complete(loc, first):
        done = sorted(active[first:], key=end)
        active[first:] = done
        for i in range(first + 1, len(active)):
            region = active[i]
            at = end(active[i - 1])
            if loc is not None and at == loc:
                break
            if at == end(region):
                continue
            for j in range(i + 1, len(active)):
                if end(region) == end(active[j]):
                    region = active[j]
            emit(region, at, False)
        last = active[-1]
        if first and end(last) != loc:
            emit(active[first - 1], end(last), False)
        elif not first and (loc is None or loc != end(last)):
            emit(last, end(last), False, skipped=True)
        del active[first:]

    combined = combine(regions)
    for index, region in enumerate(combined):
        here = start(region)
        staying = [r for r in active if not end(r) <= here]
        if len(staying) != len(active):
            first = len(staying)
            active[:] = staying + [r for r in active if end(r) <= here]
            complete(here, first)
        gap = region[7] == GAP
        if here == end(region):
            skipped = index + 1 == len(combined) or region[7] == SKIPPED
            emit(active[-1] if active else region, here, not gap, skipped)
            if skipped and active:
                emit(active[-1], here, False)
            continue
        if index + 1 == len(combined) or here != start(combined[index + 1]):
            emit(region, here, not gap)
        active.append(region)
    if active:
        complete(None, 0)
    return built


def line_counts(built: list[Segment]) -> tuple[int, int]:
    """(mapped lines, covered lines) as llvm-cov's LineCoverageIterator and
    LineCoverageStats count them."""
    if not built:
        return 0, 0
    mapped = covered = 0
    wrapped = None
    index = 0
    line = built[0].line
    while index < len(built):
        here = []
        while index < len(built) and built[index].line == line:
            here.append(built[index])
            index += 1
        starts = [s for s in here if not s.gap and s.has_count and s.entry]
        skipped_start = bool(here) and not here[0].has_count and here[0].entry
        is_mapped = not skipped_start and ((wrapped is not None and wrapped.has_count) or bool(starts))
        is_mapped = is_mapped or any(s.entry and s.has_count for s in here)
        if is_mapped:
            count = wrapped.count if wrapped is not None else 0
            if starts:
                count = max([count] + [s.count for s in starts])
            mapped += 1
            covered += 1 if count else 0
        if here:
            wrapped = here[-1]
        line += 1
    return mapped, covered


def main_file(function: dict) -> int | None:
    """findMainViewFileID: the first file no expansion region expands into."""
    expanded = {r[6] for r in function["regions"] if r[7] == EXPANSION}
    for index in range(len(function["filenames"])):
        if index not in expanded:
            return index
    return None


def file_lines(functions: list[dict], counts: list[list[int]]) -> dict[str, tuple[int, int]]:
    """(mapped, covered) lines per file, each function group counted by its
    best instantiation, as llvm-cov's file summaries count them."""
    groups: dict[tuple, list[tuple[int, int]]] = {}
    for function, regions_counts in zip(functions, counts):
        main = main_file(function)
        if main is None:
            continue
        regions = [[*r[:4], c, *r[5:]] for r, c in zip(function["regions"], regions_counts)]
        own = [r for r in regions if r[5] == main]
        if not own:
            continue
        key = (function["filenames"][main], start(own[0]))
        groups.setdefault(key, []).append(line_counts(segments(own)))
    files: dict[str, list[int]] = {}
    for (name, _), summaries in groups.items():
        total = files.setdefault(name, [0, 0])
        total[0] += max(s[0] for s in summaries)
        total[1] += max(s[1] for s in summaries)
    return {name: (lines, hit) for name, (lines, hit) in files.items()}


def load(path: Path) -> dict:
    try:
        data = json.loads(path.read_text())["data"][0]
    except (OSError, ValueError, KeyError, IndexError, TypeError) as error:
        raise Problem(f"{path}: not a cargo llvm-cov JSON report ({error})") from error
    if not isinstance(data.get("functions"), list) or not data.get("files"):
        raise Problem(f"{path}: the report has no files or no per-function data; export it with "
                      "`cargo llvm-cov report --json`, not --summary-only")
    return data


def identity(function: dict) -> tuple:
    """A function instantiation and the regions it maps, without counts."""
    return (function["name"], tuple(function["filenames"]),
            tuple((*r[:4], *r[5:]) for r in function["regions"]))


def keyed(data: dict) -> dict[tuple, list[dict]]:
    out: dict[tuple, list[dict]] = {}
    for function in data["functions"]:
        out.setdefault(identity(function), []).append(function)
    return out


def reported(data: dict) -> dict[str, tuple[int, int]]:
    return {f["filename"]: (f["summary"]["lines"]["count"], f["summary"]["lines"]["covered"]) for f in data["files"]}


def totals(per_file: dict[str, tuple[int, int]], files: list[str]) -> tuple[int, int]:
    return (sum(per_file.get(name, (0, 0))[0] for name in files),
            sum(per_file.get(name, (0, 0))[1] for name in files))


def shard_reports(directory: Path, shards: int) -> list[Path]:
    """`coverage-data-<n>/coverage.json` for every shard from 1 to `shards`."""
    found = {}
    for path in sorted(directory.glob("coverage-data-*/coverage.json")):
        number = re.fullmatch(r"coverage-data-(\d+)", path.parent.name)
        if number is None:
            raise Problem(f"{path.parent} is not named coverage-data-<n>")
        found[int(number.group(1))] = path
    missing = sorted(set(range(1, shards + 1)) - set(found))
    if missing:
        raise Problem(f"no coverage report from shard(s) {missing} under {directory}")
    extra = sorted(set(found) - set(range(1, shards + 1)))
    if extra:
        raise Problem(f"coverage reports from shard(s) {extra}, outside 1 to {shards}")
    return [found[n] for n in range(1, shards + 1)]


def merge(paths: list[Path]) -> tuple[int, int, list[str]]:
    """(mapped lines, covered lines, notes) of the shards together."""
    if len(paths) < 2:
        raise Problem(f"expected the report of every shard, got {len(paths)}")
    shards = [load(path) for path in paths]
    files = sorted(reported(shards[0]))
    functions = [keyed(data) for data in shards]
    shape = {key: len(group) for key, group in functions[0].items()}
    notes = []
    for path, data, mine_keyed in zip(paths, shards, functions):
        if sorted(reported(data)) != files or {k: len(g) for k, g in mine_keyed.items()} != shape:
            raise Problem(f"{path}: the shard reports other files or functions than {paths[0]}; "
                          "the shards were not built from the same code")
        counts = [[r[4] for r in f["regions"]] for f in data["functions"]]
        mine = file_lines(data["functions"], counts)
        theirs = reported(data)
        wrong = [name for name in files if mine.get(name, (0, 0)) != theirs[name]]
        if wrong:
            shown = ", ".join(f"{n} (llvm-cov {theirs[n]}, here {mine.get(n, (0, 0))})" for n in wrong[:5])
            raise Problem(f"{path}: this merge counts {len(wrong)} file(s) differently from llvm-cov, so its "
                          f"number would not be the local gate's: {shown}")
        lines, hit = totals(mine, files)
        notes.append(f"{path}: {hit} of {lines} lines, as llvm-cov counts this shard alone")
    merged, summed = [], []
    for key, group in functions[0].items():
        for index, function in enumerate(group):
            merged.append(function)
            summed.append([sum(values) for values in zip(*(
                [r[4] for r in shard[key][index]["regions"]] for shard in functions))])
    lines, hit = totals(file_lines(merged, summed), files)
    if not lines:
        raise Problem("the merged report maps no lines")
    return lines, hit, notes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    which = parser.add_mutually_exclusive_group(required=True)
    which.add_argument("--floor-from", type=Path, help="test config whose rust-coverage command names the floor")
    which.add_argument("--floor", type=float, help="the floor in percent")
    parser.add_argument("--shards", type=int, required=True, help="how many shards ran")
    parser.add_argument("directory", type=Path, help="holds coverage-data-<n>/coverage.json for each shard")
    args = parser.parse_args()
    try:
        floor = args.floor if args.floor is not None else floor_from(args.floor_from)
        lines, hit, notes = merge(shard_reports(args.directory, args.shards))
    except Problem as problem:
        print(f"coverage-merge: FAILED {problem}")
        return 1
    for note in notes:
        print(f"coverage-merge: {note}")
    percent = hit * 100.0 / lines
    print(f"coverage-merge: the {args.shards} shards together cover {hit} of {lines} lines, "
          f"{percent:.2f} percent; the floor is {floor:g} percent")
    if percent < floor:
        print(f"coverage-merge: FAILED line coverage {percent:.2f} percent is under the {floor:g} percent floor")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

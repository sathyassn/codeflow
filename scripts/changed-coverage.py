#!/usr/bin/env python3
"""Report changed executable lines with an explicit base; fail below 80%."""

from __future__ import annotations

import argparse
import ast
import hashlib
import importlib.util
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from coverage_exemptions import Problem, exemptions, path_name

FLOOR, TARGET = 80, 90
REVISION = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
HUNK = re.compile(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:.*)\Z")
FILES = {"llvm": "coverage.json", "lcov": "coverage.info", "cobertura": "coverage.xml"}


def read(path, limit=128 * 1024 * 1024):
    if path.is_symlink() or path.parent.is_symlink() or not path.is_file():
        raise Problem(f"{path}: expected a regular report file")
    if path.stat().st_size > limit:
        raise Problem(f"{path}: input exceeds {limit} bytes")
    return path.read_bytes()


def revision(value, label):
    if not isinstance(value, str) or not REVISION.fullmatch(value) or set(value) == {"0"}:
        raise Problem(f"{label}: no resolvable comparison revision; supply the caller's resolved commit")
    return value


def git_path(value, prefixed=True):
    if value.startswith('"'):
        # Git quotes non-ASCII bytes with octal escapes when quotePath is on.
        quoted = "".join(chr(byte) if byte < 128 else f"\\{byte:03o}" for byte in value.encode("utf-8"))
        value = ast.literal_eval("b" + quoted).decode("utf-8")
    if value == "/dev/null":
        return None
    if not prefixed:
        return path_name(value)
    if not value.startswith(("a/", "b/")):
        raise Problem(f"diff path lacks the expected a/ or b/ prefix: {value!r}")
    return path_name(value[2:])


def changed_lines(diff):
    """Parse a zero-context Git diff; verify hunk lengths before using them."""
    result = {}
    current = None
    old_left = new_left = 0
    line_number = 0
    in_hunk = False
    for row in diff.splitlines():
        if row.startswith("\\ No newline at end of file"):
            continue
        if in_hunk and (old_left or new_left):
            if row.startswith("+") and new_left:
                result.setdefault(current, set()).add(line_number)
                new_left -= 1
                line_number += 1
            elif row.startswith("-") and old_left:
                old_left -= 1
            else:
                raise Problem("diff is not a complete zero-context hunk")
            continue
        in_hunk = False
        if row.startswith("diff --git "):
            current = None
        elif row.startswith("+++ "):
            current = git_path(row[4:])
        elif row.startswith("rename to "):
            result.setdefault(git_path(row[10:], prefixed=False), set())
        elif row.startswith("@@"):
            match = HUNK.fullmatch(row)
            if match is None:
                raise Problem("malformed changed-line diff hunk")
            old_start, old_count, new_start, new_count = match.groups()
            old_left = int(old_count) if old_count is not None else 1
            new_left = int(new_count) if new_count is not None else 1
            line_number = int(new_start)
            if new_left and (current is None or line_number == 0):
                raise Problem("diff adds lines without a valid head path")
            if current is not None:
                result.setdefault(current, set())
            in_hunk = True
        elif row.startswith(("+", "-")) and not row.startswith("--- "):
            raise Problem("diff line appears outside its hunk")
    if old_left or new_left:
        raise Problem("diff ends before its hunk is complete")
    return result


def line_map(content, kind, root):
    """Return normalized executable line numbers and whether any hit them."""
    result = {}

    def add(name, number, hits):
        if type(number) is not int or type(hits) is not int or number <= 0 or hits < 0:
            raise Problem("coverage line numbers must be positive and hit counts nonnegative")
        lines = result.setdefault(path_name(name, root), {})
        lines[number] = lines.get(number, False) or hits > 0

    if kind == "lcov":
        name = None
        for row in content.decode("utf-8").splitlines():
            if row.startswith("SF:"):
                name = row[3:]
                result.setdefault(path_name(name, root), {})
            elif row.startswith("DA:"):
                fields = row[3:].split(",")
                if name is None or len(fields) not in (2, 3):
                    raise Problem("malformed LCOV DA record")
                add(name, int(fields[0]), int(fields[1]))
            elif row == "end_of_record":
                name = None
    elif kind == "cobertura":
        if b"<!DOCTYPE" in content.upper() or b"<!ENTITY" in content.upper():
            raise Problem("Cobertura DTD and entity declarations are refused")
        tree = ET.fromstring(content)
        if tree.tag != "coverage":
            raise Problem("Cobertura root must be coverage")
        for node in tree.iter("class"):
            name = node.attrib["filename"]
            result.setdefault(path_name(name, root), {})
            for line in node.findall("./lines/line"):
                add(name, int(line.attrib["number"]), int(line.attrib["hits"]))
    elif kind == "llvm":
        spec = importlib.util.spec_from_file_location("coverage_merge", Path(__file__).with_name("coverage-merge.py"))
        merger = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(merger)
        data = json.loads(content)["data"]
        if not isinstance(data, list) or len(data) != 1:
            raise Problem("expected one llvm-cov export data section")
        section = data[0]
        allowed = {file["filename"] for file in section["files"]}
        for name in allowed:
            result.setdefault(path_name(name, root), {})
        # Function counters preserve each executable line, including branches
        # and generic instantiations; the aggregate merger remains independent.
        work = 0
        for function in section["functions"]:
            for region in function["regions"]:
                if (not isinstance(region, list) or len(region) < 8
                        or any(type(value) is not int or value < 0 for value in region)
                        or region[0] < 1 or region[2] < region[0]
                        or region[2] - region[0] > 1000000):
                    raise Problem("invalid or oversized llvm-cov region")
                work += region[2] - region[0] + 1
                if work > 10000000:
                    raise Problem("llvm-cov region expansion exceeds the line budget")
            main = merger.main_file(function)
            if main is None:
                continue
            name = function["filenames"][main]
            if name not in allowed:
                continue
            regions = [r for r in function["regions"] if r[5] == main]
            built = merger.segments(regions)
            wrapped = None
            index = 0
            number = built[0].line if built else 0
            while index < len(built):
                here = []
                while index < len(built) and built[index].line == number:
                    here.append(built[index])
                    index += 1
                starts = [s for s in here if not s.gap and s.has_count and s.entry]
                skipped = bool(here) and not here[0].has_count and here[0].entry
                mapped = not skipped and ((wrapped is not None and wrapped.has_count) or bool(starts))
                mapped = mapped or any(s.entry and s.has_count for s in here)
                if mapped:
                    add(name, number, max([wrapped.count if wrapped is not None else 0] + [s.count for s in starts]))
                if here:
                    wrapped = here[-1]
                number += 1
    else:
        raise Problem(f"unsupported coverage format: {kind}")
    if not result:
        raise Problem(f"{kind} report has no per-line coverage files")
    return result


def configured_roster(config, plan):
    targets = json.loads(read(config))["targets"]
    coverage = [t for t in targets if t.get("name") == "rust-coverage"
                and t.get("enabled", True) and not t.get("ci_skip")
                and t.get("modes", {}).get("full", {}).get("command")]
    if len(coverage) != 1:
        raise Problem("configured coverage targets must include exactly one enabled rust-coverage")
    count = json.loads(read(plan))["partitions"]
    if type(count) is not int or not 1 <= count <= 128:
        raise Problem("coverage shard plan requires 1 to 128 partitions")
    return {("rust-coverage", number): count for number in range(1, count + 1)}


def shard_coverage(directory, roster, head, run, attempt):
    seen = set()
    merged = {}
    records = sorted(directory.glob("coverage-data-*/record.json"))
    for path in records:
        record = json.loads(read(path, 65536))
        if not isinstance(record, dict) or any(type(record.get(field)) is not int for field in ("version", "shard", "shards")):
            raise Problem(f"{path}: invalid shard record field types")
        for field, expected in (("version", 1), ("revision", head), ("run", run), ("attempt", attempt)):
            if record.get(field) != expected:
                raise Problem(f"{path}: stale or invalid shard {field}; expected {expected}")
        if record.get("status") != "success":
            raise Problem(f"{path}: failed shard status {record.get('status')!r}")
        key = (record["target"], record["shard"])
        if key in seen:
            raise Problem(f"{path}: duplicate shard {key}")
        if key not in roster or record["shards"] != roster[key]:
            raise Problem(f"{path}: shard roster does not match configured targets: {key}")
        seen.add(key)
        kind = record["format"]
        if kind not in FILES:
            raise Problem(f"{path}: unsupported coverage format {kind!r}")
        report = read(path.parent / FILES[kind])
        if hashlib.sha256(report).hexdigest() != record.get("sha256"):
            raise Problem(f"{path}: coverage report digest mismatch")
        source_root = record["source_root"]
        if not isinstance(source_root, str) or not source_root:
            raise Problem(f"{path}: missing source root")
        for name, lines in line_map(report, kind, source_root).items():
            destination = merged.setdefault(name, {})
            for number, hit in lines.items():
                destination[number] = destination.get(number, False) or hit
    missing = sorted(set(roster) - seen)
    if missing:
        raise Problem(f"missing coverage shard(s): {missing}")
    # An artifact without a record cannot silently disappear from the roster.
    folders = {p.parent for p in records}
    if any(p not in folders for p in directory.glob("coverage-data-*")):
        raise Problem("coverage shard artifact has no binding record")
    return merged


def report(event, base, head, diff, inventory, coverage, excluded, suffixes):
    heading = f"Changed-line coverage; floor {FLOOR}%; target {TARGET}%"
    if not base and event == "local":
        return 0, heading + "\nunavailable: no comparison base; nothing enforced\n"
    revision(base, "base")
    revision(head, "head")
    changed = changed_lines(diff)
    paths = {path_name(name) for name in inventory}
    if set(changed) - paths:
        raise Problem("diff paths do not match the comparison range inventory")
    code = {name for name in paths if name.endswith(tuple(suffixes)) and name not in excluded}
    added = {name: numbers for name, numbers in changed.items() if name in code and numbers}
    rows = [heading, f"Base {base}; head {head}"]
    for name in sorted(paths & set(excluded)):
        entry = excluded[name]
        rows.append(f"Exempt {name!r}; owner {entry['owner']}; {entry['reason']}; remove when {entry['remove_when']}")
    # A diff header proves a deletion-only edit or pure rename exists.
    # An empty/truncated diff cannot explain a code path in the inventory.
    if code - set(changed):
        raise Problem("empty changed set for a range that changes code")
    measured = hit = 0
    for name, numbers in sorted(added.items()):
        if name not in coverage:
            raise Problem(f"{name!r}: changed code has no coverage file")
        mapped = sorted(numbers & set(coverage[name]))
        if not mapped:
            raise Problem(f"{name!r}: empty changed set of executable lines for a range that changes code")
        covered = sum(coverage[name][number] for number in mapped)
        measured += len(mapped)
        hit += covered
        rows.append(f"{name!r}: {covered}/{len(mapped)} changed executable lines ({covered * 100 / len(mapped):.2f}%)")
        for number in mapped:
            rows.append(f"  {name!r}:{number}: {'covered' if coverage[name][number] else 'uncovered'}")
    if not measured:
        rows.append("No added executable code lines; nothing enforced")
        return 0, "\n".join(rows) + "\n"
    percent = hit * 100 / measured
    rows.append(f"Changed lines: {hit}/{measured} ({percent:.2f}%); floor {FLOOR}%; target {TARGET}%")
    if percent < FLOOR:
        rows.append("FAILED changed-line coverage is under the 80% floor")
    return int(percent < FLOOR), "\n".join(rows) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    bind = sub.add_parser("bind", help="bind one successful shard's report bytes")
    bind.add_argument("--directory", type=Path, required=True)
    bind.add_argument("--format", choices=FILES, default="llvm")
    bind.add_argument("--source-root", required=True)
    bind.add_argument("--target", default="rust-coverage")
    bind.add_argument("--shard", type=int, required=True)
    bind.add_argument("--shards", type=int, required=True)
    check = sub.add_parser("report", help="read bound shards and the caller's explicit comparison")
    check.add_argument("--event", choices=("pull_request", "push", "local", "schedule", "workflow_dispatch"), required=True)
    check.add_argument("--base")
    check.add_argument("--diff", type=Path)
    check.add_argument("--paths", type=Path, help="NUL-separated added/modified head paths")
    check.add_argument("--directory", type=Path)
    check.add_argument("--config", type=Path, default=Path(".codeflow/test-config.json"))
    check.add_argument("--plan", type=Path, default=Path("scripts/coverage-test-times.json"))
    check.add_argument("--exemptions", type=Path, default=Path(".codeflow/coverage-exemptions.json"))
    check.add_argument("--code-suffix", action="append", default=None)
    check.add_argument("--output", type=Path)
    for command in (bind, check):
        command.add_argument("--head", required=command is bind)
        command.add_argument("--run", required=command is bind)
        command.add_argument("--attempt", required=command is bind)
    args = parser.parse_args()
    try:
        local_without_base = args.action == "report" and args.event == "local" and not args.base
        if args.action == "report" and not local_without_base:
            revision(args.base, "base")
        if not local_without_base:
            revision(args.head, "head")
            if (not isinstance(args.run, str) or not isinstance(args.attempt, str)
                    or not args.run.isdecimal() or not args.attempt.isdecimal() or int(args.attempt) < 1):
                raise Problem("run and attempt must be decimal identifiers, attempt at least 1")
        if args.action == "bind":
            if not 1 <= args.shard <= args.shards <= 128:
                raise Problem("shard must be within the configured 1 to shards range")
            payload = read(args.directory / FILES[args.format])
            record = {"version": 1, "revision": args.head, "run": args.run, "attempt": args.attempt,
                      "target": args.target, "shard": args.shard, "shards": args.shards, "status": "success",
                      "format": args.format, "source_root": args.source_root,
                      "sha256": hashlib.sha256(payload).hexdigest()}
            with (args.directory / "record.json").open("x", encoding="utf-8") as output:
                output.write(json.dumps(record, indent=2) + "\n")
            return 0
        excluded = exemptions(args.exemptions)
        if local_without_base:
            status, text = report(args.event, None, args.head, "", [], {}, excluded, [".rs"])
        else:
            if not args.diff or not args.paths or not args.directory:
                raise Problem("resolved comparison needs --diff, --paths and --directory")
            coverage = shard_coverage(args.directory, configured_roster(args.config, args.plan),
                                      args.head, args.run, args.attempt)
            raw = read(args.paths)
            if raw and not raw.endswith(b"\x00"):
                raise Problem("changed-path inventory must end in NUL")
            paths = raw.decode("utf-8").split("\x00")[:-1] if raw else []
            status, text = report(args.event, args.base, args.head, read(args.diff).decode("utf-8"),
                                  paths, coverage, excluded, args.code_suffix or [".rs"])
    except (Problem, OSError, ValueError, KeyError, IndexError, TypeError, SyntaxError, ET.ParseError) as error:
        status, text = 1, f"Changed-line coverage; floor {FLOOR}%; target {TARGET}%\nFAILED {error}\n"
    print(text, end="")
    if getattr(args, "output", None):
        try:
            with args.output.open("x", encoding="utf-8") as output:
                output.write(text)
        except OSError as error:
            print(f"FAILED cannot create report output: {error}")
            return 1
    return status


if __name__ == "__main__":
    sys.exit(main())

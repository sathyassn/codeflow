#!/usr/bin/env python3
"""Discover daily Windows sources and report only the commit actually tested.

The scheduler runs this script from its own revision. Test jobs check out
each discovered SHA separately; no daily result supplies PR merge evidence.

Which lines: main and each active integration line. The refs are listed from the
remote on every run and nothing stores them, so a new line joins and a deleted
one leaves on the next run. A line is active when its own tree at discovery has
what the runner calls: every script named in windows-daily-run.yml, each with the
subcommands and flags the runner passes it, and the partition measurements. The
test reads those files and runs nothing from the line. A line that lacks any of
them is not proven active: it stays out and the run's summary names it and the
reason. main is always selected. A selected line that fails fails visibly. If
the listing or a read fails, the run fails and offers no lines. Other branches
are not covered.

What a run skips: no Windows job, and no selected line for its age or an unchanged
tip. It runs Windows only: Linux gates and coverage are not part of it. A commit
that was never a line's tip at discovery is not tested, and a later tip gets no
credit.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


WINDOWS_JOBS = frozenset({
    "windows-build", "windows-tests", "windows", "windows-release-journey",
    "windows-journeys",
})
SHA = re.compile(r"[0-9a-f]{40}")
REF = re.compile(r"refs/heads/(?:main|integration/[A-Za-z0-9_./-]+)")
MAIN = "refs/heads/main"
RUNNER = Path(__file__).resolve().parents[1] / ".github/workflows/windows-daily-run.yml"
# The runner's own reporting script comes from the scheduler's revision, not the line.
RUNNER_OWN = "scripts/windows-daily.py"
# What the partition script reads, which no command names.
RUNNER_DATA = "scripts/windows-test-times.json"
COMMAND = re.compile(r"\bpython3? (?:-\w+ )*(scripts/[\w.-]+\.py)([^&\n\"]*)")


def identity(ref: str, sha: str) -> None:
    if not SHA.fullmatch(sha) or sha == "0" * 40:
        raise ValueError("the tested SHA must be a full, nonzero Git commit id")
    if (not REF.fullmatch(ref) or ".." in ref or "//" in ref
            or ref.endswith(("/", ".", ".lock"))
            or any(part.startswith(".") for part in ref.split("/"))):
        raise ValueError(f"unsupported daily source ref: {ref!r}")


def sources(remote_refs: str) -> dict:
    """List main and every existing integration ref, before the active test."""
    found: dict[str, str] = {}
    for line in remote_refs.splitlines():
        fields = line.split()
        if len(fields) != 2:
            raise ValueError("remote ref listing contains a malformed row")
        sha, ref = fields
        if ref != "refs/heads/main" and not ref.startswith("refs/heads/integration/"):
            continue
        identity(ref, sha)
        if ref in found:
            raise ValueError(f"remote ref listing repeats {ref}")
        found[ref] = sha
    if "refs/heads/main" not in found:
        raise ValueError("remote ref listing is missing refs/heads/main")
    # GitHub expands at most 256 jobs from one matrix; never truncate lines.
    if len(found) > 256:
        raise ValueError("daily sources exceed GitHub's 256-job matrix limit")
    return {"include": [
        {"ref": ref, "sha": sha,
         "key": hashlib.sha256(f"{ref}\n{sha}".encode()).hexdigest()}
        for ref, sha in sorted(found.items())
    ]}


def runner_needs(workflow: str) -> dict[str, set[str]]:
    """Each script the runner calls in the line's tree, with what it passes it."""
    needs: dict[str, set[str]] = {RUNNER_DATA: set()}
    for script, arguments in COMMAND.findall(workflow):
        if script == RUNNER_OWN:
            continue
        words = arguments.split()
        wanted = needs.setdefault(script, set())
        if words and not words[0].startswith(("-", "$")):
            wanted.add(words[0])
        wanted.update(word for word in words if word.startswith("--"))
    if len(needs) == 1:
        raise ValueError("the runner workflow names no script, so no line can be proven active")
    return needs


def missing_from_line(needs: dict[str, set[str]], read) -> list[str]:
    """What the runner needs and this line's tree lacks. `read` gives a file's text,
    None when the tree has no such file, and raises when it cannot tell."""
    missing = []
    for path in sorted(needs):
        text = read(path)
        if text is None:
            missing.append(f"no {path}")
            continue
        for word in sorted(needs[path]):
            if word.startswith("--"):
                found = re.search(rf"""["']{re.escape(word)}["']""", text)
            else:
                found = re.search(rf"""add_parser\(\s*["']{re.escape(word)}["']""", text)
            if not found:
                missing.append(f"{path} has no {word}")
    return missing


def select_active(listed: dict, needs: dict[str, set[str]], read_at) -> tuple[dict, list[dict]]:
    """Keep main and every line whose tree has what the runner needs. A read that
    raises stops discovery: an unread line is neither selected nor called inactive."""
    selected, left_out = [], []
    for row in listed["include"]:
        missing = [] if row["ref"] == MAIN else missing_from_line(
            needs, lambda path, sha=row["sha"]: read_at(sha, path))
        if missing:
            left_out.append({"ref": row["ref"], "sha": row["sha"], "missing": missing})
        else:
            selected.append(row)
    return {"include": selected}, left_out


def tree_reader():
    """Read files of a commit fetched from origin; a failed git command raises."""
    fetched: set[str] = set()

    def read(sha: str, path: str) -> str | None:
        if sha not in fetched:
            subprocess.check_output(["git", "fetch", "-q", "--no-tags", "--depth=1", "origin", sha],
                                    text=True, timeout=120)
            fetched.add(sha)
        entry = subprocess.check_output(["git", "ls-tree", sha, "--", path], text=True, timeout=60)
        if not entry.startswith("100"):
            return None
        return subprocess.check_output(["git", "show", f"{sha}:{path}"], text=True, timeout=60)

    return read


def checked_out(ref: str, expected: str, actual: str) -> str:
    identity(ref, expected)
    if actual != expected:
        raise ValueError(f"{ref}: checkout is {actual}, expected {expected}; no tests were credited")
    return actual


def verdict(ref: str, sha: str, needs: dict) -> dict:
    identity(ref, sha)
    if not isinstance(needs, dict) or any(
        not isinstance(job, dict) or not isinstance(job.get("outputs", {}), dict)
        for job in needs.values()
    ):
        raise ValueError("daily job results must be an object of job records with outputs")
    failures = []
    if set(needs) != WINDOWS_JOBS:
        failures.append("the daily verdict did not receive exactly the five required Windows jobs")
    for name in sorted(WINDOWS_JOBS):
        job = needs.get(name, {})
        if job.get("result") != "success":
            failures.append(f"{name} concluded {job.get('result', 'missing')}; success is required")
        if job.get("outputs", {}).get("sha") != sha:
            failures.append(f"{name} did not report the requested tested SHA {sha}")
    exact = set(needs) == WINDOWS_JOBS and all(
        job.get("outputs", {}).get("sha") == sha for job in needs.values()
    )
    return {"ref": ref, "requested_sha": sha, "tested_sha": sha if exact else None,
            "result": "failure" if failures else "success",
            "jobs": needs, "failures": failures}


def summary(text: str) -> None:
    print(text)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(path).open("a", encoding="utf-8") as handle:
            handle.write(text + "\n")


def output(name: str, value: str) -> None:
    if path := os.environ.get("GITHUB_OUTPUT"):
        with Path(path).open("a", encoding="utf-8") as handle:
            handle.write(f"{name}={value}\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("discover", help="resolve main and integration refs from origin")
    for command in ("identity", "verdict"):
        sub = commands.add_parser(command)
        sub.add_argument("--ref", required=True)
        sub.add_argument("--sha", required=True)
    args = parser.parse_args()
    try:
        if args.command == "discover":
            refs = subprocess.check_output(
                ["git", "ls-remote", "--heads", "origin", "main", "integration/*"],
                text=True, timeout=60,
            )
            needs = runner_needs(RUNNER.read_text(encoding="utf-8"))
            matrix, left_out = select_active(sources(refs), needs, tree_reader())
            report = "## Daily Windows source commits\n\n" + "\n".join(
                f"- `{row['ref']}` at `{row['sha']}`" for row in matrix["include"]
            )
            if left_out:
                report += "\n\n## Left out: not proven active\n\n" + "\n".join(
                    f"- `{row['ref']}` at `{row['sha']}`: {'; '.join(row['missing'])}" for row in left_out
                )
            output("matrix", json.dumps(matrix, separators=(",", ":")))
            summary(report)
        elif args.command == "identity":
            actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
            output("sha", checked_out(args.ref, args.sha, actual))
            summary(f"Windows source: `{args.ref}` at `{actual}`. This result covers only this commit.")
        else:
            needs = json.loads(os.environ["DAILY_WINDOWS_NEEDS"])
            result = verdict(args.ref, args.sha, needs)
            Path("daily-windows-result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
            summary(f"## Daily Windows: {result['result']}\n\n"
                    f"Source: `{args.ref}`\n\nRequested SHA: `{args.sha}`\n\n"
                    f"Tested SHA reported by all jobs: `{result['tested_sha'] or 'not established'}`\n\n"
                    "This result gives no evidence for a newer branch tip or a pull request.\n\n"
                    + "\n".join(f"- {failure}" for failure in result["failures"]))
            return int(bool(result["failures"]))
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        print(f"windows-daily: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

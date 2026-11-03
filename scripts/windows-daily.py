#!/usr/bin/env python3
"""Discover daily Windows sources and report only the commit actually tested.

The scheduler runs this script from its own revision. Test jobs check out
each discovered SHA separately; no daily result supplies PR merge evidence.
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


def identity(ref: str, sha: str) -> None:
    if not SHA.fullmatch(sha) or sha == "0" * 40:
        raise ValueError("the tested SHA must be a full, nonzero Git commit id")
    if (not REF.fullmatch(ref) or ".." in ref or "//" in ref
            or ref.endswith(("/", ".", ".lock"))
            or any(part.startswith(".") for part in ref.split("/"))):
        raise ValueError(f"unsupported daily source ref: {ref!r}")


def sources(remote_refs: str) -> dict:
    """Keep main and every existing integration ref, including planning lines."""
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
            matrix = sources(refs)
            output("matrix", json.dumps(matrix, separators=(",", ":")))
            summary("## Daily Windows source commits\n\n" + "\n".join(
                f"- `{row['ref']}` at `{row['sha']}`" for row in matrix["include"]
            ))
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

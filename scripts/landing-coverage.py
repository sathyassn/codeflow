#!/usr/bin/env python3
"""Check a landing candidate's full-gate record and print its coverage (TSK-369).

A landing candidate's pull request cites one full gate run. This script reads
that run's `run.json` and accepts it only as evidence for the exact
candidate commit:

- the record exists and is a full-mode run that passed, with tracked files
  unchanged during the run and a clean worktree;
- its `revision` is the candidate commit and its `git_tree` is that commit's
  tree;
- its rust-coverage entry carries line, function and region totals and a
  line floor, the floor equals the global rule in the candidate's own
  `.codeflow/test-config.json`, and the line total meets it;
- every full-mode target the candidate's config enables ran and passed,
  unless it is named with `--hosted`.

A target named with `--hosted TARGET=RUN_ID` did not run locally and is
credited to a hosted job only when that GitHub Actions run is complete, its
head commit has the candidate's tree, and the one job pinned for the target
below completed with success, in the workflow part whose `only:` list names
the target at that commit. A failed, missing, stale (an earlier tree),
wrong-tree or wrongly scoped (another job or a partial shard) result is
refused. Such a run is reported as not complete locally, never as complete
local evidence.

Anything the script cannot confirm fails: it exits 1 with the reasons and
prints nothing citable.

Usage: landing-coverage.py --candidate <commit> --run <run.json>
                           [--hosted TARGET=RUN_ID ...] [--hosted-evidence DIR]
                           [--repo DIR] [--github OWNER/REPO]

Without --hosted-evidence the hosted run and its jobs are read with
`gh api`; with it, DIR holds run-<id>.json and jobs-<id>.json, the same API
responses saved earlier.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COVERAGE_TARGET = "rust-coverage"
MEASURES = ("lines", "functions", "regions")
WORKFLOW = ".github/workflows/codeflow-ci.yml"
# The one hosted job that may stand for each target, and the matrix part
# whose `only:` list must name it. A target missing here has no hosted
# credit.
HOSTED_JOBS = {
    "present-browser": ("codeflow gates (release present)", "present"),
}


class Refused(Exception):
    """A reason the record is not evidence for the candidate."""


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True)
    if result.returncode != 0:
        raise Refused(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def commit_of(repo: Path, rev: str) -> str:
    return git(repo, "rev-parse", "--verify", "--end-of-options", f"{rev}^{{commit}}")


def tree_of(repo: Path, commit: str) -> str:
    return git(repo, "rev-parse", "--verify", "--end-of-options", f"{commit}^{{tree}}")


def full_targets(config: dict) -> set[str]:
    return {t["name"] for t in config.get("targets", [])
            if t.get("enabled", True) and "full" in t.get("modes", {})}


def configured_floor(config: dict) -> int | None:
    for target in config.get("targets", []):
        if target.get("name") == COVERAGE_TARGET:
            rules = (target.get("coverage") or {}).get("rules", [])
            floors = [r.get("minimum") for r in rules if r.get("scope") == "global"]
            return max(floors) if floors else None
    return None


def check_record(record: dict, candidate: str, tree: str, config: dict) -> dict:
    """The record's coverage entry, after every local check passes."""
    problems = []
    if record.get("schema_version") != 2:
        problems.append(f"run record schema {record.get('schema_version')!r}, expected 2")
    if record.get("mode") != "full":
        problems.append(f"mode {record.get('mode')!r}, expected full")
    for field in ("passed", "generation_unchanged", "clean"):
        if record.get(field) is not True:
            problems.append(f"{field} is {record.get(field)!r}")
    if record.get("revision") != candidate:
        problems.append(f"revision {record.get('revision')!r} is not the candidate {candidate}")
    if record.get("git_tree") != tree:
        problems.append(f"git_tree {record.get('git_tree')!r} is not the candidate's tree {tree}")
    entries = [c for c in record.get("coverage") or [] if c.get("target") == COVERAGE_TARGET]
    if len(entries) != 1:
        problems.append(f"{len(entries)} coverage entries for {COVERAGE_TARGET}, expected 1")
        raise Refused("; ".join(problems))
    entry = entries[0]
    totals = entry.get("totals") or {}
    for measure in MEASURES:
        value = totals.get(measure)
        if not (isinstance(value, dict) and all(isinstance(value.get(k), (int, float))
                                                for k in ("count", "covered", "percent"))):
            problems.append(f"coverage {measure} total missing")
    floor = configured_floor(config)
    if floor is None:
        problems.append(f"the candidate's config sets no global coverage rule for {COVERAGE_TARGET}")
    elif entry.get("floor") != floor:
        problems.append(f"recorded floor {entry.get('floor')!r} is not the candidate's floor {floor}")
    if entry.get("data_missing") or entry.get("thresholds_failed"):
        problems.append("coverage data missing or a coverage threshold failed")
    lines = totals.get("lines") or {}
    if floor is not None and isinstance(lines.get("percent"), (int, float)) and lines["percent"] < floor:
        problems.append(f"line coverage {lines['percent']:.2f}% is under the floor {floor}%")
    if problems:
        raise Refused("; ".join(problems))
    return entry


def local_gaps(record: dict, config: dict) -> set[str]:
    """Full-mode targets the candidate enables that did not pass in this run."""
    if record.get("ci_skipped"):
        raise Refused(f"targets skipped in CI: {', '.join(record['ci_skipped'])}")
    passed = {r.get("name") for r in record.get("results", []) if r.get("status") == "passed"}
    ran = {r.get("name") for r in record.get("results", [])}
    failed = ran - passed
    if failed:
        raise Refused(f"targets that ran and did not pass: {', '.join(sorted(failed))}")
    unknown = ran - full_targets(config)
    if unknown:
        raise Refused(f"targets the candidate's config does not run in full mode: {', '.join(sorted(unknown))}")
    return full_targets(config) - passed


def hosted_scope(repo: Path, head: str, part: str) -> list[str]:
    """The `only:` list of the workflow part `part` at commit `head`."""
    text = git(repo, "show", f"{head}:{WORKFLOW}")
    found = re.findall(rf"^\s*- part: {re.escape(part)}\n\s*only: (\S+)\s*$", text, re.MULTILINE)
    if len(found) != 1:
        raise Refused(f"{WORKFLOW} at {head[:12]} has {len(found)} `{part}` parts, expected 1")
    return found[0].split(",")


def check_hosted(repo: Path, target: str, run: dict, jobs: dict, candidate: str, tree: str) -> str:
    """The citable credit line for `target`, after every hosted check passes."""
    if target not in HOSTED_JOBS:
        raise Refused(f"{target}: no hosted job may stand for it")
    job_name, part = HOSTED_JOBS[target]
    run_id = run.get("id")
    if run.get("path") != WORKFLOW:
        raise Refused(f"{target}: run {run_id} is workflow {run.get('path')!r}, expected {WORKFLOW}")
    if run.get("status") != "completed":
        raise Refused(f"{target}: run {run_id} is {run.get('status')!r}, not completed")
    head = run.get("head_sha") or ""
    try:
        head_tree = tree_of(repo, head)
    except Refused as error:
        raise Refused(f"{target}: run {run_id} head {head!r} is not readable here: {error}") from None
    if head_tree != tree:
        older = subprocess.run(["git", "-C", str(repo), "merge-base", "--is-ancestor", head, candidate],
                               capture_output=True).returncode == 0
        kind = "stale: it ran on an earlier tree" if older else "it ran on another tree"
        raise Refused(f"{target}: run {run_id} at {head[:12]} is {kind}, not the candidate's tree {tree}")
    named = [j for j in jobs.get("jobs", []) if j.get("name") == job_name]
    if not named:
        others = sorted(j.get("name", "") for j in jobs.get("jobs", []))
        raise Refused(f"{target}: run {run_id} has no job {job_name!r} (jobs: {others})")
    if len(named) != 1:
        raise Refused(f"{target}: run {run_id} has {len(named)} jobs named {job_name!r}, expected 1")
    job = named[0]
    if job.get("run_id") not in (None, run_id):
        raise Refused(f"{target}: job {job.get('id')} belongs to run {job.get('run_id')}, not {run_id}")
    if job.get("status") != "completed" or job.get("conclusion") != "success":
        raise Refused(f"{target}: job {job_name!r} in run {run_id} is {job.get('status')!r} "
                      f"with conclusion {job.get('conclusion')!r}, not a completed success")
    scope = hosted_scope(repo, head, part)
    if target not in scope:
        raise Refused(f"{target}: job {job_name!r} runs only {','.join(scope)}, not {target}")
    return f"{target} by hosted job {job_name!r}, run {run_id}, job {job.get('id')}, head {head}"


def load_hosted(run_id: str, evidence: Path | None, github: str) -> tuple[dict, dict]:
    if not re.fullmatch(r"\d+", run_id):
        raise Refused(f"hosted run id {run_id!r} is not a number")
    if evidence is not None:
        try:
            return (json.loads((evidence / f"run-{run_id}.json").read_text()),
                    json.loads((evidence / f"jobs-{run_id}.json").read_text()))
        except (OSError, ValueError) as error:
            raise Refused(f"hosted evidence for run {run_id} unreadable: {error}") from None
    def api(path: str) -> dict:
        result = subprocess.run(["gh", "api", f"repos/{github}/actions/runs/{run_id}{path}"],
                                capture_output=True, text=True)
        if result.returncode != 0:
            raise Refused(f"gh api for run {run_id} failed: {result.stderr.strip()}")
        return json.loads(result.stdout)
    return api(""), api("/jobs?per_page=100")


def check(args: argparse.Namespace) -> list[str]:
    repo = Path(args.repo)
    candidate = commit_of(repo, args.candidate)
    tree = tree_of(repo, candidate)
    try:
        record = json.loads(Path(args.run).read_text())
    except (OSError, ValueError) as error:
        raise Refused(f"run record {args.run} unreadable: {error}") from None
    try:
        config = json.loads(git(repo, "show", f"{candidate}:.codeflow/test-config.json"))
    except ValueError as error:
        raise Refused(f"the candidate's test config is not JSON: {error}") from None
    entry = check_record(record, candidate, tree, config)
    hosted = {}
    for claim in args.hosted:
        target, sep, run_id = claim.partition("=")
        if not sep or target in hosted:
            raise Refused(f"--hosted {claim!r}: expected one TARGET=RUN_ID per target")
        hosted[target] = run_id
    gaps = local_gaps(record, config)
    if gaps != set(hosted):
        raise Refused(f"targets not run locally: {sorted(gaps) or 'none'}; credited to a hosted job: "
                      f"{sorted(hosted) or 'none'}; these must match")
    if record.get("complete") is not (not hosted):
        raise Refused(f"complete is {record.get('complete')!r} with {len(hosted)} hosted targets")
    credits = [check_hosted(repo, target, *load_hosted(run_id, args.hosted_evidence, args.github),
                            candidate, tree)
               for target, run_id in sorted(hosted.items())]
    totals = entry["totals"]
    measures = ", ".join(f"{m} {totals[m]['percent']:.2f}% ({totals[m]['covered']}/{totals[m]['count']})"
                         for m in MEASURES)
    lines = [f"run {record.get('run_id')}: revision {candidate}, git tree {tree}, "
             f"tracked digest {record.get('tree')}",
             f"coverage ({COVERAGE_TARGET}): {measures}; line floor {entry['floor']}%"]
    if credits:
        lines.append("not complete locally; credited to hosted jobs: " + "; ".join(credits))
    else:
        lines.append("complete local full gate, passed")
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--run", required=True, help="the run's run.json")
    parser.add_argument("--hosted", action="append", default=[], metavar="TARGET=RUN_ID")
    parser.add_argument("--hosted-evidence", type=Path)
    parser.add_argument("--repo", default=str(ROOT))
    parser.add_argument("--github", default="sathyassn/codeflow")
    args = parser.parse_args(argv)
    try:
        lines = check(args)
    except Refused as error:
        print(f"landing-coverage: refused: {error}", file=sys.stderr)
        return 1
    for line in lines:
        print(f"landing-coverage: {line}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

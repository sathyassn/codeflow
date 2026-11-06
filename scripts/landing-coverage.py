#!/usr/bin/env python3
"""Check a landing candidate's full-gate record and print its coverage (TSK-369).

A landing candidate's pull request cites one full gate run. This script reads
that run's `run.json` and accepts it only as evidence for the exact
candidate commit:

- the record exists, sits in a folder named by its own `run_id`, and is a
  full-mode run that passed, with tracked files unchanged during the run and
  a clean worktree;
- its `revision` is the candidate commit, its `git_tree` is that commit's
  tree, its `config_digest` is the digest of the candidate's
  `.codeflow/test-config.json`, and its tracked digest `tree` equals the
  digest the gate computes for this checkout, which must be the candidate,
  clean;
- its rust-coverage entry carries line, function and region totals whose
  counts are consistent (covered at most count, percent equal to
  covered/count), a line floor equal to the global rule in the candidate's
  own config, no missing data and no failed threshold, and the line total
  meets the floor;
- every full-mode target the candidate's config enables ran and passed,
  unless it is named with `--hosted`.

A target named with `--hosted TARGET=RUN_ID` did not run locally and is
credited to a hosted job only when that GitHub Actions run of this
repository's CI workflow, on this repository's own branch (no fork), by the
event pinned for the target below, completed with success; its head commit
has the candidate's tree; every one of its jobs was read; and the one job
pinned for the target completed with success, where that job's own
`only:` list in the workflow at that commit names the target. A failed, missing, stale (an earlier tree),
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
import hashlib
import json
import math
import os
import re
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COVERAGE_TARGET = "rust-coverage"
MEASURES = ("lines", "functions", "regions")
WORKFLOW = ".github/workflows/codeflow-ci.yml"
# The one hosted job that may stand for each target, the matrix part of
# that job whose `only:` list must name it, and the one event that may start
# it. A landing candidate on an integration line gets the release parts only
# from a manual run (`gates-release` skips pull requests into integration/**),
# so the event is workflow_dispatch. A target missing here has no hosted
# credit.
HOSTED_JOBS = {
    "present-browser": ("codeflow gates (release present)", "present", "workflow_dispatch"),
}
JOBS_PAGE = 100


class Refused(Exception):
    """A reason the record is not evidence for the candidate."""


def git_bytes(repo: Path, *args: str) -> bytes:
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True)
    if result.returncode != 0:
        raise Refused(f"git {' '.join(args)} failed: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def git(repo: Path, *args: str) -> str:
    return git_bytes(repo, *args).decode().strip()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def tracked_digest(repo: Path, candidate: str) -> str:
    """The gate's tracked-snapshot digest (`delivery::tracked` and
    `delivery::digest`) for this checkout, which must be the candidate with
    no change: each index entry's name maps to the digest of its file mode
    (4 bytes, little-endian) and bytes, and the digest is taken over that
    map as compact JSON in name order."""
    if commit_of(repo, "HEAD") != candidate:
        raise Refused("this checkout is not the candidate: run the check from a checkout of it")
    if git(repo, "status", "--porcelain", "--untracked-files=no"):
        raise Refused("this checkout has tracked changes: run the check from a clean checkout")
    top = Path(git(repo, "rev-parse", "--show-toplevel"))
    entries = {}
    for record in git_bytes(repo, "ls-files", "-s", "-z").split(b"\0"):
        if not record:
            continue
        meta, _, raw = record.partition(b"\t")
        mode = meta.split(b" ")[0]
        if mode == b"160000":
            raise Refused("the checkout has a submodule, whose snapshot this check does not compute")
        name = raw.decode()
        path = top / name
        try:
            if path.is_symlink():
                data = os.fsencode(os.readlink(path))
            else:
                data = path.read_bytes()
            bits = int(mode, 8) if os.name == "nt" else os.lstat(path).st_mode
        except OSError as error:
            raise Refused(f"cannot read tracked {name}: {error}") from None
        entries[name] = sha256(struct.pack("<I", bits) + data)
    text = json.dumps(dict(sorted(entries.items())), separators=(",", ":"), ensure_ascii=False)
    return sha256(text.encode())


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


def is_count(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def measure_problem(name: str, value) -> str | None:
    """Why a recorded total is not a consistent measure, or None."""
    if not isinstance(value, dict):
        return f"coverage {name} total missing"
    count, covered, percent = value.get("count"), value.get("covered"), value.get("percent")
    if not (is_count(count) and is_count(covered)):
        return f"coverage {name} total missing"
    if not isinstance(percent, (int, float)) or isinstance(percent, bool):
        return f"coverage {name} total missing"
    if covered > count:
        return f"coverage {name} covers {covered} of {count}"
    expected = 100.0 if count == 0 else covered * 100 / count
    if not math.isclose(percent, expected, rel_tol=1e-9, abs_tol=1e-9):
        return f"coverage {name} percent {percent} is not {covered}/{count}"
    return None


def check_record(record: dict, candidate: str, tree: str, config: dict,
                 config_digest: str, snapshot: str) -> dict:
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
    if record.get("config_digest") != config_digest:
        problems.append(f"config_digest {record.get('config_digest')!r} is not the candidate's "
                        f"test config {config_digest}")
    if record.get("tree") != snapshot:
        problems.append(f"tracked digest {record.get('tree')!r} is not this checkout's {snapshot}")
    entries = [c for c in record.get("coverage") or [] if c.get("target") == COVERAGE_TARGET]
    if len(entries) != 1:
        problems.append(f"{len(entries)} coverage entries for {COVERAGE_TARGET}, expected 1")
        raise Refused("; ".join(problems))
    entry = entries[0]
    totals = entry.get("totals") or {}
    measures_ok = True
    for measure in MEASURES:
        problem = measure_problem(measure, totals.get(measure))
        if problem:
            problems.append(problem)
            measures_ok = False
    floor = configured_floor(config)
    recorded = entry.get("floor")
    if not is_count(floor):
        problems.append(f"the candidate's config sets no global coverage rule for {COVERAGE_TARGET}")
    elif not is_count(recorded) or recorded != floor:
        problems.append(f"recorded floor {recorded!r} is not the candidate's floor {floor}")
    if entry.get("data_missing") is not False:
        problems.append(f"data_missing is {entry.get('data_missing')!r}, expected false")
    failed = entry.get("thresholds_failed")
    if not is_count(failed) or failed != 0:
        problems.append(f"thresholds_failed is {failed!r}, expected 0")
    lines = totals.get("lines") or {}
    if measures_ok and is_count(floor) and lines["percent"] < floor:
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


def job_scope(text: str, job_name: str, part: str) -> list[str]:
    """The `only:` list of matrix part `part` in the one workflow job whose
    name, with that part filled in, is `job_name`."""
    lines = text.split("\n")
    if "jobs:" not in lines:
        raise Refused(f"{WORKFLOW} has no jobs")
    body = lines[lines.index("jobs:") + 1:]
    starts = [i for i, line in enumerate(body) if re.fullmatch(r"  [A-Za-z0-9_-]+:\s*", line)]
    found = []
    for start, end in zip(starts, starts[1:] + [len(body)]):
        block = "\n".join(body[start:end])
        name = re.search(r"^    name: (.+?)\s*$", block, re.MULTILINE)
        if not name or name.group(1).replace("${{ matrix.part }}", part) != job_name:
            continue
        found.append(re.findall(rf"^\s*- part: {re.escape(part)}\n\s*only: (\S+)\s*$", block, re.MULTILINE))
    if len(found) != 1 or len(found[0]) != 1:
        raise Refused(f"{WORKFLOW} has {len(found)} jobs named {job_name!r} and "
                      f"{sum(map(len, found))} `{part}` parts in them, expected one of each")
    return found[0][0].split(",")


def check_hosted(repo: Path, target: str, run: dict, jobs: dict, candidate: str, tree: str,
                 github: str) -> str:
    """The citable credit line for `target`, after every hosted check passes."""
    if target not in HOSTED_JOBS:
        raise Refused(f"{target}: no hosted job may stand for it")
    job_name, part, event = HOSTED_JOBS[target]
    run_id = run.get("id")
    if run.get("path") != WORKFLOW:
        raise Refused(f"{target}: run {run_id} is workflow {run.get('path')!r}, expected {WORKFLOW}")
    for field in ("repository", "head_repository"):
        name = (run.get(field) or {}).get("full_name")
        if name != github:
            raise Refused(f"{target}: run {run_id} {field} is {name!r}, expected {github}")
    if run.get("event") != event:
        raise Refused(f"{target}: run {run_id} was started by {run.get('event')!r}, expected {event}")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise Refused(f"{target}: run {run_id} is {run.get('status')!r} with conclusion "
                      f"{run.get('conclusion')!r}, not a completed success")
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
    listed = jobs.get("jobs")
    total = jobs.get("total_count")
    if not isinstance(listed, list) or not is_count(total) or total != len(listed):
        raise Refused(f"{target}: run {run_id} lists {len(listed) if isinstance(listed, list) else 'no'} "
                      f"jobs of total_count {total!r}; every job must be read")
    named = [j for j in listed if j.get("name") == job_name]
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
    scope = job_scope(git(repo, "show", f"{head}:{WORKFLOW}"), job_name, part)
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
    jobs: list = []
    total = None
    for page in range(1, 101):
        body = api(f"/jobs?per_page={JOBS_PAGE}&page={page}")
        total = body.get("total_count")
        listed = body.get("jobs") or []
        jobs.extend(listed)
        if len(listed) < JOBS_PAGE or (is_count(total) and len(jobs) >= total):
            break
    return api(""), {"total_count": total, "jobs": jobs}


def check(args: argparse.Namespace) -> list[str]:
    repo = Path(args.repo)
    candidate = commit_of(repo, args.candidate)
    tree = tree_of(repo, candidate)
    try:
        record = json.loads(Path(args.run).read_text())
    except (OSError, ValueError) as error:
        raise Refused(f"run record {args.run} unreadable: {error}") from None
    if not isinstance(record, dict):
        raise Refused(f"run record {args.run} is not a JSON object")
    if record.get("run_id") != Path(args.run).resolve().parent.name:
        raise Refused(f"run record {args.run} is not in the folder its run_id "
                      f"{record.get('run_id')!r} names")
    blob = git_bytes(repo, "show", f"{candidate}:.codeflow/test-config.json")
    try:
        config = json.loads(blob)
    except ValueError as error:
        raise Refused(f"the candidate's test config is not JSON: {error}") from None
    snapshot = tracked_digest(repo, candidate)
    entry = check_record(record, candidate, tree, config, sha256(blob), snapshot)
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
                            candidate, tree, args.github)
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

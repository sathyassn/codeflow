#!/usr/bin/env python3
"""Discover daily Windows and Linux coverage sources, and judge their runs from
what GitHub records.

Each line's run is started at that line's own ref (windows-daily.yml), so no line
code runs under main's ref and nothing it writes to a cache reaches main's scope.
The run's own workflow file and scripts come from the line, so a line can change
what its run reports. So this script, run from main, gives the only verdict, from
GitHub's record of the run. No daily result supplies PR merge evidence.

Which lines: main and each active integration line. The refs are listed from the
remote on every run and nothing stores them, so a new line joins and a deleted
one leaves on the next run. A line is active when its own tree at discovery has
what the runner calls and its copy of windows-daily-run.yml is byte-identical to
main's: every script named in that file, each with the subcommands and flags it
passes. The test reads those files and runs nothing from the line. A line that
fails it stays out, and the run's summary names it and the reason. A line left out has no daily evidence: its row in the verdict says so, the
overall verdict can pass without it, and that pass says nothing about it. main is always selected. A selected line that fails fails visibly. If the
listing or a read fails, the run fails and offers no lines. Other branches are not
covered.

What a run skips: no Windows job and no Linux coverage shard. A selected line gets
no new run when its exact tip
already has a passing daily run recorded, younger than the age windows-daily.yml
declares, which keeps image drift covered at a known interval. A failed, missing,
unfinished or unreadable run, or an age that is absent or not a positive whole
number, means no skip. It runs every Windows job and the Linux coverage shards with
their report; the other Linux gates are not part of it. A commit that was never a
line's tip at discovery is not tested, and a later tip gets no credit.

What is credited: a requested line's run, started after the request, whose recorded
workflow path, event, branch and exact head SHA all match, which GitHub records as
completed with success, with every expected job successful, at a commit whose copy
of windows-daily-run.yml is byte-identical to main's at the request, whose hash the
request records, and whose recorded Linux coverage total is for that commit and at
or above the floor in main's .codeflow/test-config.json. The verdict reports each
line's total and the commit measured. Anything else is no credit,
and a run that is missing or has not finished is no evidence, never a pass. A line
that was skipped is judged the same way, against its recorded run. A daily coverage
result is never merge evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from urllib.parse import quote


WINDOWS_JOBS = frozenset({
    "windows-build", "windows-tests", "windows", "windows-release-journey",
    "windows-journeys",
})
# The runner's Linux coverage jobs, with the ids and commands of codeflow-ci.yml's.
COVERAGE_JOBS = frozenset({"gates-coverage", "gates-coverage-report"})
COVERAGE_SHARDS = 2
SHA = re.compile(r"[0-9a-f]{40}")
REF = re.compile(r"refs/heads/(?:main|integration/[A-Za-z0-9_./-]+)")
MAIN = "refs/heads/main"
RUNNER_PATH = ".github/workflows/windows-daily-run.yml"
RUNNER = Path(__file__).resolve().parents[1] / RUNNER_PATH
DISPATCHER_PATH = ".github/workflows/windows-daily.yml"
PARTITIONS = 16
# Each job's name in the runner; a test compares these with the file.
RUN_JOB_NAMES = frozenset({
    "windows test build", "windows checks (doc, clippy, Node)", "windows release journey", "windows journeys",
    *(f"windows tests ({n} of {PARTITIONS})" for n in range(1, PARTITIONS + 1)),
    "linux coverage report", *(f"linux coverage ({n} of {COVERAGE_SHARDS})" for n in range(1, COVERAGE_SHARDS + 1)),
})
RUN_EVENT = "workflow_dispatch"
DISPATCHER_EVENTS = frozenset({"schedule", "workflow_dispatch"})
# The request is read six hours after it is made; a day-old one means a daily run was missed.
MAX_REQUEST_AGE = timedelta(hours=30)
REQUESTED_FILE = "daily-windows-requested.json"
RESULT_FILE = "daily-windows-result.json"
LEFT_OUT = "left out, no daily evidence and no daily coverage"
# The coverage report job keeps the total and the commit it measured here.
COVERAGE_ARTIFACT = "daily-coverage-total"
COVERAGE_FILE = "daily-coverage-total.json"
CONFIG = Path(__file__).resolve().parents[1] / ".codeflow/test-config.json"
# Declared once, in windows-daily.yml; the dispatcher records it for the reader.
MAX_AGE_VARIABLE = "DAILY_PROOF_MAX_AGE_DAYS"
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
    return {"include": [{"ref": ref, "sha": sha} for ref, sha in sorted(found.items())]}


def runner_needs(workflow: str) -> dict[str, set[str]]:
    """Each script the runner calls in the line's tree, with what it passes it."""
    needs: dict[str, set[str]] = {}
    for script, arguments in COMMAND.findall(workflow):
        words = arguments.split()
        wanted = needs.setdefault(script, set())
        # A first word that is a name is a subcommand; a path or a quoted command is not.
        if words and re.fullmatch(r"[a-z][a-z0-9-]*", words[0]):
            wanted.add(words[0])
        wanted.update(word for word in words if word.startswith("--"))
    if not needs:
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


def runner_hash(text: str | None) -> str | None:
    return None if text is None else hashlib.sha256(text.encode("utf-8")).hexdigest()


def runner_differs(read, runner_text: str) -> list[str]:
    """A run is credited only under main's runner, so a line with another one gets no
    run. `read` gives a file's text, None when the tree has no such file."""
    return [] if read(RUNNER_PATH) == runner_text else [f"{RUNNER_PATH} differs from main's"]


def select_active(listed: dict, needs: dict[str, set[str]], read_at, runner_text: str) -> tuple[dict, list[dict]]:
    """Keep main and every line whose tree has what the runner needs. A read that
    raises stops discovery: an unread line is neither selected nor called inactive."""
    selected, left_out = [], []
    for row in listed["include"]:
        read = lambda path, sha=row["sha"]: read_at(sha, path)
        missing = [] if row["ref"] == MAIN else runner_differs(read, runner_text) + missing_from_line(needs, read)
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


def when(stamp: str) -> datetime:
    return datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def latest_request(runs: list, now: datetime) -> dict:
    """The newest finished dispatcher run on main, if it is recent enough to mean today's."""
    found = [run for run in runs if run.get("path") == DISPATCHER_PATH and run.get("head_branch") == "main"
             and run.get("event") in DISPATCHER_EVENTS and run.get("status") == "completed"]
    if not found:
        raise ValueError("GitHub records no finished daily Windows dispatcher run on main; no line has evidence")
    latest = max(found, key=lambda run: when(run["created_at"]))
    if now - when(latest["created_at"]) > MAX_REQUEST_AGE:
        raise ValueError(f"the newest dispatcher run is from {latest['created_at']}; no line has evidence for today")
    return latest


def requested_rows(request: dict) -> tuple[list, list, list]:
    """The lines started, those skipped and those left out. A malformed request stops the read."""
    rows, skipped, left_out = request.get("include"), request.get("skipped"), request.get("left_out")
    if not all(isinstance(part, list) for part in (rows, skipped, left_out)):
        raise ValueError("the dispatcher's request is malformed")
    for row in rows + skipped + left_out:
        identity(row["ref"], row["sha"])
    judged = [row["ref"] for row in rows + skipped]
    if MAIN not in judged:
        raise ValueError("the dispatcher's request does not include main")
    if set(judged) & {row["ref"] for row in left_out}:
        raise ValueError("the dispatcher's request both judges a line and leaves it out")
    return rows, skipped, left_out


def requested_runner_hash(request: dict) -> str:
    """The hash of main's runner when the request was made; a day's lines are judged against it."""
    recorded = request.get("runner_sha256")
    if not isinstance(recorded, str) or not re.fullmatch(r"[0-9a-f]{64}", recorded):
        raise ValueError("the dispatcher's request does not record main's runner")
    return recorded


def main_floor() -> float:
    """The line floor of main's rust-coverage target, read by coverage-merge.py from
    main's checkout, so a line that lowers its own floor is still held to main's."""
    spec = importlib.util.spec_from_file_location("coverage_merge", Path(__file__).resolve().parent / "coverage-merge.py")
    merge = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(merge)
    try:
        return float(merge.floor_from(CONFIG))
    except merge.Problem as problem:
        raise ValueError(f"main's coverage floor cannot be read: {problem}") from problem


def coverage_record(coverage_of, run_id, sha: str) -> tuple[dict | None, str | None]:
    """The run's Linux coverage total for `sha`, or why there is none. The file comes
    from the line's run, so only its counts and commit are read, and checked."""
    try:
        record = coverage_of(run_id) if coverage_of else None
    except (ValueError, KeyError, TypeError, AttributeError, OSError, subprocess.SubprocessError) as error:
        return None, f"no Linux coverage total could be read: {error}"
    if not isinstance(record, dict):
        return None, "the run recorded no Linux coverage total"
    lines, covered, measured = record.get("lines"), record.get("covered"), record.get("sha")
    if not (type(lines) is int and type(covered) is int and 0 < lines and 0 <= covered <= lines):
        return None, "the run's Linux coverage total is malformed"
    if measured != sha:
        return None, f"the run's Linux coverage total is for {measured!r}, not {sha}"
    return {"lines": lines, "covered": covered, "percent": round(covered * 100.0 / lines, 2), "sha": sha}, None


def judge(row: dict, requested_at: datetime, runs: list, jobs_of, read_runner, main_runner: str,
          coverage_of=None, floor: float | None = None) -> dict:
    """Credit a requested line only from GitHub's record of its run. Every path but
    one, including any mismatch, a missing run, a late one and a coverage total that
    is missing, for another commit or under main's floor, is not credited."""
    ref, sha = row["ref"], row["sha"]
    branch = ref.removeprefix("refs/heads/")
    result = {"ref": ref, "requested_sha": sha, "tested_sha": None, "run_id": None}

    def uncredited(state: str, reason: str) -> dict:
        return {**result, "result": state, "reasons": [reason]}

    since = [run for run in runs if when(run["created_at"]) >= requested_at]
    if not since:
        return uncredited("no-evidence", "GitHub records no run started since the request")
    run = max(since, key=lambda candidate: when(candidate["created_at"]))
    result["run_id"] = run.get("id")
    recorded = {"path": RUNNER_PATH, "event": RUN_EVENT, "head_branch": branch, "head_sha": sha}
    wrong = [f"{name} is {run.get(name)!r}, expected {want!r}" for name, want in recorded.items()
             if run.get(name) != want]
    if wrong:
        return {**result, "result": "no-credit", "reasons": wrong}
    if run.get("status") != "completed":
        return uncredited("no-evidence", f"the run has not finished: {run.get('status')!r}")
    coverage, missing = coverage_record(coverage_of, run["id"], sha)
    result["coverage"] = coverage
    if run.get("conclusion") != "success":
        return uncredited("failure", f"the run concluded {run.get('conclusion')!r}")
    if runner_hash(read_runner(sha)) != main_runner:
        return uncredited("no-credit", f"{RUNNER_PATH} at this commit differs from main's at the request")
    jobs = {job.get("name"): job.get("conclusion") for job in jobs_of(run["id"])}
    wrong = [f"job {name!r} concluded {jobs.get(name, 'missing')!r}" for name in sorted(RUN_JOB_NAMES)
             if jobs.get(name) != "success"]
    wrong += [f"unexpected job {name!r}" for name in sorted(set(jobs) - RUN_JOB_NAMES)]
    if wrong:
        return {**result, "result": "no-credit", "reasons": wrong}
    if missing:
        return uncredited("no-credit", missing)
    floor = main_floor() if floor is None else floor
    result["floor"] = floor
    if coverage["covered"] * 100.0 < floor * coverage["lines"]:
        return uncredited("failure", f"Linux line coverage {coverage['percent']:.2f} percent is under main's "
                                     f"{floor:g} percent floor")
    return {**result, "tested_sha": sha, "result": "credited", "reasons": []}


def max_age_days(value) -> int:
    """The declared age, or 0, which skips nothing, when it is not a positive whole number."""
    return int(value) if re.fullmatch(r"[1-9][0-9]{0,3}", str(value)) else 0


def judge_line(row: dict, since: datetime, api, read_at, main_runner: str, coverage_of=None) -> dict:
    """Read GitHub's record of a line's runs and judge the newest one since `since`."""
    repo = os.environ["GITHUB_REPOSITORY"]
    branch = quote(row["ref"].removeprefix("refs/heads/"), safe="")
    runs = api(f"repos/{repo}/actions/workflows/windows-daily-run.yml/runs"
               f"?branch={branch}&event={RUN_EVENT}&per_page=30")["workflow_runs"]
    return judge(row, since, runs,
                 lambda run: api(f"repos/{repo}/actions/runs/{run}/jobs?filter=latest&per_page=100")["jobs"],
                 lambda sha: read_at(sha, RUNNER_PATH), main_runner, coverage_of or download_coverage)


def skip_proven(matrix: dict, age: int, api, read_at, main_runner: str, now: datetime,
                coverage_of=None) -> tuple[dict, list]:
    """Move out each line whose exact tip has a passing run within the age. Anything
    that stops the check, a read included, leaves the line to run: it fails closed."""
    to_run, skipped = [], []
    for row in matrix["include"]:
        proof = None
        if age:
            try:
                proof = judge_line(row, now - timedelta(days=age), api, read_at, main_runner, coverage_of)
            except (ValueError, KeyError, TypeError, AttributeError, OSError, subprocess.SubprocessError):
                proof = None
        if proof and proof["result"] == "credited":
            skipped.append({**row, "run_id": proof["run_id"]})
        else:
            to_run.append(row)
    return {"include": to_run}, skipped


def left_out_line(row: dict) -> dict:
    """A line left out has its own row, so a reader looking for it finds no evidence."""
    return {"ref": row["ref"], "requested_sha": row["sha"], "tested_sha": None, "run_id": None,
            "result": "no-evidence", "left_out": True, "reasons": row["missing"]}


def read_evidence(api, download, read_at, now: datetime, coverage_of=None) -> dict:
    """Judge every requested line. The result passes when each line started or skipped
    is credited. A line left out does not decide it, and that pass says nothing about it."""
    repo = os.environ["GITHUB_REPOSITORY"]
    request_run = latest_request(
        api(f"repos/{repo}/actions/workflows/windows-daily.yml/runs?branch=main&per_page=30")["workflow_runs"], now)
    request = download(request_run["id"])
    rows, skipped, left_out = requested_rows(request)
    main_runner = requested_runner_hash(request)
    requested_at = when(request_run["created_at"])
    judged = [judge_line(row, requested_at, api, read_at, main_runner, coverage_of) for row in rows]
    age = timedelta(days=max_age_days(request.get("max_age_days")))
    # A skipped line has no new run, so it stands on its recorded one, within the same age.
    for row in skipped:
        judged.append({**judge_line(row, requested_at - age, api, read_at, main_runner, coverage_of),
                       "skipped": True})
    passed = bool(judged) and all(line["result"] == "credited" for line in judged)
    return {"result": "success" if passed else "failure", "request_run": request_run["id"],
            "requested_at": request_run["created_at"], "lines": judged + [left_out_line(row) for row in left_out]}


def gh_api(path: str) -> dict:
    return json.loads(subprocess.check_output(["gh", "api", path], text=True, timeout=120))


def download_request(run_id: int) -> dict:
    with tempfile.TemporaryDirectory() as folder:
        subprocess.check_output(["gh", "run", "download", str(run_id), "--repo", os.environ["GITHUB_REPOSITORY"],
                                 "--name", "daily-windows-requested", "--dir", folder], text=True, timeout=120)
        return json.loads((Path(folder) / REQUESTED_FILE).read_text(encoding="utf-8"))


def download_coverage(run_id: int) -> dict:
    with tempfile.TemporaryDirectory() as folder:
        subprocess.check_output(["gh", "run", "download", str(run_id), "--repo", os.environ["GITHUB_REPOSITORY"],
                                 "--name", COVERAGE_ARTIFACT, "--dir", folder], text=True, timeout=120)
        return json.loads((Path(folder) / COVERAGE_FILE).read_text(encoding="utf-8"))


def coverage_line(line: dict) -> str:
    """What the verdict says about a judged line's Linux coverage."""
    coverage = line.get("coverage")
    if not coverage:
        return "\n  - no Linux coverage total recorded"
    floor = f"; main's floor is {line['floor']:g} percent" if "floor" in line else ""
    return (f"\n  - Linux line coverage {coverage['percent']:.2f} percent ({coverage['covered']} of "
            f"{coverage['lines']} lines) measured at `{coverage['sha']}`{floor}")


def verdict_report(result: dict) -> str:
    text = f"## Daily Windows and Linux coverage verdict: {result['result']}\n\n"
    text += "This result gives no evidence for a newer branch tip or a pull request, and is never merge evidence.\n"
    text += "It covers only the lines credited below.\n"
    for line in result.get("lines", []):
        carried = f", skipped: stands on run {line['run_id']}" if line.get("skipped") else ""
        state = LEFT_OUT if line.get("left_out") else line["result"]
        text += f"\n- `{line['ref']}` at `{line['requested_sha']}`: {state}{carried}"
        text += "".join(f"\n  - {reason}" for reason in line["reasons"])
        if not line.get("left_out"):
            text += coverage_line(line)
    for reason in result.get("reasons", []):
        text += f"\n- {reason}"
    return text


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
    commands.add_parser("read", help="judge the requested runs from GitHub's record; run this from main")
    tested = commands.add_parser("identity", help="stop a run whose checkout is not the requested commit")
    tested.add_argument("--ref", required=True)
    tested.add_argument("--sha", required=True)
    args = parser.parse_args()
    try:
        if args.command == "discover":
            refs = subprocess.check_output(
                ["git", "ls-remote", "--heads", "origin", "main", "integration/*"],
                text=True, timeout=60,
            )
            runner_text = RUNNER.read_text(encoding="utf-8")
            read_at = tree_reader()
            matrix, left_out = select_active(sources(refs), runner_needs(runner_text), read_at, runner_text)
            age = max_age_days(os.environ.get(MAX_AGE_VARIABLE))
            main_runner = runner_hash(runner_text)
            matrix, skipped = skip_proven(matrix, age, gh_api, read_at, main_runner, datetime.now(timezone.utc))
            report = "## Daily Windows source commits\n\n" + "\n".join(
                f"- `{row['ref']}` at `{row['sha']}`" for row in matrix["include"]
            )
            if skipped:
                report += f"\n\n## Skipped: a passing run within {age} days at this exact tip\n\n" + "\n".join(
                    f"- `{row['ref']}` at `{row['sha']}`: run {row['run_id']}" for row in skipped
                )
            if left_out:
                report += f"\n\n## {LEFT_OUT.capitalize()}: not proven active\n\n" + "\n".join(
                    f"- `{row['ref']}` at `{row['sha']}`: {'; '.join(row['missing'])}" for row in left_out
                )
            Path(REQUESTED_FILE).write_text(json.dumps(
                {**matrix, "skipped": skipped, "left_out": left_out, "max_age_days": age,
                 "runner_sha256": main_runner}, indent=2) + "\n",
                encoding="utf-8")
            output("matrix", json.dumps(matrix, separators=(",", ":")))
            output("runs", "true" if matrix["include"] else "false")
            summary(report)
        elif args.command == "identity":
            actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
            output("sha", checked_out(args.ref, args.sha, actual))
            summary(f"Windows source: `{args.ref}` at `{actual}`. This result covers only this commit.")
        else:
            try:
                result = read_evidence(gh_api, download_request, tree_reader(), datetime.now(timezone.utc))
            except (ValueError, KeyError, TypeError, AttributeError, OSError, subprocess.SubprocessError) as error:
                result = {"result": "failure", "lines": [], "reasons": [f"no line has evidence: {error}"]}
            Path(RESULT_FILE).write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
            summary(verdict_report(result))
            return int(result["result"] != "success")
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        print(f"windows-daily: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

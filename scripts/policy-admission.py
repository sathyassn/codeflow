#!/usr/bin/env python3
"""Merge admission for this repository's `trusted: commit standards` check
(TSK-1239), started when the pull request's codeflow-ci run completes, with
no runner held while that run is pending.

`admit` runs inside the commit standards job. It judges once and never
waits: it passes only when the pull request is still open, ready, at the
judged head and on the judged target; the newest codeflow-ci pull_request
run of that pull request at exactly that head completed with success and
every named verdict job succeeded (the predicate of `ci-reuse.py admit`,
TSK-449, reused unchanged); and the pull request was not retargeted after
that run was created. A run's `pull_requests` entry names the pull
request's current base, never the base it ran against, so a retarget is
read from the pull request's timeline. A pending or absent run refuses at
once, and the completion of that run starts the check again.

`rerun` runs in codeflow-admission.yml on `workflow_run` completed. For a
completed codeflow-ci pull_request run whose pull request is open, ready and
still at that run's head, it re-runs the commit standards job of the newest
codeflow-policy run of that pull request at that head, so the native job
judges again with the run complete. It never writes a check or a status,
never reads an artifact and never runs code from the pull request; every
value it acts on comes from the API and is validated first. A newest policy
run that is not completed is left alone, because it judges the completed
run itself.

Any API error, unexpected record or failed write refuses: `admit` exits 1,
and `rerun` exits 1 without re-running, so the check keeps its last,
non-admitting verdict until someone re-runs it.

Usage: policy-admission.py admit --repo OWNER/NAME --head SHA --pull N --base REF
                                 --workflow PATH --require NAME [--require NAME ...]
       policy-admission.py rerun --repo OWNER/NAME --run ID --head SHA
                                 --workflow PATH --policy-workflow PATH --job NAME
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import sys
import urllib.request
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
_SPEC = importlib.util.spec_from_file_location("ci_reuse", HERE / "ci-reuse.py")
reuse = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(reuse)

Skip, Pending, SHA = reuse.Skip, reuse.Pending, reuse.SHA


def timestamp(value, what: str) -> datetime:
    if not isinstance(value, str):
        raise Skip(f"{what} has no timestamp")
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise Skip(f"{what} has an unreadable timestamp {value!r}") from error


def current_pull(api, pull: int) -> dict:
    record = api.get(f"pulls/{pull}")
    if not isinstance(record, dict) or record.get("number") != pull:
        raise Skip(f"the API returned no record for pull request #{pull}")
    return record


def admit(head: str, pull: int, base: str, workflow: str, required: set[str], api) -> int:
    """The id of the codeflow-ci run that admits pull request `pull` at
    `head` into `base`; raises Skip (Pending for a run not yet complete)."""
    if not SHA.match(head):
        raise Skip(f"the head {head!r} is not a commit id")
    record = current_pull(api, pull)
    if record.get("state") != "open":
        raise Skip(f"pull request #{pull} is {record.get('state')}")
    if record.get("draft") is not False:
        raise Skip(f"pull request #{pull} is a draft; mark it ready so its verdicts run at this head")
    now_head = (record.get("head") or {}).get("sha")
    if now_head != head:
        raise Skip(f"pull request #{pull} is now at {now_head}, not the judged head {head}; "
                   "the run for the new head decides")
    now_base = (record.get("base") or {}).get("ref")
    if now_base != base:
        raise Skip(f"pull request #{pull} now targets {now_base}, not the judged {base}; "
                   "the run queued by the retarget decides")
    run_id = reuse.admit(head, workflow, required, api, pull)
    run = api.get(f"actions/runs/{run_id}")
    if not isinstance(run, dict) or run.get("id") != run_id:
        raise Skip(f"the API returned no record for run {run_id}")
    created = timestamp(run.get("created_at"), f"run {run_id}")
    retargets = [timestamp(event.get("created_at"), "a base change")
                 for event in api.pages(f"issues/{pull}/timeline", None)
                 if isinstance(event, dict) and event.get("event") == "base_ref_changed"]
    if retargets and max(retargets) >= created:
        raise Skip(f"pull request #{pull} was retargeted at {max(retargets).isoformat()}, after run {run_id} "
                   f"was created at {created.isoformat()}, so that run tested another target; push a commit, "
                   "or close and reopen the pull request, to start a run against the current target")
    return run_id


def admit_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="policy-admission.py admit", description=admit.__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--pull", type=int, required=True)
    parser.add_argument("--base", required=True, help="the target branch the check judged")
    parser.add_argument("--workflow", required=True, help="the workflow file whose run must pass")
    parser.add_argument("--require", action="append", default=[], help="a verdict job that must succeed")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args(argv)
    if not args.require:
        print("policy-admission admit: refused: no verdict job is named")
        return 1
    api = reuse.Api(args.api, args.repo, os.environ.get("GH_TOKEN", ""))
    try:
        run = admit(args.head, args.pull, args.base, args.workflow, set(args.require), api)
    except Pending as reason:
        print(f"policy-admission admit: refused: {reason}; this check runs again when that run completes "
              "(codeflow-admission.yml), so no re-run is needed")
        return 1
    except Skip as reason:
        print(f"policy-admission admit: refused: {reason}")
        return 1
    except Exception as error:  # noqa: BLE001 - any failure refuses admission
        print(f"policy-admission admit: refused: the decision failed ({type(error).__name__}: {error})")
        return 1
    print(f"policy-admission admit: run {run} passed every verdict at head {args.head}")
    return 0


class Writer(reuse.Api):
    """The read API plus the one write `rerun` makes."""

    def post(self, path: str) -> None:
        request = urllib.request.Request(f"{self.url}/repos/{self.repo}/{path}", data=b"{}", method="POST", headers={
            "Accept": "application/vnd.github+json", "Authorization": f"Bearer {self.token}",
            "Content-Type": "application/json", "X-GitHub-Api-Version": "2022-11-28"})
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status not in (200, 201, 204):
                raise Skip(f"POST {path} answered {response.status}")


def rerun(run_id: int, head: str, workflow: str, policy_workflow: str, job_name: str, api) -> str:
    """Re-run the commit standards job for the pull request of completed
    codeflow-ci run `run_id`; returns what was done. Raises Skip on any
    record that cannot be trusted."""
    if not SHA.match(head):
        raise Skip(f"the head {head!r} is not a commit id")
    run = api.get(f"actions/runs/{run_id}")
    if not isinstance(run, dict) or run.get("id") != run_id:
        raise Skip(f"the API returned no record for run {run_id}")
    if run.get("event") != "pull_request" or run.get("path") != workflow or run.get("head_sha") != head:
        raise Skip(f"run {run_id} is not a {workflow} pull_request run at {head}")
    if run.get("status") != "completed":
        raise Skip(f"run {run_id} is {run.get('status')}, not completed")
    pulls = [pr.get("number") for pr in run.get("pull_requests") or [] if isinstance(pr, dict)]
    if len(pulls) != 1 or not isinstance(pulls[0], int):
        return (f"nothing re-run: run {run_id} names {len(pulls)} pull requests in this repository "
                "(a fork's pull request names none); re-run its check by hand")
    pull = pulls[0]
    record = current_pull(api, pull)
    if record.get("state") != "open" or record.get("draft") is not False:
        return f"nothing re-run: pull request #{pull} is {record.get('state')}, draft {record.get('draft')}"
    if (record.get("head") or {}).get("sha") != head:
        return f"nothing re-run: pull request #{pull} moved past {head}; the run for its new head decides"
    policy = [r for r in api.pages(f"actions/runs?head_sha={head}&event=pull_request_target", "workflow_runs")
              if r.get("event") == "pull_request_target" and r.get("head_sha") == head
              and r.get("path") == policy_workflow and isinstance(r.get("id"), int)
              and any(pr.get("number") == pull for pr in r.get("pull_requests") or [] if isinstance(pr, dict))]
    if not policy:
        return f"nothing re-run: no {policy_workflow} run of #{pull} exists at {head}; the first one judges itself"
    newest = max(policy, key=lambda r: (r.get("run_number", 0), r["id"]))
    if newest.get("status") != "completed":
        return (f"nothing re-run: policy run {newest['id']} is {newest.get('status')}; "
                f"it judges completed run {run_id} itself")
    jobs = [j for j in api.jobs(newest["id"]) if j.get("name") == job_name and isinstance(j.get("id"), int)]
    if len(jobs) != 1:
        raise Skip(f"policy run {newest['id']} lists {len(jobs)} jobs named {job_name!r}")
    api.post(f"actions/jobs/{jobs[0]['id']}/rerun")
    return f"re-ran {job_name!r} (job {jobs[0]['id']} of policy run {newest['id']}) for #{pull} at {head}"


def rerun_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="policy-admission.py rerun", description=rerun.__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--run", type=int, required=True, help="the completed codeflow-ci run")
    parser.add_argument("--head", required=True, help="that run's head commit")
    parser.add_argument("--workflow", required=True, help="the codeflow-ci workflow file")
    parser.add_argument("--policy-workflow", required=True, help="the workflow file of the commit standards job")
    parser.add_argument("--job", required=True, help="the job to re-run")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args(argv)
    api = Writer(args.api, args.repo, os.environ.get("GH_TOKEN", ""))
    try:
        print(f"policy-admission rerun: {rerun(args.run, args.head, args.workflow, args.policy_workflow, args.job, api)}")
    except Skip as reason:
        print(f"policy-admission rerun: refused: {reason}")
        return 1
    except Exception as error:  # noqa: BLE001 - any failure re-runs nothing
        print(f"policy-admission rerun: refused: the re-run failed ({type(error).__name__}: {error})")
        return 1
    return 0


def main() -> int:
    commands = {"admit": admit_main, "rerun": rerun_main}
    if sys.argv[1:2] and sys.argv[1] in commands:
        return commands[sys.argv[1]](sys.argv[2:])
    print(__doc__.split("Usage:", 1)[1].strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())

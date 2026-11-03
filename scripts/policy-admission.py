#!/usr/bin/env python3
"""Prepare trusted policy inputs and publish exact-head admission (TSK-1239).

This file and ci-reuse.py execute from the workflow's default-branch commit,
never from a pull request checkout. No artifact is downloaded or interpreted.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("ci_reuse", Path(__file__).with_name("ci-reuse.py"))
reuse = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reuse)
WORKFLOW = ".github/workflows/codeflow-ci.yml"
REQUIRED = {"codeflow gates", "windows"}
CHECK_NAME = "commit standards"


def digest(body: str | None) -> str:
    if body is not None and not isinstance(body, str):
        raise reuse.Skip("the pull request body is unreadable")
    # Match jq -r, including the newline written to the body file.
    return hashlib.sha256(((body or "") + "\n").encode()).hexdigest()


def event_identity(event_name: str, event: dict, repo: str) -> dict:
    if event.get("repository", {}).get("full_name") != repo:
        raise reuse.Skip("the event belongs to another repository")
    if event_name == "pull_request_target":
        pull = event["pull_request"]
        identity = {"pull": event["number"], "head": pull["head"]["sha"],
                    "base": pull["base"]["ref"]}
    elif event_name == "workflow_run":
        run = event["workflow_run"]
        if run.get("event") != "pull_request" or run.get("path") != WORKFLOW or run.get("status") != "completed":
            raise reuse.Skip("the completion is not this CI workflow's pull request run")
        pulls = run.get("pull_requests") or []
        if len(pulls) != 1:
            raise reuse.Skip("the completion has no unique pull request identity")
        identity = {"pull": pulls[0]["number"], "head": run["head_sha"],
                    "base": pulls[0]["base"]["ref"], "run": run["id"],
                    "attempt": run["run_attempt"], "conclusion": run["conclusion"]}
        if any(type(identity[key]) is not int or identity[key] < 1 for key in ("run", "attempt")):
            raise reuse.Skip("the completion has no valid run and attempt")
    else:
        raise reuse.Skip("this event does not request pull request admission")
    if type(identity["pull"]) is not int or identity["pull"] < 1 or not reuse.SHA.fullmatch(identity["head"]):
        raise reuse.Skip("the event has no valid pull request and head")
    if not isinstance(identity["base"], str) or not identity["base"]:
        raise reuse.Skip("the event has no target branch")
    return identity


def read_current(api, identity: dict) -> dict:
    pull = api.pull(identity["pull"])
    if (pull.get("number") != identity["pull"]
            or pull.get("base", {}).get("repo", {}).get("full_name") != api.repo
            or pull.get("head", {}).get("sha") != identity["head"]
            or pull.get("base", {}).get("ref") != identity["base"]):
        raise reuse.Skip("the pull request identity, head or target has changed")
    if "body" not in pull:
        raise reuse.Skip("the current body is missing")
    digest(pull["body"])
    return pull


def read_run(api, identity: dict) -> dict:
    run = reuse.newest_run(identity["head"], WORKFLOW, api, identity["pull"])
    if "run" in identity:
        direct = api.get(f"actions/runs/{identity['run']}")
        for key, expected in (("id", identity["run"]), ("run_attempt", identity["attempt"]),
                              ("head_sha", identity["head"]), ("conclusion", identity["conclusion"]),
                              ("event", "pull_request"), ("path", WORKFLOW), ("status", "completed")):
            if direct.get(key) != expected or run.get(key) != expected:
                raise reuse.Skip("the completion is stale or another attempt now decides")
    # A PR edit can retarget the same head. Keep the old CI identity here so
    # admit rejects its target and publication replaces an earlier green with
    # failure; silently returning would leave that earlier check green.
    return run


def prepare(api, identity: dict, directory: Path, actor: str = "unknown") -> dict:
    pull = read_current(api, identity)
    if "run" in identity:
        read_run(api, identity)
    base_sha = pull["base"]["sha"]
    if not reuse.SHA.fullmatch(base_sha):
        raise reuse.Skip("the target has no commit identity")
    fields = {"pull": str(identity["pull"]), "head": identity["head"], "base": identity["base"],
              "base_sha": base_sha, "head_ref": pull["head"]["ref"], "digest": digest(pull["body"]),
              "actor": actor if pull["head"].get("repo", {}).get("full_name") == api.repo else "unknown"}
    # Branch names enter step outputs and later env values, never shell code.
    if any(not isinstance(value, str) or "\n" in value or "\r" in value for value in fields.values()):
        raise reuse.Skip("a policy input cannot be represented as one step-output line")
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "pr-body.md").write_text((pull["body"] or "") + "\n", encoding="utf-8")
    return fields


def publish(reader, writer, identity: dict, judged_digest: str, validation: str, details_url: str) -> str:
    """Publish only authenticated current identities; stale/unknown data writes nothing.

    A known current failed/pending CI run writes failure. API errors propagate.
    All policy invocations for a PR are serialized by the workflow queue.
    """
    pull = read_current(reader, identity)
    try:
        run = read_run(reader, identity)
    except reuse.Pending:
        if "run" in identity:
            raise
        run = None
    external_id = f"codeflow-policy:pull:{identity['pull']}"
    checks = [check for check in reader.pages(
        f"commits/{identity['head']}/check-runs?check_name=commit%20standards&filter=all", "check_runs")
        if check.get("name") == CHECK_NAME and check.get("external_id") == external_id
        and check.get("app", {}).get("slug") == "github-actions"]
    if len(checks) > 1:
        raise reuse.Skip("more than one admission check owns this pull request head")
    before_write = read_current(reader, identity)
    if (before_write.get("state"), before_write.get("draft"), digest(before_write["body"])) != (
            pull.get("state"), pull.get("draft"), digest(pull["body"])):
        raise reuse.Skip("the pull request changed before check initialization")
    pending = {"name": CHECK_NAME, "external_id": external_id, "status": "in_progress",
               "details_url": details_url,
               "output": {"title": "Checking current admission", "summary": "Current policy and CI identities are being rechecked."}}
    if checks:
        check_id = checks[0]["id"]
        writer.write(f"check-runs/{check_id}", pending, "PATCH")
    else:
        created = writer.write("check-runs", {**pending, "head_sha": identity["head"]}, "POST")
        check_id = created["id"]
    if type(check_id) is not int or check_id < 1:
        raise reuse.Skip("the check API returned no check identity")
    # Errors after initialization leave a pending check, never an older green.
    conclusion = "failure"
    if (validation == "success" and pull.get("state") == "open" and pull.get("draft") is False
            and digest(pull["body"]) == judged_digest):
        try:
            reuse.admit(identity["head"], WORKFLOW, REQUIRED, reader, identity["pull"], identity["base"],
                        identity.get("run"), identity.get("attempt"))
        except reuse.Skip:
            pass
        else:
            conclusion = "success"
    # Recheck after all reads and immediately before the only write. GitHub
    # has no atomic compare-and-publish operation; later events judge changes
    # made after this last read, as with the existing policy final recheck.
    latest = read_current(reader, identity)
    if (latest.get("state"), latest.get("draft"), digest(latest["body"])) != (
            pull.get("state"), pull.get("draft"), digest(pull["body"])):
        raise reuse.Skip("the pull request changed before check publication")
    try:
        final_run = read_run(reader, identity)
    except reuse.Pending:
        if run is not None or "run" in identity:
            raise
        final_run = None
    if final_run != run:
        raise reuse.Skip("the run changed before check publication")
    evidence = "No current CI run" if run is None else f"CI run {run['id']}, attempt {run['run_attempt']}"
    payload = {"name": CHECK_NAME, "external_id": external_id, "status": "completed",
               "conclusion": conclusion, "details_url": details_url,
               "output": {"title": f"Pull request admission: {conclusion}",
                          "summary": f"{evidence}. Current PR identity, body, policy validation and named CI verdicts must all pass."}}
    writer.write(f"check-runs/{check_id}", payload, "PATCH")
    return conclusion


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("prepare", "publish"))
    args = parser.parse_args()
    repo = os.environ["GITHUB_REPOSITORY"]
    url = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    token = os.environ.get("GH_TOKEN", "")
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        identity = event_identity(os.environ["GITHUB_EVENT_NAME"], event, repo)
        # This repository is public. The publisher deliberately reads public
        # metadata without authentication; its token has no Actions/PR scopes.
        reader = reuse.Api(url, repo, "" if args.mode == "publish" else token)
        if args.mode == "prepare":
            # Only a pull_request_target event supplies the policy actor.
            # Rerunning CI cannot borrow that actor's automation exceptions.
            actor = event.get("sender", {}).get("login", "unknown") if os.environ["GITHUB_EVENT_NAME"] == "pull_request_target" else "unknown"
            fields = prepare(reader, identity, Path(os.environ["RUNNER_TEMP"]), actor)
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                for key, value in fields.items():
                    output.write(f"{key}={value}\n")
            print(f"current body judged: sha256 {fields['digest']}")
        else:
            details_url = f"{os.environ['GITHUB_SERVER_URL']}/{repo}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
            result = publish(reader, reuse.Api(url, repo, token), identity,
                             os.environ.get("JUDGED_DIGEST", ""), os.environ["VALIDATION_RESULT"], details_url)
            print(f"exact-head admission published: {result}")
            return 0 if result == "success" else 1
    except Exception:
        # API payloads and body text must never enter logs or step outputs.
        print("policy admission refused: cannot establish current PR, target and CI attempt identities")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

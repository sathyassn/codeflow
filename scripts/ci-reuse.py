#!/usr/bin/env python3
"""Decide whether a push to main reuses a pull request run's verdict (TSK-265).

A pull request run of codeflow-ci tests the merge of its head into main. When
the merge commit a push brings has exactly the tree of that head, and the
head's codeflow-ci pull request run passed every job, the push run would test
the same tree again. The operator approved reusing that verdict on
2026-10-11, only on an exact match. The push run then skips the gate and
Windows jobs, and its `codeflow gates` and `windows` verdicts pass and name
the reused run.

Reuse needs every one of these, read from git objects and GitHub's records,
never from a pull request's branch name or other text it controls:

1. the event is a push to `refs/heads/main`, the ref GitHub records for the
   push; a push to master, an integration line or any other ref runs every
   job;
2. exactly one merged pull request has the pushed commit as its merge commit;
3. the pushed commit and the pull request head have the same tree id;
4. the pushed commit's first parent (main before the merge) is an ancestor of
   the head, so every pull request run against main tested the head's own
   tree: the merge of a head into a base the head already contains is the
   head, and main only moves forward;
5. the push changes none of the build cache inputs (the toolchain, the cargo
   and npm lock files, the manifests, and this workflow, which names the
   cache keys and what each job builds), so a push that invalidates the
   caches, or adds a job whose cache main lacks, still runs and saves them on
   main for later pull requests;
6. main already holds the present part's apt cache for this runner image
   and the locked Playwright version, keyed exactly as the workflow keys it.
   That key names the runner image and has no fallback, and only a run on
   main saves the copy later pull requests restore; when it is missing (a
   new runner image), this push runs every job so the present part saves it;
7. the newest codeflow-ci run with event pull_request, that head commit and
   this workflow file names the merged pull request with base main,
   concluded success, and every job of its latest attempt
   concluded success, apart from the jobs that only run on a push (this
   decision), which must be skipped; the named verdicts must be among them.

Anything else, an API or git error included, runs every job.

Usage: ci-reuse.py --repo OWNER/NAME --sha SHA --event EVENT --workflow-ref REF --ref REF
                   --image-os OS --image-version VERSION
                   --push-only NAME --require NAME [--require NAME ...]

`ci-reuse.py admit --repo OWNER/NAME --head SHA --pull N --workflow PATH --trusted-file PATH ...
--require NAME [--wait SECONDS]` is the merge admission `commit standards` runs (TSK-449,
TSK-1001): it reads pull request N live and passes only when every trusted
file hashes the same at its head as at its base commit, and the newest
pull_request run of PATH at exactly that head, titled by the trusted
workflow for N and that base branch and commit, completed with success and
every named verdict job succeeded. It waits up to SECONDS for a pending or
absent run and refuses everything else.

`ci-reuse.py edit --repo OWNER/NAME --head SHA --pull N --base-ref REF --base-sha SHA
--workflow-ref REF --run-id ID --require NAME ...` decides, for a run an
`edited` event started without changing the base, whether the newest earlier
run of the same pull request, head and base passed, so its verdicts stand.
"""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

SHA = re.compile(r"^[0-9a-f]{40}$")
MAIN = "refs/heads/main"
# Paths whose change invalidates a build cache that later pull request runs
# restore from main (rust-cache keys on the toolchain, the cargo config and
# every Cargo.lock and Cargo.toml; setup-node keys on the npm lock files).
# The workflow names the cache keys and what each job builds into them, so
# a change to it can add a cache main does not hold yet (TSK-270: the
# coverage shards' cache).
CACHE_INPUTS = ("rust-toolchain", "rust-toolchain.toml", ".cargo/config.toml",
                "*Cargo.toml", "*Cargo.lock", "*package-lock.json", "*.node-version",
                ".github/workflows/codeflow-ci.yml")


# The Playwright version the present part reads, and the lock it must equal
# (the workflow's "Read the locked Playwright version" step).
PRESENT_LOCK = "crates/codeflow-present/web/package-lock.json"
PORTAL_LOCK = "docs-portal/package-lock.json"


class Skip(Exception):
    """A reason the push runs every job."""


def cache_input(path: str) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in CACHE_INPUTS)


def playwright_version(root: Path) -> str:
    """The locked playwright-core version, as the workflow reads it."""
    versions = []
    for lock in (PRESENT_LOCK, PORTAL_LOCK):
        try:
            packages = json.loads((root / lock).read_text(encoding="utf-8"))["packages"]
            versions.append(packages["node_modules/playwright-core"]["version"])
        except (OSError, ValueError, KeyError, TypeError) as error:
            raise Skip(f"cannot read the locked Playwright version from {lock}: {error}") from error
    if versions[0] != versions[1]:
        raise Skip(f"playwright-core differs: present {versions[0]}, portal {versions[1]}")
    return versions[0]


def apt_cache_key(image_os: str, image_version: str, version: str) -> str:
    """The present part's apt cache key, built as codeflow-ci.yml builds it:
    `playwright-apt-${ImageOS:-unknown}-${ImageVersion:-unknown}-<version>`."""
    return f"playwright-apt-{image_os or 'unknown'}-{image_version or 'unknown'}-{version}"


def decide(event: str, sha: str, workflow_path: str, push_only: set[str], required: set[str], git, api,
           apt_key: str, ref: str) -> int:
    """The id of the pull request run whose verdict the push reuses; raises
    Skip with the reason when the push must run every job."""
    if event != "push":
        raise Skip(f"a {event} run never reuses a verdict")
    if ref != MAIN:
        raise Skip(f"only a push to {MAIN} reuses a verdict; {ref!r} runs every job")
    if not SHA.match(sha):
        raise Skip(f"the pushed commit {sha!r} is not a commit id")
    pulls = [pull for pull in api.pulls(sha)
             if pull.get("merged_at") and pull.get("merge_commit_sha") == sha]
    if len(pulls) != 1:
        raise Skip(f"{len(pulls)} merged pull requests have {sha} as their merge commit; reuse needs exactly one")
    pull = pulls[0]
    head = (pull.get("head") or {}).get("sha", "")
    if not SHA.match(head):
        raise Skip(f"pull request #{pull.get('number')} names no head commit")
    pushed_tree, head_tree = git.tree(sha), git.tree(head)
    if pushed_tree != head_tree:
        raise Skip(f"the pushed tree {pushed_tree} differs from the tree {head_tree} of pull request "
                   f"#{pull.get('number')} head {head}")
    parents = git.parents(sha)
    if not parents:
        raise Skip(f"{sha} has no parent")
    if not git.is_ancestor(parents[0], head):
        raise Skip(f"head {head} does not contain {parents[0]}, main before the merge, so a pull request "
                   "run may have tested a different merge")
    changed = sorted(path for path in git.changed(parents[0], sha) if cache_input(path))
    if changed:
        raise Skip(f"the push changes build cache inputs ({', '.join(changed)}); every job runs so main's "
                   "caches are saved for later pull requests")
    try:
        caches = api.caches(apt_key, ref)
    except Exception as error:  # noqa: BLE001 - a failed lookup runs every job
        raise Skip(f"the apt cache lookup failed ({type(error).__name__}: {error})") from error
    if not any(cache.get("key") == apt_key and cache.get("ref") == ref for cache in caches):
        raise Skip(f"{ref} holds no apt cache {apt_key!r}; every job runs so the present part saves it")
    runs = [run for run in api.runs(head)
            if run.get("event") == "pull_request" and run.get("head_sha") == head
            and run.get("path") == workflow_path and isinstance(run.get("id"), int)]
    if not runs:
        raise Skip(f"no {workflow_path} pull_request run exists for head {head}")
    run = max(runs, key=lambda r: (r.get("run_number", 0), r.get("run_attempt", 0), r["id"]))
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise Skip(f"run {run['id']} for head {head} is {run.get('status')} with conclusion {run.get('conclusion')}")
    if not any(pr.get("number") == pull["number"] and (pr.get("base") or {}).get("ref") == "main"
               for pr in run.get("pull_requests") or []):
        raise Skip(f"run {run['id']} is not associated with merged pull request #{pull['number']} into main")
    jobs = api.jobs(run["id"])
    if not jobs:
        raise Skip(f"run {run['id']} lists no jobs")
    for job in jobs:
        name, conclusion = job.get("name"), job.get("conclusion")
        expected = "skipped" if name in push_only else "success"
        if conclusion != expected:
            raise Skip(f"job {name!r} of run {run['id']} concluded {conclusion}; reuse needs {expected}")
    missing = sorted(required - {job.get("name") for job in jobs})
    if missing:
        raise Skip(f"run {run['id']} has no job named {', '.join(missing)}")
    return run["id"]


class Pending(Skip):
    """An admission that may still pass once the head's run finishes."""


# The title the trusted codeflow-ci workflow gives each pull request run
# (its `run-name`): the pull request and the base branch and commit the run
# tested. It is read only after the workflow file is proved to be the
# base's own, so the pull request cannot write it.
RUN_TITLE = re.compile(r"^codeflow-ci pull request #(\d+) into (\S+) at ([0-9a-f]{40})$")


def run_title(run: dict) -> tuple[int, str, str] | None:
    """The pull request number, base branch and base commit a run's title
    records, or None when the title is missing or not that form."""
    match = RUN_TITLE.match(run.get("display_title") or "")
    if not match:
        return None
    return int(match.group(1)), match.group(2), match.group(3)


def file_digest(git, revision: str, path: str) -> str:
    """The SHA-256 of `path` as committed at `revision`; raises Skip when it
    cannot be read."""
    return hashlib.sha256(git.show(revision, path)).hexdigest()


def trusted_files_problem(git, base: str, head: str, paths: list[str]) -> str | None:
    """Why the head's copy of a trusted file is not the base's, or None.
    The run tested the merge of the head into the base; a file whose bytes
    are equal at both carries the base's content into that merge, so the
    run's jobs, names and verdict logic are the base's, never the pull
    request's."""
    if not paths:
        return "no trusted file was named; admission binds the workflow file and the verdict script"
    for path in paths:
        try:
            trusted, candidate = file_digest(git, base, path), file_digest(git, head, path)
        except Skip as error:
            return f"cannot read {path} at the base {base} and the head {head}: {error}"
        if trusted != candidate:
            return (f"the pull request changes {path} (base sha256 {trusted}, head sha256 {candidate}), so its "
                    "run's jobs and verdicts come from a file the pull request controls; land that change through "
                    "a human-merged pull request whose review covers the workflow, or revert it here")
    return None


def matching_runs(api, head: str, workflow_path: str, pull: int, base_ref: str, base_sha: str,
                  exclude: int | None = None) -> list[dict]:
    """The pull_request runs of `workflow_path` at exactly `head` whose
    trusted title names pull request `pull` and the base `base_ref` at
    `base_sha`."""
    found = []
    for run in api.runs(head):
        if not (run.get("event") == "pull_request" and run.get("head_sha") == head
                and run.get("path") == workflow_path and isinstance(run.get("id"), int)
                and run.get("id") != exclude):
            continue
        if run_title(run) == (pull, base_ref, base_sha):
            found.append(run)
    return found


def newest(runs: list[dict]) -> dict:
    return max(runs, key=lambda r: (r.get("run_number", 0), r.get("run_attempt", 0), r["id"]))


def verdicts_problem(api, run: dict, required: set[str]) -> str | None:
    jobs = {job.get("name"): job.get("conclusion") for job in api.jobs(run["id"])}
    for name in sorted(required):
        if jobs.get(name) != "success":
            return (f"verdict {name!r} of run {run['id']} concluded {jobs.get(name) or 'nothing'}; "
                    "a draft's verdicts are skipped, so mark the pull request ready to run them at this head")
    return None


def admit(head: str, workflow_path: str, required: set[str], api, pull: int, git=None,
          trusted: list[str] | None = None) -> int:
    """Merge admission for a pull request (TSK-449 AC-6, TSK-1001): the id
    of the newest codeflow-ci pull_request run that tested exactly this
    pull request's current head against its current base, when it
    completed with success and every required verdict job succeeded.

    Every input is a trusted identity, never a name the pull request sets:
    the pull request is read live (its head and base commits and its base
    branch); the workflow file and the verdict script must hash the same at
    the head as at the base (a pull request that edits them would choose its
    own jobs); and the run must carry the title the trusted workflow writes,
    naming this pull request, base branch and base commit, so a run made
    for another base never admits after a retarget. Anything missing,
    unreadable or different refuses. A draft's run skips its verdicts
    (neutral), so it never admits; only a pending or absent run is worth
    waiting for."""
    if not SHA.match(head):
        raise Skip(f"the head {head!r} is not a commit id")
    live = api.pull(pull)
    live_head = (live.get("head") or {}).get("sha", "")
    base = live.get("base") or {}
    base_ref, base_sha = base.get("ref") or "", base.get("sha") or ""
    if live.get("state") != "open":
        raise Skip(f"pull request #{pull} is {live.get('state')}, not open")
    if live_head != head:
        raise Skip(f"pull request #{pull} is now at head {live_head or 'unknown'}, not {head}; this check belongs "
                   "to an older event, so re-run it for the current head")
    if not base_ref or not SHA.match(base_sha):
        raise Skip(f"pull request #{pull} names no base branch and commit ({base_ref!r} at {base_sha!r})")
    problem = trusted_files_problem(git, base_sha, head, list(trusted or [])) if git else \
        "no git reader was given, so the workflow file cannot be proved to be the base's"
    if problem:
        raise Skip(problem)
    runs = matching_runs(api, head, workflow_path, pull, base_ref, base_sha)
    if not runs:
        raise Pending(f"no {workflow_path} pull_request run of #{pull} at head {head} tested the base {base_ref} "
                      f"at {base_sha}; a retarget or a base change starts one")
    run = newest(runs)
    if run.get("status") != "completed":
        raise Pending(f"run {run['id']} for head {head} is {run.get('status')}")
    if run.get("conclusion") != "success":
        raise Skip(f"run {run['id']} for head {head} concluded {run.get('conclusion')}")
    problem = verdicts_problem(api, run, required)
    if problem:
        raise Skip(problem)
    return run["id"]


def edit_reuse(head: str, workflow_path: str, required: set[str], api, pull: int, base_ref: str,
               base_sha: str, current: int) -> int:
    """For a pull request run started by an `edited` event that kept the
    base (a title or body edit), the id of the newest earlier run of the
    same pull request, head and base, when it completed with success and
    every required verdict succeeded; its verdicts stand and every gate job
    is skipped. Raises Skip, and every gate runs, otherwise: no such run,
    or a newest one that failed, was cancelled or is still running."""
    if not SHA.match(head) or not SHA.match(base_sha) or not base_ref:
        raise Skip(f"the head {head!r} or the base {base_ref!r} at {base_sha!r} is not readable")
    runs = matching_runs(api, head, workflow_path, pull, base_ref, base_sha, exclude=current)
    if not runs:
        raise Skip(f"no earlier run of #{pull} at head {head} tested the base {base_ref} at {base_sha}")
    run = newest(runs)
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise Skip(f"the newest earlier run {run['id']} is {run.get('status')} with conclusion "
                   f"{run.get('conclusion')}")
    problem = verdicts_problem(api, run, required)
    if problem:
        raise Skip(problem)
    return run["id"]


def edit_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="ci-reuse.py edit", description=edit_reuse.__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--pull", type=int, required=True)
    parser.add_argument("--base-ref", required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--run-id", type=int, required=True, help="this run, never reused")
    parser.add_argument("--require", action="append", default=[], help="a verdict job that must have succeeded")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args(argv)
    try:
        path = workflow_path(args.workflow_ref, args.repo)
        run = edit_reuse(args.head, path, set(args.require), Api(args.api, args.repo, os.environ.get("GH_TOKEN", "")),
                         args.pull, args.base_ref, args.base_sha, args.run_id)
    except Skip as reason:
        print(f"ci-reuse edit: every job runs: {reason}")
        return 0
    except Exception as error:  # noqa: BLE001 - any failure runs every job
        print(f"ci-reuse edit: every job runs: the decision failed ({type(error).__name__}: {error})")
        return 0
    print(f"ci-reuse edit: reusing run {run}, which tested this head against this base")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"run={run}\n")
    return 0


def admit_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="ci-reuse.py admit", description=admit.__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--pull", type=int, required=True, help="the pull request the run must belong to")
    parser.add_argument("--workflow", required=True, help="the workflow file whose run must pass")
    parser.add_argument("--trusted-file", action="append", default=[],
                        help="a file that must hash the same at the head as at the base")
    parser.add_argument("--require", action="append", default=[], help="a verdict job that must succeed")
    parser.add_argument("--wait", type=int, default=0, help="seconds to wait for a pending or absent run")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args(argv)
    api = Api(args.api, args.repo, os.environ.get("GH_TOKEN", ""))
    deadline = time.monotonic() + max(args.wait, 0)
    while True:
        try:
            run = admit(args.head, args.workflow, set(args.require), api, args.pull, Git(), args.trusted_file)
        except Pending as reason:
            if time.monotonic() + POLL_SECONDS > deadline:
                print(f"ci-reuse admit: refused: {reason}; re-run this check once that run completes")
                return 1
            print(f"ci-reuse admit: waiting: {reason}")
            time.sleep(POLL_SECONDS)
            continue
        except Skip as reason:
            print(f"ci-reuse admit: refused: {reason}")
            return 1
        except Exception as error:  # noqa: BLE001 - any failure refuses admission
            print(f"ci-reuse admit: refused: the decision failed ({type(error).__name__}: {error})")
            return 1
        print(f"ci-reuse admit: run {run} passed every verdict at head {args.head} against its current base")
        return 0


POLL_SECONDS = 30


class Git:
    def __init__(self, cwd: str = "."):
        self.cwd = cwd

    def run(self, *args: str) -> str:
        done = subprocess.run(["git", *args], cwd=self.cwd, capture_output=True, text=True, check=False)
        if done.returncode != 0:
            raise Skip(f"git {' '.join(args)} failed: {done.stderr.strip()}")
        return done.stdout.strip()

    def ensure(self, commit: str) -> None:
        present = subprocess.run(["git", "cat-file", "-e", f"{commit}^{{commit}}"], cwd=self.cwd,
                                 capture_output=True, check=False)
        if present.returncode != 0:
            self.run("fetch", "--no-tags", "--filter=blob:none", "origin", commit)

    def tree(self, commit: str) -> str:
        self.ensure(commit)
        return self.run("rev-parse", "--verify", f"{commit}^{{tree}}")

    def parents(self, commit: str) -> list[str]:
        self.ensure(commit)
        return self.run("rev-list", "--parents", "-n", "1", commit).split()[1:]

    def is_ancestor(self, ancestor: str, commit: str) -> bool:
        done = subprocess.run(["git", "merge-base", "--is-ancestor", ancestor, commit], cwd=self.cwd,
                              capture_output=True, check=False)
        if done.returncode not in (0, 1):
            raise Skip(f"git merge-base failed: {done.stderr.decode(errors='replace').strip()}")
        return done.returncode == 0

    def show(self, commit: str, path: str) -> bytes:
        """The bytes of `path` as committed at `commit`."""
        self.ensure(commit)
        done = subprocess.run(["git", "show", f"{commit}:{path}"], cwd=self.cwd, capture_output=True, check=False)
        if done.returncode != 0:
            raise Skip(f"git show {commit}:{path} failed: {done.stderr.decode(errors='replace').strip()}")
        return done.stdout

    def changed(self, base: str, commit: str) -> list[str]:
        return [line for line in self.run("diff", "--name-only", "--no-renames", base, commit).splitlines() if line]


class Api:
    def __init__(self, url: str, repo: str, token: str):
        self.url, self.repo, self.token = url.rstrip("/"), repo, token

    def get(self, path: str) -> dict | list:
        request = urllib.request.Request(f"{self.url}/repos/{self.repo}/{path}", headers={
            "Accept": "application/vnd.github+json", "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28"})
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)

    def pages(self, path: str, key: str | None) -> list[dict]:
        found: list[dict] = []
        for page in range(1, 11):
            body = self.get(f"{path}{'&' if '?' in path else '?'}per_page=100&page={page}")
            items = body if key is None else body.get(key, [])
            found.extend(items)
            total = None if key is None else body.get("total_count")
            if len(items) < 100 or (total is not None and len(found) >= total):
                if total is not None and len(found) != total:
                    raise Skip(f"{path} listed {len(found)} of {total} entries")
                return found
        raise Skip(f"{path} has more than 1000 entries")

    def pulls(self, sha: str) -> list[dict]:
        return self.pages(f"commits/{sha}/pulls", None)

    def pull(self, number: int) -> dict:
        body = self.get(f"pulls/{number}")
        if not isinstance(body, dict):
            raise Skip(f"pulls/{number} did not return a pull request")
        return body

    def runs(self, head: str) -> list[dict]:
        return self.pages(f"actions/runs?head_sha={head}&event=pull_request", "workflow_runs")

    def jobs(self, run: int) -> list[dict]:
        return self.pages(f"actions/runs/{run}/jobs?filter=latest", "jobs")

    def caches(self, key: str, ref: str) -> list[dict]:
        query = urllib.parse.urlencode({"key": key, "ref": ref})
        return self.pages(f"actions/caches?{query}", "actions_caches")


def workflow_path(ref: str, repo: str) -> str:
    """The workflow file of `github.workflow_ref` (owner/name/path@ref)."""
    prefix = f"{repo}/"
    if not ref.startswith(prefix) or "@" not in ref:
        raise Skip(f"workflow ref {ref!r} does not name a workflow file of {repo}")
    return ref[len(prefix):].split("@", 1)[0]


def main() -> int:
    if sys.argv[1:2] == ["admit"]:
        return admit_main(sys.argv[2:])
    if sys.argv[1:2] == ["edit"]:
        return edit_main(sys.argv[2:])
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--event", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--ref", required=True, help="the pushed ref, whose caches later runs restore")
    parser.add_argument("--image-os", default="", help="the runner's ImageOS")
    parser.add_argument("--image-version", default="", help="the runner's ImageVersion")
    parser.add_argument("--push-only", action="append", default=[], help="a job that runs only on a push")
    parser.add_argument("--require", action="append", default=[], help="a verdict job the reused run must have")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args()
    try:
        path = workflow_path(args.workflow_ref, args.repo)
        key = apt_cache_key(args.image_os, args.image_version, playwright_version(Path.cwd()))
        run = decide(args.event, args.sha, path, set(args.push_only), set(args.require), Git(),
                     Api(args.api, args.repo, os.environ.get("GH_TOKEN", "")), key, args.ref)
    except Skip as reason:
        print(f"ci-reuse: every job runs: {reason}")
        return 0
    except Exception as error:  # noqa: BLE001 - any failure runs every job
        print(f"ci-reuse: every job runs: the decision failed ({type(error).__name__}: {error})")
        return 0
    print(f"ci-reuse: reusing pull request run {run}, which tested this exact tree")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"run={run}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

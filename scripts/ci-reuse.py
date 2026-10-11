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
   and npm lock files, the manifests), so a push that invalidates the caches
   still runs and saves them on main for later pull requests;
6. main already holds the present part's apt cache for this runner image
   and the locked Playwright version, keyed exactly as the workflow keys it.
   That key names the runner image and has no fallback, and only a run on
   main saves the copy later pull requests restore; when it is missing (a
   new runner image), this push runs every job so the present part saves it;
7. the newest codeflow-ci run with event pull_request, that head commit and
   this workflow file concluded success, and every job of its latest attempt
   concluded success, apart from the jobs that only run on a push (this
   decision), which must be skipped; the named verdicts must be among them.

Anything else, an API or git error included, runs every job.

Usage: ci-reuse.py --repo OWNER/NAME --sha SHA --event EVENT --workflow-ref REF --ref REF
                   --image-os OS --image-version VERSION
                   --push-only NAME --require NAME [--require NAME ...]
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

SHA = re.compile(r"^[0-9a-f]{40}$")
MAIN = "refs/heads/main"
# Paths whose change invalidates a build cache that later pull request runs
# restore from main (rust-cache keys on the toolchain, the cargo config and
# every Cargo.lock and Cargo.toml; setup-node keys on the npm lock files).
CACHE_INPUTS = ("rust-toolchain", "rust-toolchain.toml", ".cargo/config.toml",
                "*Cargo.toml", "*Cargo.lock", "*package-lock.json", "*.node-version")


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

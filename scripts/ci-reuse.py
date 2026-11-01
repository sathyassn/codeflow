#!/usr/bin/env python3
"""Decide whether a push to main or an integration line reuses a pull request
run's verdict (TSK-265, TSK-359).

A pull request run of codeflow-ci tests the merge of its head into its base. When
the merge commit a push brings has exactly the tree of that head, and the
head's codeflow-ci pull request run passed every job, the push run would test
the same tree again. The operator approved reusing that verdict on
2026-10-11 for main and on 2026-10-17 for integration lines, only on an
exact match. The push run then skips the gate and
Windows jobs, and its `codeflow gates` and `windows` verdicts pass and name
the reused run.

Reuse needs every one of these, read from git objects and GitHub's records,
never from a pull request's branch name or other text it controls:

1. the event is a push to `refs/heads/main` or `refs/heads/integration/**`,
   the ref GitHub records for the push; a push to master or any other ref
   runs every job;
2. exactly one merged pull request has the pushed commit as its merge commit,
   and its base is the pushed line;
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
6. the pushed line or main already holds the present part's apt cache for
   this runner image and the locked Playwright version, keyed exactly as the
   workflow keys it (a pull request restores from its base and from main).
   That key names the runner image and has no fallback; when it is missing
   (a new runner image), this push runs every job so the present part saves
   it;
7. the newest codeflow-ci run with event pull_request, that head commit and
   this workflow file that names the merged pull request (by number) with
   the pushed line as its base, concluded success, and every job of its
   latest attempt concluded success, apart from the jobs that only run on a
   push (this decision), which must be skipped, and the jobs a line push
   skips as well (the release parts), which may be skipped; the verdict jobs
   of the pushed workflow must be among them;
8. the run's own `ci-provenance` (Ubuntu) and `ci-provenance-windows`
   artifacts, written by the run's provenance jobs and listed under that
   run, each record the same pull request number, base line, head, merge
   tree (equal to the pushed tree), workflow file blob (equal to the pushed
   commit's) and runner image (ImageOS and exactly equal ImageVersion) as
   this push's Ubuntu reuse job and Windows image job, and a base commit
   that the head contains. A run without either artifact, or with any other
   value, is not reused.

Anything else, an API or git error included, runs every job.

Usage: ci-reuse.py --repo OWNER/NAME --sha SHA --event EVENT --workflow-ref REF --ref REF
                   --image-os OS --image-version VERSION
                   --push-only NAME --require NAME [--require NAME ...]

`ci-reuse.py admit --repo OWNER/NAME --head SHA --pull N --workflow PATH --require NAME
[--wait SECONDS]` is the merge admission `commit standards` runs (TSK-449):
it passes only when the newest pull_request run of PATH for pull request N
at exactly that head
completed with success and every named verdict job succeeded, waiting up to
SECONDS for a pending or absent run, and refuses everything else.
"""

from __future__ import annotations

import argparse
import fnmatch
import io
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

SHA = re.compile(r"^[0-9a-f]{40}$")
MAIN = "refs/heads/main"
LINE_PREFIX = "refs/heads/integration/"
PROVENANCE = "ci-provenance"
PROVENANCE_WINDOWS = "ci-provenance-windows"
PROVENANCE_FIELDS = ("pull", "base_ref", "base_commit", "head_commit", "merge_commit", "merge_tree",
                     "workflow", "workflow_blob", "image_os", "image_version")
MAX_PROVENANCE_BYTES = 65536
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


def line_of(ref: str) -> str:
    """The branch a reusable push names: main or an integration line."""
    if ref != MAIN and not (ref.startswith(LINE_PREFIX) and len(ref) > len(LINE_PREFIX)):
        raise Skip(f"only a push to {MAIN} or {LINE_PREFIX}* reuses a verdict; {ref!r} runs every job")
    return ref[len("refs/heads/"):]


def verdict_jobs(workflow_text: str) -> set[str]:
    """The names of the jobs the pushed workflow runs `always()`: its verdict
    jobs, which carry the reuse contract, so a verdict added later joins it."""
    names: set[str] = set()
    in_jobs, name, always = False, None, False

    def close() -> None:
        if name and always:
            names.add(name)

    for line in workflow_text.splitlines():
        if re.match(r"^\S", line):
            close()
            in_jobs, name, always = line.rstrip() == "jobs:", None, False
            continue
        if not in_jobs:
            continue
        if re.match(r"^  [A-Za-z0-9_-]+:\s*(#.*)?$", line):
            close()
            name, always = None, False
        elif (found := re.match(r"^    name:\s*(.+?)\s*$", line)):
            name = found.group(1).strip("\"'")
        elif re.match(r"^    if:\s*\$\{\{\s*always\(\)", line):
            always = True
    close()
    return names


def check_provenance(record: object, *, pull: int, line: str, head: str, tree: str, workflow: str, blob: str,
                     image: tuple[str, str], git) -> None:
    """Raises Skip unless the run's recorded provenance is the push's."""
    if not isinstance(record, dict) or set(record) != set(PROVENANCE_FIELDS) | {"schema"} or record["schema"] != 1:
        raise Skip("the run's provenance record is not the expected schema 1")
    expected = {"pull": pull, "base_ref": line, "head_commit": head, "merge_tree": tree, "workflow": workflow,
                "workflow_blob": blob, "image_os": image[0], "image_version": image[1]}
    for key, want in expected.items():
        if not want or record[key] != want:
            raise Skip(f"the run recorded {key} {record[key]!r}; this push needs {want!r}")
    for key in ("base_commit", "merge_commit"):
        if not isinstance(record[key], str) or not SHA.match(record[key]):
            raise Skip(f"the run recorded no valid {key}")
    if not git.is_ancestor(record["base_commit"], head):
        raise Skip(f"head {head} does not contain the base {record['base_commit']} the run merged with, so the "
                   "tested merge may differ from the head")


def read_provenance(api, run: dict, head: str, name: str = PROVENANCE):
    """The run's one unexpired artifact called `name`, parsed."""
    listed = [a for a in api.artifacts(run["id"]) if a.get("name") == name]
    if len(listed) != 1:
        raise Skip(f"run {run['id']} has {len(listed)} {name} artifacts; reuse needs exactly one")
    artifact = listed[0]
    owner = artifact.get("workflow_run") or {}
    if artifact.get("expired") or owner.get("id") != run["id"] or owner.get("head_sha") != head:
        raise Skip(f"the {name} artifact of run {run['id']} is expired or belongs to another run")
    if not isinstance(artifact.get("id"), int) or artifact.get("size_in_bytes", 0) > MAX_PROVENANCE_BYTES:
        raise Skip(f"the {name} artifact of run {run['id']} is missing an id or too large")
    try:
        with zipfile.ZipFile(io.BytesIO(api.artifact_zip(artifact["id"]))) as archive:
            info = archive.getinfo("provenance.json")
            if info.file_size > MAX_PROVENANCE_BYTES or len(archive.namelist()) != 1:
                raise Skip("the provenance archive holds more than provenance.json")
            return json.loads(archive.read(info))
    except Skip:
        raise
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        raise Skip(f"the {name} artifact of run {run['id']} is unreadable ({type(error).__name__})") from error


def decide(event: str, sha: str, workflow_path: str, push_only: set[str], required: set[str], git, api,
           apt_key: str, ref: str, images: dict[str, tuple[str, str]] | None = None,
           workflow_text: str | None = None,
           line_skips: frozenset[str] = frozenset()) -> int:
    """The id of the pull request run whose verdict the push reuses; raises
    Skip with the reason when the push must run every job."""
    if event != "push":
        raise Skip(f"a {event} run never reuses a verdict")
    line = line_of(ref)
    if not SHA.match(sha):
        raise Skip(f"the pushed commit {sha!r} is not a commit id")
    pulls = [pull for pull in api.pulls(sha)
             if pull.get("merged_at") and pull.get("merge_commit_sha") == sha]
    if len(pulls) != 1:
        raise Skip(f"{len(pulls)} merged pull requests have {sha} as their merge commit; reuse needs exactly one")
    pull = pulls[0]
    if (pull.get("base") or {}).get("ref") != line:
        raise Skip(f"pull request #{pull.get('number')} merged into {(pull.get('base') or {}).get('ref')!r}, "
                   f"not {line!r}")
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
        caches = api.caches(apt_key, ref) + (api.caches(apt_key, MAIN) if ref != MAIN else [])
    except Exception as error:  # noqa: BLE001 - a failed lookup runs every job
        raise Skip(f"the apt cache lookup failed ({type(error).__name__}: {error})") from error
    if not any(cache.get("key") == apt_key and cache.get("ref") in (ref, MAIN) for cache in caches):
        raise Skip(f"neither {ref} nor {MAIN} holds apt cache {apt_key!r}; every job runs so the present part "
                   "saves it")
    runs = [run for run in api.runs(head)
            if run.get("event") == "pull_request" and run.get("head_sha") == head
            and run.get("path") == workflow_path and isinstance(run.get("id"), int)
            and any(pr.get("number") == pull["number"] and (pr.get("base") or {}).get("ref") == line
                    for pr in run.get("pull_requests") or [])]
    if not runs:
        raise Skip(f"no {workflow_path} pull_request run of #{pull['number']} into {line} exists for head {head}")
    run = max(runs, key=lambda r: (r.get("run_number", 0), r.get("run_attempt", 0), r["id"]))
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise Skip(f"run {run['id']} for head {head} is {run.get('status')} with conclusion {run.get('conclusion')}")
    jobs = api.jobs(run["id"])
    if not jobs:
        raise Skip(f"run {run['id']} lists no jobs")
    skippable = line_skips if ref != MAIN else frozenset()
    for job in jobs:
        name, conclusion = job.get("name"), job.get("conclusion")
        if name in push_only:
            allowed = {"skipped"}
        elif name in skippable:
            allowed = {"success", "skipped"}
        else:
            allowed = {"success"}
        if conclusion not in allowed:
            raise Skip(f"job {name!r} of run {run['id']} concluded {conclusion}; reuse needs "
                       f"{' or '.join(sorted(allowed))}")
    if workflow_text is not None:
        pushed_verdicts = verdict_jobs(workflow_text)
        if not required <= pushed_verdicts:
            raise Skip("the pushed workflow no longer has the verdict job(s) "
                       f"{', '.join(sorted(required - pushed_verdicts))}")
        required = required | pushed_verdicts
    missing = sorted(required - {job.get("name") for job in jobs})
    if missing:
        raise Skip(f"run {run['id']} has no job named {', '.join(missing)}")
    for name in (PROVENANCE, PROVENANCE_WINDOWS):
        check_provenance(read_provenance(api, run, head, name), pull=pull["number"], line=line, head=head,
                         tree=pushed_tree, workflow=workflow_path, blob=git.blob(sha, workflow_path),
                         image=(images or {}).get(name, ("", "")), git=git)
    return run["id"]


class Pending(Skip):
    """An admission that may still pass once the head's run finishes."""


def admit(head: str, workflow_path: str, required: set[str], api, pull: int) -> int:
    """Merge admission for a pull request (TSK-449 AC-6): the id of the
    newest codeflow-ci pull_request run at exactly `head` when it completed
    with success and every required verdict job concluded success. A draft's
    run skips its verdicts (neutral), so it never admits; a pending, absent,
    cancelled or failed run, one only for another head, or one that names
    another pull request (a green run of the same commit elsewhere), refuses."""
    if not SHA.match(head):
        raise Skip(f"the head {head!r} is not a commit id")
    runs = [run for run in api.runs(head)
            if run.get("event") == "pull_request" and run.get("head_sha") == head
            and run.get("path") == workflow_path and isinstance(run.get("id"), int)
            and any(pr.get("number") == pull for pr in run.get("pull_requests") or [])]
    if not runs:
        raise Pending(f"no {workflow_path} pull_request run of #{pull} exists for head {head}")
    run = max(runs, key=lambda r: (r.get("run_number", 0), r.get("run_attempt", 0), r["id"]))
    if run.get("status") != "completed":
        raise Pending(f"run {run['id']} for head {head} is {run.get('status')}")
    if run.get("conclusion") != "success":
        raise Skip(f"run {run['id']} for head {head} concluded {run.get('conclusion')}")
    jobs = {job.get("name"): job.get("conclusion") for job in api.jobs(run["id"])}
    for name in sorted(required):
        if jobs.get(name) != "success":
            raise Skip(f"verdict {name!r} of run {run['id']} concluded {jobs.get(name) or 'nothing'}; "
                       "a draft's verdicts are skipped, so mark the pull request ready to run them at this head")
    return run["id"]


def admit_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="ci-reuse.py admit", description=admit.__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--pull", type=int, required=True, help="the pull request the run must belong to")
    parser.add_argument("--workflow", required=True, help="the workflow file whose run must pass")
    parser.add_argument("--require", action="append", default=[], help="a verdict job that must succeed")
    parser.add_argument("--wait", type=int, default=0, help="seconds to wait for a pending or absent run")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args(argv)
    api = Api(args.api, args.repo, os.environ.get("GH_TOKEN", ""))
    deadline = time.monotonic() + max(args.wait, 0)
    while True:
        try:
            run = admit(args.head, args.workflow, set(args.require), api, args.pull)
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
        print(f"ci-reuse admit: run {run} passed every verdict at head {args.head}")
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

    def blob(self, commit: str, path: str) -> str:
        self.ensure(commit)
        return self.run("rev-parse", "--verify", f"{commit}:{path}")

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

    def artifacts(self, run: int) -> list[dict]:
        return self.pages(f"actions/runs/{run}/artifacts", "artifacts")

    def artifact_zip(self, artifact: int) -> bytes:
        """The artifact's archive. GitHub answers with a redirect to storage;
        the token goes to GitHub only, never to the storage host."""
        class Bare(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: ANN001
                if not newurl.startswith("https://"):
                    return None
                return urllib.request.Request(newurl, headers={"Accept": "application/octet-stream"})
        request = urllib.request.Request(f"{self.url}/repos/{self.repo}/actions/artifacts/{artifact}/zip", headers={
            "Accept": "application/vnd.github+json", "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28"})
        with urllib.request.build_opener(Bare).open(request, timeout=60) as response:
            data = response.read(MAX_PROVENANCE_BYTES + 1)
        if len(data) > MAX_PROVENANCE_BYTES:
            raise Skip("the provenance archive is too large")
        return data

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
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--event", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--ref", required=True, help="the pushed ref, whose caches later runs restore")
    parser.add_argument("--image-os", default="", help="the runner's ImageOS")
    parser.add_argument("--image-version", default="", help="the runner's ImageVersion")
    parser.add_argument("--windows-image-os", default="", help="the Windows runner's ImageOS")
    parser.add_argument("--windows-image-version", default="", help="the Windows runner's ImageVersion")
    parser.add_argument("--push-only", action="append", default=[], help="a job that runs only on a push")
    parser.add_argument("--require", action="append", default=[], help="a verdict job the reused run must have")
    parser.add_argument("--line-skip", action="append", default=[],
                        help="a job a push to an integration line skips as well, so its skip is no failure")
    parser.add_argument("--api", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    args = parser.parse_args()
    try:
        path = workflow_path(args.workflow_ref, args.repo)
        key = apt_cache_key(args.image_os, args.image_version, playwright_version(Path.cwd()))
        text = Path(path).read_text(encoding="utf-8")
        run = decide(args.event, args.sha, path, set(args.push_only), set(args.require), Git(),
                     Api(args.api, args.repo, os.environ.get("GH_TOKEN", "")), key, args.ref,
                     {PROVENANCE: (args.image_os, args.image_version),
                      PROVENANCE_WINDOWS: (args.windows_image_os, args.windows_image_version)}, text,
                     frozenset(args.line_skip))
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

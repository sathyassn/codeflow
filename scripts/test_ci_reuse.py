#!/usr/bin/env python3
"""Controls for scripts/ci-reuse.py (TSK-265 AC-4).

A push reuses a pull request run's verdict only on an exact match: one
merged pull request, the same tree, main before the merge inside the head,
no cache input changed, and the head's newest codeflow-ci pull_request run
green in every job. Each other case runs every job.
"""

import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("ci_reuse", ROOT / "scripts" / "ci-reuse.py")
reuse = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reuse)

PUSHED, HEAD, MAIN = "a" * 40, "b" * 40, "c" * 40
WORKFLOW = ".github/workflows/codeflow-ci.yml"
PUSH_ONLY = {"reuse a pull request verdict"}
MAIN_REF = "refs/heads/main"
APT_KEY = "playwright-apt-ubuntu24-20261005.1-1.56.1"
REQUIRED = {"codeflow gates", "windows"}


class FakeGit:
    def __init__(self, trees=None, parents=None, contains=True, changed=()):
        self.trees = trees or {PUSHED: "t1", HEAD: "t1"}
        self.parent_list = parents if parents is not None else [MAIN, HEAD]
        self.contains = contains
        self.changed_paths = list(changed)

    def tree(self, commit):
        return self.trees[commit]

    def parents(self, commit):
        return self.parent_list

    def is_ancestor(self, ancestor, commit):
        return self.contains

    def changed(self, base, commit):
        return self.changed_paths


def job(name, conclusion="success"):
    return {"name": name, "conclusion": conclusion}


GREEN_JOBS = [job("codeflow gates (coverage)"), job("codeflow gates"), job("windows tests (1 of 12)"),
              job("windows"), job("secret scan"), job("reuse a pull request verdict", "skipped")]


def run(**changes):
    record = {"id": 77, "run_number": 5, "run_attempt": 1, "event": "pull_request", "head_sha": HEAD,
              "path": WORKFLOW, "status": "completed", "conclusion": "success",
              "pull_requests": [{"number": 95, "base": {"ref": "main"}}]}
    record.update(changes)
    return record


class FakeApi:
    def __init__(self, pulls=None, runs=None, jobs=None, caches=None):
        self.cache_list = caches if caches is not None else [{"key": APT_KEY, "ref": MAIN_REF}]
        self.pull_list = pulls if pulls is not None else [
            {"number": 95, "merged_at": "2026-10-11T00:43:00Z", "merge_commit_sha": PUSHED, "head": {"sha": HEAD}}]
        self.run_list = runs if runs is not None else [run()]
        self.job_lists = jobs if jobs is not None else {77: GREEN_JOBS}

    def pulls(self, sha):
        return self.pull_list

    def runs(self, head):
        return self.run_list

    def jobs(self, run_id):
        return self.job_lists.get(run_id, [])

    def caches(self, key, ref):
        if isinstance(self.cache_list, Exception):
            raise self.cache_list
        return self.cache_list


def decide(event="push", git=None, api=None, ref=MAIN_REF):
    return reuse.decide(event, PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, git or FakeGit(), api or FakeApi(),
                        APT_KEY, ref)


class DecisionTests(unittest.TestCase):
    def skipped(self, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(**kwargs)
        return str(caught.exception)

    def test_an_exact_match_reuses_the_run(self):
        self.assertEqual(decide(), 77)

    def test_a_pull_request_run_never_reuses(self):
        self.assertIn("pull_request run never reuses", self.skipped(event="pull_request"))

    def test_a_push_to_any_ref_but_main_runs_everything(self):
        # Every other input matches, the cache included, so only the ref refuses.
        for ref in ("refs/heads/integration/EPC-1", "refs/heads/master", "refs/heads/main-old", "main"):
            with self.subTest(ref=ref):
                api = FakeApi(caches=[{"key": APT_KEY, "ref": ref}])
                self.assertIn("only a push to refs/heads/main", self.skipped(api=api, ref=ref))

    def test_a_different_tree_runs_everything(self):
        self.assertIn("differs", self.skipped(git=FakeGit(trees={PUSHED: "t1", HEAD: "t2"})))

    def test_a_head_without_main_before_the_merge_runs_everything(self):
        self.assertIn("does not contain", self.skipped(git=FakeGit(contains=False)))

    def test_a_push_that_changes_a_cache_input_runs_everything(self):
        for path in ("Cargo.lock", "crates/codeflow-cli/Cargo.toml", "rust-toolchain.toml",
                     ".cargo/config.toml", "docs-portal/package-lock.json", "docs-portal/.node-version",
                     ".github/workflows/codeflow-ci.yml"):
            with self.subTest(path=path):
                self.assertIn("cache inputs", self.skipped(git=FakeGit(changed=["README.md", path])))
        self.assertEqual(decide(git=FakeGit(changed=["README.md", "scripts/ci-reuse.py"])), 77)

    def test_no_pull_request_or_an_unmerged_or_other_one_runs_everything(self):
        for pulls in ([], [{"number": 1, "merged_at": None, "merge_commit_sha": PUSHED, "head": {"sha": HEAD}}],
                      [{"number": 1, "merged_at": "x", "merge_commit_sha": MAIN, "head": {"sha": HEAD}}]):
            with self.subTest(pulls=pulls):
                self.assertIn("reuse needs exactly one", self.skipped(api=FakeApi(pulls=pulls)))

    def test_a_failed_cancelled_running_or_missing_run_runs_everything(self):
        for runs in ([run(conclusion="failure")], [run(conclusion="cancelled")],
                     [run(status="in_progress", conclusion=None)], []):
            with self.subTest(runs=runs):
                self.skipped(api=FakeApi(runs=runs))

    def test_the_newest_run_decides_even_when_an_older_one_passed(self):
        runs = [run(), run(id=78, run_number=6, conclusion="failure")]
        self.assertIn("run 78", self.skipped(api=FakeApi(runs=runs)))

    def test_a_newer_success_for_another_pull_request_is_not_reused(self):
        runs = [run(), run(id=78, run_number=6,
                           pull_requests=[{"number": 96, "base": {"ref": "main"}}])]
        self.assertIn("merged pull request #95 into main",
                      self.skipped(api=FakeApi(runs=runs, jobs={77: GREEN_JOBS, 78: GREEN_JOBS})))

    def test_a_newer_success_into_integration_is_not_reused(self):
        runs = [run(), run(id=78, run_number=6,
                           pull_requests=[{"number": 95, "base": {"ref": "integration/epic"}}])]
        self.assertIn("merged pull request #95 into main",
                      self.skipped(api=FakeApi(runs=runs, jobs={77: GREEN_JOBS, 78: GREEN_JOBS})))

    def test_a_run_without_pull_request_associations_is_not_reused(self):
        for associations in ([], None):
            with self.subTest(associations=associations):
                self.assertIn("merged pull request #95 into main",
                              self.skipped(api=FakeApi(runs=[run(pull_requests=associations)])))

    def test_another_workflow_another_event_or_another_head_runs_everything(self):
        for other in (run(path=".github/workflows/codeflow-policy.yml"), run(event="push"),
                      run(event="pull_request_target"), run(head_sha=MAIN)):
            with self.subTest(run=other):
                self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run",
                              self.skipped(api=FakeApi(runs=[other])))

    def test_a_skipped_failed_or_missing_job_runs_everything(self):
        for jobs in ([*GREEN_JOBS[:2], job("windows tests (1 of 12)", "skipped"), *GREEN_JOBS[3:]],
                     [*GREEN_JOBS[:3], job("windows", "failure"), *GREEN_JOBS[4:]],
                     [*GREEN_JOBS[:5], job("reuse a pull request verdict", "success")],
                     [job for job in GREEN_JOBS if job["name"] != "windows"],
                     []):
            with self.subTest(jobs=jobs):
                self.skipped(api=FakeApi(jobs={77: jobs}))

    def test_main_holding_the_apt_cache_for_this_image_reuses(self):
        self.assertEqual(decide(api=FakeApi(caches=[{"key": "playwright-apt-ubuntu24-20260928.2-1.56.1",
                                                     "ref": MAIN_REF},
                                                    {"key": APT_KEY, "ref": MAIN_REF}])), 77)

    def test_a_missing_apt_cache_runs_everything(self):
        self.assertIn("holds no apt cache", self.skipped(api=FakeApi(caches=[])))

    def test_a_failed_apt_cache_lookup_runs_everything(self):
        self.assertIn("lookup failed", self.skipped(api=FakeApi(caches=OSError("HTTP 403"))))

    def test_an_apt_cache_for_another_image_or_ref_runs_everything(self):
        for cache in ({"key": "playwright-apt-ubuntu24-20260928.2-1.56.1", "ref": MAIN_REF},
                      {"key": APT_KEY + "0", "ref": MAIN_REF},
                      {"key": APT_KEY, "ref": "refs/pull/135/merge"}):
            with self.subTest(cache=cache):
                self.assertIn("holds no apt cache", self.skipped(api=FakeApi(caches=[cache])))

    def test_the_apt_key_is_built_as_the_workflow_builds_it(self):
        self.assertEqual(reuse.apt_cache_key("ubuntu24", "20261005.1", "1.56.1"), APT_KEY)
        self.assertEqual(reuse.apt_cache_key("", "", "1.56.1"), "playwright-apt-unknown-unknown-1.56.1")
        version = reuse.playwright_version(ROOT)
        self.assertRegex(version, r"^\d+\.\d+\.\d+")
        workflow = (ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text()
        self.assertIn('echo "image=${ImageOS:-unknown}-${ImageVersion:-unknown}" >> "$GITHUB_OUTPUT"', workflow)
        self.assertIn("key: playwright-apt-${{ steps.playwright.outputs.image }}-${{ steps.playwright.outputs.version }}",
                      workflow)
        self.assertIn("packages['node_modules/playwright-core'].version", workflow)

    def test_differing_or_unreadable_playwright_locks_run_everything(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(reuse.Skip):
                reuse.playwright_version(root)
            for lock, version in ((reuse.PRESENT_LOCK, "1.0.0"), (reuse.PORTAL_LOCK, "2.0.0")):
                (root / lock).parent.mkdir(parents=True)
                (root / lock).write_text(json.dumps(
                    {"packages": {"node_modules/playwright-core": {"version": version}}}))
            with self.assertRaises(reuse.Skip) as caught:
                reuse.playwright_version(root)
            self.assertIn("differs", str(caught.exception))

    def test_cache_inputs_match_only_their_files(self):
        self.assertTrue(reuse.cache_input("Cargo.toml"))
        self.assertFalse(reuse.cache_input("docs/Cargo.toml.md"))
        self.assertFalse(reuse.cache_input("scripts/release.py"))
        # TSK-270: the workflow names the cache keys, so a change to it can
        # add a cache main lacks; the policy workflow builds nothing.
        self.assertTrue(reuse.cache_input(".github/workflows/codeflow-ci.yml"))
        self.assertFalse(reuse.cache_input(".github/workflows/codeflow-policy.yml"))

    def test_the_workflow_path_comes_from_the_workflow_ref(self):
        self.assertEqual(reuse.workflow_path(f"sathyassn/codeflow/{WORKFLOW}@refs/heads/main", "sathyassn/codeflow"),
                         WORKFLOW)
        with self.assertRaises(reuse.Skip):
            reuse.workflow_path(f"someone/else/{WORKFLOW}@refs/heads/main", "sathyassn/codeflow")


class MainTests(unittest.TestCase):
    def main(self, decision):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            output.write_text("")
            argv = ["ci-reuse.py", "--repo", "sathyassn/codeflow", "--sha", PUSHED, "--event", "push",
                    "--workflow-ref", f"sathyassn/codeflow/{WORKFLOW}@refs/heads/main", "--ref", MAIN_REF,
                    "--image-os", "ubuntu24", "--image-version", "20261005.1",
                    "--push-only", "reuse a pull request verdict", "--require", "windows"]
            with mock.patch.object(reuse, "decide", side_effect=decision), mock.patch("sys.argv", argv), \
                    mock.patch.object(reuse, "playwright_version", return_value="1.56.1"), \
                    mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), redirect_stdout(io.StringIO()):
                code = reuse.main()
            return code, output.read_text()

    def test_reuse_writes_the_run_id(self):
        self.assertEqual(self.main(lambda *args: 77), (0, "run=77\n"))

    def test_a_reason_or_an_error_writes_nothing_and_never_fails(self):
        def refuse(*args):
            raise reuse.Skip("trees differ")

        def broken(*args):
            raise KeyError("head")
        self.assertEqual(self.main(refuse), (0, ""))
        self.assertEqual(self.main(broken), (0, ""))


class AdmissionTests(unittest.TestCase):
    """TSK-449 AC-6: `commit standards` admits a pull request only on a
    completed successful ready run at the exact head; each other case
    refuses, and only a pending or absent run is worth waiting for."""

    def admitted(self, api):
        return reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95)

    def refused(self, api, pending=False):
        with self.assertRaises(reuse.Skip) as caught:
            self.admitted(api)
        self.assertEqual(isinstance(caught.exception, reuse.Pending), pending, str(caught.exception))
        return str(caught.exception)

    def test_a_successful_ready_run_at_the_exact_head_admits(self):
        self.assertEqual(self.admitted(FakeApi()), 77)

    def test_a_draft_neutral_run_at_the_same_head_refuses(self):
        draft = [job("codeflow gates", "skipped"), job("windows", "skipped"), job("secret scan")]
        reason = self.refused(FakeApi(jobs={77: draft}))
        self.assertIn("'codeflow gates' of run 77 concluded skipped", reason)
        self.assertIn("mark the pull request ready", reason)

    def test_no_ready_run_refuses_and_may_be_waited_for(self):
        self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run of #95 exists",
                      self.refused(FakeApi(runs=[]), pending=True))

    def test_a_pending_run_refuses_and_may_be_waited_for(self):
        for status in ("queued", "in_progress", "waiting"):
            with self.subTest(status=status):
                api = FakeApi(runs=[run(status=status, conclusion=None)])
                self.assertIn(f"is {status}", self.refused(api, pending=True))

    def test_a_cancelled_or_failed_run_refuses(self):
        for conclusion in ("cancelled", "failure", "timed_out", "skipped"):
            with self.subTest(conclusion=conclusion):
                self.assertIn(f"concluded {conclusion}", self.refused(FakeApi(runs=[run(conclusion=conclusion)])))

    def test_a_green_run_on_another_head_refuses(self):
        api = FakeApi(runs=[run(head_sha=MAIN)])
        self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run", self.refused(api, pending=True))

    def test_a_green_run_at_the_same_head_for_another_pull_request_refuses(self):
        other = run(pull_requests=[{"number": 96, "base": {"ref": "main"}}])
        self.assertIn("run of #95 exists", self.refused(FakeApi(runs=[other]), pending=True))
        self.refused(FakeApi(runs=[run(pull_requests=[])]), pending=True)

    def test_another_workflow_or_event_never_admits(self):
        for changes in ({"path": ".github/workflows/other.yml"}, {"event": "push"}):
            with self.subTest(changes=changes):
                self.refused(FakeApi(runs=[run(**changes)]), pending=True)

    def test_the_newest_run_at_the_head_decides(self):
        # A ready run started after the draft's neutral run is still pending.
        api = FakeApi(runs=[run(), run(id=78, run_number=6, status="in_progress", conclusion=None)])
        self.assertIn("run 78", self.refused(api, pending=True))
        # A missing verdict job refuses.
        self.assertIn("'windows' of run 77 concluded nothing",
                      self.refused(FakeApi(jobs={77: [job("codeflow gates")]})))

    def test_main_waits_only_for_a_pending_run_and_fails_closed(self):
        def run_main(api, wait):
            out = io.StringIO()
            with mock.patch.object(reuse, "Api", return_value=api), \
                    mock.patch.object(reuse.time, "sleep") as sleep, redirect_stdout(out):
                code = reuse.admit_main(["--repo", "o/r", "--head", HEAD, "--workflow", WORKFLOW,
                                         "--require", "codeflow gates", "--require", "windows",
                                         "--pull", "95", "--wait", str(wait)])
            return code, out.getvalue(), sleep.call_count
        self.assertEqual(run_main(FakeApi(), 0)[0], 0)
        code, text, slept = run_main(FakeApi(runs=[run(status="in_progress", conclusion=None)]), 0)
        self.assertEqual((code, slept), (1, 0), text)
        code, text, slept = run_main(FakeApi(runs=[run(conclusion="failure")]), 600)
        self.assertEqual((code, slept), (1, 0), text)

        class Finishing(FakeApi):
            calls = 0

            def runs(self, head):
                Finishing.calls += 1
                return [run()] if Finishing.calls > 2 else [run(status="in_progress", conclusion=None)]
        with mock.patch.object(reuse.time, "monotonic", return_value=0.0):
            code, text, slept = run_main(Finishing(), 600)
        self.assertEqual((code, slept), (0, 2), text)

        class Broken(FakeApi):
            def runs(self, head):
                raise OSError("network down")
        code, text, _ = run_main(Broken(), 0)
        self.assertEqual(code, 1)
        self.assertIn("refused: the decision failed", text)


class GitTests(unittest.TestCase):
    """The git reader against a real repository: a merge of an up-to-date
    head has the head's tree, and main before the merge is in the head."""

    def test_trees_parents_ancestry_and_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false",
                                       *args], cwd=directory, check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q", "-b", "main")
            Path(directory, "a.txt").write_text("a\n")
            git("add", "a.txt")
            git("commit", "-q", "-m", "base")
            base = git("rev-parse", "HEAD")
            git("switch", "-q", "-c", "topic")
            Path(directory, "Cargo.lock").write_text("lock\n")
            git("add", "Cargo.lock")
            git("commit", "-q", "-m", "topic")
            head = git("rev-parse", "HEAD")
            git("switch", "-q", "main")
            git("merge", "-q", "--no-ff", "-m", "merge", "topic")
            merged = git("rev-parse", "HEAD")
            reader = reuse.Git(directory)
            self.assertEqual(reader.tree(merged), reader.tree(head))
            self.assertEqual(reader.parents(merged), [base, head])
            self.assertTrue(reader.is_ancestor(base, head))
            self.assertFalse(reader.is_ancestor(merged, head))
            self.assertEqual(reader.changed(base, merged), ["Cargo.lock"])


if __name__ == "__main__":
    unittest.main()

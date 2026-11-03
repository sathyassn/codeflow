#!/usr/bin/env python3
"""Controls for scripts/ci-reuse.py (TSK-265 AC-4).

A push reuses a pull request run's verdict only on an exact match: one
merged pull request, the same tree, main before the merge inside the head,
no cache input changed, and the head's newest codeflow-ci pull_request run
green in every job. Each other case runs every job.
"""

import importlib.util
import copy
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
        self.repo = "o/r"
        self.current = {"number": 95, "state": "open", "draft": False, "body": "current body",
                        "head": {"sha": HEAD}, "base": {"ref": "main", "repo": {"full_name": self.repo}}}
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

    def pull(self, number):
        return self.current

    def attempt_jobs(self, run_id, attempt):
        return self.jobs(run_id)

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
    refuses without sleeping. Current PR identity, target and attempt decide."""

    def admitted(self, api):
        return reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95, "main")

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
        self.assertIn("'codeflow gates' of run 77 did not uniquely succeed", reason)
        self.assertIn("mark the pull request ready", reason)

    def test_no_ready_run_refuses_immediately(self):
        self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run of #95 exists",
                      self.refused(FakeApi(runs=[]), pending=True))

    def test_a_pending_run_refuses_immediately(self):
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
        self.assertIn("'windows' of run 77 did not uniquely succeed",
                      self.refused(FakeApi(jobs={77: [job("codeflow gates")]})))

    def test_current_identity_head_target_and_readiness_are_required(self):
        for change in ({"number": 96}, {"state": "closed"}, {"draft": True},
                       {"draft": None}, {"head": {"sha": MAIN}},
                       {"base": {"ref": "other", "repo": {"full_name": "o/r"}}},
                       {"base": {"ref": "main", "repo": {"full_name": "other/repo"}}}):
            with self.subTest(change=change):
                api = FakeApi()
                api.current.update(change)
                self.refused(api)

    def test_a_body_edit_before_admission_uses_current_body(self):
        api = FakeApi()
        api.current["body"] = "body edited after CI began"
        self.assertEqual(self.admitted(api), 77)

    def test_body_or_pr_changes_during_admission_refuse(self):
        for change in ({"body": "new body"}, {"head": {"sha": MAIN}},
                       {"base": {"ref": "retarget", "repo": {"full_name": "o/r"}}},
                       {"draft": True}, {"state": "closed"}):
            with self.subTest(change=change):
                class Changing(FakeApi):
                    def attempt_jobs(self, run_id, attempt):
                        self.current = {**self.current, **change}
                        return super().attempt_jobs(run_id, attempt)
                self.refused(Changing())

    def test_a_success_on_the_previous_target_never_admits(self):
        api = FakeApi(runs=[run(pull_requests=[{"number": 95, "base": {"ref": "old"}}])])
        self.assertIn("another target", self.refused(api))

    def test_completion_must_name_the_latest_attempt(self):
        api = FakeApi(runs=[run(run_attempt=2)])
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95, "main", 77, 1)
        self.assertEqual(reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95, "main", 77, 2), 77)
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95, "main", 76, 2)

    def test_a_rerun_started_during_job_lookup_refuses(self):
        class Rerunning(FakeApi):
            def attempt_jobs(self, run_id, attempt):
                self.run_list = [run(run_attempt=2, status="in_progress", conclusion=None)]
                return GREEN_JOBS
        self.assertIn("changed", self.refused(Rerunning()))

    def test_a_new_run_started_during_job_lookup_refuses(self):
        class NewRun(FakeApi):
            def attempt_jobs(self, run_id, attempt):
                self.run_list = [run(), run(id=78, run_number=6)]
                return GREEN_JOBS
        self.assertIn("changed", self.refused(NewRun()))

    def test_jobs_are_read_for_the_exact_attempt(self):
        class Attempts(FakeApi):
            def attempt_jobs(self, run_id, attempt):
                # Latest attempt's skipped job must defeat an older green one.
                return GREEN_JOBS if attempt == 1 else [job("windows", "skipped")]
        self.refused(Attempts(runs=[run(run_attempt=2)]))

    def test_duplicate_verdicts_cannot_hide_a_failed_job(self):
        self.refused(FakeApi(jobs={77: [*GREEN_JOBS, job("windows", "failure")]}))

    def test_admission_requires_named_verdicts(self):
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, set(), FakeApi(), 95, "main")

    def test_main_is_single_pass_and_api_errors_fail_closed(self):
        def run_main(api):
            out = io.StringIO()
            with mock.patch.object(reuse, "Api", return_value=api), redirect_stdout(out):
                code = reuse.admit_main(["--repo", "o/r", "--head", HEAD, "--workflow", WORKFLOW,
                                         "--require", "codeflow gates", "--require", "windows",
                                         "--pull", "95", "--base", "main"])
            return code, out.getvalue()
        self.assertEqual(run_main(FakeApi())[0], 0)
        self.assertEqual(run_main(FakeApi(runs=[run(status="in_progress", conclusion=None)]))[0], 1)
        self.assertEqual(run_main(FakeApi(runs=[]))[0], 1)
        self.assertEqual(run_main(FakeApi(runs=[run(conclusion="failure")]))[0], 1)
        for method in ("pull", "runs", "attempt_jobs"):
            with self.subTest(method=method):
                api = FakeApi()
                with mock.patch.object(api, method, side_effect=OSError("private response")):
                    code, text = run_main(api)
                self.assertEqual(code, 1)
                self.assertIn("cannot verify", text)
                self.assertNotIn("private response", text)

    def test_admission_contains_no_sleep_or_wait_option(self):
        import inspect
        source = inspect.getsource(reuse.admit_main)
        self.assertNotIn("sleep", source)
        self.assertNotIn("--wait", source)

    def test_consumer_template_baseline_and_manifest_keep_the_same_permissions(self):
        import hashlib
        template = (ROOT / "assets/base/ci/codeflow-policy.yml").read_bytes()
        baseline = (ROOT / ".codeflow/.baseline/.github/workflows/codeflow-policy.yml").read_bytes()
        manifest = json.loads((ROOT / ".codeflow/manifest.json").read_text())
        self.assertEqual(template, baseline)
        self.assertEqual(hashlib.sha256(template).hexdigest(),
                         manifest["files"][".github/workflows/codeflow-policy.yml"]["sha256"])
        self.assertIn(b"permissions: {}", template)
        self.assertIn(b"      contents: read\n      pull-requests: read", template)

    def test_consumer_template_has_no_admission_mechanism(self):
        template = (ROOT / "assets/base/ci/codeflow-policy.yml").read_text()
        for marker in ("ci-reuse.py", "workflow_run:", "checks: write", "check-runs"):
            self.assertNotIn(marker, template)


POLICY_SPEC = importlib.util.spec_from_file_location("policy_admission", ROOT / "scripts/policy-admission.py")
policy = importlib.util.module_from_spec(POLICY_SPEC)
POLICY_SPEC.loader.exec_module(policy)


class PolicyApi(FakeApi):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.current["base"]["sha"] = MAIN
        self.current["head"]["ref"] = "task/example"
        self.check_list = []
        self.writes = []

    def pull(self, number):
        return copy.deepcopy(self.current)

    def get(self, path):
        if path.startswith("actions/runs/"):
            return copy.deepcopy(self.run_list[0])
        raise AssertionError(path)

    def pages(self, path, key):
        if path.startswith(f"commits/{HEAD}/check-runs?") and key == "check_runs":
            return self.check_list
        raise AssertionError((path, key))

    def write(self, path, payload, method):
        self.writes.append((path, payload, method))
        return {"id": 100}


def policy_event(completion=True):
    api = PolicyApi()
    event = {"repository": {"full_name": "o/r"}, "number": 95,
             "pull_request": api.current, "workflow_run": run()}
    name = "workflow_run" if completion else "pull_request_target"
    return name, event


class PublisherTests(unittest.TestCase):
    def identity(self, completion=True):
        name, event = policy_event(completion)
        return policy.event_identity(name, event, "o/r")

    def publish(self, reader=None, writer=None, identity=None, result="success", body_digest=None):
        reader = reader or PolicyApi()
        writer = writer or reader
        identity = identity or self.identity()
        return policy.publish(reader, writer, identity,
                              body_digest or policy.digest(reader.current["body"]), result,
                              "https://github.com/o/r/actions/runs/99")

    def test_success_is_written_only_to_the_bound_head(self):
        api = PolicyApi()
        self.assertEqual(self.publish(api), "success")
        path, pending, method = api.writes[0]
        self.assertEqual((path, method), ("check-runs", "POST"))
        self.assertEqual(pending["head_sha"], HEAD)
        self.assertEqual(pending["status"], "completed")
        self.assertEqual(pending["conclusion"], "failure")
        payload = api.writes[-1][1]
        self.assertEqual(payload["name"], "commit standards")
        self.assertEqual(payload["conclusion"], "success")
        self.assertIn("CI run 77, attempt 1", payload["output"]["summary"])
        self.assertNotIn("current body", json.dumps(payload))

    def test_failed_cancelled_skipped_and_pending_ci_never_write_success(self):
        for changes in ({"conclusion": "failure"}, {"conclusion": "cancelled"},
                        {"conclusion": "skipped"}, {"status": "in_progress", "conclusion": None}):
            with self.subTest(changes=changes):
                api = PolicyApi(runs=[run(**changes)])
                # PR events can report pending as failure without waiting.
                self.assertEqual(self.publish(api, identity=self.identity(False)), "failure")
                self.assertEqual(api.writes[-1][1]["conclusion"], "failure")

    def test_failed_and_cancelled_completions_write_failure(self):
        for conclusion in ("failure", "cancelled"):
            api = PolicyApi(runs=[run(conclusion=conclusion)])
            identity = {**self.identity(), "conclusion": conclusion}
            self.assertEqual(self.publish(api, identity=identity), "failure")

    def test_missing_run_refuses_without_waiting(self):
        api = PolicyApi(runs=[])
        self.assertEqual(self.publish(api, identity=self.identity(False)), "failure")
        self.assertEqual(api.writes[-1][1]["conclusion"], "failure")
        with self.assertRaises(policy.reuse.Skip):
            self.publish(PolicyApi(runs=[]))

    def test_validation_failure_and_changed_body_cannot_be_admitted(self):
        for result in ("failure", "cancelled", "skipped"):
            self.assertEqual(self.publish(PolicyApi(), result=result), "failure")
        self.assertEqual(self.publish(PolicyApi(), body_digest="0" * 64), "failure")

    def test_retarget_event_replaces_an_earlier_success_with_failure(self):
        api = PolicyApi()
        api.current["base"]["ref"] = "integration/new-target"
        identity = {**self.identity(False), "base": "integration/new-target"}
        self.assertEqual(self.publish(api, identity=identity), "failure")
        self.assertEqual(api.writes[-1][1]["conclusion"], "failure")

    def test_draft_closed_or_skipped_verdict_refuses(self):
        for changes in ({"draft": True}, {"state": "closed"}):
            api = PolicyApi()
            api.current.update(changes)
            self.assertEqual(self.publish(api), "failure")
        self.assertEqual(self.publish(PolicyApi(jobs={77: [job("codeflow gates", "skipped"), job("windows")]})), "failure")

    def test_stale_head_target_attempt_or_completion_never_admits(self):
        for changes in ({"head": MAIN}, {"base": "old-target"}, {"attempt": 2},
                        {"run": 78}, {"conclusion": "failure"}):
            with self.subTest(changes=changes):
                api = PolicyApi()
                with self.assertRaises(policy.reuse.Skip):
                    self.publish(api, identity={**self.identity(), **changes})
                self.assertEqual(len(api.writes), 1)
                self.assertEqual(api.writes[0][1]["conclusion"], "failure")

    def test_api_failure_at_each_read_never_writes_success(self):
        for method in ("pull", "runs", "get", "attempt_jobs"):
            api, writer = PolicyApi(), PolicyApi()
            with self.subTest(method=method), mock.patch.object(api, method, side_effect=OSError("API error")):
                with self.assertRaises(OSError):
                    self.publish(api, writer)
                self.assertFalse(any(payload.get("conclusion") == "success" for _, payload, _ in writer.writes))

    def test_initial_read_error_revokes_an_earlier_green_at_the_same_head(self):
        reader, writer = PolicyApi(), PolicyApi()
        writer.current_conclusion = "success"
        def write(path, payload, method):
            writer.current_conclusion = payload.get("conclusion", "pending")
            writer.writes.append((path, payload, method))
            return {"id": 100}
        with mock.patch.object(reader, "pull", side_effect=OSError("initial read unavailable")), \
                mock.patch.object(writer, "write", side_effect=write):
            with self.assertRaises(OSError):
                self.publish(reader, writer)
        self.assertEqual(writer.current_conclusion, "failure")
        self.assertEqual(writer.writes[0][1]["head_sha"], HEAD)

    def test_head_move_after_verdict_read_keeps_failure_at_the_event_head(self):
        class Moving(PolicyApi):
            def attempt_jobs(self, run_id, attempt):
                self.current["head"]["sha"] = MAIN
                return GREEN_JOBS
        api = Moving()
        with self.assertRaises(policy.reuse.Skip):
            self.publish(api)
        self.assertEqual(len(api.writes), 1)
        self.assertEqual(api.writes[0][1]["head_sha"], HEAD)
        self.assertEqual(api.writes[0][1]["conclusion"], "failure")

    def test_rerun_after_verdict_read_never_writes_success(self):
        class Rerunning(PolicyApi):
            def attempt_jobs(self, run_id, attempt):
                self.run_list = [run(run_attempt=2)]
                return GREEN_JOBS
        api = Rerunning()
        with self.assertRaises(policy.reuse.Skip):
            self.publish(api)
        self.assertEqual(len(api.writes), 1)
        self.assertEqual(api.writes[0][1]["conclusion"], "failure")

    def test_only_the_freshly_created_check_can_be_promoted(self):
        api = PolicyApi()
        api.check_list = [{"id": 200, "name": "commit standards", "app": {"slug": "github-actions"},
                           "external_id": "codeflow-policy:pull:95"},
                          {"id": 201, "name": "commit standards", "app": {"slug": "other"},
                           "external_id": "codeflow-policy:pull:95"}]
        with mock.patch.object(api, "pages", side_effect=AssertionError("existing checks must not be selected")):
            self.assertEqual(self.publish(api), "success")
        self.assertEqual([path for path, _, _ in api.writes], ["check-runs", "check-runs/100"])
        self.assertEqual(api.check_list[0]["id"], 200)

    def test_missing_ambiguous_or_wrong_event_identity_refuses(self):
        for field, value in (("pull_requests", []), ("pull_requests", [{"number": 95}, {"number": 96}]),
                             ("event", "push"), ("path", "wrong.yml"), ("run_attempt", 0)):
            name, event = policy_event()
            event["workflow_run"][field] = value
            with self.subTest(field=field), self.assertRaises(policy.reuse.Skip):
                policy.event_identity(name, event, "o/r")
        name, event = policy_event()
        with self.assertRaises(policy.reuse.Skip):
            policy.event_identity(name, event, "another/repository")

    def test_prepare_reads_current_body_and_writes_no_body_step_output(self):
        api = PolicyApi()
        api.current["body"] = "latest body, with $(untrusted shell)"
        with tempfile.TemporaryDirectory() as directory:
            fields = policy.prepare(api, self.identity(False), Path(directory))
            self.assertEqual(Path(directory, "pr-body.md").read_text(), api.current["body"] + "\n")
            self.assertEqual(fields["digest"], policy.digest(api.current["body"]))
            self.assertNotIn(api.current["body"], json.dumps(fields))
            self.assertEqual(fields["base_sha"], MAIN)

    def test_step_output_newlines_and_missing_body_are_refused(self):
        for value in ("task/line\nhead=injected", "task/line\rhead=injected"):
            api = PolicyApi()
            api.current["head"]["ref"] = value
            with tempfile.TemporaryDirectory() as directory, self.assertRaises(policy.reuse.Skip):
                policy.prepare(api, self.identity(False), Path(directory))
        api = PolicyApi()
        del api.current["body"]
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(policy.reuse.Skip):
            policy.prepare(api, self.identity(False), Path(directory))

    def test_publisher_reads_public_metadata_without_its_write_token(self):
        request = None
        def response(value, **kwargs):
            nonlocal request
            request = value
            return io.BytesIO(b'{}')
        with mock.patch.object(policy.reuse.urllib.request, "urlopen", side_effect=response):
            policy.reuse.Api("https://api.github.com", "o/r", "").get("actions/runs/77")
        self.assertIsNone(request.get_header("Authorization"))

    def test_prepare_cli_uses_current_body_and_only_emits_digest(self):
        api = PolicyApi()
        api.current["body"] = "body edited after the native event"
        name, event = policy_event(False)
        with tempfile.TemporaryDirectory() as directory:
            event_path = Path(directory, "event.json")
            event_path.write_text(json.dumps(event))
            output_path = Path(directory, "outputs")
            env = {"GITHUB_REPOSITORY": "o/r", "GITHUB_EVENT_NAME": name,
                   "GITHUB_EVENT_PATH": str(event_path), "RUNNER_TEMP": directory,
                   "GITHUB_OUTPUT": str(output_path)}
            out = io.StringIO()
            with mock.patch.dict(os.environ, env), mock.patch("sys.argv", ["policy-admission.py", "prepare"]), \
                    mock.patch.object(policy.reuse, "Api", return_value=api), redirect_stdout(out):
                self.assertEqual(policy.main(), 0)
            self.assertEqual(Path(directory, "pr-body.md").read_text(), api.current["body"] + "\n")
            self.assertIn(f"digest={policy.digest(api.current['body'])}", output_path.read_text())
            self.assertNotIn(api.current["body"], output_path.read_text() + out.getvalue())

    def test_invalid_created_check_id_cannot_be_promoted(self):
        for check_id in (None, True, "100", 0):
            reader, writer = PolicyApi(), PolicyApi()
            with self.subTest(check_id=check_id), \
                    mock.patch.object(writer, "write", return_value={"id": check_id}) as write, \
                    mock.patch.object(reader, "pull", side_effect=AssertionError("no reads before a valid failing check")):
                with self.assertRaises(policy.reuse.Skip):
                    self.publish(reader, writer)
                self.assertEqual(write.call_count, 1)
                self.assertEqual(write.call_args.args[1]["conclusion"], "failure")

    def test_final_body_or_readiness_change_keeps_the_failing_check(self):
        for change in ({"body": "last read changed body"}, {"draft": True}, {"state": "closed"}):
            class Changing(PolicyApi):
                calls = 0
                def pull(self, number):
                    self.calls += 1
                    if self.calls == 4:
                        self.current.update(change)
                    return super().pull(number)
            api = Changing()
            with self.subTest(change=change), self.assertRaises(policy.reuse.Skip):
                self.publish(api)
            self.assertEqual(len(api.writes), 1)
            self.assertEqual(api.writes[0][1]["conclusion"], "failure")

    def test_new_run_after_admission_never_promotes_the_old_run(self):
        class Changing(PolicyApi):
            calls = 0
            def pull(self, number):
                self.calls += 1
                if self.calls == 4:
                    self.run_list = [run(id=78, run_number=6)]
                return super().pull(number)
        api = Changing()
        with self.assertRaises(policy.reuse.Skip):
            self.publish(api, identity=self.identity(False))
        self.assertEqual(len(api.writes), 1)
        self.assertEqual(api.writes[0][1]["conclusion"], "failure")


class PublisherCliTests(unittest.TestCase):
    """The real CLI entry, event file, Request encoding and JSON API adapter,
    with the external HTTP transport controlled. No socket/Actions journey
    or hosted required-check behavior is claimed by these tests."""

    def journey(self, fail_first_read=False):
        api = PolicyApi()
        writes, reads = [], []
        def transport(request, **kwargs):
            path = request.full_url.removeprefix("https://api.fixture.invalid")
            method = request.get_method()
            auth = request.get_header("Authorization")
            if method in ("POST", "PATCH"):
                payload = json.loads(request.data)
                writes.append((method, path, payload, auth))
                return io.BytesIO(b'{"id": 100}')
            reads.append((path, auth))
            if path.endswith("/pulls/95"):
                if fail_first_read:
                    error = policy.reuse.urllib.error.HTTPError(request.full_url, 503, "fixture read unavailable", {}, None)
                    error.close()
                    raise error
                value = api.current
            elif "/attempts/1/jobs?" in path:
                value = {"total_count": len(GREEN_JOBS), "jobs": GREEN_JOBS}
            elif "/actions/runs?" in path:
                value = {"total_count": 1, "workflow_runs": [run()]}
            elif path.endswith("/actions/runs/77"):
                value = run()
            else:
                raise AssertionError(path)
            return io.BytesIO(json.dumps(value).encode())

        with tempfile.TemporaryDirectory() as directory:
            name, event = policy_event()
            event_path = Path(directory, "event.json")
            event_path.write_text(json.dumps(event))
            env = {"GITHUB_REPOSITORY": "o/r", "GH_TOKEN": "fixture-token",
                   "GITHUB_API_URL": "https://api.fixture.invalid", "GITHUB_EVENT_NAME": name,
                   "GITHUB_EVENT_PATH": str(event_path), "GITHUB_SERVER_URL": "https://github.com",
                   "GITHUB_RUN_ID": "99", "RUNNER_TEMP": directory,
                   "GITHUB_OUTPUT": str(Path(directory, "outputs")), "VALIDATION_RESULT": "success",
                   "JUDGED_DIGEST": policy.digest(api.current["body"])}
            out = io.StringIO()
            with mock.patch.dict(os.environ, env), mock.patch("sys.argv", ["policy-admission.py", "publish"]), \
                    mock.patch.object(policy.reuse.urllib.request, "urlopen", side_effect=transport), redirect_stdout(out):
                code = policy.main()
        return subprocess.CompletedProcess([], code, out.getvalue(), ""), writes, reads

    def test_success_traverses_cli_entry_and_http_request_encoding(self):
        done, writes, reads = self.journey()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertEqual([method for method, _, _, _ in writes], ["POST", "PATCH"])
        self.assertEqual(writes[0][2]["head_sha"], HEAD)
        self.assertEqual(writes[0][2]["conclusion"], "failure")
        self.assertEqual(writes[-1][1], "/repos/o/r/check-runs/100")
        self.assertEqual(writes[-1][2]["conclusion"], "success")
        self.assertTrue(reads)
        self.assertTrue(all(auth is None for _, auth in reads))
        self.assertTrue(all(auth == "Bearer fixture-token" for _, _, _, auth in writes))
        self.assertNotIn("current body", done.stdout + done.stderr)

    def test_initial_http_error_leaves_the_required_check_failing(self):
        done, writes, _ = self.journey(fail_first_read=True)
        self.assertEqual(done.returncode, 1)
        self.assertEqual(len(writes), 1)
        self.assertEqual(writes[0][2]["conclusion"], "failure")
        self.assertIn("refused", done.stdout)
        self.assertNotIn("current body", done.stdout + done.stderr)


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

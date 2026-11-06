#!/usr/bin/env python3
"""Controls for scripts/ci-reuse.py (TSK-265 AC-4, TSK-359 AC-2 and AC-3).

A push to main or an integration line reuses a pull request run's verdict
only on an exact match: one merged pull request into that line, the same
tree, the line before the merge inside the head, no cache input changed, the
newest codeflow-ci pull_request run of that pull request into that line green
in every job, and that run's own provenance record naming the same pull
request, line, head, tree, workflow blob and runner image. Each other case
runs every job.
"""

import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
import zipfile
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
LINE = "integration/EPC-022-the-3-2-0-milestone-fixes"
LINE_REF = f"refs/heads/{LINE}"
IMAGE = ("ubuntu24", "20261005.1")
WIN_IMAGE = ("win25", "20261001.2")
IMAGES = {reuse.PROVENANCE: IMAGE, reuse.PROVENANCE_WINDOWS: WIN_IMAGE}
BLOB = "d" * 40
BASE = "e" * 40
RELEASE_JOB = "codeflow gates (release ${{ matrix.part }})"
WORKFLOW_TEXT = """name: codeflow-ci
on:
  push:
jobs:
  reuse:
    name: reuse a pull request verdict
    if: github.event_name == 'push'
  gates-verdict:
    if: ${{ always() && !(github.event_name == 'pull_request') }}
    name: codeflow gates
  windows-verdict:
    if: ${{ always() }}
    name: windows
  gates:
    if: ${{ !cancelled() && needs.reuse.outputs.run == '' && (github.event_name == 'push') }}
    name: codeflow gates (${{ matrix.part }})
  windows-build:
    if: ${{ !cancelled() && needs.reuse.outputs.run == '' }}
    name: windows test build
    needs: reuse
  windows-tests:
    if: ${{ !cancelled() && needs.windows-build.result == 'success' }}
    name: windows tests (${{ matrix.partition }} of 12)
    needs: windows-build
"""
PUSH_RUN = 500
PUSH_IMAGE = {"ubuntu-24.04": ("ubuntu-24.04", "20261025.353.1"),
              "windows-latest": ("windows-2025-vs2026", "20261021.278.1")}
JOB_LABEL = {}
_ids = iter(range(1000, 10 ** 6))


def log(image, version, after_step=False):
    """A job log head as GitHub writes it (TSK-359; from job 124950310866)."""
    block = ["##[group]Runner Image", f"Image: {image}", f"Version: {version}",
             "Included Software: https://github.com/actions/runner-images/blob/x/y/Readme.md", "##[endgroup]"]
    head = ["\ufeff2026-11-01T19:15:59.8257055Z Current runner version: '2.338.2'",
            "##[group]Runner Image Provisioner", "Hosted Compute Agent", "Version: 20261021.618", "##[endgroup]"]
    step = ["##[group]Run actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803", "##[endgroup]"]
    lines = head + (step + block if after_step else block + step)
    return "\n".join(line if line.startswith("\ufeff") else f"2026-11-01T19:15:59.8312716Z {line}" for line in lines)


class FakeGit:
    def __init__(self, trees=None, parents=None, contains=True, changed=(), blob=BLOB):
        self.blob_id = blob
        self.trees = trees or {PUSHED: "t1", HEAD: "t1"}
        self.parent_list = parents if parents is not None else [MAIN, HEAD]
        self.contains = contains
        self.changed_paths = list(changed)

    def tree(self, commit):
        return self.trees[commit]

    def blob(self, commit, path):
        return self.blob_id

    def parents(self, commit):
        return self.parent_list

    def is_ancestor(self, ancestor, commit):
        return self.contains

    def changed(self, base, commit):
        return self.changed_paths


def job(name, conclusion="success", label=None):
    label = label or ("windows-latest" if name.startswith("windows") else "ubuntu-24.04")
    job_id = next(_ids)
    JOB_LABEL[job_id] = label
    return {"id": job_id, "name": name, "conclusion": conclusion, "labels": [label]}


PUSH_JOBS = [job("reuse a pull request verdict", label="ubuntu-24.04"),
             job("read the Ubuntu runner image", label="ubuntu-24.04"),
             job("read the Windows runner image", label="windows-latest")]


GREEN_JOBS = [job("codeflow gates (coverage)"), job("codeflow gates"),
              *(job(f"windows tests ({n} of 12)") for n in range(1, 13)),
              job("windows tier", label="ubuntu-24.04"), job("windows tier journeys", "skipped"),
              job("windows"), job("secret scan"), job("reuse a pull request verdict", "skipped")]


def run(base="main", **changes):
    record = {"id": 77, "run_number": 5, "run_attempt": 1, "event": "pull_request", "head_sha": HEAD,
              "path": WORKFLOW, "status": "completed", "conclusion": "success",
              "pull_requests": [{"number": 95, "base": {"ref": base}}]}
    record.update(changes)
    return record


def provenance(base="main", windows=False, **changes):
    record = {"schema": 1, "pull": 95, "base_ref": base, "base_commit": BASE, "head_commit": HEAD,
              "merge_commit": "f" * 40, "merge_tree": "t1", "workflow": WORKFLOW, "workflow_blob": BLOB,
              "image_os": (WIN_IMAGE if windows else IMAGE)[0],
              "image_version": (WIN_IMAGE if windows else IMAGE)[1]}
    record.update(changes)
    return record


def archive(record, name="provenance.json", extra=()):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as handle:
        handle.writestr(name, json.dumps(record))
        for other in extra:
            handle.writestr(other, "{}")
    return buffer.getvalue()


def artifact(run_id=77, head=HEAD, windows=False, **changes):
    record = {"id": (1900 if windows else 900) + run_id,
              "name": reuse.PROVENANCE_WINDOWS if windows else reuse.PROVENANCE, "expired": False, "size_in_bytes": 400,
              "workflow_run": {"id": run_id, "head_sha": head}}
    record.update(changes)
    return record


class FakeApi:
    def __init__(self, pulls=None, runs=None, jobs=None, caches=None, base="main", artifacts=None, zips=None,
                 logs=None):
        self.cache_queries = []
        self.logs = logs or {}
        self.cache_list = caches if caches is not None else [{"key": APT_KEY, "ref": MAIN_REF}]
        self.pull_list = pulls if pulls is not None else [
            {"number": 95, "merged_at": "2026-10-11T00:43:00Z", "merge_commit_sha": PUSHED, "head": {"sha": HEAD},
             "base": {"ref": base}}]
        self.run_list = runs if runs is not None else [run(base)]
        self.job_lists = {PUSH_RUN: PUSH_JOBS, **(jobs if jobs is not None else {77: GREEN_JOBS})}
        self.artifact_lists = artifacts if artifacts is not None else {77: [artifact(), artifact(windows=True)]}
        self.zips = zips if zips is not None else {977: archive(provenance(base)),
                                                   1977: archive(provenance(base, windows=True))}

    def artifacts(self, run_id):
        result = self.artifact_lists.get(run_id, [])
        if isinstance(result, Exception):
            raise result
        return result

    def artifact_zip(self, artifact_id):
        return self.zips[artifact_id]

    def pulls(self, sha):
        return self.pull_list

    def runs(self, head):
        return self.run_list

    def jobs(self, run_id):
        return self.job_lists.get(run_id, [])

    def job_log_head(self, job_id):
        found = self.logs.get(job_id)
        if isinstance(found, Exception):
            raise found
        if callable(found):
            return found()
        return found if found is not None else log(*PUSH_IMAGE[JOB_LABEL[job_id]])

    def caches(self, key, ref):
        self.cache_queries.append((key, ref))
        if isinstance(self.cache_list, Exception):
            raise self.cache_list
        return self.cache_list


def decide(event="push", git=None, api=None, ref=MAIN_REF, images=None, text=WORKFLOW_TEXT, line_skips=None,
           run_id=PUSH_RUN):
    skips = frozenset({RELEASE_JOB}) if line_skips is None else line_skips
    return reuse.decide(event, PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, git or FakeGit(), api or FakeApi(),
                        APT_KEY, ref, images or IMAGES, text, skips, run_id)


class DecisionTests(unittest.TestCase):
    def skipped(self, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(**kwargs)
        return str(caught.exception)

    def test_an_exact_match_reuses_the_run(self):
        self.assertEqual(decide(), 77)

    def test_a_pull_request_run_never_reuses(self):
        self.assertIn("pull_request run never reuses", self.skipped(event="pull_request"))

    def test_a_push_to_any_ref_but_main_or_a_line_runs_everything(self):
        # Every other input matches, the cache included, so only the ref refuses.
        for ref in ("refs/heads/integration/", "refs/heads/master", "refs/heads/main-old", "main",
                    "refs/heads/feature/integration/x", "refs/tags/integration/x"):
            with self.subTest(ref=ref):
                api = FakeApi(caches=[{"key": APT_KEY, "ref": ref}])
                self.assertIn("only a push to refs/heads/main or", self.skipped(api=api, ref=ref))

    def test_an_integration_line_push_reuses_the_run_of_its_own_pull_request(self):
        api = FakeApi(base=LINE)
        self.assertEqual(decide(api=api, ref=LINE_REF), 77)

    def test_a_line_may_rely_on_the_cache_main_holds(self):
        # A pull request restores caches from its base and from main.
        self.assertEqual(decide(api=FakeApi(base=LINE), ref=LINE_REF), 77)
        api = FakeApi(base=LINE, caches=[{"key": APT_KEY, "ref": "refs/heads/other"}])
        self.assertIn("holds apt cache", self.skipped(api=api, ref=LINE_REF))

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
        self.assertEqual(decide(git=FakeGit(changed=["README.md", "scripts/test_ci_reuse.py"])), 77)

    def test_a_push_that_changes_the_decider_runs_everything(self):
        # The workflow also pins the decider's hash; either alone refuses.
        self.assertIn("scripts/ci-reuse.py", self.skipped(git=FakeGit(changed=["README.md", "scripts/ci-reuse.py"])))
        self.assertTrue(reuse.cache_input("scripts/ci-reuse.py"))

    def test_no_pull_request_or_an_unmerged_or_other_one_runs_everything(self):
        for pulls in ([], [{"number": 1, "merged_at": None, "merge_commit_sha": PUSHED, "head": {"sha": HEAD}}],
                      [{"number": 1, "merged_at": "x", "merge_commit_sha": MAIN, "head": {"sha": HEAD}}]):
            with self.subTest(pulls=pulls):
                self.assertIn("reuse needs exactly one", self.skipped(api=FakeApi(pulls=pulls)))

    def test_two_merged_pull_requests_for_one_commit_run_everything(self):
        merged = {"number": 95, "merged_at": "x", "merge_commit_sha": PUSHED, "head": {"sha": HEAD},
                  "base": {"ref": "main"}}
        self.assertIn("2 merged pull requests", self.skipped(api=FakeApi(pulls=[merged, {**merged, "number": 96}])))

    def test_a_draft_run_whose_verdicts_were_skipped_runs_everything(self):
        # A draft pull request run ends its verdicts neutral (skipped), and
        # skips the gate jobs; it must never stand in for a ready run.
        draft = [job("secret scan"), job("codeflow gates", "skipped"), job("windows", "skipped"),
                 job("reuse a pull request verdict", "skipped")]
        self.assertIn("'codeflow gates'", self.skipped(api=FakeApi(jobs={77: draft})))

    def test_a_failed_cancelled_running_or_missing_run_runs_everything(self):
        for runs in ([run(conclusion="failure")], [run(conclusion="cancelled")],
                     [run(status="in_progress", conclusion=None)], []):
            with self.subTest(runs=runs):
                self.skipped(api=FakeApi(runs=runs))

    def test_the_newest_run_decides_even_when_an_older_one_passed(self):
        runs = [run(), run(id=78, run_number=6, conclusion="failure")]
        self.assertIn("run 78", self.skipped(api=FakeApi(runs=runs)))

    def test_a_newer_success_for_another_pull_request_is_ignored(self):
        # The same head, another pull request: the newer run cannot stand in.
        runs = [run(), run(id=78, run_number=6, pull_requests=[{"number": 96, "base": {"ref": "main"}}],
                           conclusion="failure")]
        self.assertEqual(decide(api=FakeApi(runs=runs, jobs={77: GREEN_JOBS, 78: GREEN_JOBS})), 77)

    def test_two_pull_requests_with_one_head_and_different_bases_are_told_apart(self):
        # Head H into line A landed; a newer green run for H into line B exists
        # with a tree that holds changes absent from H (TSK-359 counterexample).
        newer = run(LINE, id=78, run_number=6)
        api = FakeApi(runs=[run(), newer], jobs={77: GREEN_JOBS, 78: GREEN_JOBS},
                      artifacts={77: [artifact(), artifact(windows=True)], 78: [artifact(78), artifact(78, windows=True)]},
                      zips={977: archive(provenance()), 1977: archive(provenance(windows=True)),
                            978: archive(provenance(LINE, merge_tree="t-other"))})
        self.assertEqual(decide(api=api), 77)
        only_other = FakeApi(runs=[newer], jobs={78: GREEN_JOBS}, artifacts={78: [artifact(78)]},
                             zips={978: archive(provenance(LINE, merge_tree="t-other"))})
        self.assertIn("into main exists", self.skipped(api=only_other))

    def test_a_retargeted_pull_request_does_not_reuse_the_run_for_its_old_base(self):
        # The run tested the pull request into the old base; it merged into the line.
        api = FakeApi(base=LINE, runs=[run("main")])
        self.assertIn(f"into {LINE} exists", self.skipped(api=api, ref=LINE_REF))

    def test_a_pull_request_merged_into_another_line_runs_everything(self):
        self.assertIn("not 'main'", self.skipped(api=FakeApi(base=LINE)))
        self.assertIn(f"not {LINE!r}", self.skipped(api=FakeApi(base="main"), ref=LINE_REF))

    def test_a_run_without_pull_request_associations_is_not_reused(self):
        for associations in ([], None):
            with self.subTest(associations=associations):
                self.assertIn("pull_request run of #95 into main",
                              self.skipped(api=FakeApi(runs=[run(pull_requests=associations)])))

    def test_the_skipped_release_job_name_is_the_workflow_text_unexpanded(self):
        # The script, not a shell argument, holds the name, so Actions never
        # evaluates the `${{ }}`. GitHub records the skipped matrix job under
        # its `name:` text as written (run 41335856425).
        workflow = (ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text()
        self.assertIn(f"    name: {RELEASE_JOB}\n", workflow)
        self.assertEqual(reuse.LINE_SKIPS, frozenset({RELEASE_JOB}))
        self.assertNotIn("--line-skip", workflow)

    def test_the_release_parts_may_be_skipped_on_a_line_only(self):
        jobs = [*GREEN_JOBS, job(RELEASE_JOB, "skipped")]
        default = reuse.decide("push", PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, FakeGit(), FakeApi(base=LINE, jobs={77: jobs}),
                               APT_KEY, LINE_REF, IMAGES, WORKFLOW_TEXT, run_id=PUSH_RUN)
        self.assertEqual(default, 77)
        self.assertEqual(decide(api=FakeApi(base=LINE, jobs={77: jobs}), ref=LINE_REF), 77)
        self.assertIn("release", self.skipped(api=FakeApi(jobs={77: jobs})))
        self.assertIn("release", self.skipped(api=FakeApi(base=LINE, jobs={77: jobs}), ref=LINE_REF, line_skips=frozenset()))


class ProvenanceTests(unittest.TestCase):
    def skipped(self, api, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(api=api, **kwargs)
        return str(caught.exception)

    def test_a_run_without_a_provenance_artifact_runs_everything(self):
        self.assertIn("0 ci-provenance artifacts", self.skipped(FakeApi(artifacts={77: []})))

    def test_a_run_without_the_windows_record_runs_everything(self):
        api = FakeApi(artifacts={77: [artifact()]})
        self.assertIn("0 ci-provenance-windows", self.skipped(api))

    def test_a_windows_image_other_than_the_push_windows_image_runs_everything(self):
        for changed in ({"image_os": "win22"}, {"image_version": "20260101.1"}):
            with self.subTest(changed=changed):
                api = FakeApi(zips={977: archive(provenance()), 1977: archive(provenance(windows=True, **changed))})
                self.assertIn("image_", self.skipped(api))
        self.assertIn("image_", self.skipped(FakeApi(), images={reuse.PROVENANCE: IMAGE}))

    def test_two_provenance_artifacts_or_another_name_run_everything(self):
        self.assertIn("2 ci-provenance", self.skipped(FakeApi(artifacts={77: [artifact(), artifact(id=5)]})))
        self.assertIn("0 ci-provenance", self.skipped(FakeApi(artifacts={77: [artifact(name="other")]})))

    def test_an_expired_or_foreign_artifact_runs_everything(self):
        for changed in ({"expired": True}, {"workflow_run": {"id": 12, "head_sha": HEAD}},
                        {"workflow_run": {"id": 77, "head_sha": MAIN}}, {"size_in_bytes": 10 ** 9}):
            with self.subTest(changed=changed):
                self.skipped(FakeApi(artifacts={77: [artifact(**changed)]}))

    def test_a_record_for_any_other_value_runs_everything(self):
        for key, value in (("pull", 96), ("base_ref", LINE), ("head_commit", MAIN), ("merge_tree", "t-other"),
                           ("workflow", ".github/workflows/other.yml"), ("workflow_blob", "0" * 40),
                           ("image_os", "ubuntu22"), ("image_version", "20260101.1"), ("base_commit", "nope")):
            with self.subTest(key=key):
                api = FakeApi(zips={977: archive(provenance(**{key: value}))})
                self.skipped(api)

    def test_a_record_with_a_missing_or_extra_field_or_schema_runs_everything(self):
        full = provenance()
        for record in ({k: v for k, v in full.items() if k != "merge_tree"}, {**full, "extra": 1},
                       {**full, "schema": 2}, [], "text"):
            with self.subTest(record=record):
                self.skipped(FakeApi(zips={977: archive(record)}))

    def test_a_blank_image_runs_everything(self):
        self.skipped(FakeApi(zips={977: archive(provenance(image_os="", image_version="")),
                                   1977: archive(provenance(windows=True))}),
                     images={reuse.PROVENANCE: ("", ""), reuse.PROVENANCE_WINDOWS: WIN_IMAGE})

    def test_a_base_the_head_does_not_contain_runs_everything(self):
        self.assertIn("does not contain the base", self.skipped(FakeApi(), git=NoRecordedBase()))

    def test_a_bad_archive_runs_everything(self):
        for blob in (b"not a zip", archive(provenance(), name="other.json"),
                     archive(provenance(), extra=("more.txt",))):
            with self.subTest(blob=blob[:10]):
                self.skipped(FakeApi(zips={977: blob}))

    def test_an_artifact_listing_or_download_error_runs_everything(self):
        with self.assertRaises(OSError):
            decide(api=FakeApi(artifacts={77: OSError("HTTP 500")}))

        class Failing(FakeApi):
            def artifact_zip(self, artifact_id):
                raise OSError("HTTP 404")
        self.assertIn("unreadable", self.skipped(Failing()))

    def test_the_verdict_jobs_come_from_the_pushed_workflow(self):
        self.assertEqual(reuse.verdict_jobs(WORKFLOW_TEXT), {"codeflow gates", "windows"})
        added = WORKFLOW_TEXT + "  native:\n    name: native platforms\n    if: ${{ always() }}\n"
        self.assertEqual(reuse.verdict_jobs(added), {"codeflow gates", "windows", "native platforms"})
        # A verdict the run lacks is not frozen out: the run must hold the new job too.
        self.assertIn("native platforms", self.skipped(FakeApi(), text=added))
        # A pushed workflow that lost a required verdict is refused.
        self.assertIn("no longer has", self.skipped(FakeApi(), text=WORKFLOW_TEXT.replace("windows", "w2")))

    def test_the_gates_never_reused_are_not_verdict_jobs(self):
        names = reuse.verdict_jobs((ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text())
        self.assertTrue({"codeflow gates", "windows"} <= names)
        for never in ("secret scan", "gitleaks allowlist canary", "security review"):
            self.assertNotIn(never, names)


class ImageBindingTests(unittest.TestCase):
    """TSK-359: each job the push skips must have run on the push's image."""

    def skipped(self, api, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(api=api, **kwargs)
        return str(caught.exception)

    def job_id(self, name):
        return next(j["id"] for j in GREEN_JOBS if j["name"] == name)

    def test_matching_images_on_every_reused_job_reuse(self):
        self.assertEqual(decide(), 77)

    def test_a_drifted_ubuntu_gate_job_is_refused(self):
        drifted = {self.job_id("codeflow gates (coverage)"): log("ubuntu-24.04", "20261018.96.1")}
        message = self.skipped(FakeApi(logs=drifted))
        self.assertIn("'codeflow gates (coverage)' ran on image ubuntu-24.04 20261018.96.1", message)
        self.assertIn("20261025.353.1", message)

    def test_a_drifted_windows_job_image_or_version_is_refused(self):
        name = "windows tests (1 of 12)"
        for image, version in (("windows-2022", "20261021.278.1"), ("windows-2025-vs2026", "20261030.294.1")):
            with self.subTest(image=image, version=version):
                self.assertIn(f"{name!r} ran on image", self.skipped(FakeApi(logs={self.job_id(name): log(image, version)})))

    def test_a_drifted_push_image_is_refused(self):
        push = {PUSH_JOBS[1]["id"]: log("ubuntu-24.04", "20261101.1.1")}
        self.assertIn("ran on image", self.skipped(FakeApi(logs=push)))

    def test_a_job_the_push_also_runs_may_use_another_image(self):
        # The secret scan runs on every push, so its image is not reused.
        self.assertEqual(decide(api=FakeApi(logs={self.job_id("secret scan"): log("ubuntu-24.04", "1")})), 77)

    def test_a_runner_image_block_printed_after_a_step_started_is_refused(self):
        forged = {self.job_id("codeflow gates (coverage)"): log(*PUSH_IMAGE["ubuntu-24.04"], after_step=True)}
        self.assertIn("no Runner Image block", self.skipped(FakeApi(logs=forged)))

    def test_a_log_error_on_a_reused_job_runs_everything(self):
        with self.assertRaises(OSError):
            decide(api=FakeApi(logs={self.job_id("codeflow gates (coverage)"): OSError("HTTP 404")}))

    def test_a_job_on_an_unknown_label_is_refused(self):
        jobs = [*GREEN_JOBS, job("codeflow gates (arm)", label="ubuntu-24.04-arm")]
        self.assertIn("ubuntu-24.04-arm", self.skipped(FakeApi(jobs={77: jobs})))

    def test_a_missing_failed_repeated_or_off_label_push_image_job_is_refused(self):
        ubuntu, windows = PUSH_JOBS[1], PUSH_JOBS[2]
        for push in ([PUSH_JOBS[0], ubuntu], [PUSH_JOBS[0], {**ubuntu, "conclusion": "failure"}, windows],
                     [PUSH_JOBS[0], ubuntu, ubuntu, windows], [PUSH_JOBS[0], {**ubuntu, "labels": ["windows-latest"]}, windows]):
            with self.subTest(push=[(j["name"], j["conclusion"], j["labels"]) for j in push]):
                api = FakeApi()
                api.job_lists[PUSH_RUN] = push
                self.skipped(api)

    def test_a_push_image_log_not_served_yet_is_retried_then_refused(self):
        calls = []

        def late():
            calls.append(1)
            if len(calls) < 3:
                raise OSError("HTTP 404")
            return log(*PUSH_IMAGE["ubuntu-24.04"])
        with mock.patch.object(reuse, "LOG_RETRY_SECONDS", 0):
            self.assertEqual(decide(api=FakeApi(logs={PUSH_JOBS[1]["id"]: late})), 77)
            self.assertIn("gave no runner image", self.skipped(FakeApi(logs={PUSH_JOBS[1]["id"]: OSError("HTTP 404")})))

    def test_no_run_id_or_no_workflow_is_refused(self):
        for run_id in (None, 0, -1):
            with self.subTest(run_id=run_id):
                self.assertIn("runner images cannot be compared", self.skipped(FakeApi(), run_id=run_id))
        self.assertIn("runner images cannot be compared", self.skipped(FakeApi(), text=None))

    def test_a_run_with_no_reused_job_is_refused(self):
        jobs = [j for j in GREEN_JOBS if j["name"] != "codeflow gates (coverage)"
                and not j["name"].startswith("windows tests (")]
        self.assertIn("no image was compared", self.skipped(FakeApi(jobs={77: jobs})))

    def test_the_reused_jobs_come_from_the_pushed_workflow(self):
        patterns = reuse.reused_jobs((ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text())

        def reused(name):
            return any(p.fullmatch(name) for p in patterns)
        for name in ("codeflow build", "codeflow gates (checks)", "codeflow gates (coverage 1 of 2)",
                     "codeflow gates (coverage report)", "windows test build", "windows tests (3 of 12)",
                     "windows checks (doc, clippy, Node)", "windows journeys", "windows release journey"):
            self.assertTrue(reused(name), name)
        for name in ("codeflow gates", "windows", "secret scan", "security review", "gitleaks allowlist canary",
                     "reuse a pull request verdict", "read the Windows runner image",
                     "record the tested merge (ci-provenance)"):
            self.assertFalse(reused(name), name)

    def test_the_runner_image_block_of_a_real_log_is_read(self):
        self.assertEqual(reuse.runner_image(log("windows-2025-vs2026", "20261021.278.1")),
                         ("windows-2025-vs2026", "20261021.278.1"))
        for text in ("", log("", "1"), "##[group]Runner Image\nImage: x\n"):
            with self.subTest(text=text[:20]):
                with self.assertRaises(reuse.Skip):
                    reuse.runner_image(text)


class BaseBindingTests(unittest.TestCase):
    """TSK-359: the base the run merged with is the line before the push or
    an ancestor of it."""

    def test_a_base_off_the_destination_history_is_refused(self):
        with self.assertRaises(reuse.Skip) as caught:
            decide(git=OffLineBase())
        self.assertIn("is not main before this push", str(caught.exception))

    def test_the_first_parent_or_an_ancestor_of_it_is_accepted(self):
        self.assertEqual(decide(git=FakeGit()), 77)
        self.assertEqual(decide(api=FakeApi(zips={977: archive(provenance(base_commit=MAIN)),
                                                  1977: archive(provenance(windows=True, base_commit=MAIN))})), 77)


class OffLineBase(FakeGit):
    """The head contains the base and the line, but the line never held the base."""

    def is_ancestor(self, ancestor, commit):
        return not (ancestor == BASE and commit == MAIN)


class NoRecordedBase(FakeGit):
    def is_ancestor(self, ancestor, commit):
        return ancestor != BASE


class RefusalTests(unittest.TestCase):
    """AC-3 refusals for the run, its jobs and the apt cache."""

    def skipped(self, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(**kwargs)
        return str(caught.exception)

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
        self.assertIn("holds apt cache", self.skipped(api=FakeApi(caches=[])))

    def test_a_missing_line_cache_names_the_key_refs_and_conditional_save(self):
        api = FakeApi(base=LINE, caches=[])
        reason = self.skipped(api=api, ref=LINE_REF)
        self.assertEqual(api.cache_queries, [(APT_KEY, LINE_REF), (APT_KEY, MAIN_REF)])
        self.assertIn(f"neither {LINE_REF} nor {MAIN_REF} holds apt cache {APT_KEY!r}", reason)
        self.assertIn("saving the cache requires that job to succeed", reason)

    def test_the_exact_line_cache_allows_reuse(self):
        api = FakeApi(base=LINE, caches=[{"key": APT_KEY, "ref": LINE_REF}])
        self.assertEqual(decide(api=api, ref=LINE_REF), 77)
        self.assertEqual(api.cache_queries, [(APT_KEY, LINE_REF), (APT_KEY, MAIN_REF)])

    def test_a_failed_apt_cache_lookup_runs_everything(self):
        self.assertIn("lookup failed", self.skipped(api=FakeApi(caches=OSError("HTTP 403"))))

    def test_an_apt_cache_for_another_image_or_ref_runs_everything(self):
        for cache in ({"key": "playwright-apt-ubuntu24-20260928.2-1.56.1", "ref": MAIN_REF},
                      {"key": APT_KEY + "0", "ref": MAIN_REF},
                      {"key": APT_KEY, "ref": "refs/pull/135/merge"}):
            with self.subTest(cache=cache):
                self.assertIn("holds apt cache", self.skipped(api=FakeApi(caches=[cache])))

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
        self.assertEqual(self.main(lambda *args, **kwargs: 77), (0, "run=77\n"))

    def test_a_reason_or_an_error_writes_nothing_and_never_fails(self):
        def refuse(*args, **kwargs):
            raise reuse.Skip("trees differ")

        def broken(*args, **kwargs):
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


ADMISSION_SPEC = importlib.util.spec_from_file_location("policy_admission", ROOT / "scripts" / "policy-admission.py")
admission = importlib.util.module_from_spec(ADMISSION_SPEC)
ADMISSION_SPEC.loader.exec_module(admission)
POLICY = ".github/workflows/codeflow-policy.yml"
STANDARDS = "trusted: commit standards"
CREATED = "2026-11-05T10:00:00Z"


def pull_record(**changes):
    record = {"number": 95, "state": "open", "draft": False, "head": {"sha": HEAD}, "base": {"ref": "main"}}
    record.update(changes)
    return record


def policy_run(**changes):
    record = {"id": 500, "run_number": 9, "run_attempt": 1, "event": "pull_request_target", "head_sha": HEAD,
              "path": POLICY, "status": "completed", "conclusion": "failure",
              "pull_requests": [{"number": 95, "base": {"ref": "main"}}]}
    record.update(changes)
    return record


class AdmissionApi(FakeApi):
    """The API boundary of policy-admission.py: GET records by path, the
    paged lists, and the one POST, which is recorded, never sent."""

    def __init__(self, pull=None, timeline=None, policy_runs=None, policy_jobs=None, created=CREATED, broken=(),
                 **kwargs):
        super().__init__(**kwargs)
        self.pull = pull if pull is not None else pull_record()
        self.timeline = timeline if timeline is not None else [{"event": "labeled", "created_at": CREATED}]
        self.policy_runs = policy_runs if policy_runs is not None else [policy_run()]
        self.policy_jobs = policy_jobs if policy_jobs is not None else {500: [{"id": 9001, "name": STANDARDS},
                                                                               {"id": 9002, "name": "trusted: release impact"}]}
        self.created, self.broken, self.posts = created, set(broken), []

    def fail(self, what):
        if what in self.broken:
            raise OSError(f"{what} unreachable")

    def get(self, path):
        if path == "pulls/95":
            self.fail("pull")
            return self.pull
        if path.startswith("actions/runs/"):
            self.fail("run")
            run_id = int(path.rsplit("/", 1)[1])
            found = [r for r in self.run_list if r.get("id") == run_id]
            return {**found[0], "created_at": self.created} if found else {"message": "Not Found"}
        raise AssertionError(f"unexpected GET {path}")

    def pages(self, path, key):
        if path == "issues/95/timeline":
            self.fail("timeline")
            return self.timeline
        if path.startswith("actions/runs?") and "pull_request_target" in path:
            self.fail("policy runs")
            return self.policy_runs
        raise AssertionError(f"unexpected list {path}")

    def jobs(self, run_id):
        if run_id in self.policy_jobs:
            self.fail("policy jobs")
            return self.policy_jobs[run_id]
        return super().jobs(run_id)

    def post(self, path):
        self.fail("post")
        self.posts.append(path)


class CompletionAdmissionTests(unittest.TestCase):
    """TSK-1239 AC-2 to AC-4: the commit standards job judges once, with
    the TSK-449 predicate, and rechecks the pull request's identity, head,
    target and readiness; a pending run refuses without waiting."""

    def admitted(self, api, base="main"):
        return admission.admit(HEAD, 95, base, WORKFLOW, REQUIRED, api)

    def refused(self, api, pending=False, base="main"):
        with self.assertRaises(admission.Skip) as caught:
            self.admitted(api, base)
        self.assertEqual(isinstance(caught.exception, admission.Pending), pending, str(caught.exception))
        return str(caught.exception)

    def test_a_completed_green_run_at_the_exact_head_admits(self):
        self.assertEqual(self.admitted(AdmissionApi()), 77)

    def test_the_predicate_is_the_newest_run_with_every_named_verdict(self):
        newer = AdmissionApi(runs=[run(), run(id=78, run_number=6, status="in_progress", conclusion=None)])
        self.assertIn("run 78", self.refused(newer, pending=True))
        self.assertIn("'windows' of run 77 concluded nothing",
                      self.refused(AdmissionApi(jobs={77: [job("codeflow gates")]})))

    def test_a_missing_or_pending_run_refuses_at_once(self):
        self.assertIn("exists for head", self.refused(AdmissionApi(runs=[]), pending=True))
        for status in ("queued", "in_progress", "waiting"):
            with self.subTest(status=status):
                api = AdmissionApi(runs=[run(status=status, conclusion=None)])
                self.assertIn(f"is {status}", self.refused(api, pending=True))

    def test_a_cancelled_or_failed_run_refuses(self):
        for conclusion in ("cancelled", "failure", "timed_out", "skipped"):
            with self.subTest(conclusion=conclusion):
                self.assertIn(f"concluded {conclusion}",
                              self.refused(AdmissionApi(runs=[run(conclusion=conclusion)])))

    def test_a_wrongly_skipped_verdict_refuses(self):
        for name in sorted(REQUIRED):
            with self.subTest(verdict=name):
                jobs = [job(other) if other != name else job(name, "skipped") for other in sorted(REQUIRED)]
                self.assertIn(f"{name!r} of run 77 concluded skipped", self.refused(AdmissionApi(jobs={77: jobs})))

    def test_a_head_that_moved_refuses(self):
        api = AdmissionApi(pull=pull_record(head={"sha": MAIN}))
        self.assertIn(f"now at {MAIN}", self.refused(api))

    def test_a_retargeted_pull_request_refuses(self):
        # The base read now differs from the base the check judged.
        self.assertIn("now targets main, not the judged integration/x",
                      self.refused(AdmissionApi(), base="integration/x"))
        # A run created before the newest retarget tested the old target,
        # even though its pull_requests entry names the current base.
        for at in ("2026-11-05T10:00:00Z", "2026-11-05T11:00:00Z"):
            with self.subTest(retargeted=at):
                api = AdmissionApi(timeline=[{"event": "base_ref_changed", "created_at": "2026-11-05T09:00:00Z"},
                                             {"event": "base_ref_changed", "created_at": at}])
                self.assertIn("was retargeted", self.refused(api))
        earlier = AdmissionApi(timeline=[{"event": "base_ref_changed", "created_at": "2026-11-05T09:59:59Z"}])
        self.assertEqual(self.admitted(earlier), 77)

    def test_a_body_edit_admits_on_the_same_completed_run(self):
        # A body edit queues a policy run (never cancelling the earlier one,
        # TSK-1040) and starts no codeflow-ci run, so that run judges the
        # same completed run and admits; its last read rechecks the body.
        api = AdmissionApi(timeline=[{"event": "renamed", "created_at": "2026-11-05T12:00:00Z"}])
        self.assertEqual(self.admitted(api), 77)

    def test_a_rerun_attempt_replaces_the_older_one(self):
        # A re-run keeps the run id and raises run_attempt; the record read
        # is the newest attempt, so a failed or pending newer attempt refuses.
        self.assertIn("concluded failure",
                      self.refused(AdmissionApi(runs=[run(run_attempt=2, conclusion="failure")])))
        self.assertIn("is in_progress",
                      self.refused(AdmissionApi(runs=[run(run_attempt=2, status="in_progress", conclusion=None)]),
                                   pending=True))
        self.assertEqual(self.admitted(AdmissionApi(runs=[run(run_attempt=2)])), 77)

    def test_a_closed_draft_or_unknown_pull_request_refuses(self):
        self.assertIn("is closed", self.refused(AdmissionApi(pull=pull_record(state="closed"))))
        self.assertIn("is a draft", self.refused(AdmissionApi(pull=pull_record(draft=True))))
        self.assertIn("is a draft", self.refused(AdmissionApi(pull=pull_record(draft=None))))
        self.assertIn("no record for pull request", self.refused(AdmissionApi(pull={"message": "Not Found"})))

    def test_an_unreadable_record_refuses(self):
        self.assertIn("no record for run", self.refused(_MissingRun()))
        self.assertIn("has no timestamp", self.refused(AdmissionApi(created=None)))
        self.assertIn("unreadable timestamp", self.refused(AdmissionApi(created="yesterday")))
        bad = AdmissionApi(timeline=[{"event": "base_ref_changed", "created_at": None}])
        self.assertIn("has no timestamp", self.refused(bad))

    def test_main_refuses_any_api_error_and_never_waits(self):
        def run_main(api, *extra):
            out = io.StringIO()
            with mock.patch.object(admission.reuse, "Api", return_value=api), \
                    mock.patch.object(admission.reuse.time, "sleep") as sleep, redirect_stdout(out):
                code = admission.admit_main(["--repo", "o/r", "--head", HEAD, "--pull", "95", "--base", "main",
                                             "--workflow", WORKFLOW, "--require", "codeflow gates",
                                             "--require", "windows", *extra])
            self.assertEqual(sleep.call_count, 0)
            return code, out.getvalue()
        self.assertEqual(run_main(AdmissionApi())[0], 0)
        code, text = run_main(AdmissionApi(runs=[run(status="queued", conclusion=None)]))
        self.assertEqual(code, 1)
        self.assertIn("runs again when that run completes", text)
        for broken in ("pull", "run", "timeline"):
            with self.subTest(broken=broken):
                code, text = run_main(AdmissionApi(broken={broken}))
                self.assertEqual(code, 1)
                self.assertIn("refused: the decision failed (OSError", text)

        class RunsDown(AdmissionApi):
            def runs(self, head):
                raise OSError("network down")
        self.assertEqual(run_main(RunsDown())[0], 1)
        with self.assertRaises(SystemExit):
            with redirect_stdout(io.StringIO()), mock.patch("sys.stderr", io.StringIO()):
                admission.admit_main(["--repo", "o/r", "--head", HEAD, "--pull", "95", "--base", "main",
                                      "--workflow", WORKFLOW, "--require", "windows", "--wait", "60"])

    def test_no_named_verdict_refuses(self):
        out = io.StringIO()
        with redirect_stdout(out):
            code = admission.admit_main(["--repo", "o/r", "--head", HEAD, "--pull", "95", "--base", "main",
                                         "--workflow", WORKFLOW])
        self.assertEqual(code, 1)
        self.assertIn("no verdict job is named", out.getvalue())


class _MissingRun(AdmissionApi):
    def get(self, path):
        if path.startswith("actions/runs/"):
            return {"message": "Not Found"}
        return super().get(path)


class CompletionRerunTests(unittest.TestCase):
    """TSK-1239 AC-2 and AC-4: on codeflow-ci completion the commit
    standards job is re-run once, only for the run's own open, ready pull
    request at that head; nothing is written but the job re-run."""

    def rerun(self, api, run_id=77, head=HEAD):
        return admission.rerun(run_id, head, WORKFLOW, POLICY, STANDARDS, api)

    def test_a_completed_run_reruns_the_newest_policy_job(self):
        api = AdmissionApi(policy_runs=[policy_run(id=499, run_number=8), policy_run()],
                           policy_jobs={500: [{"id": 9001, "name": STANDARDS}], 499: [{"id": 8001, "name": STANDARDS}]})
        self.assertIn("re-ran", self.rerun(api))
        self.assertEqual(api.posts, ["actions/jobs/9001/rerun"])

    def test_a_failed_or_cancelled_run_still_reruns_so_the_check_refuses(self):
        for conclusion in ("failure", "cancelled", "skipped"):
            with self.subTest(conclusion=conclusion):
                api = AdmissionApi(runs=[run(conclusion=conclusion)], policy_runs=[policy_run(conclusion="success")])
                self.rerun(api)
                self.assertEqual(api.posts, ["actions/jobs/9001/rerun"])

    def test_a_policy_run_not_yet_completed_is_left_to_judge_itself(self):
        for status in ("queued", "in_progress", "pending"):
            with self.subTest(status=status):
                api = AdmissionApi(policy_runs=[policy_run(), policy_run(id=501, run_number=10, status=status)])
                self.assertIn("judges completed run 77 itself", self.rerun(api))
                self.assertEqual(api.posts, [])

    def test_nothing_is_rerun_for_a_moved_draft_closed_or_unmatched_pull_request(self):
        cases = {
            "moved": AdmissionApi(pull=pull_record(head={"sha": MAIN})),
            "draft": AdmissionApi(pull=pull_record(draft=True)),
            "closed": AdmissionApi(pull=pull_record(state="closed")),
            "fork": AdmissionApi(runs=[run(pull_requests=[])]),
            "two": AdmissionApi(runs=[run(pull_requests=[{"number": 95}, {"number": 96}])]),
            "no policy run": AdmissionApi(policy_runs=[]),
            "other pull request": AdmissionApi(policy_runs=[policy_run(pull_requests=[{"number": 96}])]),
            "other head": AdmissionApi(policy_runs=[policy_run(head_sha=MAIN)]),
            "other workflow": AdmissionApi(policy_runs=[policy_run(path=".github/workflows/other.yml")]),
            "other event": AdmissionApi(policy_runs=[policy_run(event="pull_request")]),
        }
        for name, api in cases.items():
            with self.subTest(case=name):
                self.assertIn("nothing re-run", self.rerun(api))
                self.assertEqual(api.posts, [])

    def test_an_untrusted_completion_record_refuses(self):
        cases = {
            "bad head": (AdmissionApi(), 77, "not-a-sha"),
            "head mismatch": (AdmissionApi(), 77, MAIN),
            "push run": (AdmissionApi(runs=[run(event="push")]), 77, HEAD),
            "other workflow": (AdmissionApi(runs=[run(path=".github/workflows/x.yml")]), 77, HEAD),
            "still running": (AdmissionApi(runs=[run(status="in_progress", conclusion=None)]), 77, HEAD),
            "unknown run": (AdmissionApi(), 1234, HEAD),
            "no job": (AdmissionApi(policy_jobs={500: [{"id": 9002, "name": "trusted: release impact"}]}), 77, HEAD),
            "two jobs": (AdmissionApi(policy_jobs={500: [{"id": 1, "name": STANDARDS}, {"id": 2, "name": STANDARDS}]}),
                         77, HEAD),
        }
        for name, (api, run_id, head) in cases.items():
            with self.subTest(case=name):
                with self.assertRaises(admission.Skip):
                    self.rerun(api, run_id, head)
                self.assertEqual(api.posts, [])

    def test_main_reruns_nothing_on_any_api_error(self):
        def run_main(api):
            out = io.StringIO()
            with mock.patch.object(admission, "Writer", return_value=api), redirect_stdout(out):
                code = admission.rerun_main(["--repo", "o/r", "--run", "77", "--head", HEAD, "--workflow", WORKFLOW,
                                             "--policy-workflow", POLICY, "--job", STANDARDS])
            return code, out.getvalue()
        self.assertEqual(run_main(AdmissionApi())[0], 0)
        for broken in ("run", "pull", "policy runs", "policy jobs", "post"):
            with self.subTest(broken=broken):
                api = AdmissionApi(broken={broken})
                code, text = run_main(api)
                self.assertEqual(code, 1, text)
                self.assertIn("refused", text)
                self.assertEqual(api.posts, [])
        code, text = run_main(AdmissionApi(policy_jobs={500: []}))
        self.assertEqual(code, 1)
        self.assertIn("lists 0 jobs", text)

    def test_the_writer_posts_only_to_the_api_host_and_refuses_a_bad_status(self):
        writer = admission.Writer("https://api.example", "o/r", "t0k")
        seen = []

        class Response:
            def __init__(self, status):
                self.status = status

            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

        def opener(status):
            def urlopen(request, timeout):
                seen.append((request.get_method(), request.full_url, request.get_header("Authorization")))
                return Response(status)
            return urlopen
        with mock.patch.object(admission.urllib.request, "urlopen", opener(201)):
            writer.post("actions/jobs/9001/rerun")
        self.assertEqual(seen, [("POST", "https://api.example/repos/o/r/actions/jobs/9001/rerun", "Bearer t0k")])
        with mock.patch.object(admission.urllib.request, "urlopen", opener(202)), self.assertRaises(admission.Skip):
            writer.post("actions/jobs/9001/rerun")

    def test_the_command_line_names_its_two_commands(self):
        err = io.StringIO()
        with mock.patch.object(admission.sys, "argv", ["policy-admission.py"]), mock.patch("sys.stderr", err):
            self.assertEqual(admission.main(), 2)
        self.assertIn("policy-admission.py admit", err.getvalue())


class WindowsTierTests(unittest.TestCase):
    """A reduced-tier result never counts as full-tier evidence, so a line
    push from such a run runs every job and the partitions run there."""

    REDUCED = [item for item in GREEN_JOBS if not item["name"].startswith("windows tests (")
               and item["name"] != "windows tier journeys"] + [
        job("windows tests (${{ matrix.partition }} of 12)", "skipped"),
        job("windows test build", "skipped"), job("windows journeys", "skipped"),
        job("windows tier journeys")]

    def reason(self, jobs, ref=LINE_REF):
        api = FakeApi(base=LINE if ref == LINE_REF else "main", jobs={77: jobs})
        with self.assertRaises(reuse.Skip) as caught:
            reuse.decide("push", PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, FakeGit(), api, APT_KEY, ref, IMAGES,
                         WORKFLOW_TEXT, run_id=PUSH_RUN)
        return str(caught.exception)

    def test_a_reduced_tier_run_is_not_reused_by_a_line_push(self):
        self.assertIn("reduced Windows tier", self.reason(self.REDUCED))

    def test_the_named_reason_says_the_partitions_run_on_this_push(self):
        self.assertIn("partitions run on this push", self.reason(self.REDUCED))

    def test_a_missing_or_repeated_partition_is_not_a_full_tier(self):
        full = [item for item in GREEN_JOBS if item["name"] != "windows tests (12 of 12)"]
        self.assertIn("did not run every Windows test partition", self.reason(full))
        self.assertIn("did not run every Windows test partition",
                      self.reason(GREEN_JOBS + [job("windows tests (1 of 12)")]))

    def test_mixed_partition_counts_are_not_a_full_tier(self):
        mixed = [item for item in GREEN_JOBS if item["name"] != "windows tests (12 of 12)"]
        self.assertIn("did not run every Windows test partition",
                      self.reason(mixed + [job("windows tests (12 of 13)")]))

    def test_a_skipped_partition_job_is_refused_by_its_conclusion(self):
        skipped = [item for item in GREEN_JOBS if not item["name"].startswith("windows tests (")] + [
            job("windows tests (${{ matrix.partition }} of 12)", "skipped")]
        self.assertIn("concluded skipped", self.reason(skipped))

    def test_a_run_with_no_partition_job_at_all_is_not_a_full_tier(self):
        none = [item for item in GREEN_JOBS if not item["name"].startswith("windows tests (")]
        self.assertIn("did not run every Windows test partition", self.reason(none))

    def test_a_full_tier_run_still_reuses_on_a_line_and_on_main(self):
        for ref, base in ((LINE_REF, LINE), (MAIN_REF, "main")):
            with self.subTest(ref=ref):
                api = FakeApi(base=base)
                self.assertEqual(reuse.decide("push", PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, FakeGit(), api, APT_KEY,
                                              ref, IMAGES, WORKFLOW_TEXT, run_id=PUSH_RUN), 77)

    def test_a_tier_journey_job_that_ran_refuses_even_beside_every_partition(self):
        ran = [item for item in GREEN_JOBS if item["name"] != "windows tier journeys"] + [
            job("windows tier journeys")]
        self.assertIn("reduced Windows tier", self.reason(ran))


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

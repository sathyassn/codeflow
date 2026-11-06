#!/usr/bin/env python3
"""Tests for scripts/landing-coverage.py (TSK-369).

Each case builds a small Git repository with a test config and the CI
workflow, a run record for its candidate commit, and, for hosted credit,
saved GitHub API responses. A record that is missing, names another commit
or tree, lacks a coverage measure or floor, or leaves a target unproven is
refused; so is every hosted result that is failed, missing, stale, of
another tree or wrongly scoped.
"""

from __future__ import annotations

import importlib.util
import io
import json
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "landing-coverage.py"
spec = importlib.util.spec_from_file_location("landing_coverage", SCRIPT)
landing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(landing)

CONFIG = {"targets": [
    {"name": "rust-build", "modes": {"full": {"command": "true"}}},
    {"name": "rust-coverage", "modes": {"full": {"command": "true"}},
     "coverage": {"format": "llvm-cov-json", "path": "target/llvm-cov-summary.json",
                  "rules": [{"scope": "global", "minimum": 90}]}},
    {"name": "present-browser", "modes": {"full": {"command": "true"}}},
    {"name": "rust-workspace", "modes": {"essential": {"command": "true"}}},
]}
# The shape of this repository's workflow: two jobs each with a `present`
# part, and only the release one runs present-browser.
WORKFLOW = """name: codeflow CI
on: [push]
jobs:
  gates:
    name: codeflow gates (${{ matrix.part }})
    strategy:
      matrix:
        include:
          - part: present
            only: present-supply-chain,cf-present-qualification
  gates-release:
    name: codeflow gates (release ${{ matrix.part }})
    strategy:
      matrix:
        include:
          - part: prose
            only: prose-exhaustive
          - part: present
            only: present-browser,present-binary-delta
"""
REPO = "sathyassn/codeflow"
JOB = "codeflow gates (release present)"


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True).stdout.strip()


class Fixture:
    def __init__(self, directory: Path):
        self.repo = directory / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.email", "test@example.invalid")
        git(self.repo, "config", "user.name", "test")
        git(self.repo, "config", "commit.gpgsign", "false")
        (self.repo / ".codeflow").mkdir()
        (self.repo / ".github/workflows").mkdir(parents=True)
        self.earlier = self.commit("one", CONFIG, WORKFLOW)
        self.candidate = self.commit("two", CONFIG, WORKFLOW)
        self.tree = git(self.repo, "rev-parse", f"{self.candidate}^{{tree}}")
        # A commit beside the candidate, with another tree.
        git(self.repo, "checkout", "-q", "-b", "side", self.earlier)
        self.other = self.commit("three", CONFIG, WORKFLOW)
        # The candidate's tree again, in a merge-like commit of its own.
        self.same_tree = git(self.repo, "commit-tree", self.tree, "-p", self.other, "-m", "same tree")
        self.evidence = directory / "hosted"
        self.evidence.mkdir()
        (directory / "run-1").mkdir()
        self.record_path = directory / "run-1" / "run.json"
        self.use(self.candidate)

    def use(self, commit: str) -> None:
        """Check out `commit`, as the check runs from the candidate's checkout."""
        git(self.repo, "checkout", "-q", "--detach", commit)
        self.snapshot = landing.tracked_digest(self.repo, commit)
        self.config_digest = landing.sha256(
            subprocess.run(["git", "-C", str(self.repo), "show", f"{commit}:.codeflow/test-config.json"],
                           check=True, capture_output=True).stdout)

    def commit(self, text: str, config: dict, workflow: str) -> str:
        (self.repo / "file").write_text(text)
        (self.repo / ".codeflow/test-config.json").write_text(json.dumps(config))
        (self.repo / ".github/workflows/codeflow-ci.yml").write_text(workflow)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", text)
        return git(self.repo, "rev-parse", "HEAD")

    def record(self, **changes) -> dict:
        def count(count, covered):
            return {"count": count, "covered": covered, "percent": covered * 100 / count}
        record = {
            "schema_version": 2, "run_id": "run-1", "revision": self.candidate,
            "git_tree": self.tree, "tree": self.snapshot, "config_digest": self.config_digest,
            "mode": "full", "passed": True,
            "complete": True, "clean": True, "generation_unchanged": True, "ci_skipped": [],
            "results": [{"name": n, "status": "passed"}
                        for n in ("rust-build", "rust-coverage", "present-browser")],
            "coverage": [{"target": "rust-coverage", "floor": 90, "data_missing": False,
                          "thresholds_failed": 0,
                          "totals": {"lines": count(1000, 927), "functions": count(100, 90),
                                     "regions": count(2000, 1800)}}],
        }
        record.update(changes)
        return record

    def hosted_record(self) -> dict:
        record = self.record(complete=False)
        record["results"] = [r for r in record["results"] if r["name"] != "present-browser"]
        return record

    def save_hosted(self, run_id: int, head: str, jobs: list[dict], total: int | None = None, **run) -> None:
        body = {"id": run_id, "path": landing.WORKFLOW, "status": "completed",
                "conclusion": "success", "head_sha": head, "event": "workflow_dispatch",
                "repository": {"full_name": REPO}, "head_repository": {"full_name": REPO}}
        body.update(run)
        (self.evidence / f"run-{run_id}.json").write_text(json.dumps(body))
        (self.evidence / f"jobs-{run_id}.json").write_text(json.dumps(
            {"total_count": len(jobs) if total is None else total, "jobs": jobs}))

    def run(self, record: dict | None, *extra: str, candidate: str | None = None) -> tuple[int, str, str]:
        if record is not None:
            self.record_path.write_text(json.dumps(record))
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = landing.main(["--candidate", candidate or self.candidate, "--run",
                                 str(self.record_path), "--repo", str(self.repo),
                                 "--hosted-evidence", str(self.evidence), *extra])
        return code, out.getvalue(), err.getvalue()


def job(name: str = JOB, status: str = "completed", conclusion: str = "success", run_id: int = 7) -> dict:
    return {"id": 70, "run_id": run_id, "name": name, "status": status, "conclusion": conclusion}


class LocalRecord(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.f = Fixture(Path(self.directory.name))

    def tearDown(self):
        self.directory.cleanup()

    def refused(self, record, expected: str, *extra: str, candidate: str | None = None):
        code, out, err = self.f.run(record, *extra, candidate=candidate)
        self.assertEqual(code, 1, out)
        self.assertEqual(out, "", "a refused record printed citable lines")
        self.assertIn(expected, err)

    def test_a_complete_passed_record_for_the_candidate_is_cited(self):
        code, out, err = self.f.run(self.f.record())
        self.assertEqual(code, 0, err)
        self.assertIn(f"revision {self.f.candidate}, git tree {self.f.tree}, "
                      f"tracked digest {self.f.snapshot}", out)
        self.assertIn("lines 92.70% (927/1000), functions 90.00% (90/100), "
                      "regions 90.00% (1800/2000); line floor 90%", out)
        self.assertIn("complete local full gate, passed", out)

    def test_a_missing_or_unreadable_record_is_refused(self):
        self.refused(None, "unreadable")
        self.f.record_path.write_text("{not json")
        self.refused(None, "unreadable")

    def test_a_record_for_another_commit_or_tree_is_refused(self):
        self.refused(self.f.record(revision=self.f.earlier), "is not the candidate")
        self.refused(self.f.record(git_tree="0" * 40), "is not the candidate's tree")
        # The record of the candidate cited for another commit.
        self.refused(self.f.record(), "is not the candidate", candidate=self.f.earlier)

    def test_a_run_that_did_not_pass_cleanly_is_refused(self):
        for field, value, reason in (("passed", False, "passed is False"),
                                     ("clean", False, "clean is False"),
                                     ("generation_unchanged", False, "generation_unchanged is False"),
                                     ("mode", "essential", "expected full"),
                                     ("schema_version", 1, "expected 2")):
            with self.subTest(field=field):
                self.refused(self.f.record(**{field: value}), reason)

    def test_coverage_without_every_measure_or_the_floor_is_refused(self):
        for measure in ("lines", "functions", "regions"):
            with self.subTest(measure=measure):
                record = self.f.record()
                del record["coverage"][0]["totals"][measure]
                self.refused(record, f"coverage {measure} total missing")
        self.refused(self.f.record(coverage=[]), "0 coverage entries")
        record = self.f.record()
        record["coverage"][0]["floor"] = 80
        self.refused(record, "is not the candidate's floor 90")
        record = self.f.record()
        record["coverage"][0]["totals"]["lines"] = {"count": 1000, "covered": 899, "percent": 89.9}
        self.refused(record, "under the floor")
        record = self.f.record()
        record["coverage"][0]["data_missing"] = True
        self.refused(record, "data_missing is True, expected false")

    def test_a_target_that_did_not_pass_or_run_is_refused(self):
        record = self.f.record()
        record["results"][2]["status"] = "failed"
        self.refused(record, "did not pass: present-browser")
        record = self.f.record(complete=False)
        record["results"] = record["results"][:2]
        self.refused(record, "not run locally: ['present-browser']")
        self.refused(self.f.record(ci_skipped=["present-browser"]), "skipped in CI")
        self.refused(self.f.record(complete=False), "complete is False with 0 hosted")


class ForgedRecord(unittest.TestCase):
    """Grok F1 on e3760a011: a hand-edited record with the right revision,
    tree and floor is refused unless its own numbers and digests agree."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.f = Fixture(Path(self.directory.name))

    def tearDown(self):
        self.directory.cleanup()

    def refused(self, record, expected: str):
        code, out, err = self.f.run(record)
        self.assertEqual(code, 1, out)
        self.assertEqual(out, "", "a forged record printed citable lines")
        self.assertIn(expected, err)

    def entry(self, record):
        return record["coverage"][0]

    def test_the_reviewed_forgery_is_refused(self):
        record = self.f.record(tree="anything")
        entry = self.entry(record)
        entry["totals"]["lines"] = {"count": 1000, "covered": 0, "percent": 90}
        del entry["data_missing"], entry["thresholds_failed"]
        code, out, err = self.f.run(record)
        self.assertEqual((code, out), (1, ""))
        for reason in ("lines percent 90 is not 0/1000", "data_missing is None",
                       "thresholds_failed is None", "tracked digest 'anything'"):
            self.assertIn(reason, err)

    def test_inconsistent_measures_are_refused(self):
        cases = {
            "covered above count": ({"count": 10, "covered": 11, "percent": 110.0}, "covers 11 of 10"),
            "percent not covered/count": ({"count": 10, "covered": 9, "percent": 95.0}, "is not 9/10"),
            "boolean percent": ({"count": 1, "covered": 1, "percent": True}, "lines total missing"),
            "boolean count": ({"count": True, "covered": 1, "percent": 100.0}, "lines total missing"),
            "negative count": ({"count": -1, "covered": -1, "percent": 100.0}, "lines total missing"),
            "empty not 100": ({"count": 0, "covered": 0, "percent": 0.0}, "is not 0/0"),
        }
        for name, (value, reason) in cases.items():
            with self.subTest(name):
                record = self.f.record()
                self.entry(record)["totals"]["lines"] = value
                self.refused(record, reason)

    def test_missing_data_and_threshold_fields_must_be_exact(self):
        for field, value, reason in (("data_missing", None, "data_missing is None"),
                                     ("data_missing", 0, "data_missing is 0"),
                                     ("thresholds_failed", False, "thresholds_failed is False"),
                                     ("thresholds_failed", 1, "thresholds_failed is 1"),
                                     ("thresholds_failed", "0", "thresholds_failed is '0'"),
                                     ("floor", True, "recorded floor True")):
            with self.subTest(field=field, value=value):
                record = self.f.record()
                self.entry(record)[field] = value
                self.refused(record, reason)

    def test_the_config_and_tracked_digests_must_be_the_candidate_checkout(self):
        self.refused(self.f.record(config_digest="0" * 64), "is not the candidate's test config")
        self.refused(self.f.record(tree="0" * 64), "is not this checkout's")
        # A tracked file changed in the checkout.
        (self.f.repo / "file").write_text("changed")
        self.refused(self.f.record(), "has tracked changes")
        git(self.f.repo, "checkout", "-q", "--", "file")
        # A checkout of another commit.
        git(self.f.repo, "checkout", "-q", "--detach", self.f.earlier)
        self.refused(self.f.record(), "this checkout is not the candidate")

    def test_a_record_outside_its_own_run_folder_is_refused(self):
        self.refused(self.f.record(run_id="run-2"), "is not in the folder its run_id")
        self.refused([], "is not a JSON object")

    def test_the_digest_follows_the_gates_snapshot_rule(self):
        # One file of mode 0o100644: the gate hashes the 4 little-endian mode
        # bytes and the file bytes, then the compact JSON map of names.
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            git(repo, "init", "-q")
            git(repo, "config", "user.email", "test@example.invalid")
            git(repo, "config", "user.name", "test")
            (repo / "a").write_bytes(b"x")
            (repo / "a").chmod(0o644)
            git(repo, "add", "a")
            git(repo, "-c", "commit.gpgsign=false", "commit", "-q", "-m", "a")
            head = git(repo, "rev-parse", "HEAD")
            entry = landing.sha256(bytes.fromhex("a4810000") + b"x")
            expected = landing.sha256(json.dumps({"a": entry}, separators=(",", ":")).encode())
            self.assertEqual(landing.tracked_digest(repo, head), expected)


class HostedCredit(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.f = Fixture(Path(self.directory.name))

    def tearDown(self):
        self.directory.cleanup()

    def refused(self, expected: str, record: dict | None = None, claim: str = "present-browser=7"):
        code, out, err = self.f.run(record or self.f.hosted_record(), "--hosted", claim)
        self.assertEqual(code, 1, out)
        self.assertEqual(out, "", "a refused credit printed citable lines")
        self.assertIn(expected, err)

    def test_a_completed_success_of_the_named_job_on_the_same_tree_is_credited(self):
        for head in (self.f.candidate, self.f.same_tree):
            with self.subTest(head=head):
                self.f.save_hosted(7, head, [job(), job("codeflow gates (release prose)")])
                code, out, err = self.f.run(self.f.hosted_record(), "--hosted", "present-browser=7")
                self.assertEqual(code, 0, err)
                self.assertIn(f"not complete locally; credited to hosted jobs: present-browser by "
                              f"hosted job '{JOB}', run 7, job 70, head {head}", out)
                self.assertNotIn("complete local full gate", out)

    def test_this_repositorys_workflow_scopes_the_release_present_job(self):
        text = (Path(__file__).resolve().parents[1] / landing.WORKFLOW).read_text()
        self.assertIn("present-browser", landing.job_scope(text, JOB, "present"))
        self.assertNotIn("present-browser", landing.job_scope(text, "codeflow gates (present)", "present"))

    def test_a_failed_foreign_fork_or_other_event_run_is_refused(self):
        for run, reason in (({"conclusion": "failure"}, "conclusion 'failure'"),
                            ({"conclusion": None}, "conclusion None"),
                            ({"repository": {"full_name": "attacker/not-codeflow"}}, "repository is 'attacker"),
                            ({"head_repository": {"full_name": "attacker/fork"}},
                             "head_repository is 'attacker/fork'"),
                            ({"head_repository": None}, "head_repository is None"),
                            ({"event": "pull_request"}, "started by 'pull_request'")):
            with self.subTest(run=run):
                self.f.save_hosted(7, self.f.candidate, [job()], **run)
                self.refused(reason)

    def test_a_partial_jobs_list_is_refused(self):
        self.f.save_hosted(7, self.f.candidate, [job()], total=250)
        self.refused("lists 1 jobs of total_count 250")
        (self.f.evidence / "jobs-7.json").write_text(json.dumps({"jobs": [job()]}))
        self.refused("total_count None")

    def test_the_reviewed_hosted_forgery_is_refused(self):
        self.f.save_hosted(7, self.f.candidate, [job()], total=250, conclusion="failure",
                           repository={"full_name": "attacker/not-codeflow"},
                           head_repository={"full_name": "attacker/fork"})
        self.refused("expected sathyassn/codeflow")

    def test_a_failed_or_unfinished_result_is_refused(self):
        for status, conclusion in (("completed", "failure"), ("completed", "cancelled"),
                                   ("completed", "skipped"), ("in_progress", None)):
            with self.subTest(conclusion=conclusion):
                self.f.save_hosted(7, self.f.candidate, [job(status=status, conclusion=conclusion)])
                self.refused("not a completed success")
        self.f.save_hosted(7, self.f.candidate, [job()], status="in_progress", conclusion=None)
        self.refused("not a completed success")

    def test_a_missing_result_is_refused(self):
        self.refused("hosted evidence for run 7 unreadable")
        self.f.save_hosted(7, self.f.candidate, [])
        self.refused(f"has no job '{JOB}'")
        self.refused("is not a number", claim="present-browser=latest")

    def test_a_stale_result_from_an_earlier_tree_is_refused(self):
        self.f.save_hosted(7, self.f.earlier, [job()])
        self.refused("stale: it ran on an earlier tree")

    def test_a_result_for_another_tree_is_refused(self):
        self.f.save_hosted(7, self.f.other, [job()])
        self.refused("it ran on another tree")
        self.f.save_hosted(7, "f" * 40, [job()])
        self.refused("is not readable here")

    def test_a_wrongly_scoped_result_is_refused(self):
        # Another job of the run.
        self.f.save_hosted(7, self.f.candidate, [job("codeflow gates (release prose)")])
        self.refused(f"has no job '{JOB}'")
        # A partial shard of the job.
        self.f.save_hosted(7, self.f.candidate, [job(f"{JOB[:-1]} 1 of 2)")])
        self.refused(f"has no job '{JOB}'")
        # The same name twice, a job of another run, another workflow.
        self.f.save_hosted(7, self.f.candidate, [job(), job()])
        self.refused("2 jobs named")
        self.f.save_hosted(7, self.f.candidate, [job(run_id=8)])
        self.refused("belongs to run 8")
        self.f.save_hosted(7, self.f.candidate, [job()], path=".github/workflows/other.yml")
        self.refused("expected .github/workflows/codeflow-ci.yml")
        # A target no hosted job may stand for.
        record = self.f.hosted_record()
        record["results"] = [r for r in record["results"] if r["name"] != "rust-build"]
        record["results"].append({"name": "present-browser", "status": "passed"})
        self.f.save_hosted(7, self.f.candidate, [job()])
        self.refused("no hosted job may stand for it", record, "rust-build=7")

    def test_a_job_whose_part_does_not_run_the_target_is_refused(self):
        # The release present part no longer runs present-browser; the other
        # job's present part is given it, which must not count.
        narrowed = WORKFLOW.replace("only: present-browser,present-binary-delta",
                                    "only: present-binary-delta").replace(
            "only: present-supply-chain,cf-present-qualification", "only: present-browser")
        git(self.f.repo, "checkout", "-q", "-b", "narrow", self.f.candidate)
        head = self.f.commit("two", CONFIG, narrowed)
        # Same content as the candidate except the workflow, so give the
        # check that commit as the candidate itself.
        self.f.save_hosted(7, head, [job()])
        self.f.use(head)
        record = self.f.hosted_record()
        record["revision"] = head
        record["git_tree"] = git(self.f.repo, "rev-parse", f"{head}^{{tree}}")
        code, out, err = self.f.run(record, "--hosted", "present-browser=7", candidate=head)
        self.assertEqual(code, 1, out)
        self.assertIn("runs only present-binary-delta, not present-browser", err)

    def test_a_credit_claim_must_match_the_targets_not_run(self):
        self.f.save_hosted(7, self.f.candidate, [job()])
        # Claimed while the target ran locally too.
        record = self.f.record(complete=False)
        self.refused("these must match", record)
        # A record that claims complete while a target is credited.
        record = self.f.hosted_record()
        record["complete"] = True
        self.refused("complete is True with 1 hosted", record)
        self.refused("expected one TARGET=RUN_ID", claim="present-browser")


if __name__ == "__main__":
    unittest.main()
